//! Active offline Diplomacy73: Tab6ABC40 and sidebar65393A enter state8,
//! whose48C998 caller opens6586D0/657DA0. Existing modal ownership supplies
//! pause, input capture, cursor, audio and resume. No second alliance graph.

use crate::app::match_runtime::session::SessionMode;
use crate::app::{App, AppState};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::diplomacy::{DiplomacyLayout, DiplomacyRow, diplomacy_layout};
use winit::event::MouseButton;

pub(crate) fn open(state: &mut AppState) {
    // Tab6ABC40 returns in mode0. Sidebar653946 instead requests the still
    // separate campaign briefing9; this module owns the noncampaign state8.
    if state.match_state.session.mode() == SessionMode::Campaign
        || super::campaign_input::is_locked(state)
    {
        return;
    }
    App::enter_in_game_menu_state(state, InGameMenuState::Diplomacy);
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

/// Refresh658640 puts the controlled House in row0, swapping the original
/// House0 into that House's row. Preserve encounter order; do not sort kills.
pub(super) fn row_owners(sim: &Simulation, rules: &RuleSet) -> Vec<crate::sim::intern::InternedId> {
    let mut owners = sim.session.house_order.clone();
    if let Some(local) = owners
        .iter()
        .position(|owner| sim.house_is_human_player(*owner))
    {
        owners.swap(0, local);
    }
    owners.retain(|owner| {
        sim.houses.get(owner).is_some_and(|house| {
            !house.multiplay_passive
                && house.country.is_some_and(|country| {
                    rules.country_is_multiplayer(sim.interner.resolve(country))
                })
        })
    });
    owners
}

pub(crate) fn rows(state: &AppState) -> Vec<DiplomacyRow> {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return Vec::new();
    };
    let sim = &runtime.simulation;
    let rules = &runtime.resources.rules;
    let colors = &state.match_state.match_presentation.house_color_map;
    let computer = crate::app::frontend::shell_pass::resolve_csf(state, "TXT_COMPUTER");
    row_owners(sim, rules)
        .into_iter()
        .map(|owner| {
            let house = &sim.houses[&owner];
            let owner_name = sim.interner.resolve(owner);
            let is_local = sim.house_is_human_player(owner);
            let scheme = if house.is_defeated {
                // Original6583B2 resolves literal Grey, not a fitted RGB constant.
                crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, "Grey")
                    .and_then(|entry| u8::try_from(entry).ok())
                    .map(crate::rules::house_colors::HouseColorIndex)
            } else {
                colors.get(owner_name).copied()
            }
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
            let local_name = is_local
                .then(|| state.match_state.session.local_player_name())
                .flatten();
            DiplomacyRow {
                name: crate::app::match_runtime::sim_tick::house_display_name(
                    house.is_human,
                    local_name,
                    &computer,
                    owner_name,
                ),
                rgb: crate::rules::house_colors::house_text_rgb(&rules.color_schemes, scheme),
                kills: house.stats.kills() as i32,
                allied: sim.session.current_house.is_some_and(|local| {
                    crate::map::houses::is_allied_with(
                        &sim.house_alliances,
                        sim.interner.resolve(local),
                        owner_name,
                    )
                }),
                // Row0 has no alliance control; refresh6584CF hides one
                // for a defeated House. Offline computer controls are inert.
                alliance_visible: !is_local && !house.is_defeated,
                flag_country: house
                    .country
                    .and_then(|country| rules.country_index(sim.interner.resolve(country)))
                    .map(|country| i32::from(country.0)),
            }
        })
        .collect()
}

fn over_return(state: &AppState) -> Option<()> {
    let layout = layout(state)?;
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    layout.back.contains(x, y).then_some(())
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let over = over_return(state);
    if let Some(dialog) = state.match_state.match_presentation.diplomacy.as_mut() {
        dialog.buttons.hovered = over;
    }
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let over = over_return(state);
    let Some(dialog) = state.match_state.match_presentation.diplomacy.as_mut() else {
        return;
    };
    dialog.buttons.hovered = over;
    if pressed {
        dialog.buttons.press(over);
        if over.is_some() {
            App::play_skirmish_shell_generic_click_sound(state);
        }
    } else if dialog.buttons.release(over).is_some() {
        // Callback6581AD only accepts Return423, then closes to state0.
        App::enter_in_game_menu_state(state, InGameMenuState::Closed);
    }
}

pub(crate) fn wheel(state: &mut AppState, lines: f32) {
    let count = rows(state).len();
    if let Some(dialog) = state.match_state.match_presentation.diplomacy.as_mut() {
        dialog.scroll(count, -(lines * 3.0) as i32);
    }
    state.platform.window.request_redraw();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diplomacy_uses_country_multiplay_and_swaps_local_without_sorting_kills() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n2=Neutral\n\
             [Americans]\nMultiplay=yes\n[Russians]\nMultiplay=yes\n\
             [Neutral]\nMultiplay=no\nMultiplayPassive=no\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let a = sim.interner.intern("First");
        let b = sim.interner.intern("Local");
        let c = sim.interner.intern("Neutral");
        let america = sim.interner.intern("Americans");
        let russia = sim.interner.intern("Russians");
        let neutral = sim.interner.intern("Neutral");
        for (id, country, human) in [(a, russia, false), (b, america, true), (c, neutral, false)] {
            let mut house =
                crate::sim::house_state::HouseState::new(id, 0, Some(country), human, 0, 10);
            house.stats = crate::sim::house_state::MatchStatistics::from_totals_for_test(
                if id == a { 100 } else { 0 },
                0,
                0,
                0,
                0,
            );
            sim.houses.insert(id, house);
        }
        sim.session.house_order = vec![a, b, c];
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(b);
        assert_eq!(row_owners(&sim, &rules), vec![b, a]);
    }
}
