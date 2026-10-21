//! The setup's Cooperative campaign (LAN, and WOL where wired: Use Map,
//! the host's join, Quick Co-op): MPCooperative's slots (vtable
//! `0x007EE27C`) at their native call sites in the setup, Choose Map, the
//! host's join and the guest's packets. The progress itself belongs to the
//! runtime's `CooperativeProgressState`; this module only reads the session
//! the slots see and applies their effects (the country refresh, a changed
//! scenario, the `C0` text).

use super::NetworkSetupRequest;
use crate::app::AppState;
use crate::app::frontend::skirmish_session::cooperative::{
    CooperativeCampaignText, CooperativeSlotContext,
};
use crate::skirmish_modes::MpModeClass;

/// What the slots read from the session: the scenario file names in catalog
/// order (`[0xA8B8CC]`), the nodes' names local first (`0xA8DA78`), the
/// current scenario (`[0xA8B254]`) and whether Choose Map is on top.
struct SlotInputs {
    scenarios: Vec<String>,
    nodes: Vec<String>,
    current: usize,
    chooser_on_top: bool,
}

impl SlotInputs {
    fn read(state: &AppState) -> Self {
        let records = state.frontend.scenario_catalog.records();
        let launch = state.match_state.session.network_setup();
        let mut nodes: Vec<String> = Vec::new();
        if let Some(launch) = launch {
            // Every machine's node 0 is its local player.
            nodes.push(launch.local_player().name.clone());
            nodes.extend(
                launch
                    .humans
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != launch.local_player_index)
                    .map(|(_, human)| human.name.clone()),
            );
        }
        Self {
            scenarios: records
                .iter()
                .map(|record| record.file_name.clone())
                .collect(),
            nodes,
            current: state.frontend.multiplayer_shell_runtime.scenario_index(),
            chooser_on_top: state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .is_some(),
        }
    }

    fn with<R>(&self, f: impl FnOnce(CooperativeSlotContext, &[&str]) -> R) -> R {
        let scenarios: Vec<&str> = self.scenarios.iter().map(String::as_str).collect();
        let nodes: Vec<&str> = self.nodes.iter().map(String::as_str).collect();
        f(
            CooperativeSlotContext {
                scenarios: &scenarios,
                nodes: &nodes,
                chooser_on_top: self.chooser_on_top,
            },
            &nodes,
        )
    }
}

fn is_cooperative(state: &AppState, mode_id: i32) -> bool {
    crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, mode_id)
        .is_some_and(|mode| mode.class == MpModeClass::Cooperative)
}

/// The current mode (`[0xA8B23C]`) is Cooperative.
pub(crate) fn current(state: &AppState) -> bool {
    state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.mode.class == MpModeClass::Cooperative)
}

/// `0x005E26C0`: a setup dialog (BC, BD, C2 or C9) exists. Quick Match's
/// setup has none.
fn setup_open(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| !protocol.quick_match)
}

/// `0x00775940(0xBC)`: the host's setup dialog is on the shell stack.
fn host_setup_open(state: &AppState) -> bool {
    setup_open(state) && state.match_state.session.host()
}

/// `0x005C2500`: every row's country becomes the stage's single country or
/// Random. Nothing changes without the campaign's stage. `+0x18`'s
/// per-node `+0x1C` re-check that follows always passes after it.
fn refresh_countries(state: &mut AppState) -> anyhow::Result<()> {
    refresh_rows(state, true)
}

/// `0x005C2500` over the rows; `ai_rows` false keeps the AI rows' countries,
/// as BC open's row loop (`0x005ED762`) restores them from the profile
/// (`0x004E3F70`) after its refreshes.
fn refresh_rows(state: &mut AppState, ai_rows: bool) -> anyhow::Result<()> {
    let runtime = &state.frontend.multiplayer_shell_runtime;
    if runtime.cooperative_setup_view().player_countries.is_none() {
        return Ok(());
    }
    let country = runtime.cooperative_default_country();
    state
        .match_state
        .session
        .reset_network_countries(country, ai_rows)?;
    state.platform.window.request_redraw();
    Ok(())
}

/// `+0x60`'s tail: the refresh it took, then the `C0` text to every guest.
/// A guest never sends one (its LAN 0x17 goes to nodes 1..N-1, which it
/// has none of in its own node list).
fn publish(state: &mut AppState, published: Option<(bool, Option<String>)>) -> anyhow::Result<()> {
    let Some((refresh, text)) = published else {
        return Ok(());
    };
    if refresh {
        refresh_countries(state)?;
    }
    if let Some(text) = text.filter(|_| state.match_state.session.host()) {
        super::actions::queue(state, NetworkSetupRequest::PublishCooperativeCampaign(text));
    }
    Ok(())
}

/// The host's BC open (`0x005ED6FE..0x005ED717`) in Cooperative, after the
/// profile restore chose the scenario (`+0x44` there): `+0x20`, then
/// `+0x24`, then `+0x18` with no chooser (row 0, no swap: `0x005E66D0`
/// reads a dead list), then `+0x7C`, which only repaints.
pub(super) fn host_opened(state: &mut AppState) -> anyhow::Result<()> {
    if !current(state) {
        return Ok(());
    }
    let inputs = SlotInputs::read(state);
    let current = inputs.current;
    let host_setup = host_setup_open(state);
    let selected = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        inputs.with(|context, _| {
            parts
                .progress
                .enter_mode(parts.registry, parts.rng, context, current, host_setup)
        })
    };
    if let Some(index) = selected.filter(|index| *index != current) {
        select_scenario(state, index, true)?;
    }
    refresh_rows(state, false)?;
    let inputs = SlotInputs::read(state);
    let refresh = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        parts.progress.request_refresh();
        inputs.with(|context, _| {
            parts.progress.accept_chooser_row(
                parts.registry,
                parts.rng,
                CooperativeSlotContext {
                    chooser_on_top: false,
                    ..context
                },
                0,
            )
        })
    };
    if refresh {
        refresh_rows(state, false)?;
    }
    Ok(())
}

/// `MPGameOptions__SelectScenario` (`0x005E7BF0`) from `+0x20`: the
/// scenario its record's first map names becomes the setup's map. It writes
/// only the map globals and the row count `[0xA8BB0C]`, keeping every row,
/// start and team. `bc_open` adds what BC's init runs after its helper,
/// the capacity pass `0x005EC3A0` at `0x005DED73`; ChooseMap's port carries
/// it.
/// RESIDUAL: that port also resets starts and teams (`0x005EE6A0`,
/// `0x005EE510`), which BC's init is not traced to run; its rows are the
/// defaults then.
fn select_scenario(state: &mut AppState, index: usize, bc_open: bool) -> anyhow::Result<()> {
    let Some(file) = state
        .frontend
        .scenario_catalog
        .records()
        .get(index)
        .map(|record| record.file_name.clone())
    else {
        return Ok(());
    };
    let Some(capacity) = state
        .frontend
        .scenario_catalog
        .shell_maps()
        .iter()
        .find(|map| map.file_name.eq_ignore_ascii_case(&file))
        .map(|map| map.player_capacity)
    else {
        log::warn!("Cooperative selected {file}, which has no loadable map entry");
        return Ok(());
    };
    if bc_open {
        let mode = state
            .match_state
            .session
            .network_setup()
            .expect("admitted setup")
            .mode
            .clone();
        let removed = state
            .match_state
            .session
            .select_network_map(mode, file.clone(), capacity)?;
        debug_assert!(removed.is_empty(), "a Cooperative map seats both players");
    } else {
        state
            .match_state
            .session
            .select_network_scenario(file.clone(), capacity)?;
    }
    // +0x20 writes [0xA8B254] before the selection.
    state
        .frontend
        .multiplayer_shell_runtime
        .set_scenario_index(index);
    let metadata = super::local_map_metadata(state, &file)?;
    state
        .match_state
        .session
        .accept_network_map_metadata(metadata, None, true)?;
    state.frontend.random_map_retention.select_map(&file);
    Ok(())
}

/// Choose Map's list in Cooperative (`+0x4C`): one `"%d. %s"` row per
/// campaign whose reserve's map is among `records` (the mode's valid
/// scenarios), with that record as the row's data. A game-type click
/// (`0x005E6C14..0x005E6C9B`) first rebuilds the reserves (`+0x0C` true,
/// `+0x10`). None when `mode_id` is not Cooperative or no setup is open.
pub(crate) fn chooser_rows(
    state: &mut AppState,
    mode_id: i32,
    records: &[usize],
    clicked: bool,
) -> Option<Vec<(usize, String)>> {
    if !is_cooperative(state, mode_id) || !setup_open(state) {
        return None;
    }
    let inputs = SlotInputs::read(state);
    let files: Vec<(usize, String)> = records
        .iter()
        .filter_map(|&index| {
            let record = state.frontend.scenario_catalog.records().get(index)?;
            Some((index, record.file_name.clone()))
        })
        .collect();
    let files: Vec<(usize, &str)> = files
        .iter()
        .map(|(index, file)| (*index, file.as_str()))
        .collect();
    let rows = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        if clicked {
            parts.progress.rebuild_reserves(parts.registry, parts.rng);
        }
        inputs.with(|_, nodes| {
            parts
                .progress
                .chooser_rows(parts.registry, parts.rng, nodes, &files)
        })
    };
    Some(
        rows.into_iter()
            .map(|row| {
                let key = state
                    .frontend
                    .multiplayer_shell_runtime
                    .cooperative_campaign_name(row.campaign)
                    .unwrap_or_default()
                    .to_owned();
                let name = crate::app::App::csf_label(state, &key, &key);
                (row.record, format!("{}. {}", row.campaign + 1, name))
            })
            .collect(),
    )
}

/// Use Map (`0x005E7160`) in Cooperative, after the setup took the map: a
/// mode change runs `+0x20` (its scenario choice is overwritten by Use
/// Map's own, `0x005E7371`), then `+0x24`, `+0x18` for the chosen row with
/// the chooser still on top, `+0x7C` and `+0x60`.
pub(super) fn map_used(state: &mut AppState, mode_changed: bool, row: usize) -> anyhow::Result<()> {
    if !current(state) {
        return Ok(());
    }
    let inputs = SlotInputs::read(state);
    let current = inputs.current;
    let host_setup = host_setup_open(state);
    let setup = setup_open(state);
    let refresh = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        inputs.with(|context, _| {
            if mode_changed {
                parts
                    .progress
                    .enter_mode(parts.registry, parts.rng, context, current, host_setup);
            }
            parts.progress.request_refresh();
            parts
                .progress
                .accept_chooser_row(parts.registry, parts.rng, context, row)
        })
    };
    if mode_changed || refresh {
        refresh_countries(state)?;
    }
    let published = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        parts.progress.publish(parts.registry, setup)
    };
    publish(state, published)
}

/// The host's join accept (LAN `0x005E1BF2`, WOL `0x007A6E5A`) →
/// `0x005E2FA0`: the chooser's
/// selected mode when Choose Map is on top, else the current one, gets
/// `+0x94`, which makes the record if needed, binds the two names and
/// publishes it. Ordinary modes' `+0x94` does nothing.
pub(super) fn guest_joined(state: &mut AppState) -> anyhow::Result<()> {
    let mode_id = match state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
    {
        Some(modal) => modal.selected_mode_id,
        None => match state.match_state.session.network_setup() {
            Some(launch) => launch.mode.id,
            None => return Ok(()),
        },
    };
    if !is_cooperative(state, mode_id) {
        return Ok(());
    }
    let inputs = SlotInputs::read(state);
    let setup = setup_open(state);
    let published = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        inputs.with(|_, nodes| parts.progress.join(parts.registry, parts.rng, nodes, setup))
    };
    publish(state, published)
}

/// The join's country (`0x005E1945..0x005E1973`): the requested country if
/// the current mode admits it (`+0x1C`), else the mode's default (`+0x28`).
/// Only Cooperative's slots differ from "any country".
pub(super) fn admit_join_country(
    state: &AppState,
    slot: &mut crate::skirmish_launch::SkirmishLocalSlot,
) {
    if !current(state) {
        return;
    }
    let runtime = &state.frontend.multiplayer_shell_runtime;
    let requested = if slot.country_random {
        -2
    } else {
        i32::from(slot.country.raw_index())
    };
    if runtime.cooperative_admits(requested) {
        return;
    }
    match runtime.cooperative_default_country() {
        Some(country) => {
            slot.country = country;
            slot.country_random = false;
        }
        None => slot.country_random = true,
    }
}

/// A received `C0` (`0x005E43CE`): every mode object's `+0x58`, which only
/// MPCooperative takes, then its `+0x5C`; their refresh repaints the
/// countries while a setup dialog is open. A text missing a field calls no
/// mode.
pub(super) fn campaign_received(state: &mut AppState, text: &str) -> anyhow::Result<()> {
    let Some(text) = CooperativeCampaignText::parse(text) else {
        return Ok(());
    };
    let inputs = SlotInputs::read(state);
    let setup = setup_open(state);
    let refresh = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        inputs.with(|_, nodes| {
            parts
                .progress
                .receive_campaign(parts.registry, parts.rng, nodes, &text);
        });
        parts.progress.receive_stage(&text, setup)
    };
    if refresh {
        refresh_countries(state)?;
    }
    Ok(())
}

/// An options parse that changes the mode (`0x005E541D..0x005E5466`):
/// the new mode's `+0x20`, then `+0x7C`, which only repaints. A setup
/// dialog's guest (BD, C9) and both Quick Co-op machines run it; none has
/// the host's setup on the shell stack. A missing record draws on
/// `[0xA8B254]`'s scenario. The Quick Co-op host's options name no map it
/// keeps: `+0x20` may select the first map's scenario
/// (`0x005C1B79..0x005C1BA1`), which the later lookup at `0x007A6E0B`
/// selects again. The countries refresh only with a setup dialog.
pub(super) fn mode_entered(state: &mut AppState) -> anyhow::Result<()> {
    if !current(state) {
        return Ok(());
    }
    let inputs = SlotInputs::read(state);
    let current = inputs.current;
    let host_setup = host_setup_open(state);
    let selected = {
        let parts = state.frontend.multiplayer_shell_runtime.cooperative_parts();
        inputs.with(|context, _| {
            parts
                .progress
                .enter_mode(parts.registry, parts.rng, context, current, host_setup)
        })
    };
    // RESIDUAL: a guest's own SelectScenario is not ported; the host's
    // options name its map.
    if let Some(index) = selected
        && state.match_state.session.host()
    {
        select_scenario(state, index, false)?;
    }
    if setup_open(state) {
        refresh_countries(state)?;
    }
    Ok(())
}
