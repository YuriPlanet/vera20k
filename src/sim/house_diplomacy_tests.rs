//! Rust regression tests of the live diplomacy through its production owner.
//! Expected graphs, anger and target drops follow the instruction reading of
//! CanAlly (`0x00501540`), MakeAlly (`0x004F9B70`), MakeEnemy (`0x004F9F90`)
//! and Computer_Paranoid (`0x00501640`); no native execution backs them.

use std::collections::BTreeSet;

use crate::map::houses::HouseAllianceMap;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// Retail's side layout: Neutral is the `Civilian` side's, Special the
/// `Mutant` side's, and both are MultiplayPassive.
const RULES: &str = "[MultiplayerDialogSettings]\nAlliesAllowed=yes\n\
     [Sides]\nGDI=Americans\nNod=Russians\nCivilian=Neutral\nMutant=Special\n\
     [Countries]\n0=Americans\n1=Russians\n2=Neutral\n3=Special\n\
     [Neutral]\nMultiplayPassive=yes\n[Special]\nMultiplayPassive=yes\n\
     [VehicleTypes]\n0=TANK\n[TANK]\nStrength=1000\n";

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(RULES)).expect("diplomacy rules")
}

/// Houses `(name, country, human)` in that array order, in a skirmish.
fn sim_with(rules: &RuleSet, houses: &[(&str, &str, bool)]) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::with_seed(9);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let ids = houses
        .iter()
        .map(|&(name, country, human)| {
            let id = sim.interner.intern(name);
            let side = rules.country_side_index(country).expect("country side").0;
            let country_id = sim.interner.intern(country);
            let mut house = HouseState::new(id, side, Some(country_id), human, 0, 10);
            house.multiplay_passive =
                crate::sim::house_state::resolve_multiplay_passive(Some(rules), Some(country));
            sim.houses.insert(id, house);
            sim.session.house_order.push(id);
            id
        })
        .collect();
    (sim, ids)
}

/// Each house's allies, by its graph key.
fn allies(sim: &Simulation, name: &str) -> BTreeSet<String> {
    sim.house_alliances
        .get(&name.to_ascii_uppercase())
        .cloned()
        .unwrap_or_default()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_ascii_uppercase()).collect()
}

fn paranoid(sim: &mut Simulation, rules: &RuleSet) {
    sim.computer_paranoid(rules, None, FrameEffects::default());
}

/// Each computer house allies the other and turns on the human. Neutral is
/// refused for its `Civilian` side (`0x0050158F`); Special for leaving the
/// computer no enemy (`0x00501630`). Neutral and Special, not human
/// themselves, ally both computers one way, and are passive to the human
/// (`0x004F9FB8`, `0x004F9FC9`).
#[test]
fn computer_paranoid_allies_the_computers_against_the_human() {
    let rules = rules();
    let (mut sim, ids) = sim_with(
        &rules,
        &[
            ("Player", "Americans", true),
            ("Computer1", "Russians", false),
            ("Computer2", "Russians", false),
            ("Neutral", "Neutral", false),
            ("Special", "Special", false),
        ],
    );
    paranoid(&mut sim, &rules);

    assert_eq!(allies(&sim, "Player"), set(&[]));
    assert_eq!(allies(&sim, "Computer1"), set(&["Computer2"]));
    assert_eq!(allies(&sim, "Computer2"), set(&["Computer1"]));
    assert_eq!(allies(&sim, "Neutral"), set(&["Computer1", "Computer2"]));
    assert_eq!(allies(&sim, "Special"), set(&["Computer1", "Computer2"]));
    let player = ids[0];
    for computer in &ids[1..3] {
        let house = &sim.houses[computer];
        assert_eq!(house.grudge_scores.get(&player), Some(&1));
        assert_eq!(house.enemy_house, Some(player));
    }
    for passive in &ids[3..] {
        assert!(sim.houses[passive].grudge_scores.is_empty());
        assert_eq!(sim.houses[passive].enemy_house, None);
    }

    // A second call allies no one anew and angers each computer again.
    paranoid(&mut sim, &rules);
    assert_eq!(allies(&sim, "Computer1"), set(&["Computer2"]));
    assert_eq!(sim.houses[&ids[1]].grudge_scores.get(&player), Some(&2));
}

/// A lone computer allies no one: Special would leave it without an enemy.
#[test]
fn a_lone_computer_allies_no_one() {
    let rules = rules();
    let (mut sim, _) = sim_with(
        &rules,
        &[
            ("Player", "Americans", true),
            ("Computer1", "Russians", false),
            ("Neutral", "Neutral", false),
            ("Special", "Special", false),
        ],
    );
    paranoid(&mut sim, &rules);
    assert_eq!(allies(&sim, "Computer1"), set(&[]));
    assert_eq!(allies(&sim, "Neutral"), set(&["Computer1"]));
    assert_eq!(allies(&sim, "Special"), set(&["Computer1"]));
}

/// The gate (`0x00501640..0x00501675`): Rules `AlliesAllowed=`, and no
/// computer slot with a team, which `Create_Houses` records
/// (`0x006881F5..0x00688209`).
#[test]
fn computer_paranoid_needs_allies_allowed_and_no_computer_team() {
    let mut rules = rules();
    rules.general.allies_allowed = false;
    let houses = [
        ("Player", "Americans", true),
        ("Computer1", "Russians", false),
        ("Computer2", "Russians", false),
    ];
    let (mut sim, ids) = sim_with(&rules, &houses);
    paranoid(&mut sim, &rules);
    assert!(sim.house_alliances.is_empty());
    assert!(sim.houses[&ids[1]].grudge_scores.is_empty());

    use crate::sim::scenario_bootstrap::{NormalizedSkirmishSlot, populate_launch_houses};
    use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition, LaunchTeam};
    let rules = self::rules();
    let slot = |name: &str, human, team| NormalizedSkirmishSlot {
        owner_name: name.to_string(),
        // Indices in this fixture's [Countries]: Americans 0, Russians 1.
        country: LaunchCountry::from_country_index(if human { 0 } else { 1 }),
        color_index: 0,
        start_position: LaunchStartPosition::Auto,
        team,
        is_human: human,
        observer: false,
        source_human_index: None,
        difficulty: crate::sim::house_state::HouseDifficulty::Normal,
    };
    for (human_team, computer_team, paranoid_runs) in [
        (LaunchTeam::Team(0), LaunchTeam::None, true),
        (LaunchTeam::None, LaunchTeam::Team(1), false),
    ] {
        let mut sim = Simulation::with_seed(9);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        populate_launch_houses(
            &mut sim,
            &[
                slot("Player", true, human_team),
                slot("Computer1", false, computer_team),
                slot("Computer2", false, LaunchTeam::None),
            ],
            &rules,
        );
        assert_eq!(sim.session.computer_slot_has_team(), !paranoid_runs);
        paranoid(&mut sim, &rules);
        assert_eq!(
            allies(&sim, "Computer1") == set(&["Computer2"]),
            paranoid_runs,
            "human team {human_team:?}, computer team {computer_team:?}"
        );
    }
}

/// MakeAlly's Logic walk (`0x004F9C7F..0x004F9D48`): a house's objects drop
/// a target its house now allies; others keep theirs.
#[test]
fn make_ally_drops_the_targets_on_the_new_ally() {
    let rules = rules();
    let (mut sim, _) = sim_with(
        &rules,
        &[
            ("Player", "Americans", true),
            ("Computer1", "Russians", false),
            ("Computer2", "Russians", false),
        ],
    );
    let spawn = |sim: &mut Simulation, owner: &str, rx| {
        sim.spawn_object_at_height("TANK", owner, rx, 10, 0, 0, &rules)
            .expect("tank spawns")
    };
    let first = spawn(&mut sim, "Computer1", 10);
    let second = spawn(&mut sim, "Computer2", 12);
    let player = spawn(&mut sim, "Player", 14);
    let aim = |sim: &mut Simulation, id, target| {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target)));
    };
    aim(&mut sim, first, second);
    aim(&mut sim, second, player);
    paranoid(&mut sim, &rules);
    let target = |id| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .attack_target
            .target()
    };
    assert_eq!(target(first), None);
    assert_eq!(
        target(second),
        Some(crate::sim::combat::TargetKind::Entity(player))
    );
}

/// Within a pass the remembered reveal viewers (`vision::RevealViewers`),
/// direct and allied, follow the ally graph's writers: MakeAlly and MakeEnemy
/// change them at once. Debug builds also check each remembered answer
/// against a fresh one.
#[test]
fn ally_writes_reach_the_remembered_reveal_viewers() {
    let rules = rules();
    let (mut sim, ids) = sim_with(
        &rules,
        &[
            ("Player", "Americans", true),
            ("Computer1", "Russians", false),
            ("Computer2", "Russians", false),
        ],
    );
    for &id in &ids {
        sim.fog
            .by_owner
            .get_or_insert_with(id, || crate::sim::vision::OwnerVisibility::new(8, 8));
    }
    let config = crate::sim::vision::VisionConfig::default();
    let viewers = |sim: &mut Simulation| {
        let direct = sim.reveal_viewers.direct(
            &sim.fog,
            &sim.house_alliances,
            ids[1],
            &config,
            &sim.interner,
        );
        let allied =
            sim.reveal_viewers
                .allied(&sim.fog, &sim.house_alliances, ids[1], &sim.interner);
        (direct.to_vec(), allied.to_vec())
    };
    let (alone, with_ally) = (vec![ids[1]], vec![ids[1], ids[2]]);
    sim.reveal_viewers.begin();
    assert_eq!(viewers(&mut sim), (alone.clone(), alone.clone()));
    sim.make_ally(ids[1], ids[2], &rules, None, FrameEffects::default());
    assert_eq!(viewers(&mut sim), (with_ally.clone(), with_ally));
    sim.make_enemy(ids[1], ids[2], &rules);
    assert_eq!(viewers(&mut sim), (alone.clone(), alone));
    sim.reveal_viewers.end();
}

/// MakeEnemy adds the anger first (`0x004F9FDD`), while the house is still
/// an ally the rescan skips, then breaks both sides (`0x004FA020`,
/// `0x004FA094`). The next call finds no alliance and names the enemy.
#[test]
fn make_enemy_angers_before_it_breaks_both_sides() {
    let rules = rules();
    let (mut sim, ids) = sim_with(
        &rules,
        &[
            ("Player", "Americans", true),
            ("Computer1", "Russians", false),
            ("Computer2", "Russians", false),
        ],
    );
    let both = HouseAllianceMap::from([
        ("COMPUTER1".to_string(), set(&["Computer2"])),
        ("COMPUTER2".to_string(), set(&["Computer1"])),
    ]);
    sim.install_house_alliances(
        crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(both),
        &rules,
    );
    let (first, second) = (ids[1], ids[2]);
    sim.make_enemy(first, second, &rules);
    assert_eq!(allies(&sim, "Computer1"), set(&[]));
    assert_eq!(allies(&sim, "Computer2"), set(&[]));
    assert_eq!(sim.houses[&first].grudge_scores.get(&second), Some(&1));
    assert_eq!(sim.houses[&first].enemy_house, None);
    assert!(sim.houses[&second].grudge_scores.is_empty());

    sim.make_enemy(first, second, &rules);
    assert_eq!(sim.houses[&first].grudge_scores.get(&second), Some(&2));
    assert_eq!(sim.houses[&first].enemy_house, Some(second));
}

/// TAction30's 0x0050C210 can call ComputerParanoid before clearing placement
/// scope. CanAlly 0x005015A1 admits that scope before defeated/count gates, while
/// MakeAlly 0x004F9C31 skips its live target/Paranoid tail. Initial saved-Allies
/// metadata and native GameMode 4 remain outside the current model.
#[test]
fn initial_scope_allies_keep_targets_and_can_ally_without_a_remaining_enemy() {
    let rules = rules();
    let (mut sim, ids) = sim_with(
        &rules,
        &[
            ("Computer1", "Russians", false),
            ("Computer2", "Russians", false),
        ],
    );
    let first = sim
        .spawn_object_at_height("TANK", "Computer1", 10, 10, 0, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object_at_height("TANK", "Computer2", 12, 10, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(second)));
    sim.with_object_placement_scope(|sim| {
        paranoid(sim, &rules);
        assert!(sim.object_placement_scope_active());
    });
    assert!(!sim.object_placement_scope_active());
    assert_eq!(allies(&sim, "Computer1"), set(&["Computer2"]));
    assert_eq!(allies(&sim, "Computer2"), set(&["Computer1"]));
    assert_eq!(
        sim.substrate
            .entities
            .get(first)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(second))
    );
    sim.with_object_placement_scope(|sim| sim.make_enemy(ids[0], ids[1], &rules));
    assert!(allies(&sim, "Computer1").is_empty());
    assert!(allies(&sim, "Computer2").is_empty());
}
