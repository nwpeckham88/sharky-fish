use crate::internet_metadata::InternetMetadataMatch;
use crate::messages::ProcessingDecision;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const INTERNAL_SIDECAR_SUFFIX: &str = ".sharky.json";
const JELLYFIN_METADATA_SUFFIX: &str = ".nfo";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarDecision {
    pub arguments: Vec<String>,
    pub requires_two_pass: bool,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedItemSidecar {
    pub version: u32,
    pub relative_path: String,
    pub media_type: String,
    pub library_id: Option<String>,
    pub managed_status: String,
    pub review_note: Option<String>,
    pub review_updated_at: Option<u64>,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub first_seen_at: Option<u64>,
    pub last_updated_at: u64,
    pub selected_metadata: Option<InternetMetadataMatch>,
    pub last_decision: Option<SidecarDecision>,
}

pub fn sidecar_relative_path(relative_path: &str) -> Option<String> {
    internal_sidecar_relative_path(relative_path)
}

pub fn internal_sidecar_relative_path(relative_path: &str) -> Option<String> {
    let path = Path::new(relative_path);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path.file_stem()?.to_str()?;
    let file_name = format!("{}{}", stem, INTERNAL_SIDECAR_SUFFIX);
    let sidecar = if parent.as_os_str().is_empty() {
        PathBuf::from(file_name)
    } else {
        parent.join(file_name)
    };
    Some(sidecar.to_string_lossy().replace('\\', "/"))
}

pub fn sidecar_absolute_path(library_root: &Path, relative_path: &str) -> Option<PathBuf> {
    Some(library_root.join(internal_sidecar_relative_path(relative_path)?))
}

pub fn metadata_sidecar_relative_path(relative_path: &str) -> Option<String> {
    let path = Path::new(relative_path);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path.file_stem()?.to_str()?;
    let file_name = format!("{}{}", stem, JELLYFIN_METADATA_SUFFIX);
    let sidecar = if parent.as_os_str().is_empty() {
        PathBuf::from(file_name)
    } else {
        parent.join(file_name)
    };
    Some(sidecar.to_string_lossy().replace('\\', "/"))
}

pub fn metadata_sidecar_absolute_path(library_root: &Path, relative_path: &str) -> Option<PathBuf> {
    Some(library_root.join(metadata_sidecar_relative_path(relative_path)?))
}

pub async fn find_metadata_sidecar_relative_path(
    library_root: &Path,
    relative_path: &str,
) -> Result<Option<String>> {
    let Some(candidate) = metadata_sidecar_relative_path(relative_path) else {
        return Ok(None);
    };
    let Some(candidate_abs) = metadata_sidecar_absolute_path(library_root, relative_path) else {
        return Ok(None);
    };

    if tokio::fs::try_exists(candidate_abs).await? {
        return Ok(Some(candidate));
    }

    Ok(None)
}

pub async fn read_sidecar(
    library_root: &Path,
    relative_path: &str,
) -> Result<Option<ManagedItemSidecar>> {
    let Some(sidecar_path) = sidecar_absolute_path(library_root, relative_path) else {
        return Ok(None);
    };

    if !tokio::fs::try_exists(&sidecar_path).await? {
        return Ok(None);
    }

    let raw = tokio::fs::read_to_string(&sidecar_path).await?;
    let parsed = serde_json::from_str::<ManagedItemSidecar>(&raw)?;
    Ok(Some(parsed))
}

pub async fn write_sidecar(library_root: &Path, sidecar: &ManagedItemSidecar) -> Result<()> {
    let Some(sidecar_path) = sidecar_absolute_path(library_root, &sidecar.relative_path) else {
        anyhow::bail!("unable to build sidecar path");
    };

    if let Some(parent) = sidecar_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let raw = serde_json::to_vec_pretty(sidecar)?;
    tokio::fs::write(sidecar_path, raw).await?;
    Ok(())
}

pub fn sidecar_decision_from_processing(decision: &ProcessingDecision) -> SidecarDecision {
    SidecarDecision {
        arguments: decision.arguments.clone(),
        requires_two_pass: decision.requires_two_pass,
        rationale: decision.rationale.clone(),
    }
}

pub fn processing_decision_from_sidecar(sidecar: &SidecarDecision) -> ProcessingDecision {
    ProcessingDecision {
        job_id: 0,
        arguments: sidecar.arguments.clone(),
        requires_two_pass: sidecar.requires_two_pass,
        rationale: sidecar.rationale.clone(),
    }
}

pub async fn write_jellyfin_nfo(
    library_root: &Path,
    relative_path: &str,
    selected: &InternetMetadataMatch,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Option<String>> {
    let Some(sidecar_path) = metadata_sidecar_absolute_path(library_root, relative_path) else {
        return Ok(None);
    };

    if let Some(parent) = sidecar_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let xml = render_jellyfin_nfo(selected, season, episode);
    tokio::fs::write(&sidecar_path, xml).await?;
    Ok(metadata_sidecar_relative_path(relative_path))
}

fn render_jellyfin_nfo(
    selected: &InternetMetadataMatch,
    season: Option<u32>,
    episode: Option<u32>,
) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

    if let (Some(season), Some(episode)) = (season, episode) {
        xml.push_str("<episodedetails>\n");
        push_xml_tag(&mut xml, 1, "title", &format!("{} - S{:02}E{:02}", selected.title, season, episode));
        push_xml_tag(&mut xml, 1, "showtitle", &selected.title);
        push_xml_tag(&mut xml, 1, "season", &season.to_string());
        push_xml_tag(&mut xml, 1, "episode", &episode.to_string());
        push_xml_tag_if_some(&mut xml, 1, "year", selected.year.map(|value| value.to_string()).as_deref());
        push_unique_ids(&mut xml, selected);
        push_xml_tag_if_some(&mut xml, 1, "plot", selected.overview.as_deref());
        push_xml_tag_if_some(&mut xml, 1, "rating", selected.rating.map(|value| format!("{value:.1}")).as_deref());
        for genre in &selected.genres {
            push_xml_tag(&mut xml, 1, "genre", genre);
        }
        xml.push_str("</episodedetails>\n");
        return xml;
    }

    xml.push_str("<movie>\n");
    push_xml_tag(&mut xml, 1, "title", &selected.title);
    push_xml_tag(&mut xml, 1, "originaltitle", &selected.title);
    push_xml_tag_if_some(&mut xml, 1, "year", selected.year.map(|value| value.to_string()).as_deref());
    push_unique_ids(&mut xml, selected);
    push_xml_tag_if_some(&mut xml, 1, "plot", selected.overview.as_deref());
    push_xml_tag_if_some(&mut xml, 1, "rating", selected.rating.map(|value| format!("{value:.1}")).as_deref());
    for genre in &selected.genres {
        push_xml_tag(&mut xml, 1, "genre", genre);
    }
    xml.push_str("</movie>\n");
    xml
}

fn push_unique_ids(xml: &mut String, selected: &InternetMetadataMatch) {
    if let Some(imdb_id) = selected.imdb_id.as_deref().filter(|value| !value.trim().is_empty()) {
        push_xml_tag(xml, 1, "id", imdb_id);
        push_xml_tag_with_attrs(xml, 1, "uniqueid", &[('t', "type=\"imdb\" default=\"true\"")], imdb_id);
    }
    if let Some(tvdb_id) = selected.tvdb_id {
        push_xml_tag_with_attrs(
            xml,
            1,
            "uniqueid",
            &[('t', "type=\"tvdb\" default=\"false\"")],
            &tvdb_id.to_string(),
        );
    }
}

fn push_xml_tag(xml: &mut String, indent: usize, tag: &str, value: &str) {
    push_xml_tag_with_attrs(xml, indent, tag, &[], value);
}

fn push_xml_tag_if_some(xml: &mut String, indent: usize, tag: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        push_xml_tag(xml, indent, tag, value);
    }
}

fn push_xml_tag_with_attrs(
    xml: &mut String,
    indent: usize,
    tag: &str,
    attrs: &[(char, &str)],
    value: &str,
) {
    let padding = "    ".repeat(indent);
    xml.push_str(&padding);
    xml.push('<');
    xml.push_str(tag);
    for (_, attr) in attrs {
        xml.push(' ');
        xml.push_str(attr);
    }
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(tag);
    xml.push_str(">\n");
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
