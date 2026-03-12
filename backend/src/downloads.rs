use crate::db;
use crate::filesystem_audit::{self, AuditedFile, FileSystemFacts};

use anyhow::Result;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct DownloadsSummary {
    pub total_items: usize,
    pub total_bytes: u64,
    pub linked_import_count: usize,
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
    pub classification: String,
    pub linked_library_paths_count: usize,
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
pub struct DownloadsLinkedPathsResponse {
    pub path: String,
    pub linked_paths: Vec<String>,
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
    library_paths_by_inode: HashMap<(u64, u64), Vec<String>>,
    library_paths_by_name: HashMap<String, Vec<String>>,
}

pub async fn summarize(pool: &SqlitePool, downloads_root: &Path) -> Result<DownloadsSummary> {
    let context = build_context(pool).await?;
    let items = filesystem_audit::collect_media_files(downloads_root, "downloads")?;
    Ok(summary_from_items(&context, &items))
}

pub async fn list_items(
    pool: &SqlitePool,
    downloads_root: &Path,
    options: DownloadsListOptions,
) -> Result<DownloadsItemsResponse> {
    let context = build_context(pool).await?;
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

    let mut entries = filesystem_audit::collect_media_files(downloads_root, "downloads")?
        .into_iter()
        .filter_map(|item| {
            let enriched = enrich_item(&context, item);
            if let Some(query) = query.as_deref() {
                let haystack = format!("{} {}", enriched.file_name, enriched.relative_path)
                    .to_ascii_lowercase();
                if !haystack.contains(query) {
                    return None;
                }
            }
            if let Some(classification) = classification_filter.as_deref() {
                if enriched.classification != classification {
                    return None;
                }
            }
            Some(enriched)
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
    let linked_paths = context
        .library_paths_by_inode
        .get(&(facts.device_id, facts.inode))
        .cloned()
        .unwrap_or_default();

    Ok(DownloadsLinkedPathsResponse {
        path: relative_path.to_string(),
        linked_paths,
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

async fn build_context(pool: &SqlitePool) -> Result<DownloadAuditContext> {
    let rows = db::list_library_inode_records(pool).await?;
    let mut library_paths_by_inode = HashMap::<(u64, u64), Vec<String>>::new();
    let mut library_paths_by_name = HashMap::<String, Vec<String>>::new();

    for row in rows {
        let key = (row.device_id as u64, row.inode as u64);
        library_paths_by_inode
            .entry(key)
            .or_default()
            .push(row.relative_path.clone());

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
                .push(row.relative_path);
        }
    }

    Ok(DownloadAuditContext {
        library_paths_by_inode,
        library_paths_by_name,
    })
}

fn enrich_item(context: &DownloadAuditContext, item: AuditedFile) -> DownloadItem {
    let linked_paths = context
        .library_paths_by_inode
        .get(&(item.filesystem.device_id, item.filesystem.inode))
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
    } else if !duplicate_paths.is_empty() {
        "possibly_duplicated"
    } else {
        "download_orphan"
    }
    .to_string();

    DownloadItem {
        file_name: item.file_name,
        relative_path: item.relative_path,
        path: item.path,
        size_bytes: item.filesystem.size_bytes,
        modified_at: item.filesystem.modified_at,
        path_root_kind: item.path_root_kind,
        filesystem: item.filesystem,
        classification,
        linked_library_paths_count: linked_paths.len(),
        duplicate_library_paths_count: duplicate_paths.len(),
    }
}

fn summary_from_items(context: &DownloadAuditContext, items: &[AuditedFile]) -> DownloadsSummary {
    let enriched = items
        .iter()
        .cloned()
        .map(|item| enrich_item(context, item))
        .collect::<Vec<_>>();
    summary_from_download_items(&enriched)
}

fn summary_from_download_items(items: &[DownloadItem]) -> DownloadsSummary {
    DownloadsSummary {
        total_items: items.len(),
        total_bytes: items.iter().map(|item| item.size_bytes).sum(),
        linked_import_count: items
            .iter()
            .filter(|item| item.classification == "linked_import")
            .count(),
        orphan_count: items
            .iter()
            .filter(|item| item.classification == "download_orphan")
            .count(),
        possibly_duplicated_count: items
            .iter()
            .filter(|item| item.classification == "possibly_duplicated")
            .count(),
        hard_linked_count: items.iter().filter(|item| item.filesystem.is_hard_linked).count(),
    }
}