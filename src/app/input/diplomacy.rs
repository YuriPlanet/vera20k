//! The in-battle Diplomacy dialog 0x73 (in-scenario state 8): its entry
//! requests, its rows from the simulation and its physical input.
//!
//! Entry: the sidebar Diplomacy gadget (`0x006538FE`, input 0x80F2) and the
//! Tab fallback (`Process_Command` 0x0055E08A -> `Request_Diplomacy_Menu`
//! 0x006ABC40). Rows: the proc's 0x497 arm (0x00657DEB) and its 250 ms
//! refresh `UpdateDiplomacyDialog` 0x00658640 -> 0x00658330. The rows are
//! recomputed from the simulation at every use instead of on that timer: in
//! skirmish the world is frozen under the dialog, and in LAN/WOL a frame-rate
//! refresh shows the same values up to 250 ms earlier.

use crate::app::match_runtime::session::SessionMode;
use crate::app::presentation::house_ui::HouseUiSources;
use crate::app::{App, AppState};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::skirmish_modes::MpModeClass;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::diplomacy::{
    DIPLOMACY_ROWS, DiplomacyAlliesBox, DiplomacyButton, DiplomacyChatBox, DiplomacyControl,
    DiplomacyLayout, DiplomacyRow, DiplomacyView, diplomacy_layout,
};
use winit::event::MouseButton;

/// Which control asked for the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiplomacyRequest {
    /// The sidebar Diplomacy gadget, input 0x80F2.
    SidebarButton,
    /// The Tab key's fallback.
    Hotkey,
}

/// The in-scenario state a request writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestedState {
    Nothing,
    /// State 8.
    Diplomacy,
    /// State 9: `ShowMissionRestateBriefing`.
    MissionBriefing,
}

/// The sidebar gadget (`0x00653905..0x65394A`) writes state 9 in campaign
/// and in LAN/WOL Cooperative, else 8. `Request_Diplomacy_Menu` writes 8
/// whenever GameMode is not 0, co-op included, and nothing in campaign.
pub(crate) fn requested_state(
    request: DiplomacyRequest,
    mode: SessionMode,
    mode_class: Option<MpModeClass>,
) -> RequestedState {
    match request {
        DiplomacyRequest::Hotkey if mode.is_nonzero() => RequestedState::Diplomacy,
        DiplomacyRequest::Hotkey => RequestedState::Nothing,
        DiplomacyRequest::SidebarButton
            if !mode.is_nonzero()
                || (mode.is_network()
                    && mode_class.is_some_and(MpModeClass::restates_mission_for_diplomacy)) =>
        {
            RequestedState::MissionBriefing
        }
        DiplomacyRequest::SidebarButton => RequestedState::Diplomacy,
    }
}

/// Write the requested state; `State_Machine` admits it on the way in.
pub(crate) fn open(state: &mut AppState, request: DiplomacyRequest) {
    let mode_class = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| runtime.simulation.session().multiplayer_mode_class());
    match requested_state(request, state.match_state.session_mode(), mode_class) {
        RequestedState::Nothing => {}
        RequestedState::Diplomacy => {
            App::enter_in_game_menu_state(state, InGameMenuState::Diplomacy);
        }
        RequestedState::MissionBriefing => crate::app::input::restate::open(state),
    }
}

/// The per-match switches the rows read besides the houses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiplomacyRules {
    /// `Rules+0x14B5` AlliesAllowed, layered through the mode INI.
    pub allies_allowed: bool,
    /// `[0xA8B244] == 2`: WOL Quick Co-op.
    pub quick_coop: bool,
    /// `[0xA8B538]`: the local player is defeated (or observing).
    pub local_inactive: bool,
    /// GameMode is not 0: `IsControlledByHuman` reads only `+0x1EC`.
    pub game_mode_nonzero: bool,
    /// The chat mask `0xA8D108`, by House array index.
    pub listeners: [bool; 8],
}

/// The rows and the headers they decide.
pub(crate) struct ResolvedRows {
    pub rows: [Option<DiplomacyRow>; DIPLOMACY_ROWS],
    /// 0x597: some row showed an Allies box at init.
    pub allies_header: bool,
    /// 0x425: some row is human-controlled.
    pub chat_header: bool,
}

/// The House array index row `row` shows. Init 0x00657E8B: the player's
/// own slot shows House 0 and row 0 the player.
fn house_for_row(row: usize, player_index: usize) -> usize {
    match row {
        0 => player_index,
        row if row == player_index => 0,
        row => row,
    }
}

/// The rows, named and coloured as the score rows are.
pub(crate) fn resolve_rows(
    sim: &Simulation,
    rules: &RuleSet,
    switches: DiplomacyRules,
    sources: &HouseUiSources<'_>,
    country_name: impl Fn(InternedId) -> Option<String>,
) -> ResolvedRows {
    let empty = ResolvedRows {
        rows: Default::default(),
        allies_header: false,
        chat_header: false,
    };
    let order = sim.houses().order();
    let Some(local) = sim.session().current_house() else {
        return empty;
    };
    let Some(player_index) = sim.houses().index_of(local) else {
        return empty;
    };
    let observer = sim.session().observer_house();
    let allied = |asker, other| {
        crate::sim::combat::combat_weapon::is_ally_by_object(
            Some(sim.house_alliances()),
            sim.interner(),
            asker,
            other,
        )
    };
    let multiplay = |house: InternedId| {
        sim.houses()
            .get(&house)
            .and_then(|state| state.country())
            .is_some_and(|country| rules.country_multiplay(sim.interner().resolve(country)))
    };
    let house_for_row = |row| house_for_row(row, player_index);
    let allies_shown = switches.allies_allowed && observer != Some(local) && !switches.quick_coop;
    let mut allies_header = false;
    let mut chat_header = false;
    let rows = std::array::from_fn(|row| {
        let house = *order.get(house_for_row(row))?;
        let state = sim.houses().get(&house)?;
        // Init hides rows 1..=7 whose House type lacks Multiplay; 0x00658330
        // blanks row 0's texts for such a type.
        if !multiplay(house) {
            return None;
        }
        let human = state.is_controlled_by_human(switches.game_mode_nonzero);
        let allies = (row > 0 && allies_shown).then(|| {
            allies_header = true;
            DiplomacyAlliesBox {
                checked: allied(local, house),
                partner: allied(house, local),
                enabled: !switches.local_inactive && human,
            }
        });
        if row > 0 && human {
            chat_header = true;
        }
        // Only LAN/WOL human rows show the box, and 0x00658525 hides it once
        // the House sent EXIT (House+0x248). 0x00657F8E..0x00657FCC checks
        // it from the mask byte of the row's House, or always once the
        // local player is out.
        let chat = (row > 0 && human && !state.left_game()).then(|| DiplomacyChatBox {
            checked: switches.local_inactive
                || switches
                    .listeners
                    .get(house_for_row(row))
                    .copied()
                    .unwrap_or(false),
            enabled: !switches.local_inactive,
        });
        // UpdateDiplomacyDialog skips a non-player MultiplayPassive House, so
        // its row keeps the init texts: empty, with the init box state.
        if house != local && state.multiplay_passive() {
            return Some(DiplomacyRow {
                insignia: None,
                name: String::new(),
                rgb: [0; 3],
                kills: None,
                allies,
                chat,
            });
        }
        let is_observer = observer == Some(house);
        // 0x00658393..0x006583CF: "LightGrey" for the observer, "Grey" once
        // defeated, else the House's own scheme.
        let named = |name| sources.named_scheme_rgb(name).unwrap_or([0xFF; 3]);
        let rgb = if is_observer {
            named("LightGrey")
        } else if state.is_defeated() {
            named("Grey")
        } else {
            sources.text_rgb(sim, house)
        };
        let insignia = if is_observer {
            crate::app::frontend::skirmish_shell_render::flag_pcx_for_side_item_data(-3)
        } else {
            state
                .country()
                .and_then(|country| rules.country_index(sim.interner().resolve(country)))
                .and_then(|index| {
                    crate::app::frontend::skirmish_shell_render::flag_pcx_for_side_item_data(
                        i32::from(index.0),
                    )
                })
        };
        Some(DiplomacyRow {
            insignia,
            name: sources.ui_name(sim, house, state.country(), &country_name),
            rgb,
            // Σ of the two per-victim kill tables (+0x53E4, +0x5438), `%d`.
            kills: Some(state.stats().kills() as i32),
            // 0x00658330 hides a defeated House's box; nothing re-shows it.
            allies: allies.filter(|_| !state.is_defeated()),
            chat,
        })
    });
    ResolvedRows {
        rows,
        allies_header,
        chat_header,
    }
}

/// The open dialog's contents from the running match.
pub(crate) fn view(state: &AppState) -> DiplomacyView {
    let mut view = DiplomacyView::default();
    let mode = state.match_state.session_mode();
    let (Some(sim), Some(rules)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
        state.rules(),
    ) else {
        return view;
    };
    let computer = crate::app::frontend::shell_pass::resolve_csf(state, "TXT_COMPUTER");
    let switches = DiplomacyRules {
        allies_allowed: rules.general.allies_allowed,
        quick_coop: mode == SessionMode::Wol
            && state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                == Some(crate::ui::wol_shell::WolMode::QuickCoop),
        local_inactive: sim.local_player_defeated(),
        game_mode_nonzero: mode.is_nonzero(),
        listeners: std::array::from_fn(|index| {
            state.match_state.match_presentation.chat.listens(index)
        }),
    };
    let resolved = resolve_rows(
        sim,
        rules,
        switches,
        &HouseUiSources::from_state(state, &computer),
        |country| crate::app::presentation::house_ui::country_display_name(state, sim, country),
    );
    view.rows = resolved.rows;
    view.allies_header = resolved.allies_header;
    view.chat_header = resolved.chat_header;
    if mode.is_network() {
        // 0x00658126: the MPGameMode title.
        view.mode_name = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| {
                crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)
            })
            .map(|mode| {
                crate::app::frontend::shell_pass::resolve_csf(state, &mode.ui_name_key).into_owned()
            });
    }
    // 0x006580E0..0x0065810C: TauntsOn shows in LAN/WOL with LANTaunts /
    // WOLTaunts; the 250 ms refresh never touches it.
    view.taunts = state.frontend.multiplayer_shell_runtime.taunts_on(mode);
    view.map_name = if mode.is_network() {
        crate::app::shell_network_setup::map_display_name(state)
    } else {
        crate::app::frontend::skirmish_shell_render::skirmish_selected_map_name(state)
    }
    .unwrap_or_default()
    .to_string();
    view
}

pub(crate) fn layout(state: &AppState) -> Option<DiplomacyLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(diplomacy_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
    ))
}

fn control_under_pointer(state: &AppState) -> Option<DiplomacyControl> {
    let layout = layout(state)?;
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    view(state).control_at(&layout, x, y)
}

fn return_under(control: Option<DiplomacyControl>) -> Option<DiplomacyButton> {
    (control == Some(DiplomacyControl::Return)).then_some(DiplomacyButton::Return)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let over = control_under_pointer(state);
    let interaction = &mut state.match_state.match_presentation.diplomacy;
    interaction.hovered = over;
    interaction.button.hovered = return_under(over);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let over = control_under_pointer(state);
    state.match_state.match_presentation.diplomacy.hovered = over;
    let target = return_under(over);
    if pressed {
        state
            .match_state
            .match_presentation
            .diplomacy
            .button
            .press(target);
        if target.is_some() {
            // Common type2 button 0x0061374B..0x00613771: click at button-down.
            App::play_skirmish_shell_generic_click_sound(state);
        }
        match over {
            Some(DiplomacyControl::Chat(row)) => toggle_chat_box(state, row),
            Some(DiplomacyControl::Allies(row)) => press_allies_box(state, row),
            Some(DiplomacyControl::Taunts) => toggle_taunts_box(state),
            _ => {}
        }
    } else if state
        .match_state
        .match_presentation
        .diplomacy
        .button
        .release(target)
        == Some(DiplomacyButton::Return)
    {
        // 0x006581AD: Return ends the modal; State_Machine leaves state 8.
        App::enter_in_game_menu_state(state, InGameMenuState::Closed);
    }
}

/// A press on an enabled Allies box. The checkbox proc flips the box and
/// plays GUICheckboxSound; dialog 0x73 flips it straight back
/// (`0x00658245..0x00658261`) and calls `Ally` with the row's House
/// (`0x00658269`). The box shows the sim's mask again at the next refresh,
/// so VERA, which draws the box from the mask, keeps no box state. A hidden
/// or disabled box takes no click.
fn press_allies_box(state: &mut AppState, row: usize) {
    let Some(allies) = view(state)
        .rows
        .get(row)
        .and_then(|row| row.as_ref()?.allies)
    else {
        return;
    };
    if !allies.enabled {
        return;
    }
    let Some(house) = state.match_state.sim_runtime.as_ref().and_then(|runtime| {
        let sim = &runtime.simulation;
        let player = sim.houses().index_of(sim.session().current_house()?)?;
        sim.houses()
            .order()
            .get(house_for_row(row, player))
            .copied()
    }) else {
        return;
    };
    let sound = state
        .rules()
        .and_then(|rules| rules.general.gui_checkbox_sound.clone());
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
    ally(state, house);
}

/// `Ally 0x00731840`: queue EVENT_ALLY toward `house` for the player's
/// House. Gates (`0x00731840..0x00731877`): a nonzero GameMode, the
/// Scenario's FixedAlliance bit (`0x400`) clear, AlliesAllowed and
/// AllyChangeAllowed. FixedAlliance has no VERA field: no skirmish or LAN
/// writer sets it (`SpecialClass::Init 0x006B8AE0` clears it).
/// RESIDUAL: a full OutList (128 records, `[0xA802C8]`) drops the event
/// natively; VERA's queue has no such cap.
pub(crate) fn ally(state: &mut AppState, house: InternedId) {
    let Some(local) = crate::app::input::commands::preferred_local_owner_name(state) else {
        return;
    };
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    let rules = &runtime.resources.rules;
    let sim = &mut runtime.simulation;
    if !sim.session().game_mode_nonzero()
        || !rules.general.allies_allowed
        || !rules.general.ally_change_allowed
    {
        return;
    }
    let Some(index) = sim.houses().index_of(house) else {
        return;
    };
    crate::app::input::commands::schedule_command_in_sim(
        sim,
        &local,
        crate::sim::command::Command::Ally {
            house_index: index as i32,
        },
    );
}

/// `AllianceCommandClass::Execute 0x00536F20` → `0x007318F0`: toggle the
/// alliance with the owner of the first selected object, when the player is
/// not defeated and that owner is another human House; then `Ally`'s gates.
pub(crate) fn toggle_alliance_hotkey(state: &mut AppState) {
    let first = crate::app::input::dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    )
    .first()
    .copied();
    let Some(owner) = state.match_state.sim_runtime.as_ref().and_then(|runtime| {
        let sim = &runtime.simulation;
        let local = sim.session().current_house()?;
        if sim.houses().get(&local)?.is_defeated() {
            return None;
        }
        let owner = sim.entities().get(first?)?.owner();
        (owner != local && sim.houses().get(&owner)?.is_human()).then_some(owner)
    }) else {
        return;
    };
    ally(state, owner);
}

/// A press on an enabled chat box: the checkbox proc flips its state, plays
/// GUICheckboxSound and notifies the dialog with the new state
/// (`0x0061670E..0x0061678B`); 0x73 writes it to the mask byte of the row's
/// House (`0x0065827A..0x006582C3`). A hidden or disabled box takes no click.
fn toggle_chat_box(state: &mut AppState, row: usize) {
    let Some(chat) = view(state).rows.get(row).and_then(|row| row.as_ref()?.chat) else {
        return;
    };
    if !chat.enabled {
        return;
    }
    let Some(index) = state.match_state.sim_runtime.as_ref().and_then(|runtime| {
        let sim = &runtime.simulation;
        let player = sim.houses().index_of(sim.session().current_house()?)?;
        // The writer maps the slot the same way (0x006582B5..0x006582BF).
        Some(house_for_row(row, player))
    }) else {
        return;
    };
    state
        .match_state
        .match_presentation
        .chat
        .set_listens(index, !chat.checked);
    let sound = state
        .rules()
        .and_then(|rules| rules.general.gui_checkbox_sound.clone());
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

/// A press on the TauntsOn box: the checkbox proc flips it only for a press
/// on its 18x18 icon (client x and y below `0x12`, `0x00616700..0x00616708`),
/// plays GUICheckboxSound and notifies with the new state; 0x73 writes it to
/// the mode's byte (`0x006582F9` LANTaunts, `0x00658313` WOLTaunts), live
/// for both send and receive. It is never disabled.
fn toggle_taunts_box(state: &mut AppState) {
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    if !crate::ui::skirmish_shell::checkbox_icon_rect(
        layout.taunts,
        crate::ui::shell::geom::ShellScale::RETAIL,
    )
    .contains(x, y)
    {
        return;
    }
    let mode = state.match_state.session_mode();
    let runtime = &mut state.frontend.multiplayer_shell_runtime;
    let Some(on) = runtime.taunts_on(mode) else {
        return;
    };
    runtime.set_taunts_on(mode, !on);
    let sound = state
        .rules()
        .and_then(|rules| rules.general.gui_checkbox_sound.clone());
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

#[cfg(test)]
#[path = "diplomacy_tests.rs"]
mod tests;
