//! A player's Detonate: the lone selected carrier's own rung of
//! `TechnoClass::What_Action_OnObject` (`0x006FFF8D..0x00700003`) and the
//! DETONATE event it sends (`EventClass::Execute` `0x004C7823`). Retail
//! rules set `[CombatDamage] CanDetonateTimeBomb=no`; a mod's `yes` is
//! layered over Hills' map.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

pub(crate) struct BombScene {
    pub(crate) rules: RuleSet,
    pub(crate) sim: Simulation,
    pub(crate) grid: PathGrid,
    pub(crate) player: InternedId,
    pub(crate) tank: u64,
}

/// The player's (the Americans') Crazy Ivan ordered to bomb the player's
/// own Grizzly; runs until the bomb is on it. `patch` layers over retail
/// rules.
pub(crate) fn own_bombed_tank(patch: &str) -> Option<BombScene> {
    let patch = IniFile::from_str(patch);
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)?
        .rules
        .with_fresh_serial();
    let mut sim = Simulation::with_seed(43);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    for (name, human) in [("Americans", true), ("Russians", false)] {
        let id = sim.interner.intern(name);
        let side = rules.country_side_index_raw(name).unwrap();
        sim.houses
            .register(HouseState::new(id, side, Some(id), human, 10_000, 10));
    }
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let player = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(player));
    let ivan = sim
        .spawn_object_at_height("IVAN", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("MTNK", "Americans", 12, 10, 0, 0, &rules)
        .unwrap();
    assert!(!sim.bomb_detonate_action(tank, &rules), "no bomb yet");
    sim.queue_command(CommandEnvelope::new(
        player,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: ivan,
            target_id: tank,
        },
    ));
    let mut scene = BombScene {
        rules,
        sim,
        grid,
        player,
        tank,
    };
    for _ in 0..150 {
        scene.step();
        if scene.armed() {
            return Some(scene);
        }
    }
    panic!("the Ivan never planted his bomb");
}

impl BombScene {
    pub(crate) fn step(&mut self) {
        let commands = self.sim.take_due_commands();
        self.sim
            .advance_tick(&commands, Some(&self.rules), Some(&self.grid), None, 33);
    }

    pub(crate) fn armed(&self) -> bool {
        self.sim
            .substrate
            .entities
            .get(self.tank)
            .is_some_and(|tank| tank.bomb.is_some())
    }

    /// The player's click on the lone selected tank over itself: the
    /// DETONATE event the input layer sends.
    pub(crate) fn click_detonate(&mut self) {
        let record = self
            .sim
            .encode_detonate_record(self.tank)
            .expect("a bombed carrier");
        self.sim.queue_command(CommandEnvelope::new(
            self.player,
            self.sim.session.tick() + 1,
            Command::SynchronizedRecord { record },
        ));
        self.step();
        self.step();
    }

    pub(crate) fn tank_alive(&self) -> bool {
        self.sim
            .substrate
            .entities
            .get(self.tank)
            .is_some_and(|tank| tank.lifecycle.object_alive() && tank.health.current() > 0)
    }
}

/// With `CanDetonateTimeBomb=yes` the player's own bombed Grizzly offers
/// Detonate over itself, and the click's DETONATE event sets the bomb off at
/// once, long before its 450-frame fuse: IvanDamage 450 destroys it.
#[test]
fn a_player_detonates_a_timed_bomb_on_click() {
    let Some(mut scene) = own_bombed_tank("[CombatDamage]\nCanDetonateTimeBomb=yes\n") else {
        return;
    };
    assert!(scene.sim.bomb_detonate_action(scene.tank, &scene.rules));
    assert!(scene.tank_alive());
    scene.click_detonate();
    assert!(!scene.armed());
    assert!(!scene.tank_alive(), "the blast destroys the Grizzly");
}

/// Retail's `CanDetonateTimeBomb=no` offers no Detonate, so the bomb waits
/// for its fuse.
#[test]
fn retail_rules_offer_no_detonate() {
    let Some(scene) = own_bombed_tank("") else {
        return;
    };
    assert!(!scene.rules.combat_damage.can_detonate_time_bomb);
    assert!(!scene.sim.bomb_detonate_action(scene.tank, &scene.rules));
}

/// Ares `TechnoExt::CanDetonateAttachedBomb @ 100433A0` (hook `0x006FFEC0`):
/// the planting weapon's `IvanBomb.CanDetonateTimeBomb=yes` offers Detonate
/// over retail's Rules `no`.
#[test]
fn the_planting_weapons_key_offers_detonate() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let Some(mut scene) = own_bombed_tank("[IvanBomber]\nIvanBomb.CanDetonateTimeBomb=yes\n")
    else {
        return;
    };
    assert!(!scene.rules.combat_damage.can_detonate_time_bomb);
    assert!(scene.sim.bomb_detonate_action(scene.tank, &scene.rules));
    scene.click_detonate();
    assert!(!scene.tank_alive());
}
