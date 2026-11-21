//! The shell dialogs' control rects as data: `shell_layout.ini`, built into
//! the game and read once. One section per dialog (`[0x114]`, or
//! `[0x113.Modern]` for a variant), one line per control:
//! `<id>=<Rule>,x,y,w,h`, the resource template in DLU (or pixels, per the
//! rule) and the rule that places it. Ids are hex resource ids, or a page's
//! own ids for statics the template leaves at `0xFFFF`.
//!
//! Rules on the front-end shell ([`place`]) are the [`AnchorRule`] names
//! ([`AnchorRule::named`]), `Px` for a rect already in 800x600 pixels,
//! `Centred` for pixels from the dialog's centred corner,
//! `ComboFace` for a combo's closed face (its `ChildWindow` cut to the face
//! height), `NewEdit` for a NewEdit's window after its init inset
//! (`0x00614CCF`), `Preview` for a right-anchored picture that keeps its
//! created size, and `None` to drop a control from an overlay section.
//! Rules in a running game ([`in_game`]): `SideButton` (a sidebar row,
//! `0x0060B000`), `LowerButton` (`0x0060B350`), `Child` (an ordinary child,
//! `0x0060B7A0`), `Title` and `StatusLine`. [`in_box`] places a centred
//! modal box's children and [`map`] hands a page its templates.

use std::sync::OnceLock;

use super::descriptor::AnchorRule;
use super::geom::{RectPx, ShellScale, center_offset, child_window, dlu_rect};
use super::in_game_shell::{self, InGameShellLayout};
use super::layout::{Placer, RectTable};
use super::menu_page::{MenuPageLayout, MenuPageSpec};

const TEXT: &str = include_str!("shell_layout.ini");

/// One control line: its id, rule and template rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub id: u16,
    pub rule: &'static str,
    pub rect: RectPx,
}

type Sections = Vec<(&'static str, Vec<Entry>)>;

fn sections() -> &'static Sections {
    static SECTIONS: OnceLock<Sections> = OnceLock::new();
    SECTIONS.get_or_init(|| parse(TEXT).0)
}

/// The sections of `text` and the lines it could not read.
fn parse(text: &'static str) -> (Sections, Vec<&'static str>) {
    let mut sections: Sections = Vec::new();
    let mut bad = Vec::new();
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim(), Vec::new()));
            continue;
        }
        match (entry(line), sections.last_mut()) {
            (Some(entry), Some((_, entries))) => entries.push(entry),
            _ => bad.push(line),
        }
    }
    (sections, bad)
}

fn entry(line: &'static str) -> Option<Entry> {
    let (id, value) = line.split_once('=')?;
    let id = u16::from_str_radix(id.trim().strip_prefix("0x")?, 16).ok()?;
    let mut fields = value.split(',').map(str::trim);
    let rule = fields.next()?;
    let mut numbers = [0; 4];
    let mut count = 0;
    for field in fields {
        *numbers.get_mut(count)? = field.parse().ok()?;
        count += 1;
    }
    let [x, y, w, h] = numbers;
    (count == 0 || count == 4).then_some(Entry {
        id,
        rule,
        rect: RectPx::new(x, y, w, h),
    })
}

/// Section `dialog`'s lines, in file order.
pub fn entries(dialog: &str) -> &'static [Entry] {
    sections()
        .iter()
        .find(|(name, _)| *name == dialog)
        .map_or(&[], |(_, entries)| entries.as_slice())
}

/// Control `id`'s line in section `dialog`.
pub fn entry_of(dialog: &str, id: u16) -> Option<&'static Entry> {
    entries(dialog).iter().find(|entry| entry.id == id)
}

/// Section `dialog` placed on the front-end shell at `scale`'s mapping of a
/// `width`x`height` screen ([`Placer::on`]).
pub fn place(dialog: &str, scale: ShellScale, width: i32, height: i32) -> RectTable {
    let placer = Placer::on(scale, width, height);
    entries(dialog)
        .iter()
        .filter_map(|e| {
            let RectPx { x, y, w, h } = e.rect;
            let rect = match e.rule {
                "Px" => placer.px(x, y, w, h),
                "Centred" => {
                    // Pixels from the dialog's corner, which stands at the
                    // half-margins beyond 800x600 (`0x0060AF50`).
                    let (dx, dy) = if scale.is_unscaled() {
                        (center_offset(width, 800), center_offset(height, 600))
                    } else {
                        (0, 0)
                    };
                    placer.px(x + dx, y + dy, w, h)
                }
                "NewEdit" => {
                    let window = child_window(x, y, w, h);
                    placer.px(window.x + 1, window.y + 1, window.w - 2, window.h - 2)
                }
                "Preview" => {
                    // 0x0060B1D0 moves it; it keeps its created window size.
                    let at = scale.base_rect(placer.at(AnchorRule::RightAnchor, x, y, w, h));
                    let created = child_window(x, y, w, h);
                    placer.px(at.x, at.y, created.w, created.h)
                }
                "ComboFace" => {
                    let window = child_window(x, y, w, h);
                    let face = crate::ui::skirmish_shell::COMBO_FACE_H;
                    placer.px(window.x, window.y, window.w, face)
                }
                rule => placer.at(AnchorRule::named(rule)?, x, y, w, h),
            };
            Some((e.id, rect))
        })
        .collect()
}

/// A menu page's layout and section `dialog`'s other controls: the
/// section's heading `0x694` and status line `0x695`, where it has them,
/// replace the family's.
pub fn menu_page(
    spec: &MenuPageSpec,
    dialog: &str,
    scale: ShellScale,
    width: i32,
    height: i32,
) -> (MenuPageLayout, RectTable) {
    let mut page = super::menu_page::compute_layout_for(spec, scale, width, height);
    let mut rects = place(dialog, scale, width, height);
    for (id, slot) in [(0x694, &mut page.title), (0x695, &mut page.status_help)] {
        if let Some(rect) = rects.get(id) {
            *slot = rect;
            rects.remove(id);
        }
    }
    (page, rects)
}

/// Section `dialog` laid over `rects`: each line replaces its control's
/// rect, and a `None` line drops the control.
pub fn overlay(rects: &mut RectTable, dialog: &str, scale: ShellScale, width: i32, height: i32) {
    let placed = place(dialog, scale, width, height);
    for e in entries(dialog) {
        match placed.get(e.id) {
            Some(rect) => rects.insert(e.id, rect),
            None => rects.remove(e.id),
        }
    }
}

/// Section `dialog`'s templates each placed by `place`, for a page that maps
/// its controls itself.
pub fn map(dialog: &str, place: impl Fn(RectPx) -> RectPx) -> RectTable {
    entries(dialog)
        .iter()
        .map(|e| (e.id, place(e.rect)))
        .collect()
}

/// Section `dialog` in a centred modal box `frame`: each template's common
/// resized window ([`child_window`]) from the box's corner, a `NewEdit`
/// after its init inset, `Px` pixels from the corner.
pub fn in_box(dialog: &str, frame: RectPx) -> RectTable {
    entries(dialog)
        .iter()
        .filter_map(|e| {
            let RectPx { x, y, w, h } = e.rect;
            let r = child_window(x, y, w, h).translate(frame.x, frame.y);
            let rect = match e.rule {
                "ChildWindow" => r,
                "Px" => e.rect.translate(frame.x, frame.y),
                "NewEdit" => RectPx::new(r.x + 1, r.y + 1, r.w - 2, r.h - 2),
                _ => return None,
            };
            Some((e.id, rect))
        })
        .collect()
}

/// Section `dialog` placed in a running game's right-panel shell, its sidebar
/// buttons `button_size` pixels.
pub fn in_game(
    dialog: &str,
    shell: &InGameShellLayout,
    width: i32,
    height: i32,
    button_size: [i32; 2],
) -> RectTable {
    entries(dialog)
        .iter()
        .filter_map(|e| {
            let RectPx { x, y, w, h } = e.rect;
            let rect = match e.rule {
                "SideButton" => shell.button_rect(dlu_rect(x, y, w, h), button_size),
                "LowerButton" => shell.lower_button_rect(width, button_size),
                "Child" => in_game_shell::ordinary_child_rect(dlu_rect(x, y, w, h), width, height),
                "Title" => in_game_shell::right_panel_title_rect(width),
                "StatusLine" => in_game_shell::status_line_rect(height),
                "Px" => e.rect,
                _ => return None,
            };
            Some((e.id, rect))
        })
        .collect()
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    /// Every line reads, and names a rule its placement knows.
    #[test]
    fn every_line_reads_and_names_a_known_rule() {
        let (sections, bad) = parse(TEXT);
        assert_eq!(bad, Vec::<&str>::new());
        let in_game_rules = [
            "SideButton",
            "LowerButton",
            "Child",
            "Title",
            "StatusLine",
            "Px",
        ];
        for (name, entries) in &sections {
            for e in entries {
                let known = AnchorRule::named(e.rule).is_some()
                    || matches!(
                        e.rule,
                        "Px" | "Centred" | "ComboFace" | "NewEdit" | "Preview" | "None"
                    )
                    || in_game_rules.contains(&e.rule);
                assert!(known, "[{name}] {:#X}={}", e.id, e.rule);
            }
        }
    }
}
