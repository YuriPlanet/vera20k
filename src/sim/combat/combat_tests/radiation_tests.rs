//! Radiation sites: periodic damage, building exemption and delays, the
//! Desolator re-fire loop, and Phobos radiation types and colors.

use super::*;

// ---------------------------------------------------------------------------
// Radiation field (substrate Slice 7): periodic foot-unit damage through the
// [Radiation] RadSiteWarhead, building exemption, and the deployed
// self-irradiator (Desolator) re-fire loop.
// ---------------------------------------------------------------------------

/// Desolator-shaped rules: a deployable radiation infantry, a soft infantry
/// victim, a heavy-armor vehicle victim, and a building.
fn radiation_rules() -> RuleSet {
    radiation_rules_with_art(&IniFile::empty())
}

fn radiation_rules_with_art(art: &IniFile) -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "\
[General]\nVeteranArmor=1.5\n\n\
[InfantryTypes]\n0=DESO\n1=E2\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[DESO]\nStrength=200\nArmor=plate\nSpeed=4\nPrimary=RadBeamWeapon\nSecondary=RadEruptionWeapon\nDeployer=yes\nDeployFire=yes\nImmuneToRadiation=yes\n\n\
[E2]\nStrength=300\nArmor=none\nSpeed=4\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nVeteranAbilities=STRONGER\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[RadBeamWeapon]\nDamage=25\nROF=70\nRange=6\nWarhead=RadSite\n\n\
[RadEruptionWeapon]\nDamage=1\nROF=60\nRange=4\nAreaFire=yes\nWarhead=RadEruptionWarhead\nRadLevel=500\nReport=DesolatorDeploy\n\n\
[RadEruptionWarhead]\nVerses=100%,100%,100%,20%,10%,10%,0%,0%,0%,100%,100%\nInfDeath=7\nRadiation=yes\nCellSpread=10\n\n\
[RadSite]\nVerses=100%,100%,100%,50%,10%,10%,0%,0%,0%,100%,100%\nInfDeath=7\nRadiation=yes\n\n\
[Radiation]\nRadDurationMultiple=1\nRadApplicationDelay=16\nRadLevelMax=500\nRadLevelDelay=90\nRadLevelFactor=.2\nRadSiteWarhead=RadSite\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, art).expect("radiation rules should parse")
}

/// A canonical map is required by Foot's GetCoords -> Map::GetCell lookup.
fn radiation_scene(rules: &RuleSet) -> crate::sim::world::Simulation {
    let mut sim = crate::sim::world::Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim
}

/// Advance the real object visits, including Foot4DA530's radiation prefix
/// before movement. The combat tail alone does not apply periodic radiation.
fn rad_frame(sim: &mut crate::sim::world::Simulation, rules: &RuleSet, binary_frame: u32) {
    sim.session.set_binary_frame_for_test(binary_frame);
    assert!(
        sim.advance_tick(&[], Some(rules), None, None, 100)
            .frame_committed
    );
}

/// A radiation detonation of `rad_level` and `spread` at cell `(rx, ry)` in
/// frame 0.
fn irradiate(
    sim: &mut crate::sim::world::Simulation,
    rules: &RuleSet,
    (rx, ry): (u16, u16),
    rad_level: i32,
    spread: i32,
) {
    irradiate_bound(sim, rules, (rx, ry), rad_level, spread, None);
}

/// A radiation detonation bound to a Phobos `[RadiationTypes]` entry by name.
fn irradiate_bound(
    sim: &mut crate::sim::world::Simulation,
    rules: &RuleSet,
    (rx, ry): (u16, u16),
    rad_level: i32,
    spread: i32,
    rad_type: Option<&str>,
) {
    sim.radiation.apply_detonation(
        crate::sim::radiation::RadDetonation {
            rx,
            ry,
            rad_level,
            spread,
            rad_type: rad_type.map(str::to_owned),
            firing_house: None,
            invoker: None,
        },
        0,
        sim.session.theater(),
        rules,
        sim.resolved_terrain.as_ref(),
        &mut sim.lighting_sources,
    );
}

/// [`irradiate_bound`] with Phobos `RadHasOwner`/`RadHasInvoker` attribution:
/// the detonation names a firing house and an invoker object.
#[cfg(feature = "phobos")]
fn irradiate_attributed(
    sim: &mut crate::sim::world::Simulation,
    rules: &RuleSet,
    (rx, ry): (u16, u16),
    rad_level: i32,
    spread: i32,
    rad_type: Option<&str>,
    firing_house: Option<crate::sim::intern::InternedId>,
    invoker: Option<u64>,
) {
    sim.radiation.apply_detonation(
        crate::sim::radiation::RadDetonation {
            rx,
            ry,
            rad_level,
            spread,
            rad_type: rad_type.map(str::to_owned),
            firing_house,
            invoker,
        },
        0,
        sim.session.theater(),
        rules,
        sim.resolved_terrain.as_ref(),
        &mut sim.lighting_sources,
    );
}

/// Radiation damage applies only on `frame % RadApplicationDelay == 0`
/// boundaries. Original reader/Foot producer/Object receiver outcomes are
/// pinned in spatial_oracle/radiation_damage_boundary. This checks selected
/// final health/admission, not the known intermediate radiation precision gap.
#[test]
fn rad_damage_fires_on_application_delay_boundary_only() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/radiation_damage_boundary.json",
    ))
    .unwrap();
    let native_row = |frame: u32, armor: u8| {
        native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["frame"] == frame && row["armor"] == armor)
            .unwrap()
    };
    let rules = radiation_rules();
    for index in 0..11 {
        let verse = f64::from_bits(
            rules
                .warhead("RadSite")
                .unwrap()
                .interaction(crate::rules::armor::ArmorId::from_index(index))
                .multiplier()
                .bits(),
        );
        assert_eq!(
            format!("{:016x}", verse.to_bits()),
            native_row(16, 5)["verses_bits"][index],
            "original RadSite reader, armor{index}"
        );
    }
    let mut sim = radiation_scene(&rules);
    let inf = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("infantry spawns");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 5, 0, &rules)
        .expect("tank spawns");
    irradiate(&mut sim, &rules, (5, 5), 500, 2);

    // Original Foot4DA554 skips the application on frame15.
    assert_eq!(native_row(15, 0)["admitted"], false);
    assert_eq!(native_row(15, 5)["admitted"], false);
    rad_frame(&mut sim, &rules, 15);
    assert_eq!(
        i64::from(sim.substrate.entities.get(inf).unwrap().health.current()),
        native_row(15, 0)["final_health"].as_i64().unwrap()
    );
    assert_eq!(
        i64::from(sim.substrate.entities.get(tank).unwrap().health.current()),
        native_row(15, 5)["final_health"].as_i64().unwrap()
    );

    // Original spread65B9C0/Cell487CB0/Foot4DA5FA emits base59 at the
    // side cell, while Rust currently emits60. Original 10% reader bits and
    // Object5F5390 nevertheless produce the same selected health295. See
    // the companion's required radiation precision follow-up; no base parity.
    assert_eq!(native_row(16, 0)["admitted"], true);
    assert_eq!(native_row(16, 5)["admitted"], true);
    rad_frame(&mut sim, &rules, 16);
    let inf_hp = sim.substrate.entities.get(inf).unwrap().health.current();
    let tank_hp = sim.substrate.entities.get(tank).unwrap().health.current();
    assert_eq!(
        i64::from(inf_hp),
        native_row(16, 0)["final_health"].as_i64().unwrap()
    );
    assert_eq!(
        i64::from(tank_hp),
        native_row(16, 5)["final_health"].as_i64().unwrap()
    );
    // Sourceless damage must not arm retaliation.
    assert!(
        sim.substrate
            .entities
            .get(inf)
            .unwrap()
            .attack_target
            .target()
            .is_none()
    );

    // Frame 17: off-boundary again.
    assert_eq!(native_row(17, 0)["admitted"], false);
    assert_eq!(native_row(17, 5)["admitted"], false);
    rad_frame(&mut sim, &rules, 17);
    assert_eq!(
        sim.substrate.entities.get(inf).unwrap().health.current(),
        inf_hp
    );
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().health.current(),
        tank_hp
    );
}

#[test]
fn gsi_04_07_damage_periodic_radiation_enters_direct_receiver_once() {
    let rules = radiation_rules();
    let mut sim = radiation_scene(&rules);
    let tank = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .expect("veteran heavy target spawns");
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .veterancy
        .set_rank_for_test(100);
    irradiate(&mut sim, &rules, (5, 5), 500, 2);

    rad_frame(&mut sim, &rules, 16);
    let target = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(
        target.health.current(),
        294,
        "raw 100 / VeteranArmor 1.5 = 66; ftol(66 x heavy 10%) = 6 once"
    );
    assert!(
        target.attack_target.target().is_none(),
        "periodic radiation cannot arm retaliation"
    );
    assert!(
        !sim.sound_events.iter().any(|event| matches!(
            event,
            crate::sim::world::SimSoundEvent::UnderAttack { .. }
                | crate::sim::world::SimSoundEvent::AllyUnderAttack { .. }
        )),
        "null source house cannot publish an under-attack notification"
    );
}

/// FootClass::AI skips the radiation application only for an object in the
/// air: vt+0x54 (`0x004DA588`), IsInAir (`0x005F6B90`), a height of at least
/// 208. A paratrooper falls through that line; a Hover unit stays below it.
#[test]
fn radiation_skips_only_objects_in_the_air() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::LocomotorState;

    let rules = radiation_rules();
    let mut sim = radiation_scene(&rules);
    let mut spawn = |kind, rx| {
        sim.spawn_object(kind, "Americans", rx, 5, 0, &rules)
            .expect("victim spawns")
    };
    let high = spawn("E2", 5);
    let low = spawn("E2", 6);
    let crossing = spawn("E2", 7);
    let hover = spawn("MTNK", 4);
    // Foot4DA539 calls TechnoAI first: 6FA655 -> Mission5B3067 ->
    // Object5F3F2C..5F3F52 adds FallRate to Z before IsInAir4DA588.
    // Seed the descent so the actual radiation queries see 208 and 207;
    // a third object crosses the threshold during that same Object AI.
    for (id, height) in [(high, 211), (low, 210), (crossing, 208)] {
        let paratrooper = sim.substrate.entities.get_mut(id).unwrap();
        paratrooper.begin_parachute_fall(height);
        paratrooper.set_fall_rate_for_test(-3);
    }
    sim.substrate.entities.get_mut(hover).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Hover));
    Simulation::set_object_height_unmarked(&mut sim.substrate, &sim.resolved_terrain, hover, 120);
    irradiate(&mut sim, &rules, (5, 5), 500, 2);

    rad_frame(&mut sim, &rules, 16);
    for (id, height) in [(high, 208), (low, 207), (crossing, 205)] {
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .position
                .exact_z_leptons(),
            Some(height),
            "Object AI descends before the radiation query"
        );
    }
    let health = |id| sim.substrate.entities.get(id).unwrap().health.current();
    assert_eq!(health(high), 300, "in the air at 208");
    assert!(health(low) < 300, "not in the air below 208");
    assert!(
        health(crossing) < 300,
        "the query reads the altitude after this frame's descent"
    );
    assert!(health(hover) < 300, "a hovering unit is not in the air");
}

/// Buildings never take radiation damage; an ImmuneToRadiation unit on the
/// same cell is also exempt.
/// Retail has no building radiation path; Phobos adds one, so this retail
/// guarantee only holds without the capability.
#[cfg(not(feature = "phobos"))]
#[test]
fn buildings_take_no_rad_damage() {
    let rules = radiation_rules();
    let mut sim = radiation_scene(&rules);
    let building = sim
        .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
        .expect("building spawns");
    let deso = sim
        .spawn_object("DESO", "Americans", 6, 5, 0, &rules)
        .expect("desolator spawns");
    irradiate(&mut sim, &rules, (5, 5), 500, 2);

    rad_frame(&mut sim, &rules, 16);
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current(),
        750,
        "buildings are exempt from radiation damage"
    );
    assert_eq!(
        sim.substrate.entities.get(deso).unwrap().health.current(),
        200,
        "ImmuneToRadiation units are exempt"
    );
}

/// Phobos custom radiation: the stock scene plus a `[RadiationTypes]` entry
/// with its own level cap, factor and warhead, bound by `RadType=`.
#[cfg(feature = "phobos")]
fn phobos_radiation_rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "\
[InfantryTypes]\n0=DESO\n1=E2\n\n\
[DESO]\nStrength=200\nArmor=plate\nSpeed=4\nPrimary=RadBeamWeapon\nSecondary=RadEruptionWeapon\nDeployer=yes\nDeployFire=yes\nImmuneToRadiation=yes\n\n\
[E2]\nStrength=300\nArmor=none\nSpeed=4\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
[RadEruptionWeapon]\nDamage=1\nROF=60\nRange=4\nAreaFire=yes\nWarhead=RadEruptionWarhead\nRadLevel=500\nRadType=ToxicField\nReport=DesolatorDeploy\n\n\
[RadEruptionWarhead]\nVerses=100%,100%,100%,20%,10%,10%,0%,0%,0%,100%,100%\nInfDeath=7\nRadiation=yes\nCellSpread=10\n\n\
[BuildingTypes]\n0=GAPOWR\n1=RADBLDG\n2=IRONBLDG\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[RADBLDG]\nStrength=750\nArmor=none\n\n\
[IRONBLDG]\nStrength=750\nArmor=none\nImmuneToRadiation=yes\n\n\
[Radiation]\nRadDurationMultiple=1\nRadApplicationDelay=16\nRadApplicationDelay.Building=16\nRadLevelMax=500\nRadLevelDelay=90\nRadLevelFactor=.2\nRadSiteWarhead=RadSite\n\n\
[Warheads]\n0=RadSite\n1=RadSiteToxic\n2=RadSiteBlast\n\n\
[RadiationTypes]\n0=ToxicField\n1=OtherField\n2=VioletField\n3=DemoPurple\n4=OwnerRad\n5=BlastRad\n\n\
[ToxicField]\nRadLevelFactor=1.0\nRadLevelMax=250\nRadSiteWarhead=RadSiteToxic\n\n\
[OtherField]\nRadLevelFactor=0.1\n\n\
[VioletField]\nRadColor=255,0,255\nRadLightFactor=1.0\nRadTintFactor=1.0\n\n\
[DemoPurple]\nRadColor=255,0,255\nRadLightFactor=0.6\nRadTintFactor=2.0\nRadDurationMultiple=6\nRadLevelMax=400\n\n\
[OwnerRad]\nRadLevelFactor=1.0\nRadLevelMax=250\nRadHasOwner=yes\nRadHasInvoker=yes\n\n\
[BlastRad]\nRadLevelFactor=1.0\nRadLevelMax=250\nRadSiteWarhead=RadSiteBlast\nRadSiteWarhead.Detonate=yes\nRadSiteWarhead.Detonate.Full=no\n\n\
[RadSiteBlast]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nCellSpread=3\n\n\
[RadSiteToxic]\nVerses=100%,100%,100%,50%,10%,10%,0%,0%,0%,100%,100%\nInfDeath=7\nRadiation=yes\n",
    );
    RuleSet::from_ini(&ini).expect("phobos radiation rules should parse")
}

/// A bound radiation type drives the site's level clamp, its damage factor
/// and its own warhead; the default-type path on the same scene keeps the
/// globals.
#[cfg(feature = "phobos")]
#[test]
fn phobos_custom_radiation_type_damage_uses_the_bound_types_parameters() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    let bound = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("bound victim spawns");
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("ToxicField"));
    rad_frame(&mut sim, &rules, 16);
    assert_eq!(
        i64::from(sim.substrate.entities.get(bound).unwrap().health.current()),
        50,
        "RadLevelMax=250 clamps the site level and RadLevelFactor=1.0 applies it"
    );

    let mut stock = radiation_scene(&rules);
    let default = stock
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("default victim spawns");
    irradiate(&mut stock, &rules, (5, 5), 500, 2);
    rad_frame(&mut stock, &rules, 16);
    assert_eq!(
        i64::from(
            stock
                .substrate
                .entities
                .get(default)
                .unwrap()
                .health
                .current()
        ),
        200,
        "the default Radiation entry keeps the global 500 level and 0.2 factor"
    );
}

/// A bound type's `RadColor`, `RadLightFactor` and `RadTintFactor` shape the
/// site's published light, so two types on the same scene light differently.
#[cfg(feature = "phobos")]
#[test]
fn phobos_a_bound_types_color_drives_the_site_light() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    irradiate_bound(&mut sim, &rules, (6, 6), 500, 2, Some("VioletField"));
    let light = sim
        .radiation
        .site_at((6, 6))
        .expect("site at the detonation center")
        .light_source()
        .expect("a live site publishes its light");
    assert!(light.intensity > 0, "RadLightFactor=1.0 lights the field");
    assert_eq!(light.tint[1], 0, "no green channel in 255,0,255");
    assert_eq!(light.tint[0], light.tint[2], "red and blue match: purple");
    assert!(light.tint[0] > 0);
}

/// `UseGlobalRadApplicationDelay=false` moves the admission gate to each
/// bound type's own `RadApplicationDelay`.
#[cfg(feature = "phobos")]
#[test]
fn phobos_per_type_application_delay_gates_damage_when_the_global_gate_is_off() {
    let mut rules = phobos_radiation_rules();
    rules.radiation.use_global_rad_application_delay = false;
    // Rebind the type's own delay by editing its retained entry through the
    // catalog the projection carries.
    let mut sim = radiation_scene(&rules);
    let inf = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("infantry spawns");
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("ToxicField"));
    // The global 16-frame gate is off and the type's delay is the global 16
    // only because the entry reads none; damage still lands on frame 16.
    rad_frame(&mut sim, &rules, 16);
    let after_16 = i64::from(sim.substrate.entities.get(inf).unwrap().health.current());
    assert!(
        after_16 < 300,
        "the type's own 16-frame delay admits frame 16"
    );

    // A type whose own delay is 32 skips the 16 boundary.
    let mut slow = phobos_radiation_rules();
    slow.radiation.use_global_rad_application_delay = false;
    slow.radiation
        .radiation_types
        .apply_pass(&crate::rules::ini_parser::IniFile::from_str(
            "[ToxicField]\nRadApplicationDelay=32\n",
        ));
    let mut sim = radiation_scene(&slow);
    let inf = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &slow)
        .expect("infantry spawns");
    irradiate_bound(&mut sim, &slow, (5, 5), 500, 2, Some("ToxicField"));
    rad_frame(&mut sim, &slow, 16);
    assert_eq!(
        i64::from(sim.substrate.entities.get(inf).unwrap().health.current()),
        300,
        "the type's own 32-frame delay skips frame 16"
    );
    rad_frame(&mut sim, &slow, 32);
    assert_eq!(
        i64::from(sim.substrate.entities.get(inf).unwrap().health.current()),
        50,
        "and admits frame 32"
    );
}

/// Native keeps concurrent sites of different types at one center; this
/// model's one-site-per-center merges and retains the existing binding.
#[cfg(feature = "phobos")]
#[test]
fn phobos_merge_at_one_center_keeps_the_existing_bound_type() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("ToxicField"));
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("OtherField"));
    let site = sim.radiation.site_at((5, 5)).expect("site present");
    assert_eq!(
        site.rad_type(),
        Some("ToxicField"),
        "the existing site keeps its bound type on merge"
    );
    let inf = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("infantry spawns");
    rad_frame(&mut sim, &rules, 16);
    assert_eq!(
        i64::from(sim.substrate.entities.get(inf).unwrap().health.current()),
        50,
        "the merged site still damages through the first type's parameters"
    );
}

/// A deployed Desolator force-fires its deploy weapon at its own cell when no
/// site exists there, arming a full-level site; while the site's effective
/// level stays at or above a third of the weapon's RadLevel the gate is
/// closed and it does not fire again.
#[test]
fn deployed_desolator_self_irradiates_and_refires_below_third() {
    let art = IniFile::from_str(
        "[DESO]\nSequence=DesoFixture\n[DesoFixture]\nDeployed=100,1,0\nDeployedFire=100,3,0\n",
    );
    let mut rules = radiation_rules_with_art(&art);
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut sim = crate::sim::world::Simulation::new();
    // Infantry AreaFire compares actual Cell identities. Supply the map that
    // owns both the firer's ObjectGetCell result and its self-target Cell.
    // The firer's ordinary Infantry Unlimbo also reads Clear/Foot admission.
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        16,
    ));
    let deso = sim
        .spawn_object("DESO", "Americans", 10, 10, 0, &rules)
        .expect("desolator spawns");
    sim.substrate
        .entities
        .get_mut(deso)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(28)
        .unwrap();
    // Supply each due Guard dispatch directly, as this phase-level fixture
    // did before. FireAt now belongs to that native mission, not a late
    // global target synthesizer. Its new Bullet takes the ordinary tail.
    let guard_visit = |sim: &mut crate::sim::world::Simulation, frame| {
        sim.session.set_binary_frame_for_test(frame);
        let mission = &mut sim.substrate.entities.get_mut(deso).unwrap().mission;
        mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: mission.suspended(),
            queued: MissionId::NONE,
            movement_bypass_latch: mission.movement_bypass_latch(),
            handler_state: mission.handler_state(),
            mission_start_frame: mission.mission_start_frame(),
            ai_counter: mission.ai_counter(),
            dispatch_timer: MissionDispatchTimer::at_frame(frame),
        });
        sim.fire_events.clear_for_test();
        let first_tail = sim.next_stable_object_id();
        sim.object_ai_visit_one(
            deso,
            Some(&rules),
            &mut crate::sim::world::ObjectAiCtx::default(),
        );
        sim.visit_combat_tail(
            first_tail,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        );
        sim.fire_events.clone()
    };

    // Tick 1: gate open (no site) → self-targeted deploy-weapon shot.
    let events = guard_visit(&mut sim, 1);
    assert_eq!(events.len(), 1, "deployed self-irradiate fires");
    assert_eq!(
        sim.interner.resolve(events[0].weapon_id),
        "RadEruptionWeapon"
    );
    assert_eq!(events[0].target, TargetKind::Cell(10, 10));
    let site = sim
        .radiation
        .site_at((10, 10))
        .expect("detonation armed a site at the desolator's cell");
    assert_eq!(site.level(), 500);
    assert_eq!(sim.radiation.cell_level((10, 10)), 500.0);

    // Tick 2: gate closed (effective 500 ≥ 500/3) → no fire, self-target
    // cleared.
    let events = guard_visit(&mut sim, 2);
    assert_eq!(events.len(), 0, "gate closed after re-arm");
    assert!(
        sim.substrate
            .entities
            .get(deso)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "synthesized self-target is cleared once the gate closes"
    );

    // Decay the site below RadLevel/3 (= 166): effective = remaining×500/500
    // drops below 166 once remaining < 167.
    for frame in 3..=340 {
        sim.radiation
            .tick_decay(frame, &rules, None, &mut sim.lighting_sources);
    }
    let site = sim.radiation.site_at((10, 10)).expect("site still alive");
    assert!(crate::sim::radiation::RadiationState::current_site_level(site) < 500 / 3);

    // Gate reopens → fires again and merges the site back up.
    let events = guard_visit(&mut sim, 341);
    assert_eq!(events.len(), 1, "gate reopens below one third");
    let site = sim.radiation.site_at((10, 10)).expect("merged site");
    assert!(site.level() > 500, "re-detonation merged effective + added");
}

/// The demo mod's own values: `RadColor=255,0,255`, `RadLightFactor=0.6`,
/// `RadTintFactor=2.0`, duration x6 and a 400 level cap.
#[cfg(feature = "phobos")]
#[test]
fn phobos_demo_purple_values_reach_the_light() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    irradiate_bound(&mut sim, &rules, (7, 7), 500, 2, Some("DemoPurple"));
    let site = sim.radiation.site_at((7, 7)).expect("site");
    let light = site.light_source().expect("published light");
    println!(
        "DEMO light tint={:?} intensity={} level={} duration={}",
        light.tint,
        light.intensity,
        site.level(),
        site.duration()
    );
    assert_eq!(site.level(), 400, "RadLevelMax clamps 500");
    assert_eq!(site.duration(), 2400, "six frames per level");
    assert!(light.tint[0] > 0, "red must add: {:?}", light.tint);
    assert_eq!(light.tint[0], light.tint[2]);
    assert_eq!(light.tint[1], 0);
}

/// The compiled capability decides which owner colors the light. Phobos
/// replaces the retail activation with `RadSiteExt::CreateLight`, which tints
/// from the bound type's `RadColor` with no theater test, so a purple field
/// stays purple on a SNOW scenario.
#[cfg(feature = "phobos")]
#[test]
fn phobos_radiation_color_ignores_the_theater() {
    let rules = phobos_radiation_rules();
    for theater in ["SNOW", "TEMPERATE"] {
        let mut sim = radiation_scene(&rules);
        sim.session_mut_for_test()
            .set_theater_for_test(theater.to_owned());
        irradiate_bound(&mut sim, &rules, (8, 8), 500, 2, Some("DemoPurple"));
        let light = sim
            .radiation
            .site_at((8, 8))
            .expect("site")
            .light_source()
            .expect("published light");
        assert_eq!(
            light.tint,
            [2000, 0, 2000],
            "{theater} keeps the type's three channels"
        );
    }
}

/// The retail owner keeps native `RadSite65B5EA..65B684`: on SNOW the light
/// substitutes signed -128 for red and blue and keeps only `RadColor`'s
/// green, so the theater decides which channels the field uses. Ares's
/// RadSiteClass_Radiate_UnhardcodeSnow (10056DB0, hook 0x65B5FB) jumps past
/// this substitution; `radiation::ares_radiation_keeps_rad_color_on_snow`
/// covers a game where Ares plays.
#[cfg(not(feature = "phobos"))]
#[test]
fn retail_snow_theater_substitutes_red_and_blue() {
    let rules = radiation_rules();
    let mut sim = radiation_scene(&rules);
    sim.session_mut_for_test()
        .set_theater_for_test("SNOW".to_owned());
    irradiate(&mut sim, &rules, (8, 8), 500, 2);
    let light = sim
        .radiation
        .site_at((8, 8))
        .expect("site")
        .light_source()
        .expect("published light");
    assert_eq!(
        light.tint,
        [-501, 0, -501],
        "red and blue take the snow -128 channel, green keeps [Radiation] RadColor"
    );
}

/// Phobos `RadHasOwner`/`RadHasInvoker`: the site keeps the round's house and
/// firer, and its damage call carries them.
#[cfg(feature = "phobos")]
#[test]
fn phobos_rad_has_owner_and_invoker_reach_the_site_and_its_damage() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    let victim = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .expect("victim spawns");
    let shooter = sim
        .spawn_object("E2", "Americans", 8, 8, 0, &rules)
        .expect("shooter spawns");
    let owner = sim.interner.get("Americans").expect("owner interned");
    irradiate_attributed(
        &mut sim,
        &rules,
        (5, 5),
        500,
        2,
        Some("OwnerRad"),
        Some(owner),
        Some(shooter),
    );
    let (kept_house, kept_invoker) = {
        let site = sim.radiation.site_at((5, 5)).expect("site");
        (site.firing_house(), site.invoker())
    };
    assert_eq!(kept_house, Some(owner), "RadHasOwner keeps the house");
    assert_eq!(kept_invoker, Some(shooter), "RadHasInvoker keeps the firer");
    rad_frame(&mut sim, &rules, 16);
    let after = sim
        .substrate
        .entities
        .get(victim)
        .map_or(0, |entity| entity.health.current());
    assert!(
        after < 300,
        "the attributed damage reaches the victim (health {after})"
    );

    // A type without the flags keeps neither, even though the detonation
    // named them.
    let mut plain = radiation_scene(&rules);
    plain
        .spawn_object("E2", "Americans", 10, 10, 0, &rules)
        .expect("plain scene interns its house");
    let plain_owner = plain.interner.get("Americans").expect("owner interned");
    irradiate_attributed(
        &mut plain,
        &rules,
        (5, 5),
        500,
        2,
        Some("ToxicField"),
        Some(plain_owner),
        Some(shooter),
    );
    let site = plain.radiation.site_at((5, 5)).expect("site");
    assert_eq!(site.firing_house(), None, "the flag decides retention");
    assert_eq!(site.invoker(), None);
}

/// Phobos `BuildingClass_AI_Radiation @ 0x43FB23`: a building takes its
/// foundation's radiation on `RadApplicationDelay.Building` boundaries, and
/// each type clamps its level at `RadLevelMax` and scales it by its own
/// `RadLevelFactor` through its own warhead.
#[cfg(feature = "phobos")]
#[test]
fn phobos_buildings_take_radiation_on_the_building_delay() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    let building = sim
        .spawn_object("RADBLDG", "Americans", 5, 5, 0, &rules)
        .expect("building spawns");
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("ToxicField"));

    rad_frame(&mut sim, &rules, 15);
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current(),
        750,
        "off the building's application-delay boundary"
    );
    rad_frame(&mut sim, &rules, 16);
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current(),
        500,
        "RadLevelMax 250 x RadLevelFactor 1.0 reaches the building"
    );
}

/// The branch is opt-in: with no `RadApplicationDelay.Building` the shared
/// gate's building delay is 0, which native treats as "skip". Immune
/// buildings and a warhead that verses the armor at 0% stay untouched.
#[cfg(feature = "phobos")]
#[test]
fn phobos_building_radiation_respects_the_delay_key_immunity_and_verses() {
    let mut rules = phobos_radiation_rules();
    rules.radiation.application_delay_building = 0;
    let mut sim = radiation_scene(&rules);
    let plain = sim
        .spawn_object("RADBLDG", "Americans", 5, 5, 0, &rules)
        .expect("building spawns");
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("ToxicField"));
    rad_frame(&mut sim, &rules, 16);
    assert_eq!(
        sim.substrate.entities.get(plain).unwrap().health.current(),
        750,
        "delay 0 disables building radiation"
    );

    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    let immune = sim
        .spawn_object("IRONBLDG", "Americans", 5, 5, 0, &rules)
        .expect("immune building spawns");
    let wood = sim
        .spawn_object("GAPOWR", "Americans", 9, 9, 0, &rules)
        .expect("wood building spawns");
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 2, Some("ToxicField"));
    irradiate_bound(&mut sim, &rules, (9, 9), 500, 2, Some("ToxicField"));
    rad_frame(&mut sim, &rules, 16);
    assert_eq!(
        sim.substrate.entities.get(immune).unwrap().health.current(),
        750,
        "ImmuneToRadiation buildings are exempt"
    );
    assert_eq!(
        sim.substrate.entities.get(wood).unwrap().health.current(),
        750,
        "the type's warhead verses wood at 0%"
    );
}

/// Phobos `RadSiteWarhead.Detonate`: with the key set, a site's damage is a
/// warhead detonation at the target, so the warhead's `CellSpread` reaches
/// neighbours the field itself never covered. The direct-receiver form stays
/// on the target alone.
#[cfg(feature = "phobos")]
#[test]
fn phobos_rad_site_warhead_detonate_splashes_around_the_target() {
    let rules = phobos_radiation_rules();
    let mut sim = radiation_scene(&rules);
    let near = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("near victim spawns");
    let far = sim
        .spawn_object("E2", "Americans", 8, 5, 0, &rules)
        .expect("far victim spawns");
    // spread 0 covers only the centre cell, so the far cell starts clean.
    irradiate_bound(&mut sim, &rules, (5, 5), 500, 0, Some("BlastRad"));
    assert_eq!(
        sim.radiation.cell_level((8, 5)),
        0.0,
        "no field on the far cell"
    );
    rad_frame(&mut sim, &rules, 16);
    assert!(
        sim.substrate.entities.get(near).unwrap().health.current() < 300,
        "the target takes the detonation"
    );
    assert!(
        sim.substrate.entities.get(far).unwrap().health.current() < 300,
        "the warhead's CellSpread reaches the neighbour"
    );

    // Control: the same field without the key hits only the target.
    let mut plain = radiation_scene(&rules);
    let near = plain
        .spawn_object("E2", "Americans", 5, 5, 0, &rules)
        .expect("near victim spawns");
    let far = plain
        .spawn_object("E2", "Americans", 8, 5, 0, &rules)
        .expect("far victim spawns");
    irradiate_bound(&mut plain, &rules, (5, 5), 500, 0, Some("ToxicField"));
    rad_frame(&mut plain, &rules, 16);
    assert!(plain.substrate.entities.get(near).unwrap().health.current() < 300);
    assert_eq!(
        plain.substrate.entities.get(far).unwrap().health.current(),
        300,
        "the direct receiver leaves the neighbour alone"
    );
}
