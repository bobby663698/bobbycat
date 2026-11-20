// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The blend modes, worked out from CapCut's own shader source.
//!
//! bobbycat reproduces what CapCut does, not what the specification says, so
//! the oracle is CapCut's MixMode fragment shader - the premultiplied half of
//! it - transcribed into Rust. The engine transcribes the same file into
//! WGSL. One original, two languages, two people, two derivations: that is
//! the cross-check. An oracle written from a *different* source would prove
//! only that bobbycat disagrees with the specification, which is not the
//! question.
//!
//! What this file has read, and what it has not, so a reader can judge for
//! themselves which numbers are independent:
//!
//! | read | not read |
//! |---|---|
//! | CapCut's `Resources/MixMode/*/xshader/blend.frag` - the oracle here | `blend_modes.wgsl` |
//! | `reference.rs`'s six existing arms, while checking visibility | `reference.rs`'s six new arms |
//! | engine's `PARITY_TABLE.md`, while answering it | - |
//!
//! Nothing here touches a compositor yet: `reference.rs`'s `mix()` is still
//! three-argument, and the half of this file that draws a frame and compares
//! it waits for `coverage` to be added to that signature. What is here is
//! arithmetic and the properties arithmetic must have.
//!
//! One deliberate exception is `Add`, which CapCut ships no MixMode package
//! for at all: it is the fixed-function `plus-lighter` the pipeline already
//! performs, and is marked as derived rather than transcribed.

/// Every mode, in menu order. The first six exist in `Blend` today; the six
/// after them are the ones being added.
const MODES: [Mode; 12] = [
    Mode::Normal,
    Mode::Multiply,
    Mode::Screen,
    Mode::Add,
    Mode::Lighten,
    Mode::Darken,
    Mode::Overlay,
    Mode::HardLight,
    Mode::SoftLight,
    Mode::ColorBurn,
    Mode::ColorDodge,
    Mode::LinearBurn,
];

/// The six being added, in the order a failure should list them.
const NEW_MODES: [Mode; 6] = [
    Mode::Overlay,
    Mode::HardLight,
    Mode::SoftLight,
    Mode::ColorBurn,
    Mode::ColorDodge,
    Mode::LinearBurn,
];

/// The shader's own `Eps`, which turns each division's `NaN` into a defined
/// value at the two ends.
const EPS: f32 = 1e-6;

/// A blend mode, named as CapCut's `blend.frag` names it. Not `Blend`: this
/// file must not inherit a transcription, the spelling of the enum included,
/// and mapping onto `Blend` is a later, separate step.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Normal,
    Multiply,
    Screen,
    Add,
    Lighten,
    Darken,
    Overlay,
    HardLight,
    SoftLight,
    ColorBurn,
    ColorDodge,
    LinearBurn,
}

/// One channel of a blend, **premultiplied** - `fc` is the foreground's
/// colour times its alpha, `bc` the background's, `fa` and `ba` the two
/// alphas - returning the premultiplied result the shader's `*PreMul`
/// function returns.
///
/// This is the path bobbycat draws through: one function per mode, the
/// result already composited, no fixed-function blend on top of it.
// The shader compares exactly - `fc == 0.0`, `bc == ba` - and a transcription
// that swapped those for `<=` would answer differently at the ends the two
// special branches exist to define. So the comparisons stay as written.
#[allow(clippy::float_cmp)]
fn premul(mode: Mode, fc: f32, bc: f32, fa: f32, ba: f32) -> f32 {
    match mode {
        // NormalBlendPreMul: source-over.
        Mode::Normal => fc + bc * (1.0 - fa),
        // MultiplyBlendPreMul.
        Mode::Multiply => fc * (1.0 - ba) + bc * (1.0 - fa) + fc * bc,
        // ScreenBlendPreMul - no coverage terms, screen being its own
        // composite.
        Mode::Screen => fc + bc - fc * bc,
        // Not in CapCut's MixMode packages: the plus-lighter the pipeline's
        // fixed-function add already performs.
        Mode::Add => (fc + bc).min(1.0),
        // LightenBlendPreMul.
        Mode::Lighten => fc * (1.0 - ba) + bc * (1.0 - fa) + (fc * ba).max(bc * fa),
        // DarkenBlendPreMul.
        Mode::Darken => fc * (1.0 - ba) + bc * (1.0 - fa) + (fc * ba).min(bc * fa),
        // OverlayPreMul: judged on the ground.
        Mode::Overlay => {
            if 2.0 * bc < ba {
                fc * (1.0 - ba) + bc * (1.0 - fa) + 2.0 * fc * bc
            } else {
                fc * (1.0 + ba) + bc * (1.0 + fa) - 2.0 * fc * bc - fa * ba
            }
        }
        // HardLightPreMul: the same arithmetic, judged on the foreground.
        Mode::HardLight => {
            if 2.0 * fc < fa {
                fc * (1.0 - ba) + bc * (1.0 - fa) + 2.0 * fc * bc
            } else {
                fc * (1.0 + ba) + bc * (1.0 + fa) - 2.0 * fc * bc - fa * ba
            }
        }
        // SoftLightPreMul: two branches, and only two. CapCut takes the
        // square root everywhere above a half; the specification's
        // polynomial below a quarter of ground is absent - see the last test
        // in this file, which records the difference rather than arguing
        // with it.
        Mode::SoftLight => {
            let t = bc / ba.max(EPS);
            if 2.0 * fc <= fa {
                fc * (1.0 - ba) + bc * (1.0 - fa) + bc * (fa + (2.0 * fc - fa) * (1.0 - t))
            } else {
                fc * (1.0 - ba) + bc * (1.0 + fa) + t.sqrt() * (2.0 * fc * ba - fa * ba)
                    - 2.0 * fc * bc
            }
        }
        // ColorBurn.
        Mode::ColorBurn => {
            if fc == 0.0 && bc == ba {
                bc * (1.0 - fa) + fa * ba
            } else if fc == 0.0 {
                bc * (1.0 - fa)
            } else {
                let t = (fc * ba).max(EPS);
                let burnt = ((ba - bc) * fa / t).min(1.0);
                fc * (1.0 - ba) + bc * (1.0 - fa) + fa * ba * (1.0 - burnt)
            }
        }
        // ColorDodgePreMul.
        Mode::ColorDodge => {
            if fc == fa && bc == 0.0 {
                fc * (1.0 - ba) + fa * ba
            } else if fc == fa {
                fc * (1.0 - ba) + bc * (1.0 - fa) + fa * ba
            } else {
                let t = (ba * (fa - fc)).max(EPS);
                fc * (1.0 - ba) + bc * (1.0 - fa) + fa * ba * (bc * fa / t).min(1.0)
            }
        }
        // LinearBurnBlendPreMul, written as the shader writes it: one
        // expression, no branch.
        Mode::LinearBurn => {
            fc * (1.0 - ba) + bc * (1.0 - fa) + (fc * ba + bc * fa - fa * ba).max(0.0)
        }
    }
}

/// `B(Cb, Cs)` as the `*BlendStraight` half of each file defines it, in
/// straight colour - the six being added, which is all this is needed for.
/// The shader's macros take `(f, b)`; the arguments here are `(cb, cs)`.
#[allow(clippy::float_cmp)]
fn straight_blend(mode: Mode, cb: f32, cs: f32) -> f32 {
    let (f, b) = (cs, cb);
    match mode {
        Mode::Overlay => {
            if b < 0.5 {
                2.0 * b * f
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - f)
            }
        }
        Mode::HardLight => {
            if f < 0.5 {
                2.0 * f * b
            } else {
                1.0 - 2.0 * (1.0 - f) * (1.0 - b)
            }
        }
        Mode::SoftLight => {
            if f < 0.5 {
                2.0 * b * f + b * b * (1.0 - 2.0 * f)
            } else {
                b.sqrt() * (2.0 * f - 1.0) + 2.0 * b * (1.0 - f)
            }
        }
        Mode::ColorBurn => {
            if f == 0.0 {
                f
            } else {
                (1.0 - (1.0 - b) / f).max(0.0)
            }
        }
        Mode::ColorDodge => {
            if f == 1.0 {
                f
            } else {
                (b / (1.0 - f)).min(1.0)
            }
        }
        Mode::LinearBurn => (f + b - 1.0).max(0.0),
        _ => unreachable!("only the six being added have a straight form here"),
    }
}

/// What a pixel stores: the premultiplied result over its own alpha. This is
/// the number the 2.4 encode sees.
fn expected(mode: Mode, cs: f32, alpha_s: f32, cb: f32, alpha_b: f32) -> f32 {
    let co = premul(mode, cs * alpha_s, cb * alpha_b, alpha_s, alpha_b);
    let alpha_o = alpha_s + alpha_b - alpha_s * alpha_b;
    if alpha_o <= 0.0 { 0.0 } else { co / alpha_o }
}

/// A stored level as linear light: BT.1886's 2.4, which both compositors
/// upload with. A specification, not a transcription.
fn decode(level: u8) -> f32 {
    (f32::from(level) / 255.0).powf(2.4)
}

/// Linear light back to the stored level nearest it.
fn encode(light: f32) -> u8 {
    let light = light.clamp(0.0, 1.0);
    let mut nearest = f32::INFINITY;
    let mut best = 0;
    for level in 0..=255u8 {
        let distance = (decode(level) - light).abs();
        if distance < nearest {
            nearest = distance;
            best = level;
        }
    }
    best
}

/// `I1`: a fully transparent foreground leaves the ground as it was. The
/// `(1-ba)` term of every mode meets here, on the premultiplied value, which
/// holds at `ba = 0` too - where the straight colour is 0 by definition and
/// would make a false green of this.
#[test]
fn a_transparent_foreground_leaves_the_ground_alone() {
    for mode in MODES {
        for ba in [0.0, 0.25, 0.5, 1.0] {
            let co = premul(mode, 0.0, 0.3 * ba, 0.0, ba);
            assert!(
                (co - 0.3 * ba).abs() < 1e-6,
                "{mode:?} at ba {ba}: {co}, not {}",
                0.3 * ba
            );
        }
    }
}

/// `I2`: a fully transparent ground is the foreground itself - the `(1-fa)`
/// term, likewise alone.
#[test]
fn a_transparent_ground_leaves_the_foreground_alone() {
    for mode in MODES {
        for fa in [0.0, 0.25, 0.5, 1.0] {
            let co = premul(mode, 0.9 * fa, 0.0, fa, 0.0);
            assert!(
                (co - 0.9 * fa).abs() < 1e-6,
                "{mode:?} at fa {fa}: {co}, not {}",
                0.9 * fa
            );
        }
    }
}

/// `I3`: both opaque, the premultiplied path must equal the straight one.
/// Two transcriptions of two different functions in the same file - if the
/// premultiplied form above were mis-copied, this is what says so.
#[test]
fn the_two_paths_agree_at_full_alpha() {
    for mode in NEW_MODES {
        for (cb, cs) in [(0.3, 0.8), (0.8, 0.3), (0.05, 0.8), (0.0, 0.0), (1.0, 1.0)] {
            let premultiplied = premul(mode, cs, cb, 1.0, 1.0);
            let straight = straight_blend(mode, cb, cs);
            assert!(
                (premultiplied - straight).abs() < 1e-5,
                "{mode:?} at Cb {cb}, Cs {cs}: {premultiplied} not {straight}"
            );
        }
    }
}

/// Overlay and hard light are one function with the operands exchanged, so
/// they agree whenever both colours fall on the same side of a half - and a
/// test at such an input cannot tell them apart: it passes with the two
/// functions swapped. The discriminating input has to cross the half.
#[test]
fn overlay_and_hard_light_agree_on_one_side_of_half() {
    for (cb, cs) in [(0.2, 0.3), (0.8, 0.7), (0.1, 0.9), (0.6, 0.45)] {
        let overlay = premul(Mode::Overlay, cs, cb, 1.0, 1.0);
        let hard = premul(Mode::HardLight, cs, cb, 1.0, 1.0);
        let apart = (overlay - hard).abs();
        if (cb <= 0.5) == (cs <= 0.5) {
            assert!(apart < 1e-6, "Cb {cb}, Cs {cs}: {overlay} vs {hard}");
        } else {
            assert!(apart > 1e-3, "Cb {cb}, Cs {cs} should differ: {overlay}");
        }
    }
}

/// The three functions with a branch at a half meet there rather than jump:
/// the two sides are written differently and are the same curve. Hard light
/// and soft light branch on the foreground, overlay on the ground - so a
/// test that nudges the wrong one of the two crosses no branch at all and
/// passes without comparing anything.
#[test]
fn the_branches_meet_at_the_half() {
    let at = premul(Mode::HardLight, 0.5, 0.3, 1.0, 1.0);
    let below = premul(Mode::HardLight, 0.5 - 1e-5, 0.3, 1.0, 1.0);
    let above = premul(Mode::HardLight, 0.5 + 1e-5, 0.3, 1.0, 1.0);
    assert!((below - at).abs() < 1e-4, "hard light: {below}, {at}, {above}");
    assert!((above - at).abs() < 1e-4, "hard light: {below}, {at}, {above}");

    let at = premul(Mode::Overlay, 0.6, 0.5, 1.0, 1.0);
    let below = premul(Mode::Overlay, 0.6, 0.5 - 1e-5, 1.0, 1.0);
    let above = premul(Mode::Overlay, 0.6, 0.5 + 1e-5, 1.0, 1.0);
    assert!((below - at).abs() < 1e-4, "overlay: {below}, {at}, {above}");
    assert!((above - at).abs() < 1e-4, "overlay: {below}, {at}, {above}");

    let at = premul(Mode::SoftLight, 0.5, 0.7, 1.0, 1.0);
    let below = premul(Mode::SoftLight, 0.5 - 1e-5, 0.7, 1.0, 1.0);
    let above = premul(Mode::SoftLight, 0.5 + 1e-5, 0.7, 1.0, 1.0);
    assert!((below - at).abs() < 1e-4, "soft light: {below}, {at}, {above}");
    assert!((above - at).abs() < 1e-4, "soft light: {below}, {at}, {above}");
}

/// Bright over dark: the two colours sit either side of a half, so overlay
/// takes the multiply side and hard light the screen side, and the six new
/// modes land on six different stored levels.
///
/// The quantisation is the point. Two modes less than a level apart are two
/// modes CI cannot tell apart - a green run would say nothing about either -
/// so the discriminating input has to survive being written into eight bits.
#[test]
fn the_discriminating_input_survives_eight_bit_quantisation() {
    let cs = decode(232);
    let cb = decode(77);
    let mut levels: Vec<u8> = NEW_MODES
        .iter()
        .map(|mode| encode(expected(*mode, cs, 1.0, cb, 1.0)))
        .collect();
    eprintln!("the six at Cb 77, Cs 232: {levels:?}");
    let count = levels.len();
    levels.sort_unstable();
    levels.dedup();
    assert_eq!(levels.len(), count, "two modes share a stored level");
}

/// Colour burn saturates to zero and colour dodge to white. These two ends
/// are exact however the input is quantised, which makes them the safest
/// cells to hold a shader to.
#[test]
fn saturation_is_exact_at_both_ends() {
    let burnt = premul(Mode::ColorBurn, 0.1, 0.3, 1.0, 1.0);
    assert!(burnt < 1e-6, "a burn past its quotient is not zero: {burnt}");
    let dodged = premul(Mode::ColorDodge, 0.9, 0.3, 1.0, 1.0);
    assert!((dodged - 1.0).abs() < 1e-6, "a dodge past it is not one: {dodged}");
}

/// CapCut is not the specification, and this is where the two part company.
///
/// Above a half of foreground the specification takes a square root of the
/// ground - except below a quarter of ground, where it takes the polynomial
/// `((16b-12)b+4)b` instead. CapCut's `SoftLightPreMul` has no polynomial:
/// one branch, one square root, everywhere. On a mid-dark ground the two
/// agree to the level and the difference is invisible; on a very dark one it
/// is ten levels, which is a picture somebody could see.
///
/// This test is the answer to "why not the specification's formula": because
/// bobbycat reproduces CapCut.
#[test]
fn soft_light_differs_from_the_specification_below_a_quarter_ground() {
    let cs = 0.8;
    for (cb, least) in [(0.05, 5u8), (0.20, 0)] {
        let cut = straight_blend(Mode::SoftLight, cb, cs);
        let w3c = if cb <= 0.25 {
            cb + (2.0 * cs - 1.0) * (((16.0 * cb - 12.0) * cb + 4.0) * cb - cb)
        } else {
            cb + (2.0 * cs - 1.0) * (cb.sqrt() - cb)
        };
        let apart = encode(cut).abs_diff(encode(w3c));
        eprintln!(
            "soft light at Cb {cb}: CapCut {} against the specification {w3c}, {apart} levels",
            encode(cut)
        );
        if least == 0 {
            assert_eq!(apart, 0, "Cb {cb}: {apart} levels apart, expected none");
        } else {
            assert!(apart >= least, "Cb {cb}: only {apart} levels apart");
        }
    }
}
