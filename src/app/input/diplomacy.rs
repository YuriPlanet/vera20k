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

use super::dialog_page::InGameDialog;
use crate::app::match_runtime::session::SessionMode;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::presentation::house_ui::HouseUiSources;
use crate::app::{App, AppState};
use crate::rules::rule_policy::RulePolicy;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::scenario_session::ExecutionTopology;
use crate::sim::world::Simulation;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::diplomacy::{
    self, DIPLOMACY_ROWS, DiplomacyAlliesBox, DiplomacyChatBox, DiplomacyControl, DiplomacyLayout,
    DiplomacyRow, DiplomacyView, diplomacy_layout,
};
use crate::ui::shell::page::PageEvent;

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
    topology: ExecutionTopology,
    rule_policy: RulePolicy,
) -> RequestedState {
    match request {
        DiplomacyRequest::Hotkey if rule_policy.uses_campaign_diplomacy() => {
            RequestedState::Nothing
        }
        DiplomacyRequest::Hotkey => RequestedState::Diplomacy,
        DiplomacyRequest::SidebarButton
            if rule_policy.uses_campaign_diplomacy()
                || (topology.is_lockstep()
                    && rule_policy.restates_mission_for_network_diplomacy()) =>
        {
            RequestedState::MissionBriefing
        }
        DiplomacyRequest::SidebarButton => RequestedState::Diplomacy,
    }
}

/// Write the requested state; `State_Machine` admits it on the way in.
pub(crate) fn open(state: &mut AppState, request: DiplomacyRequest) {
    let mode = state.match_state.session_mode();
    let (topology, rule_policy) = state.match_state.sim_runtime.runtime().map_or_else(
        || (mode.execution_topology(), mode.stock_rule_policy(None)),
        |runtime| {
            let session = runtime.simulation.session();
            (session.execution_topology(), session.rule_policy())
        },
    );
    if request == DiplomacyRequest::SidebarButton
        && topology.is_lockstep()
        && rule_policy.restates_mission_for_network_diplomacy()
        && crate::app::input::briefing_diplomacy::open(state)
    {
        return;
    }
    match requested_state(request, topology, rule_policy) {
        RequestedState::Nothing => {}
        RequestedState::Diplomacy => {
            App::enter_in_game_menu_state(state, InGameMenuState::Diplomacy);
        }
        RequestedState::MissionBriefing => crate::app::input::restate::open(state),
    }
}

/// The per-match switches the rows read besides the houses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiplomacyRules<'a> {
    /// `Rules+0x14B5` AlliesAllowed, layered through the mode INI.
    pub allies_allowed: bool,
    /// `[0xA8B244] == 2`: WOL Quick Co-op.
    pub quick_coop: bool,
    /// `[0xA8B538]`: the local player is defeated (or observing).
    pub local_inactive: bool,
    /// The chat mask `0xA8D108`, by House array index.
    pub listeners: &'a [bool],
}

/// The rows and the headers they decide.
pub(crate) struct ResolvedRows {
    pub rows: Vec<Option<DiplomacyRow>>,
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
        rows: Vec::new(),
        allies_header: false,
        chat_header: false,
    };
    let order = sim.houses().order();
    let rule_policy = sim.session().rule_policy();
    let Some(local) = sim.session().current_house() else {
        return empty;
    };
    let Some(player_index) = sim.houses().index_of(local) else {
        return empty;
    };
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
    let allies_shown =
        switches.allies_allowed && !sim.session().is_observer_house(local) && !switches.quick_coop;
    let mut allies_header = false;
    let mut chat_header = false;
    // VERA extends the native eight rows using the same House-order swap.
    let mut rows: Vec<_> = (0..order.len().max(DIPLOMACY_ROWS))
        .map(|row| {
            let house = *order.get(house_for_row(row))?;
            let state = sim.houses().get(&house)?;
            // Init hides rows 1..=7 whose House type lacks Multiplay; 0x00658330
            // blanks row 0's texts for such a type.
            if !multiplay(house) {
                return None;
            }
            let human = state.is_controlled_by_human(rule_policy);
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
            let is_observer = sim.session().is_observer_house(house);
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
                    .map(str::to_owned)
            } else {
                state
                    .country()
                    .and_then(|country| {
                        rules.country_flag_surface(sim.interner().resolve(country))
                    })
                    .or_else(|| {
                        state
                            .country()
                            .and_then(|country| {
                                rules.country_index(sim.interner().resolve(country))
                            })
                            .and_then(|index| {
                                crate::app::frontend::skirmish_shell_render::flag_pcx_for_side_item_data(
                                    i32::from(index.0),
                                )
                            })
                            .map(str::to_owned)
                    })
            };
            Some(DiplomacyRow {
                insignia,
                name: sources.ui_name(sim, house, state.country(), &country_name),
                rgb,
                // Σ of the two 20-entry per-victim kill tables (+0x53E4,
                // +0x5438), `%d`.
                kills: Some(state.stats().kills() as i32),
                // 0x00658330 hides a defeated House's box; nothing re-shows it.
                allies: allies.filter(|_| !state.is_defeated()),
                chat,
            })
        })
        .collect();
    // Neutral/Special often follow the playable Houses. They must not add a
    // blank page to the retail eight-row dialog.
    while rows.len() > DIPLOMACY_ROWS && rows.last().is_some_and(Option::is_none) {
        rows.pop();
    }
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
            .runtime()
            .map(|runtime| &runtime.simulation),
        state.rules(),
    ) else {
        return view;
    };
    let computer = state.csf_text("TXT_COMPUTER");
    let listeners: Vec<_> = (0..sim.houses().order().len())
        .map(|index| state.match_state.match_presentation.chat.listens(index))
        .collect();
    let switches = DiplomacyRules {
        allies_allowed: rules.general.allies_allowed,
        quick_coop: mode == SessionMode::Wol
            && state
                .match_state
                .session
                .network_wol_start()
                .is_some_and(|terms| terms.quick_coop),
        local_inactive: sim.local_player_defeated(),
        listeners: &listeners,
    };
    let resolved = resolve_rows(
        sim,
        rules,
        switches,
        &HouseUiSources::from_state(state, &computer),
        |country| crate::app::presentation::house_ui::country_display_name(state, sim, country),
    );
    let paging = state.match_state.match_presentation.diplomacy_page;
    let range = paging.range(resolved.rows.len(), DIPLOMACY_ROWS);
    view.first_row = range.start;
    view.page_index = paging.index(resolved.rows.len(), DIPLOMACY_ROWS);
    view.page_count = crate::ui::shell::paging::RowPage::count(resolved.rows.len(), DIPLOMACY_ROWS);
    view.rows =
        std::array::from_fn(|slot| resolved.rows.get(range.start + slot).cloned().flatten());
    view.allies_header = resolved.allies_header;
    view.chat_header = resolved.chat_header;
    if mode.execution_topology().is_lockstep() {
        // 0x00658126: the MPGameMode title.
        view.mode_name = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| {
                crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)
            })
            .map(|mode| state.csf_text(&mode.ui_name_key));
    }
    // 0x006580E0..0x0065810C: TauntsOn shows in LAN/WOL with LANTaunts /
    // WOLTaunts; the 250 ms refresh never touches it.
    view.taunts = state.frontend.multiplayer_shell_runtime.taunts_on(mode);
    view.map_name = if mode.execution_topology().is_lockstep() {
        crate::app::shell_network_setup::map_display_name(state)
    } else {
        crate::app::frontend::skirmish_shell_render::skirmish_selected_map_name(state)
    }
    .unwrap_or_default()
    .to_string();
    view
}

/// 0x73: title 0x694 and Return as the lower right-panel button.
pub(crate) const DIALOG: InGameDialog = InGameDialog {
    id: diplomacy::DIALOG,
    title: Some(diplomacy::TITLE_LABEL),
    lower: (0x423, diplomacy::RETURN_LABEL),
    label: "Active Diplomacy Shell 73",
    ui: super::dialog_page::shared_ui,
    ui_mut: super::dialog_page::shared_ui_mut,
    // The boxes play GUICheckboxSound where their proc cases do, below.
    own_checkbox_sound: true,
};

pub(crate) fn layout(state: &AppState) -> Option<DiplomacyLayout> {
    super::dialog_page::layout(state, diplomacy_layout)
}

/// One pointer message on 0x73 (`down`: `None` moves), over the rows as
/// they stand now.
pub(super) fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some(layout) = layout(state) else {
        return;
    };
    let view = view(state);
    let rect = |id| view.rect(&layout, id);
    let sync = |gui: &mut vera20k_gui::Gui, root| view.sync(&layout, gui, root);
    for event in DIALOG.route(state, down, diplomacy::spec, rect, sync) {
        let (PageEvent::Activated(id) | PageEvent::Toggled(id, _)) = event else {
            continue;
        };
        match DiplomacyControl::from_id(id) {
            // 0x006581AD: Return ends the modal; State_Machine leaves state 8.
            Some(DiplomacyControl::Return) => {
                App::enter_in_game_menu_state(state, InGameMenuState::Closed);
            }
            Some(control @ (DiplomacyControl::PreviousPage | DiplomacyControl::NextPage)) => {
                state.match_state.match_presentation.diplomacy_page.change(
                    control == DiplomacyControl::NextPage,
                    view.page_count * DIPLOMACY_ROWS,
                    DIPLOMACY_ROWS,
                );
                state.platform.window.request_redraw();
            }
            Some(DiplomacyControl::Chat(row)) => toggle_chat_box(state, &view, row),
            Some(DiplomacyControl::Allies(row)) => press_allies_box(state, &view, row),
            Some(DiplomacyControl::Taunts) => toggle_taunts_box(state),
            _ => {}
        }
    }
}

/// A press on an enabled Allies box. The checkbox proc flips the box and
/// plays GUICheckboxSound; dialog 0x73 flips it straight back
/// (`0x00658245..0x00658261`) and calls `Ally` with the row's House
/// (`0x00658269`). The box shows the sim's mask again at the next refresh,
/// so VERA, which draws the box from the mask, keeps no box state. A hidden
/// or disabled box takes no click.
fn press_allies_box(state: &mut AppState, view: &DiplomacyView, row: usize) {
    let logical_row = view.first_row + row;
    let Some(house) = state.match_state.sim_runtime.runtime().and_then(|runtime| {
        let sim = &runtime.simulation;
        let player = sim.houses().index_of(sim.session().current_house()?)?;
        sim.houses()
            .order()
            .get(house_for_row(logical_row, player))
            .copied()
    }) else {
        return;
    };
    play_checkbox_sound(state);
    ally(state, house);
}

fn play_checkbox_sound(state: &mut AppState) {
    let sound = state
        .rules()
        .and_then(|rules| rules.general.gui_checkbox_sound.clone());
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

/// `Ally 0x00731840`: queue EVENT_ALLY toward `house` for the player's
/// House. Gates (`0x00731840..0x00731877`): a nonzero GameMode, the
/// Scenario's FixedAlliance bit (`0x400`) clear, AlliesAllowed and
/// AllyChangeAllowed. VERA's room adds its In-game Allying checkbox
/// (`GameOptions::ally_change_allowed`) beside the rules' AllyChangeAllowed.
/// FixedAlliance is set only by a WOL clan game's
/// Apply_Squad_Alliances (`0x0069B170`); `SpecialClass::Init 0x006B8AE0`
/// clears it.
/// Sim-scale deviation: a full OutList (128 records, `[0xA802C8]`) drops the
/// event natively; VERA's far larger queue refuses the click instead.
pub(crate) fn ally(state: &mut AppState, house: InternedId) {
    let Some(local) = crate::app::input::commands::preferred_command_owner(state) else {
        return;
    };
    let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    let rules = &runtime.resources.rules;
    let sim = &mut runtime.simulation;
    if !alliance_change_allowed(sim.session(), rules) {
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

fn alliance_change_allowed(
    session: &crate::sim::scenario_session::ScenarioSession,
    rules: &RuleSet,
) -> bool {
    session.rule_policy().allows_manual_alliances()
        && !session.fixed_alliance()
        && rules.general.allies_allowed
        && rules.general.ally_change_allowed
        && session.game_options().ally_change_allowed
}

/// `AllianceCommandClass::Execute 0x00536F20` → `0x007318F0`: toggle the
/// alliance with the owner of the first selected object, when the player is
/// not defeated and that owner is another human House; then `Ally`'s gates.
pub(crate) fn toggle_alliance_hotkey(state: &mut AppState) {
    let first = crate::app::input::dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    )
    .first()
    .copied();
    let Some(owner) = state.match_state.sim_runtime.runtime().and_then(|runtime| {
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
fn toggle_chat_box(state: &mut AppState, view: &DiplomacyView, row: usize) {
    let logical_row = view.first_row + row;
    let Some(chat) = view.rows.get(row).and_then(|row| row.as_ref()?.chat) else {
        return;
    };
    let Some(index) = state.match_state.sim_runtime.runtime().and_then(|runtime| {
        let sim = &runtime.simulation;
        let player = sim.houses().index_of(sim.session().current_house()?)?;
        // The writer maps the slot the same way (0x006582B5..0x006582BF).
        Some(house_for_row(logical_row, player))
    }) else {
        return;
    };
    state
        .match_state
        .match_presentation
        .chat
        .set_listens(index, !chat.checked);
    play_checkbox_sound(state);
}

/// A press on the TauntsOn box: the checkbox proc flips it only for a press
/// on its 18x18 icon (client x and y below `0x12`, `0x00616700..0x00616708`),
/// plays GUICheckboxSound and notifies with the new state; 0x73 writes it to
/// the mode's byte (`0x006582F9` LANTaunts, `0x00658313` WOLTaunts), live
/// for both send and receive. It is never disabled.
fn toggle_taunts_box(state: &mut AppState) {
    let mode = state.match_state.session_mode();
    let runtime = &mut state.frontend.multiplayer_shell_runtime;
    let Some(on) = runtime.taunts_on(mode) else {
        return;
    };
    runtime.set_taunts_on(mode, !on);
    play_checkbox_sound(state);
}

#[cfg(all(test, tests_app_input_d_z))]
#[path = "diplomacy_tests.rs"]
mod tests;
