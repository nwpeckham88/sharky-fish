use crate::db;
use crate::filesystem_audit::{self, AuditedFile, FileSystemFacts};

use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct DownloadsSummary {
    pub total_items: usize,
    pub total_bytes: u64,
    pub linked_import_count: usize,
    pub checksum_duplicate_count: usize,
    pub orphan_count: usize,
    pub possibly_duplicated_count: usize,
    pub hard_linked_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadItem {
    pub file_name: String,
    pub relative_path: String,
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub path_root_kind: String,
    pub filesystem: FileSystemFacts,
    pub checksum_blake3: String,
    pub classification: String,
    pub linked_library_paths_count: usize,
    pub checksum_library_paths_count: usize,
    pub duplicate_library_paths_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadsItemsResponse {
    pub items: Vec<DownloadItem>,
    pub total_items: usize,
    pub limit: usize,
    pub offset: usize,
    pub summary: DownloadsSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadsLibraryMatch {
    pub path: String,
    pub library_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadsLinkedPathsResponse {
    pub path: String,
    pub linked_paths: Vec<DownloadsLibraryMatch>,
    pub checksum_paths: Vec<DownloadsLibraryMatch>,
    pub heuristic_paths: Vec<DownloadsLibraryMatch>,
    pub checksum_blake3: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteDownloadResponse {
    pub path: String,
    pub deleted: bool,
    pub linked_library_paths_count: usize,
    pub warning: Option<String>,
    pub frees_space: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadsListOptions {
    pub query: Option<String>,
    pub classification: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone)]
struct DownloadAuditContext {
    library_paths_by_inode: HashMap<(u64, u64), Vec<DownloadsLibraryMatch>>,
    library_paths_by_checksum: HashMap<String, Vec<DownloadsLibraryMatch>>,
    library_paths_by_name: HashMap<String, Vec<DownloadsLibraryMatch>>,
}

#[derive(Debug, Clone)]
struct DownloadChecksumResolver {
    pool: SqlitePool,
    cache_by_path: Arc<HashMap<String, db::DownloadChecksumCacheRow>>,
}

pub async fn summarize(pool: &SqlitePool, downloads_root: &Path) -> Result<DownloadsSummary> {
    let context = build_context(pool).await?;
    let checksum_resolver = build_checksum_resolver(pool).await?;
    let items = filesystem_audit::collect_media_files(downloads_root, "downloads")?;
    let enriched = enrich_items(context, checksum_resolver, items).await?;
    Ok(summary_from_download_items(&enriched))
}

pub async fn list_items(
    pool: &SqlitePool,
    downloads_root: &Path,
    options: DownloadsListOptions,
) -> Result<DownloadsItemsResponse> {
    let context = build_context(pool).await?;
    let checksum_resolver = build_checksum_resolver(pool).await?;
    let query = options
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let classification_filter = options
        .classification
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"))
        .map(str::to_ascii_lowercase);

    let mut entries = enrich_items(
        context,
        checksum_resolver,
        filesystem_audit::collect_media_files(downloads_root, "downloads")?,
    )
    .await?
    .into_iter()
    .filter(|enriched| {
        if let Some(query) = query.as_deref() {
            let haystack = format!(
                "{} {} {}",
                enriched.file_name, enriched.relative_path, enriched.checksum_blake3
            )
            .to_ascii_lowercase();
            if !haystack.contains(query) {
                return false;
            }
        }
        if let Some(classification) = classification_filter.as_deref() {
            return enriched.classification == classification;
        }
        true
    })
    .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });

    let summary = summary_from_download_items(&entries);
    let total_items = entries.len();
    let start = options.offset.min(total_items);
    let end = (start + options.limit).min(total_items);

    Ok(DownloadsItemsResponse {
        items: entries[start..end].to_vec(),
        total_items,
        limit: options.limit,
        offset: options.offset,
        summary,
    })
}

pub async fn linked_paths(
    pool: &SqlitePool,
    downloads_root: &Path,
    relative_path: &str,
) -> Result<DownloadsLinkedPathsResponse> {
    let context = build_context(pool).await?;
    let absolute_path = filesystem_audit::joined_path(downloads_root, relative_path)?;
    let facts = filesystem_audit::stat_path(&absolute_path).await?;
    let checksum_blake3 = checksum_for_path(pool, &absolute_path, &facts).await?;
    let linked_paths = context
        .library_paths_by_inode
        .get(&(facts.device_id, facts.inode))
        .cloned()
        .unwrap_or_default();
    let checksum_paths = context
        .library_paths_by_checksum
        .get(&checksum_blake3)
        .cloned()
        .unwrap_or_default();
    let normalized = filesystem_audit::normalized_name(
        Path::new(relative_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(relative_path),
    );
    let heuristic_paths = context
        .library_paths_by_name
        .get(&normalized)
        .cloned()
        .unwrap_or_default();

    Ok(DownloadsLinkedPathsResponse {
        path: relative_path.to_string(),
        linked_paths,
        checksum_paths,
        heuristic_paths,
        checksum_blake3,
    })
}

pub async fn delete_item(
    pool: &SqlitePool,
    downloads_root: &Path,
    relative_path: &str,
) -> Result<DeleteDownloadResponse> {
    let context = build_context(pool).await?;
    let absolute_path = filesystem_audit::joined_path(downloads_root, relative_path)?;
    let facts = filesystem_audit::stat_path(&absolute_path).await?;
    let linked_library_paths_count = context
        .library_paths_by_inode
        .get(&(facts.device_id, facts.inode))
        .map(|paths| paths.len())
        .unwrap_or(0);
    let frees_space = facts.link_count <= 1 && linked_library_paths_count == 0;

    tokio::fs::remove_file(&absolute_path).await?;
    db::delete_download_checksum_cache(pool, &absolute_path.display().to_string()).await?;

    Ok(DeleteDownloadResponse {
        path: relative_path.to_string(),
        deleted: true,
        linked_library_paths_count,
        warning: if linked_library_paths_count > 0 || facts.link_count > 1 {
            Some(
                "Deleting this download removes one directory entry only; it does not free space while other hard links still exist."
                    .into(),
            )
        } else {
            None
        },
        frees_space,
    })
}

async fn build_checksum_resolver(pool: &SqlitePool) -> Result<DownloadChecksumResolver> {
    let cache_by_path = db::list_download_checksum_cache(pool)
        .await?
        .into_iter()
        .map(|row| (row.file_path.clone(), row))
        .collect::<HashMap<_, _>>();

    Ok(DownloadChecksumResolver {
        pool: pool.clone(),
        cache_by_path: Arc::new(cache_by_path),
    })
}

async fn checksum_for_path(
    pool: &SqlitePool,
    absolute_path: &Path,
    facts: &FileSystemFacts,
) -> Result<String> {
    let file_path = absolute_path.display().to_string();
    if let Some(cached) = db::fetch_download_checksum_cache(pool, &file_path).await?
        && checksum_cache_matches(&cached, facts)
    {
        return Ok(cached.checksum_blake3);
    }

    let checksum = filesystem_audit::blake3_checksum(absolute_path).await?;
    db::upsert_download_checksum_cache(pool, &file_path, facts, &checksum).await?;
    Ok(checksum)
}

fn checksum_cache_matches(cached: &db::DownloadChecksumCacheRow, facts: &FileSystemFacts) -> bool {
    cached.size_bytes as u64 == facts.size_bytes
        && cached.modified_at as u64 == facts.modified_at
        && cached.device_id as u64 == facts.device_id
        && cached.inode as u64 == facts.inode
        && cached.link_count as u64 == facts.link_count
}

async fn build_context(pool: &SqlitePool) -> Result<DownloadAuditContext> {
    let rows = db::list_library_inode_records(pool).await?;
    let mut library_paths_by_inode = HashMap::<(u64, u64), Vec<DownloadsLibraryMatch>>::new();
    let mut library_paths_by_checksum = HashMap::<String, Vec<DownloadsLibraryMatch>>::new();
    let mut library_paths_by_name = HashMap::<String, Vec<DownloadsLibraryMatch>>::new();

    for row in rows {
        let match_record = DownloadsLibraryMatch {
            path: row.relative_path.clone(),
            library_id: row.library_id.clone(),
        };
        let key = (row.device_id as u64, row.inode as u64);
        library_paths_by_inode
            .entry(key)
            .or_default()
            .push(match_record.clone());

        if let Some(checksum_blake3) = row
            .checksum_blake3
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            library_paths_by_checksum
                .entry(checksum_blake3.to_string())
                .or_default()
                .push(match_record.clone());
        }

        let normalized = filesystem_audit::normalized_name(
            Path::new(&row.file_name)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(&row.file_name),
        );
        if !normalized.is_empty() {
            library_paths_by_name
                .entry(normalized)
                .or_default()
                .push(match_record);
        }
    }

    Ok(DownloadAuditContext {
        library_paths_by_inode,
        library_paths_by_checksum,
        library_paths_by_name,
    })
}

async fn enrich_items(
    context: DownloadAuditContext,
    checksum_resolver: DownloadChecksumResolver,
    items: Vec<AuditedFile>,
) -> Result<Vec<DownloadItem>> {
    let context = Arc::new(context);
    let checksum_resolver = Arc::new(checksum_resolver);

    let results = stream::iter(items.into_iter().map(|item| {
        let context = context.clone();
        let checksum_resolver = checksum_resolver.clone();
        async move { enrich_item(&context, &checksum_resolver, item).await }
    }))
    .buffer_unordered(6)
    .collect::<Vec<_>>()
    .await;

    results.into_iter().collect()
}

async fn enrich_item(
    context: &DownloadAuditContext,
    checksum_resolver: &DownloadChecksumResolver,
    item: AuditedFile,
) -> Result<DownloadItem> {
    let checksum_blake3 = if let Some(cached) = checksum_resolver.cache_by_path.get(&item.path) {
        if checksum_cache_matches(cached, &item.filesystem) {
            cached.checksum_blake3.clone()
        } else {
            refresh_cached_checksum(checksum_resolver, &item).await?
        }
    } else {
        refresh_cached_checksum(checksum_resolver, &item).await?
    };
    let linked_paths = context
        .library_paths_by_inode
        .get(&(item.filesystem.device_id, item.filesystem.inode))
        .cloned()
        .unwrap_or_default();
    let checksum_paths = context
        .library_paths_by_checksum
        .get(&checksum_blake3)
        .cloned()
        .unwrap_or_default();
    let normalized = filesystem_audit::normalized_name(
        Path::new(&item.file_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&item.file_name),
    );
    let duplicate_paths = context
        .library_paths_by_name
        .get(&normalized)
        .cloned()
        .unwrap_or_default();

    let classification = if !linked_paths.is_empty() {
        "linked_import"
    } else if !checksum_paths.is_empty() {
        "checksum_duplicate"
    } else if !duplicate_paths.is_empty() {
        "possibly_duplicated"
    } else {
        "download_orphan"
    }
    .to_string();

    Ok(DownloadItem {
        file_name: item.file_name,
        relative_path: item.relative_path,
        path: item.path,
        size_bytes: item.filesystem.size_bytes,
        modified_at: item.filesystem.modified_at,
        path_root_kind: item.path_root_kind,
        filesystem: item.filesystem,
        checksum_blake3,
        classification,
        linked_library_paths_count: linked_paths.len(),
        checksum_library_paths_count: checksum_paths.len(),
        duplicate_library_paths_count: duplicate_paths.len(),
    })
}

async fn refresh_cached_checksum(
    checksum_resolver: &DownloadChecksumResolver,
    item: &AuditedFile,
) -> Result<String> {
    let checksum = filesystem_audit::blake3_checksum(Path::new(&item.path)).await?;
    db::upsert_download_checksum_cache(
        &checksum_resolver.pool,
        &item.path,
        &item.filesystem,
        &checksum,
    )
    .await?;
    Ok(checksum)
}

fn summary_from_download_items(items: &[DownloadItem]) -> DownloadsSummary {
    DownloadsSummary {
        total_items: items.len(),
        total_bytes: items.iter().map(|item| item.size_bytes).sum(),
        linked_import_count: items
            .iter()
            .filter(|item| item.classification == "linked_import")
            .count(),
        checksum_duplicate_count: items
            .iter()
            .filter(|item| item.classification == "checksum_duplicate")
            .count(),
        orphan_count: items
            .iter()
            .filter(|item| item.classification == "download_orphan")
            .count(),
        possibly_duplicated_count: items
            .iter()
            .filter(|item| item.classification == "possibly_duplicated")
            .count(),
        hard_linked_count: items
            .iter()
            .filter(|item| item.filesystem.is_hard_linked)
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn unique_temp_dir() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "sharky-fish-downloads-bench-{}-{}",
            std::process::id(),
            stamp
        ))
    }

    #[tokio::test]
    async fn benchmark_downloads_checksum_cache_warm_path() -> anyhow::Result<()> {
        let root = unique_temp_dir();
        let downloads_root = root.join("downloads");
        tokio::fs::create_dir_all(&downloads_root).await?;

        // Use a handful of multi-MB files so checksum costs are measurable.
        for idx in 0..12u8 {
            let path = downloads_root.join(format!("sample-{idx:02}.mkv"));
            let data = vec![idx; 2 * 1024 * 1024];
            tokio::fs::write(path, data).await?;
        }

        let db_path = root.join("bench.sqlite");
        let pool = db::init_pool(&db_path).await?;
        let options = DownloadsListOptions {
            query: None,
            classification: None,
            limit: 500,
            offset: 0,
        };

        let first_start = Instant::now();
        let first = list_items(&pool, &downloads_root, options.clone()).await?;
        let first_elapsed = first_start.elapsed();

        let second_start = Instant::now();
        let second = list_items(&pool, &downloads_root, options).await?;
        let second_elapsed = second_start.elapsed();

        let cache_rows = db::list_download_checksum_cache(&pool).await?;

        println!(
            "downloads checksum cache benchmark: first={:?}, second={:?}, items={}, cache_rows={}",
            first_elapsed,
            second_elapsed,
            first.total_items,
            cache_rows.len()
        );

        assert_eq!(first.total_items, second.total_items);
        assert_eq!(cache_rows.len(), first.total_items);

        let _ = tokio::fs::remove_dir_all(&root).await;
        Ok(())
    }
}
