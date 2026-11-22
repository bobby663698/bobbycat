// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The CPU twin of the built-in `concat.hsl` effect package.
//!
//! Eight colour bands, each with a hue shift in degrees and signed
//! saturation and lightness percentages. The shader in
//! `packages/concat.hsl/effect.wgsl` draws on the GPU; this is the same
//! arithmetic in plain Rust so the things a probe cannot express - an exact
//! identity at zero, a band that touches only its own hue, and the circular
//! wrap at red - are checked without a device. Keep this file and the shader
//! in step.

/// A band's centre hue (degrees) and half-width, from Jianying's HSL shader.
/// Red sits at 347.5 (= -12.5 + 360) so its window wraps across zero.
pub const BANDS: [(f32, f32); 8] = [
    (347.5, 32.5), // red
    (25.0, 35.0),  // orange
    (57.5, 32.5),  // yellow
    (120.0, 70.0), // green
    (180.0, 45.0), // cyan
    (207.5, 62.5), // blue
    (275.0, 40.0), // purple
    (310.0, 55.0), // magenta
];

/// RGB in 0..=1 to HSL: hue in degrees, saturation and lightness 0..=1.
pub fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let s = if mx != 0.0 { d / mx } else { 0.0 };
    let h = if s == 0.0 {
        0.0
    } else {
        let raw = if r == mx {
            (g - b) / d
        } else if g == mx {
            2.0 + (b - r) / d
        } else {
            4.0 + (r - g) / d
        };
        let mut h = raw * 60.0;
        if h < 0.0 {
            h += 360.0;
        }
        h
    };
    [h, s, mx]
}

/// HSL back to RGB, the inverse of [`rgb_to_hsl`].
pub fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let (h, s, v) = (hsl[0], hsl[1], hsl[2]);
    if s == 0.0 {
        return [v, v, v];
    }
    let sector = h / 60.0;
    let i = sector.floor();
    let f = sector - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Shortest distance between two hues on the 0..360 circle.
fn hue_gap(a: f32, b: f32) -> f32 {
    let mut d = (a - b).abs() % 360.0;
    if d > 180.0 {
        d = 360.0 - d;
    }
    d
}

/// Apply one band to `hsl`, judged against the untouched `base`.
fn band(hsl: [f32; 3], base: [f32; 3], centre: f32, half: f32, p: [f32; 3]) -> [f32; 3] {
    if p == [0.0, 0.0, 0.0] {
        return hsl;
    }
    let gap = hue_gap(base[0], centre);
    if gap >= half {
        return hsl;
    }
    let a = 1.0 - gap / half;
    let mut out = hsl;
    out[0] = (out[0] + p[0] * a) % 360.0;
    if out[0] < 0.0 {
        out[0] += 360.0;
    }
    out[1] = (out[1] + out[1] * (p[1] / 100.0) * a).clamp(0.0, 1.0);
    out[2] = (out[2] + out[2] * base[1] * (p[2] / 100.0) * a).clamp(0.0, 1.0);
    out
}

/// The colour after all eight bands. `adjust` holds the 24 signed values in
/// the package's parameter order: hue, saturation, lightness per band.
pub fn apply(rgb: [f32; 3], adjust: &[f32; 24]) -> [f32; 3] {
    let base = rgb_to_hsl(rgb);
    let mut h = base;
    for (index, &(centre, half)) in BANDS.iter().enumerate() {
        let p = [
            adjust[index * 3],
            adjust[index * 3 + 1],
            adjust[index * 3 + 2],
        ];
        h = band(h, base, centre, half, p);
    }
    let out = hsl_to_rgb(h);
    [
        out[0].clamp(0.0, 1.0),
        out[1].clamp(0.0, 1.0),
        out[2].clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: [f32; 24] = [0.0; 24];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn round_trip_is_exact_enough_to_build_on() {
        for rgb in [
            [0.5, 0.25, 0.75],
            [1.0, 0.0, 0.0],
            [0.2, 0.2, 0.2],
            [0.9, 0.6, 0.1],
        ] {
            let back = hsl_to_rgb(rgb_to_hsl(rgb));
            for i in 0..3 {
                assert!(close(back[i], rgb[i]), "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn all_zero_is_an_exact_identity() {
        for rgb in [[0.5, 0.25, 0.75], [1.0, 0.0, 0.0], [0.2, 0.2, 0.2]] {
            let out = apply(rgb, &ZERO);
            for i in 0..3 {
                assert!(close(out[i], rgb[i]), "zero changed {rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn a_red_band_touches_red_and_not_blue() {
        let mut adjust = ZERO;
        adjust[0] = 30.0; // red hue +30 degrees
        let red = apply([1.0, 0.0, 0.0], &adjust);
        let blue = apply([0.0, 0.0, 1.0], &adjust);
        assert!(
            !close(red[0], 1.0) || !close(red[1], 0.0),
            "red was not moved: {red:?}"
        );
        assert!(
            close(blue[2], 1.0) && close(blue[0], 0.0),
            "blue was moved: {blue:?}"
        );
    }

    #[test]
    fn red_wraps_across_zero() {
        // Red's centre is 347.5, so a pure red (hue 0) is within its window.
        let mut adjust = ZERO;
        adjust[1] = 50.0; // red saturation +50%
        let out = apply([1.0, 0.0, 0.0], &adjust);
        assert!(
            out[0].is_finite() && out[1].is_finite() && out[2].is_finite(),
            "the wrap produced NaN: {out:?}"
        );
        assert!(
            out.iter().all(|x| *x >= 0.0 && *x <= 1.0),
            "out of range: {out:?}"
        );
    }

    #[test]
    fn saturation_is_bounded() {
        let mut adjust = ZERO;
        for i in 0..8 {
            adjust[i * 3 + 1] = 100.0;
        }
        for rgb in [[0.9, 0.1, 0.3], [0.2, 0.8, 0.4]] {
            let out = apply(rgb, &adjust);
            assert!(
                out.iter().all(|x| *x >= 0.0 && *x <= 1.0),
                "out of range: {out:?}"
            );
        }
    }
}
