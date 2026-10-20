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
    match requested_state(request, state.match_state.session.mode(), mode_class) {
        RequestedState::Nothing => {}
        RequestedState::Diplomacy => {
            App::enter_in_game_menu_state(state, InGameMenuState::Diplomacy);
        }
        // RESIDUAL: State 9's ShowMissionRestateBriefing is a separate
        // chain; until it lands the request opens nothing.
        RequestedState::MissionBriefing => {
            log::info!("Diplomacy button requested the mission briefing (state 9), not ported");
        }
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
}

/// The rows and the headers they decide.
pub(crate) struct ResolvedRows {
    pub rows: [Option<DiplomacyRow>; DIPLOMACY_ROWS],
    /// 0x597: some row showed an Allies box at init.
    pub allies_header: bool,
    /// 0x425: some row is human-controlled.
    pub chat_header: bool,
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
    // Init 0x00657E8B: the player's own slot shows House 0, row 0 the player.
    let house_for_row = |row: usize| match row {
        0 => player_index,
        row if row == player_index => 0,
        row => row,
    };
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
        // RESIDUAL: the chat mask 0xA8D108 has no VERA owner; it holds its
        // Init_Game value 1 for every House. Only LAN/WOL human rows show
        // the box, and 0x00658525 hides it once the House sent EXIT
        // (House+0x248).
        let chat = (row > 0 && human && !state.left_game()).then_some(DiplomacyChatBox {
            checked: true,
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
    let mode = state.match_state.session.mode();
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
        // RESIDUAL: TauntsOn 0x730 shows only here, from [0xA8D110]/[0xA8D111],
        // which have no VERA owner; the box stays hidden.
    }
    view.map_name = if mode.is_network() {
        crate::app::shell_network_setup::map_display_name(state)
    } else {
        crate::app::frontend::skirmish_shell_render::skirmish_selected_map_name(state)
    }
    .unwrap_or_default()
    .to_string();
    view
}

fn layout(state: &AppState) -> Option<DiplomacyLayout> {
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
        // RESIDUAL: an enabled Allies box (a human row while the local
        // player is active, LAN/WOL only) would queue the ALLY event
        // (0x00731840, EventClass type 3); VERA's simulation has no ALLY
        // event yet, so the click does nothing. Disabled boxes take no
        // input, which covers every skirmish row.
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

#[cfg(test)]
#[path = "diplomacy_tests.rs"]
mod tests;
