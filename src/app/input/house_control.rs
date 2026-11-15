//! VERA house control on the in-game Diplomacy page (`docs/house-control.md`).
//! Under each House's name a line says who plays it (a player, the AI at its
//! level, nobody); a click steps it to the next one through
//! `Command::SetHouseControl`. Below the last House two lines add an AI House
//! to the running match, as an enemy of all or as the player's ally
//! (`Command::AddHouse`).

use crate::app::AppState;
use crate::sim::command::Command;
use crate::sim::house_control::HouseControl;
use crate::sim::house_state::HouseDifficulty;
use crate::ui::shell::diplomacy::{
    DIPLOMACY_ROWS, DiplomacyControl, DiplomacyLayout, DiplomacyView,
};
use crate::ui::shell::geom::RectPx;

/// What a cell does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    /// The House at this House-array index and who controls it now.
    Control { house: usize, control: HouseControl },
    /// A new AI House: an ally of the player's, or an enemy of all.
    Add { ally: bool },
}

thread_local! {
    /// The last change asked for per House: (House index, its control when
    /// asked, the control asked for). A paused match runs a change only on
    /// resume; until then the line shows the request and clicks step on.
    static REQUESTED: std::cell::RefCell<Vec<(usize, Option<HouseControl>, HouseControl)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The controls in the order a click steps through them.
const CYCLE: [HouseControl; 5] = [
    HouseControl::Players,
    HouseControl::StockAi(HouseDifficulty::Easy),
    HouseControl::StockAi(HouseDifficulty::Normal),
    HouseControl::StockAi(HouseDifficulty::Hard),
    HouseControl::Nobody,
];

/// The control House `house` shows: a change asked for and not run yet
/// counts as done.
fn shown_control(state: &AppState, house: usize) -> Option<HouseControl> {
    let sim = &state.match_state.sim_runtime.runtime()?.simulation;
    let id = *sim.houses().order().get(house)?;
    let control = sim.house_control(id);
    REQUESTED
        .with_borrow(|requested| {
            requested
                .iter()
                .find(|(h, before, _)| *h == house && *before == control)
                .map(|(_, _, wanted)| Some(*wanted))
        })
        .unwrap_or(control)
}

/// The cells on the page shown, with their rects (window pixels).
pub(crate) fn cells(
    state: &AppState,
    layout: &DiplomacyLayout,
    view: &DiplomacyView,
) -> Vec<(Cell, RectPx)> {
    let Some(rt) = state.match_state.sim_runtime.runtime() else {
        return Vec::new();
    };
    let sim = &rt.simulation;
    let Some(player_index) = sim
        .session()
        .current_house()
        .and_then(|id| sim.houses().index_of(id))
    else {
        return Vec::new();
    };
    let below = |rect: RectPx| RectPx::new(rect.x, rect.y + rect.h, rect.w + rect.w / 2, rect.h);
    let mut out = Vec::new();
    for slot in 0..DIPLOMACY_ROWS {
        if view.rows[slot].is_none() {
            continue;
        }
        let house = super::diplomacy::house_for_row(view.first_row + slot, player_index);
        let Some(&id) = sim.houses().order().get(house) else {
            continue;
        };
        if sim
            .houses()
            .get(&id)
            .is_none_or(|h| h.multiplay_passive() || h.is_defeated())
        {
            continue;
        }
        let Some(control) = shown_control(state, house) else {
            continue;
        };
        let name = layout.control_rect(DiplomacyControl::Name(slot));
        out.push((Cell::Control { house, control }, below(name)));
    }
    // The add lines go on the first free row below the last House.
    if let Some(slot) = view
        .rows
        .iter()
        .rposition(Option::is_some)
        .map(|last| last + 1)
        .filter(|&slot| slot < DIPLOMACY_ROWS)
    {
        let name = layout.control_rect(DiplomacyControl::Name(slot));
        out.push((Cell::Add { ally: false }, name));
        out.push((Cell::Add { ally: true }, below(name)));
    }
    out
}

/// The text a cell shows.
pub(crate) fn text(state: &AppState, cell: &Cell) -> String {
    let csf = state.process_assets.csf.as_ref();
    let label = |key, fallback| crate::i18n::label(csf, key, fallback);
    match cell {
        Cell::Control { control, .. } => {
            let who = match control {
                HouseControl::Players => label("VERA:HouseControlPlayer", "Player"),
                HouseControl::Nobody => label("VERA:HouseControlNobody", "Nobody"),
                HouseControl::StockAi(HouseDifficulty::Easy) => label("GUI:AIEasy", "Easy AI"),
                HouseControl::StockAi(HouseDifficulty::Normal) => {
                    label("GUI:AINormal", "Medium AI")
                }
                HouseControl::StockAi(HouseDifficulty::Hard) => label("GUI:AIHard", "Brutal AI"),
            };
            format!("  {who}")
        }
        Cell::Add { ally: false } => label("VERA:AddAiHouseEnemy", "+ Add AI House (enemy)"),
        Cell::Add { ally: true } => label("VERA:AddAiHouseAlly", "+ Add AI House (ally)"),
    }
}

/// A left click at (x, y) on the page; `true` when it was one of the cells.
pub(crate) fn click(state: &mut AppState, x: i32, y: i32) -> bool {
    let Some(layout) = super::diplomacy::layout(state) else {
        return false;
    };
    let view = super::diplomacy::view(state);
    let Some((cell, _)) = cells(state, &layout, &view)
        .into_iter()
        .find(|(_, rect)| rect.contains(x, y))
    else {
        return false;
    };
    let Some(sim) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
    else {
        return true;
    };
    let command = match cell {
        Cell::Control { house, control } => {
            let Ok(index) = u8::try_from(house) else {
                return true;
            };
            let at = CYCLE.iter().position(|c| *c == control).unwrap_or(0);
            let next = CYCLE[(at + 1) % CYCLE.len()];
            let before = sim
                .houses()
                .order()
                .get(house)
                .and_then(|&id| sim.house_control(id));
            REQUESTED.with_borrow_mut(|requested| {
                requested.retain(|(h, _, _)| *h != house);
                requested.push((house, before, next));
            });
            // Players: this player takes the House.
            let sender = if next == HouseControl::Players {
                sim.local_house_control_sender()
            } else {
                -1
            };
            Command::SetHouseControl {
                house: index,
                control: next,
                sender,
            }
        }
        Cell::Add { ally } => {
            let ally = if ally {
                sim.session()
                    .current_house()
                    .and_then(|id| sim.houses().index_of(id))
                    .and_then(|i| i8::try_from(i).ok())
                    .unwrap_or(-1)
            } else {
                -1
            };
            // A country picked here; the record carries it to every peer.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let all = crate::skirmish_launch::LaunchCountry::ALL;
            Command::AddHouse {
                country: all[nanos as usize % all.len()].raw_index(),
                ally,
                control: HouseControl::StockAi(HouseDifficulty::Normal),
                sender: -1,
            }
        }
    };
    issue(state, command);
    true
}

/// Send `command` as the local House, as any order is sent.
fn issue(state: &mut AppState, command: Command) {
    let Some(rt) = state
        .match_state
        .sim_runtime
        .write_sim(crate::app::match_runtime::sim_host::SimWrite::Command)
    else {
        return;
    };
    let sim = &mut rt.simulation;
    let Some(owner) = sim
        .session()
        .current_house()
        .map(|id| sim.interner().resolve(id).to_owned())
    else {
        return;
    };
    log::info!("House control: {command:?}");
    let _ = super::commands::schedule_command_in_sim(sim, &owner, command);
}
