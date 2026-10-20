//! Ground receivers of CellClass::BlowUpBridge 0047DD70.
//!
//! Each live member supplies its current Health to the ordinary direct damage
//! receiver; death/lifecycle consequences finish before traversal resumes.
//! Evidence: HIGH_BRIDGE_RIM_REFRESH_ALGORITHM_GHIDRA_REPORT.md.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{
    EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TerrainDamageEvent,
};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

fn members(sim: &Simulation, rx: u16, ry: u16) -> impl Iterator<Item = CellObjectMember> + '_ {
    sim.cell_objects((rx, ry), MovementLayer::Ground)
}

/// `CellClass::BlowUpBridge @ 0x0047DD70`'s ground loop: each member's ReceiveDamage
/// with its own Health (`+0x6C`) and the C4 warhead, the next link taken first.
pub(super) fn apply(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    rx: u16,
    ry: u16,
    frame_effects: FrameEffects<'_>,
) {
    let warhead_ref = sim.interner.intern(&rules.bridge_warheads.c4_name);
    let mut current = members(sim, rx, ry).next();
    while let Some(member) = current {
        // 0047DD96 captures NextObject BEFORE the virtual ReceiveDamage. A
        // captured successor removed by nested effects has no next link when
        // its own iteration begins; do not resume from a precollected vector.
        let next = {
            let mut current_list = members(sim, rx, ry);
            current_list.find(|candidate| *candidate == member);
            current_list.next()
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
                        entity.health.current(),
                        0,
                        RAD_NO_ATTACKER,
                        None,
                        warhead_ref,
                        ReceiverCallFlags {
                            ignore_defenses: true,
                            arg6: true,
                        },
                    );
                    sim.commit_direct_damage_receiver(rules, registry, event, frame_effects);
                }
            }
            CellObjectMember::Terrain(stable_id) => {
                if let Some(terrain) = sim.terrain_objects.objects().get(&stable_id) {
                    let event = TerrainDamageEvent {
                        stable_id,
                        rx,
                        ry,
                        damage: terrain.health(),
                        distance_leptons: 0,
                        warhead_ref,
                        near_center_ic_isolation_eligible: false,
                    };
                    sim.commit_direct_terrain_damage_receiver(
                        rules,
                        registry,
                        event,
                        frame_effects,
                    );
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
            .construct_object_limbo_at_height(
                name,
                "Americans",
                x,
                4,
                0,
                0,
                rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        // Supply the fallout scene's pose after the constructor's zero Location.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: i32::from(x) * 256 + 128,
                y: 4 * 256 + 128,
                z: 0,
            });
        sim.reveal(id);
        id
    }

    fn stock_cascade_rules() -> RuleSet {
        // Retail RULESMD TERROR/TerrorBomb/TerrorBombWH, E1 and MTNK damage
        // inputs and their physical GI/TRST action records. Presentation
        // defaults cannot supply a native death clock or action admission.
        let art = IniFile::from_str(&format!(
            "{}\n{}",
            crate::rules::retail_ini_fixture::GI_ART_EXCERPT,
            crate::rules::retail_ini_fixture::TRST_ART_EXCERPT
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=TERROR\n[VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n[BuildingTypes]\n0=BUNK\n[CombatDamage]\nC4Warhead=Super\n\
             [E1]\nImage=GI\nStrength=125\nArmor=none\nSpeed=4\nDieSound=GIDie\n\
             [TERROR]\nImage=TRST\nStrength=75\nArmor=flak\nSpeed=6\nExplodes=yes\nDeathWeapon=TerrorBomb\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
             [BUNK]\nStrength=1000\nDieSound=BuildingDie\n\
             [TerrorBomb]\nDamage=225\nWarhead=TerrorBombWH\nSuicide=yes\n\
             [Warheads]\n0=Super\n1=TerrorBombWH\n\
             [Super]\nInfDeath=2\nPenetratesBunker=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TerrorBombWH]\nInfDeath=4\nCellSpread=2\nPercentAtMax=.5\n\
             Verses=150%,100%,100%,90%,50%,50%,100%,150%,30%,100%,100%\n",
        ), &art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        rules
    }

    #[test]
    fn bridge_ground_revisits_zero_health_gi_and_repeats_die_sound_without_double_score() {
        use crate::sim::world::SimSoundEvent;
        let rules = stock_cascade_rules();
        let mut sim = world(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, false, 1000, 10,
            ));
        let gi = place(&mut sim, &rules, "E1", 4);
        let terror = place(&mut sim, &rules, "TERROR", 4);
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![
                CellObjectMember::Entity(terror),
                CellObjectMember::Entity(gi),
            ]
        );
        apply(
            &mut sim,
            &rules,
            None,
            4,
            4,
            crate::sim::world::FrameEffects::default(),
        );
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bridge_zero_health_receiver.json",
        ))
        .unwrap();
        let repeat = native["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["initial_alive"] == false && case["die_sound_count"] == 1)
            .unwrap();
        assert_eq!(repeat["endpoint"], "007509E0");
        let sounds = sim
            .sound_events
            .iter()
            .filter(|event| {
                matches!(event,
                    SimSoundEvent::EntityDied { die_sound_id, .. } if die_sound_id == "GIDie"
                )
            })
            .count();
        assert_eq!(
            sounds,
            1 + repeat["rng_draws"].as_u64().unwrap() as usize,
            "initial fatal receiver plus the native zero-HP successor revisit"
        );
        // Object5F579A calls RecordKill before Destroy5F57AF; Infantry's
        // original vtable7EB058 +E0 is Techno702D40, whose UnitsLost store is
        // 7031AC. object_health.json executes the exact-zero E0/DC order and
        // the entry-HP0 no-callback arm; the Infantry repeat above reaches
        // only its later death tail. Both first deaths therefore count now,
        // including TERROR while its Die2 sequence remains in Logic.
        assert_eq!(
            sim.houses[&owner].stats().units_lost(),
            2,
            "both fatal callbacks count; the zero-health GI revisit adds no loss"
        );
        assert_eq!(
            sim.houses[&owner].stats().units_killed(),
            1,
            "only TerrorBomb's first GI kill is credited"
        );
        assert!(
            sim.substrate
                .entities
                .get(gi)
                .unwrap()
                .kill_record
                .is_recorded()
        );
        assert!(!sim.live_object_order_snapshot().contains(&gi));
        let head = sim.substrate.entities.get(terror).unwrap();
        assert!(!head.death.is_dying() && head.lifecycle.object_alive());
        assert_eq!(head.health.current(), 0);
        assert!(head.kill_record.is_recorded());
        assert!(sim.live_object_order_snapshot().contains(&terror));
        // Drive its actual Die2 clock through its ordinary turns and confirm
        // UnInit does not count either fatal callback again. This test does
        // not assert animation duration (completion calls UnInit520CAD).
        for frame in 1..=120 {
            if !sim
                .substrate
                .entities
                .get(terror)
                .unwrap()
                .lifecycle
                .object_alive()
            {
                break;
            }
            sim.session.set_binary_frame_for_test(frame);
            sim.advance_live_object_turn(terror, Some(&rules), Default::default())
                .expect("the corpse's ordinary turn completes");
        }
        let head = sim.substrate.entities.get(terror).unwrap();
        assert!(!head.lifecycle.object_alive());
        assert!(head.kill_record.is_recorded());
        assert!(!sim.live_object_order_snapshot().contains(&terror));
        assert_eq!(sim.houses[&owner].stats().units_lost(), 2);
        assert_eq!(sim.houses[&owner].stats().units_killed(), 1);
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
            .set_for_test(0);
        apply(
            &mut sim,
            &rules,
            None,
            4,
            4,
            crate::sim::world::FrameEffects::default(),
        );
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
                .object_alive()
        );
        assert!(sim.substrate.pending_delete.is_empty());
    }

    #[test]
    fn bridge_ground_forced_super_damages_an_occupied_tank_bunker() {
        // Stock Super has PenetratesBunker=yes. Without the ignoreDefenses
        // bypass, the linked Building arm incorrectly nullifies its damage.
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n0=TANK\n[AircraftTypes]\n\
             [BuildingTypes]\n0=NATBNK\n[TANK]\nStrength=100\nSpeed=4\n\
             [NATBNK]\nStrength=1000\nTankBunker=yes\n\
             [CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
             [Super]\nInfDeath=2\nPenetratesBunker=yes\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ),
            &IniFile::from_str("[NATBNK]\nFoundation=2x2\n"),
        )
        .unwrap();
        let mut sim = world(&rules);
        let bunker = place(&mut sim, &rules, "NATBNK", 3);
        let occupant = place(&mut sim, &rules, "TANK", 6);
        crate::sim::docking::bunker_link::install_bunker_link(&mut sim, bunker, occupant, &rules);
        assert_eq!(
            sim.substrate
                .entities
                .get(bunker)
                .unwrap()
                .bunker
                .occupant(),
            Some(occupant)
        );
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![CellObjectMember::Entity(bunker)]
        );
        apply(
            &mut sim,
            &rules,
            None,
            4,
            4,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate.entities.get(bunker).unwrap().health.current(),
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
            crate::sim::terrain_spawn::seed_terrain_animations(
                &mut sim,
                &[TerrainObject {
                    rx: 4,
                    ry: 4,
                    name: "TREE".into(),
                }],
                &rules,
                false,
            );
            let id = sim.terrain_objects.cell_index()[&(4, 4)];
            assert_eq!(
                members(&sim, 4, 4).collect::<Vec<_>>(),
                vec![CellObjectMember::Terrain(id)]
            );
            apply(
                &mut sim,
                &rules,
                None,
                4,
                4,
                crate::sim::world::FrameEffects::default(),
            );
            let tree = &sim.terrain_objects.objects()[&id];
            if wood {
                assert_eq!(tree.health(), 0);
                assert_eq!(tree.lifecycle(), TerrainObjectLifecycle::Destroyed);
                assert!(!tree.in_logic_vector());
                assert!(members(&sim, 4, 4).next().is_none());
                assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(4, 4), 0);
            } else {
                assert_eq!(tree.health(), 100);
                assert!(tree.is_live());
                assert_eq!(sim.terrain_objects.cell_index()[&(4, 4)], id);
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
        apply(
            &mut sim,
            &rules,
            None,
            4,
            4,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate.entities.get(head).unwrap().health.current(),
            0
        );
        assert_eq!(
            sim.substrate.entities.get(next).unwrap().health.current(),
            0
        );
        assert_eq!(
            sim.substrate.entities.get(tail).unwrap().health.current(),
            100
        );
        assert_eq!(
            members(&sim, 4, 4).collect::<Vec<_>>(),
            vec![CellObjectMember::Entity(tail)]
        );
    }

    #[test]
    fn bridge_ground_uses_membership_order_and_nonanchor_building_foundation() {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=BIG\n[E1]\nStrength=100\nSpeed=4\n\
             [BIG]\nStrength=1000\n\
             [CombatDamage]\nC4Warhead=KILL\n[Warheads]\n0=KILL\n\
             [KILL]\nInfDeath=3\nVerses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\n",
            ),
            &IniFile::from_str("[BIG]\nFoundation=2x1\n"),
        )
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
                .construct_object_limbo_at_height(
                    name,
                    "Americans",
                    x,
                    4,
                    0,
                    0,
                    &rules,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            // The unmarked contrast has this same pose without Cell membership.
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .position
                .put_location(crate::sim::components::DriveCoord {
                    x: i32::from(x) * 256 + 128,
                    y: 4 * 256 + 128,
                    z: 0,
                });
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
        apply(
            &mut sim,
            &rules,
            None,
            4,
            4,
            crate::sim::world::FrameEffects::default(),
        );
        for id in [older, newer, building] {
            assert_eq!(
                sim.substrate.entities.get(id).unwrap().health.current(),
                0,
                "receiver {id}"
            );
        }
        assert!(
            sim.substrate
                .entities
                .get(unmarked)
                .unwrap()
                .health
                .current()
                > 0
        );
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
