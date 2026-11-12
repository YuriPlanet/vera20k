//! Ares EMP on the building kinds whose effects run through other owners:
//! retail Hills rules with the victim tests' EMP gun layered in through the
//! map. A real shell hits a building that is the only target; the installed
//! Bullet pre-effect (`apply_ares_emp_detonation`, 46920B ->100536E0) lands
//! where the hit must fall in a narrow window or on one chosen object.

use super::victim_tests::{emp, fire, missions, offline, rules, run, scene, spawn, step};
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::MissionType;
use crate::sim::production::ProductionCategory;
use crate::sim::world::{FrameEffects, Simulation};

/// A finished building of the player's, owned and discovered.
fn building(sim: &mut Simulation, rules: &RuleSet, kind: &str, rx: u16, ry: u16) -> u64 {
    let id = finished(sim, rules, kind, "Americans", rx, ry);
    crate::sim::arena_fixture::discover(sim, id);
    crate::sim::arena_fixture::own(sim, id);
    id
}

/// A building of `owner`'s past its build-up.
fn finished(
    sim: &mut Simulation,
    rules: &RuleSet,
    kind: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    let id = spawn(sim, rules, kind, owner, rx, ry);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .finish_building_construction_for_test();
    id
}

/// The EMP warhead's pre-effect detonating at `id`'s centre, `source` firing.
fn pulse(sim: &mut Simulation, rules: &RuleSet, source: u64, id: u64) {
    let centre = crate::sim::movement::ground_pose::object_get_coords(
        sim.entities().get(id).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    sim.apply_ares_emp_detonation(
        crate::sim::projectile::ProjectileCoord {
            x: centre.x,
            y: centre.y,
            z: centre.z,
        },
        Some(source),
        rules.warhead("EMPWH").unwrap(),
        rules,
        FrameEffects::default(),
    );
}

/// 10062357..10062376: a tank linked into a Tank Bunker (BunkerLinkedItem a
/// Building) is spared by a Warhead without PenetratesBunker; the tank
/// beside it is not.
#[test]
fn ares_emp_spares_a_tank_in_a_tank_bunker() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, _grid) = scene(&rules, false);
    let source = spawn(&mut sim, &rules, "MTNK", "Russians", 4, 4);
    let bunker = building(&mut sim, &rules, "NATBNK", 15, 10);
    let inside = spawn(&mut sim, &rules, "MTNK", "Americans", 15, 12);
    let beside = spawn(&mut sim, &rules, "MTNK", "Americans", 16, 12);
    sim.substrate
        .entities
        .get_mut(bunker)
        .unwrap()
        .bunker
        .set_bunker_occupant_for_test(Some(inside));
    sim.substrate
        .entities
        .get_mut(inside)
        .unwrap()
        .bunker
        .set_bunker_link_for_test(crate::sim::docking::bunker_link::BunkerLink::Installed(
            bunker,
        ));
    pulse(&mut sim, &rules, source, beside);
    assert!(
        emp(&sim, beside) > 0,
        "the pulse reaches the tank beside it"
    );
    assert_eq!(emp(&sim, inside), 0, "the bunkered tank is spared");
}

/// A construction yard is prone (UndeploysInto). Ares Deactivate1004CDC0
/// (hook 0x70FC90) refreshes its Building queue (10013250): with the yard
/// offline the House has no online factory, so the power plant under
/// construction is held. Reactivate1004CCB0 (hook 0x70FBE0) refreshes it
/// again at expiry, which lifts the system hold. The computer's yard keeps
/// the player's sidebar pass out of the queue, and the pulse, unlike a
/// shell, does not mark it hit by an enemy for the computer's sale.
#[test]
fn ares_emp_on_a_construction_yard_holds_its_production_until_expiry() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let russians = sim.interner.intern("Russians");
    let source = spawn(&mut sim, &rules, "MTNK", "Americans", 4, 4);
    let yard = finished(&mut sim, &rules, "NACNST", "Russians", 15, 8);
    let plant = sim.interner.intern("NAPOWR");
    // The fixture arms the queue kernel's held object; Begin's own
    // admission is not the question here.
    assert!(sim.production.factories_mut().test_enqueue_kernel(
        russians,
        ProductionCategory::Building,
        plant,
        1,
        rules.object("NAPOWR").unwrap().cost,
    ));
    assert!(
        crate::sim::production::construct_active_factory_fixture(
            &mut sim,
            &rules,
            russians,
            ProductionCategory::Building,
            plant,
        )
        .is_some()
    );
    let held = |sim: &Simulation| {
        sim.production
            .factories()
            .view(russians, ProductionCategory::Building)
            .map(|factory| factory.suspended)
    };
    assert_eq!(held(&sim), Some(false));
    pulse(&mut sim, &rules, source, yard);
    assert!(emp(&sim, yard) > 0, "the yard is hit");
    assert!(offline(&sim, yard));
    assert_eq!(held(&sim), Some(true), "Deactivate refreshed the queue");
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, yard) == 0));
    assert!(!offline(&sim, yard));
    assert_eq!(held(&sim), Some(false), "Reactivate refreshed it again");
}

/// A yard hit while it repacks is not deactivated (Selling, 10062D60).
/// Mission_Selling's hook 0x44A04C (10053D60) hands its counter to the MCV,
/// whose next AI visit deactivates it (10053DF0).
#[test]
fn ares_emp_rides_out_of_a_repacking_yard_on_its_mcv() {
    let Some(mut rules) = rules() else {
        return;
    };
    // The INI fixture loads no SHPs; Sell_Back (0x00447110) refuses a type
    // without a build-up, so supply a finite one as an authored input.
    rules.set_buildup_control_for_test("GACNST", [0, 29, 1]);
    let (mut sim, grid) = scene(&rules, false);
    // A yard repacks rather than sells only in a multiplayer game with
    // MCVRedeploy (Sell's undeploy test, `0x00449CEA`).
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().mcv_redeploy = true;
    let source = spawn(&mut sim, &rules, "MTNK", "Russians", 4, 4);
    let yard = building(&mut sim, &rules, "GACNST", 15, 8);
    // Sell_Back queues and commences Selling at once (0x0044717D..0x00447185).
    assert!(sim.undeploy_building(yard, &rules, None, FrameEffects::default()));
    assert_eq!(missions(&sim, yard).0, Some(MissionType::Selling));
    pulse(&mut sim, &rules, source, yard);
    assert!(emp(&sim, yard) > 0, "the repacking yard is hit");
    assert!(!offline(&sim, yard), "Selling defers the deactivation");
    sim.substrate
        .entities
        .get_mut(yard)
        .unwrap()
        .finish_pack_up_for_test();
    step(&mut sim, &rules, &grid);
    let mcv = sim
        .entities()
        .values()
        .find(|entity| {
            entity.lifecycle.object_alive() && sim.interner.resolve(entity.type_ref()) == "AMCV"
        })
        .expect("the yard repacked")
        .stable_id();
    assert!(emp(&sim, mcv) > 0, "the MCV took the yard's counter");
    assert!(run(&mut sim, &rules, &grid, 3, |sim| offline(sim, mcv)));
}

/// A Gap Generator is prone (GapGenerator). DisableStuff 0x452480 removes
/// its gap at the hit; its operational sample (0x0043FB20 -> 0x004549B0)
/// restores it once the counter ends.
#[test]
fn ares_emp_lifts_a_gap_generators_gap_until_expiry() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let gun = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 10);
    let gap = building(&mut sim, &rules, "GAGAP", 15, 10);
    building(&mut sim, &rules, "GAPOWR", 24, 24);
    let gapping = |sim: &Simulation| {
        sim.entities()
            .get(gap)
            .unwrap()
            .gap_generator
            .viewers()
            .values()
            .any(|deposit| deposit.active())
    };
    assert!(
        run(&mut sim, &rules, &grid, 60, |sim| gapping(sim)),
        "the powered generator gaps first"
    );
    assert!(
        fire(&mut sim, &rules, &grid, gun, gap, gap),
        "the generator is hit"
    );
    assert!(!gapping(&sim), "DisableStuff removed the gap at once");
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, gap) == 0));
    assert!(run(&mut sim, &rules, &grid, 5, |sim| gapping(sim)));
}

/// The Psychic Sensor is prone (Radar, SuperWeapon). Ares's Super pass
/// (hook 0x50AF10, 10039450) holds its Psychic Reveal, which is not
/// IsPowered=, while no provider is Charging, and an EMPed provider is not
/// powered (10038F10). The stock pass never holds such a type.
#[test]
fn ares_emp_holds_a_psychic_sensors_reveal_until_expiry() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    for house in ["Americans", "Russians"] {
        let house = sim.interner.intern(house);
        crate::sim::superweapon::construct_house_supers(&mut sim, &rules, house, false);
    }
    let americans = sim.interner.intern("Americans");
    let reveal = sim.interner.intern("PsychicRevealSpecial");
    let gun = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 10);
    let sensor = building(&mut sim, &rules, "NAPSIS", 15, 9);
    building(&mut sim, &rules, "GAPOWR", 24, 24);
    let held = |sim: &Simulation| {
        let instance = &sim.super_weapons[&americans][&reveal];
        instance.is_active().then(|| instance.is_suspended())
    };
    assert!(
        run(&mut sim, &rules, &grid, 60, |sim| held(sim) == Some(false)),
        "granted and charging: {:?}",
        held(&sim)
    );
    assert!(
        fire(&mut sim, &rules, &grid, gun, sensor, sensor),
        "the sensor is hit"
    );
    assert!(
        run(&mut sim, &rules, &grid, 5, |sim| held(sim) == Some(true)),
        "held under EMP: {:?}",
        held(&sim)
    );
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, sensor) == 0));
    assert!(
        run(&mut sim, &rules, &grid, 5, |sim| held(sim) == Some(false)),
        "released at expiry: {:?}",
        held(&sim)
    );
}

/// The Robot Control Center is prone (PowersUnit). Not operational under
/// EMP (0x004555D0), its off edge (0x004549B0) loses the House's powered
/// center and the Robot Tank goes offline (LostPoweredCenter 0x0050E0E0);
/// the on edge after expiry brings it back (0x0050E010).
#[test]
fn ares_emp_on_a_robot_control_center_takes_its_robots_offline() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let gun = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 10);
    let center = building(&mut sim, &rules, "GAROBO", 15, 9);
    building(&mut sim, &rules, "GAPOWR", 24, 24);
    let robot = spawn(&mut sim, &rules, "ROBO", "Americans", 22, 16);
    let counted = |sim: &Simulation| {
        sim.entities()
            .get(center)
            .unwrap()
            .building_powered_center
            .is_counted()
    };
    assert!(run(&mut sim, &rules, &grid, 60, |sim| counted(sim)
        && !offline(sim, robot)));
    assert!(
        fire(&mut sim, &rules, &grid, gun, center, center),
        "the center is hit"
    );
    assert!(
        run(&mut sim, &rules, &grid, 5, |sim| offline(sim, robot)),
        "the center's off edge takes the robot offline"
    );
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, center) == 0));
    assert!(run(&mut sim, &rules, &grid, 5, |sim| !offline(sim, robot)));
}

/// A Prism tower charging its support beam was refused before. Ares takes
/// an EMPed tower out of its network at its next operational sample
/// (10016950, hook 0x454B3D on the 0x004549B0 off edge), as one that goes
/// offline: its support target and pending beam go.
#[test]
fn ares_emp_takes_a_charging_prism_supporter_out_of_its_network() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let source = spawn(&mut sim, &rules, "MTNK", "Russians", 2, 28);
    // Only the master reaches the mark (PrismShot Range=8); the supporter
    // stands within its support range (Range + one cell) of the master.
    let master = building(&mut sim, &rules, "ATESLA", 14, 13);
    let supporter = building(&mut sim, &rules, "ATESLA", 14, 17);
    building(&mut sim, &rules, "GAPOWR", 24, 24);
    let mark = spawn(&mut sim, &rules, "HTNK", "Russians", 14, 8);
    crate::sim::arena_fixture::discover(&mut sim, mark);
    let feeds = |sim: &Simulation| {
        sim.entities()
            .get(supporter)
            .unwrap()
            .ares_prism
            .support_target()
    };
    assert!(
        run(&mut sim, &rules, &grid, 120, |sim| feeds(sim)
            == Some(master)),
        "the master recruits the supporter"
    );
    pulse(&mut sim, &rules, source, supporter);
    assert!(emp(&sim, supporter) > 0, "the charging supporter is hit");
    step(&mut sim, &rules, &grid);
    assert_eq!(feeds(&sim), None);
    assert!(
        sim.entities()
            .get(supporter)
            .unwrap()
            .pending_building_fire
            .pending()
            .is_none()
    );
}

/// A Tesla Coil (Overpowerable), a garrisonable civilian radar building
/// (CanBeOccupied) and Cloning Vats (Cloning, prone through ImmuneToEMP=no)
/// were refused before. Each is now hit and taken offline, and none is
/// operational (0x004555D0), which the vats' clone admission (KickOutClones
/// lambda 100112D0) and every building's power consumers read; all recover
/// at expiry.
#[test]
fn ares_emp_takes_a_coil_a_garrisonable_building_and_cloning_vats_offline() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let source = spawn(&mut sim, &rules, "MTNK", "Russians", 2, 28);
    for rx in [22, 25] {
        building(&mut sim, &rules, "GAPOWR", rx, 2);
    }
    let victims = [
        building(&mut sim, &rules, "TESLA", 8, 14),
        building(&mut sim, &rules, "CANEWY05", 14, 14),
        building(&mut sim, &rules, "NACLON", 20, 14),
    ];
    for id in victims {
        pulse(&mut sim, &rules, source, id);
    }
    for id in victims {
        assert!(emp(&sim, id) > 0 && offline(&sim, id), "{id} is hit");
        assert_eq!(sim.building_operational_state(id, &rules), Some(false));
    }
    assert!(run(&mut sim, &rules, &grid, 200, |sim| victims
        .iter()
        .all(|&id| emp(sim, id) == 0)));
    for id in victims {
        assert!(!offline(&sim, id), "{id} recovers");
        assert_eq!(sim.building_operational_state(id, &rules), Some(true));
    }
}

/// A LaserFencePost is prone. DisableEMPEffect 100627F0 enables its Stuff
/// even while its House is short of power, which leaves any other type's
/// disabled; VERA has no fence sections for that to rebuild.
#[test]
fn ares_emp_expiry_enables_an_unpowered_laser_fence_posts_stuff() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let source = spawn(&mut sim, &rules, "MTNK", "Russians", 2, 28);
    let post = building(&mut sim, &rules, "NASAM", 15, 10);
    let stuff = |sim: &Simulation| {
        sim.entities()
            .get(post)
            .unwrap()
            .building_art
            .stuff_enabled()
    };
    pulse(&mut sim, &rules, source, post);
    assert!(emp(&sim, post) > 0, "the post is hit");
    assert!(!stuff(&sim), "DisableStuff 0x452480 at the hit");
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, post) == 0));
    assert_eq!(
        sim.building_operational_state(post, &rules),
        Some(false),
        "no power plant"
    );
    assert!(stuff(&sim), "EnableStuff 0x452410 whatever the power");
    assert!(offline(&sim, post), "no Reactivate without power");
}
