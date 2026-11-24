//! GetFireError through the production fire path: the facts the world
//! supplies and each class's reaction to the code.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n1=MEDV\n2=HOUND\n3=BEAST\n4=CARR\n\
         [InfantryTypes]\n\
         [BuildingTypes]\n0=COIL\n1=BUNK\n\
         [AircraftTypes]\n\
         [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
         [MEDV]\nStrength=300\nArmor=heavy\nPrimary=Mend\n\
         [HOUND]\nStrength=300\nArmor=heavy\nPrimary=Gun\nNatural=yes\n\
         [BEAST]\nStrength=300\nArmor=heavy\nPrimary=Gun\nUnnatural=yes\n\
         [CARR]\nStrength=300\nArmor=heavy\nPrimary=Launch\n\
         [COIL]\nStrength=600\nArmor=concrete\nPrimary=Zap\nPowered=yes\nPower=-75\n\
         [BUNK]\nStrength=600\nArmor=concrete\nCanBeOccupied=yes\nCanOccupyFire=yes\n\
         MaxNumberOccupants=5\n\
         [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [Mend]\nDamage=-50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [Launch]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\nSpawner=yes\n\
         [Zap]\nDamage=100\nROF=60\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("fire-error fixture rules")
}

fn spawn(
    store: &mut EntityStore,
    id: u64,
    type_name: &str,
    category: EntityCategory,
    owner: &str,
    at: (u16, u16),
) {
    let mut entity = GameEntity::test_default(id, type_name, owner, at.0, at.1);
    entity.set_category_for_test(category);
    entity.health.set_for_test(300);
    entity.lifecycle.set_in_limbo_for_test(false);
    store.insert(entity);
}

/// A flat 16x16 map whose row `y = 10` from `x = 10` to `x = 12` is a bridge
/// (cell flag `0x100`).
fn bridge_row_terrain() -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    let cells = (0..16u16)
        .flat_map(|ry| (0..16u16).map(move |rx| (rx, ry)))
        .map(|(rx, ry)| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
            if ry == 10 && (10..=12).contains(&rx) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
            cell
        })
        .collect();
    crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(16, 16, cells)
}

fn bridge_scene(firer: &str) -> crate::sim::world::Simulation {
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolved_terrain = Some(bridge_row_terrain());
    spawn(
        &mut sim.substrate.entities,
        1,
        firer,
        EntityCategory::Unit,
        "Soviet",
        (10, 10),
    );
    spawn(
        &mut sim.substrate.entities,
        2,
        "TANK",
        EntityCategory::Unit,
        "Americans",
        (12, 10),
    );
    // After the spawns: `test_default` interns their names.
    sim.interner = test_interner();
    sim
}

#[path = "fire_error_cell_tests.rs"]
mod cell_queries;

/// A world without resolved terrain asks the shared mapless range
/// (`in_range::mapless_in_range`): `Range=-2` is always in range, where the
/// flat fallback read -512 leptons as a two-cell reach, and MinimumRange
/// refuses a target inside it.
#[test]
fn a_mapless_fire_gate_takes_the_shared_range_answer() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ASW\n1=ART\n2=TANK\n\
         [InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
         [ASW]\nStrength=300\nArmor=heavy\nPrimary=Depth\n\
         [ART]\nStrength=300\nArmor=heavy\nPrimary=Shell\n\
         [TANK]\nStrength=300\nArmor=heavy\n\
         [Depth]\nDamage=50\nROF=30\nRange=-2\nWarhead=AP\nOmniFire=yes\n\
         [Shell]\nDamage=50\nROF=30\nRange=8\nMinimumRange=3\nWarhead=AP\nOmniFire=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("mapless fire fixture rules");
    let mut sim = crate::sim::world::Simulation::new();
    for (id, kind, owner, at) in [
        (1, "ASW", "Soviet", (5, 5)),
        (2, "ART", "Soviet", (5, 7)),
        (3, "TANK", "Americans", (6, 7)),
        (4, "TANK", "Americans", (10, 7)),
        (5, "TANK", "Americans", (40, 5)),
    ] {
        spawn(
            &mut sim.substrate.entities,
            id,
            kind,
            EntityCategory::Unit,
            owner,
            at,
        );
    }
    // After the spawns: `test_default` interns their names.
    sim.interner = test_interner();
    assert!(sim.resolved_terrain.is_none());
    let error = |firer: u64, kind: &str, target: u64| {
        crate::sim::combat::fire_error_world::FireSubject {
            world: &sim,
            rules: &rules,
            fog: None,
            firer: sim.substrate.entities.get(firer).unwrap(),
            obj: rules.object(kind).unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(target)),
            weapon_index: 0,
        }
        .fire_error(true)
    };
    use crate::sim::combat::fire_error::FireError;
    assert_ne!(error(1, "ASW", 5), FireError::Range, "35 cells away");
    assert_eq!(error(2, "ART", 3), FireError::Range, "inside MinimumRange");
    assert_ne!(error(2, "ART", 4), FireError::Range, "5 cells away");
    assert_eq!(error(2, "ART", 5), FireError::Range, "beyond Range");
}

/// T18 (`0x006FC2A3`): a BalloonHover target still tethered to its war
/// factory (`+0x418`, set by the factory's TETHER) and not yet high-flying
/// is illegal to fire at; once untethered it is a normal target.
#[test]
fn a_tethered_low_balloon_is_illegal_to_fire_at() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n1=ZEP\n\
         [InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
         [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
         [ZEP]\nStrength=300\nArmor=heavy\nBalloonHover=yes\n\
         [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("balloon fixture rules");
    let mut sim = crate::sim::world::Simulation::new();
    spawn(
        &mut sim.substrate.entities,
        1,
        "TANK",
        EntityCategory::Unit,
        "Americans",
        (5, 5),
    );
    spawn(
        &mut sim.substrate.entities,
        2,
        "ZEP",
        EntityCategory::Unit,
        "Soviet",
        (7, 5),
    );
    sim.interner = test_interner();
    let error = |sim: &crate::sim::world::Simulation| {
        crate::sim::combat::fire_error_world::FireSubject {
            world: sim,
            rules: &rules,
            fog: None,
            firer: sim.substrate.entities.get(1).unwrap(),
            obj: rules.object("TANK").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(2)),
            weapon_index: 0,
        }
        .fire_error(true)
    };
    use crate::sim::combat::fire_error::FireError;
    assert_ne!(error(&sim), FireError::Illegal, "untethered");
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .seldom
        .tether
        .set_for_test(Some(99));
    assert_eq!(error(&sim), FireError::Illegal, "tethered to its factory");
}

/// T9 (`0x006FC13F`) compares the target with the firer's `+0x11C` before
/// any `+0x82` test; a garrison occupant or cargo rider has no Transporter,
/// so its carrier is not T9's `Transporter::Target`. A rider whose
/// Transporter is set is still refused. vera20k#3838.
#[test]
fn a_cargo_rider_is_not_bound_to_its_carrier_as_transporter() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=CARR\n\
         [InfantryTypes]\n0=RIDER\n\
         [BuildingTypes]\n[AircraftTypes]\n\
         [CARR]\nStrength=300\nArmor=heavy\nPassengers=1\n\
         [RIDER]\nStrength=100\nArmor=none\nPrimary=Gun\n\
         [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("transporter fixture rules");
    let mut sim = crate::sim::world::Simulation::new();
    spawn(
        &mut sim.substrate.entities,
        1,
        "CARR",
        EntityCategory::Unit,
        "Americans",
        (10, 10),
    );
    let mut rider = GameEntity::test_default_of_category(
        2,
        "RIDER",
        "Americans",
        11,
        10,
        EntityCategory::Infantry,
    );
    rider.health.set_for_test(100);
    rider.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(rider);
    sim.interner = test_interner();
    let error = |sim: &crate::sim::world::Simulation| {
        crate::sim::combat::fire_error_world::FireSubject {
            world: sim,
            rules: &rules,
            fog: None,
            firer: sim.substrate.entities.get(2).unwrap(),
            obj: rules.object("RIDER").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(1)),
            weapon_index: 0,
        }
        .fire_error(true)
    };
    use crate::sim::combat::fire_error::FireError;
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
            1, false,
        ));
    assert_ne!(
        error(&sim),
        FireError::Illegal,
        "a garrison occupant or cargo rider stores no Transporter"
    );
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_passenger_role_for_test(crate::sim::passenger::PassengerRole::boarded_for_test(
            1, false,
        ));
    assert_eq!(
        error(&sim),
        FireError::Illegal,
        "a rider's Transporter is the target whatever +0x82 says"
    );
}

/// Phobos `TechnoClass_CanFire` (`0x6FC339`,
/// `Ext/Techno/Hooks.Firing.cpp:340-369`) removes the stock T16 AI-only
/// iron-curtain refusal and reads the weapon override, else the human/AI
/// globals; the defaults preserve the stock split.
#[test]
fn an_iron_curtained_target_reads_the_weapon_and_owner_globals() {
    use crate::sim::combat::fire_error::FireError;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::sim::timer::CdTimer;
    let error = |weapon_key: &str,
                 global: Option<bool>,
                 ai_global: Option<bool>,
                 human: bool,
                 slot: i32| {
        let mut combat = String::from("[CombatDamage]\n");
        if let Some(value) = global {
            combat.push_str(&format!(
                "CanTarget.IronCurtained={}\n",
                if value { "yes" } else { "no" }
            ));
        }
        if let Some(value) = ai_global {
            combat.push_str(&format!(
                "CanTargetAI.IronCurtained={}\n",
                if value { "yes" } else { "no" }
            ));
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=TANK\n1=DECOY\n[InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
             [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
             [DECOY]\nStrength=300\nArmor=heavy\n\
             [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n{weapon_key}\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{combat}"
        )))
        .expect("curtain fire fixture rules");
        let mut sim = crate::sim::world::Simulation::new();
        spawn(
            &mut sim.substrate.entities,
            1,
            "TANK",
            EntityCategory::Unit,
            "Americans",
            (5, 5),
        );
        spawn(
            &mut sim.substrate.entities,
            2,
            "DECOY",
            EntityCategory::Unit,
            "Soviet",
            (7, 5),
        );
        sim.interner = test_interner();
        let americans = sim.interner_mut_for_test().intern("Americans");
        sim.register_house(crate::sim::house_state::HouseState::new(
            americans, 0, None, human, 0, 10,
        ));
        sim.substrate.entities.get_mut(2).unwrap().invulnerability = Some(
            InvulnerabilityState::new(CdTimer::started(0, 100), InvulnKind::IronCurtain),
        );
        crate::sim::combat::fire_error_world::FireSubject {
            world: &sim,
            rules: &rules,
            fog: None,
            firer: sim.substrate.entities.get(1).unwrap(),
            obj: rules.object("TANK").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(2)),
            weapon_index: slot,
        }
        .fire_error(true)
    };
    // The stock split: AI refused, human allowed.
    assert_eq!(error("", None, None, false, 0), FireError::Illegal);
    assert_ne!(error("", None, None, true, 0), FireError::Illegal);
    // The AI global admits the shot.
    assert_ne!(error("", None, Some(true), false, 0), FireError::Illegal);
    // The human global refuses it.
    assert_eq!(error("", Some(false), None, true, 0), FireError::Illegal);
    // The weapon override beats either global.
    assert_ne!(
        error("CanTarget.IronCurtained=yes\n", Some(false), None, true, 0),
        FireError::Illegal
    );
    assert_eq!(
        error("CanTarget.IronCurtained=no\n", None, Some(true), false, 0),
        FireError::Illegal
    );
    // T21 resolves the selected slot before the curtain arm, so an empty
    // slot answers Cant rather than Illegal.
    assert_eq!(error("", None, None, false, 1), FireError::Cant);
}

/// Phobos `TechnoClass_CanFire`'s berzerk arm
/// (`Ext/Techno/Hooks.Firing.cpp:370-371`): `[CombatDamage] BerzerkTargeting=`
/// (default `all`) limits the houses a berzerk firer may target; a
/// non-berzerk firer is unaffected.
#[test]
fn berzerk_firers_respect_berzerk_targeting() {
    use crate::sim::combat::fire_error::FireError;
    let error = |combat: &str, berserk: bool| {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=TANK\n[InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
             [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
             [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [CombatDamage]\n{combat}"
        )))
        .expect("berzerk fire fixture rules");
        let mut sim = crate::sim::world::Simulation::new();
        spawn(
            &mut sim.substrate.entities,
            1,
            "TANK",
            EntityCategory::Unit,
            "Soviet",
            (5, 5),
        );
        spawn(
            &mut sim.substrate.entities,
            2,
            "TANK",
            EntityCategory::Unit,
            "Soviet",
            (7, 5),
        );
        sim.interner = test_interner();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .berserk
            .set_active_for_test(berserk);
        crate::sim::combat::fire_error_world::FireSubject {
            world: &sim,
            rules: &rules,
            fog: None,
            firer: sim.substrate.entities.get(1).unwrap(),
            obj: rules.object("TANK").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(2)),
            weapon_index: 0,
        }
        .fire_error(true)
    };
    assert_ne!(error("", false), FireError::Illegal, "not berzerk");
    assert_ne!(
        error("BerzerkTargeting=enemies\n", false),
        FireError::Illegal,
        "not berzerk, restricted global"
    );
    assert_ne!(
        error("", true),
        FireError::Illegal,
        "the default `all` admits the firer's own house"
    );
    assert_eq!(
        error("BerzerkTargeting=enemies\n", true),
        FireError::Illegal,
        "`enemies` refuses the firer's own house"
    );
}
