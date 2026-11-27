//! Shared A3 lifecycle/input. Original5FBEF0/5FB320 and command assignment5FBB38.
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::state::FrontendState;
use crate::app::input::hotkeys::{self, BindingAssignmentError, catalog::registered_commands};
#[cfg(any(
    all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"),
    target_os = "android"
))]
use crate::app::keyboard_shim::KeyEventExtModifierSupplement;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::{App, AppState, PlatformState};
use crate::ui::shell::keyboard::*;
use crate::ui::shell::page::PageEvent;
use crate::util::time::Instant;
use vera20k_gui::{Prop, Value};
use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::{Key, KeyCode, KeyLocation, NamedKey};
#[cfg(not(any(
    all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"),
    target_os = "android"
)))]
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::Window;

/// A keyboard edge after the front-end/dialog admission gates. Both a window
/// event and a literal-key production capture enter this same binding owner.
pub(crate) struct InGameKeyEdge<'a> {
    pub physical: KeyCode,
    pub logical: &'a Key,
    pub unmodified: &'a Key,
    pub location: KeyLocation,
    pub state: ElementState,
    pub repeat: bool,
    /// The characters the press types (WM_CHAR), for the chat edit.
    pub text: Option<&'a str>,
}

pub(crate) fn in_game_key_edge(
    state: &mut AppState,
    edge: InGameKeyEdge<'_>,
    paused_at_event: bool,
) {
    // Releases must clear custom scroll state even when chat or a pause owns
    // the key. Forget its old direction before resolving a new press as well.
    if state
        .match_state
        .input
        .camera_scroll_keys
        .release(edge.physical)
    {
        state.match_state.input.keys_held.remove(&edge.physical);
    }
    // The Formation Line key comes up whoever owns the edge, as above; so
    // does the ferry's waypoint key.
    if !edge.state.is_pressed() {
        crate::app::input::ferry::via_key_edge(
            &state.frontend,
            &mut state.match_state,
            edge.physical,
            false,
        );
        state
            .match_state
            .input
            .formation_line
            .release_key(edge.physical);
    }
    // Native chat owns its edit keys before game-local chords or hotkeys.
    if crate::app::input::chat::key_edge(state, &edge) {
        if !edge.state.is_pressed() {
            state.match_state.input.keys_held.remove(&edge.physical);
            if let Some(scroll_key) = hotkeys::physical_scroll_key(edge.physical) {
                state.match_state.input.keys_held.remove(&scroll_key);
            }
        }
        return;
    }
    // Shared pause keeps network chat usable without admitting gameplay keys.
    // Local modal/capture filtering has already happened in the window owner.
    if !hotkeys::input_admitted_while_paused(paused_at_event, edge.logical) {
        return;
    }
    let is_escape = matches!(edge.logical, Key::Named(NamedKey::Escape));
    let locked = crate::app::input::campaign_input::is_locked(&state.match_state);
    // Esc dismisses a client module's node before the game's own Escape
    // (RFC 0001 section 10, `Dismiss`).
    if is_escape
        && edge.state.is_pressed()
        && !edge.repeat
        && !locked
        && !paused_at_event
        && crate::app::ui_modules::key_escape(state)
    {
        return;
    }
    let binding_key = hotkeys::binding_logical_key(edge.logical, edge.unmodified, edge.location);
    let mut resolution = state.match_state.input.hotkey_bindings.resolve_event(
        binding_key,
        edge.location,
        state.match_state.input.hotkey_modifiers,
    );
    // EncodeAndEnqueue54F200 admits Escape while A8ED9C is set and refuses
    // other buffered keys. ProcessCommand55DF80 then falls back to Options
    // for the Escape base key even if its ordinary binding cannot execute.
    if locked && is_escape {
        resolution = hotkeys::HotkeyResolution::Command(hotkeys::HotkeyCommand::Options);
    }
    // VERA ferry: while its drop click is armed, the Planning Mode key marks
    // the clicks that add waypoints and does not open Planning Mode.
    if state.match_state.input.ferry.armed()
        && !locked
        && !paused_at_event
        && resolution == hotkeys::HotkeyResolution::Command(hotkeys::HotkeyCommand::PlanningMode)
    {
        if edge.state.is_pressed() {
            crate::app::input::ferry::via_key_edge(
                &state.frontend,
                &mut state.match_state,
                edge.physical,
                true,
            );
        }
        return;
    }
    let owned_planning_release =
        !edge.state.is_pressed() && state.match_state.input.planning.owns_key(edge.physical);
    if owned_planning_release || !locked || is_escape {
        let planning_consumed = crate::app::input::planning_mode::key_edge(
            &state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &state.persistence,
            resolution,
            edge.physical,
            edge.state,
            edge.repeat,
        );
        let consumed = planning_consumed
            || crate::app::input::dispatch::handle_type_select_key_edge(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                &state.persistence,
                resolution,
                edge.physical,
                edge.state,
                edge.repeat,
            );
        // VERA formation line: its key is held, not run.
        if edge.state.is_pressed()
            && !consumed
            && !locked
            && !paused_at_event
            && resolution
                == hotkeys::HotkeyResolution::Command(hotkeys::HotkeyCommand::FormationLine)
        {
            state
                .match_state
                .input
                .formation_line
                .press_key(edge.physical);
        }
        if edge.state.is_pressed() && !edge.repeat && !consumed {
            crate::app::input::dispatch::handle_hotkey_pressed(state, resolution, edge.physical);
        }
    }
    // A key no game hotkey holds goes to the client modules' hotkeys: game
    // hotkeys win (RFC 0001 section 10).
    if edge.state.is_pressed()
        && !edge.repeat
        && !locked
        && !paused_at_event
        && resolution == hotkeys::HotkeyResolution::Unhandled
    {
        let modifiers = state.match_state.input.hotkey_modifiers;
        crate::app::ui_modules::key_unused(state, binding_key, edge.location, modifiers);
    }
    // Paused capture changes no held-key state, even on the closing Escape.
    if paused_at_event {
        return;
    }
    if edge.state.is_pressed() {
        if let Some(scroll_key) = hotkeys::camera_command_key(resolution)
            && !locked
        {
            state
                .match_state
                .input
                .camera_scroll_keys
                .press(edge.physical, scroll_key);
            state.match_state.input.keys_held.insert(edge.physical);
        } else if let Some(scroll_key) = hotkeys::fallback_scroll_key(resolution)
            && !locked
        {
            state.match_state.input.keys_held.insert(scroll_key);
        } else if hotkeys::physical_scroll_key(edge.physical).is_none() {
            state.match_state.input.keys_held.insert(edge.physical);
        }
    } else {
        // Release clears an admitted scroll flag even after a binding/NumLock change.
        state.match_state.input.keys_held.remove(&edge.physical);
        if let Some(scroll_key) = hotkeys::fallback_scroll_key(resolution)
            .or_else(|| hotkeys::physical_scroll_key(edge.physical))
        {
            state.match_state.input.keys_held.remove(&scroll_key);
        }
    }
}

pub(crate) fn layout(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<KeyboardLayout> {
    layout_and_pointer(platform, renderer, frontend, match_state, persistence)
        .map(|(layout, _)| layout)
}

/// [`layout`] and the pointer in its pixels: the active-game shell's in game,
/// the window's on the front end.
fn layout_and_pointer(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<(KeyboardLayout, (i32, i32))> {
    let dialog = frontend.keyboard_dialog.as_ref()?;
    let (width, height, shell, pointer) = if dialog.parent == KeyboardParent::GameControls {
        let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
        )?;
        (
            shell.width,
            shell.height,
            Some((shell.layout, shell.button_size)),
            crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            ),
        )
    } else {
        let (x, y) = match_state.input.cursor_position();
        (
            renderer.render_width(&frontend.screen) as i32,
            renderer.render_height(&frontend.screen) as i32,
            None,
            (x.round() as i32, y.round() as i32),
        )
    };
    Some((
        crate::app::frontend::shell_layout::keyboard_layout(&renderer.gpu, width, height, shell),
        pointer,
    ))
}
/// Makes A3's nodes current without input (rects, the category's popup
/// rows, the texts its statics show): its renderer's call every drawn frame
/// before it paints from them. Texts allocate only when they change.
pub(crate) fn sync(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> Option<KeyboardLayout> {
    let layout = layout(platform, renderer, frontend, match_state, persistence)?;
    let dialog = frontend.keyboard_dialog.as_ref()?;
    let catalog = registered_commands();
    let bindings = &match_state.input.hotkey_bindings;
    let selected = dialog
        .selected()
        .and_then(|row| dialog.rows.get(row).copied());
    let shortcut = selected
        .and_then(|row| catalog.get(dialog.commands[row].command_index))
        .map(|metadata| {
            bindings
                .first_key(metadata.command)
                .map(key_name)
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let owner = bindings
        .command_at(dialog.captured)
        .filter(|_| dialog.current_owner_visible)
        .and_then(|owner| {
            dialog.commands.iter().position(|row| {
                catalog
                    .get(row.command_index)
                    .is_some_and(|metadata| metadata.command == owner)
            })
        });
    let capture = key_name(dialog.captured);
    let error = dialog.error.map(|key| process_assets.csf_label(key, key));
    let dialog = frontend.keyboard_dialog.as_mut()?;
    let rows = Value::Rect(layout.category_rows(dialog.categories.len()));
    dialog.ui.set_scale(layout.scale);
    let KeyboardState {
        ui,
        commands,
        categories,
        category,
        title,
        ..
    } = dialog;
    super::dialog_page::sync_dialog(
        ui,
        KEYBOARD_PAGE.dialog,
        // KeyboardState::new built A3's controls.
        || vera20k_gui::NodeSpec::new(vera20k_gui::NodeKind::Dialog),
        |id| layout.rect(id),
        |gui, root| {
            use crate::ui::shell::page::{set, set_text};
            // The heading's text as `refresh_title` resolved it; the page
            // shows it once its reveal starts.
            let heading = crate::ui::shell::in_game_shell::TITLE;
            set_text(gui, root, heading, Prop::Text, title.text());
            let face = KeyboardControl::Category.id();
            set(gui, root, face, Prop::PopupRect, rows);
            let name = categories.get(*category).map_or("", String::as_str);
            set_text(gui, root, face, Prop::Text, name);
            let description = selected.map_or("", |row| commands[row].description.as_str());
            set_text(gui, root, DESCRIPTION_TEXT, Prop::Text, description);
            let id = KeyboardControl::CurrentShortcut.id();
            set_text(gui, root, id, Prop::Text, &shortcut);
            let name = owner.map_or("", |row| commands[row].name.as_str());
            set_text(
                gui,
                root,
                KeyboardControl::CurrentOwner.id(),
                Prop::Text,
                name,
            );
            set_text(gui, root, CAPTURE_TEXT, Prop::Text, &capture);
            let error = error.as_deref().unwrap_or("");
            set_text(gui, root, KeyboardControl::Error.id(), Prop::Text, error);
        },
    );
    Some(layout)
}

/// A command's name, category or description ([`ProcessAssets::csf_label`]).
fn localized(process_assets: &ProcessAssets, key: &str, parameter: Option<u8>) -> String {
    let text = process_assets.csf_label(key, key);
    match parameter {
        Some(value) => text
            .replace("%2d", &format!("{value:2}"))
            .replace("%d", &value.to_string()),
        None => text,
    }
}
pub(crate) fn open(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    parent: KeyboardParent,
) {
    if parent == KeyboardParent::GameControls {
        // BBB52C sets state4/result1;4E1D9A applies+writes before dispatcher48CA03.
        // VERA's Game Controls is a page of the in-game menu, which applies and
        // writes its controls as it closes for this state.
        App::enter_in_game_menu_state(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            crate::ui::pause_menu::InGameMenuState::Keyboard,
        );
    }
    // Phobos registers `FireTacticalSW1..10` and `ToggleSWSidebar` only when
    // their rules switches are on, so the page does not list them otherwise.
    #[cfg(feature = "phobos")]
    let sw_sidebar_globals =
        crate::app::state::active_rules(frontend, match_state).map(|rules| rules.sw_sidebar);
    // Ares registers its commands at startup (hook 0x533058); VERA lists them
    // for a game Ares plays.
    let ares = crate::app::state::active_rules(frontend, match_state)
        .is_some_and(crate::rules::ruleset::RuleSet::ares);
    let commands = registered_commands()
        .iter()
        .enumerate()
        .filter(|(_, metadata)| ares || !hotkeys::catalog::is_ares_command(metadata.command))
        .filter(|(_, metadata)| {
            #[cfg(feature = "phobos")]
            {
                sw_sidebar_globals.is_none_or(|globals| match metadata.command {
                    hotkeys::HotkeyCommand::FireTacticalSw(_) => globals.keys_enabled,
                    hotkeys::HotkeyCommand::ToggleSwSidebar => globals.toggle_key_enabled,
                    _ => true,
                })
            }
            #[cfg(not(feature = "phobos"))]
            {
                let _ = metadata;
                true
            }
        })
        .map(|(index, metadata)| KeyboardCommandRow {
            command_index: index,
            name: localized(process_assets, metadata.name_key, metadata.parameter),
            category: localized(process_assets, metadata.category_key, None),
            description: localized(process_assets, metadata.description_key, metadata.parameter),
        })
        .collect();
    frontend.keyboard_dialog = Some(KeyboardState::new(
        parent,
        commands,
        vec![localized(
            process_assets,
            hotkeys::catalog::VERA_CATEGORY_KEY,
            None,
        )],
    ));
    refresh_title(frontend, process_assets);
    platform.window.request_redraw();
}
/// Resolve the open page's heading into its static, as it opens and after
/// the string table's labels change; the page paints it from there.
pub(crate) fn refresh_title(frontend: &mut FrontendState, process_assets: &ProcessAssets) {
    let text = process_assets.csf_label(KEYBOARD_PAGE.title_key, "Keyboard Options");
    if let Some(dialog) = frontend.keyboard_dialog.as_mut() {
        dialog.title.set_text(&text, Instant::now());
    }
}
fn reload(
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    reset: bool,
) {
    let Some(config) = platform.game_config.as_ref() else {
        return;
    };
    let archive = process_assets
        .manager()
        .and_then(|a| a.get_archive_ref("KEYBOARDMD.INI"));
    match_state.input.hotkey_bindings = if reset {
        crate::app::persistence::keyboard::reset(config.paths.settings_dir(), archive)
    } else {
        crate::app::persistence::keyboard::reload(config.paths.settings_dir(), archive)
    };
    crate::app::ui_modules::rebind_game_keys(match_state);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyboardExit {
    Back,
    Cancel,
    PumpTerminated,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingExit {
    Save,
    Reload,
    Retain,
}
impl KeyboardExit {
    fn binding_action(self) -> BindingExit {
        match self {
            Self::Back => BindingExit::Save,
            Self::Cancel => BindingExit::Reload,
            Self::PumpTerminated => BindingExit::Retain,
        }
    }
    fn recreate_parent(self, parent: KeyboardParent) -> bool {
        self != Self::PumpTerminated || parent == KeyboardParent::Launcher
    }
}
/// Preserve the app's existing OS-close-to-terminal convention for a suspended
/// D5 parent. Original5FBF37's pump terminal destroys A3 without binding IO;
/// result2 does not imply that IDCANCEL5FB6D4 executed. Native WM_CLOSE
/// equivalence is not asserted by this adapter.
pub(crate) fn prepare_terminal_exit(state: &mut AppState) {
    close(state, KeyboardExit::PumpTerminated);
}
/// Back or Cancel. The front-end page slides out first (`0x005FBEF0`
/// tears A3 down through `0x00622720`); the in-game page closes at once.
fn close(state: &mut AppState, exit: KeyboardExit) {
    let launcher = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.parent == KeyboardParent::Launcher);
    if launcher && exit != KeyboardExit::PumpTerminated {
        App::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellSlideKind::Keyboard,
            move |state| {
                crate::app::input::keyboard::commit_close(
                    &mut state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &mut state.persistence,
                    exit,
                )
            },
        );
    } else {
        commit_close(
            &mut state.platform,
            &state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &mut state.persistence,
            exit,
        );
    }
}

/// A3 is gone: save or reload the bindings and rebuild the parent.
pub(crate) fn commit_close(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    exit: KeyboardExit,
) {
    let Some(dialog) = frontend.keyboard_dialog.take() else {
        return;
    };
    if exit.binding_action() == BindingExit::Save {
        if let Some(config) = platform.game_config.as_ref()
            && let Err(error) = crate::app::persistence::keyboard::save(
                config.paths.settings_dir(),
                &match_state.input.hotkey_bindings,
            )
        {
            log::warn!("Could not save VERA.INI keyboard settings: {error}");
        }
    } else if exit.binding_action() == BindingExit::Reload {
        // IDCANCEL5FB6D4 reloads CURRENT disk, including a preceding Reset All.
        reload(platform, match_state, process_assets, false);
    }
    if !exit.recreate_parent(dialog.parent) {
        return;
    }
    match dialog.parent {
        KeyboardParent::Launcher => App::open_launcher_options_dialog(
            platform,
            renderer,
            frontend,
            process_assets,
            audio,
            &persistence.options_profile,
        ),
        // State_Machine 0x0048CA0D writes state 5 after A3 returns, where
        // gamemd recreates BBB. VERA's Game Controls is the in-game menu's
        // page, so the menu is what comes back.
        KeyboardParent::GameControls => App::enter_in_game_menu_state(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            crate::ui::pause_menu::InGameMenuState::Menu,
        ),
    }
    platform.window.request_redraw();
}
fn activate(state: &mut AppState, id: KeyboardButton) {
    match id {
        KeyboardButton::Back => close(state, KeyboardExit::Back),
        KeyboardButton::ResetAll => {
            reload(
                &state.platform,
                &mut state.match_state,
                &state.process_assets,
                true,
            );
            if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                dialog.reset_categories();
            }
        }
        KeyboardButton::Assign => {
            let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
                return;
            };
            let Some(command) = dialog
                .selected_command()
                .map(|r| registered_commands()[r.command_index].command)
            else {
                return;
            };
            let result = state
                .match_state
                .input
                .hotkey_bindings
                .assign(command, dialog.captured);
            dialog.error = result.err().map(|error| match error {
                BindingAssignmentError::CannotMap => "Error:CannotMap",
                BindingAssignmentError::CannotRemap => "Error:CannotRemap",
            });
            // Parent466→465 refreshes current shortcut and clears the capture.
            dialog.captured = 0;
            dialog.current_owner_visible = false;
            // Module hotkeys yield to the new binding (RFC 0001 section 10).
            crate::app::ui_modules::rebind_game_keys(&mut state.match_state);
        }
    }
}
/// A3's control events, after their cues: a button's action, a command
/// row's selection, a category pick.
fn apply(state: &mut AppState, events: Vec<PageEvent>) {
    for event in super::dialog_page::play_cues(
        &state.frontend,
        &state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        events,
    ) {
        match event {
            PageEvent::Activated(CANCEL) => close(state, KeyboardExit::Cancel),
            PageEvent::Activated(id) => {
                if let Some(KeyboardControl::Button(button)) = KeyboardControl::from_id(id) {
                    activate(state, button);
                }
            }
            PageEvent::Selected(_, row) => {
                if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                    dialog.select_command(row as usize);
                }
            }
            PageEvent::Picked(_, row) => {
                if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                    dialog.select_category(row.max(0) as usize);
                }
            }
            _ => {}
        }
    }
}

/// Poll the captured list gesture and return its next event-loop wake deadline.
pub(crate) fn poll_scroll_repeat(window: &Window, frontend: &mut FrontendState) -> Option<Instant> {
    let dialog = frontend.keyboard_dialog.as_mut()?;
    dialog.ui.poll_then(|| window.request_redraw())
}

/// One pointer message on A3 (`down`: `None` moves). The category combo
/// opens on a press anywhere on its face; its open list takes every press,
/// and closes on it. A press on the face or the capture field moves the
/// focus there. Back, Assign and Reset All are owner-draw
/// (GUIMainButtonSound, 0x0061376B); a command row press plays the generic
/// click.
fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some((layout, (x, y))) = layout_and_pointer(
        &state.platform,
        &state.renderer,
        &state.frontend,
        &state.match_state,
        &state.persistence,
    ) else {
        return;
    };
    state.platform.window.request_redraw();
    let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
        return;
    };
    let rows = Value::Rect(layout.category_rows(dialog.categories.len()));
    dialog.ui.set_scale(layout.scale);
    let events = super::dialog_page::route(
        &mut dialog.ui,
        KEYBOARD_PAGE.dialog,
        // KeyboardState::new built A3's controls.
        || vera20k_gui::NodeSpec::new(vera20k_gui::NodeKind::Dialog),
        |id| layout.rect(id),
        |gui, root| {
            let category = KeyboardControl::Category.id();
            crate::ui::shell::page::set(gui, root, category, Prop::PopupRect, rows)
        },
        super::dialog_page::event_at(x, y, down),
    );
    apply(state, events);
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    pointer(state, None);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button == MouseButton::Left {
        pointer(state, Some(pressed));
    }
}

pub(crate) fn wheel(state: &mut AppState, lines: f32) {
    // A dialog's wheel goes to its hovered list; the point is the pointer's.
    let Some((layout, (x, y))) = layout_and_pointer(
        &state.platform,
        &state.renderer,
        &state.frontend,
        &state.match_state,
        &state.persistence,
    ) else {
        return;
    };
    let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
        return;
    };
    // The open category list (`WheelPicks`) steps its selection a notch,
    // any turn at all; otherwise the hovered command list takes three rows.
    let lines = match dialog.category_open() {
        true => lines.signum() as i32,
        false => (lines * 3.0) as i32,
    };
    dialog.ui.set_scale(layout.scale);
    dialog
        .ui
        .on_input(vera20k_gui::InputEvent::Wheel { lines, x, y });
    let events = dialog.ui.drain_events();
    let events = crate::ui::shell::page::page_events(dialog.ui.gui(), events);
    apply(state, events);
}
pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
        return;
    };
    let code = match event.physical_key {
        winit::keyboard::PhysicalKey::Code(code) => Some(code),
        _ => None,
    };
    let gui_key = code.and_then(|code| crate::ui::shell::controller::gui_key(code, false));
    if event.state.is_pressed()
        && let Some(key) = gui_key
    {
        // The open category list takes the arrows, Enter and Escape (with
        // its close cue); otherwise Escape reaches the hidden IDCANCEL.
        let open = dialog.category_open();
        let consumed = if open {
            let now = dialog.ui.now();
            dialog.ui.gui_mut().key_popup(key, now);
            true
        } else {
            dialog.ui.on_input(vera20k_gui::InputEvent::Key(key))
        };
        let mut events = dialog.ui.drain_events();
        events.extend(dialog.ui.gui_mut().drain_events());
        let events = crate::ui::shell::page::page_events(dialog.ui.gui(), events);
        apply(state, events);
        if consumed {
            return;
        }
    } else if dialog.category_open() {
        return;
    }
    let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
        return;
    };
    if !dialog.capture_focused() {
        return;
    }
    let unmodified = event.key_without_modifiers();
    let logical = hotkeys::binding_logical_key(&event.logical_key, &unmodified, event.location);
    let Some(vk) = hotkeys::logical_virtual_key(logical, event.location) else {
        return;
    };
    // Parent5FB73A rejects HOTKEYF_EXT, including arrows/Insert/Home/etc.
    let extended = event.location != winit::keyboard::KeyLocation::Numpad
        && matches!(
            event.logical_key,
            Key::Named(
                NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
                    | NamedKey::ArrowUp
                    | NamedKey::ArrowDown
                    | NamedKey::Home
                    | NamedKey::End
                    | NamedKey::PageUp
                    | NamedKey::PageDown
                    | NamedKey::Insert
            )
        );
    if let Some(captured) = capture_key(
        dialog.captured,
        vk,
        hotkeys::modifier_bits(state.platform.live_modifiers),
        event.state.is_pressed(),
        extended,
    ) {
        dialog.captured = captured;
        dialog.current_owner_visible = true;
        dialog.error = None;
    }
}

/// Stock hotkey control state plus A3's HOTKEYF_EXT rejection. The saved
/// current-Windows class probe is distinct from the original formatter oracle.
fn capture_key(before: u16, vk: u16, modifiers: u16, pressed: bool, extended: bool) -> Option<u16> {
    let modifier = match vk {
        16 => Some(0x100),
        17 => Some(0x200),
        18 => Some(0x400),
        _ => None,
    };
    if let Some(bit) = modifier {
        return if pressed {
            Some(modifiers | bit)
        } else if before & 0xff == 0 {
            Some(modifiers & !bit)
        } else {
            None
        };
    }
    if !pressed {
        return None;
    }
    // Clear occurs before DefWindowProc handles these reserved keys.
    if matches!(vk, 8 | 9 | 13 | 27 | 32 | 46) || extended {
        Some(0)
    } else {
        Some(vk | modifiers)
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    fn GetKeyNameTextW(parameter: i32, buffer: *mut u16, size: i32) -> i32;
}
fn virtual_key_name(vk: u16) -> String {
    #[cfg(windows)]
    {
        let mut buffer = [0u16; 32];
        // SAFETY: read-only platform lookup, valid fixed UTF16 buffer and size.
        let length = unsafe {
            let scan = MapVirtualKeyW(u32::from(vk), 0);
            GetKeyNameTextW(((scan << 16) | 0x02000001) as i32, buffer.as_mut_ptr(), 32)
        };
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }
    // VERA: Windows names its keys in its own language; elsewhere VERA's
    // language files name them.
    #[cfg(not(windows))]
    {
        use crate::i18n::tr;
        match vk {
            0 => String::new(),
            0x0d => tr("VERA:KeyEnter"),
            16 => tr("VERA:KeyShift"),
            17 => tr("VERA:KeyCtrl"),
            18 => tr("VERA:KeyAlt"),
            0x13 => tr("VERA:KeyPause"),
            0x14 => tr("VERA:KeyCapsLock"),
            0x1b => tr("VERA:KeyEsc"),
            0x20 => tr("VERA:KeySpace"),
            0x2c => tr("VERA:KeyPrintScreen"),
            0x90 => tr("VERA:KeyNumLock"),
            0x30..=0x5a => char::from_u32(u32::from(vk)).unwrap().to_string(),
            0x70..=0x87 => format!("F{}", vk - 0x6f),
            _ => hotkeys::portable_oem_key_name(vk)
                .map_or_else(|| format!("{vk}"), |text| text.to_string()),
        }
    }
}
/// Original61EF70: Alt,Ctrl,Shift names joined by literal '+', then low-byteVK.
/// No CSF 'None' placeholder; an empty native key name produces an empty caption.
pub(crate) fn key_name(encoded: u16) -> String {
    let mut output = String::new();
    for (mask, vk) in [(0x400, 18), (0x200, 17), (0x100, 16)] {
        if encoded & mask != 0 {
            output.push_str(&virtual_key_name(vk));
            output.push('+');
        }
    }
    output.push_str(&virtual_key_name(encoded & 0xff));
    output
}

#[cfg(all(test, tests_app_input_d_z))]
mod tests {
    use super::*;
    #[cfg(not(windows))]
    #[test]
    fn a3_punctuation_capture_assignment_and_reload_use_the_same_virtual_key() {
        use winit::keyboard::{KeyLocation, ModifiersState};
        for (plain, shifted, vk) in [
            (";", ":", 0xba),
            ("=", "+", 0xbb),
            (",", "<", 0xbc),
            ("-", "_", 0xbd),
            (".", ">", 0xbe),
            ("/", "?", 0xbf),
            ("`", "~", 0xc0),
            ("[", "{", 0xdb),
            ("\\", "|", 0xdc),
            ("]", "}", 0xdd),
            ("'", "\"", 0xde),
        ] {
            let raw = Key::Character(shifted.into());
            let unmodified = Key::Character(plain.into());
            let key = hotkeys::binding_logical_key(&raw, &unmodified, KeyLocation::Standard);
            let mapped = hotkeys::logical_virtual_key(key, KeyLocation::Standard).unwrap();
            assert_eq!(mapped, vk);
            let captured = capture_key(0, mapped, 0x100, true, false).unwrap();
            assert_eq!(capture_key(captured, mapped, 0x100, false, false), None);
            assert_eq!(key_name(captured), format!("Shift+{plain}"));
            let mut bindings = hotkeys::HotkeyBindings::default();
            bindings
                .assign(hotkeys::HotkeyCommand::StopObject, captured)
                .unwrap();
            let encoded = bindings.to_ini_string();
            let restored = hotkeys::HotkeyBindings::reload_from_ini_bytes(Some(encoded.as_bytes()));
            assert_eq!(
                restored.resolve_event(key, KeyLocation::Standard, ModifiersState::SHIFT),
                hotkeys::HotkeyResolution::Command(hotkeys::HotkeyCommand::StopObject)
            );
        }
    }

    #[test]
    fn terminal_child_exit_preserves_binding_io_distinction_and_parent_final_write() {
        // 5FB900 save,5FB6D4 reload,5FBF37 retain.55FD1E->55FCA0 resumesD5;
        // BBB was accepted before state4 and needs no second terminal write.
        assert_eq!(KeyboardExit::Back.binding_action(), BindingExit::Save);
        assert_eq!(KeyboardExit::Cancel.binding_action(), BindingExit::Reload);
        assert_eq!(
            KeyboardExit::PumpTerminated.binding_action(),
            BindingExit::Retain
        );
        for parent in [KeyboardParent::Launcher, KeyboardParent::GameControls] {
            assert!(KeyboardExit::Back.recreate_parent(parent));
            assert!(KeyboardExit::Cancel.recreate_parent(parent));
        }
        assert!(KeyboardExit::PumpTerminated.recreate_parent(KeyboardParent::Launcher));
        assert!(!KeyboardExit::PumpTerminated.recreate_parent(KeyboardParent::GameControls));
    }
    #[test]
    fn field_state_matches_scoped_windows_control_probe_and_native_ext_rejection() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/keyboard_key_names.json",
        ))
        .unwrap();
        let cases = fixture["capture"].as_array().unwrap();
        assert_eq!(cases.len(), 13);
        for case in cases {
            let before = case["before"].as_u64().unwrap() as u16;
            let vk = case["vk"].as_u64().unwrap() as u16;
            let extended = case["extended"].as_bool().unwrap();
            let native_to_game = |raw: u16| if raw & 0x800 != 0 { 0 } else { raw };
            let down = capture_key(before, vk, 0, true, extended).unwrap_or(before);
            let up = capture_key(down, vk, 0, false, extended).unwrap_or(down);
            assert_eq!(
                down,
                native_to_game(case["down"].as_u64().unwrap() as u16),
                "down VK{vk}"
            );
            assert_eq!(
                up,
                native_to_game(case["up"].as_u64().unwrap() as u16),
                "up VK{vk}"
            );
        }
    }
}
