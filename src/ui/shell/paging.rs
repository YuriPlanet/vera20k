//! Local paging for fixed-size dialog rows. The roster and ordering stay with
//! each screen's existing model; this owner stores only a page index.

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use super::geom::{RectPx, ShellAlign};

/// VERA's Previous and Next page buttons, and the "page n of m" static over
/// them, in the in-game dialogs that page their rows (0x73, 0xEA).
pub const PREVIOUS: u16 = 0xF201;
pub const NEXT: u16 = 0xF202;
pub const COUNTER: u16 = 0xF20F;
pub const PREVIOUS_LABEL: &str = "VERA:Previous";
pub const NEXT_LABEL: &str = "VERA:Next";

/// The page counter: a static centred over the Previous button.
pub fn counter_node() -> NodeSpec {
    NodeSpec::new(NodeKind::Text)
        .id(COUNTER.into())
        .with(Prop::Align, Value::Int(i32::from(ShellAlign::H_CENTER.0)))
}

/// The counter's rect, 24 px above the Previous button of `buttons`
/// ([`button_rects`]) and 20 px tall.
pub fn counter_rect(buttons: [RectPx; 2]) -> RectPx {
    let first = buttons[0];
    RectPx::new(first.x, first.y - 24, first.w, 20)
}

/// The counter's text for page `index` (from 0) of `pages`.
pub fn counter_text(index: usize, pages: usize) -> String {
    let page = index + 1;
    crate::i18n::tr_fill("VERA:PageOf", &[("page", &page), ("pages", &pages)])
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowPage {
    page: usize,
}

impl RowPage {
    pub fn count(rows: usize, page_size: usize) -> usize {
        rows.div_ceil(page_size).max(1)
    }

    pub fn index(&self, rows: usize, page_size: usize) -> usize {
        self.page.min(Self::count(rows, page_size) - 1)
    }

    pub fn range(&self, rows: usize, page_size: usize) -> std::ops::Range<usize> {
        let start = self.index(rows, page_size) * page_size;
        start..(start + page_size).min(rows)
    }

    pub fn change(&mut self, next: bool, rows: usize, page_size: usize) -> bool {
        let current = self.index(rows, page_size);
        let target = if next {
            current
                .saturating_add(1)
                .min(Self::count(rows, page_size) - 1)
        } else {
            current.saturating_sub(1)
        };
        self.page = target;
        target != current
    }
}

/// VERA's Previous/Next buttons occupy unused right-column space above the
/// native Return/Continue button. Native dialogs with eight rows omit them.
pub fn button_rects(anchor: RectPx) -> [RectPx; 2] {
    let pitch = anchor.h + 8;
    [
        RectPx::new(anchor.x, anchor.y - 2 * pitch, anchor.w, anchor.h),
        RectPx::new(anchor.x, anchor.y - pitch, anchor.w, anchor.h),
    ]
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    #[test]
    fn all_registry_rows_are_reachable_without_rebinding_visible_slots() {
        let mut page = RowPage::default();
        let mut seen = Vec::new();
        loop {
            seen.extend(page.range(32, 8));
            if !page.change(true, 32, 8) {
                break;
            }
        }
        assert_eq!(seen, (0..32).collect::<Vec<_>>());
        assert_eq!(page.range(32, 8), 24..32);
        assert!(!page.change(true, 32, 8));
        while page.change(false, 32, 8) {}
        assert_eq!(page.range(8, 8), 0..8);
        assert_eq!(RowPage::count(0, 8), 1);
    }
}
