// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The CPU twin of the built-in `concat.combine-adjust` effect package.
//!
//! Jianying's `combine_adjust` panel is a set of signed sliders, each feeding
//! one transfer function before the shader applies it. The transfer functions
//! below are transcribed from its `SeekModeScript.lua`; the shader arithmetic
//! (pow for brightness, a matrix for saturation, pow around 0 and 1 for
//! shadows and highlights, a slope for black and white) is transcribed from
//! its `colorAdjust.frag`. Here it is in plain Rust so the things a probe
//! cannot express - an exact identity at zero and bounded output at the
//! extremes - are checked without a device. Keep this and the shader in step.
//!
//! Temperature, tint and the 3D-LUT looks are left to `concat.adjust` (which
//! already has white balance and curves) and `concat.color-match`; this
//! package is the panel's arithmetic knobs.

/// Saturation slider (−1..1) to the matrix gain. Jianying: `intensity + 1`.
pub fn saturation_param(intensity: f64) -> f64 {
    intensity + 1.0
}

/// Brightness slider (−1..1) to the exponent's scale. Jianying's piecewise
/// curve: steep below 0.7, gentler above, so the top of the slider is not
/// wasted.
pub fn brightness_param(intensity: f64) -> f64 {
    if intensity > 0.0 && intensity <= 0.7 {
        0.3 * intensity
    } else if intensity > 0.7 && intensity <= 1.0 {
        0.6333 * intensity - 0.2333
    } else {
        intensity
    }
}

/// Highlight slider to the exponent above 1. Jianying's quartic polynomial.
pub fn highlight_param(intensity: f64) -> f64 {
    let (p2, p3, p4) = (intensity.powi(2), intensity.powi(3), intensity.powi(4));
    1.0 + 0.503 * intensity + 0.183 * p2 + 0.147 * p3 + 0.067 * p4
}

/// Shadow slider to the exponent. The mirror of [`highlight_param`].
pub fn shadow_param(intensity: f64) -> f64 {
    let (p2, p3, p4) = (intensity.powi(2), intensity.powi(3), intensity.powi(4));
    1.0 - 0.503 * intensity + 0.183 * p2 - 0.147 * p3 + 0.067 * p4
}

/// Contrast slider (−1..1) to a linear gain about middle grey (0.435).
/// The sigmoid branch past 1 is omitted: this implementation holds the
/// linear, monotone part every slider reaches.
pub fn contrast_param(intensity: f64) -> f64 {
    (1.0 + intensity).max(0.0)
}

/// Black and white sliders to a slope and bias, from two control points.
pub fn black_white_param(black: f64, white: f64) -> (f64, f64) {
    let xb = (black + 1.0) * 0.5;
    let xw = (white + 1.0) * 0.5;
    let yb = xb * 0.5;
    let yw = 0.5 + (xw - 0.5) * 0.5;
    let span = xw - xb;
    if span.abs() < 1e-5 {
        return (1.0, 0.0);
    }
    let slope = (yw - yb) / span;
    (slope, yb - slope * xb)
}

/// Jianying's saturation luminance weights, from `colorAdjust.frag`.
const SAT_LUMA: [f64; 3] = [0.208540, 0.702086, 0.089374];

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Apply all knobs in Jianying's order: brightness, contrast, saturation,
/// highlight, shadow, black-and-white. `rgb` is display level, 0..=1;
/// the sliders are signed 0..=1 percentages (as `-100..100` / 100).
pub fn apply(rgb: [f64; 3], knobs: &CombineKnobs) -> [f64; 3] {
    let mut c = rgb.map(clamp01);

    if knobs.brightness.abs() > 1e-6 {
        let p = 1.0 + brightness_param(knobs.brightness) * 5.0;
        c = c.map(|x| 1.0 - (1.0 - x).powf(p.max(0.001)));
    }

    if knobs.contrast.abs() > 1e-6 {
        let gain = contrast_param(knobs.contrast);
        c = c.map(|x| clamp01(gain * (x - 0.435) + 0.435));
    }

    if knobs.saturation.abs() > 1e-6 {
        let s = saturation_param(knobs.saturation);
        let base = SAT_LUMA.map(|w| w * (1.0 - s));
        c = [
            clamp01(c[0] * (base[0] + s) + c[1] * base[1] + c[2] * base[2]),
            clamp01(c[0] * base[0] + c[1] * (base[1] + s) + c[2] * base[2]),
            clamp01(c[0] * base[0] + c[1] * base[1] + c[2] * (base[2] + s)),
        ];
    }

    if knobs.highlight.abs() > 1e-6 {
        let p = highlight_param(knobs.highlight);
        c = c.map(|x| 1.0 - (1.0 - x).powf(p.max(0.001)));
    }

    if knobs.shadow.abs() > 1e-6 {
        let p = shadow_param(knobs.shadow);
        c = c.map(|x| x.powf(p.max(0.001)));
    }

    if knobs.black.abs() > 1e-6 || knobs.white.abs() > 1e-6 {
        let (slope, bias) = black_white_param(knobs.black, knobs.white);
        c = c.map(|x| clamp01(slope * x + bias));
    }

    c
}

/// The panel's signed sliders, each in −1..=1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CombineKnobs {
    /// Brightness slider, −1..=1.
    pub brightness: f64,
    /// Contrast slider, −1..=1.
    pub contrast: f64,
    /// Saturation slider, −1..=1.
    pub saturation: f64,
    /// Highlight slider, −1..=1.
    pub highlight: f64,
    /// Shadow slider, −1..=1.
    pub shadow: f64,
    /// Black point slider, −1..=1.
    pub black: f64,
    /// White point slider, −1..=1.
    pub white: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn all_zero_is_an_exact_identity() {
        for rgb in [[0.2, 0.5, 0.8], [1.0, 0.0, 0.3], [0.5, 0.5, 0.5]] {
            let out = apply(rgb, &CombineKnobs::default());
            for i in 0..3 {
                assert!(close(out[i], rgb[i]), "zero changed {rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn the_transfer_functions_match_jianying_at_their_ends() {
        assert!(close(saturation_param(0.0), 1.0));
        assert!(close(saturation_param(1.0), 2.0));
        assert!(close(brightness_param(0.7), 0.21));
        assert!(close(highlight_param(0.0), 1.0));
        assert!(close(shadow_param(0.0), 1.0));
        assert!(close(black_white_param(0.0, 0.0).0, 1.0));
    }

    #[test]
    fn positive_brightness_raises_a_mid_tone() {
        let out = apply(
            [0.3, 0.3, 0.3],
            &CombineKnobs {
                brightness: 0.5,
                ..Default::default()
            },
        );
        assert!(out[0] > 0.3, "brightening did not raise: {out:?}");
    }

    #[test]
    fn negative_brightness_lowers_a_mid_tone() {
        let out = apply(
            [0.5, 0.5, 0.5],
            &CombineKnobs {
                brightness: -0.5,
                ..Default::default()
            },
        );
        assert!(out[0] < 0.5, "darkening did not lower: {out:?}");
    }

    #[test]
    fn saturation_keeps_a_grey_grey() {
        let out = apply(
            [0.4, 0.4, 0.4],
            &CombineKnobs {
                saturation: 1.0,
                ..Default::default()
            },
        );
        assert!(
            close(out[0], out[1]) && close(out[1], out[2]),
            "grey gained colour: {out:?}"
        );
    }

    #[test]
    fn the_panel_never_leaves_the_unit_range() {
        let knobs = CombineKnobs {
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            highlight: 1.0,
            shadow: 1.0,
            black: 1.0,
            white: 1.0,
        };
        for rgb in [[0.0, 0.0, 0.0], [0.5, 0.2, 0.9], [1.0, 1.0, 1.0]] {
            let out = apply(rgb, &knobs);
            assert!(
                out.iter().all(|x| *x >= 0.0 && *x <= 1.0),
                "out of range: {out:?}"
            );
        }
    }

    #[test]
    fn highlight_lifts_more_than_shadow_at_the_same_positive_setting() {
        let hi = apply(
            [0.8, 0.8, 0.8],
            &CombineKnobs {
                highlight: 0.8,
                ..Default::default()
            },
        );
        let sh = apply(
            [0.8, 0.8, 0.8],
            &CombineKnobs {
                shadow: 0.8,
                ..Default::default()
            },
        );
        assert!(
            hi[0] >= sh[0],
            "highlight and shadow crossed over: {hi:?} vs {sh:?}"
        );
    }
}
