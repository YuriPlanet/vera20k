//! Complete-frame regressions for the GADEPT/MTNK waiter route (#937).
//!
//! Retail Battle/AnyTown rules and fixed ART use their production readers.
//! Immediate construction uses the same constructor/Reveal/Mark owners as
//! production delivery; Queue/Place and naturally damaged health are covered
//! separately by the saved release map observation. The flat arena and direct
//! damage call below are prepared inputs, not a whole native match comparison.
//!
//! Native ordering: Building43F180 -> PlaceDown5683C0 -> AddContent47E8A0 ->
//! Building453D60 marks ground0x80; Foot4DAEDC retries70D7E0 after its own
//! Process. Original admission controls: refinery_dock.{json,md}. Repair
//! cadence/payment/release controls: building_repair.depot_service.{json,md}.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::command::Command;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::occupancy::BUILDING_OCCUPATION_BIT;
use crate::sim::world::Simulation;

fn linked(sim: &Simulation, depot: u64, tank: u64) -> bool {
    sim.substrate
        .entities
        .get(depot)
        .unwrap()
        .radio_contacts
        .contains(tank)
        && sim
            .substrate
            .entities
            .get(tank)
            .unwrap()
            .radio_contacts
            .contains(depot)
}

fn advance(sim: &mut Simulation, rules: &RuleSet) {
    sim.advance_tick(&[], Some(rules), None, None, 1000 / 15);
}

#[test]
fn retail_shipyard_repairs_and_releases_a_damaged_destroyer() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 31);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        32,
        (0..19).flat_map(|y| (0..19).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .add_credits(20_000);
    sim.spawn_object("GAPOWR", "Americans", 22, 22, 0, rules)
        .unwrap();
    // Keep both houses alive while the ordinary frame clock runs service.
    sim.spawn_object("NAPOWR", "Russians", 27, 27, 0, rules)
        .unwrap();
    let yard = sim
        .spawn_object("GAYARD", "Americans", 10, 10, 0, rules)
        .unwrap();
    let ship = sim
        .spawn_object("DEST", "Americans", 16, 12, 0, rules)
        .unwrap();
    for _ in 0..3 {
        advance(&mut sim, rules);
    }
    let ap = sim.interner.intern("AP");
    sim.commit_direct_damage_receiver(
        rules,
        None,
        EntityDamageEvent::direct_receiver(
            ship,
            200,
            0,
            0,
            None,
            ap,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        crate::sim::world::FrameEffects::default(),
    );
    let strength = rules.object("DEST").unwrap().strength;
    assert_eq!(
        sim.substrate.entities.get(ship).unwrap().health.current(),
        strength - 200
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::clicked_mission(
            ship,
            MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(yard)
        ),
        Some(rules),
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(ship)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::object(yard)),
        "the successful docking handshake must install the repair destination"
    );
    for _ in 0..2000 {
        advance(&mut sim, rules);
        let unit = sim.substrate.entities.get(ship).unwrap();
        if unit.health.current() == strength && !linked(&sim, yard, ship) {
            assert!(unit.locomotor.as_ref().unwrap().is_powered());
            assert!(sim.houses.get(&owner).unwrap().economy().credits() < 20_000);
            return;
        }
    }
    let unit = sim.substrate.entities.get(ship).unwrap();
    let building = sim.substrate.entities.get(yard).unwrap();
    panic!(
        "frame={} ship HP={} cell={:?} nav={:?} mission={:?}; yard mission={:?} state={} online={} linked={}",
        sim.session.binary_frame(),
        unit.health.current(),
        (unit.position.rx(), unit.position.ry()),
        unit.navigation.nav_com(),
        unit.mission.current(),
        building.mission.current(),
        building.mission.handler_state(),
        building.building_online(),
        linked(&sim, yard, ship)
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn constructed_depot_waiters_take_the_freed_slot_in_live_logic_order() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    for reverse_waiter_visits in [false, true] {
        let mut sim = Simulation::with_seed(31);
        crate::sim::arena_fixture::flat_arena(&mut sim, rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, Some(owner), true, 20_000, 10));
        let tanks = std::array::from_fn::<_, 3, _>(|i| {
            sim.spawn_object("MTNK", "Americans", 14 + i as u16, 11, 0, rules)
                .expect("constructed tank Reveal")
        });
        let ap = sim.interner.intern("AP");
        for tank in tanks {
            // A prepared nonfatal direct ReceiveDamage input. Do not write
            // health/estimated-health or bypass their shared receiver owner.
            sim.commit_direct_damage_receiver(
                rules,
                None,
                EntityDamageEvent::direct_receiver(
                    tank,
                    200,
                    0,
                    0,
                    None,
                    ap,
                    ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: false,
                    },
                ),
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                sim.substrate.entities.get(tank).unwrap().health.current(),
                100
            );
        }
        sim.spawn_object("GAPOWR", "Americans", 20, 20, 0, rules)
            .expect("constructed power plant Reveal");
        let depot = sim
            .spawn_object("GADEPT", "Americans", 10, 10, 0, rules)
            .expect("constructed depot Reveal");
        for y in 10..13 {
            for x in 10..13 {
                assert_ne!(
                    sim.substrate.raw_cell_occupation.ground_bits(x, y) & BUILDING_OCCUPATION_BIT,
                    0,
                    "Building Mark owns every foundation cell"
                );
            }
        }
        let building = sim.substrate.entities.get(depot).unwrap();
        assert!(building.lifecycle.cell_marked() && building.in_logic_vector.is_member());
        // Constructors register the starting tanks before the later depot.
        // The second case explicitly changes only the prepared Logic order,
        // proving that stable IDs/storage order do not decide admission.
        if reverse_waiter_visits {
            let mut order = sim.live_object_order_snapshot();
            let second = order.iter().position(|id| *id == tanks[1]).unwrap();
            let third = order.iter().position(|id| *id == tanks[2]).unwrap();
            order.swap(second, third);
            sim.set_logic_order_for_test(order);
        }
        let waiter_order = if reverse_waiter_visits {
            [tanks[2], tanks[1]]
        } else {
            [tanks[1], tanks[2]]
        };
        for _ in 0..30 {
            advance(&mut sim, rules);
        }
        for tank in [tanks[0], tanks[2], tanks[1]] {
            assert!(sim.apply_command(
                "Americans",
                &Command::clicked_mission(
                    tank,
                    MissionType::Enter,
                    crate::sim::combat::TargetKind::Entity(depot)
                ),
                Some(rules)
            ));
        }
        assert!(linked(&sim, depot, tanks[0]));
        for tank in &tanks[1..] {
            assert_eq!(
                sim.substrate.entities.get(*tank).unwrap().pending_entry(),
                Some(depot)
            );
        }
        let mut released = false;
        for _ in 0..3000 {
            advance(&mut sim, rules);
            if !linked(&sim, depot, tanks[0]) {
                released = true;
                break;
            }
        }
        assert!(
            released,
            "first tank completes service and releases its contacts"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(tanks[0])
                .unwrap()
                .health
                .current(),
            rules.object("MTNK").unwrap().strength
        );
        // Both pending Foot turns preceded this Building release. Their next
        // complete turns, rather than an end-of-frame retry sweep, claim it.
        for tank in waiter_order {
            assert!(!linked(&sim, depot, tank));
        }
        advance(&mut sim, rules);
        assert!(
            linked(&sim, depot, waiter_order[0]),
            "first pending Foot turn claims slot"
        );
        assert!(!linked(&sim, depot, waiter_order[1]));
        let admitted = sim.substrate.entities.get(waiter_order[0]).unwrap();
        assert_eq!(admitted.pending_entry(), None);
        assert_eq!(admitted.mission.current().known(), Some(MissionType::Enter));
        assert_eq!(
            admitted.navigation.nav_com(),
            Some(NavTargetRef::Object { id: depot })
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(waiter_order[1])
                .unwrap()
                .pending_entry(),
            Some(depot)
        );
        let mut complete = false;
        for _ in 0..6000 {
            advance(&mut sim, rules);
            if tanks.iter().all(|tank| {
                let unit = sim.substrate.entities.get(*tank).unwrap();
                unit.health.current() == rules.object("MTNK").unwrap().strength
                    && unit.pending_entry().is_none()
                    && !linked(&sim, depot, *tank)
                    && !((10..13).contains(&unit.position.rx())
                        && (10..13).contains(&unit.position.ry()))
                    && unit
                        .locomotor
                        .as_ref()
                        .is_some_and(|loco| loco.is_powered())
            }) {
                complete = true;
                break;
            }
        }
        assert!(
            complete,
            "all three tanks repair, detach and drive off the marked foundation"
        );
    }
}
