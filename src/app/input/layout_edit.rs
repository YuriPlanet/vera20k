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
//! The mode is a modal dialog of the match's GUI (`dialog_ui`) over its
//! chrome: the buttons and the parts are its nodes, so the GUI's hit test,
//! capture and drag take the pointer, and nothing under the dialog does.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use vera20k_gui::{NodeId, NodeKind, Prop, Value};

use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::presentation::text_hud::{self, TextHud};
use crate::app::presentation::{field_parts, layout_parts, sidebar_render};
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::render::batch::SpriteInstance;
use crate::ui::added_parts::{AddedKind, Holder};
use crate::ui::field_layout::{Along, FieldPart, Hold};
use crate::ui::game_screen::GameScreen;
use crate::ui::sidebar::{ArtKey, PartKind, Rect};

/// The mode's state while it is on.
#[derive(Debug, Default)]
pub(crate) struct LayoutEditState {
    grab: Option<Grab>,
    /// The player's file as it was when the mode came on.
    before: Vec<u8>,
}

/// The mode's dialog on `dialog_ui`: VERA's own, no retail resource.
const EDIT_DIALOG: crate::ui::shell::descriptor::DialogResourceId =
    crate::ui::shell::descriptor::DialogResourceId(0xFE01);
/// Its nodes' ids: the two buttons, then a part each from `ID_TARGET_BASE`.
const ID_SAVE: i32 = 1;
const ID_RESET: i32 = 2;
const ID_TARGET_BASE: i32 = 100;

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

impl EditButton {
    fn id(self) -> i32 {
        match self {
            EditButton::Save => ID_SAVE,
            EditButton::Reset => ID_RESET,
        }
    }

    fn of(id: i32) -> Option<Self> {
        match id {
            ID_SAVE => Some(EditButton::Save),
            ID_RESET => Some(EditButton::Reset),
            _ => None,
        }
    }
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

pub(crate) fn active(presentation: &MatchPresentationState) -> bool {
    presentation.layout_edit.is_some()
}

/// The Edit Layout key: turn the mode on, or leave it without saving. It
/// exists in a match only.
pub(crate) fn toggle(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    if active(&match_state.match_presentation) {
        finish(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            false,
        );
    } else {
        start(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
    }
}

/// Turn the mode on, for the key or Options' Edit Layout button. A match only,
/// and a mode already on stays as it is.
pub(crate) fn start(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    if frontend.screen != crate::ui::game_screen::GameScreen::InGame
        || active(&match_state.match_presentation)
    {
        return;
    }
    let presentation = &mut match_state.match_presentation;
    presentation.layout_edit = Some(LayoutEditState {
        before: presentation.player_layout_text.clone(),
        ..Default::default()
    });
    // Whatever the mouse was doing in the match ends here; the mode's dialog
    // takes the pointer over the chrome.
    match_state.input.cancel_tactical_drag();
    crate::app::input::chrome_input::cancel(&mut match_state.match_presentation);
    match_state
        .match_presentation
        .dialog_ui
        .push_modal(EDIT_DIALOG);
    crate::app::input::messages::post_layout_edit(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        crate::app::input::messages::LayoutEditLine::On,
    );
}

/// Esc while the mode is on: leave it without saving.
pub(crate) fn cancel(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    finish(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        false,
    );
}

/// End the mode: Save writes what moved, anything else puts back the file
/// the mode started from.
fn finish(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    save: bool,
) {
    let presentation = &mut match_state.match_presentation;
    let Some(edit) = presentation.layout_edit.take() else {
        return;
    };
    presentation.dialog_ui.close(EDIT_DIALOG);
    let changed = presentation.player_layout_text != edit.before;
    if edit.leave(&mut presentation.player_layout_text, save) {
        crate::app::presentation::player_layout::write(platform, &match_state.match_presentation);
    } else if changed {
        // The parts go back where the mode found them.
        relayout(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        );
    }
    crate::app::input::messages::post_layout_edit(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        persistence,
        if save {
            crate::app::input::messages::LayoutEditLine::Saved
        } else {
            crate::app::input::messages::LayoutEditLine::Cancelled
        },
    );
}

/// Reset: the side's layout leaves the player's file, so its parts stand as
/// the mod, or the original, has them. Save keeps it.
fn reset(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    let Some(side) = sidebar_render::local_side_name(frontend, match_state).map(str::to_owned)
    else {
        return;
    };
    let presentation = &mut match_state.match_presentation;
    let Some(layout) = presentation.match_layouts.layout_name_for_side(Some(&side)) else {
        return;
    };
    presentation.player_layout_text = crate::ui::match_layout::MatchLayoutFile::without_layout(
        &presentation.player_layout_text,
        layout,
    );
    relayout(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
    );
}

/// Read the layouts again from the player's text and lay the match out by
/// them.
fn relayout(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    crate::app::presentation::player_layout::rebuild(
        platform,
        frontend,
        match_state,
        persistence,
        false,
    );
    sidebar_render::refresh_sidebar_projection(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
    );
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
pub(crate) fn targets(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> Vec<Target> {
    let presentation = &match_state.match_presentation;
    let spec = presentation.sidebar_layout_spec;
    let frame = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        &frontend.screen,
        &match_state.match_presentation.sidebar_layout_spec,
    );
    let scale = frame.scale();
    let to_frame = |x: f32, y: f32, w: f32, h: f32| frame.rect_to_frame_px(Rect { x, y, w, h });
    let mut out = Vec::new();

    // Every part of the sidebar that draws: a button or a tab where the view
    // has it, the map picture as big as the layout makes it, a panel as big
    // as its picture. A part after its panel lies over it, so it is grabbed
    // first.
    let layout = sidebar_render::current_sidebar_layout(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let atlas = sidebar_render::current_sidebar_chrome(frontend, match_state);
    let view = sidebar_render::current_sidebar_view(&match_state.match_presentation);
    let size = |part, key| {
        atlas
            .and_then(|atlas| atlas.art(part, key, 0))
            .map(|art| art.pixel_size)
    };
    for part in spec.parts_in_order() {
        let [x, y] = layout.origin(&spec, part);
        let rect = match part.kind() {
            PartKind::Button | PartKind::Tab => view
                .and_then(|view| view.button(part))
                .map(|(rect, _)| [rect.x, rect.y, rect.w, rect.h]),
            PartKind::Minimap => {
                let map = layout.minimap_rect(&spec);
                Some([map.x, map.y, map.w, map.h])
            }
            PartKind::CameoGrid => size(part, ArtKey::RowArt)
                .map(|[w, _]| [x, layout.cameo_grid_top, w, layout.cameo_grid_height()]),
            // The bar is thin; its handle is wide enough to grab.
            PartKind::PowerBar => {
                let power = crate::ui::sidebar::power_bar_rect(&layout, spec);
                Some([power.x, power.y, power.w.max(12.0), power.h])
            }
            PartKind::Image | PartKind::Radar => size(part, ArtKey::Art).map(|[w, h]| [x, y, w, h]),
            PartKind::Text | PartKind::Panel => None,
        };
        if let Some([x, y, w, h]) = rect.filter(|&[.., w, h]| w > 0.0 && h > 0.0) {
            out.push(Target {
                name: part.name().to_owned(),
                rect: to_frame(x, y, w, h),
                xy: spec.place(part).map(|v| v.round() as i32),
                per_pixel: [1.0 / scale; 2],
            });
        }
    }

    // The battlefield's pieces have a box each, most often the whole
    // battlefield: a handle shows where the piece sits in it.
    let field = field_parts::layout(&match_state.match_presentation);
    for (part, part_box) in [
        (FieldPart::Messages, field.messages),
        (FieldPart::Timers, field.timers),
        (FieldPart::Banner, field.banner),
    ] {
        if !field.shows(part) {
            continue;
        }
        let [x, y, w, h] = field_parts::rect(
            platform,
            renderer,
            &frontend.screen,
            &match_state.match_presentation,
            persistence,
            part,
        );
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
    let placed = layout_parts::placed(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    );
    let view = sidebar_render::current_sidebar_view(&match_state.match_presentation);
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
            _ => placed.get(index).map(|placed| {
                layout_parts::click_rect(
                    platform,
                    renderer,
                    &frontend.screen,
                    &match_state.match_presentation,
                    persistence,
                    placed,
                )
            }),
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
fn buttons(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    persistence: &PersistenceState,
) -> [(EditButton, String, [i32; 4]); 2] {
    let font = &renderer.bit_font;
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
    let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        screen,
        presentation,
        persistence,
    );
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

/// Puts the mode's dialog on `dialog_ui` (again, when the controller was
/// reset under it) with a node for each button and each part, the buttons
/// first and the parts drawn on top before the ones under them, so the
/// dialog's press finds the button, else the topmost part. Returns the
/// parts by their node's `id - ID_TARGET_BASE`.
fn sync_nodes(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> Vec<Target> {
    let targets = targets(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    );
    let buttons = buttons(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let ui = &mut match_state.match_presentation.dialog_ui;
    if ui.root_in(EDIT_DIALOG).is_none() {
        ui.push_modal(EDIT_DIALOG);
    }
    let Some(root) = ui.root_in(EDIT_DIALOG) else {
        return targets;
    };
    let gui = ui.gui_mut();
    let old: Vec<NodeId> = gui
        .tree()
        .children(root)
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    for node in old {
        let _ = gui.remove(node);
    }
    let mut add = |id: i32, [x, y, w, h]: [i32; 4], draggable: bool| {
        let Ok(node) = gui.create(NodeKind::Button, Some(root)) else {
            return;
        };
        let _ = gui.set(node, Prop::Id, Value::Int(id));
        let _ = gui.set(
            node,
            Prop::Rect,
            Value::Rect(vera20k_gui::Rect::new(x, y, w, h)),
        );
        if draggable {
            // A press grabs the part; it follows from the first move.
            let _ = gui.set(node, Prop::Draggable, Value::Bool(true));
            let _ = gui.set(node, Prop::DragThreshold, Value::Int(0));
        }
    };
    for (button, _, rect) in &buttons {
        add(button.id(), *rect, false);
    }
    for (index, target) in targets.iter().enumerate().rev() {
        add(ID_TARGET_BASE + index as i32, target.rect, true);
    }
    targets
}

/// The button the mode's dialog holds pressed.
fn pressed_button(presentation: &MatchPresentationState) -> Option<EditButton> {
    let ui = &presentation.dialog_ui;
    let node = ui.pressed_node_in(EDIT_DIALOG)?;
    EditButton::of(ui.gui().id_of(node)?)
}

/// A left press on a button holds it; one elsewhere grabs the topmost part
/// under the cursor. Letting go over the held button runs it.
pub(crate) fn mouse(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    button: winit::event::MouseButton,
    pressed: bool,
) {
    if button != winit::event::MouseButton::Left {
        return;
    }
    let cursor = match_state.input.cursor_px();
    if pressed {
        let targets = sync_nodes(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
        );
        let ui = &mut match_state.match_presentation.dialog_ui;
        ui.on_pointer_move(cursor.0, cursor.1);
        ui.on_pointer_down(cursor.0, cursor.1);
        let _ = ui.drain_events();
        let grabbed = ui
            .gui()
            .drag()
            .and_then(|drag| ui.gui().id_of(drag.node))
            .and_then(|id| usize::try_from(id - ID_TARGET_BASE).ok());
        let grab = grabbed
            .and_then(|index| targets.into_iter().nth(index))
            .map(|target| Grab {
                name: target.name,
                cursor,
                start: target.xy,
                per_pixel: target.per_pixel,
                at: target.xy,
            });
        if let Some(edit) = &mut match_state.match_presentation.layout_edit {
            edit.grab = grab;
        }
        return;
    }
    let ui = &mut match_state.match_presentation.dialog_ui;
    ui.on_pointer_up(cursor.0, cursor.1);
    let events = ui.drain_events();
    let ran = events.iter().find_map(|event| match event {
        vera20k_gui::UiEvent::Activated { node } => EditButton::of(ui.gui().id_of(*node)?),
        _ => None,
    });
    let Some(edit) = &mut match_state.match_presentation.layout_edit else {
        return;
    };
    edit.grab = None;
    match ran {
        Some(EditButton::Save) => finish(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            true,
        ),
        Some(EditButton::Reset) => reset(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        ),
        None => {}
    }
}

/// A grabbed part follows the cursor.
pub(crate) fn cursor_moved(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    let cursor = match_state.input.cursor_px();
    let Some(edit) = &mut match_state.match_presentation.layout_edit else {
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
    let Some(side) = sidebar_render::local_side_name(frontend, match_state).map(str::to_owned)
    else {
        return;
    };
    let presentation = &mut match_state.match_presentation;
    presentation.player_layout_text = placed_in_text(
        &presentation.player_layout_text,
        presentation.match_layouts.layout_name_for_side(Some(&side)),
        &side,
        &name,
        at,
    );
    relayout(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
    );
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
/// The edit mode's overlay drawn through `hud`: the name boxes, the
/// outlines and the texts, each lane in drawing order.
pub(crate) fn instances(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    hud: &mut TextHud,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
) {
    let camera = [match_state.input.camera_x, match_state.input.camera_y];
    let pieces = pieces(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    );
    draw_pieces(&renderer.bit_font, &pieces, camera, hud)
}

/// One piece of the overlay in the frame's pixels: a box filled black, an
/// outline's side, or a text, in the order they draw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EditPiece {
    Box([f32; 4]),
    Side([f32; 4], [f32; 3]),
    Text(String, [f32; 2], [f32; 3]),
}

/// The overlay's pieces: each target's outline, name box and name, then
/// each button's box, outline and label. A name moves down past names
/// already standing at its corner.
fn pieces(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> Vec<EditPiece> {
    let Some(edit) = &match_state.match_presentation.layout_edit else {
        return Vec::new();
    };
    // The side's text colour as its layout gives it.
    let tint = crate::render::sidebar_text::native_outline_from(
        match_state.match_presentation.sidebar_layout_spec.side_text,
    );
    let font = &renderer.bit_font;
    let grabbed = edit.grab.as_ref().map(|grab| grab.name.as_str());
    let outline = |[x, y, w, h]: [f32; 4], colour: [f32; 3]| {
        [
            (x, y, w, 1.0),
            (x, y + h - 1.0, w, 1.0),
            (x, y, 1.0, h),
            (x + w - 1.0, y, 1.0, h),
        ]
        .map(|(x, y, w, h)| EditPiece::Side([x, y, w, h], colour))
    };
    let mut out = Vec::new();
    // Where names already stand: two parts that start at one corner get
    // their names one under the other.
    let mut named: Vec<[f32; 2]> = Vec::new();
    let line = font.cell_height() + 2.0;
    for target in targets(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    ) {
        let rect = target.rect.map(|v| v as f32);
        let colour = if grabbed == Some(target.name.as_str()) {
            [1.0; 3]
        } else {
            tint
        };
        out.extend(outline(rect, colour));
        let mut at = [rect[0] + 1.0, rect[1] + 1.0];
        while named
            .iter()
            .any(|taken| (taken[0] - at[0]).abs() < 24.0 && (taken[1] - at[1]).abs() < line)
        {
            at[1] += line;
        }
        named.push(at);
        out.push(EditPiece::Box([
            at[0],
            at[1],
            font.text_width(&target.name) as f32 + 4.0,
            line,
        ]));
        out.push(EditPiece::Text(
            target.name.clone(),
            [at[0] + 2.0, at[1] + 1.0],
            colour,
        ));
    }
    for (button, label, rect) in buttons(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    ) {
        let [x, y, w, h] = rect.map(|v| v as f32);
        let colour = if pressed_button(&match_state.match_presentation) == Some(button) {
            [1.0; 3]
        } else {
            tint
        };
        out.push(EditPiece::Box([x, y, w, h]));
        out.extend(outline([x, y, w, h], colour));
        out.push(EditPiece::Text(
            label.to_string(),
            [
                x + ((w - font.text_width(&label) as f32) / 2.0).round(),
                y + ((h - font.cell_height()) / 2.0).round(),
            ],
            colour,
        ));
    }
    out
}

/// Name boxes black behind the outlines (0.00021, 0.000205) behind the
/// texts (0.00020); the outlines and texts in their exact colours.
const BOX_DEPTH: f32 = 0.00021;
const SIDE_DEPTH: f32 = 0.000205;
const TEXT_DEPTH: f32 = 0.00020;

/// The pieces as nodes of `hud`, returned as the name boxes, the outlines
/// and the glyphs, each in drawing order.
fn draw_pieces(
    font: &crate::render::bit_font::BitFont,
    pieces: &[EditPiece],
    camera: [f32; 2],
    hud: &mut TextHud,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
) {
    let whole = |[x, y, w, h]: [f32; 4]| {
        vera20k_gui::Rect::new(x as i32, y as i32, w.ceil() as i32, h.ceil() as i32)
    };
    hud.begin();
    for piece in pieces {
        match piece {
            EditPiece::Box(rect) => hud.fill(
                whole(*rect),
                [rect[0] + camera[0], rect[1] + camera[1]],
                [rect[2], rect[3]],
                text_hud::rgb([0; 3]),
                BOX_DEPTH,
            ),
            EditPiece::Side(rect, colour) => hud.fill_tinted(
                whole(*rect),
                [rect[0] + camera[0], rect[1] + camera[1]],
                [rect[2], rect[3]],
                text_hud::rounded(*colour),
                SIDE_DEPTH,
                Some(*colour),
            ),
            EditPiece::Text(text, at, colour) => hud.text_tinted(
                whole([
                    at[0],
                    at[1],
                    font.text_width(text) as f32,
                    font.cell_height(),
                ]),
                text,
                [at[0] + camera[0], at[1] + camera[1]],
                text_hud::rounded(*colour),
                TEXT_DEPTH,
                None,
                Some(*colour),
            ),
        }
    }
    hud.finish(font);
    let (boxes, sides) = hud
        .boxes()
        .copied()
        .partition(|quad| quad.depth == BOX_DEPTH);
    (boxes, sides, hud.glyphs().copied().collect())
}

#[cfg(all(test, tests_app_input_d_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::ui::match_layout::MatchLayoutFile;
    use crate::ui::sidebar::{SidebarChromeLayoutSpec, SidebarPart, SidebarTheme};

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
            .for_side(Some("Nod"), false)
            .unwrap()
            .apply_to_sidebar(&mut soviet);
        assert_eq!(soviet.place(SidebarPart::Radar), [12.0, 71.0]);
        // A side with no layout gets the player's own, in the same file.
        let text = placed_in_text(&text, None, "GDI", "Timers", [5, 30]);
        let file = read(&text);
        assert!(file.problems().is_empty(), "{:?}", file.problems());
        assert_eq!(file.layout_name_for_side(Some("GDI")), Some("OwnGDI"));
        let mut allied = SidebarChromeLayoutSpec::for_theme(SidebarTheme::Allied);
        file.for_side(Some("GDI"), true)
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
