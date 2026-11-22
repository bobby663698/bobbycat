// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Colour matching by histogram transfer: learn a per-channel curve from a
//! reference picture and apply it to the edit.
//!
//! Jianying's `color_match_select_target` keeps its actual transfer curve in
//! a closed `general_lens` node (`gpu_mode: 1`, a `colormatch_cache_path`);
//! only 28 lines of `Feature.js` ship, and they pass the reference image to
//! that private node. There is no shader to transcribe. What is here is the
//! standard algorithm the feature performs, then handed to [`concat.curves`]'
//! curve parameters for the GPU to apply.
//!
//! The match is the percentile transfer: for each channel, take the ref's
//! luminance-sorted pixels and the source's, and map a source level to the
//! reference level at the same percentile. This is distribution matching -
//! it carries the reference's tonal shape, not its colours pixel for pixel -
//! which is what a colourist means by "match the look of this shot".

/// Number of levels in a luma histogram. 256 for an 8-bit picture.
pub const LEVELS: usize = 256;

/// A histogram of one channel over the picture's pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    /// One count per 8-bit level, in channel order's own level space.
    pub bins: [u32; LEVELS],
    /// How many pixels were counted; zero for an empty picture.
    pub total: u64,
}

// Arrays only derive Default up to 32 elements on this toolchain.
impl Default for Histogram {
    fn default() -> Self {
        Self {
            bins: [0; LEVELS],
            total: 0,
        }
    }
}

impl Histogram {
    /// Count straight RGB pixels, weighting each by nothing (one pixel one
    /// vote). Levels are the 8-bit encoding, 0..=255.
    pub fn from_channel(pixels: &[[u8; 4]], channel: usize) -> Self {
        let mut bins = [0u32; LEVELS];
        for pixel in pixels {
            let level = pixel[channel] as usize;
            bins[level] = bins[level].saturating_add(1);
        }
        let total = bins.iter().map(|&n| u64::from(n)).sum();
        Self { bins, total }
    }

    /// The cumulative distribution, each bin in `0..=1`.
    pub fn cdf(&self) -> [f64; LEVELS] {
        let mut out = [0.0; LEVELS];
        if self.total == 0 {
            return out;
        }
        let mut running = 0u64;
        for (level, &count) in self.bins.iter().enumerate() {
            running += u64::from(count);
            out[level] = running as f64 / self.total as f64;
        }
        out
    }
}

/// Invert a cumulative distribution: the level at percentile `p` (0..=1).
/// Returns the first level whose cdf reaches `p`.
fn level_at(cdf: &[f64; LEVELS], p: f64) -> f64 {
    for (level, &value) in cdf.iter().enumerate() {
        if value >= p {
            return level as f64 / (LEVELS - 1) as f64;
        }
    }
    1.0
}

/// Learn the transfer curve from `source` to `reference`.
///
/// The result has [`LEVELS`] entries: index is a source level in 0..=255,
/// the value the level should become, also in 0..=255. At each percentile
/// the source's level is moved to the reference's level at that percentile.
pub fn transfer(source: &Histogram, reference: &Histogram) -> Vec<f64> {
    let source_cdf = source.cdf();
    let reference_cdf = reference.cdf();
    (0..LEVELS)
        .map(|level| {
            // The percentile of a level is its bin's upper edge on the
            // source's cumulative curve. Looking at the previous bin's edge
            // would push sparse low levels into the reference's zero bin.
            let p = source_cdf[level];
            level_at(&reference_cdf, p) * (LEVELS - 1) as f64
        })
        .collect()
}

/// Apply a learned transfer to one level, with `strength` in 0..=1 blending
/// from the identity (0) to the full match (1).
pub fn apply_level(curve: &[f64], level: u8, strength: f64) -> u8 {
    let s = strength.clamp(0.0, 1.0);
    let moved = curve[level as usize];
    let out = f64::from(level) * (1.0 - s) + moved * s;
    out.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_levels(levels: &[u8]) -> [[u8; 4]; 4] {
        let mut out = [[0u8; 4]; 4];
        for (i, &level) in levels.iter().take(4).enumerate() {
            out[i] = [level, level, level, 255];
        }
        out
    }

    #[test]
    fn a_histogram_counts_its_channel_and_totals() {
        let px = gray_levels(&[10, 10, 200, 255]);
        let h = Histogram::from_channel(&px, 0);
        assert_eq!(h.total, 4);
        assert_eq!(h.bins[10], 2);
        assert_eq!(h.bins[200], 1);
        assert_eq!(h.bins[255], 1);
    }

    #[test]
    fn an_empty_picture_has_a_zero_cdf() {
        let h = Histogram::default();
        assert!(h.cdf().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn matching_a_picture_to_itself_is_the_identity() {
        let px = gray_levels(&[10, 80, 160, 240]);
        let h = Histogram::from_channel(&px, 0);
        let curve = transfer(&h, &h);
        for &level in &[10u8, 80, 160, 240] {
            let got = apply_level(&curve, level, 1.0);
            // The transfer is quantile-based; at the four sample levels it
            // lands within a step of where they started.
            assert!(
                (i32::from(got) - i32::from(level)).abs() <= 2,
                "{level} -> {got}"
            );
        }
    }

    #[test]
    fn strength_zero_is_an_exact_identity() {
        let px = gray_levels(&[10, 80, 160, 240]);
        let src = Histogram::from_channel(&px, 0);
        let dark = gray_levels(&[40, 60, 80, 100]);
        let ref_h = Histogram::from_channel(&dark, 0);
        let curve = transfer(&src, &ref_h);
        for level in 0..=255u8 {
            assert_eq!(apply_level(&curve, level, 0.0), level);
        }
    }

    #[test]
    fn a_brighter_reference_raises_the_source() {
        let src_px = gray_levels(&[20, 40, 60, 80]);
        let ref_px = gray_levels(&[120, 140, 160, 180]);
        let src = Histogram::from_channel(&src_px, 0);
        let reference = Histogram::from_channel(&ref_px, 0);
        let curve = transfer(&src, &reference);
        let moved = apply_level(&curve, 40, 1.0);
        assert!(
            moved > 100,
            "a dark source should land in the bright reference: {moved}"
        );
    }

    #[test]
    fn curve_is_monotone() {
        let src_px = gray_levels(&[10, 90, 170, 250]);
        let ref_px = gray_levels(&[30, 60, 200, 230]);
        let src = Histogram::from_channel(&src_px, 0);
        let reference = Histogram::from_channel(&ref_px, 0);
        let curve = transfer(&src, &reference);
        for i in 1..curve.len() {
            assert!(curve[i] + 1e-9 >= curve[i - 1], "not monotone at {i}");
        }
    }
}
