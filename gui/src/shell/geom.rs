//! Pixel geometry the widgets lay out on: window-space rects and the
//! uniform mapping of a layout authored at a fixed base size into a live
//! window.
//!
//! Render-agnostic; depends on plain integers only. The host picks the base
//! size and when to scale; its own layout (dialog templates, panels, anchors)
//! builds on these.

/// Pixel rect in window space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RectPx {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn translate(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
            w: self.w,
            h: self.h,
        }
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        // Widened: the far edge of a rect at the end of i32 is past it.
        x >= self.x
            && y >= self.y
            && i64::from(x) < i64::from(self.x) + i64::from(self.w)
            && i64::from(y) < i64::from(self.y) + i64::from(self.h)
    }

    /// Without a negative size, the far edges saturated so that `x + w` and
    /// `y + h` fit an i32.
    pub fn bounded(self) -> Self {
        Self::new(
            self.x,
            self.y,
            self.w.clamp(0, i32::MAX - self.x.max(0)),
            self.h.clamp(0, i32::MAX - self.y.max(0)),
        )
    }

    /// What both share; of zero size where they share nothing.
    pub fn intersect(self, other: Self) -> Self {
        let (x, y) = (self.x.max(other.x), self.y.max(other.y));
        let right = self.x.saturating_add(self.w).min(other.x.saturating_add(other.w));
        let bottom = self.y.saturating_add(self.h).min(other.y.saturating_add(other.h));
        Self::new(x, y, right.saturating_sub(x).max(0), bottom.saturating_sub(y).max(0))
    }
}

/// Corner-rounded uniform mapping from a layout authored at a base size
/// (`base_w`x`base_h`) into a live window: one factor on both axes, the
/// largest that fits the window, with the scaled layout centred and the spare
/// width or height left to the window's clear. Identity when the window is the
/// base size, so a fill-window layout equals the authored layout there.
///
/// Holds the base and the window it maps into, not the derived factor and
/// origin, so a layout carrying one stays `Eq` for the structs that compare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellScale {
    base_w: i32,
    base_h: i32,
    screen_w: i32,
    screen_h: i32,
    /// The scaled layout stands at the window's right edge instead of its
    /// centre ([`Self::anchored_right`]).
    right: bool,
}

impl ShellScale {
    /// The mapping that changes nothing and names no base: every rect maps to
    /// itself. What a [`crate::Gui`] uses until the host gives it a base.
    pub const IDENTITY: Self = Self::unscaled(1, 1);

    /// The unscaled mapping of a `base_w`x`base_h` layout: the host places the
    /// layout in the window itself, never scaled. Every art fit keyed on this
    /// scale is then a no-op.
    pub const fn unscaled(base_w: i32, base_h: i32) -> Self {
        Self::fit(base_w, base_h, base_w, base_h)
    }

    /// A `base_w`x`base_h` layout scaled to fit a `screen_w`x`screen_h`
    /// window at its own shape, centred.
    pub const fn fit(base_w: i32, base_h: i32, screen_w: i32, screen_h: i32) -> Self {
        Self {
            base_w,
            base_h,
            screen_w,
            screen_h,
            right: false,
        }
    }

    /// This mapping's base fitted into a `screen_w`x`screen_h` window instead,
    /// centred.
    pub const fn in_window(self, screen_w: i32, screen_h: i32) -> Self {
        Self::fit(self.base_w, self.base_h, screen_w, screen_h)
    }

    /// The size the layout is authored at.
    pub const fn base(self) -> (i32, i32) {
        (self.base_w, self.base_h)
    }

    /// The same mapping with the scaled layout at the window's right edge,
    /// the spare width all at its left, for a host page that spreads into it.
    pub fn anchored_right(self) -> Self {
        Self {
            right: true,
            ..self
        }
    }

    /// The same mapping with the scaled layout centred again.
    pub fn centred(self) -> Self {
        Self {
            right: false,
            ..self
        }
    }

    /// The one factor both axes scale by: the largest at which the base
    /// layout still fits the window. Glyphs, the software cursor and every
    /// rect use it, so the layout keeps its authored proportions.
    pub fn factor(self) -> f32 {
        let fit_w = self.screen_w as f32 / self.base_w as f32;
        let fit_h = self.screen_h as f32 / self.base_h as f32;
        fit_w.min(fit_h)
    }

    /// Where the scaled base layout sits in the window: centred, the spare
    /// width or height split between the two sides.
    pub fn dialog_rect(self) -> RectPx {
        let factor = self.factor();
        let w = (self.base_w as f32 * factor).round() as i32;
        let h = (self.base_h as f32 * factor).round() as i32;
        let spare = self.screen_w - w;
        let x = if self.right { spare } else { spare / 2 };
        RectPx::new(x, (self.screen_h - h) / 2, w, h)
    }

    /// The whole window this scale maps into. A layout's `screen` is this on
    /// both paths: prompts, the eject box and the input side centre in it.
    pub fn window(self) -> RectPx {
        RectPx::new(0, 0, self.screen_w, self.screen_h)
    }

    /// Where art drawn from the dialog's top-left corner sits in the window:
    /// `unscaled`, wherever the host's unscaled page puts it, on the unscaled
    /// mapping, and the scaled dialog's corner otherwise.
    pub fn dialog_origin_or(self, unscaled: (i32, i32)) -> (i32, i32) {
        if self.is_unscaled() {
            unscaled
        } else {
            let dialog = self.dialog_rect();
            (dialog.x, dialog.y)
        }
    }

    /// Is this the unscaled mapping — place the base layout in the window,
    /// never scale it? Callers that keep a native-size draw for the unscaled
    /// layout test this instead of inferring it from a rect.
    pub fn is_unscaled(self) -> bool {
        self.screen_w == self.base_w && self.screen_h == self.base_h
    }

    pub fn scale_rect(self, rect: RectPx) -> RectPx {
        let factor = self.factor();
        let origin = self.dialog_rect();
        let edge = |authored: i32| (authored as f32 * factor).round() as i32;
        let x0 = origin.x + edge(rect.x);
        let y0 = origin.y + edge(rect.y);
        let x1 = origin.x + edge(rect.x + rect.w);
        let y1 = origin.y + edge(rect.y + rect.h);
        RectPx::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    /// The window-space point an authored point came to sit on, for layers placed
    /// by origin rather than by rectangle. Identity on the unscaled mapping.
    pub fn scale_point(self, point: [i32; 2]) -> [i32; 2] {
        let rect = self.scale_rect(RectPx::new(point[0], point[1], 0, 0));
        [rect.x, rect.y]
    }

    /// Chrome art authored at `(w, h)` pixels in the base layout, in window
    /// space. Identity on the unscaled mapping.
    pub fn art_size(self, w: f32, h: f32) -> [f32; 2] {
        let factor = self.factor();
        [w * factor, h * factor]
    }

    /// The base-space point a scaled device point came from: the point
    /// form of [`Self::base_rect`]. A cursor mapped through it lets pointer
    /// arithmetic run on the authored pixels the paint path used.
    pub(crate) fn base_point(self, x: i32, y: i32) -> (i32, i32) {
        let origin = self.dialog_rect();
        let factor = self.factor();
        let back = |offset: i32| (offset as f32 / factor).round() as i32;
        (back(x - origin.x), back(y - origin.y))
    }

    /// The base-space rect a scaled device rect came from: the inverse of
    /// [`Self::scale_rect`]. Control geometry derived from a device rect maps
    /// through this first, so the insets and row counts run on the authored
    /// numbers the paint and input paths agree on.
    ///
    /// Each edge is mapped back on its own, as `scale_rect` rounded it: a device
    /// edge is within half a pixel of the authored edge times the factor, so
    /// from 1x up every edge, and therefore every authored size an exact-size
    /// art lookup keys on, comes back exactly. Dividing the rounded device
    /// width instead can miss by one below 2x.
    pub fn base_rect(self, rect: RectPx) -> RectPx {
        let (x0, y0) = self.base_point(rect.x, rect.y);
        let (x1, y1) = self.base_point(rect.x + rect.w, rect.y + rect.h);
        RectPx::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Derive a control's sub-rect from its device rect: map back to the
    /// authored numbers, apply `derive` there, then scale the result. Paint
    /// and hit-testing both go through this, so a scaled control's drawn art
    /// and its clickable area are the same rect. The unscaled mapping hands the
    /// rect to `derive` untouched, keeping every unscaled page byte-identical.
    pub fn derive_rect(self, rect: RectPx, derive: impl FnOnce(RectPx) -> RectPx) -> RectPx {
        if self.is_unscaled() {
            derive(rect)
        } else {
            self.scale_rect(derive(self.base_rect(rect)))
        }
    }
}

/// Every rect a layout owns, mapped onto the live window in one pass.
///
/// Each impl is an exhaustive struct literal, so a rect field added to a layout
/// is a compile error here until it is stretched too: a page cannot half-scale.
pub trait ScaledRects {
    fn scaled_rects(&self, scale: ShellScale) -> Self;
}

impl ScaledRects for RectPx {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        scale.scale_rect(*self)
    }
}

impl<T: ScaledRects, const N: usize> ScaledRects for [T; N] {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        std::array::from_fn(|index| self[index].scaled_rects(scale))
    }
}

impl<T: ScaledRects> ScaledRects for Vec<T> {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        self.iter().map(|value| value.scaled_rects(scale)).collect()
    }
}

impl<A: ScaledRects, B: ScaledRects> ScaledRects for (A, B) {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        (self.0.scaled_rects(scale), self.1.scaled_rects(scale))
    }
}

/// A key carried beside a rect (a resource id, a caption or art name) stays
/// as it is.
macro_rules! unscaled {
    ($($ty:ty),*) => {
        $(impl ScaledRects for $ty {
            fn scaled_rects(&self, _: ShellScale) -> Self {
                *self
            }
        })*
    };
}
unscaled!(u16, &'static str);

/// The window-filling variant of a layout authored at `base_w`x`base_h`:
/// `layout` laid out at the base size, then scaled to fit the
/// `screen_w`x`screen_h` window, keeping its shape. Identity at the base size.
pub fn fit_window<L: ScaledRects>(
    base_w: i32,
    base_h: i32,
    screen_w: u32,
    screen_h: u32,
    layout: impl FnOnce(u32, u32) -> L,
) -> L {
    let scale = ShellScale::fit(base_w, base_h, screen_w as i32, screen_h as i32);
    layout(base_w as u32, base_h as u32).scaled_rects(scale)
}

/// A layout on `scale`'s mapping into a `screen_w`x`screen_h` window: laid out
/// at the window's own size for the unscaled mapping, else laid out at the
/// mapping's base size and scaled.
pub fn layout_on<L: ScaledRects>(
    scale: ShellScale,
    screen_w: i32,
    screen_h: i32,
    layout: impl FnOnce(i32, i32) -> L,
) -> L {
    if scale.is_unscaled() {
        layout(screen_w, screen_h)
    } else {
        let (base_w, base_h) = scale.base();
        layout(base_w, base_h).scaled_rects(scale)
    }
}

/// Implements [`ScaledRects`] for a layout struct as one exhaustive struct
/// literal: every listed field scales, `keep` fields copy through, and the
/// leading `[scale, screen]` fields of a layout take the new mapping and the
/// whole window (`[scale]` for a layout without a window rect). A field missing from the list is a compile error.
#[macro_export]
macro_rules! scaled_rects {
    ($ty:ty { $([$scale:ident $(, $window:ident)?])? $($field:ident),* $(,)? $(; keep: $($keep:ident),* $(,)?)? }) => {
        impl $crate::shell::geom::ScaledRects for $ty {
            fn scaled_rects(&self, scale: $crate::shell::geom::ShellScale) -> Self {
                Self {
                    $($scale: scale, $($window: scale.window(),)?)?
                    $($field: $crate::shell::geom::ScaledRects::scaled_rects(&self.$field, scale),)*
                    $($($keep: ::core::clone::Clone::clone(&self.$keep),)*)?
                }
            }
        }
    };
}

impl<T: ScaledRects> ScaledRects for Option<T> {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        self.as_ref().map(|value| value.scaled_rects(scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layout authored at 800x600, the size these cases are worked out at.
    const BASE_W: i32 = 800;
    const BASE_H: i32 = 600;
    const UNSCALED: ShellScale = ShellScale::unscaled(BASE_W, BASE_H);

    fn fit(screen_w: i32, screen_h: i32) -> ShellScale {
        ShellScale::fit(BASE_W, BASE_H, screen_w, screen_h)
    }

    #[test]
    fn fill_window_scale_keeps_the_shell_shape_and_centres_it() {
        // 21:9: the height limits the factor to 2.4, and the 1920-wide shell is
        // centred with 760 px of window on either side.
        let wide = fit(3440, 1440);
        assert_eq!(wide.factor(), 2.4);
        assert_eq!(wide.dialog_rect(), RectPx::new(760, 0, 1920, 1440));
        assert_eq!(
            wide.scale_rect(RectPx::new(644, 199, 156, 42)),
            RectPx::new(2306, 478, 374, 100)
        );
        assert_eq!(wide.base_point(2306, 478), (644, 199));

        // A window taller than 4:3 centres the shell vertically instead.
        let tall = fit(1200, 1200);
        assert_eq!(tall.factor(), 1.5);
        assert_eq!(tall.dialog_rect(), RectPx::new(0, 150, 1200, 900));

        // A 4:3 window is filled exactly, and 800x600 is the identity.
        assert_eq!(fit(1600, 1200).dialog_rect(), RectPx::new(0, 0, 1600, 1200));
        assert_eq!(UNSCALED.dialog_rect(), RectPx::new(0, 0, BASE_W, BASE_H));
    }

    #[test]
    fn base_rect_recovers_every_authored_rect_from_1x_up() {
        // Exact-size art (the combo faces, the trackbar frames) is looked up by
        // the authored size read back from a device rect. Dividing the rounded
        // device width missed by one below 2x (480 of these at 1024x768).
        let windows = [
            (1024, 768),
            (1280, 720),
            (1366, 768),
            (1600, 600),
            (1920, 1080),
            (3440, 1440),
        ];
        for (w, h) in windows {
            let scale = fit(w, h);
            for x in 0..BASE_W {
                for width in [38, 44, 117, 150, 180, 207] {
                    let authored = RectPx::new(x, x % BASE_H, width, 23);
                    assert_eq!(
                        scale.base_rect(scale.scale_rect(authored)),
                        authored,
                        "{w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn derive_rect_runs_the_derivation_in_authored_pixels() {
        // A control sub-rect inset by 2 px on each axis, the shape every
        // paint/hit-test derivation in the shells uses.
        fn inset(rect: RectPx) -> RectPx {
            RectPx::new(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4)
        }

        // The unscaled mapping hands the rect to `derive` untouched, so the
        // default path keeps its exact bytes.
        let authored = RectPx::new(100, 200, 207, 23);
        assert_eq!(UNSCALED.derive_rect(authored, inset), inset(authored));

        // A stretched device rect round-trips to the authored numbers, so the 2 px
        // inset is 2 authored pixels (4 device) rather than 2 device pixels: the
        // paint and input paths can never disagree by a scale factor.
        let scale = fit(1600, 1200);
        let device = scale.scale_rect(authored);
        assert_eq!(scale.base_rect(device), authored);
        assert_eq!(
            scale.derive_rect(device, inset),
            scale.scale_rect(inset(authored))
        );
        assert_eq!(
            scale.derive_rect(device, inset),
            RectPx::new(204, 404, 406, 38)
        );
    }

    #[test]
    fn identity_maps_every_rect_to_itself() {
        let rect = RectPx::new(100, 200, 207, 23);
        assert!(ShellScale::IDENTITY.is_unscaled());
        assert_eq!(ShellScale::IDENTITY.factor(), 1.0);
        assert_eq!(ShellScale::IDENTITY.scale_rect(rect), rect);
        assert_eq!(ShellScale::IDENTITY.base_rect(rect), rect);
        assert_eq!(
            ShellScale::IDENTITY.derive_rect(rect, |r| r.translate(2, 2)),
            rect.translate(2, 2)
        );
    }
}
