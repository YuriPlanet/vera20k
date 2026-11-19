//! Phobos `OpenTopped.DecloakToFire` through the production fire visit:
//! a passenger admitted by GetFireError uncloaks its open-topped carrier.
use super::world_fire;
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::combat::TargetKind;
use crate::sim::movement::FacingClass;
use crate::sim::world::{FrameEffects, Simulation};

/// A carrier `CARR` (open-topped) with a `RIDER` passenger holding a `GUN`
/// target two cells east. `decloak_to_fire` appends the carrier's key.
fn scene(decloak_to_fire: bool) -> (Simulation, RuleSet, u64, u64) {
    let carrier_key = if decloak_to_fire {
        "OpenTopped.DecloakToFire=yes\n"
    } else {
        ""
    };
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=CARR\n1=RIDER\n2=DUMMY\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=5\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\n\
         Warhead=HIT\nFireInTransport=yes\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CARR]\nStrength=800\nArmor=heavy\nSpeed=5\nPassengers=1\nOpenTopped=yes\n{carrier_key}\
         [RIDER]\nStrength=200\nArmor=none\nSpeed=4\nPrimary=GUN\nOmniFire=yes\n\
         [DUMMY]\nStrength=1000\nArmor=heavy\nCanPassiveAquire=no\n"
    ));
    let rules = RuleSet::from_ini(&ini).expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0D_EC_10);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(100);
    let carrier = sim
        .spawn_object("CARR", "Americans", 10, 10, 0, &rules)
        .expect("carrier");
    let rider = sim
        .spawn_object("RIDER", "Americans", 12, 10, 0, &rules)
        .expect("rider");
    let target = sim
        .spawn_object("DUMMY", "Russians", 14, 10, 0, &rules)
        .expect("target");
    for id in [carrier, rider, target] {
        let entity = sim
            .entities_mut_for_test()
            .get_mut(id)
            .expect("placed object");
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let owner = sim.entities().get(target).map(|entity| entity.owner());
    let victim = sim
        .entities()
        .get(target)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("target");
    sim.fog
        .mark_visible_for_owner(owner.expect("owner"), victim.0, victim.1);
    let east = 0x4000;
    let actor = sim.entities_mut_for_test().get_mut(rider).expect("rider");
    actor.body_facing.snap(east, 100);
    actor.barrel_facing = Some(FacingClass::new(east, 0));
    actor.passenger_role.set_inside_transport(carrier, true);
    let mut cloak = CloakRuntime::new(100);
    cloak.establish_unlimbo_fully_cloaked();
    sim.entities_mut_for_test()
        .get_mut(carrier)
        .expect("carrier")
        .cloak = Some(Box::new(cloak));
    assert!(
        sim.assign_target_represented(
            rider,
            Some(TargetKind::Entity(target)),
            &rules,
            None,
            FrameEffects::empty(),
        )
        .is_ok(),
        "the rider holds the target"
    );
    (sim, rules, carrier, rider)
}

#[test]
fn a_firing_open_topped_passenger_uncloaks_its_carrier() {
    for (decloak, expected) in [(true, 3), (false, 2)] {
        let (mut sim, rules, carrier, rider) = scene(decloak);
        assert_eq!(
            sim.entities()
                .get(carrier)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state(),
            2,
            "the carrier starts fully cloaked"
        );
        sim.commit_fire_visit(
            world_fire::FireVisit::UnitTarget(rider),
            &rules,
            None,
            FrameEffects::empty(),
        );
        assert_eq!(
            sim.entities()
                .get(carrier)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state(),
            expected,
            "DecloakToFire={decloak}: the carrier begins uncloaking only with the key"
        );
    }
}
