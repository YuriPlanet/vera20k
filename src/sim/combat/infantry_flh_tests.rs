//! Phobos `TechnoClass::GetFLH @ 0x006F3AEB`'s infantry arms: the deployed and
//! prone burst rows, then `InfantryExt::GetSimpleFLH`, through the production
//! fire coordinate.

use super::fire_coord::{FireSource, fire_coordinate};
use super::laser;
use super::{TargetKind, fire_coord};
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::flh::Flh;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionLeafState;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{LifecycleOutput, Simulation};
use crate::util::fixed_math::SimFixed;

const ART: &str = "[EUART]\nPrimaryFireFLH=100,20,30\n\
                  PrimaryFireFLH.Burst0=200,40,50\n\
                  DeployedPrimaryFireFLH.Burst0=300,60,70\n\
                  PronePrimaryFireFLH.Burst0=400,80,90\n\
                  [DEPART]\nPrimaryFireFLH=300,60,70\n\
                  [PRONART]\nPrimaryFireFLH=400,80,90\n\
                  [BASEART]\nPrimaryFireFLH=200,40,50\n\
                  [SIMPART]\nPrimaryFireFLH=100,20,30\n\
                  DeployedPrimaryFireFLH=500,10,10\n\
                  [SIMCART]\nPrimaryFireFLH=500,10,10\n\
                  [CHASERART]\nPrimaryFireFLH=0,0,0\n";

fn rules() -> RuleSet {
    let art_ini = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[InfantryTypes]\n0=EU\n1=DEP\n2=PRO\n3=BASE\n4=SIMP\n5=SIMC\n\
             [VehicleTypes]\n0=CHASER\n\
             [EU]\nImage=EUART\nStrength=100\nPrimary=GUN\n\
             [DEP]\nImage=DEPART\nStrength=100\nPrimary=GUN\n\
             [PRO]\nImage=PRONART\nStrength=100\nPrimary=GUN\n\
             [BASE]\nImage=BASEART\nStrength=100\nPrimary=GUN\n\
             [SIMP]\nImage=SIMPART\nStrength=100\nPrimary=GUN\n\
             [SIMC]\nImage=SIMCART\nStrength=100\nPrimary=GUN\n\
             [CHASER]\nImage=CHASERART\nStrength=1000\n\
             [GUN]\nIsLaser=yes\nIsHouseColor=yes\nLaserDuration=15\n\
             LaserInnerColor=100,120,140\nLaserOuterColor=10,20,30\n\
             LaserOuterSpread=0,0,0\nLaserPositionUpdate=Firer\n",
        ),
        &art_ini,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
}

fn spawn(sim: &mut Simulation, id: u64, kind: &str, deployed: bool, prone: bool) {
    let mut entity = GameEntity::test_default(id, kind, "Americans", 10, 10);
    entity.set_type_ref_for_test(sim.intern(kind));
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_category_for_test(if kind == "CHASER" {
        EntityCategory::Unit
    } else {
        EntityCategory::Infantry
    });
    entity.position.set_sub_x_for_test(SimFixed::from_num(128));
    entity.position.set_sub_y_for_test(SimFixed::from_num(128));
    entity.position.set_exact_z_leptons_for_test(Some(0));
    entity.type_facts.set_foundation_for_test("1x1".into());
    if deployed {
        entity.mission_leaf = MissionLeafState::infantry_raw_for_test(0, 28);
    }
    if prone {
        entity
            .infantry
            .get_or_insert_with(Default::default)
            .set_is_prone_for_test(true);
    }
    sim.substrate.entities.insert(entity);
}

/// `kind` under the given states, against a control type that carries the
/// expected offset in its ordinary primary slot at the same coordinate.
fn assert_flh_matches(kind: &str, deployed: bool, prone: bool, control: &str, what: &str) {
    let mut sim = Simulation::new();
    let rules = rules();
    spawn(&mut sim, 1, kind, deployed, prone);
    spawn(&mut sim, 2, control, false, false);
    let shot = |id: u64| {
        let source = sim.substrate.entities.get(id).unwrap();
        fire_coordinate(
            &sim,
            &rules,
            &FireSource::of_entity(source),
            rules.object(if id == 1 { kind } else { control }).unwrap(),
            0,
            0,
            Flh::default(),
        )
        .coord
    };
    let expected = shot(2);
    assert_eq!(
        shot(1),
        expected,
        "{what}: {kind} deployed={deployed} prone={prone}"
    );
    assert_ne!(expected, ProjectileCoord::new(0, 0, 0));
}

#[test]
fn an_undeployed_infantryman_keeps_the_type_burst_rows() {
    assert_flh_matches("EU", false, false, "BASE", "PrimaryFireFLH.Burst0");
}

#[test]
fn a_deployed_infantryman_prefers_its_own_burst_rows() {
    assert_flh_matches("EU", true, false, "DEP", "DeployedPrimaryFireFLH.Burst0");
}

#[test]
fn a_prone_infantryman_prefers_its_own_burst_rows() {
    assert_flh_matches("EU", false, true, "PRO", "PronePrimaryFireFLH.Burst0");
}

#[test]
fn deployed_wins_over_prone_in_the_burst_rows() {
    // Both states on, the deployed row set (300,60,70) answers, so the same
    // control matches only the deployed offset.
    assert_flh_matches("EU", true, true, "DEP", "IsDeployed before Crawling");
}

#[test]
fn a_state_vector_without_rows_falls_to_the_simple_override() {
    // SIMPART has DeployedPrimaryFireFLH but no deployed burst row, so
    // GetBurstFLH finds nothing and GetSimpleFLH supplies the offset.
    assert_flh_matches("SIMP", true, false, "SIMC", "InfantryExt::GetSimpleFLH");
}

#[test]
fn a_tracked_laser_freezes_the_deployed_burst_offset() {
    let mut sim = Simulation::new();
    let rules = rules();
    spawn(&mut sim, 1, "EU", true, false);
    spawn(&mut sim, 2, "CHASER", false, false);
    laser::fired(
        &mut sim,
        &rules,
        1,
        TargetKind::Entity(2),
        0,
        rules.weapon("GUN").unwrap(),
        0,
    );
    let birth = *sim
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::LaserCreated(birth) => Some(birth),
            _ => None,
        })
        .last()
        .expect("laser birth");
    let track = birth.track.expect("Firer tracking");
    assert_eq!(
        track.local_flh(),
        Flh::from([300, 60, 70]),
        "SetLaserTrackingData stores GetBurstFLH's deployed row"
    );
}
