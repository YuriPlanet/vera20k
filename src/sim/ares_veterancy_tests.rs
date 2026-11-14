//! Ares UpdateVeterancy10046AF0 and RegisterDestruction_Veterancy1004EEF0
//! through the AI promotion sample and the kill record.
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture::{clear_ground_ini, rival_arena};
use crate::sim::combat::veterancy::{LEVEL_ROOKIE, LEVEL_VETERAN};
use crate::sim::combat::{KillCallback, KillSource};
use crate::sim::world::{SimSoundEvent, Simulation, UninitContext};
use crate::util::native_x87::NativeF32Bits;

fn rules(body: &str) -> RuleSet {
    let ini: IniFile = clear_ground_ini(&format!(
        "[General]\nVeteranRatio=3.0\nVeteranCap=2\n[Countries]\n0=Americans\n1=Russians\n{body}"
    ));
    RuleSet::from_ini(&ini).expect("fixture rules")
}

fn promotions(sim: &mut Simulation) -> usize {
    sim.take_sound_events_for_test()
        .iter()
        .filter(|event| matches!(event, SimSoundEvent::UnitPromoted { .. }))
        .count()
}

fn board(sim: &mut Simulation, transport: u64, passengers: Vec<u64>) {
    sim.substrate
        .entities
        .get_mut(transport)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .set_passengers_for_test(passengers);
}

/// A veteran APC converts to its Promote.VeteranType, adds
/// Promote.VeteranExperience, flashes for VeteranFlashTimer and silently
/// promotes its trainable passenger to its own accumulator; the stock
/// detector does none of these for a veteran.
#[test]
fn promotion_converts_adds_experience_flashes_and_promotes_passengers() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(
        "[AudioVisual]\nVeteranFlashTimer=7\n\
         [VehicleTypes]\n0=APC\n1=APC2\n[InfantryTypes]\n0=E1\n1=DOG\n\
         [APC]\nStrength=100\nSpeed=4\nPassengers=2\nPromote.VeteranType=APC2\n\
         Promote.VeteranExperience=0.5\nPromote.IncludePassengers=yes\n\
         [APC2]\nStrength=100\nSpeed=4\nPassengers=2\n\
         [E1]\nStrength=100\nSpeed=4\n[DOG]\nStrength=100\nSpeed=4\nTrainable=no\n",
    );
    let mut sim = rival_arena(&rules, 7);
    let apc = sim
        .spawn_object("APC", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let e1 = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let dog = sim
        .spawn_object("DOG", "Americans", 14, 10, 0, &rules)
        .unwrap();
    sim.object_ai_stage(Some(&rules));
    board(&mut sim, apc, vec![e1, dog]);
    assert_eq!(promotions(&mut sim), 0, "the first sample only caches");
    sim.substrate
        .entities
        .get_mut(apc)
        .unwrap()
        .veterancy
        .set_veteran();
    sim.object_ai_stage(Some(&rules));

    let actor = sim.substrate.entities.get(apc).unwrap();
    assert_eq!(sim.interner.resolve(actor.type_ref()), "APC2");
    assert_eq!(f32::from_bits(actor.veterancy.raw().bits()), 1.5);
    assert_eq!(actor.veterancy.rank_cache(), LEVEL_VETERAN);
    // Seeded to 7 here, then stepped once later in the same visit.
    assert_eq!(actor.flashing.frames(), 6);
    let e1 = sim.substrate.entities.get(e1).unwrap();
    assert_eq!(e1.veterancy.raw(), NativeF32Bits::ONE);
    assert_eq!(e1.veterancy.rank_cache(), LEVEL_VETERAN);
    let dog = sim.substrate.entities.get(dog).unwrap();
    assert_eq!(dog.veterancy.raw(), NativeF32Bits::POSITIVE_ZERO);
    assert_eq!(dog.veterancy.rank_cache(), LEVEL_ROOKIE);
    assert_eq!(
        promotions(&mut sim),
        1,
        "only the APC's own promotion speaks"
    );
}

/// An untrainable airstrike plane credits its designator when that
/// designator has Experience.FromAirstrike, scaled by its
/// Experience.AirstrikeModifier; without the key the kill pays nobody.
#[test]
fn airstrike_kill_credits_the_designator_scaled() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for from_airstrike in [true, false] {
        let rules = rules(&format!(
            "[InfantryTypes]\n0=BORIS\n1=DRONE\n2=VICTIM\n\
             [BORIS]\nStrength=100\nSpeed=4\nCost=1000\n\
             Experience.FromAirstrike={from_airstrike}\nExperience.AirstrikeModifier=50%\n\
             [DRONE]\nStrength=100\nSpeed=4\nCost=1000\nTrainable=no\n\
             [VICTIM]\nStrength=100\nSpeed=4\nCost=3000\n"
        ));
        let mut sim = rival_arena(&rules, 9);
        let boris = sim
            .spawn_object("BORIS", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let drone = sim
            .spawn_object("DRONE", "Americans", 12, 10, 0, &rules)
            .unwrap();
        let victim = sim
            .spawn_object("VICTIM", "Russians", 14, 10, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(drone)
            .unwrap()
            .airstrike
            .set_test_link(Some(boris));
        sim.record_the_kill(
            victim,
            KillSource::Techno(Some(drone)),
            KillCallback::Terminal,
            UninitContext::with_rules(&rules),
        );
        // 3000 x 50% over 1000 x VeteranRatio 3 is half a rank.
        let expected = if from_airstrike { 0.5 } else { 0.0 };
        let raw = |id| {
            f32::from_bits(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .veterancy
                    .raw()
                    .bits(),
            )
        };
        assert_eq!(raw(boris), expected, "FromAirstrike={from_airstrike}");
        assert_eq!(raw(drone), 0.0);
    }
}

/// An elite IFV with Experience.PromotePassengers passes the kill to its
/// trainable gunner, times Experience.PassengerModifier, and the gunner,
/// which never reaches its own AI sample, is promoted at once, without flash.
#[test]
fn elite_ifv_kill_promotes_its_gunner_immediately() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(
        "[VehicleTypes]\n0=IFV\n[InfantryTypes]\n0=E1\n1=VICTIM\n\
         [IFV]\nStrength=100\nSpeed=4\nCost=900\nGunner=yes\nPassengers=1\n\
         Experience.PromotePassengers=yes\nExperience.PassengerModifier=200%\n\
         [E1]\nStrength=100\nSpeed=4\nCost=100\n\
         [VICTIM]\nStrength=100\nSpeed=4\nCost=150\n",
    );
    let mut sim = rival_arena(&rules, 11);
    let ifv = sim
        .spawn_object("IFV", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let gunner = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let victim = sim
        .spawn_object("VICTIM", "Russians", 14, 10, 0, &rules)
        .unwrap();
    sim.object_ai_stage(Some(&rules));
    board(&mut sim, ifv, vec![gunner]);
    sim.substrate
        .entities
        .get_mut(ifv)
        .unwrap()
        .veterancy
        .set_elite();
    let _ = promotions(&mut sim);
    sim.record_the_kill(
        victim,
        KillSource::Techno(Some(ifv)),
        KillCallback::Terminal,
        UninitContext::with_rules(&rules),
    );
    // 150 x 200% over 100 x VeteranRatio 3 is exactly one rank.
    let gunner = sim.substrate.entities.get(gunner).unwrap();
    assert_eq!(gunner.veterancy.raw(), NativeF32Bits::ONE);
    assert_eq!(gunner.veterancy.rank_cache(), LEVEL_VETERAN);
    assert_eq!(gunner.flashing.frames(), 0);
    let ifv = sim.substrate.entities.get(ifv).unwrap();
    assert_eq!(f32::from_bits(ifv.veterancy.raw().bits()), 2.0);
    assert_eq!(promotions(&mut sim), 1);
}

/// A placed academy raises its owner's new trainable infantry to its
/// Academy.InfantryVeterancy unless Academy.Ignore lists the type; a
/// capture moves the bonus to the captor, and a Limbo takes it away.
#[test]
fn academy_bonus_follows_placement_capture_and_limbo() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(
        "[BuildingTypes]\n0=ACAD\n[InfantryTypes]\n0=E1\n1=E2\n\
         [ACAD]\nStrength=500\nAcademy.InfantryVeterancy=1.5\nAcademy.Ignore=E2\n\
         [E1]\nStrength=100\nSpeed=4\n[E2]\nStrength=100\nSpeed=4\n",
    );
    let mut sim = rival_arena(&rules, 13);
    let mut column = 14;
    let mut raw = |sim: &mut Simulation, name: &str, owner: &str| {
        column += 1;
        let id = sim
            .spawn_object(name, owner, column, 20, 0, &rules)
            .unwrap();
        f32::from_bits(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .veterancy
                .raw()
                .bits(),
        )
    };
    let academy = sim
        .spawn_object("ACAD", "Americans", 10, 10, 0, &rules)
        .unwrap();
    assert_eq!(raw(&mut sim, "E1", "Americans"), 1.5);
    assert_eq!(raw(&mut sim, "E2", "Americans"), 0.0, "Academy.Ignore");
    assert_eq!(raw(&mut sim, "E1", "Russians"), 0.0);
    let russians = sim.interner.intern("Russians");
    sim.change_owner_with_rules(academy, russians, &rules, None, Default::default());
    assert_eq!(raw(&mut sim, "E1", "Americans"), 0.0);
    assert_eq!(raw(&mut sim, "E1", "Russians"), 1.5);
    sim.techno_limbo_with_rules(academy, &rules, None);
    assert_eq!(raw(&mut sim, "E1", "Russians"), 0.0);
}
