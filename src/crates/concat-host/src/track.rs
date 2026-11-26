// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! Following one box through a clip: the frames, and the tracker on them.
//!
//! The window draws the box; this reads the clip's own frames and follows
//! it. Between them the two agree on nothing but pixels, which is why the
//! tracker is given them at one width ([`FOLLOW_WIDTH`]) rather than at the
//! media's own: a 4K source tracked at 4K costs sixteen times the flow for
//! no answer the mask can use.
//!
//! The run reads [`FOLLOW_RATE`] frames a second, not the source's own -
//! ten is where a target's travel between two reads is still inside the
//! flow's reach, and a minute of footage is six hundred reads rather than
//! eighteen hundred. Each is decoded by its own source time, so a clip that
//! is slowed, sped up or held shows the frames it really plays.
//!
//! A target that cannot be followed - the box vanished, a file that stopped
//! decoding - ends the run where it stands, and the path so far is still a
//! path: the caller decides whether half a follow is worth keeping.

use concat_core::roi::{Roi, RoiFractions};
use concat_vision::follow::Follower;

/// How many frames a second the tracker reads.
pub const FOLLOW_RATE: f64 = 10.0;

/// The width a frame is decoded at, the height following the picture.
pub const FOLLOW_WIDTH: u32 = 960;

/// What to follow, and where.
#[derive(Clone, Debug)]
pub struct Request {
    /// The media file.
    pub path: String,
    /// The source second the clip starts at.
    pub from: f64,
    /// The source second it ends at, exclusive.
    pub to: f64,
    /// The box on the source picture, in its own pixels at
    /// [`FOLLOW_WIDTH`] across.
    pub roi: Roi,
}

/// A followed box, from the first read to the last.
#[derive(Clone, Debug, Default)]
pub struct Path {
    /// Each stop: a fraction of the run, and the box there as a mask's
    /// numbers.
    pub stops: Vec<(f64, RoiFractions)>,
    /// How many frames were read.
    pub frames: u32,
    /// The share of the box's points that kept up, averaged: one is a
    /// target that was always there, nought one that never was.
    pub kept: f32,
    /// True when the box stopped holding the target: the path ends where
    /// the target was last seen, not where the run meant to read to.
    pub lost: bool,
}

/// Follows `request`'s box, calling `progress` with the fraction done after
/// each frame.
pub fn follow(request: &Request, mut progress: impl FnMut(f32)) -> Result<Path, String> {
    let span = (request.to - request.from).max(0.0);
    // Two reads at least: one frame has no travel to measure, and a clip
    // shorter than a tenth of a second is still a clip.
    let count = ((span * FOLLOW_RATE).round() as u32).max(2);
    let mut stops = Vec::with_capacity(count as usize);
    let mut follower: Option<Follower> = None;
    let mut kept = 0.0f32;
    let mut measured = 0u32;
    let mut frames = 0u32;
    let mut lost = false;

    for index in 0..count {
        let at = index as f64 / f64::from(count - 1);
        let seconds = request.from + span * at;
        let frame = crate::media::still_at(&request.path, seconds, FOLLOW_WIDTH)?;
        let picture = (frame.width(), frame.height());
        frames += 1;
        let roi = match follower.as_mut() {
            None => {
                let started = Follower::new(&frame, request.roi)
                    .ok_or_else(|| "the box is not one that can be followed".to_owned())?;
                let roi = started.roi();
                follower = Some(started);
                roi
            }
            Some(following) => {
                let step = following.step(&frame);
                // The box stopped holding the target: the stop it would
                // have written is the cover, not the target, so the run
                // ends at the frame before it rather than following the
                // background for the rest of the clip.
                if step.lost {
                    lost = true;
                    break;
                }
                kept += step.kept;
                measured += 1;
                step.roi
            }
        };
        if let Some(numbers) = roi.fractions(picture.0, picture.1) {
            stops.push((at, numbers));
        }
        progress(at as f32);
    }

    // Fewer stops than reads is a picture that vanished mid-run: a path of
    // one stop is a box that never moved, which is a true answer and a
    // useless one, so it is refused rather than drawn over the clip.
    if stops.len() < 2 {
        return Err("the clip had no picture to follow".to_owned());
    }
    Ok(Path {
        stops,
        frames,
        kept: if measured == 0 {
            1.0
        } else {
            kept / measured as f32
        },
        lost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_with_no_span_asks_for_two_reads() {
        // The count is the only part that can be tested without decoding;
        // `follow` itself needs a file, which the host's tests do not make.
        let request = Request {
            path: "none.mp4".to_owned(),
            from: 2.0,
            to: 2.0,
            roi: Roi {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
        };
        let span = (request.to - request.from).max(0.0);
        assert_eq!(((span * FOLLOW_RATE).round() as u32).max(2), 2);
    }
}
