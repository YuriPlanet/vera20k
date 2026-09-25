//! Ground receivers of CellClass::BlowUpBridge 0047DD70.
//!
//! Each live member supplies its current Health to the ordinary direct damage
//! receiver; death/lifecycle consequences finish before traversal resumes.
//! Evidence: HIGH_BRIDGE_RIM_REFRESH_ALGORITHM_GHIDRA_REPORT.md.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{
    EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TerrainDamageEvent,
};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::Simulation;

fn members(sim: &Simulation, rx: u16, ry: u16) -> impl Iterator<Item = CellObjectMember> + '_ {
    sim.substrate.occupancy.cell_objects(
        rx,
        ry,
        MovementLayer::Ground,
        sim.production.terrain_object_cells.get(&(rx, ry)).copied(),
    )
}

pub(super) fn apply(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
) {
    let warhead_ref = sim.interner.intern(&rules.bridge_warheads.c4_name);
    let mut current = members(sim, rx, ry).next();
    while let Some(member) = current {
        // 0047DD96 captures NextObject BEFORE the virtual ReceiveDamage. A
        // captured successor removed by nested effects has no next link when
        // its own iteration begins; do not resume from a precollected vector.
        let next = {
            let mut live = members(sim, rx, ry);
            live.find(|candidate| *candidate == member)
                .and_then(|_| live.next())
        };
        match member {
            CellObjectMember::Entity(stable_id) => {
                if let Some(entity) = sim.substrate.entities.get(stable_id) {
                    // Native passes &Health. For retail C4Warhead=Super, forced
                    // damage reaches the ordinary fatal Object path, which
                    // writes HP0 before callbacks with either input location.
                    // Override-only early packet writes are NOT equivalent:
                    // see bridge_health_alias.py and the report's open residual.
                    let event = EntityDamageEvent::direct_receiver(
                        stable_id,
                        i32::from(entity.health.current),
                        0,
                        RAD_NO_ATTACKER,
                        None,
                        warhead_ref,
                        ReceiverCallFlags {
                            ignore_defenses: true,
                            arg6: true,
                        },
                    );
                    sim.commit_direct_damage_receiver(rules, registry, event);
                }
            }
            CellObjectMember::Terrain(stable_id) => {
                if let Some(terrain) = sim.production.terrain_objects.get(&stable_id) {
                    let event = TerrainDamageEvent {
                        stable_id,
                        rx,
                        ry,
                        damage: terrain.health,
                        distance_leptons: 0,
                        warhead_ref,
                        near_center_ic_isolation_eligible: false,
                    };
                    sim.commit_direct_terrain_damage_receiver(rules, registry, event);
                }
            }
        }
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::world::LifecycleTestEvent;

    fn world(rules: &RuleSet) -> Simulation {
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let cells = (0..8)
            .flat_map(|y| {
                (0..8).map(move |x| {
                    crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                })
            })
            .collect();
        sim.resolved_terrain =
            Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(8, 8, cells));
        sim
    }

    fn place(sim: &mut Simulation, rules: &RuleSet, name: &str, x: u16) -> u64 {
        let id = sim
            .construct_object_limbo_at_height(name, "Americans", x, 4, 0, 0, rules)
            .unwrap();
        sim.reveal(id);
        id
    }

    fn stock_cascade_rules() -> RuleSet {
        // Retail RULESMD TERROR/TerrorBomb/TerrorBombWH, E1 and MTNK damage
        // inputs; unrelated art/voice/debris definitions are omitted.
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=TERROR\n[VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n[BuildingTypes]\n0=BUNK\n[CombatDamage]\nC4Warhead=Super\n\
             [E1]\nStrength=125\nArmor=none\nSpeed=4\nDieSound=GIDie\n\
             [TERROR]\nStrength=75\nArmor=flak\nSpeed=6\nExplodes=yes\nDeathWeapon=TerrorBomb\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
             [BUNK]\nStrength=1000\nFoundation=1x1\nDieSound=BuildingDie\n\
             [TerrorBomb]\nDamage=225\nWarhead=TerrorBombWH\nSuicide=yes\n\
             [Warheads]\n0=Super\n1=TerrorBombWH\n\
             [Super]\nInfDeath=2\nPenetratesBunker=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TerrorBombWH]\nInfDeath=4\nCellSpread=2\nPercentAtMax=.5\n\
             Verses=150%,100%,100%,90%,50%,50%,100%,150%,30%,100%,100%\n",
        ))
        .unwrap()
    }

    #[test]
    fn bridge_ground_revisits_zero_health_gi_and_repeats_die_sound_without_double_score() {
        use crate::sim::world::SimSoundEvent;
        let rules = stock_cascade_rules();
        let mut sim = world(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, false, 1000, 10),
        );
        let gi = place(&mut sim, &rules, "E1", 4);
        let terror = place(&mut sim, &rules, "TERROR", 4);
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![
                CellObjectMember::Entity(terror),
                CellObjectMember::Entity(gi),
            ]
        );
        apply(&mut sim, &rules, None, 4, 4);
        let native: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/spatial_oracle/bridge_zero_health_receiver.json"
        )))
        .unwrap();
        let repeat = native["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["initial_alive"] == false && case["die_sound_count"] == 1)
            .unwrap();
        assert_eq!(repeat["endpoint"], "007509E0");
        let sounds = sim.sound_events.iter().filter(|event| matches!(event,
            SimSoundEvent::EntityDied { die_sound_id, .. } if sim.interner.resolve(*die_sound_id) == "GIDie"
        )).count();
        assert_eq!(
            sounds,
            1 + repeat["rng_draws"].as_u64().unwrap() as usize,
            "initial fatal receiver plus the native zero-HP successor revisit"
        );
        assert_eq!(
            sim.houses[&owner].stats.units_lost, 1,
            "the retired GI is counted once; Terrorist still has its death sequence"
        );
        assert_eq!(
            sim.houses[&owner].stats.units_killed, 1,
            "only TerrorBomb's first GI kill is credited"
        );
        assert!(sim.substrate.entities.get(gi).unwrap().destruction_recorded);
        assert!(!sim.live_object_order_snapshot().contains(&gi));
        let head = sim.substrate.entities.get(terror).unwrap();
        assert!(head.infantry_terminal.is_some());
        assert!(!head.destruction_recorded);
        // The rules catalog supplies a default Die2 even without art input.
        // Drive its normal terminal visits through retirement before checking
        // the eventual total; this test does not assert animation duration.
        for _ in 0..120 {
            if sim
                .substrate
                .entities
                .get(terror)
                .unwrap()
                .destruction_recorded
            {
                break;
            }
            assert!(sim.visit_infantry_terminal(terror, Some(&rules), Default::default()));
        }
        assert!(
            sim.substrate
                .entities
                .get(terror)
                .unwrap()
                .destruction_recorded
        );
        assert_eq!(sim.houses[&owner].stats.units_lost, 2);
        assert_eq!(sim.houses[&owner].stats.units_killed, 1);
    }

    #[test]
    fn bridge_ground_revisited_terrorist_executes_its_death_weapon_again() {
        let rules = stock_cascade_rules();
        let mut sim = world(&rules);
        let tank = place(&mut sim, &rules, "MTNK", 4);
        let next = place(&mut sim, &rules, "TERROR", 4);
        let head = place(&mut sim, &rules, "TERROR", 4);
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![
                CellObjectMember::Entity(head),
                CellObjectMember::Entity(next),
                CellObjectMember::Entity(tank),
            ]
        );
        apply(&mut sim, &rules, None, 4, 4);
        // Two original deaths leave the heavy tank alive. The captured
        // successor's repeated DeathWeapon kills it; its unlinked NextObject
        // cannot pass it a later direct C4 receiver.
        assert_eq!(sim.substrate.entities.get(tank).unwrap().health.current, 0);
    }

    #[test]
    fn bridge_ground_zero_health_building_skips_the_techno_death_tail() {
        use crate::sim::world::SimSoundEvent;
        let rules = stock_cascade_rules();
        let mut sim = world(&rules);
        let building = place(&mut sim, &rules, "BUNK", 4);
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .health
            .current = 0;
        apply(&mut sim, &rules, None, 4, 4);
        assert!(
            !sim.sound_events
                .iter()
                .any(|event| matches!(event, SimSoundEvent::EntityDied { .. }))
        );
        assert!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .lifecycle
                .object_alive
        );
        assert!(sim.substrate.pending_delete.is_empty());
    }

    #[test]
    fn bridge_ground_infantry_action_preserves_same_sequence_and_retired_membership() {
        use crate::sim::animation::{Animation, SequenceKind};
        use crate::sim::world::InfantryDeathSequence;
        let rules = stock_cascade_rules();
        let mut sim = world(&rules);
        let gi = place(&mut sim, &rules, "E1", 4);
        let mut animation = Animation::new(SequenceKind::Die2);
        animation.frame_index = 7;
        sim.substrate.entities.get_mut(gi).unwrap().animation = Some(animation);
        sim.begin_infantry_death_sequence(gi, InfantryDeathSequence::Die2);
        assert_eq!(
            sim.substrate
                .entities
                .get(gi)
                .unwrap()
                .animation
                .as_ref()
                .unwrap()
                .frame_index,
            7
        );
        sim.uninit_with_rules(gi, &rules);
        let pending = sim.substrate.pending_delete.clone();
        sim.begin_infantry_death_sequence(gi, InfantryDeathSequence::Die1);
        let object = sim.substrate.entities.get(gi).unwrap();
        assert_eq!(
            object.animation.as_ref().unwrap().sequence,
            SequenceKind::Die1
        );
        assert_eq!(object.animation.as_ref().unwrap().frame_index, 0);
        assert!(object.infantry_terminal.is_none());
        assert!(!object.lifecycle.object_alive);
        assert!(!sim.live_object_order_snapshot().contains(&gi));
        assert_eq!(sim.substrate.pending_delete, pending);
    }

    #[test]
    fn bridge_ground_forced_super_damages_an_occupied_tank_bunker() {
        // Stock Super has PenetratesBunker=yes. Without the ignoreDefenses
        // bypass, the linked Building arm incorrectly nullifies its damage.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=TANK\n[AircraftTypes]\n\
             [BuildingTypes]\n0=NATBNK\n[TANK]\nStrength=100\nSpeed=4\n\
             [NATBNK]\nStrength=1000\nFoundation=2x2\nTankBunker=yes\n\
             [CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
             [Super]\nInfDeath=2\nPenetratesBunker=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let mut sim = world(&rules);
        let bunker = place(&mut sim, &rules, "NATBNK", 3);
        let occupant = place(&mut sim, &rules, "TANK", 6);
        crate::sim::docking::bunker_link::install_bunker_link(&mut sim, bunker, occupant, &rules);
        assert_eq!(
            sim.substrate.entities.get(bunker).unwrap().bunker_occupant,
            Some(occupant)
        );
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![CellObjectMember::Entity(bunker)]
        );
        apply(&mut sim, &rules, None, 4, 4);
        assert_eq!(
            sim.substrate.entities.get(bunker).unwrap().health.current,
            0
        );
    }

    #[test]
    fn bridge_ground_dispatches_terrain_and_keeps_stock_wood_gate() {
        use crate::map::overlay::TerrainObject;
        use crate::sim::terrain_object::TerrainObjectLifecycle;
        for wood in [false, true] {
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [TerrainTypes]\n0=TREE\n[TREE]\nStrength=100\nTemperateOccupationBits=7\n\
                 [CombatDamage]\nC4Warhead=C4\n[Warheads]\n0=C4\n\
                 [C4]\nWood={}\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
                if wood { "yes" } else { "no" },
            )))
            .unwrap();
            let mut sim = world(&rules);
            crate::sim::terrain_spawn::seed_terrain_spawners(
                &mut sim,
                &[TerrainObject {
                    rx: 4,
                    ry: 4,
                    name: "TREE".into(),
                }],
                &rules,
                false,
            );
            let id = sim.production.terrain_object_cells[&(4, 4)];
            assert_eq!(
                members(&sim, 4, 4).collect::<Vec<_>>(),
                vec![CellObjectMember::Terrain(id)]
            );
            apply(&mut sim, &rules, None, 4, 4);
            let tree = &sim.production.terrain_objects[&id];
            if wood {
                assert_eq!(tree.health, 0);
                assert_eq!(tree.lifecycle, TerrainObjectLifecycle::Destroyed);
                assert!(!tree.in_logic_vector);
                assert!(members(&sim, 4, 4).next().is_none());
                assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
            } else {
                assert_eq!(tree.health, 100);
                assert!(tree.is_live());
                assert_eq!(sim.production.terrain_object_cells[&(4, 4)], id);
            }
        }
    }

    #[test]
    fn bridge_ground_stops_after_nested_death_removes_captured_successor() {
        // Head's synchronous DeathWeapon removes the captured next member.
        // The tail survives that warhead and must not receive a later C4 call
        // through a stale precollected member vector.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=NEXT\n1=TAIL\n[VehicleTypes]\n0=HEAD\n\
             [AircraftTypes]\n[BuildingTypes]\n[CombatDamage]\nC4Warhead=Super\n\
             [HEAD]\nStrength=100\nArmor=light\nSpeed=4\nExplodes=yes\nDeathWeapon=Boom\n\
             [NEXT]\nStrength=1\nArmor=none\nSpeed=4\n\
             [TAIL]\nStrength=100\nArmor=heavy\nSpeed=4\n\
             [Boom]\nDamage=100\nWarhead=EXP\n[Warheads]\n0=Super\n1=EXP\n\
             [Super]\nInfDeath=2\nPenetratesBunker=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [EXP]\nCellSpread=0.5\nPercentAtMax=1\nInfDeath=3\n\
             Verses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let mut sim = world(&rules);
        let tail = place(&mut sim, &rules, "TAIL", 4);
        let next = place(&mut sim, &rules, "NEXT", 4);
        let head = place(&mut sim, &rules, "HEAD", 4);
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![
                CellObjectMember::Entity(head),
                CellObjectMember::Entity(next),
                CellObjectMember::Entity(tail),
            ]
        );
        apply(&mut sim, &rules, None, 4, 4);
        assert_eq!(sim.substrate.entities.get(head).unwrap().health.current, 0);
        assert_eq!(sim.substrate.entities.get(next).unwrap().health.current, 0);
        assert_eq!(
            sim.substrate.entities.get(tail).unwrap().health.current,
            100
        );
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![CellObjectMember::Entity(tail)]
        );
    }

    #[test]
    fn bridge_ground_uses_membership_order_and_nonanchor_building_foundation() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=BIG\n[E1]\nStrength=100\nSpeed=4\n\
             [BIG]\nStrength=1000\nFoundation=2x1\n\
             [CombatDamage]\nC4Warhead=KILL\n[Warheads]\n0=KILL\n\
             [KILL]\nInfDeath=3\nVerses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let cells = (0..8)
            .flat_map(|y| {
                (0..8).map(move |x| {
                    crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                })
            })
            .collect();
        sim.resolved_terrain =
            Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(8, 8, cells));
        let mut create = |name, x, marked| {
            let id = sim
                .construct_object_limbo_at_height(name, "Americans", x, 4, 0, 0, &rules)
                .unwrap();
            if marked {
                sim.reveal(id);
            }
            id
        };
        let older = create("E1", 4, true);
        let newer = create("E1", 4, true);
        let building = create("BIG", 3, true);
        let unmarked = create("E1", 4, false);
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![
                CellObjectMember::Entity(newer),
                CellObjectMember::Entity(older),
                CellObjectMember::Entity(building),
            ]
        );
        apply(&mut sim, &rules, None, 4, 4);
        for id in [older, newer, building] {
            assert_eq!(
                sim.substrate.entities.get(id).unwrap().health.current,
                0,
                "receiver {id}"
            );
        }
        assert!(sim.substrate.entities.get(unmarked).unwrap().health.current > 0);
        let order: Vec<_> = sim
            .lifecycle_test_events_for_test()
            .iter()
            .filter_map(|event| {
                if let LifecycleTestEvent::UninitClassPre { stable_id } = event {
                    [older, newer].contains(stable_id).then_some(*stable_id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            order,
            vec![newer, older],
            "native cell-list order, not ascending stable IDs"
        );
    }
}
