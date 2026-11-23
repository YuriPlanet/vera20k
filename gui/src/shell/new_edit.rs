//! The native single-line `NewEdit` (window proc `0x00614B30`, paint
//! `NewEdit__DrawTextAndCaret` `0x00623880`): the shell's owner-draw
//! replacement for the resource Edit control. It edits UTF-16 units with a
//! caret only (no selection range, no mouse caret placement). Every
//! out-of-game screen's edit field runs on it; the host measures and paints.

/// NewEdit614B30's UTF-16 editor.
#[derive(Debug, Clone, Default)]
pub struct NewEdit {
    pub units: Vec<u16>,
    pub caret: usize,
    pub first_visible_unit: usize,
    pub focused: bool,
    /// `ES_PASSWORD`: the paint passes `(style >> 5) & 1` to
    /// `NewEdit__DrawTextAndCaret` `0x00623880`, which draws every unit as
    /// `*` (`0x00614FDC..0x00615009`).
    pub password: bool,
}

impl NewEdit {
    /// NewEdit614B30's shared assignment: limits count UTF-16 units, and zero
    /// means unlimited. D7 uses five/eleven units; its port has no limit.
    pub fn set_units_limited(&mut self, units: impl IntoIterator<Item = u16>, limit: usize) {
        self.units = units
            .into_iter()
            .take(if limit == 0 { usize::MAX } else { limit })
            .collect();
        self.caret = self.units.len();
        self.first_visible_unit = 0;
    }

    /// NewEdit614B30 at615226..61527A: reject-character table, UTF-16
    /// limit, control-character gate and optional byte-only input (<0x100).
    /// One unit lets callers run their native EN_CHANGE handler after each
    /// admitted WM_CHAR instead of applying that handler after a pasted batch.
    pub(crate) fn insert_unit_filtered(
        &mut self,
        unit: u16,
        limit: usize,
        byte_only: bool,
        rejected: &[u16],
    ) -> bool {
        if unit <= 0x1f
            || (byte_only && unit >= 0x100)
            || rejected.contains(&unit)
            || (limit != 0 && self.units.len() >= limit)
        {
            return false;
        }
        self.units.insert(self.caret, unit);
        self.caret += 1;
        true
    }

    /// NewEdit multiline Enter614DEB appends CRLF to the end, independently
    /// of the caret and EM_LIMITTEXT, before notifying its parent with501.
    pub fn append_enter_line_end(&mut self) {
        self.units.extend([13, 10]);
    }

    /// NewEdit4B3 getter614D49: caller capacity includes its forced final NUL.
    pub fn wide_text(&self, capacity: usize) -> Vec<u16> {
        self.units
            .iter()
            .copied()
            .take(capacity.saturating_sub(1))
            .take_while(|unit| *unit != 0)
            .collect()
    }

    /// NewEdit message 0x4B5 (614D28): narrow each UTF-16 unit to its low byte,
    /// then terminate the caller's buffer at capacity-1. This is not ACP: each
    /// byte widens back to the scalar of the same value.
    pub fn narrow_text(&self, capacity: usize) -> String {
        self.units
            .iter()
            .take(capacity.saturating_sub(1))
            .map(|unit| *unit as u8)
            .take_while(|byte| *byte != 0)
            .map(char::from)
            .collect()
    }

    /// Removes the unit before the caret; a surrogate pair goes whole, so
    /// a character outside the BMP never leaves half of itself behind.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let pair = self.caret >= 2
            && (0xDC00..0xE000).contains(&self.units[self.caret - 1])
            && (0xD800..0xDC00).contains(&self.units[self.caret - 2]);
        let start = self.caret - if pair { 2 } else { 1 };
        self.units.drain(start..self.caret);
        self.caret = start;
    }

    pub fn delete(&mut self) {
        if self.caret < self.units.len() {
            self.units.remove(self.caret);
        }
    }

    pub fn left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.caret = (self.caret + 1).min(self.units.len());
    }
    pub fn home(&mut self) {
        self.caret = 0;
    }
    pub fn end(&mut self) {
        self.caret = self.units.len();
    }

    pub fn display_text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    /// The units `start..end` as the NewEdit paints them (masked for
    /// `ES_PASSWORD`).
    pub fn painted_range(&self, start: usize, end: usize) -> String {
        let end = end.min(self.units.len());
        let start = start.min(end);
        if self.password {
            "*".repeat(end - start)
        } else {
            String::from_utf16_lossy(&self.units[start..end])
        }
    }

    /// The painted text from the first visible unit on.
    pub fn painted_text(&self) -> String {
        self.painted_range(self.first_visible_unit, self.units.len())
    }

    /// NewEdit__DrawTextAndCaret `0x00623880` with an open IME composition
    /// (`0x006238D0..0x006239C2`): the composition units are spliced in at the
    /// caret, the painted caret sits at the composition cursor and the scroll
    /// keeps that position visible. Returns the painted editor and the
    /// composition's unit range in it.
    pub fn with_composition(
        &self,
        composition: &[u16],
        cursor: usize,
        available: u32,
        measure: impl Fn(&str) -> u32,
    ) -> (Self, std::ops::Range<usize>) {
        let caret = self.caret.min(self.units.len());
        let mut view = self.clone();
        view.units.splice(caret..caret, composition.iter().copied());
        view.caret = caret + cursor.min(composition.len());
        view.scroll_to_caret(available, measure);
        (view, caret..caret + composition.len())
    }

    /// Keep the insertion point visible using the active font's measured width.
    pub(crate) fn scroll_to_caret(&mut self, available: u32, measure: impl Fn(&str) -> u32) {
        self.first_visible_unit = self.first_visible_unit.min(self.caret);
        while self.first_visible_unit < self.caret {
            let prefix = self.painted_range(self.first_visible_unit, self.caret);
            if measure(&prefix) <= available {
                break;
            }
            self.first_visible_unit += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_composition_splices_at_the_caret_and_scrolls_to_its_cursor() {
        let mut edit = NewEdit::default();
        for unit in "abcd".encode_utf16() {
            edit.insert_unit_filtered(unit, 0, false, &[]);
        }
        edit.left();
        edit.left();
        let composition: Vec<u16> = "xyz".encode_utf16().collect();
        // Ten pixels a unit: thirty pixels show three units.
        let (view, units) = edit.with_composition(&composition, 2, 30, |text| {
            text.encode_utf16().count() as u32 * 10
        });
        assert_eq!(view.display_text(), "abxyzcd");
        assert_eq!(units, 2..5);
        assert_eq!(view.caret, 4, "the caret sits at the composition cursor");
        assert_eq!(view.first_visible_unit, 1, "scrolled to keep it in view");
        assert_eq!(edit.display_text(), "abcd", "the edit keeps its own text");
    }

    #[test]
    fn backspace_takes_a_surrogate_pair_whole_and_one_bmp_unit_otherwise() {
        let mut edit = NewEdit::default();
        for unit in "a\u{1F600}b地".encode_utf16() {
            edit.insert_unit_filtered(unit, 0, false, &[]);
        }
        edit.backspace();
        edit.backspace();
        assert_eq!((edit.display_text().as_str(), edit.caret), ("a\u{1F600}", 3));
        edit.backspace();
        assert_eq!((edit.display_text().as_str(), edit.caret), ("a", 1));
        // A lone low surrogate goes alone.
        edit.units = vec![0x61, 0xDC00];
        edit.caret = 2;
        edit.backspace();
        assert_eq!(edit.units, [0x61]);
    }
}
