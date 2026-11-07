//! The band box's owner gate is the owning House's `IsHumanPlayer`
//! (`0x004AC2BE` in the band callback `0x004AC2B0`), not identity with the
//! local player's House. SOV04's Premier Romanov belongs to the Germans, a
//! `PlayerControl=yes` House in a campaign, and the original game drags a box
//! over him exactly as over a Russian conscript.

use super::*;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::Simulation;

/// The box the retail-coordinate fixtures use: it covers units standing on
/// cells (10,10), (12,10) and (14,10).
const BOX: (f32, f32, f32, f32) = (-40.0, 300.0, 160.0, 400.0);

struct Cast {
    local: InternedId,
    player_controlled: InternedId,
    computer: InternedId,
}

/// The local human House, a second House the player controls and a plain
/// computer House. `second_is_human` makes the second House a human player in
/// its own right, the multiplayer shape.
fn world(campaign: bool, second_is_human: bool) -> (Simulation, Cast) {
    let mut sim = Simulation::new();
    let local = sim.intern("Russians");
    let player_controlled = sim.intern("Germans");
    let computer = sim.intern("YuriCountry");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::from_legacy_nonzero(!campaign));
    sim.houses_mut_for_test()
        .register(HouseState::new(local, 0, None, true, 0, 10));
    let mut second = HouseState::new(player_controlled, 1, None, second_is_human, 0, 10);
    second.set_player_control_for_test(true);
    sim.houses_mut_for_test().register(second);
    sim.houses_mut_for_test()
        .register(HouseState::new(computer, 2, None, false, 0, 10));
    spawn_mobile(&mut sim, 1, 10, 10, player_controlled, "RMNV");
    spawn_mobile(&mut sim, 2, 12, 10, local, "E2");
    spawn_mobile(&mut sim, 3, 14, 10, computer, "INIT");
    (
        sim,
        Cast {
            local,
            player_controlled,
            computer,
        },
    )
}

fn spawn_mobile(
    sim: &mut Simulation,
    sid: u64,
    rx: u16,
    ry: u16,
    owner: InternedId,
    type_id: &str,
) {
    let type_ref = sim.intern(type_id);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    // Revealed onto the map — the picker refuses limbo objects.
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.entities_mut_for_test().insert(entity);
}

fn encounter_order(sim: &Simulation) -> Vec<u64> {
    sim.entities()
        .values()
        .map(|entity| entity.stable_id())
        .collect()
}

fn band(source: impl PickSource, sim: &Simulation, local_owner: Option<&str>) -> SelectionMutation {
    let order = encounter_order(sim);
    compute_box_selection_snapshot_with_playfield(
        source,
        &order,
        &order,
        &[],
        None,
        local_owner,
        BOX.0,
        BOX.1,
        BOX.2,
        BOX.3,
        false,
        None,
        Some(sim.houses()),
        Some(sim.interner()),
        false,
    )
    .expect("the box covers drawn objects")
}

#[test]
fn campaign_band_box_admits_a_player_controlled_house_like_the_players_own() {
    let (sim, houses) = world(true, false);
    assert!(sim.house_is_human_player(houses.local));
    assert!(sim.house_is_human_player(houses.player_controlled));
    assert!(!sim.house_is_human_player(houses.computer));

    let snapshot = band(&sim, &sim, Some("Russians"));
    assert!(snapshot.clear);
    assert_eq!(
        snapshot.select,
        vec![1, 2],
        "Romanov joins the conscript; the computer unit stays out"
    );
}

#[test]
fn multiplayer_band_box_keeps_to_the_saved_current_house() {
    // Nonzero game mode: `0x50B6F0` compares the House with `A83D4C` alone,
    // so another human's House is refused even with PlayerControl set.
    let (sim, houses) = world(false, true);
    assert!(!sim.house_is_human_player(houses.player_controlled));

    let snapshot = band(&sim, &sim, Some("Russians"));
    assert_eq!(snapshot.select, vec![2]);
}

#[test]
fn band_box_without_a_local_player_stays_open_to_every_owner() {
    let (sim, _) = world(true, false);
    let snapshot = band(&sim, &sim, None);
    assert_eq!(snapshot.select, vec![1, 2, 3]);
}

#[test]
fn bare_store_band_box_keeps_exact_local_owner_identity() {
    // A source without a house table answers no `IsHumanPlayer`; the headless
    // fixtures keep collecting the local owner's objects and nothing else.
    let (sim, _) = world(true, false);
    let snapshot = band(sim.entities(), &sim, Some("Russians"));
    assert_eq!(snapshot.select, vec![2]);
}
