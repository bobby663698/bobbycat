// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! Following one box through a clip.
//!
//! A person draws a box around what they want followed (see
//! [`concat_core::roi`]) and this finds it again on every frame after: the
//! box's travel, frame by frame, in source pixels.
//!
//! It is the same machinery [`crate::stabilize`] uses for the camera,
//! pointed at one small box instead of the whole picture. Points are
//! seeded inside the box - the strongest corners first, a grid after them,
//! since a box drawn over a smooth cheek has corners only at its edge -
//! and each is followed into the next frame by pyramidal Lucas-Kanade flow
//! and then back again, because a point that does not return where it
//! started was never found. The box moves by the median of what the points
//! that survived say, not the mean: one point locking onto a passing
//! background costs the median nothing, and the mean everything.
//!
//! A box that loses its points - the target left the frame, or turned away
//! from the light - stays where it was and says so in `kept`. Geometry is
//! not enough on its own: a box sitting over the background still follows
//! the background after the target that was in it has gone, and every point
//! keeps up while reporting a target that is not there. So each frame also
//! asks whether the box still holds the same thing, by comparing its own
//! insides with the frame before: a covered target reads near nought, and
//! two frames of that in a row ends the run (`lost`), since there is
//! nothing left to follow from.
//!
//! The box's size never changes here. Scale is a second question, and one
//! the crop of a mask answers better than a tracker.

use concat_core::frame::Frame;
use concat_core::roi::Roi;

use crate::stabilize::track::{Gray, Pyramid};

/// A box as it was found on one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Followed {
    /// The frame it was found on, counted from the clip's first: zero is
    /// the frame the box was drawn on.
    pub frame: u32,
    /// The box there, in source pixels, the size it was drawn at.
    pub roi: Roi,
    /// How many of the box's points kept up, `0..=1`. Under
    /// [`Follower::KEPT`] the box did not move: the number is then the
    /// reason, and a caller showing a path can break it there.
    pub kept: f32,
    /// Whether this frame passed the appearance check and belongs in the path.
    pub accepted: bool,
    /// True when the box stopped holding what it was drawn around: covered,
    /// replaced, or gone. The box then stands at the last frame that did
    /// hold it, `kept` is nought, and the run has nothing more to say.
    pub lost: bool,
}

/// Follows one box, one frame at a time.
pub struct Follower {
    /// The frame before the one being followed, and the points in it.
    previous: Option<Pyramid>,
    /// Where the box's points are in `previous`.
    points: Vec<(f32, f32)>,
    /// The box as it stands.
    roi: Roi,
    /// Frames followed so far.
    frame: u32,
    /// What the box held on the last frame believed, so the next one can be
    /// asked whether it still holds it.
    look: Option<Look>,
    /// Frames in a row that did not.
    strikes: u32,
}

impl Follower {
    /// The share of a box's points that has to keep up for its move to be
    /// believed: a quarter. Lower would let three points of twelve drag a
    /// box across the frame on noise.
    pub const KEPT: f32 = 0.25;

    /// How much this frame's box has to look like the last one's, as the
    /// correlation of their samples. A covered target reads near nought, a
    /// frame of motion blur reads well above this, and a box being followed
    /// through a pan reads above nine tenths.
    pub const ALIKE: f32 = 0.40;

    /// How many frames in a row may look unlike the box before the run is
    /// over: two, so one blurred or flashing frame does not end a follow,
    /// while a target that is actually covered ends it in a fifth of a
    /// second at the rate this is read at.
    pub const STRIKES: u32 = 2;

    /// The most points a box is followed by. Enough that no single wrong
    /// one moves the median, few enough that a frame costs a millisecond.
    const MOST_POINTS: usize = 64;

    /// The fewest that still says where the box went.
    const FEWEST_POINTS: usize = 4;

    /// A box on `frame`, ready to follow what is inside it.
    ///
    /// `None` when the box is not one this can follow: empty, smaller than
    /// [`Roi::MIN_SIDE`] either way, or not wholly inside the frame. A box
    /// that hangs over the edge has no pixels there to follow.
    pub fn new(frame: &Frame, roi: Roi) -> Option<Follower> {
        if roi.is_empty() || !roi.is_confirmable() {
            return None;
        }
        if roi.x.checked_add(roi.width)? > frame.width()
            || roi.y.checked_add(roi.height)? > frame.height()
        {
            return None;
        }
        let pyramid = Pyramid::of(Gray::of_frame(frame));
        let points = seed(&pyramid, roi);
        let look = Some(Look::of(pyramid.base(), roi));
        Some(Follower {
            previous: Some(pyramid),
            points,
            roi,
            frame: 0,
            look,
            strikes: 0,
        })
    }

    /// The box as it stands, which is where the last followed frame put it.
    pub fn roi(&self) -> Roi {
        self.roi
    }

    /// Follows the box onto `frame`, the next one of the clip in order.
    ///
    /// Called with a frame that is not the next one - a seek, a re-run at a
    /// different rate - it still answers, but the answer means nothing: the
    /// points were found on a frame this one does not continue.
    pub fn step(&mut self, frame: &Frame) -> Followed {
        self.frame += 1;
        let next = Pyramid::of(Gray::of_frame(frame));
        let Some(previous) = self.previous.as_ref() else {
            // Only reachable if a caller builds a Follower and clears it,
            // which nothing does; keeping the box still is the safe answer.
            self.previous = Some(next);
            return Followed {
                frame: self.frame,
                roi: self.roi,
                kept: 0.0,
                accepted: false,
                lost: false,
            };
        };

        let mut shifts: Vec<(f32, f32)> = Vec::with_capacity(self.points.len());
        let mut moved_to: Vec<(f32, f32)> = Vec::with_capacity(self.points.len());
        for &from in &self.points {
            let Some(to) = previous.flow(&next, from) else {
                continue;
            };
            // Followed back again: a point that lands where it started was
            // a point, one that does not was a coincidence of gradients.
            let Some(back) = next.flow(previous, to) else {
                continue;
            };
            if (back.0 - from.0).powi(2) + (back.1 - from.1).powi(2) > 0.49 {
                continue;
            }
            shifts.push((to.0 - from.0, to.1 - from.1));
            moved_to.push(to);
        }

        let kept = moved_to.len() as f32 / self.points.len().max(1) as f32;
        let believed = moved_to.len() >= Self::FEWEST_POINTS && kept >= Self::KEPT;
        let mut candidate = self.roi;
        if believed {
            let (dx, dy) = (median(&mut shifts, true), median(&mut shifts, false));
            let x = (self.roi.x as f32 + dx)
                .round()
                .clamp(0.0, limit(frame.width(), self.roi.width));
            let y = (self.roi.y as f32 + dy)
                .round()
                .clamp(0.0, limit(frame.height(), self.roi.height));
            candidate = self.roi.moved_to(x as u32, y as u32);
        }

        // Geometry that kept up is not the same as the target still being
        // there: a box over the background follows the background. Ask the
        // box's own insides whether they are still the thing they were.
        let look = Look::of(next.base(), candidate);
        let alike = self
            .look
            .as_ref()
            .is_none_or(|held| held.alike(&look) >= Self::ALIKE);
        if !alike {
            // Keep the accepted box and frame so a transient occlusion can
            // recover from the last reliable point.
            self.strikes += 1;
            return Followed {
                frame: self.frame,
                roi: self.roi,
                kept: 0.0,
                accepted: false,
                lost: self.strikes >= Self::STRIKES,
            };
        }
        self.strikes = 0;
        self.look = Some(look);
        self.roi = candidate;

        // The points that kept up carry on; when too few did, the box is
        // re-seeded where it now stands rather than followed by the three
        // that are left, which would report the same three for ever.
        if moved_to.len() < Self::FEWEST_POINTS * 2 {
            self.points = seed(&next, self.roi);
        } else {
            self.points = moved_to;
        }
        self.previous = Some(next);
        Followed {
            frame: self.frame,
            roi: self.roi,
            kept,
            accepted: true,
            lost: false,
        }
    }
}

/// The furthest a box's corner may sit: the frame's side less the box's,
/// or nothing when the box is wider than the frame.
fn limit(frame_side: u32, box_side: u32) -> f32 {
    frame_side.saturating_sub(box_side) as f32
}

/// What a box holds, as a grid of brightness readings: enough identity to
/// ask the next frame whether it is still looking at the same thing, and
/// cheap enough to take on every frame.
///
/// A grid rather than every pixel because a target that is covered is
/// covered everywhere; a few dozen readings across the box say so as well
/// as all of them would, and cost nothing.
#[derive(Clone, Debug)]
struct Look {
    /// Row by row across the box, from its middle outward.
    samples: Vec<f32>,
}

impl Look {
    /// The box `roi` as it stands in `picture`.
    fn of(picture: &Gray, roi: Roi) -> Look {
        const GRID: usize = 8;
        let mut samples = Vec::with_capacity(GRID * GRID);
        for row in 0..GRID {
            for column in 0..GRID {
                let x = roi.x as f32 + roi.width as f32 * (column as f32 + 0.5) / GRID as f32;
                let y = roi.y as f32 + roi.height as f32 * (row as f32 + 0.5) / GRID as f32;
                samples.push(at(picture, x, y));
            }
        }
        Look { samples }
    }

    /// How much `other` is this look again, `-1..=1`: one when the readings
    /// move together, nought when they do not.
    ///
    /// A look with no contrast of its own is nothing to compare against -
    /// a box drawn on flat paint - and answers one, leaving the points to
    /// say where it went. Readings that went flat where the target had
    /// detail answer nought: whatever is there now is not the target.
    fn alike(&self, other: &Look) -> f32 {
        let count = self.samples.len().min(other.samples.len());
        if count == 0 {
            return 1.0;
        }
        let mean = |values: &[f32]| values[..count].iter().sum::<f32>() / count as f32;
        let (mean_self, mean_other) = (mean(&self.samples), mean(&other.samples));
        let (mut covariance, mut spread_self, mut spread_other) = (0.0f32, 0.0f32, 0.0f32);
        for index in 0..count {
            let a = self.samples[index] - mean_self;
            let b = other.samples[index] - mean_other;
            covariance += a * b;
            spread_self += a * a;
            spread_other += b * b;
        }
        if spread_self <= f32::EPSILON {
            return 1.0;
        }
        if spread_other <= f32::EPSILON {
            return 0.0;
        }
        covariance / (spread_self.sqrt() * spread_other.sqrt())
    }
}

/// One reading of `picture`, held at its edges.
fn at(picture: &Gray, x: f32, y: f32) -> f32 {
    let across = (x.max(0.0) as usize).min(picture.width.saturating_sub(1));
    let down = (y.max(0.0) as usize).min(picture.height.saturating_sub(1));
    picture.data[down * picture.width + across]
}

/// Follows `frames`, in order, from the box drawn on the first, and
/// answers the path a mask is given: each stop a fraction of the run and
/// the box's numbers there.
///
/// `picture` is the size the frames are decoded at, which is what the
/// fractions are relative to. A run of one frame answers one stop at zero,
/// and a run of none answers nothing.
///
/// Stops use their original indices in the full sequence. Unconfirmed
/// frames leave gaps, and confirmed loss ends tracking. If the iterator
/// has no exact length, its remaining items are counted for the time base.
pub fn run(
    frames: impl IntoIterator<Item = Frame>,
    roi: Roi,
    picture: (u32, u32),
) -> Vec<(f64, concat_core::roi::RoiFractions)> {
    let mut frames = frames.into_iter().enumerate();
    let (lower, upper) = frames.size_hint();
    let planned = (Some(lower) == upper).then_some(lower);
    let Some((_, first)) = frames.next() else {
        return Vec::new();
    };
    let Some(mut follower) = Follower::new(&first, roi) else {
        return Vec::new();
    };
    let mut boxes = vec![(0, follower.roi())];
    let mut read = 1;
    for (index, frame) in frames.by_ref() {
        read = index + 1;
        let step = follower.step(&frame);
        if step.lost {
            break;
        }
        if step.accepted {
            boxes.push((index, step.roi));
        }
    }
    let planned = planned.unwrap_or_else(|| read + frames.count());
    let last = planned.saturating_sub(1).max(1) as f64;
    boxes
        .into_iter()
        .filter_map(|(index, roi)| {
            let numbers = roi.fractions(picture.0, picture.1)?;
            Some((index as f64 / last, numbers))
        })
        .collect()
}

/// The middle of `shifts`' `x` or `y` readings, and zero for none.
///
/// The readings are sorted in place, which is why the pair arrives whole:
/// the median of one axis is the same work as the other.
fn median(shifts: &mut [(f32, f32)], across: bool) -> f32 {
    if shifts.is_empty() {
        return 0.0;
    }
    shifts.sort_by(|a, b| {
        let (a, b) = if across { (a.0, b.0) } else { (a.1, b.1) };
        a.total_cmp(&b)
    });
    let middle = shifts.len() / 2;
    let pick = |shift: &(f32, f32)| if across { shift.0 } else { shift.1 };
    if shifts.len() % 2 == 1 {
        pick(&shifts[middle])
    } else {
        (pick(&shifts[middle - 1]) + pick(&shifts[middle])) / 2.0
    }
}

/// Where to start following a box: its strongest corners, then a grid for
/// whatever the corners missed.
///
/// Corners alone would fail on a box drawn over something smooth with an
/// edge - a face, a card - and a grid alone would place points on flat
/// paint the flow cannot solve. Both are held inside the box by a margin,
/// since the flow reads a window around each point and a point on the edge
/// would read what is behind it.
fn seed(pyramid: &Pyramid, roi: Roi) -> Vec<(f32, f32)> {
    let base = pyramid.base();
    let margin = ((roi.width.min(roi.height) / 8) as f32).clamp(2.0, 10.0);
    let (left, top) = (roi.x as f32 + margin, roi.y as f32 + margin);
    let (right, bottom) = (
        (roi.x + roi.width).saturating_sub(1) as f32 - margin,
        (roi.y + roi.height).saturating_sub(1) as f32 - margin,
    );
    if right <= left || bottom <= top {
        return Vec::new();
    }
    let inside = |point: (f32, f32)| {
        point.0 >= left
            && point.0 <= right
            && point.1 >= top
            && point.1 <= bottom
            && point.0 >= 0.0
            && point.1 >= 0.0
            && point.0 <= base.width as f32 - 1.0
            && point.1 <= base.height as f32 - 1.0
    };

    let mut points: Vec<(f32, f32)> = pyramid
        .corners(24, 4, 5.0, &[])
        .into_iter()
        .filter(|point| inside(*point))
        .collect();

    // A grid across the box, for the corners a smooth target has none of.
    const GRID: usize = 5;
    for row in 0..GRID {
        for column in 0..GRID {
            let x = left + (right - left) * (column as f32 + 0.5) / GRID as f32;
            let y = top + (bottom - top) * (row as f32 + 0.5) / GRID as f32;
            let point = (x.floor(), y.floor());
            if !inside(point) {
                continue;
            }
            let near = points
                .iter()
                .any(|other| (other.0 - point.0).powi(2) + (other.1 - point.1).powi(2) < 9.0);
            if !near {
                points.push(point);
            }
        }
    }
    points.truncate(Follower::MOST_POINTS);
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with texture everywhere: waves of a few periods and
    /// angles, plus soft squares whose edges give the flow something.
    fn scene(x: f64, y: f64) -> f64 {
        let mut v = 128.0;
        for (k, (fx, fy, phase)) in [
            (0.083, 0.037, 0.3),
            (-0.041, 0.097, 1.1),
            (0.13, 0.11, 2.0),
            (0.021, -0.15, 0.7),
        ]
        .iter()
        .enumerate()
        {
            v += (28.0 / (k as f64 + 1.0).sqrt()) * (x * fx + y * fy + phase).sin();
        }
        v + 30.0
            * (3.0
                * (x * std::f64::consts::PI / 21.0).sin()
                * (y * std::f64::consts::PI / 17.0).sin())
            .tanh()
    }

    /// The scene moved by (`dx`, `dy`): what a camera pan looks like.
    fn frame(width: u32, height: u32, dx: f64, dy: f64) -> Frame {
        let mut picture = Frame::black(width, height);
        for y in 0..height {
            for x in 0..width {
                let value = scene(x as f64 - dx, y as f64 - dy).clamp(0.0, 255.0) as u8;
                picture.set_pixel(x, y, [value, value, value, 255]);
            }
        }
        picture
    }

    #[test]
    fn a_box_follows_what_moves_behind_it() {
        let first = frame(320, 240, 0.0, 0.0);
        let roi = Roi {
            x: 100,
            y: 80,
            width: 60,
            height: 60,
        };
        let mut follower = Follower::new(&first, roi).expect("a box inside the frame");

        // The target travels three pixels right and two down every frame.
        for step in 1..=12u32 {
            let next = frame(320, 240, 3.0 * step as f64, 2.0 * step as f64);
            let followed = follower.step(&next);
            assert_eq!(followed.frame, step);
            let expected = (100 + 3 * step, 80 + 2 * step);
            let found = (followed.roi.x, followed.roi.y);
            let off = (found.0 as i64 - expected.0 as i64).abs()
                + (found.1 as i64 - expected.1 as i64).abs();
            assert!(
                off <= 1,
                "frame {step}: found {found:?}, wanted {expected:?}"
            );
            assert!(followed.kept > 0.5, "frame {step}: kept {}", followed.kept);
            assert_eq!(
                (followed.roi.width, followed.roi.height),
                (roi.width, roi.height),
                "the box's size never changes"
            );
        }
    }

    #[test]
    fn a_target_that_stops_moving_keeps_its_box_still() {
        let still = frame(240, 180, 0.0, 0.0);
        let roi = Roi {
            x: 80,
            y: 60,
            width: 48,
            height: 48,
        };
        let mut follower = Follower::new(&still, roi).expect("a box");
        for _ in 0..6 {
            let followed = follower.step(&still);
            assert_eq!(followed.roi, roi);
        }
    }

    #[test]
    fn a_flat_frame_does_not_drag_the_box_anywhere() {
        let textured = frame(240, 180, 0.0, 0.0);
        let roi = Roi {
            x: 80,
            y: 60,
            width: 48,
            height: 48,
        };
        let mut follower = Follower::new(&textured, roi).expect("a box");
        let flat = Frame::black(240, 180);
        let followed = follower.step(&flat);
        assert_eq!(followed.roi, roi, "a box with nothing to follow stays put");
        assert!(followed.kept < Follower::KEPT, "kept {}", followed.kept);
    }

    #[test]
    fn a_box_leaving_the_frame_is_held_at_the_edge() {
        let first = frame(320, 240, 0.0, 0.0);
        let roi = Roi {
            x: 240,
            y: 100,
            width: 60,
            height: 60,
        };
        let mut follower = Follower::new(&first, roi).expect("a box");
        for step in 1..=8u32 {
            // Far enough that the box would pass the right edge.
            let next = frame(320, 240, 12.0 * step as f64, 0.0);
            let followed = follower.step(&next);
            assert!(
                followed.roi.x as u32 + followed.roi.width <= 320,
                "frame {step}: {} leaves the frame",
                followed.roi.x
            );
        }
    }

    #[test]
    fn a_box_the_tracker_cannot_follow_is_refused() {
        let picture = frame(160, 120, 0.0, 0.0);
        let too_small = Roi {
            x: 20,
            y: 20,
            width: 4,
            height: 4,
        };
        assert!(Follower::new(&picture, too_small).is_none());

        let past_the_edge = Roi {
            x: 140,
            y: 20,
            width: 48,
            height: 48,
        };
        assert!(Follower::new(&picture, past_the_edge).is_none());

        let inside = Roi {
            x: 20,
            y: 20,
            width: 48,
            height: 48,
        };
        assert!(Follower::new(&picture, inside).is_some());
    }

    #[test]
    fn a_target_that_is_covered_ends_the_run() {
        let first = frame(320, 240, 0.0, 0.0);
        let roi = Roi {
            x: 100,
            y: 80,
            width: 60,
            height: 60,
        };
        let mut follower = Follower::new(&first, roi).expect("a box inside the frame");

        // The target travels two pixels right a frame, and from the fourth
        // frame on something flat comes over it and stays.
        let mut lost_at = None;
        let mut last_accepted = roi;
        for step in 1..=12u32 {
            let mut next = frame(320, 240, 2.0 * step as f64, 0.0);
            if step >= 4 {
                let (x, y) = (100 + 2 * step, 80);
                for down in y..y + roi.height {
                    for across in x..x + roi.width {
                        next.set_pixel(across, down, [70, 70, 70, 255]);
                    }
                }
            }
            let followed = follower.step(&next);
            if followed.accepted {
                last_accepted = followed.roi;
            } else {
                assert_eq!(followed.roi, last_accepted);
            }
            if followed.lost {
                lost_at = Some(step);
                break;
            }
        }
        let at = lost_at.expect("a covered box is a lost one");
        assert!(at <= 6, "lost at frame {at}, wanted by six");
        assert!(
            follower.roi().x < 100 + 2 * at,
            "the box went after the cover: {} at frame {at}",
            follower.roi().x
        );
    }

    #[test]
    fn a_run_that_loses_its_target_stops_where_it_did() {
        let roi = Roi {
            x: 100,
            y: 80,
            width: 60,
            height: 60,
        };
        let frames: Vec<Frame> = (0..=10u32)
            .map(|step| {
                let mut picture = frame(320, 240, 2.0 * step as f64, 0.0);
                if step >= 5 {
                    let (x, y) = (100 + 2 * step, 80);
                    for down in y..y + roi.height {
                        for across in x..x + roi.width {
                            picture.set_pixel(across, down, [70, 70, 70, 255]);
                        }
                    }
                }
                picture
            })
            .collect();
        let reads = std::cell::Cell::new(0);
        let known = run(
            frames
                .iter()
                .cloned()
                .inspect(|_| reads.set(reads.get() + 1)),
            roi,
            (320, 240),
        );
        let unknown = run(frames.into_iter().filter(|_| true), roi, (320, 240));
        let times = |path: Vec<(f64, concat_core::roi::RoiFractions)>| {
            path.into_iter().map(|(at, _)| at).collect::<Vec<_>>()
        };
        // The target is visible on frames 0 through 4 of the original 11.
        assert_eq!(times(known), [0.0, 0.1, 0.2, 0.3, 0.4]);
        assert_eq!(reads.get(), 7, "two bad frames confirm the loss");
        assert_eq!(times(unknown), [0.0, 0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn a_single_covered_frame_leaves_a_gap_and_the_follow_recovers() {
        let roi = Roi {
            x: 100,
            y: 80,
            width: 60,
            height: 60,
        };
        let frames = [
            frame(320, 240, 0.0, 0.0),
            frame(320, 240, 2.0, 0.0),
            Frame::black(320, 240),
            frame(320, 240, 6.0, 0.0),
            frame(320, 240, 8.0, 0.0),
        ];
        let path = run(frames, roi, (320, 240));
        let times: Vec<_> = path.iter().map(|(at, _)| *at).collect();
        assert_eq!(times, [0.0, 0.25, 0.75, 1.0]);
        assert!(path[2].1.x > path[1].1.x, "the box resumes following");
    }

    #[test]
    fn the_median_ignores_one_point_that_went_its_own_way() {
        let mut shifts = [
            (2.0f32, 1.0f32),
            (2.0, 1.0),
            (2.0, 1.0),
            (2.0, 1.0),
            (40.0, 1.0),
        ];
        assert_eq!(median(&mut shifts, true), 2.0);
        assert_eq!(median(&mut shifts, false), 1.0);
        let mut none: [(f32, f32); 0] = [];
        assert_eq!(median(&mut none, true), 0.0);
    }

    #[test]
    fn the_median_of_an_even_count_is_between_the_two_middles() {
        let mut shifts = [(1.0f32, 0.0f32), (3.0, 0.0)];
        assert_eq!(median(&mut shifts, true), 2.0);
    }

    #[test]
    fn a_whole_run_answers_a_path_from_zero_to_one() {
        let roi = Roi {
            x: 100,
            y: 80,
            width: 60,
            height: 60,
        };
        let frames: Vec<Frame> = (0..=10)
            .map(|step| frame(320, 240, 2.0 * step as f64, 1.0 * step as f64))
            .collect();
        let path = run(frames, roi, (320, 240));
        assert_eq!(path.len(), 11);
        assert_eq!(path[0].0, 0.0);
        assert_eq!(path[10].0, 1.0);
        // The first stop is the box as drawn.
        assert!((path[0].1.x - roi.fractions(320, 240).unwrap().x).abs() < 1e-12);
        // And the last has moved right, as the target did.
        assert!(path[10].1.x > path[0].1.x, "{:?}", path.last());
    }

    #[test]
    fn a_run_of_no_frames_answers_nothing() {
        let roi = Roi {
            x: 10,
            y: 10,
            width: 40,
            height: 40,
        };
        assert!(run(Vec::new(), roi, (320, 240)).is_empty());
    }

    #[test]
    fn a_run_of_one_frame_answers_one_stop() {
        let roi = Roi {
            x: 10,
            y: 10,
            width: 40,
            height: 40,
        };
        let path = run(vec![frame(320, 240, 0.0, 0.0)], roi, (320, 240));
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].0, 0.0);
    }
}
