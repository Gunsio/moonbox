use std::{
    fs,
    path::{Path, PathBuf},
};

use image::{GenericImageView, imageops::FilterType};

use super::model::{TimelineAttachment, TimelineKind, WorkbenchData};

const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const PREVIEW_COLUMNS: u32 = 36;
const PREVIEW_ROWS: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewRgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewCell {
    pub top: PreviewRgb,
    pub bottom: Option<PreviewRgb>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePreviewStatus {
    Rendered,
    MissingPath,
    UnsupportedPath(String),
    TooLarge { bytes: u64, limit: u64 },
    DecodeError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineImagePreview {
    pub event_id: String,
    pub label: String,
    pub path: Option<String>,
    pub dimensions: Option<(u32, u32)>,
    pub status: ImagePreviewStatus,
    pub rows: Vec<Vec<PreviewCell>>,
}

impl TimelineImagePreview {
    pub fn is_rendered(&self) -> bool {
        self.status == ImagePreviewStatus::Rendered
    }
}

pub fn build_timeline_image_previews(
    data: &WorkbenchData,
    selected_event: usize,
) -> Vec<TimelineImagePreview> {
    selected_timeline_group_indices(data, selected_event)
        .into_iter()
        .filter_map(|index| data.timeline.get(index))
        .flat_map(|event| {
            event
                .metadata
                .attachments
                .iter()
                .filter(|attachment| timeline_attachment_is_image(attachment))
                .map(move |attachment| preview_attachment(&event.id, attachment))
        })
        .collect()
}

fn selected_timeline_group_indices(data: &WorkbenchData, selected_event: usize) -> Vec<usize> {
    let Some(event) = data.timeline.get(selected_event) else {
        return Vec::new();
    };
    if event.kind != TimelineKind::Assistant {
        return vec![selected_event];
    }

    let mut start = selected_event;
    while start > 0
        && data
            .timeline
            .get(start - 1)
            .is_some_and(|event| event.kind == TimelineKind::Assistant)
    {
        start -= 1;
    }

    let mut end = selected_event;
    while data
        .timeline
        .get(end + 1)
        .is_some_and(|event| event.kind == TimelineKind::Assistant)
    {
        end += 1;
    }

    (start..=end).collect()
}

fn preview_attachment(event_id: &str, attachment: &TimelineAttachment) -> TimelineImagePreview {
    let label = timeline_attachment_label(attachment);
    let Some(path) = attachment
        .path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    else {
        return TimelineImagePreview {
            event_id: event_id.into(),
            label,
            path: None,
            dimensions: None,
            status: ImagePreviewStatus::MissingPath,
            rows: Vec::new(),
        };
    };

    let path_buf = PathBuf::from(path);
    let display_path = path_buf.display().to_string();
    let unsupported = unsupported_image_path_reason(&path_buf);
    if let Some(reason) = unsupported {
        return TimelineImagePreview {
            event_id: event_id.into(),
            label,
            path: Some(display_path),
            dimensions: None,
            status: ImagePreviewStatus::UnsupportedPath(reason),
            rows: Vec::new(),
        };
    }

    let Ok(metadata) = fs::metadata(&path_buf) else {
        return TimelineImagePreview {
            event_id: event_id.into(),
            label,
            path: Some(display_path),
            dimensions: None,
            status: ImagePreviewStatus::UnsupportedPath("file is not readable".into()),
            rows: Vec::new(),
        };
    };
    if metadata.len() > MAX_IMAGE_BYTES {
        return TimelineImagePreview {
            event_id: event_id.into(),
            label,
            path: Some(display_path),
            dimensions: None,
            status: ImagePreviewStatus::TooLarge {
                bytes: metadata.len(),
                limit: MAX_IMAGE_BYTES,
            },
            rows: Vec::new(),
        };
    }

    let image = match image::open(&path_buf) {
        Ok(image) => image,
        Err(error) => {
            return TimelineImagePreview {
                event_id: event_id.into(),
                label,
                path: Some(display_path),
                dimensions: None,
                status: ImagePreviewStatus::DecodeError(error.to_string()),
                rows: Vec::new(),
            };
        }
    };
    let (width, height) = image.dimensions();
    let thumbnail = image.thumbnail(PREVIEW_COLUMNS, PREVIEW_ROWS * 2);
    let thumbnail = image::imageops::resize(
        &thumbnail.to_rgba8(),
        thumbnail.width().max(1),
        thumbnail.height().max(1),
        FilterType::Triangle,
    );
    let rows = thumbnail_to_half_blocks(&thumbnail);

    TimelineImagePreview {
        event_id: event_id.into(),
        label,
        path: Some(display_path),
        dimensions: Some((width, height)),
        status: ImagePreviewStatus::Rendered,
        rows,
    }
}

fn unsupported_image_path_reason(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    if text.contains("://") {
        return Some("remote URL is not a local preview artifact".into());
    }
    if text.contains("<path:redacted>") {
        return Some("path was redacted".into());
    }
    if !path.is_absolute() {
        return Some("path is not absolute".into());
    }
    if !path.exists() {
        return Some("file does not exist".into());
    }
    if !path.is_file() {
        return Some("path is not a file".into());
    }
    None
}

fn thumbnail_to_half_blocks(image: &image::RgbaImage) -> Vec<Vec<PreviewCell>> {
    let mut rows = Vec::new();
    let width = image.width();
    let height = image.height();
    let mut y = 0;
    while y < height {
        let mut row = Vec::new();
        for x in 0..width {
            let top = rgba_to_preview_rgb(image.get_pixel(x, y));
            let bottom = (y + 1 < height).then(|| rgba_to_preview_rgb(image.get_pixel(x, y + 1)));
            row.push(PreviewCell { top, bottom });
        }
        rows.push(row);
        y += 2;
    }
    rows
}

fn rgba_to_preview_rgb(pixel: &image::Rgba<u8>) -> PreviewRgb {
    let [red, green, blue, alpha] = pixel.0;
    if alpha == u8::MAX {
        return PreviewRgb { red, green, blue };
    }
    let alpha = u16::from(alpha);
    let blend = |channel: u8| -> u8 {
        let channel = u16::from(channel);
        ((channel * alpha + 20 * (255 - alpha)) / 255) as u8
    };
    PreviewRgb {
        red: blend(red),
        green: blend(green),
        blue: blend(blue),
    }
}

fn timeline_attachment_is_image(attachment: &TimelineAttachment) -> bool {
    attachment
        .mime_type
        .as_deref()
        .is_some_and(|mime_type| mime_type.starts_with("image/"))
        || attachment
            .path
            .as_deref()
            .is_some_and(path_has_image_extension)
}

fn path_has_image_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg"
            )
        })
        .unwrap_or(false)
}

fn timeline_attachment_label(attachment: &TimelineAttachment) -> String {
    attachment
        .name
        .clone()
        .or_else(|| attachment.path.clone())
        .or_else(|| attachment.id.clone())
        .unwrap_or_else(|| "unnamed".into())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::core::model::{
        BranchNode, ChecklistItem, CliTool, SessionStatus, SessionSummary, TimelineEvent,
        TimelineEventMetadata, WorkCapsule,
    };

    fn workbench_with_attachment(path: Option<String>) -> WorkbenchData {
        WorkbenchData {
            source: CliTool::Codex,
            target: CliTool::Hermes,
            sessions: vec![SessionSummary {
                id: "s1".into(),
                cli: CliTool::Codex,
                title: "fixture".into(),
                cwd: "/tmp".into(),
                updated_at: "2026-06-09T00:00:00Z".into(),
                updated: "now".into(),
                runtime_status: Default::default(),
                runtime_reason: None,
                status: SessionStatus::Healthy,
                branch: None,
                token_count: None,
                health_reason: None,
                event_count: 1,
                resume_command: "codex resume s1".into(),
                source_provenance: Default::default(),
                source_path: None,
                source_size_bytes: None,
                parse_skip_count: 0,
                provider_metadata: None,
                anatomy: None,
            }],
            timeline: vec![TimelineEvent {
                id: "evt-001".into(),
                time: "12:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "image".into(),
                metadata: TimelineEventMetadata {
                    attachments: vec![TimelineAttachment {
                        id: Some("img-1".into()),
                        name: Some("preview.png".into()),
                        path,
                        mime_type: Some("image/png".into()),
                        ..TimelineAttachment::default()
                    }],
                    ..Default::default()
                },
            }],
            source_adapters: Vec::new(),
            capsule: WorkCapsule {
                version: 1,
                source_cli: CliTool::Codex,
                target_cli: CliTool::Hermes,
                source_session: "s1".into(),
                rewind_point: "evt-001".into(),
                compiler: "fixture".into(),
                handoff_label: "moonbox/hermes-rewind-evt-001".into(),
                goal: "preview".into(),
                state: "fixture".into(),
                decisions: Vec::new(),
                todo: vec![ChecklistItem {
                    done: false,
                    text: "inspect preview".into(),
                }],
                evidence: Vec::new(),
                risks: Vec::new(),
                handoff_artifact: None,
                handoff_artifact_path: None,
                handoff_runner: None,
                handoff_skill: None,
                raw_source_map: None,
                raw_refs: Vec::new(),
                coverage: Default::default(),
                redaction: Default::default(),
            },
            branches: vec![BranchNode {
                id: "root".into(),
                label: "source".into(),
                detail: "fixture".into(),
                active: true,
            }],
            compilers: Vec::new(),
        }
    }

    #[test]
    fn image_preview_reports_missing_path_without_decoding() {
        let data = workbench_with_attachment(None);

        let previews = build_timeline_image_previews(&data, 0);

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, ImagePreviewStatus::MissingPath);
        assert!(previews[0].rows.is_empty());
    }

    #[test]
    fn image_preview_decodes_local_png_into_half_block_rows() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "moonbox-preview-{}-{nanos}.png",
            std::process::id(),
        ));
        let image = image::RgbImage::from_fn(8, 4, |x, y| {
            if x < 4 && y < 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 80, 255])
            }
        });
        image.save(&path).expect("write png");
        let data = workbench_with_attachment(Some(path.display().to_string()));

        let previews = build_timeline_image_previews(&data, 0);

        let _ = fs::remove_file(&path);
        assert_eq!(previews.len(), 1);
        assert!(previews[0].is_rendered());
        assert_eq!(previews[0].dimensions, Some((8, 4)));
        assert!(!previews[0].rows.is_empty());
    }
}
