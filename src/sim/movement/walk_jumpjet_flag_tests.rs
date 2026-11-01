//! E1's first stationary Move with a map-authored JumpJet flag and its retail Walk.
use crate::rules::{ini_parser::IniFile, locomotor_type::LocomotorKind, ruleset::RuleSet};
use crate::sim::{mission::MissionId, world::Simulation};

fn mapped_gi() -> Option<(
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    u64,
)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str("[E1]\nJumpJet=yes\n"),
    )?
    .rules;
    let object = rules.object("E1").unwrap();
    assert!(object.jumpjet);
    assert_eq!(object.locomotor, LocomotorKind::Walk);
    // Retail layered rules on the existing flat entry map, not a Hills map load.
    let (mut sim, _, registry) = crate::sim::world::entry_test_fixture::fixture();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let id = sim
        .spawn_object("E1", "Americans", 15, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .mission_assign_exact(
            id,
            MissionId::from_known(crate::rules::mission_data::MissionType::Move),
            sim.session.binary_frame(),
        )
        .unwrap();
    assert!(
        sim.assign_infantry_destination(
            id,
            crate::sim::components::NavTargetRef::cell(16, 15),
            &rules,
            Some(&registry),
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap()
    );
    Some((sim, rules, registry, id))
}

#[test]
fn retail_e1_jumpjet_flag_keeps_walk_through_first_short_move() {
    let Some((mut sim, rules, registry, id)) = mapped_gi() else {
        return;
    };
    // Original gamemd.exe 75B2F1 -> Infantry+4F8 @00521EB0: a Walk's
    // CLSID fails the Jumpjet comparison even when the type's flag is set.
    for _ in 0..100 {
        sim.advance_app_frame(
            &[],
            &rules,
            Some(&registry),
            67,
            crate::sim::world::TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("a Walk short path must not abort the frame");
        if sim
            .entities()
            .get(id)
            .unwrap()
            .navigation
            .nav_com()
            .is_none()
        {
            break;
        }
    }
    let actor = sim.entities().get(id).unwrap();
    assert_eq!((actor.position.rx(), actor.position.ry()), (16, 15));
    assert!(actor.navigation.nav_com().is_none());
    let locomotor = actor.locomotor.as_ref().unwrap();
    assert_eq!(locomotor.active_kind(), LocomotorKind::Walk);
    assert!(locomotor.piggyback().is_none());
    assert!(locomotor.walk_destination().is_none());
}

#[test]
fn retail_e1_jumpjet_flag_failed_path_receiver_preserves_foot_prefix() {
    use crate::rules::mission_data::MissionType;
    let Some((mut sim, rules, _, id)) = mapped_gi() else {
        return;
    };
    // Original gamemd.exe Infantry+4F4 @00521DD0 only discards GetClassID
    // after Foot4DC030: Move preserves navigation; Hunt clears it.
    let before = sim.state_hash();
    sim.walk_failed_path_receiver(id, &rules, crate::sim::world::FrameEffects::default())
        .unwrap();
    assert_eq!(sim.state_hash(), before);
    sim.substrate
        .mission_assign_exact(
            id,
            MissionId::from_known(MissionType::Hunt),
            sim.session.binary_frame(),
        )
        .unwrap();
    sim.walk_failed_path_receiver(id, &rules, crate::sim::world::FrameEffects::default())
        .unwrap();
    let actor = sim.entities().get(id).unwrap();
    assert!(actor.navigation.nav_com().is_none());
    assert!(actor.attack_target.target().is_none());
    assert!(
        actor
            .locomotor
            .as_ref()
            .unwrap()
            .walk_destination()
            .is_none()
    );
    assert_eq!(
        actor.locomotor.as_ref().unwrap().active_kind(),
        LocomotorKind::Walk
    );
}
