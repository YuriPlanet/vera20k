//! The buttons and texts a side's layout adds (`ui::added_parts`): where each
//! sits this frame, what it shows, how it is drawn and what a click gives.
//!
//! Drawing and clicks take one list ([`placed`]), so a part is clicked where
//! it is drawn. A button's command is one the game already has: a
//! command-bar button's (`uimd.ini` `ButtonList=` names) runs as that button
//! does, any other name is a hotkey's from the keyboard file and runs as the
//! key does.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::hotkeys::HotkeyCommand;
use crate::render::batch::SpriteInstance;
use crate::ui::added_parts::{AddedKind, Holder, TextValue};

/// What a layout button's `Command=` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonCommand {
    /// A command-bar button, by its place in `COMMAND_NAMES`.
    Bar(usize),
    Hotkey(HotkeyCommand),
}

/// The command a name gives, without regard to capitals. The command bar's
/// names come first: `TypeSelect` and `PlanningMode` are in both lists, and
/// a click should do what the bar's button does.
pub(crate) fn command_named(name: &str) -> Option<ButtonCommand> {
    crate::ui::sidebar::command_bar::COMMAND_NAMES
        .iter()
        .position(|bar| bar.eq_ignore_ascii_case(name))
        .map(ButtonCommand::Bar)
        .or_else(|| crate::app::input::hotkeys::command_named(name).map(ButtonCommand::Hotkey))
}

/// A button's padding around its label, on each side.
const BUTTON_PAD: [i32; 2] = [5, 2];

/// One added part as this frame shows it. [`placed`] gives one for each
/// part, in the layout's order.
#[derive(Debug, Clone)]
pub(crate) struct Placed {
    /// The sidebar or the battlefield: for a group's part, its group's.
    pub holder: Holder,
    /// `x, y, width, height`: frame pixels in the battlefield, the
    /// sidebar's layout pixels in the sidebar.
    pub rect: [i32; 4],
    pub text: String,
    pub button: bool,
    /// A button's picture now, when its `Art=` file was found: frame 0, or
    /// frame 1 while the button is held down.
    pub picture: Option<crate::render::sidebar_chrome::SidebarChromeEntry>,
    /// Whether the button is held down.
    pub pressed: bool,
    /// A row or a column, which places its parts and draws nothing itself,
    /// or a cameo grid, which the sidebar's view draws and clicks.
    pub group: bool,
}

/// What a text part shows now.
fn value_text(state: &AppState, value: TextValue) -> String {
    let view = crate::app::presentation::sidebar_render::current_sidebar_view(state);
    let sim = state
        .match_state
        .sim_runtime
        .presentation()
        .map(|runtime| &runtime.simulation);
    let house = || {
        let sim = sim?;
        sim.houses().get(&sim.session().current_house()?)
    };
    match value {
        TextValue::Credits => view.map_or(0, |view| view.credits).to_string(),
        TextValue::Power => view.map_or(0, |view| view.power_produced).to_string(),
        TextValue::Drain => view.map_or(0, |view| view.power_drained).to_string(),
        TextValue::Name => state
            .match_state
            .session
            .local_player_name()
            .unwrap_or_default()
            .to_owned(),
        // Fifteen frames to the second, as the timers count.
        TextValue::Time => crate::app::presentation::super_timers::time_text(
            sim.map_or(0, |sim| (sim.session().binary_frame() / 15) as i32),
        ),
        TextValue::Kills => house().map_or(0, |house| house.stats().kills()).to_string(),
        TextValue::Losses => house()
            .map_or(0, |house| house.stats().losses())
            .to_string(),
    }
}

/// A button's label: the text of a string-table name, else the word itself.
fn label_text(state: &AppState, label: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get(label))
        .unwrap_or(label)
        .to_owned()
}

/// Every added part of the local side's layout, where this frame shows it,
/// one for each part in the layout's order.
pub(crate) fn placed(state: &AppState) -> Vec<Placed> {
    let parts = &state.match_state.match_presentation.added_parts;
    if parts.is_empty() {
        return Vec::new();
    }
    let font = &state.renderer.bit_font;
    let frame = state.sidebar_frame();
    let (side_width, side_height) = frame.surface_size();
    let side = [
        frame.layout_left() as i32,
        0,
        side_width as i32,
        side_height as i32,
    ];
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let field = [x as i32, y as i32, width as i32, height as i32];
    let art = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .map(|atlas| &atlas.layout_art);
    let gadgets = &state.match_state.match_presentation.in_game_gadgets;
    // Each part's own content and size first, at no place yet.
    let mut placed: Vec<Placed> = parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let pressed = gadgets.layout_button_pressed(index);
            let (text, button, picture) = match &part.kind {
                AddedKind::Button {
                    label, art: name, ..
                } => {
                    let picture = name
                        .as_deref()
                        .and_then(|name| art?.frames(name))
                        .map(|frames| *frames[usize::from(pressed)]);
                    (label_text(state, label), true, picture)
                }
                AddedKind::Text { value } => (value_text(state, *value), false, None),
                // A group places its parts; a cameo grid is the sidebar
                // view's (`ui::sidebar::append_added_grids`).
                AddedKind::Group { .. } | AddedKind::CameoGrid { .. } => {
                    (String::new(), false, None)
                }
            };
            let pad = if button { BUTTON_PAD } else { [0, 0] };
            // A picture is as big as its file draws it; words as their text.
            let natural = match (&part.kind, picture) {
                (AddedKind::Group { .. } | AddedKind::CameoGrid { .. }, _) => [0, 0],
                (_, Some(picture)) => picture.pixel_size.map(|v| v as i32),
                (_, None) => [
                    font.text_width(&text) as i32 + 2 * pad[0],
                    font.cell_height() as i32 + 2 * pad[1],
                ],
            };
            Placed {
                holder: part.holder,
                rect: [0, 0, natural[0], natural[1]],
                text,
                button,
                picture,
                pressed,
                group: matches!(
                    part.kind,
                    AddedKind::Group { .. } | AddedKind::CameoGrid { .. }
                ),
            }
        })
        .collect();
    // A row or a column is as big as its parts, one after another.
    let mut in_group = vec![[0; 2]; parts.len()];
    for (group, part) in parts.iter().enumerate() {
        let AddedKind::Group { column, gap } = part.kind else {
            continue;
        };
        let members: Vec<usize> = (0..parts.len())
            .filter(|&index| parts[index].holder == Holder::Group(group))
            .collect();
        let sizes: Vec<[i32; 2]> = members
            .iter()
            .map(|&index| {
                let [_, _, width, height] = placed[index].rect;
                [
                    parts[index].place.width.unwrap_or(width),
                    parts[index].place.height.unwrap_or(height),
                ]
            })
            .collect();
        let wrap = if column {
            part.place.height
        } else {
            part.place.width
        };
        let (places, whole) = crate::ui::added_parts::flow(&sizes, column, gap, wrap);
        placed[group].rect = [0, 0, whole[0], whole[1]];
        for ((&index, place), size) in members.iter().zip(places).zip(sizes) {
            in_group[index] = place;
            placed[index].rect = [0, 0, size[0], size[1]];
        }
    }
    // The parts the sidebar and the battlefield hold take their places
    // there; a group's parts follow their group.
    for (index, part) in parts.iter().enumerate() {
        let area = match part.holder {
            Holder::Side => side,
            Holder::Field => field,
            Holder::Group(_) => continue,
        };
        let [_, _, width, height] = placed[index].rect;
        placed[index].rect = part.rect_in(area, [width, height]);
    }
    for (index, part) in parts.iter().enumerate() {
        let Holder::Group(group) = part.holder else {
            continue;
        };
        let (holder, [x, y, _, _]) = (placed[group].holder, placed[group].rect);
        let [_, _, width, height] = placed[index].rect;
        placed[index].holder = holder;
        placed[index].rect = [
            x + in_group[index][0],
            y + in_group[index][1],
            width,
            height,
        ];
    }
    placed
}

/// The frame-pixel rectangle a placed part takes clicks in.
pub(crate) fn click_rect(state: &AppState, placed: &Placed) -> [i32; 4] {
    let [x, y, width, height] = placed.rect;
    match placed.holder {
        Holder::Field | Holder::Group(_) => placed.rect,
        Holder::Side => {
            let rect = state
                .sidebar_frame()
                .rect_to_frame(crate::ui::sidebar::Rect {
                    x: x as f32,
                    y: y as f32,
                    w: width as f32,
                    h: height as f32,
                });
            [
                rect.x.round() as i32,
                rect.y.round() as i32,
                rect.w.round() as i32,
                rect.h.round() as i32,
            ]
        }
    }
}

/// Give the command of the layout's button `index`, as its key or its
/// command-bar button would.
pub(crate) fn clicked(state: &mut AppState, index: usize, right: bool) {
    let command = state
        .match_state
        .match_presentation
        .added_parts
        .get(index)
        .and_then(|part| match &part.kind {
            AddedKind::Button { command, .. } => command_named(command),
            AddedKind::Text { .. } | AddedKind::Group { .. } | AddedKind::CameoGrid { .. } => None,
        });
    match command {
        Some(ButtonCommand::Bar(id)) => {
            crate::app::input::dispatch::dispatch_command_bar(state, id, right)
        }
        Some(ButtonCommand::Hotkey(command)) => crate::app::input::dispatch::handle_hotkey_pressed(
            state,
            crate::app::input::hotkeys::HotkeyResolution::Command(command),
            // The key matters only to the developer chords of an unbound key.
            winit::keyboard::KeyCode::F24,
        ),
        None => {}
    }
}

/// The picture quads of the buttons in `holder` that have one, for a lane
/// drawn with the sidebar's picture atlas. A picture is not stretched: it
/// draws at its own size from the part's corner.
pub(crate) fn pictures(state: &AppState, holder: Holder, camera: [f32; 2]) -> Vec<SpriteInstance> {
    placed(state)
        .into_iter()
        .filter(|part| part.holder == holder && !part.group)
        .filter_map(|part| {
            let picture = part.picture?;
            Some(SpriteInstance {
                position: [
                    part.rect[0] as f32 + camera[0],
                    part.rect[1] as f32 + camera[1],
                ],
                size: picture.pixel_size,
                uv_origin: picture.uv_origin,
                uv_size: picture.uv_size,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            })
        })
        .collect()
}

/// The quads of the parts in `holder` that have no picture: for a
/// white-texture lane a button's box (a black fill inside a one-pixel frame
/// in `tint`, the two swapped while it is held down) and the black a text
/// over the battlefield stands on, and every such part's text. `camera` is
/// what the lane's shader subtracts.
pub(crate) fn instances(
    state: &AppState,
    holder: Holder,
    tint: [f32; 3],
    camera: [f32; 2],
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let font = &state.renderer.bit_font;
    let (mut boxes, mut texts) = (Vec::new(), Vec::new());
    for part in placed(state)
        .into_iter()
        .filter(|part| part.holder == holder && part.picture.is_none() && !part.group)
    {
        let [x, y, width, height] = part.rect.map(|v| v as f32);
        let pad = if part.button { BUTTON_PAD } else { [0, 0] };
        let quad = |x: f32, y: f32, width: f32, height: f32, tint: [f32; 3]| SpriteInstance {
            position: [x + camera[0], y + camera[1]],
            size: [width, height],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            tint,
            alpha: 1.0,
            ..Default::default()
        };
        // A held button shows its label dark on the side's colour.
        let (fill, ink) = if part.pressed {
            (tint, [0.0; 3])
        } else {
            ([0.0; 3], tint)
        };
        if part.button {
            boxes.push(quad(x, y, width, height, tint));
            boxes.push(quad(x + 1.0, y + 1.0, width - 2.0, height - 2.0, fill));
        } else if holder == Holder::Field {
            // Over the battlefield a text stands on black, one pixel out on
            // every side, as the timers' and the messages' text does.
            boxes.push(quad(x - 1.0, y - 1.0, width + 2.0, height + 2.0, [0.0; 3]));
        }
        texts.extend(font.build_text(
            &part.text,
            x + pad[0] as f32,
            y + pad[1] as f32,
            1.0,
            0.0,
            ink,
            camera,
        ));
    }
    (boxes, texts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_names_a_command_bar_button_or_a_hotkey() {
        // The bar's own names win, so a click does what its button does.
        assert_eq!(command_named("guard"), Some(ButtonCommand::Bar(6)));
        assert_eq!(command_named("TypeSelect"), Some(ButtonCommand::Bar(3)));
        assert_eq!(
            command_named("stopobject"),
            Some(ButtonCommand::Hotkey(HotkeyCommand::StopObject))
        );
        assert_eq!(
            command_named("TeamSelect_3"),
            Some(ButtonCommand::Hotkey(HotkeyCommand::TeamSelect(3)))
        );
        assert_eq!(command_named("Levitate"), None);
    }

    #[test]
    fn the_sample_layouts_buttons_name_commands_the_game_has() {
        let mut file = crate::ui::match_layout::MatchLayoutFile::from_bytes(
            crate::test_fixture::text("docs/examples/veraui.ini").as_bytes(),
        );
        file.check_commands(|name| command_named(name).is_some());
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        let buttons = file
            .for_side(Some("Nod"))
            .unwrap()
            .added()
            .iter()
            .filter(|part| matches!(part.kind, AddedKind::Button { .. }))
            .count();
        assert_eq!(buttons, 2);
    }
}
