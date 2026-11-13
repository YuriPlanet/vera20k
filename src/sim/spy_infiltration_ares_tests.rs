//! Ares' infiltration wrapper, `BuildingExt::ExtData::InfiltratedBy @
//! 10012000` (hook `0x004571E0`), and the veterancy it leaves the spy's house.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::VeterancyRank;
use crate::sim::house_state::{AresSpyVeterancy, HouseState};
use crate::sim::intern::InternedId;
use crate::sim::power_system::PowerState;
use crate::sim::spy_infiltration::{AresInfiltrationArms, InfiltrationEffect};
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::world::{SimSoundEvent, Simulation};

const FRAME: u32 = 400;

/// `H0` owns the buildings with 2000 credits; the human `H1` sends the spy.
fn fixture(ini: &str) -> (Simulation, RuleSet, InternedId, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(ini)).unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    for (name, human, credits) in [("H0", false, 2000), ("H1", true, 0)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, None, human, credits, 10));
    }
    let (owner, spy) = (sim.interner.intern("H0"), sim.interner.intern("H1"));
    sim.power_states
        .map_mut_for_test()
        .insert(owner, PowerState::settled_for_test());
    sim.session.set_binary_frame_for_test(FRAME);
    (sim, rules, owner, spy)
}

fn effects(sim: &mut Simulation) -> Vec<InfiltrationEffect> {
    sim.take_sound_events_for_test()
        .into_iter()
        .filter_map(|event| match event {
            SimSoundEvent::BuildingInfiltrated { effect, .. } => Some(effect),
            _ => None,
        })
        .collect()
}

/// A `SpyEffect.Custom=` power plant war factory takes the Ares arms its
/// keys name, not the original power arm (`SpyPowerBlackout=`): its own
/// outage, half the owner's money capped at the amount, a C4 fuse of
/// `C4Delay=` for the negative delay without C4AppliedBy, and the vehicle
/// (by `Factory=`) and naval veterancy.
#[test]
fn custom_infiltration_takes_the_ares_arms_instead_of_the_original() {
    let (mut sim, rules, owner, spy) = fixture(
        "[General]\nSpyPowerBlackout=1000\nC4Delay=.03\n\
         [BuildingTypes]\n0=B\n\
         [B]\nStrength=800\nPower=100\nFactory=UnitType\nSpyEffect.Custom=yes\n\
         SpyEffect.PowerOutageDuration=300\nSpyEffect.StolenMoneyPercentage=50%\n\
         SpyEffect.StolenMoneyAmount=700\nSpyEffect.SabotageDelay=-1\n\
         SpyEffect.UnitVeterancy=yes\nSpyEffect.NavalVeterancy=yes\n",
    );
    let building = sim.spawn_object("B", "H0", 37, 52, 0, &rules).unwrap();
    sim.take_sound_events_for_test();
    for house in sim.houses.values_mut() {
        house.take_tech_recheck();
    }

    sim.infiltrate_building(building, spy, &rules);

    let blackout = sim.power_states[&owner].blackout_timer_for_test();
    assert_eq!(
        [blackout.start_frame(), blackout.duration()],
        [FRAME as i32, 300]
    );
    assert_eq!(sim.houses[&owner].economy().credits(), 1300);
    assert_eq!(sim.houses[&spy].economy().credits(), 700);
    let fuse = sim.substrate.entities.get(building).unwrap();
    let pending = fuse
        .building
        .pending_c4_detonation
        .pending()
        .expect("armed");
    assert_eq!(
        [pending.timer().start_frame(), pending.timer().duration()],
        [FRAME as i32, rules.c4_delay_frames()]
    );
    assert_eq!(pending.source_entity_id(), None);
    let house = &sim.houses[&spy];
    assert!(house.veteran_units() && house.naval_veterancy());
    assert!(!house.veteran_infantry() && !house.aircraft_veterancy());
    assert!(house.tech_recheck_pending());
    assert_eq!(
        effects(&mut sim),
        [InfiltrationEffect::Ares(AresInfiltrationArms {
            power_outage: true,
            cash_stolen: true,
            veterancy_stolen: true,
            ..Default::default()
        })]
    );
}

/// The wrapper leaves a building of a house the spy's house is allied to
/// alone (`IsAlliedWith` at `10012000`), where the original arm (own house
/// only) would black its power out.
#[test]
fn an_allied_spy_leaves_the_building_alone() {
    let ini = "[General]\nSpyPowerBlackout=1000\n\
               [BuildingTypes]\n0=B\n[B]\nStrength=800\nPower=100\n";
    let blackout = |allied: bool| {
        let (mut sim, rules, owner, spy) = fixture(ini);
        let building = sim.spawn_object("B", "H0", 37, 52, 0, &rules).unwrap();
        if allied {
            sim.house_alliances_mut_for_test()
                .entry("H1".to_string())
                .or_default()
                .insert("H0".to_string());
        }
        sim.infiltrate_building(building, spy, &rules);
        sim.power_states[&owner]
            .blackout_timer_for_test()
            .duration()
    };
    assert_eq!(blackout(false), 1000);
    assert_ne!(blackout(true), 1000);
}

/// `SpyEffect.ResetSuperweapons` restarts the owner's charged Super of the
/// type; `SpyEffect.SuperWeapon` grants the spy's house its Super one time
/// (charged at once) unless `SpyEffect.SuperWeaponPermanent`.
#[test]
fn custom_infiltration_resets_and_grants_supers() {
    let ini = |permanent: bool| {
        format!(
            "[SuperWeaponTypes]\n0=SW0\n1=SW1\n[SW0]\nType=MultiMissile\nRechargeTime=5\n\
             [SW1]\nType=MultiMissile\nRechargeTime=5\n\
             [BuildingTypes]\n0=B\n[B]\nStrength=800\nSuperWeapon=SW0\n\
             SpyEffect.Custom=yes\nSpyEffect.ResetSuperweapons=yes\n\
             SpyEffect.SuperWeapon=SW1\nSpyEffect.SuperWeaponPermanent={}\n",
            if permanent { "yes" } else { "no" }
        )
    };
    for permanent in [false, true] {
        let (mut sim, rules, owner, spy) = fixture(&ini(permanent));
        let building = sim.spawn_object("B", "H0", 37, 52, 0, &rules).unwrap();
        let (sw0, sw1) = (sim.interner.intern("SW0"), sim.interner.intern("SW1"));
        for (house, sw, active) in [(owner, sw0, true), (spy, sw1, false)] {
            let mut instance = SuperWeaponInstance::new(sw, house, 0);
            instance.set_is_active_for_test(active);
            instance.set_is_ready_for_test(active);
            sim.super_weapons
                .map_mut_for_test()
                .entry(house)
                .or_default()
                .insert(sw, instance);
        }
        sim.take_sound_events_for_test();

        sim.infiltrate_building(building, spy, &rules);

        let instance = |house, sw| sim.super_weapons.get(&house).unwrap().get(&sw).unwrap();
        assert!(!instance(owner, sw0).is_ready(), "reset");
        let granted = instance(spy, sw1);
        assert!(granted.is_active());
        assert_eq!(
            granted.is_ready(),
            !permanent,
            "a one-time grant is charged"
        );
        assert_eq!(
            effects(&mut sim),
            [InfiltrationEffect::Ares(AresInfiltrationArms {
                supers_reset: true,
                super_granted: true,
                ..Default::default()
            })]
        );
    }
}

/// The Unit, Aircraft and Building Init hooks (`0x0074689B`, `0x00413FD2`,
/// `0x00442D1B`): the house's SpyEffect veterancy makes a new `Trainable=`
/// naval unit, aircraft or building veteran; a land vehicle still needs the
/// war factory's.
#[test]
fn spy_veterancy_makes_new_naval_units_aircraft_and_buildings_veteran() {
    let (mut sim, rules, _, spy) = fixture(
        "[VehicleTypes]\n0=BOAT\n1=TANK\n[BOAT]\nStrength=300\nNaval=yes\n\
         [TANK]\nStrength=300\n[AircraftTypes]\n0=JET\n[JET]\nStrength=100\n\
         [BuildingTypes]\n0=B\n[B]\nStrength=800\nTrainable=yes\n",
    );
    let rank = |sim: &mut Simulation, kind: &str, rx: u16| {
        let id = sim.spawn_object(kind, "H1", rx, 40, 0, &rules).unwrap();
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .veterancy
            .rank_kind()
    };
    assert_eq!(rank(&mut sim, "BOAT", 30), VeterancyRank::Rookie);
    sim.houses
        .get_mut(&spy)
        .unwrap()
        .grant_ares_spy_veterancy(AresSpyVeterancy {
            naval: true,
            aircraft: true,
            buildings: true,
            ..Default::default()
        });
    assert_eq!(rank(&mut sim, "BOAT", 32), VeterancyRank::Veteran);
    assert_eq!(rank(&mut sim, "JET", 34), VeterancyRank::Veteran);
    assert_eq!(rank(&mut sim, "B", 36), VeterancyRank::Veteran);
    assert_eq!(rank(&mut sim, "TANK", 40), VeterancyRank::Rookie);
}

const SABOTAGE_RULES: &str = "[InfantryTypes]\n0=SAB\n1=SPY\n2=SEAL\n3=THIEF\n\
    [SAB]\nStrength=100\nSpeed=4\nSpeedType=Foot\nInfiltrate=yes\nSaboteur=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [SPY]\nStrength=100\nSpeed=4\nSpeedType=Foot\nInfiltrate=yes\nAgent=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [SEAL]\nStrength=100\nSpeed=4\nSpeedType=Foot\nVeteranAbilities=C4\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [THIEF]\nStrength=100\nSpeed=4\nSpeedType=Foot\nInfiltrate=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [BuildingTypes]\n0=PLANT\n1=VAULT\n\
    [PLANT]\nStrength=750\nPower=100\nSpyable=yes\nCapturable=yes\n\
    [VAULT]\nStrength=750\nCapturable=yes\nImmuneToSaboteurs=yes\n\
    [General]\nC4Delay=.03\n";

fn sabotage_arena() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(SABOTAGE_RULES),
        &IniFile::from_str("[PLANT]\nFoundation=2x2\n[VAULT]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x519FF8);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(FRAME);
    (sim, rules)
}

/// `InfantryExt::GetInfiltrateAction @ 100252D0`: rank C4 over a `CanC4=`
/// building first, a spy only into another side's `Spyable=` building, a
/// Saboteur into a building that is not `ImmuneToSaboteurs`, and anyone
/// that is neither (`Infiltrate=` or not) into a `Capturable=` one.
#[test]
fn the_infiltrate_action_follows_the_ares_order() {
    use crate::sim::world::InfiltrateAction as A;
    let (mut sim, rules) = sabotage_arena();
    let spawn = |sim: &mut Simulation, kind: &str, owner: &str, rx: u16| {
        sim.spawn_object(kind, owner, rx, 4, 0, &rules).unwrap()
    };
    let plant = sim
        .spawn_object("PLANT", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let vault = sim
        .spawn_object("VAULT", "Russians", 14, 10, 0, &rules)
        .unwrap();
    let own_plant = sim
        .spawn_object("PLANT", "Americans", 10, 16, 0, &rules)
        .unwrap();
    let saboteur = spawn(&mut sim, "SAB", "Americans", 4);
    let spy = spawn(&mut sim, "SPY", "Americans", 5);
    let thief = spawn(&mut sim, "THIEF", "Americans", 6);
    let seal = spawn(&mut sim, "SEAL", "Americans", 7);
    let action = |sim: &Simulation, actor, target| sim.infiltrate_action(actor, target, &rules);
    assert_eq!(action(&sim, saboteur, plant), A::Sabotage);
    assert_eq!(action(&sim, saboteur, vault), A::None, "immune");
    assert_eq!(action(&sim, spy, plant), A::Spy);
    assert_eq!(action(&sim, spy, own_plant), A::None, "its own house");
    assert_eq!(action(&sim, thief, vault), A::Capture);
    assert_eq!(action(&sim, seal, plant), A::Capture, "a rookie without C4");
    sim.substrate
        .entities
        .get_mut(seal)
        .unwrap()
        .veterancy
        .set_veteran();
    assert_eq!(action(&sim, seal, plant), A::C4, "C4 by rank");
}

/// A Saboteur on Capture arriving in its NavCom building, then PerCell's
/// building entry; answers whether it is still alive.
fn arrive(sim: &mut Simulation, rules: &RuleSet, building: u64) -> bool {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::world::FrameEffects;
    let saboteur = sim
        .spawn_object("SAB", "Americans", 13, 4, 0, rules)
        .unwrap();
    sim.substrate
        .mission_assign_exact(saboteur, MissionId::from_known(MissionType::Capture), 0)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(saboteur)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: building }));
    let at = sim.substrate.entities.get(building).unwrap().position;
    sim.foot_set_location_marked(
        saboteur,
        DriveCoord {
            x: i32::from(at.rx()) * 256 + 128,
            y: i32::from(at.ry()) * 256 + 128,
            z: 0,
        },
        rules,
        None,
        FrameEffects::default(),
    );
    let result = sim
        .infantry_per_cell_building_entry(saboteur, rules, None, FrameEffects::default())
        .unwrap();
    assert!(result.return_before_foot);
    sim.substrate
        .entities
        .get(saboteur)
        .is_some_and(|saboteur| saboteur.is_object_alive())
}

/// `InfantryClass_UpdatePosition_Saboteur @ 100260D0` (hook `0x00519FF8`):
/// the Saboteur plants a `C4Delay=` charge and is spent; a second one finds
/// the charge there and is turned away, as is one entering a building it
/// may not blow up.
#[test]
fn a_saboteur_plants_one_charge_and_is_spent() {
    let (mut sim, rules) = sabotage_arena();
    let plant = sim
        .spawn_object("PLANT", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let vault = sim
        .spawn_object("VAULT", "Russians", 14, 10, 0, &rules)
        .unwrap();

    assert!(!arrive(&mut sim, &rules, plant), "spent");
    let pending = sim
        .substrate
        .entities
        .get(plant)
        .unwrap()
        .building
        .pending_c4_detonation
        .pending();
    let timer = pending.expect("planted").timer();
    assert_eq!(
        [timer.start_frame(), timer.duration()],
        [FRAME as i32, rules.c4_delay_frames()]
    );

    assert!(arrive(&mut sim, &rules, plant), "a charge is already there");
    assert_eq!(
        sim.substrate
            .entities
            .get(plant)
            .unwrap()
            .building
            .pending_c4_detonation
            .pending(),
        pending
    );
    assert!(arrive(&mut sim, &rules, vault), "ImmuneToSaboteurs");
    assert!(
        sim.substrate
            .entities
            .get(vault)
            .unwrap()
            .building
            .pending_c4_detonation
            .pending()
            .is_none()
    );
}

/// `SpyEffect.RevealProduction` adds the spy's house to DisplayProductionTo
/// (`10012000`), and the building passing to that house drops it again
/// (hooks `0x00448312`/`0x00448D95`).
#[test]
fn reveal_production_marks_the_building_until_the_spy_owns_it() {
    let (mut sim, rules, _, spy) = fixture(
        "[BuildingTypes]\n0=B\n[B]\nStrength=800\nSpyEffect.Custom=yes\n\
         SpyEffect.RevealProduction=yes\n",
    );
    let building = sim.spawn_object("B", "H0", 37, 52, 0, &rules).unwrap();
    sim.take_sound_events_for_test();
    let shows = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .shows_production_to(spy)
    };
    assert!(!shows(&sim));

    sim.infiltrate_building(building, spy, &rules);

    assert!(shows(&sim));
    assert_eq!(
        effects(&mut sim),
        [InfiltrationEffect::Ares(AresInfiltrationArms {
            production_revealed: true,
            ..Default::default()
        })]
    );
    sim.change_owner_with_rules(
        building,
        spy,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(!shows(&sim));
}

/// `SpyEffect.StolenTechIndex` (`10012000`, after the power outage arm): the
/// spy's house gains the bits, which a `Prerequisite.StolenTechs=` type
/// needs (RequirementsMet `10021FF0`), and its tech tree is rechecked.
#[test]
fn stolen_tech_index_feeds_prerequisite_stolen_techs() {
    use crate::sim::production::{RequirementStatus, requirements_met};
    let (mut sim, rules, _owner, spy) = fixture(
        "[BuildingTypes]\n0=B\n[VehicleTypes]\n0=Q\n\
         [B]\nStrength=800\nSpyEffect.Custom=yes\nSpyEffect.StolenTechIndex=2,5\n\
         [Q]\nTechLevel=1\nPrerequisite.StolenTechs=5\n",
    );
    let building = sim.spawn_object("B", "H0", 37, 52, 0, &rules).unwrap();
    let product = rules.object("Q").unwrap();
    let status = |sim: &Simulation| requirements_met(sim, &rules, spy, &sim.houses[&spy], product);
    assert_eq!(status(&sim), RequirementStatus::Incomplete);
    sim.take_sound_events_for_test();
    for house in sim.houses.values_mut() {
        house.take_tech_recheck();
    }

    sim.infiltrate_building(building, spy, &rules);

    assert_eq!(
        effects(&mut sim),
        [InfiltrationEffect::Ares(AresInfiltrationArms {
            tech_stolen: true,
            ..Default::default()
        })]
    );
    assert!(sim.houses[&spy].tech_recheck_pending());
    assert_eq!(status(&sim), RequirementStatus::Complete);
}
