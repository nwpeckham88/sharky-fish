use crate::config::AppConfig;
use crate::filesystem_audit::FileSystemFacts;
use crate::internet_metadata::InternetMetadataMatch;
use crate::messages::{
    MediaProbe, ProcessingDecision, ReviewExecutionMode, ReviewOrganizationProposal,
    ReviewProcessingProposal, ReviewProposal,
};
use crate::organizer;

pub fn build_review_proposal(
    config: &AppConfig,
    relative_path: &str,
    library_id: Option<&str>,
    selected_metadata: Option<&InternetMetadataMatch>,
    filesystem: FileSystemFacts,
    probe: &MediaProbe,
    decision: &ProcessingDecision,
) -> ReviewProposal {
    let scope = selected_metadata
        .map(|selected| match selected.media_kind.trim().to_ascii_lowercase().as_str() {
            "movie" => "movie_folder",
            _ => "file",
        })
        .unwrap_or("file")
        .to_string();

    let target_relative_path = selected_metadata.and_then(|selected| {
        organizer::preview_target_relative_path(config, relative_path, library_id, selected).ok()
    });
    let organize_needed = target_relative_path
        .as_deref()
        .map(|target| target != relative_path)
        .unwrap_or(false);

    let processing = if decision.arguments.is_empty() {
        None
    } else {
        Some(ReviewProcessingProposal {
            arguments: decision.arguments.clone(),
            requires_two_pass: decision.requires_two_pass,
            rationale: decision.rationale.clone(),
        })
    };

    let mut warnings = Vec::new();
    if filesystem.is_hard_linked && processing.is_some() {
        warnings.push(
            "Processing this item will create a new file and break the current hard-link relationship."
                .into(),
        );
    }
    if filesystem.is_hard_linked {
        warnings.push(
            "Organize-only changes keep the same inode and preserve the shared storage relationship."
                .into(),
        );
    }

    let mut allowed_modes = Vec::new();
    if organize_needed && processing.is_some() {
        allowed_modes.push(ReviewExecutionMode::FullPlan);
    }
    if organize_needed {
        allowed_modes.push(ReviewExecutionMode::OrganizeOnly);
    }
    if processing.is_some() {
        allowed_modes.push(ReviewExecutionMode::ProcessOnly);
    }
    if allowed_modes.is_empty() {
        allowed_modes.push(ReviewExecutionMode::FullPlan);
    }

    let recommendation_reason = re_source_reason(probe);
    let recommendation = if recommendation_reason.is_some() {
        "re_source"
    } else if organize_needed && processing.is_some() {
        "full_plan"
    } else if organize_needed {
        "organize"
    } else if processing.is_some() {
        "process"
    } else {
        "keep"
    }
    .to_string();

    if let Some(reason) = recommendation_reason.as_ref() {
        warnings.push(reason.clone());
    }

    ReviewProposal {
        relative_path: relative_path.to_string(),
        filesystem,
        organization: ReviewOrganizationProposal {
            current_relative_path: relative_path.to_string(),
            target_relative_path,
            organize_needed,
            scope,
        },
        processing,
        recommendation,
        recommendation_reason,
        warnings,
        allowed_modes,
    }
}

fn re_source_reason(probe: &MediaProbe) -> Option<String> {
    let video = probe.streams.iter().find(|stream| stream.codec_type == "video")?;
    let codec = video.codec_name.trim().to_ascii_lowercase();
    let width = video.width.unwrap_or(0);
    let height = video.height.unwrap_or(0);
    let bitrate_mbps = video.bit_rate.map(|value| value as f64 / 1_000_000.0)?;

    let is_uhd = width >= 3800 || height >= 2100;
    let is_avc = matches!(codec.as_str(), "h264" | "avc" | "avc1");

    if is_uhd && is_avc && bitrate_mbps <= 16.0 {
        return Some(format!(
            "Planner recommends re-source: this 4K AVC stream is only about {:.1} Mbps, so another lossy transcode is more likely to compound damage than improve efficiency.",
            bitrate_mbps
        ));
    }

    None
}