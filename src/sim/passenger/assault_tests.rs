//! The `Assaulter=` arms of `BuildingClass::CanBeOccupiedBy`
//! (`0x00457DAD..0x00457DD3`) and `InfantryClass::AddGarrisonOccupant`
//! (`0x00522A09..0x00522A45`), through a player's Capture order. Retail
//! rules leave every infantry type unassaulting; a mod's `[BORIS]
//! Assaulter=yes` is layered over Hills' map, and Boris's retail `AKM`
//! names `AssaultAnim=UCBLOOD`.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

pub(crate) struct AssaultScene {
    pub(crate) rules: RuleSet,
    pub(crate) sim: Simulation,
    pub(crate) grid: PathGrid,
    pub(crate) boris: u64,
    pub(crate) bunker: u64,
    pub(crate) occupants: Vec<u64>,
}

/// A Russian battle bunker held by two Russian conscripts, and the
/// Americans' Boris ordered to capture it. `extra` is layered over the
/// assaulter patch.
pub(crate) fn ordered_assault(extra: &str) -> Option<AssaultScene> {
    let patch = IniFile::from_str(&format!("[BORIS]\nAssaulter=yes\n{extra}"));
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)?
        .rules
        .with_fresh_serial();
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let object = rules.object("BORIS").unwrap();
    assert!(object.assaulter && !object.occupier);
    assert_eq!(
        rules.weapon("AKM").unwrap().assault_anim.as_deref(),
        Some("UCBLOOD")
    );

    let mut sim = Simulation::with_seed(41);
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
    let spawn = |sim: &mut Simulation, kind: &str, owner: &str, rx: u16, ry: u16| {
        sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, &rules)
            .unwrap_or_else(|| panic!("retail {kind} spawns"))
    };
    let bunker = spawn(&mut sim, "NABNKR", "Russians", 14, 10);
    let occupants: Vec<u64> = (0..2)
        .map(|i| spawn(&mut sim, "E2", "Russians", 12, 13 + i))
        .collect();
    for &occupant in &occupants {
        assert!(
            crate::sim::passenger::add_garrison_occupant(
                &mut sim,
                &rules,
                occupant,
                bunker,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap()
        );
    }
    let boris = spawn(&mut sim, "BORIS", "Americans", 11, 10);
    let americans = sim.interner.intern("Americans");
    sim.queue_command(CommandEnvelope::new(
        americans,
        sim.session.tick() + 1,
        Command::CaptureBuilding {
            engineer_id: boris,
            target_building_id: bunker,
        },
    ));
    Some(AssaultScene {
        rules,
        sim,
        grid,
        boris,
        bunker,
        occupants,
    })
}

impl AssaultScene {
    /// Step until the bunker holds nobody, at most 300 frames.
    pub(crate) fn run_until_cleared(&mut self) {
        for _ in 0..300 {
            let commands = self.sim.take_due_commands();
            self.sim
                .advance_tick(&commands, Some(&self.rules), Some(&self.grid), None, 33);
            if self.occupant_count() == 0 {
                return;
            }
        }
        panic!("Boris never cleared the bunker");
    }

    pub(crate) fn occupant_count(&self) -> u32 {
        self.sim
            .substrate
            .entities
            .get(self.bunker)
            .map_or(0, |bunker| bunker.passenger_role.occupants().count() as u32)
    }

    pub(crate) fn stats(&self, house: &str) -> (u32, u32) {
        let id = self.sim.interner.get(house).unwrap();
        let stats = self.sim.houses.get(&id).unwrap().stats();
        (stats.kills(), stats.losses())
    }
}

/// Boris, an assaulter, walks into the enemy bunker and kills both
/// occupants in it: each gets a UCBLOOD splash at its port, both leave the
/// game, the bunker stays Russian and empty, and Boris stays out, alive.
/// gamemd books the kills the wrong way round (`0x004586D6` calls the
/// assaulter's Record_The_Kill with the occupant as the killer): the
/// Russians score two kills, the Americans two losses.
#[test]
fn an_assaulter_kills_the_occupants_of_an_enemy_building() {
    let Some(mut scene) = ordered_assault("") else {
        return;
    };
    scene.run_until_cleared();
    let sim = &scene.sim;
    for &occupant in &scene.occupants {
        assert!(
            sim.substrate
                .entities
                .get(occupant)
                .is_none_or(|entity| !entity.lifecycle.object_alive()),
            "occupant {occupant} killed"
        );
    }
    let bunker = sim.substrate.entities.get(scene.bunker).unwrap();
    assert_eq!(sim.interner.resolve(bunker.owner()), "Russians");
    let boris = sim.substrate.entities.get(scene.boris).unwrap();
    assert!(boris.lifecycle.object_alive() && !boris.passenger_role.is_inside_transport());
    assert_eq!(boris.category(), EntityCategory::Infantry);
    let splashes = sim
        .substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id()) == "UCBLOOD")
        .count();
    assert_eq!(splashes, 2);
    assert_eq!(scene.stats("Russians"), (2, 0));
    assert_eq!(scene.stats("Americans"), (0, 2));
}
