//! `UnitClass::UpdateVisceroid @ 0x00737180` through the Unit AI. Retail
//! rules name `[General] SmallVisceroid=VISC_SML` and `LargeVisceroid=VISC_LRG`
//! but define neither type; a mod's sections are layered over Hills' map.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

const VISCEROIDS: &str = "\
[VehicleTypes]
900=VISC_SML
901=VISC_LRG
[VISC_SML]
SmallVisceroid=yes
Strength=200
Speed=4
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[VISC_LRG]
LargeVisceroid=yes
Strength=500
Speed=4
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
";

pub(crate) struct VisceroidScene {
    pub(crate) rules: RuleSet,
    pub(crate) sim: Simulation,
    pub(crate) grid: PathGrid,
    pub(crate) first: u64,
    pub(crate) second: u64,
}

/// Two small visceroids of a computer house in neighbouring cells.
pub(crate) fn two_small_visceroids() -> Option<VisceroidScene> {
    let patch = IniFile::from_str(VISCEROIDS);
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)?
        .rules
        .with_fresh_serial();
    assert!(rules.object("VISC_SML").unwrap().small_visceroid);
    let mut sim = Simulation::with_seed(47);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let id = sim.interner.intern("Russians");
    let side = rules.country_side_index_raw("Russians").unwrap();
    sim.houses
        .register(HouseState::new(id, side, Some(id), false, 0, 10));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let first = sim
        .spawn_object_at_height("VISC_SML", "Russians", 12, 12, 0, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object_at_height("VISC_SML", "Russians", 13, 12, 0, 0, &rules)
        .unwrap();
    Some(VisceroidScene {
        rules,
        sim,
        grid,
        first,
        second,
    })
}

impl VisceroidScene {
    pub(crate) fn step(&mut self) {
        let commands = self.sim.take_due_commands();
        self.sim
            .advance_tick(&commands, Some(&self.rules), Some(&self.grid), None, 33);
    }

    pub(crate) fn type_name(&self, id: u64) -> Option<String> {
        let entity = self.sim.substrate.entities.get(id)?;
        entity
            .lifecycle
            .object_alive()
            .then(|| self.sim.interner.resolve(entity.type_ref()).to_owned())
    }

    pub(crate) fn cell(&self, id: u64) -> Option<(u16, u16)> {
        let entity = self.sim.substrate.entities.get(id)?;
        Some((entity.position.rx(), entity.position.ry()))
    }

    /// The visceroid whose NavCom is the other one.
    pub(crate) fn seeker(&self) -> Option<u64> {
        [(self.first, self.second), (self.second, self.first)]
            .into_iter()
            .find(|&(id, other)| {
                self.sim
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|entity| entity.navigation.nav_com())
                    == Some(NavTargetRef::object(other))
            })
            .map(|(id, _)| id)
    }
}

/// Two small visceroids side by side: the first one's update finds the
/// other in a neighbour cell and sends it over (vt+0x480(this, 1)), and
/// Can_Enter_Cell lets it into that cell (`0x0073F5B5`); gamemd never merges
/// them, so both are still small visceroids, in one cell.
#[test]
fn small_visceroids_seek_each_other_and_stay_small() {
    let Some(mut scene) = two_small_visceroids() else {
        return;
    };
    scene.step();
    assert!(
        scene.seeker().is_some(),
        "one visceroid heads for the other"
    );
    for _ in 0..150 {
        scene.step();
        if scene.cell(scene.first) == scene.cell(scene.second) {
            break;
        }
    }
    assert_eq!(scene.cell(scene.first), scene.cell(scene.second));
    for id in [scene.first, scene.second] {
        assert_eq!(scene.type_name(id).as_deref(), Some("VISC_SML"));
    }
}
