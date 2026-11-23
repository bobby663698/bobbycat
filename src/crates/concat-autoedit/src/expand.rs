// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! `ip_plan_v1` -> the editor's own commands.
//!
//! MCPP's `clip` is a finished segment; BobbyCat's clip is one lane object on
//! the timeline. One plan clip therefore expands into several commands, and
//! a whole candidate is one `Batch`. Media length is not in the plan, so the
//! host supplies probed duration and picture size for every asset before an
//! expansion can compute a tail-trim delta or a `fill` scale.
//!
//! Only `ip_plan_v1` is accepted. Unknown fields - including every v3
//! addition - are a hard parse error, never a silent drop.

use crate::candidate::Candidate;
use concat_project::Command;
use concat_project::commands::{ClipPatch, NewMedia, TrimEdge};
use concat_project::model::{ColorSpace, TextStyle};
use serde::Deserialize;
use std::collections::BTreeMap;
use thiserror::Error;

/// A host probe, cached across the whole plan: one asset, one probe.
#[derive(Clone, Debug)]
pub struct Probe {
    pub asset_id: String,
    /// Path the host would hand to `AddMedia`.
    pub path: String,
    pub duration: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_audio: bool,
}

/// What one plan became, including the features the first version reports
/// instead of silently dropping.
#[derive(Clone, Debug)]
pub struct Expansion {
    pub candidates: Vec<Candidate>,
    /// Things the plan asked for that no command expresses yet, in plan
    /// order. The caller must show or refuse these; ignoring them is a bug.
    pub unsupported: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error("the plan is not valid ip_plan_v1 JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported schema version `{0}`; only ip_plan_v1 is accepted")]
    Version(String),
    #[error("no probe for asset `{0}`; tail trims need the media duration")]
    MissingProbe(String),
    #[error("asset `{0}`: source_start + duration exceeds the probed length")]
    BadTiming(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema_version: String,
    #[allow(dead_code)]
    manifest_id: String,
    #[allow(dead_code)]
    profile_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    notes: Option<String>,
    clips: Vec<PlanClip>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanClip {
    clip_id: String,
    title: String,
    edit_reason: String,
    segments: Vec<Segment>,
    #[serde(default)]
    texts: Vec<Text>,
    #[serde(default)]
    audio_tracks: Vec<Audio>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Fit {
    Fit,
    Fill,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    asset_id: String,
    source_start: f64,
    duration: f64,
    fit: Fit,
    include_audio: bool,
    #[serde(default)]
    crop: Option<Crop>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Crop {
    #[allow(dead_code)]
    mode: String,
    #[allow(dead_code)]
    anchor_x: f64,
    #[allow(dead_code)]
    anchor_y: f64,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Position {
    Top,
    Center,
    Bottom,
}

impl Position {
    fn offset_y(self) -> f64 {
        match self {
            Self::Top => -0.35,
            Self::Center => 0.0,
            Self::Bottom => 0.35,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    start: f64,
    end: f64,
    text: String,
    position: Position,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audio {
    asset_id: String,
    #[allow(dead_code)]
    role: String,
    source_start: f64,
    #[allow(dead_code)]
    timeline_start: f64,
    duration: f64,
    #[serde(default)]
    gain_db: Option<f64>,
    #[serde(default)]
    fade_in: Option<f64>,
    #[serde(default)]
    fade_out: Option<f64>,
    #[serde(default)]
    duck_under_speech: bool,
}

/// Expand one v1 plan. `canvas` is the (width, height) of the project frame,
/// used only by `fill`.
pub fn expand(json: &str, probes: &[Probe], canvas: (u32, u32)) -> Result<Expansion, ExpandError> {
    let plan: Plan = serde_json::from_str(json)?;
    if plan.schema_version != "ip_plan_v1" {
        return Err(ExpandError::Version(plan.schema_version));
    }
    let probed: BTreeMap<&str, &Probe> = probes
        .iter()
        .map(|probe| (probe.asset_id.as_str(), probe))
        .collect();
    let mut added_media: BTreeMap<String, String> = BTreeMap::new();
    let mut next_media = 1usize;
    let mut next_clip = 1usize;
    let mut candidates = Vec::with_capacity(plan.clips.len());
    let mut unsupported = Vec::new();

    for clip in &plan.clips {
        let mut commands = Vec::new();
        let mut audio_assets = Vec::new();

        for segment in &clip.segments {
            let probe = get_probe(&probed, &segment.asset_id)?;
            check_timing(
                probe,
                segment.source_start,
                segment.duration,
                &segment.asset_id,
            )?;
            if let Some(crop) = &segment.crop {
                unsupported.push(format!(
                    "crop(mode={}, anchor={},{}) on `{}`",
                    crop.mode, crop.anchor_x, crop.anchor_y, segment.asset_id
                ));
            }
            let media_id = ensure_media(
                probe,
                &mut added_media,
                &mut next_media,
                &mut commands,
                false,
            );
            let clip_id = format!("c{}", next_clip);
            next_clip += 1;
            commands.push(Command::AddClip {
                media_id: media_id.clone(),
                track_id: "V1".to_owned(),
                start: 0.0,
                ripple: false,
            });
            if segment.source_start > 0.0 {
                commands.push(Command::TrimClip {
                    clip_id: clip_id.clone(),
                    edge: TrimEdge::Start,
                    delta: segment.source_start,
                    ripple: false,
                });
            }
            commands.push(Command::TrimClip {
                clip_id: clip_id.clone(),
                edge: TrimEdge::End,
                delta: probe.duration - segment.source_start - segment.duration,
                ripple: false,
            });
            if let (Fit::Fill, Some(width), Some(height)) = (segment.fit, probe.width, probe.height)
            {
                let scale = (canvas.0 as f64 / width as f64).max(canvas.1 as f64 / height as f64);
                commands.push(Command::SetClipTransform {
                    clip_id: clip_id.clone(),
                    scale: Some(scale),
                    offset_x: None,
                    offset_y: None,
                    rotation: None,
                    stretch_x: None,
                    stretch_y: None,
                });
            }
            commands.push(Command::UpdateClip {
                clip_id: clip_id.clone(),
                patch: ClipPatch {
                    muted: Some(!segment.include_audio),
                    ..ClipPatch::default()
                },
            });
            if segment.include_audio {
                audio_assets.push(segment.asset_id.clone());
            }
        }

        for text in &clip.texts {
            commands.push(Command::AddTextClip {
                track_id: None,
                above: true,
                start: text.start,
                style: Some(TextStyle {
                    content: text.text.clone(),
                    ..TextStyle::default()
                }),
                duration: Some(text.end - text.start),
                offset_y: Some(text.position.offset_y()),
            });
        }

        for audio in &clip.audio_tracks {
            let probe = get_probe(&probed, &audio.asset_id)?;
            check_timing(probe, audio.source_start, audio.duration, &audio.asset_id)?;
            if audio.duck_under_speech {
                unsupported.push(format!("duck_under_speech on `{}`", audio.asset_id));
            }
            let media_id = ensure_media(
                probe,
                &mut added_media,
                &mut next_media,
                &mut commands,
                true,
            );
            let clip_id = format!("c{}", next_clip);
            next_clip += 1;
            commands.push(Command::AddClip {
                media_id,
                track_id: "A1".to_owned(),
                start: 0.0,
                ripple: false,
            });
            if audio.source_start > 0.0 {
                commands.push(Command::TrimClip {
                    clip_id: clip_id.clone(),
                    edge: TrimEdge::Start,
                    delta: audio.source_start,
                    ripple: false,
                });
            }
            commands.push(Command::TrimClip {
                clip_id: clip_id.clone(),
                edge: TrimEdge::End,
                delta: probe.duration - audio.source_start - audio.duration,
                ripple: false,
            });
            commands.push(Command::UpdateClip {
                clip_id,
                patch: ClipPatch {
                    volume: audio.gain_db.map(db_to_linear),
                    fade_in: audio.fade_in,
                    fade_out: audio.fade_out,
                    ..ClipPatch::default()
                },
            });
            audio_assets.push(audio.asset_id.clone());
        }

        candidates.push(Candidate {
            id: clip.clip_id.clone(),
            title: clip.title.clone(),
            rationale: clip.edit_reason.clone(),
            commands,
            audio_assets,
            score: None,
        });
    }

    Ok(Expansion {
        candidates,
        unsupported,
    })
}

/// dB -> the linear gain `ClipPatch.volume` expects, to four decimals.
pub fn db_to_linear(db: f64) -> f64 {
    (10f64.powf(db / 20.0) * 10_000.0).round() / 10_000.0
}

fn get_probe<'a>(
    probed: &'a BTreeMap<&str, &'a Probe>,
    asset: &str,
) -> Result<&'a Probe, ExpandError> {
    probed
        .get(asset)
        .copied()
        .ok_or_else(|| ExpandError::MissingProbe(asset.to_owned()))
}

fn check_timing(probe: &Probe, start: f64, duration: f64, asset: &str) -> Result<(), ExpandError> {
    if start + duration > probe.duration {
        return Err(ExpandError::BadTiming(asset.to_owned()));
    }
    Ok(())
}

/// `AddMedia` happens once per asset in the whole plan. Later candidates
/// reuse the id; they still get their own new clips.
fn ensure_media(
    probe: &Probe,
    added: &mut BTreeMap<String, String>,
    next: &mut usize,
    commands: &mut Vec<Command>,
    audio_only: bool,
) -> String {
    if let Some(id) = added.get(&probe.asset_id) {
        return id.clone();
    }
    let id = format!("mid_{}", next);
    *next += 1;
    let name = probe
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&probe.path)
        .to_owned();
    commands.push(Command::AddMedia {
        item: NewMedia {
            path: probe.path.clone(),
            name,
            duration: Some(probe.duration),
            kind: if audio_only {
                concat_project::model::MediaKind::Audio
            } else {
                concat_project::model::MediaKind::Video
            },
            width: probe.width,
            height: probe.height,
            frame_rate: None,
            frame_rate_fraction: None,
            video_codec: None,
            audio_codec: None,
            has_audio: probe.has_audio || audio_only,
            audio_tracks: Vec::new(),
            origin: None,
            color_space: ColorSpace::default(),
        },
    });
    added.insert(probe.asset_id.clone(), id.clone());
    id
}
