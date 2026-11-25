// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! Drawing a target box on a source frame, in pixels.
//!
//! A tracker has to be told what to follow, and the only honest way to say
//! it is to let a person draw: show the frame the clip actually presents -
//! its crop, its speed, its colour, nothing from the tracks above - and take
//! the box they drag on it. Everything here is that drag's arithmetic, kept
//! apart from the window so it can be tested without one.
//!
//! Three steps, in this order, because each answers a different question:
//!
//! - [`Viewport::fit`] - where the picture sits inside the view that shows
//!   it. A picture is letterboxed to fit, so a pointer over the bars is not
//!   over the picture.
//! - [`Viewport::to_pixel`] - which source pixel a pointer is over, or none
//!   when it is over a bar. [`Viewport::clamp_to_pixel`] is the same for a
//!   drag already under way, where the pointer is allowed past the edge and
//!   the edge is what it means.
//! - [`Roi::of_drag`] - the box between two pixel points. Rounded outwards
//!   so a box drawn small still covers the pixels it was drawn over.
//!
//! A completed box is stored in source pixels, never in view coordinates:
//! the window may then be resized, and the box still means the same pixels.

/// A point in a picture: pixels from its top-left corner, downwards.
///
/// `f64` because it starts as a pointer position and is rounded once, at
/// [`Roi::of_drag`], rather than lost to integer truncation on the way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Pixels across.
    pub x: f64,
    /// Pixels down.
    pub y: f64,
}

impl Point {
    /// A point at `x` pixels across and `y` down.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// The largest picture a box may be drawn on: `u32` pixels a side, so the
/// arithmetic below cannot overflow a side's worth of pixels when both
/// corners are dragged to opposite edges.
///
/// Eight times [`crate::frame::MAX_SIDE`], which is what a frame may be
/// today; room to grow without a box silently wrapping.
const MAX_PIXELS: f64 = (crate::frame::MAX_SIDE as f64) * 8.0;

/// Where a picture sits inside the view that shows it.
///
/// The picture is scaled down or up by one factor that fits it whole and
/// centred, so the bars - if the view is not the picture's shape - are equal
/// on the two sides the picture does not fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Pixels of view to the left of the picture.
    pub left: f64,
    /// Pixels of view above it.
    pub top: f64,
    /// View pixels a source pixel is drawn as.
    pub scale: f64,
    /// The picture's width in source pixels.
    pub width: u32,
    /// The picture's height in source pixels.
    pub height: u32,
}

impl Viewport {
    /// The picture `width` by `height` fitted into a view `view_width` by
    /// `view_height`. None when either side of either is not a positive
    /// number: a view that has not been laid out yet, or a picture that is
    /// not one.
    pub fn fit(width: u32, height: u32, view_width: f64, view_height: f64) -> Option<Viewport> {
        let (width_f, height_f) = (width as f64, height as f64);
        let usable = |view: f64, side: f64| view.is_finite() && view > 0.0 && side > 0.0;
        if !usable(view_width, width_f) || !usable(view_height, height_f) {
            return None;
        }
        if width_f > MAX_PIXELS || height_f > MAX_PIXELS {
            return None;
        }
        let scale = (view_width / width_f).min(view_height / height_f);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        Some(Viewport {
            left: (view_width - width_f * scale) / 2.0,
            top: (view_height - height_f * scale) / 2.0,
            scale,
            width,
            height,
        })
    }

    /// The source pixel under a pointer at (`x`, `y`) in view pixels.
    ///
    /// None over a bar, and none for a pointer that is not a number: a drag
    /// that starts there has not started on the picture, and starting one
    /// would put the box at an edge the person never pointed at.
    pub fn to_pixel(&self, x: f64, y: f64) -> Option<Point> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let point = Point::new((x - self.left) / self.scale, (y - self.top) / self.scale);
        let inside = point.x >= 0.0
            && point.y >= 0.0
            && point.x < self.width as f64
            && point.y < self.height as f64;
        inside.then_some(point)
    }

    /// The same, for a pointer that has left the picture while a drag is
    /// under way: the nearest edge's pixel, because that is what the person
    /// meant. The far edge is the picture's size itself, so a box dragged
    /// past it ends on the last pixel rather than inside it.
    pub fn clamp_to_pixel(&self, x: f64, y: f64) -> Point {
        if !x.is_finite() || !y.is_finite() {
            // A pointer that is not a number carries no meaning; keep the
            // box where it was rather than jump it to a corner.
            return Point::new(0.0, 0.0);
        }
        Point::new(
            ((x - self.left) / self.scale).clamp(0.0, self.width as f64),
            ((y - self.top) / self.scale).clamp(0.0, self.height as f64),
        )
    }

    /// Where a source pixel is in the view: the inverse of
    /// [`Viewport::to_pixel`], for drawing the box back over the picture.
    pub fn to_view(&self, point: Point) -> Point {
        Point::new(
            self.left + point.x * self.scale,
            self.top + point.y * self.scale,
        )
    }
}

/// A target box, in source pixels: its top-left corner and its size.
///
/// Whole pixels on purpose. The tracker compares one box against the next,
/// and a corner that wandered by a thousandth of a pixel each frame would
/// look like motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Roi {
    /// Pixels from the picture's left edge.
    pub x: u32,
    /// Pixels from its top edge.
    pub y: u32,
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl Roi {
    /// The smallest box a person may confirm, each way.
    ///
    /// Eight, because a smaller one holds no pattern to follow: a tracker
    /// handed a three-pixel box would lock onto whatever noise it found
    /// first and report confident nonsense for the rest of the clip. The
    /// caller says so in the window rather than accepting it silently.
    pub const MIN_SIDE: u32 = 8;

    /// The box between two corners in source pixels, each corner rounded
    /// outwards to the whole pixel it is over.
    ///
    /// Outwards - `floor` on the near edges, `ceil` on the far ones - so the
    /// box covers every pixel the drag touched. A drag may go in any
    /// direction: dragging up and to the left is the same box as dragging
    /// down and to the right between the same two corners.
    ///
    /// Both corners are held inside the picture first, so a drag that ends
    /// outside - which happens whenever a pointer is released past the
    /// window - ends on the edge instead of making a box of nothing.
    pub fn of_drag(start: Point, end: Point, width: u32, height: u32) -> Roi {
        let held = |value: f64, limit: u32| value.clamp(0.0, limit as f64);
        let (near_x, far_x) = (
            held(start.x.min(end.x), width),
            held(start.x.max(end.x), width),
        );
        let (near_y, far_y) = (
            held(start.y.min(end.y), height),
            held(start.y.max(end.y), height),
        );
        let left = near_x.floor();
        let top = near_y.floor();
        let right = far_x.ceil();
        let bottom = far_y.ceil();
        Roi {
            x: left as u32,
            y: top as u32,
            width: (right - left).max(0.0) as u32,
            height: (bottom - top).max(0.0) as u32,
        }
    }

    /// Whether a person may confirm this box: both sides at least
    /// [`Roi::MIN_SIDE`].
    pub fn is_confirmable(&self) -> bool {
        self.width >= Self::MIN_SIDE && self.height >= Self::MIN_SIDE
    }

    /// The box's centre, in pixels.
    pub fn center(&self) -> Point {
        Point::new(
            self.x as f64 + self.width as f64 / 2.0,
            self.y as f64 + self.height as f64 / 2.0,
        )
    }

    /// Whether this box has any area at all: a drag that moved no pixel.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The same box moved so its top-left corner is (`x`, `y`), which is
    /// what a tracker writes each frame while the target travels.
    pub fn moved_to(&self, x: u32, y: u32) -> Roi {
        Roi { x, y, ..*self }
    }

    /// The box as a mask's numbers, which is how a tracked path is kept in
    /// a project: a mask's centre is a fraction of the picture's width and
    /// height from its middle, its box a fraction of the picture's shorter
    /// side.
    ///
    /// None for a picture with no area. The units are a mask's - the
    /// project's `x`, `y`, `width` and `height` - so the values go straight
    /// into one and it draws where the box was, whatever size the picture
    /// is decoded at.
    pub fn fractions(&self, picture_width: u32, picture_height: u32) -> Option<RoiFractions> {
        if picture_width == 0 || picture_height == 0 {
            return None;
        }
        let (picture_w, picture_h) = (picture_width as f64, picture_height as f64);
        let shorter = picture_w.min(picture_h);
        let centre = self.center();
        Some(RoiFractions {
            x: (centre.x - picture_w / 2.0) / picture_w,
            y: (centre.y - picture_h / 2.0) / picture_h,
            width: self.width as f64 / shorter,
            height: self.height as f64 / shorter,
        })
    }
}

/// A box as a mask's numbers: see [`Roi::fractions`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RoiFractions {
    /// The centre across, as a fraction of the picture's width from its
    /// middle.
    pub x: f64,
    /// The same down its height.
    pub y: f64,
    /// The box's width, as a fraction of the picture's shorter side.
    pub width: f64,
    /// The box's height, the same.
    pub height: f64,
}

impl RoiFractions {
    /// The box back in pixels of a `width` by `height` picture: what a
    /// project holding these numbers draws.
    pub fn to_roi(self, picture_width: u32, picture_height: u32) -> Option<Roi> {
        if picture_width == 0 || picture_height == 0 {
            return None;
        }
        let (picture_w, picture_h) = (picture_width as f64, picture_height as f64);
        let shorter = picture_w.min(picture_h);
        let (box_w, box_h) = (self.width * shorter, self.height * shorter);
        let centre = Point::new(
            picture_w / 2.0 + self.x * picture_w,
            picture_h / 2.0 + self.y * picture_h,
        );
        let left = (centre.x - box_w / 2.0).clamp(0.0, picture_w);
        let top = (centre.y - box_h / 2.0).clamp(0.0, picture_h);
        Some(Roi {
            x: left.round() as u32,
            y: top.round() as u32,
            width: box_w.round().clamp(0.0, picture_w) as u32,
            height: box_h.round().clamp(0.0, picture_h) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped frame's most common shape: 640 by 360 in a view twice
    /// as tall as it needs.
    fn landscape() -> Viewport {
        Viewport::fit(640, 360, 1000.0, 700.0).expect("a view")
    }

    #[test]
    fn a_picture_is_fitted_whole_and_centred() {
        let view = landscape();
        assert_eq!(view.scale, 1.5625);
        assert_eq!(view.left, 0.0);
        // (700 - 360 * 1.5625) / 2
        assert_eq!(view.top, 68.75);
    }

    #[test]
    fn a_taller_picture_is_letterboxed_on_the_sides() {
        let view = Viewport::fit(360, 640, 1000.25, 700.5).expect("a view");
        assert!(view.left > 0.0, "{view:?}");
        assert_eq!(view.top, 0.0);
        assert!(view.left.is_finite() && view.top.is_finite());
    }

    #[test]
    fn a_pointer_over_a_bar_is_not_over_the_picture() {
        let view = landscape();
        // Above the picture, inside the top bar.
        assert_eq!(view.to_pixel(30.0, 20.0), None);
        // Inside it.
        assert_eq!(view.to_pixel(0.0, 68.75), Some(Point::new(0.0, 0.0)));
        // Past its right edge by a hair.
        assert_eq!(view.to_pixel(1000.0, 300.0), None);
    }

    #[test]
    fn a_pointer_that_is_not_a_number_is_not_a_pixel() {
        let view = landscape();
        assert_eq!(view.to_pixel(f64::NAN, 100.0), None);
        assert_eq!(view.to_pixel(100.0, f64::INFINITY), None);
    }

    #[test]
    fn a_drag_under_way_clamps_to_the_picture() {
        let view = landscape();
        assert_eq!(view.clamp_to_pixel(-10.0, 1000.0), Point::new(0.0, 360.0));
    }

    #[test]
    fn a_view_that_is_not_one_yields_nothing() {
        assert!(Viewport::fit(0, 360, 1000.0, 700.0).is_none());
        assert!(Viewport::fit(640, 360, 0.0, 700.0).is_none());
        assert!(Viewport::fit(640, 360, f64::NAN, 700.0).is_none());
        assert!(Viewport::fit(640, 360, f64::INFINITY, 700.0).is_none());
    }

    #[test]
    fn a_reverse_drag_is_the_same_box() {
        // 90.1,100.2 back to 10.2,20.7: floors the near edges, ceils the far.
        let target = Roi::of_drag(Point::new(90.1, 100.2), Point::new(10.2, 20.7), 640, 360);
        assert_eq!(
            target,
            Roi {
                x: 10,
                y: 20,
                width: 81,
                height: 81
            }
        );

        let forward = Roi::of_drag(Point::new(10.2, 20.7), Point::new(90.1, 100.2), 640, 360);
        assert_eq!(forward, target);
    }

    #[test]
    fn a_drag_past_the_edge_ends_on_it() {
        let target = Roi::of_drag(Point::new(30.0, 40.0), Point::new(-200.0, 1000.0), 640, 360);
        assert_eq!(
            target,
            Roi {
                x: 0,
                y: 40,
                width: 30,
                height: 320
            }
        );
    }

    #[test]
    fn a_drag_to_the_far_corner_covers_the_last_pixel() {
        let target = Roi::of_drag(Point::new(10.0, 10.0), Point::new(640.0, 360.0), 640, 360);
        assert_eq!(
            target,
            Roi {
                x: 10,
                y: 10,
                width: 630,
                height: 350
            }
        );
    }

    #[test]
    fn a_box_too_small_to_hold_a_pattern_cannot_be_confirmed() {
        let target = Roi::of_drag(Point::new(30.0, 40.0), Point::new(33.0, 43.0), 640, 360);
        assert_eq!((target.width, target.height), (3, 3));
        assert!(!target.is_confirmable());

        let smallest = Roi {
            x: 0,
            y: 0,
            width: Roi::MIN_SIDE,
            height: Roi::MIN_SIDE,
        };
        assert!(smallest.is_confirmable());
        assert!(
            !Roi {
                width: Roi::MIN_SIDE - 1,
                ..smallest
            }
            .is_confirmable()
        );
        assert!(
            !Roi {
                height: Roi::MIN_SIDE - 1,
                ..smallest
            }
            .is_confirmable()
        );
    }

    #[test]
    fn a_drag_that_moved_nothing_is_empty() {
        let target = Roi::of_drag(Point::new(40.0, 40.0), Point::new(40.0, 40.0), 640, 360);
        assert!(target.is_empty());
        assert!(!target.is_confirmable());
    }

    #[test]
    fn a_box_maps_back_to_where_it_was_drawn() {
        let view = landscape();
        let box_at = view.to_view(Point::new(10.0, 20.0));
        assert_eq!(box_at, Point::new(15.625, 100.0));
        assert_eq!(
            view.to_pixel(box_at.x, box_at.y),
            Some(Point::new(10.0, 20.0))
        );
    }

    #[test]
    fn a_box_knows_its_centre_and_moves_without_changing_size() {
        let target = Roi {
            x: 10,
            y: 20,
            width: 81,
            height: 81,
        };
        assert_eq!(target.center(), Point::new(50.5, 60.5));
        let moved = target.moved_to(100, 200);
        assert_eq!(
            moved,
            Roi {
                x: 100,
                y: 200,
                width: 81,
                height: 81
            }
        );
    }

    #[test]
    fn a_box_becomes_a_masks_numbers_and_comes_back() {
        let target = Roi {
            x: 100,
            y: 80,
            width: 60,
            height: 60,
        };
        let numbers = target.fractions(640, 360).expect("a picture");
        // Centre at (130, 110), the picture's at (320, 180).
        assert!((numbers.x - (-190.0 / 640.0)).abs() < 1e-12, "{numbers:?}");
        assert!((numbers.y - (-70.0 / 360.0)).abs() < 1e-12, "{numbers:?}");
        // A box's size is a fraction of the shorter side: 360.
        assert!((numbers.width - 60.0 / 360.0).abs() < 1e-12, "{numbers:?}");
        assert!((numbers.height - 60.0 / 360.0).abs() < 1e-12, "{numbers:?}");
        assert_eq!(numbers.to_roi(640, 360), Some(target));
    }

    #[test]
    fn a_masks_numbers_survive_a_different_decode_size() {
        // The same box on the same picture at half the size: a mask does
        // not care how big the frames were that found it.
        let full = Roi {
            x: 200,
            y: 160,
            width: 120,
            height: 120,
        };
        let numbers = full.fractions(1280, 720).expect("a picture");
        assert_eq!(
            numbers.to_roi(640, 360),
            Some(Roi {
                x: 100,
                y: 80,
                width: 60,
                height: 60
            })
        );
    }

    #[test]
    fn a_picture_with_no_area_has_no_numbers() {
        let target = Roi {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        assert_eq!(target.fractions(0, 360), None);
        assert_eq!(target.fractions(640, 0), None);
        assert_eq!(RoiFractions::default().to_roi(0, 0), None);
    }
}
