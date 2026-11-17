//! The layout edit mode: in a match the player drags the interface's parts
//! with the mouse, and the game keeps where they put them in the player's own
//! layout file (`presentation::player_layout`).
//!
//! The Edit Layout key turns the mode on, and so does the Edit Layout button
//! on the Interface page of the in-game menu's Options, which closes the menu
//! first. While it is on every part that has
//! a place shows an outline and its name; a press on one grabs it and it
//! follows the cursor. The mouse reaches nothing else in the match meanwhile.
//! A drag writes the part's `X` and `Y` into the side's layout section, so it
//! is the same change a modder would write by hand.
//!
//! Two buttons end it: Save writes the file, Reset puts the side's parts back
//! as the mod, or the original, has them, for Save to keep. The key again, or
//! Esc, leaves without saving and puts back what the mode started from.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::presentation::{field_parts, layout_parts, sidebar_render};
use crate::render::batch::SpriteInstance;
use crate::ui::added_parts::{AddedKind, Holder};
use crate::ui::field_layout::{Along, FieldPart, Hold};
use crate::ui::sidebar::{Rect, SidebarPart};

/// The mode's state while it is on.
#[derive(Debug, Default)]
pub(crate) struct LayoutEditState {
    grab: Option<Grab>,
    /// The player's file as it was when the mode came on.
    before: Vec<u8>,
    /// The button a left press went down on: it acts if the press comes up
    /// on it.
    pressed: Option<EditButton>,
}

impl LayoutEditState {
    /// Leave the mode with the player's file now `text`: without `save` the
    /// text goes back to what the mode started from. Whether the file is to
    /// be written: only a Save that changed something writes.
    fn leave(self, text: &mut Vec<u8>, save: bool) -> bool {
        if *text == self.before {
            return false;
        }
        if !save {
            *text = self.before;
        }
        save
    }
}

/// The mode's two buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditButton {
    Save,
    Reset,
}

/// The part the mouse holds.
#[derive(Debug, Clone)]
struct Grab {
    name: String,
    /// The cursor and the part's `X`, `Y` when it was grabbed.
    cursor: (i32, i32),
    start: [i32; 2],
    per_pixel: [f32; 2],
    /// The place last written, so a still cursor writes nothing.
    at: [i32; 2],
}

/// A part the mode can move.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Target {
    /// The part's name: its section is `[<layout>.<name>]`.
    pub name: String,
    /// Where it shows, in frame pixels.
    pub rect: [i32; 4],
    /// Its `X` and `Y` now.
    xy: [i32; 2],
    /// How `X` and `Y` change for a frame pixel the cursor moves.
    per_pixel: [f32; 2],
}

pub(crate) fn active(state: &AppState) -> bool {
    state.match_state.match_presentation.layout_edit.is_some()
}

/// The Edit Layout key: turn the mode on, or leave it without saving. It
/// exists in a match only.
pub(crate) fn toggle(state: &mut AppState) {
    if active(state) {
        finish(state, false);
    } else {
        start(state);
    }
}

/// Turn the mode on, for the key or Options' Edit Layout button. A match only,
/// and a mode already on stays as it is.
pub(crate) fn start(state: &mut AppState) {
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame || active(state) {
        return;
    }
    let presentation = &mut state.match_state.match_presentation;
    presentation.layout_edit = Some(LayoutEditState {
        before: presentation.player_layout_text.clone(),
        ..Default::default()
    });
    // Whatever the mouse was doing in the match ends here.
    state.match_state.input.cancel_tactical_drag();
    state
        .match_state
        .match_presentation
        .in_game_gadgets
        .cancel_pointer_capture();
    crate::app::input::messages::post_layout_edit(
        state,
        crate::app::input::messages::LayoutEditLine::On,
    );
}

/// Esc while the mode is on: leave it without saving.
pub(crate) fn cancel(state: &mut AppState) {
    finish(state, false);
}

/// End the mode: Save writes what moved, anything else puts back the file
/// the mode started from.
fn finish(state: &mut AppState, save: bool) {
    let presentation = &mut state.match_state.match_presentation;
    let Some(edit) = presentation.layout_edit.take() else {
        return;
    };
    let changed = presentation.player_layout_text != edit.before;
    if edit.leave(&mut presentation.player_layout_text, save) {
        crate::app::presentation::player_layout::write(state);
    } else if changed {
        // The parts go back where the mode found them.
        relayout(state);
    }
    crate::app::input::messages::post_layout_edit(
        state,
        if save {
            crate::app::input::messages::LayoutEditLine::Saved
        } else {
            crate::app::input::messages::LayoutEditLine::Cancelled
        },
    );
}

/// Reset: the side's layout leaves the player's file, so its parts stand as
/// the mod, or the original, has them. Save keeps it.
fn reset(state: &mut AppState) {
    let Some(side) = sidebar_render::local_side_name(state).map(str::to_owned) else {
        return;
    };
    let presentation = &mut state.match_state.match_presentation;
    let Some(layout) = presentation.match_layouts.layout_name_for_side(Some(&side)) else {
        return;
    };
    presentation.player_layout_text = crate::ui::match_layout::MatchLayoutFile::without_layout(
        &presentation.player_layout_text,
        layout,
    );
    relayout(state);
}

/// Read the layouts again from the player's text and lay the match out by
/// them.
fn relayout(state: &mut AppState) {
    crate::app::presentation::player_layout::rebuild(state, false);
    sidebar_render::refresh_sidebar_projection(state);
}

/// How `X` and `Y` change for a frame pixel: by the holder's scale, and
/// counting inward when the part holds a far edge.
fn per_pixel(hold: Hold, scale: f32) -> [f32; 2] {
    let sign = |along: Along| if along == Along::End { -1.0 } else { 1.0 };
    [sign(hold.x) / scale, sign(hold.y) / scale]
}

/// A part's `X`, `Y` once the cursor has moved `delta` frame pixels.
fn dragged(start: [i32; 2], per_pixel: [f32; 2], delta: [i32; 2]) -> [i32; 2] {
    std::array::from_fn(|axis| start[axis] + (delta[axis] as f32 * per_pixel[axis]).round() as i32)
}

/// Every part the mode can move, the ones drawn on top last: the sidebar's
/// panels, the battlefield's pieces by a handle each, then what the layout
/// added.
pub(crate) fn targets(state: &AppState) -> Vec<Target> {
    let presentation = &state.match_state.match_presentation;
    let spec = presentation.sidebar_layout_spec;
    let frame = state.sidebar_frame();
    let scale = frame.scale();
    let to_frame = |x: f32, y: f32, w: f32, h: f32| {
        let rect = frame.rect_to_frame(Rect { x, y, w, h });
        [rect.x, rect.y, rect.w, rect.h].map(|v| v.round() as i32)
    };
    let mut out = Vec::new();

    // The sidebar's panels, each as big as its picture.
    let (screen_w, screen_h) = frame.layout_screen();
    let layout = crate::ui::sidebar::compute_layout_with_spec(spec, screen_w, screen_h, 0);
    let atlas = sidebar_render::current_sidebar_chrome(state);
    let size = |entry: Option<crate::render::sidebar_chrome::SidebarChromeEntry>,
                fallback: [f32; 2]| entry.map_or(fallback, |entry| entry.pixel_size);
    let credits = size(atlas.and_then(|atlas| atlas.top_strip_thin), [168.0, 16.0]);
    let top = size(
        atlas.and_then(|atlas| atlas.top_strip_sidebar),
        [168.0, 32.0],
    );
    let radar = size(atlas.map(|atlas| atlas.radar), [168.0, 110.0]);
    let middle = size(atlas.map(|atlas| atlas.side1), [168.0, 69.0]);
    let strip = size(atlas.map(|atlas| atlas.side2), [168.0, 50.0]);
    let power = crate::ui::sidebar::power_bar_rect(&layout, spec);
    let panels = [
        (
            SidebarPart::Credits,
            [layout.credits_x, layout.credits_y, credits[0], credits[1]],
            [spec.credits_x, spec.credits_y],
        ),
        (
            SidebarPart::Top,
            [layout.top_x, layout.top_y, top[0], top[1]],
            [spec.top_x, spec.top_y],
        ),
        (
            SidebarPart::Radar,
            [layout.radar_x, layout.radar_y, radar[0], radar[1]],
            [spec.radar_housing_x, spec.radar_housing_y],
        ),
        (
            SidebarPart::Middle,
            [layout.side1_x, layout.side1_y, middle[0], middle[1]],
            [spec.side1_x, spec.side1_y],
        ),
        (
            SidebarPart::Cameos,
            [
                layout.strip_x,
                layout.cameo_grid_top,
                strip[0],
                layout.cameo_grid_bottom - layout.cameo_grid_top,
            ],
            [spec.strip_x, spec.strip_y],
        ),
        (
            SidebarPart::Power,
            // The bar is thin; its handle is wide enough to grab.
            [power.x, power.y, power.w.max(12.0), power.h],
            [spec.power_bar_x, spec.power_bar_y],
        ),
    ];
    for (part, [x, y, w, h], xy) in panels {
        if spec.shows(part) && w > 0.0 && h > 0.0 {
            out.push(Target {
                name: part.name().to_owned(),
                rect: to_frame(x, y, w, h),
                xy: xy.map(|v| v.round() as i32),
                per_pixel: [1.0 / scale; 2],
            });
        }
    }

    // The battlefield's pieces have a box each, most often the whole
    // battlefield: a handle shows where the piece sits in it.
    let field = field_parts::layout(state);
    for (part, part_box) in [
        (FieldPart::Messages, field.messages),
        (FieldPart::Timers, field.timers),
        (FieldPart::Banner, field.banner),
    ] {
        if !field.shows(part) {
            continue;
        }
        let [x, y, w, h] = field_parts::rect(state, part);
        let rect = match part {
            FieldPart::Messages => [x, y, w.min(220), h.min(19)],
            FieldPart::Timers => {
                let (handle_w, handle_h) = (w.min(160), h.min(20));
                [x + w - handle_w, y + h - handle_h, handle_w, handle_h]
            }
            _ => {
                let (handle_w, handle_h) = (w.min(200), h.min(60));
                [
                    x + (w - handle_w) / 2,
                    y + (h - handle_h) / 2,
                    handle_w,
                    handle_h,
                ]
            }
        };
        out.push(Target {
            name: part.name().to_owned(),
            rect,
            xy: [part_box.x, part_box.y],
            per_pixel: per_pixel(part_box.hold, 1.0),
        });
    }

    // What the layout added: buttons, texts, rows and columns as a whole,
    // and grids of build pictures.
    let placed = layout_parts::placed(state);
    let view = sidebar_render::current_sidebar_view(state);
    for (index, part) in presentation.added_parts.iter().enumerate() {
        let part_scale = match part.holder {
            Holder::Side => scale,
            Holder::Field => 1.0,
            // A group's part moves with its group.
            Holder::Group(_) => continue,
        };
        let rect = match part.kind {
            AddedKind::CameoGrid { .. } => view
                .and_then(|view| view.added_grids.iter().find(|grid| grid.part == index))
                .map(|grid| to_frame(grid.rect.x, grid.rect.y, grid.rect.w, grid.rect.h)),
            _ => placed
                .get(index)
                .map(|placed| layout_parts::click_rect(state, placed)),
        };
        let Some(rect) = rect.filter(|rect| rect[2] > 0 && rect[3] > 0) else {
            continue;
        };
        let name = part
            .section
            .split_once('.')
            .map_or(part.section.as_str(), |(_, name)| name);
        out.push(Target {
            name: name.to_owned(),
            rect,
            xy: [part.place.x, part.place.y],
            per_pixel: per_pixel(part.place.hold, part_scale),
        });
    }
    out
}

/// The Save and Reset buttons with their labels, in frame pixels: side by
/// side in the middle of the battlefield, just under the banner's handle
/// where the original layout centres it.
fn buttons(state: &AppState) -> [(EditButton, String, [i32; 4]); 2] {
    let font = &state.renderer.bit_font;
    let labels = [
        (EditButton::Save, crate::i18n::tr("VERA:LayoutEditSave")),
        (EditButton::Reset, crate::i18n::tr("VERA:LayoutEditReset")),
    ];
    let width = labels
        .iter()
        .map(|(_, label)| font.text_width(label) as i32 + 16)
        .max()
        .unwrap_or(0)
        .max(56);
    let height = font.cell_height().round() as i32 + 8;
    let gap = 8;
    let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(state);
    let [x, y, w, h] = [x, y, w, h].map(|v| v as i32);
    let left = x + (w - 2 * width - gap) / 2;
    // `targets` gives the banner a handle up to 60 high round the middle.
    let top = y + h / 2 + h.min(60) / 2 + 6;
    let mut column = 0;
    labels.map(|(button, label)| {
        let rect = [left + column * (width + gap), top, width, height];
        column += 1;
        (button, label, rect)
    })
}

fn contains([x, y, w, h]: [i32; 4], cursor: (i32, i32)) -> bool {
    (x..x + w).contains(&cursor.0) && (y..y + h).contains(&cursor.1)
}

/// A left press on a button holds it; one elsewhere grabs the topmost part
/// under the cursor. Letting go over the held button runs it.
pub(crate) fn mouse(state: &mut AppState, button: winit::event::MouseButton, pressed: bool) {
    if button != winit::event::MouseButton::Left {
        return;
    }
    let cursor = state.match_state.input.cursor_px();
    if pressed {
        let held = buttons(state)
            .into_iter()
            .find(|(_, _, rect)| contains(*rect, cursor))
            .map(|(button, ..)| button);
        let grab = held
            .is_none()
            .then(|| {
                targets(state)
                    .into_iter()
                    .rev()
                    .find(|target| contains(target.rect, cursor))
            })
            .flatten()
            .map(|target| Grab {
                name: target.name,
                cursor,
                start: target.xy,
                per_pixel: target.per_pixel,
                at: target.xy,
            });
        if let Some(edit) = &mut state.match_state.match_presentation.layout_edit {
            edit.pressed = held;
            edit.grab = grab;
        }
        return;
    }
    let Some(edit) = &mut state.match_state.match_presentation.layout_edit else {
        return;
    };
    edit.grab = None;
    let Some(held) = edit.pressed.take() else {
        return;
    };
    let released_on_it = buttons(state)
        .iter()
        .any(|(button, _, rect)| *button == held && contains(*rect, cursor));
    match held {
        _ if !released_on_it => {}
        EditButton::Save => finish(state, true),
        EditButton::Reset => reset(state),
    }
}

/// A grabbed part follows the cursor.
pub(crate) fn cursor_moved(state: &mut AppState) {
    let cursor = state.match_state.input.cursor_px();
    let Some(edit) = &mut state.match_state.match_presentation.layout_edit else {
        return;
    };
    let Some(grab) = &mut edit.grab else {
        return;
    };
    let at = dragged(
        grab.start,
        grab.per_pixel,
        [cursor.0 - grab.cursor.0, cursor.1 - grab.cursor.1],
    );
    if at == grab.at {
        return;
    }
    grab.at = at;
    let name = grab.name.clone();
    let Some(side) = sidebar_render::local_side_name(state).map(str::to_owned) else {
        return;
    };
    let presentation = &mut state.match_state.match_presentation;
    presentation.player_layout_text = placed_in_text(
        &presentation.player_layout_text,
        presentation.match_layouts.layout_name_for_side(Some(&side)),
        &side,
        &name,
        at,
    );
    relayout(state);
}

/// The player's file with part `name` of the side's layout at `at`: `X` and
/// `Y` in `[<layout>.<name>]`. A side that has no layout gets one of the
/// player's own, `Own<side>`, with its `[Layouts]` line.
fn placed_in_text(
    text: &[u8],
    layout: Option<&str>,
    side: &str,
    name: &str,
    at: [i32; 2],
) -> Vec<u8> {
    let own = format!("Own{side}");
    let mut text = text.to_vec();
    let layout = match layout {
        Some(layout) => layout,
        None => {
            text = crate::util::ini_writer::set_ini_values(&text, "Layouts", &[(side, &own)]);
            &own
        }
    };
    let (x, y) = (at[0].to_string(), at[1].to_string());
    crate::util::ini_writer::set_ini_values(
        &text,
        &format!("{layout}.{name}"),
        &[("X", &x), ("Y", &y)],
    )
}

/// The mode's picture, for the tooltip's lanes over the whole frame: a
/// one-pixel frame around every part it can move and the part's name on
/// black in its corner, then the Save and Reset buttons, framed the same
/// with their labels in the middle. The grabbed part's and the held
/// button's are white.
pub(crate) fn instances(
    state: &AppState,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
) {
    let Some(edit) = &state.match_state.match_presentation.layout_edit else {
        return Default::default();
    };
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let tint = crate::render::sidebar_text::native_radar_outline_color(
        sidebar_render::current_sidebar_theme(state),
    );
    let font = &state.renderer.bit_font;
    let grabbed = edit.grab.as_ref().map(|grab| grab.name.as_str());
    let quad = |x: f32, y: f32, w: f32, h: f32, tint: [f32; 3], depth: f32| SpriteInstance {
        position: [x + camera[0], y + camera[1]],
        size: [w, h],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth,
        tint,
        alpha: 1.0,
        ..Default::default()
    };
    let outline = |[x, y, w, h]: [f32; 4], colour: [f32; 3]| {
        [
            (x, y, w, 1.0),
            (x, y + h - 1.0, w, 1.0),
            (x, y, 1.0, h),
            (x + w - 1.0, y, 1.0, h),
        ]
        .map(|(x, y, w, h)| quad(x, y, w, h, colour, 0.000205))
    };
    let (mut fills, mut borders, mut texts) = (Vec::new(), Vec::new(), Vec::new());
    // Where names already stand: two parts that start at one corner get
    // their names one under the other.
    let mut named: Vec<[f32; 2]> = Vec::new();
    let line = font.cell_height() + 2.0;
    for target in targets(state) {
        let rect = target.rect.map(|v| v as f32);
        let colour = if grabbed == Some(target.name.as_str()) {
            [1.0; 3]
        } else {
            tint
        };
        borders.extend(outline(rect, colour));
        let mut at = [rect[0] + 1.0, rect[1] + 1.0];
        while named
            .iter()
            .any(|taken| (taken[0] - at[0]).abs() < 24.0 && (taken[1] - at[1]).abs() < line)
        {
            at[1] += line;
        }
        named.push(at);
        fills.push(quad(
            at[0],
            at[1],
            font.text_width(&target.name) as f32 + 4.0,
            line,
            [0.0; 3],
            0.00021,
        ));
        texts.extend(font.build_text(
            &target.name,
            at[0] + 2.0,
            at[1] + 1.0,
            1.0,
            0.00020,
            colour,
            camera,
        ));
    }
    for (button, label, rect) in buttons(state) {
        let [x, y, w, h] = rect.map(|v| v as f32);
        let colour = if edit.pressed == Some(button) {
            [1.0; 3]
        } else {
            tint
        };
        fills.push(quad(x, y, w, h, [0.0; 3], 0.00021));
        borders.extend(outline([x, y, w, h], colour));
        texts.extend(font.build_text(
            &label,
            x + ((w - font.text_width(&label) as f32) / 2.0).round(),
            y + ((h - font.cell_height()) / 2.0).round(),
            1.0,
            0.00020,
            colour,
            camera,
        ));
    }
    (fills, borders, texts)
}

#[cfg(all(test, tests_app_input_d_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::ui::match_layout::MatchLayoutFile;
    use crate::ui::sidebar::{SidebarChromeLayoutSpec, SidebarTheme};

    #[test]
    fn a_drag_moves_x_and_y_by_the_holders_scale_and_inward_from_a_far_edge() {
        // A sidebar part at Large (150%) moves two units for three pixels.
        let near = per_pixel(Hold::default(), 1.5);
        assert_eq!(dragged([10, 20], near, [30, -15]), [30, 10]);
        // A piece held by the bottom right counts inward: right and down are
        // less.
        let far = per_pixel(Hold::named("BottomRight").unwrap(), 1.0);
        assert_eq!(dragged([6, 6], far, [-40, -10]), [46, 16]);
        let middle = per_pixel(Hold::named("Top").unwrap(), 1.0);
        assert_eq!(dragged([0, 4], middle, [25, 5]), [25, 9]);
    }

    #[test]
    fn a_moved_part_is_written_into_the_sides_layout_and_read_back() {
        // The mod gives the Soviets a layout; the Allies have none.
        let mod_file = IniFile::from_str("[Layouts]\nNod=Low\n[Low.Radar]\nX=4\n");
        let read = |text: &[u8]| {
            let mut text = text.to_vec();
            text.push(b'\n');
            MatchLayoutFile::from_layers(Some(&mod_file), IniFile::from_bytes(&text).ok().as_ref())
        };
        // Into the mod's layout, with no line of its own in [Layouts].
        let text = placed_in_text(b"", Some("Low"), "Nod", "Radar", [9, 70]);
        let text = placed_in_text(&text, Some("Low"), "Nod", "Radar", [12, 71]);
        let written = String::from_utf8(text.clone()).unwrap();
        assert_eq!(written.matches("[Low.Radar]").count(), 1, "{written}");
        assert!(!written.contains("[Layouts]"), "{written}");
        let mut soviet = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Soviet);
        read(&text)
            .for_side(Some("Nod"))
            .unwrap()
            .apply_to_sidebar(&mut soviet);
        assert_eq!(
            (soviet.radar_housing_x, soviet.radar_housing_y),
            (12.0, 71.0)
        );
        // A side with no layout gets the player's own, in the same file.
        let text = placed_in_text(&text, None, "GDI", "Timers", [5, 30]);
        let file = read(&text);
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        assert_eq!(file.layout_name_for_side(Some("GDI")), Some("OwnGDI"));
        let mut allied = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        file.for_side(Some("GDI"))
            .unwrap()
            .apply_to_sidebar(&mut allied);
        assert_eq!(
            allied.field.part_rect(FieldPart::Timers, [0, 0, 632, 568]),
            [5, 30, 627, 538]
        );
        assert_eq!(file.layout_name_for_side(Some("Nod")), Some("Low"));
    }

    #[test]
    fn leaving_without_save_puts_the_file_back_and_only_a_save_that_changed_it_writes() {
        let mode = || LayoutEditState {
            before: b"[OwnNod.Radar]\nX=1\n".to_vec(),
            ..Default::default()
        };
        let moved = b"[OwnNod.Radar]\nX=9\n".to_vec();
        let mut text = moved.clone();
        assert!(!mode().leave(&mut text, false));
        assert_eq!(text, mode().before);
        let mut text = moved.clone();
        assert!(mode().leave(&mut text, true));
        assert_eq!(text, moved);
        let mut text = mode().before;
        assert!(
            !mode().leave(&mut text, true),
            "nothing moved, nothing written"
        );
    }
}
