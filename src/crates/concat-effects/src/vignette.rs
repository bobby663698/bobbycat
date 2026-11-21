// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The CPU twin of the built-in `concat.vignette` effect package.
//!
//! The shader in `packages/concat.vignette/effect.wgsl` draws on the GPU and
//! is pinned by a probe at the middle of the layer: a one-colour probe fills
//! an 8x8 picture and reads back its centre, so it cannot see the corners.
//! These are the same lines of arithmetic in plain Rust, so the things a
//! probe cannot express - the middle untouched at every setting, the
//! corners darkened, and zero strength an exact identity - are checked
//! without a device. Keep this file and the shader in step.

/// The multiplier the vignette applies at `uv`.
///
/// `u` and `v` are the layer's coordinates, 0 at one edge and 1 at the
/// opposite one. `strength`, `radius` and `softness` are the package's three
/// percentages, in the order its `Params` declares them. The result is
/// 1 at the middle of the layer and smaller towards the corners, never
/// negative.
pub fn attenuation(u: f32, v: f32, strength: f32, radius: f32, softness: f32) -> f32 {
    let d = (((u - 0.5) * 2.0).powi(2) + ((v - 0.5) * 2.0).powi(2)).sqrt();
    let r = 0.35 + radius / 100.0 * 0.75;
    let w = 0.05 + softness / 100.0 * 0.6;
    let t = ((d - (r - w * 0.5)) / w).clamp(0.0, 1.0);
    let fall = t * t * (3.0 - 2.0 * t);
    let s = (strength / 100.0).clamp(0.0, 1.0);
    (1.0 - fall * s).powf(2.4)
}

/// The colour at `uv` after the vignette, straight alpha left alone.
pub fn apply(rgb: [f32; 3], u: f32, v: f32, strength: f32, radius: f32, softness: f32) -> [f32; 3] {
    let a = attenuation(u, v, strength, radius, softness);
    [rgb[0] * a, rgb[1] * a, rgb[2] * a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_pixel_is_never_darkened() {
        // The fall's inner edge is at least 0.35 - 0.025 > 0, so the middle
        // of the layer is outside it for every setting, even the hardest
        // fall (radius 0, softness 0) at full strength.
        for strength in [0.0, 1.0, 50.0, 100.0] {
            for radius in [0.0, 50.0, 100.0] {
                for softness in [0.0, 50.0, 100.0] {
                    let a = attenuation(0.5, 0.5, strength, radius, softness);
                    assert_eq!(a, 1.0, "centre at {strength}/{radius}/{softness}: {a}");
                }
            }
        }
    }

    #[test]
    fn four_corners_are_darkened() {
        // At the defaults a corner is well past the outer edge (0.9), so
        // the fall is 1 and the multiplier is 0.5^2.4, some 0.19. Distance
        // to the middle is all the shader reads, so all four agree.
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let a = attenuation(u, v, 50.0, 50.0, 50.0);
            assert!(a < 0.25, "corner ({u}, {v}) too bright: {a}");
            assert!(a > 0.0, "corner ({u}, {v}) went to black: {a}");
        }
    }

    #[test]
    fn zero_strength_matches_the_source_pixel_for_pixel() {
        // A small image, every pixel put through the CPU twin at zero
        // strength: none of them may change, at any radius or softness.
        let image = [
            [[0.0, 0.0, 0.0], [1.0, 0.5, 0.25], [4.0, 2.0, 1.0]],
            [[0.18, 0.18, 0.18], [0.5, 0.2, 0.1], [0.01, 0.02, 0.03]],
            [[0.36, 0.09, 0.18], [0.7, 0.4, 0.2], [0.9, 0.8, 0.7]],
        ];
        let (height, width) = (image.len(), image[0].len());
        for (radius, softness) in [(0.0, 0.0), (50.0, 50.0), (100.0, 100.0)] {
            for (y, row) in image.iter().enumerate() {
                for (x, source) in row.iter().enumerate() {
                    let u = (x as f32 + 0.5) / width as f32;
                    let v = (y as f32 + 0.5) / height as f32;
                    let got = apply(*source, u, v, 0.0, radius, softness);
                    assert_eq!(got, *source, "pixel ({x}, {y}) at {radius}/{softness}");
                }
            }
        }
    }
}
