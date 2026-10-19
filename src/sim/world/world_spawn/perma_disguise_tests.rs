//! Rust regression tests of Init_Managers' PermaDisguise arm
//! (`0x006F41FE..0x006F4271`) through the production constructor.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

const RULES: &str = "\
[General]
AlliedDisguise=E1
SovietDisguise=E2
ThirdDisguise=INIT
[InfantryTypes]
0=E1
1=E2
2=INIT
3=SPY
[VehicleTypes]
0=MGTK
[AircraftTypes]
[BuildingTypes]
[E1]
Strength=125
[E2]
Strength=125
[INIT]
Strength=125
[SPY]
Strength=100
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
CanDisguise=yes
PermaDisguise=yes
[MGTK]
Strength=200
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
CanDisguise=yes
";

#[test]
fn a_perma_disguised_spy_starts_wearing_its_sides_rules_disguise() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    let mut sim = Simulation::with_seed(3);
    let houses = [
        ("Americans", 0),
        ("Russians", 1),
        ("YuriCountry", 2),
        ("Neutral", 3),
    ];
    for (name, side) in houses {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, false, 0, 10));
        sim.session.house_order.push(id);
    }
    sim.session.binary_frame = 40;
    for (i, ((owner, _), worn)) in houses
        .into_iter()
        .zip(["E1", "E2", "INIT", "INIT"])
        .enumerate()
    {
        let spy = sim
            .spawn_object_at_height("SPY", owner, 10 + 3 * i as u16, 10, 0, 0, &rules)
            .expect("SPY spawns");
        let disguise = sim.substrate.entities.get(spy).unwrap().disguise.clone();
        let disguise = disguise.expect("constructed disguised");
        assert!(disguise.is_disguised());
        assert_eq!(disguise.type_id(), Some(sim.interner.intern(worn)));
        assert_eq!(disguise.house(), Some(sim.interner.intern(owner)));
        // +0x1DC keeps the constructor's zero, not the construction frame.
        assert_eq!(disguise.creation_frame(), 0);
    }
    let mirage = sim
        .spawn_object_at_height("MGTK", "Americans", 10, 20, 0, 0, &rules)
        .expect("MGTK spawns");
    assert!(
        !sim.substrate
            .entities
            .get(mirage)
            .unwrap()
            .disguise
            .as_ref()
            .is_some_and(|d| d.is_disguised()),
        "CanDisguise without PermaDisguise starts undisguised"
    );
}

/// `AlliedDisguise=` goes through InfantryType's factory (`0x0067BAC0`),
/// whose `none` is null, so the American spy starts disguised with no type.
/// The spawn read a second, typed copy that kept `none` as a name.
#[test]
fn a_none_rules_disguise_starts_the_spy_with_no_disguise_type() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        &RULES.replace("AlliedDisguise=E1", "AlliedDisguise=none"),
    ))
    .expect("rules");
    let mut sim = Simulation::with_seed(3);
    let id = sim.interner.intern("Americans");
    sim.houses
        .insert(id, HouseState::new(id, 0, None, false, 0, 10));
    sim.session.house_order.push(id);
    let spy = sim
        .spawn_object_at_height("SPY", "Americans", 10, 10, 0, 0, &rules)
        .expect("SPY spawns");
    let disguise = sim.substrate.entities.get(spy).unwrap().disguise.clone();
    let disguise = disguise.expect("constructed disguised");
    assert!(disguise.is_disguised());
    assert_eq!(disguise.type_id(), None);
}

/// Retail RULESMD through the production reader: [SPY] sets both keys,
/// [General] names E1/E2/INIT, and [MGTK] is CanDisguise only.
#[test]
fn a_retail_spy_starts_disguised_as_its_sides_infantry() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        assert!(!crate::rules::retail_ini_fixture::require_retail_ini());
        return;
    };
    let spy = rules.object("SPY").expect("retail SPY");
    assert!(spy.can_disguise && spy.perma_disguise);
    let mgtk = rules.object("MGTK").expect("retail MGTK");
    assert!(mgtk.can_disguise && !mgtk.perma_disguise);
    assert_eq!(
        rules
            .general
            .infantry_disguises
            .each_ref()
            .map(Option::as_deref),
        [Some("E1"), Some("E2"), Some("INIT")]
    );
    let mut sim = Simulation::with_seed(3);
    let id = sim.interner.intern("Russians");
    sim.houses
        .insert(id, HouseState::new(id, 1, None, false, 0, 10));
    sim.session.house_order.push(id);
    let spy = sim
        .spawn_object_at_height("SPY", "Russians", 10, 10, 0, 0, &rules)
        .expect("SPY spawns");
    let disguise = sim.substrate.entities.get(spy).unwrap().disguise.clone();
    assert_eq!(
        disguise.and_then(|d| d.type_id()),
        Some(sim.interner.intern("E2"))
    );
}
