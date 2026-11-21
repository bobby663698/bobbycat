// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The two compositors against each other, pixel by pixel: the worst a
//! frame is out, and where.
//!
//! The crate's own parity suite (`src/gpu/tests.rs`) scores a pair of
//! frames with SSIM, which is the right measure for a layer that went
//! missing or moved and the wrong one for a shader that is six levels out
//! in one corner: 0.99 says "looks the same", not "which pixel".
//! [`Deltas`] measures the largest channel difference and names its
//! coordinates, so a blend mode that is systematically wrong in one band
//! cannot hide behind a high average.
//!
//! Which tolerance a frame is held to, and why, is written down in
//! `bridge/departments/qa/PARITY_TOOL.md`. In short: a flat colour under a
//! plain blend is exact to a level; a rotated or scaled layer is held to
//! SSIM and, three pixels in from its own edge, to the same level; a layer
//! that went missing or moved is SSIM's alone.
//!
//! Every test skips on a machine with no usable adapter and says so.
//! `CONCAT_REQUIRE_GPU` turns that skip into a failure, which is how CI
//! knows the suite ran rather than did not.

use std::sync::Arc;

use concat_core::frame::{BYTES_PER_PIXEL, Frame};
use concat_core::timeline::{Blend, Transform};
use concat_render::{
    Compositor, CpuCompositor, FramePlan, PlannedLayer, WgpuCompositor, detached_clip, ssim,
};

/// The GPU compositor, or `None` on a machine that has no usable adapter.
fn gpu() -> Option<WgpuCompositor> {
    let compositor = WgpuCompositor::new();
    if compositor.is_none() {
        assert!(
            std::env::var_os("CONCAT_REQUIRE_GPU").is_none(),
            "CONCAT_REQUIRE_GPU is set and no GPU adapter is usable"
        );
        eprintln!("no usable GPU adapter; skipping");
    }
    compositor
}

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Frame {
    let mut frame = Frame::transparent(width, height);
    frame.fill(rgba);
    frame
}

/// A picture with something in every pixel - a gradient in both directions -
/// so a moved or mirrored one is caught rather than matching by being
/// uniform.
///
/// Every channel ramps smoothly on purpose. A hard edge inside the picture
/// (a bar, a spot) is the one thing this must not have: bilinear sampling
/// puts that edge's two colours either side of a sub-pixel boundary, the
/// CPU and the GPU round that boundary differently by a fraction of a
/// pixel, and on a jump of 180 levels a fraction of a pixel is forty-odd
/// levels. Measured: 77 over 232 with a hard bar gives a worst of 48 and
/// 258 pixels out three in from the layer's edge; the same picture with
/// every channel smooth gives a worst of 2 and three pixels out. The layer
/// rim already exercises bilinear sampling on a real edge - see the rotated
/// test - so an internal hard edge only adds noise to the comparison.
fn gradient(width: u32, height: u32) -> Frame {
    let mut frame = Frame::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            let r = (x * 255 / width.max(1)) as u8;
            let g = (y * 255 / height.max(1)) as u8;
            let b = 120 + (x * 100 / width.max(1)) as u8;
            frame.set_pixel(x, y, [r, g, b, 255]);
        }
    }
    frame
}

fn layer(frame: Frame) -> PlannedLayer {
    PlannedLayer::picture(detached_clip(), Arc::new(frame))
}

fn plan(width: u32, height: u32, layers: Vec<PlannedLayer>) -> FramePlan {
    FramePlan {
        layers,
        ..FramePlan::empty(width, height)
    }
}

/// How far apart two frames are, pixel by pixel: for each pixel, the
/// largest of its four channel differences, alpha among them.
struct Deltas {
    width: u32,
    height: u32,
    map: Vec<u8>,
}

impl Deltas {
    /// The differences between `expected` and `actual`, which must be the
    /// same size.
    fn new(expected: &Frame, actual: &Frame) -> Deltas {
        assert_eq!(
            (expected.width(), expected.height()),
            (actual.width(), actual.height()),
            "the two frames are not the same size"
        );
        let map = expected
            .pixels()
            .chunks_exact(BYTES_PER_PIXEL)
            .zip(actual.pixels().chunks_exact(BYTES_PER_PIXEL))
            .map(|(a, b)| {
                a.iter()
                    .zip(b)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        Deltas {
            width: expected.width(),
            height: expected.height(),
            map,
        }
    }

    /// The difference at `(x, y)`.
    fn at(&self, x: u32, y: u32) -> u8 {
        self.map[y as usize * self.width as usize + x as usize]
    }

    /// Whether every pixel within `margin` of `(x, y)` is marked: true in
    /// the middle of a region, false within `margin` of its rim.
    ///
    /// Fed one frame drawn with the layer and one drawn without it, this
    /// says "three pixels in from the layer's own edge", which is the part
    /// of a rotated or scaled layer that is held pixel for pixel (see
    /// `PARITY_TOOL.md`): along the outline a pixel mixes what is inside
    /// the layer with what is outside, and two samplers put that boundary
    /// in slightly different places.
    fn inside(&self, x: u32, y: u32, margin: u32) -> bool {
        let left = x.saturating_sub(margin);
        let right = x.saturating_add(margin).min(self.width - 1);
        let top = y.saturating_sub(margin);
        let bottom = y.saturating_add(margin).min(self.height - 1);
        (top..=bottom).all(|row| (left..=right).all(|col| self.at(col, row) > 0))
    }

    /// The worst pixel among those `keep` accepts, and how the differences
    /// among them are spread.
    fn measure(&self, tolerance: u8, keep: impl Fn(u32, u32) -> bool) -> Diff {
        let mut diff = Diff {
            max: 0,
            at: (0, 0),
            histogram: [0; 256],
            over: 0,
            first_over: None,
            counted: 0,
        };
        for y in 0..self.height {
            for x in 0..self.width {
                if !keep(x, y) {
                    continue;
                }
                let delta = self.at(x, y);
                diff.counted += 1;
                diff.histogram[usize::from(delta)] += 1;
                if delta > diff.max {
                    diff.max = delta;
                    diff.at = (x, y);
                }
                if delta > tolerance {
                    diff.over += 1;
                    diff.first_over.get_or_insert((x, y, delta));
                }
            }
        }
        diff
    }
}

/// What [`Deltas::measure`] found.
struct Diff {
    /// The largest difference among the pixels counted.
    max: u8,
    /// Where it is.
    at: (u32, u32),
    /// Pixels by their own largest channel difference: `histogram[3]` is
    /// how many are three levels out.
    histogram: [u32; 256],
    /// Pixels out by more than the tolerance.
    over: u32,
    /// The first of those, for a message that fits on a line.
    first_over: Option<(u32, u32, u8)>,
    /// How many pixels were counted at all.
    counted: u32,
}

impl Diff {
    /// One line: how many are out, the worst and where, and the spread.
    fn report(&self) -> String {
        let first = match self.first_over {
            Some((x, y, delta)) => format!(", first over at ({x}, {y}) by {delta}"),
            None => String::new(),
        };
        format!(
            "{} of {} pixels out, worst {} at ({}, {}){}, spread {}",
            self.over,
            self.counted,
            self.max,
            self.at.0,
            self.at.1,
            first,
            self.spread()
        )
    }

    /// The differences, as "level:count" for every level a person can see,
    /// and one figure for everything past sixteen.
    fn spread(&self) -> String {
        let mut words = String::new();
        let mut far = 0;
        for (level, count) in self.histogram.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            if level > 16 {
                far += count;
                continue;
            }
            if !words.is_empty() {
                words.push(' ');
            }
            words.push_str(&format!("{level}:{count}"));
        }
        if far > 0 {
            if !words.is_empty() {
                words.push(' ');
            }
            words.push_str(&format!("over 16:{far}"));
        }
        words
    }
}

/// A flat picture at half strength, multiplied over a flat ground: both
/// compositors spell the same arithmetic, so every pixel must agree to a
/// level. This is the tier a new blend mode is held to.
#[test]
fn a_flat_blend_stays_within_one_level_of_the_cpu_reference() {
    let Some(mut gpu) = gpu() else { return };
    let ground = layer(solid(64, 48, [180, 120, 60, 255]));
    let mut over = layer(solid(64, 48, [40, 200, 240, 255]));
    over.opacity = 0.5;
    over.blend = Blend::Multiply;
    let plan = plan(64, 48, vec![ground, over]);

    let expected = CpuCompositor.render(&plan);
    let actual = gpu.render(&plan);
    assert!(
        actual
            .pixels()
            .chunks_exact(BYTES_PER_PIXEL)
            .all(|px| px[3] == 255),
        "the GPU's frame is not opaque"
    );

    let diff = Deltas::new(&expected, &actual).measure(1, |_, _| true);
    eprintln!("flat multiply: {}", diff.report());
    assert_eq!(diff.over, 0, "flat multiply: {}", diff.report());
}

/// All twelve modes, flat and opaque. The six new ones reach the GPU by a
/// different road from the six old ones - a copy of the ground, a fragment
/// stage that returns the complete composite, and `blend: None` - so a
/// green check on Multiply says nothing about them. Print the worst stored
/// level per mode: a systematic disagreement then names itself instead of
/// arriving as one unexplained red check. Three levels is the allowance
/// `PARITY_TOOL.md` gives a partial-alpha layer; the printed numbers are
/// what decides whether this can be tightened to one.
#[test]
fn every_blend_lands_within_three_levels_of_the_cpu_reference() {
    let Some(mut gpu) = gpu() else { return };
    let ground = solid(64, 48, [180, 120, 60, 255]);
    let picture = solid(64, 48, [40, 200, 240, 255]);
    for blend in Blend::ALL {
        let mut over = layer(picture.clone());
        over.blend = blend;
        let plan = plan(64, 48, vec![layer(ground.clone()), over]);
        let expected = CpuCompositor.render(&plan);
        let actual = gpu.render(&plan);
        assert!(
            actual
                .pixels()
                .chunks_exact(BYTES_PER_PIXEL)
                .all(|px| px[3] == 255),
            "{blend:?}: the GPU's frame is not opaque"
        );
        let diff = Deltas::new(&expected, &actual).measure(3, |_, _| true);
        eprintln!("{blend:?}: {}", diff.report());
        assert_eq!(diff.over, 0, "{blend:?}: {}", diff.report());
    }
}

/// A gradient laid down at 1.3 times and thirty degrees: SSIM holds the
/// whole picture, and every pixel three in from the layer's own edge must
/// agree to two levels. The rim itself is bilinear sampling either side of
/// one edge, where the two samplers round differently, and is not held to a
/// level - see `PARITY_TOOL.md`.
///
/// Two rather than one, measured on the gnu build: three pixels of 4473 sit
/// two levels apart, and they are the same three at every scale tried. A
/// fraction of a pixel is the smallest disagreement two bilinear samplers
/// can have about where a texel centre is, and on the steepest ramp this
/// picture has that fraction is worth two levels. One level is what a flat
/// colour under a plain blend is held to, and stays there.
#[test]
fn a_rotated_layer_matches_the_cpu_reference_away_from_its_edge() {
    let Some(mut gpu) = gpu() else { return };
    let ground = Arc::new(solid(80, 60, [30, 30, 40, 255]));
    let mut turned = layer(gradient(48, 36));
    turned.transform = Transform {
        scale: 1.3,
        rotation: 30.0,
        offset_x: 0.05,
        offset_y: -0.07,
        ..Transform::default()
    };
    let under = PlannedLayer::picture(detached_clip(), ground);
    let bare = plan(80, 60, vec![under.clone()]);
    let plan = plan(80, 60, vec![under, turned]);

    let expected = CpuCompositor.render(&plan);
    let actual = gpu.render(&plan);

    // Where the layer reaches at all: the frame drawn with it against the
    // frame drawn without it.
    let bare_frame = CpuCompositor.render(&bare);
    let footprint = Deltas::new(&bare_frame, &expected);
    let deltas = Deltas::new(&expected, &actual);
    let whole = deltas.measure(2, |_, _| true);
    let inside = deltas.measure(2, |x, y| footprint.inside(x, y, 3));
    let score = ssim(&expected, &actual);
    eprintln!("rotated: ssim {score:.4}, {}", whole.report());
    eprintln!(
        "rotated, three pixels in from its edge: {}",
        inside.report()
    );

    assert!(
        score >= 0.98,
        "rotated: ssim {score:.4}; {}",
        whole.report()
    );
    // Without this a layer that drew nothing at all would leave nothing to
    // count and pass the assertion below for the wrong reason.
    assert!(
        inside.counted > 0,
        "no pixel is three in from the layer's edge; the case compares nothing"
    );
    assert_eq!(
        inside.over,
        0,
        "rotated, three pixels in: {}",
        inside.report()
    );
}
