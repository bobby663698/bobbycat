// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! The tracking sheet: one frame of the clip, a box drawn on it, and the
//! run that follows the box through the rest of it.
//!
//! The sheet does no photography and no tracking itself. It asks the host
//! for a still of the clip at the playhead, turns a drag on that picture
//! into a box in its own pixels ([`concat_core::roi`]), and asks the host
//! to follow the box; the answer becomes a mask on the clip, keyed on its
//! centre, which is then a mask like any other - drawn, keyed and deleted
//! by the same controls as one drawn by hand.
//!
//! The box is kept in the picture's pixels, never in the sheet's
//! coordinates: the window may be resized between the drag and the press
//! of Follow, and a box stored the other way would move with it.

use std::sync::Arc;

use concat_core::frame::Frame;
use concat_core::roi::{Point, Roi, RoiFractions, Viewport};
use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{Clip, ClipKind, Mask, MaskShape};

use crate::i18n::{t, tf};
use crate::studio::Studio;
use crate::ui::TrackingData;

/// Everything that can happen to the tracking sheet.
#[derive(Clone, Debug)]
pub enum TrackingMsg {
    /// Draw a box: read the clip's frame at the playhead first.
    Pick,
    /// The frame that came back, or why none did.
    Picked(Result<Arc<Frame>, String>),
    /// The pointer on the picture's room: 0 pressed, 1 moved, 2 released.
    /// The four numbers are the pointer and the room, in the sheet's own
    /// coordinates; which pixel they mean is worked out here.
    Point(i32, f32, f32, f32, f32),
    /// Follow the box through the clip.
    Follow,
    /// The run that came back: the mask's path, or why there is none.
    Followed(Result<Run, String>),
    Dismiss,
}

/// A finished run, on its way to a mask.
#[derive(Clone, Debug)]
pub struct Run {
    /// The clip the box was drawn on.
    pub clip_id: String,
    /// The source seconds the run read between. The first stop's own, not
    /// the clip's: a box drawn at the playhead is followed from there.
    pub from: f64,
    /// The source second it stopped at.
    pub to: f64,
    /// Each stop: a fraction of the run, and the box there.
    pub stops: Vec<(f64, RoiFractions)>,
    /// The share of the box's points that kept up.
    pub kept: f32,
}

/// What is being followed: the clip, its file, and the source seconds the
/// clip plays between.
#[derive(Clone, Debug)]
struct Target {
    clip_id: String,
    path: String,
    from: f64,
    to: f64,
}

/// The sheet's state.
#[derive(Default)]
pub struct TrackingPane {
    pub open: bool,
    /// A still is being read, or the clip is being followed.
    pub busy: bool,
    /// The frame being drawn on.
    pub image: slint::Image,
    /// Its size in its own pixels.
    picture: Option<(u32, u32)>,
    /// What the frame was read for, kept so the follow does not have to
    /// read the selection again: it may have moved on by then.
    target: Option<Target>,
    /// Where the drag began, in picture pixels, while one is under way.
    start: Option<Point>,
    /// The box as it stands, in picture pixels.
    pub roi: Option<Roi>,
    /// The room the picture was last given, in the sheet's coordinates.
    view: (f32, f32),
    /// What the sheet's footer says.
    pub status: String,
}

impl TrackingPane {
    /// Applies one message. The studio is the rest of the window; this
    /// pane's own slot on it is a blank while this runs, as every pane's is.
    pub fn update(&mut self, msg: TrackingMsg, studio: &mut Studio) {
        match msg {
            TrackingMsg::Pick => self.pick(studio),
            TrackingMsg::Picked(result) => self.picked(studio, result),
            TrackingMsg::Point(phase, x, y, width, height) => {
                self.pointed(phase, x, y, width, height);
            }
            TrackingMsg::Follow => self.follow(),
            TrackingMsg::Followed(result) => self.followed(studio, result),
            TrackingMsg::Dismiss => self.dismiss(),
        }
    }

    /// The sheet as Slint shows it.
    pub fn data(&self) -> TrackingData {
        let (x, y, width, height) = self.box_in_view();
        TrackingData {
            open: self.open,
            picture: self.image.clone(),
            box_x: x,
            box_y: y,
            box_width: width,
            box_height: height,
            show_box: self.roi.is_some(),
            status: self.status.clone().into(),
            busy: self.busy,
            confirmable: !self.busy && self.roi.is_some_and(|roi| roi.is_confirmable()),
        }
    }

    /// Reads the clip's still at the playhead, and opens the sheet on it.
    fn pick(&mut self, studio: &mut Studio) {
        if self.busy {
            return;
        }
        let Some(target) = target_of(studio) else {
            studio.notify(&t("tracking.pickAClip"), true);
            return;
        };
        self.busy = true;
        self.open = true;
        self.status = t("tracking.reading");
        self.target = Some(target.clone());
        let path = target.path.clone();
        let from = target.from;
        crate::host::spawn_long_in_project(
            move || {
                concat_host::media::still_at(&path, from, concat_host::track::FOLLOW_WIDTH)
                    .map(Arc::new)
            },
            |studio, _, _, result| {
                studio.handle(crate::panes::Msg::Tracking(TrackingMsg::Picked(result)));
            },
        );
    }

    /// The still arrived, or did not.
    fn picked(&mut self, studio: &mut Studio, result: Result<Arc<Frame>, String>) {
        self.busy = false;
        match result {
            Ok(frame) => {
                self.picture = Some((frame.width(), frame.height()));
                self.image = crate::host::image_of(&frame);
                self.roi = None;
                self.start = None;
                self.open = true;
                self.status = t("tracking.hint");
            }
            Err(error) => {
                // A file the decoder cannot read is a notice, not a sheet
                // with nothing in it.
                self.open = false;
                self.picture = None;
                self.target = None;
                log::warn!("tracking: no still for the playhead: {error}");
                studio.notify(&tf("tracking.couldNotRead", &[&error]), true);
            }
        }
    }

    /// The pointer moved on the picture's room.
    fn pointed(&mut self, phase: i32, x: f32, y: f32, width: f32, height: f32) {
        let Some(picture) = self.picture else {
            return;
        };
        let Some(view) = Viewport::fit(picture.0, picture.1, f64::from(width), f64::from(height))
        else {
            return;
        };
        self.view = (width, height);
        if phase == 0 {
            // A press on a bar starts nothing: the bars are the sheet's,
            // not the picture's.
            match view.to_pixel(f64::from(x), f64::from(y)) {
                Some(point) => {
                    self.start = Some(point);
                    self.roi = None;
                    self.status = t("tracking.hint");
                }
                None => {
                    self.start = None;
                    self.roi = None;
                    self.status = t("tracking.outside");
                }
            }
            return;
        }
        let Some(start) = self.start else {
            return;
        };
        // A pointer past the picture while dragging means its edge, which
        // is what a person letting go outside the window meant.
        let end = view.clamp_to_pixel(f64::from(x), f64::from(y));
        let roi = Roi::of_drag(start, end, picture.0, picture.1);
        self.roi = Some(roi);
        self.status = describe(roi);
        if phase == 2 {
            self.start = None;
        }
    }

    /// Follows the box through the clip, on a worker.
    fn follow(&mut self) {
        if self.busy {
            return;
        }
        let (Some(roi), Some(target)) = (self.roi, self.target.clone()) else {
            return;
        };
        if !roi.is_confirmable() {
            return;
        }
        self.busy = true;
        self.status = t("tracking.following");
        let request = concat_host::track::Request {
            path: target.path.clone(),
            from: target.from,
            to: target.to,
            roi,
        };
        let clip_id = target.clip_id.clone();
        let (from, to) = (request.from, request.to);
        crate::host::spawn_long_in_project(
            move || {
                concat_host::track::follow(&request, |_| {}).map(|path| Run {
                    clip_id,
                    from,
                    to,
                    stops: path.stops,
                    kept: path.kept,
                })
            },
            |studio, _, _, result| {
                studio.handle(crate::panes::Msg::Tracking(TrackingMsg::Followed(result)));
            },
        );
    }

    /// The run arrived, or did not.
    fn followed(&mut self, studio: &mut Studio, result: Result<Run, String>) {
        self.busy = false;
        match result {
            Ok(run) => self.apply(studio, run),
            Err(error) => {
                self.status = t("tracking.failed");
                log::warn!("tracking: the follow stopped: {error}");
                studio.notify(&tf("tracking.failedDetail", &[&error]), true);
            }
        }
    }

    /// Turns the run into a mask on the clip, as one undo step.
    fn apply(&mut self, studio: &mut Studio, run: Run) {
        let Some(clip) = studio.clip(&run.clip_id) else {
            self.dismiss();
            return;
        };
        let mut masks: Vec<Mask> = clip.masks.clone();
        let mut mask = Mask::of("", "", MaskShape::Rectangle);
        let path: Vec<(f64, RoiFractions)> = run
            .stops
            .iter()
            .map(|&(at, numbers)| (clip_fraction(clip, run.from, run.to, at), numbers))
            .collect();
        mask.follow(&path);
        masks.push(mask);
        // An empty id is minted by `tidy_all`, so a second run adds a
        // second mask rather than renaming the first.
        let masks = Mask::tidy_all(masks);
        let stops = path.len();
        let kept = run.kept;
        studio.apply(Command::UpdateClip {
            clip_id: run.clip_id,
            patch: ClipPatch {
                masks: Some(masks),
                ..Default::default()
            },
        });
        self.open = false;
        self.roi = None;
        self.start = None;
        self.picture = None;
        self.target = None;
        self.image = slint::Image::default();
        // A run that lost its target halfway is worth saying: the mask is
        // there, and the person should know to look at its tail.
        if kept < 0.5 {
            studio.notify(&tf("tracking.lost", &[&stops]), true);
        } else {
            studio.notify(&tf("tracking.followed", &[&stops]), false);
        }
    }

    fn dismiss(&mut self) {
        if self.busy {
            return;
        }
        self.open = false;
        self.roi = None;
        self.start = None;
        self.picture = None;
        self.target = None;
        self.image = slint::Image::default();
        self.status.clear();
    }

    /// The box, in the sheet's own coordinates, for Slint to draw.
    fn box_in_view(&self) -> (f32, f32, f32, f32) {
        let (Some(roi), Some(picture)) = (self.roi, self.picture) else {
            return (0.0, 0.0, 0.0, 0.0);
        };
        let Some(view) = Viewport::fit(
            picture.0,
            picture.1,
            f64::from(self.view.0),
            f64::from(self.view.1),
        ) else {
            return (0.0, 0.0, 0.0, 0.0);
        };
        let at = view.to_view(Point::new(f64::from(roi.x), f64::from(roi.y)));
        (
            at.x as f32,
            at.y as f32,
            (f64::from(roi.width) * view.scale) as f32,
            (f64::from(roi.height) * view.scale) as f32,
        )
    }
}

/// Where a moment of the run falls in the clip, as the clip's own fraction:
/// what a key is measured in.
///
/// A run is not the clip. The box is drawn at the playhead, so a clip
/// followed from its middle starts its run at nought while its first key
/// belongs at a half - writing the run's own fraction would put the path at
/// the clip's head, keyed against the wrong frames.
fn clip_fraction(clip: &Clip, from: f64, to: f64, at: f64) -> f64 {
    if !clip.duration.is_finite() || clip.duration <= 0.0 {
        return at.clamp(0.0, 1.0);
    }
    let speed = if clip.speed.is_finite() && clip.speed > 0.0 {
        clip.speed
    } else {
        1.0
    };
    let first = (from - clip.source_start) / speed;
    let span = (to - from) / speed;
    ((first + span * at) / clip.duration).clamp(0.0, 1.0)
}

/// What the footer says about a box.
fn describe(roi: Roi) -> String {
    if roi.is_confirmable() {
        tf("tracking.box", &[&roi.x, &roi.y, &roi.width, &roi.height])
    } else {
        tf("tracking.tooSmall", &[&Roi::MIN_SIDE])
    }
}

/// The clip a box can be drawn on: the first picked one that is a video,
/// with a file behind it, and the source seconds it plays between.
///
/// The still is read at the playhead, held inside the clip: a playhead
/// past either end of it - which a timeline with the wrong clip selected
/// easily has - reads the clip's own last frame rather than the file's
/// first.
fn target_of(studio: &Studio) -> Option<Target> {
    let project = studio.project();
    for id in &studio.selection {
        let Some(clip) = studio.clip(id) else {
            continue;
        };
        if clip.kind != ClipKind::Video {
            continue;
        }
        let Some(media) = project.media.iter().find(|item| item.id == clip.media_id) else {
            continue;
        };
        let end = clip.start + clip.duration;
        let last = (end - 0.001).max(clip.start);
        let playhead = f64::from(studio.playhead).clamp(clip.start, last);
        let speed = if clip.speed.is_finite() && clip.speed > 0.0 {
            clip.speed
        } else {
            1.0
        };
        let from = clip.source_start + (playhead - clip.start) * speed;
        let to = clip.source_start + (end - clip.start) * speed;
        return Some(Target {
            clip_id: clip.id.clone(),
            path: media.path.clone(),
            from,
            to,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_clip(duration: f64) -> Clip {
        let mut clip = Clip::blank("c", "t", ClipKind::Video, "clip", 0.0, duration);
        clip.source_start = 0.0;
        clip.speed = 1.0;
        clip
    }

    #[test]
    fn a_run_that_starts_at_the_playhead_lands_where_the_playhead_is() {
        let clip = a_clip(10.0);
        // Drawn five seconds into a ten-second clip: the run's nought is
        // the clip's half, and its one is the clip's one.
        assert!((clip_fraction(&clip, 5.0, 10.0, 0.0) - 0.5).abs() < 1e-9);
        assert!((clip_fraction(&clip, 5.0, 10.0, 1.0) - 1.0).abs() < 1e-9);
        // A run with no span still answers the moment it was drawn at.
        assert!((clip_fraction(&clip, 5.0, 5.0, 1.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn the_clips_own_in_point_shifts_the_run() {
        let mut clip = a_clip(10.0);
        clip.source_start = 2.0;
        // The clip plays source 2s onwards, so source 7s is its own middle.
        assert!((clip_fraction(&clip, 7.0, 12.0, 0.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_faster_clip_fits_more_source_in_the_same_timeline() {
        let mut clip = a_clip(5.0);
        clip.speed = 2.0;
        assert!((clip_fraction(&clip, 0.0, 10.0, 0.5) - 0.5).abs() < 1e-9);
        // A run that reads past the clip's tail is held at the tail.
        assert_eq!(clip_fraction(&clip, 0.0, 20.0, 1.0), 1.0);
    }
}
