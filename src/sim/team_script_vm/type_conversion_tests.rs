//! TeamClass_Update6E9443/Ares10039D7C through the production script step.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation};

fn scene() -> (Simulation, RuleSet, u64, Vec<u64>) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=OLD\n1=NEW\n2=BAD\n\
         [InfantryTypes]\n0=FOOT\n1=FOOT2\n\
         [OLD]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nAmmo=6\nInitialAmmo=5\nConvert.Script=NEW\nHarvester=yes\nStorage=3\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [NEW]\nStrength=200\nSpeed=6\nSpeedType=Hover\nMovementZone=Normal\nAmmo=3\nInitialAmmo=0\nROT=8\nTurret=yes\nTurretROT=2\nStorage=2\n\
         Locomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\
         [BAD]\nStrength=100\nSpeed=5\nConvert.Script=FOOT2\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [FOOT]\nStrength=100\nSpeed=4\nConvert.Script=FOOT2\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [FOOT2]\nStrength=120\nSpeed=4\nROT=6\nHasSpotlight=yes\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
    );
    #[cfg(feature = "ares")]
    let ini = {
        let mut ini = ini;
        ini.merge(&crate::rules::ini_parser::IniFile::from_str(
            "[Tiberiums]\n0=CONVERT_ORE\n[CONVERT_ORE]\nValue=25\nImage=1\n\
             [FOOT]\nAttachEffect.Duration=3\nAttachEffect.InitialDelay=0\nAttachEffect.FirepowerMultiplier=2\n\
             [FOOT2]\nAttachEffect.Duration=3\nAttachEffect.InitialDelay=2\nAttachEffect.FirepowerMultiplier=3\n\
             Spotlight.StartHeight=650\nSpotlight.DisableGreen=yes\n",
        ));
        ini
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 19);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let members: Vec<_> = [("OLD", 10), ("FOOT", 12), ("BAD", 14)]
        .into_iter()
        .map(|(name, x)| {
            sim.spawn_object_at_height(name, "Americans", x, 10, 0, 0, &rules)
                .unwrap()
        })
        .collect();
    let actor = sim.substrate.entities.get_mut(members[0]).unwrap();
    actor.health.set_for_test(50);
    actor.estimated_health.reset(7);
    sim.substrate
        .entities
        .get_mut(members[1])
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .power_off();
    let owner = sim.intern("Americans");
    let script = sim.intern("CONVERSION");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: vec![
            TeamScriptAction {
                action_id: 68,
                argument: 99,
            },
            TeamScriptAction {
                action_id: 5,
                argument: 1,
            },
        ],
    });
    let team = sim
        .team_script_vm
        .create_team(owner, script, members.clone(), 0);
    (sim, rules, team, members)
}

#[cfg(feature = "ares")]
#[test]
fn script68_converts_in_place_keeps_team_and_updates_native_counts_health_ammo_and_locomotor() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::util::native_x87::NativeF64Bits;
    let (mut sim, rules, team, members) = scene();
    let cargo = vec![
        crate::sim::miner::CargoBale::of_type(
            crate::rules::tiberium_type::TiberiumTypeId(0),
            crate::sim::miner::ResourceType::Ore,
            &rules,
        )
        .unwrap(),
    ];
    sim.substrate
        .entities
        .get_mut(members[0])
        .unwrap()
        .miner
        .as_mut()
        .unwrap()
        .set_cargo_for_test(cargo.clone());
    sim.update_ares_attach_effects(members[1], &rules);
    assert_eq!(
        sim.entities()
            .get(members[1])
            .unwrap()
            .crate_multipliers
            .firepower(),
        NativeF64Bits::from_bits(2.0_f64.to_bits()),
    );
    let count = sim.entities().len();
    sim.take_lifecycle_outputs_for_test();
    sim.team_step_script(team, &rules, None, FrameEffects::empty());
    let changed: Vec<_> = sim
        .take_lifecycle_outputs_for_test()
        .into_iter()
        .filter_map(|output| match output {
            crate::sim::world::LifecycleOutput::ObjectTypeChanged { stable_id } => Some(stable_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        changed,
        members[..2],
        "new models must reach atlas refresh without an entity spawn"
    );
    assert_eq!(
        sim.entities().len(),
        count,
        "same instances, not replacements"
    );
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.members().collect::<Vec<_>>(), members);
    assert!(state.advance_pending());
    assert_eq!(state.cursor(), 0);
    let actor = sim.entities().get(members[0]).unwrap();
    assert_eq!(sim.interner.resolve(actor.type_ref()), "NEW");
    assert_eq!(
        (actor.health.current(), actor.estimated_health.get()),
        (100, 100)
    );
    assert_eq!(
        actor.ammo().unwrap().current(),
        3,
        "ignore constructor InitialAmmo"
    );
    assert_eq!(
        actor.locomotor.as_ref().unwrap().active_kind(),
        LocomotorKind::Hover
    );
    assert_eq!(actor.body_facing.rot_per_frame(), 8 << 8);
    assert!(!actor.is_harvester());
    let storage = actor.miner.as_ref().unwrap();
    assert_eq!(storage.capacity_bales(), 2);
    assert_eq!(
        storage.cargo(),
        cargo.as_slice(),
        "type change does not discard held ore"
    );
    assert_eq!(
        actor.barrel_facing.as_ref().unwrap().rot_per_frame(),
        2 << 8
    );
    let foot = sim.entities().get(members[1]).unwrap();
    assert_eq!(sim.interner.resolve(foot.type_ref()), "FOOT2");
    assert!(
        !foot.locomotor.as_ref().unwrap().is_powered(),
        "equal CLSIDs retain the instance"
    );
    assert_eq!(
        foot.body_facing.rot_per_frame(),
        6 << 8,
        "not Infantry constructor ROT127"
    );
    assert!(foot.barrel_facing.is_none(), "no phantom turret");
    assert_eq!(foot.crate_multipliers.firepower(), NativeF64Bits::ONE);
    sim.update_ares_attach_effects(members[1], &rules);
    assert_eq!(
        sim.entities()
            .get(members[1])
            .unwrap()
            .crate_multipliers
            .firepower(),
        NativeF64Bits::ONE,
        "conversion retains the initial-delay sentinel until the new type's first visit",
    );
    sim.update_ares_attach_effects(members[1], &rules);
    sim.update_ares_attach_effects(members[1], &rules);
    assert_eq!(
        sim.entities()
            .get(members[1])
            .unwrap()
            .crate_multipliers
            .firepower(),
        NativeF64Bits::from_bits(3.0_f64.to_bits()),
    );
    assert_eq!(
        sim.interner
            .resolve(sim.entities().get(members[2]).unwrap().type_ref()),
        "BAD"
    );
    let foot_id = members[1];
    let light_id = sim
        .entities()
        .get(foot_id)
        .unwrap()
        .building_light
        .light()
        .unwrap();
    let visit = |sim: &mut Simulation, id| {
        sim.object_ai_visit_one(id, Some(&rules), crate::sim::world::ObjectAiCtx::default());
    };
    visit(&mut sim, light_id);
    let before_turn = sim.building_spotlight(light_id, &rules).unwrap();
    assert_eq!(before_turn.glow_flags, 4);
    sim.detach_all_pointer_expired(foot_id, &rules, None, FrameEffects::empty());
    assert!(
        sim.building_spotlight(light_id, &rules).is_some(),
        "cloak-style detach retains the light's parent"
    );
    let frame = sim.session.binary_frame();
    let old_cloak = sim
        .substrate
        .entities
        .get_mut(foot_id)
        .unwrap()
        .cloak
        .take();
    let mut cloaked = crate::sim::cloak_disguise::CloakRuntime::new(frame as i32);
    cloaked.establish_unlimbo_fully_cloaked();
    sim.substrate.entities.get_mut(foot_id).unwrap().cloak = Some(cloaked);
    assert!(
        sim.building_spotlight(light_id, &rules).is_none(),
        "cloaked parent suppresses drawing without deleting its light"
    );
    sim.substrate.entities.get_mut(foot_id).unwrap().cloak = old_cloak;
    assert!(sim.building_spotlight(light_id, &rules).is_some());
    sim.substrate
        .entities
        .get_mut(foot_id)
        .unwrap()
        .body_facing
        .snap(0x4000, frame);
    visit(&mut sim, light_id);
    let after_turn = sim.building_spotlight(light_id, &rules).unwrap();
    assert_ne!(
        before_turn.coord, after_turn.coord,
        "the existing light follows the Foot's facing"
    );
    let actor_z = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(foot_id).unwrap().position,
    )
    .z;
    assert_eq!(after_turn.beam_origin[2], actor_z + 650);
    let owner = sim.interner.get("Americans").unwrap();
    let tracking = sim.houses.get(&owner).unwrap().tracking();
    assert_eq!(
        tracking.owned_count(EntityCategory::Unit, sim.interner.get("OLD").unwrap()),
        0
    );
    assert_eq!(
        tracking.owned_count(EntityCategory::Unit, sim.interner.get("NEW").unwrap()),
        1
    );
    assert_eq!(
        tracking.active_count(EntityCategory::Unit, sim.interner.get("NEW").unwrap()),
        1
    );
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "conversion", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    assert_eq!(
        restored
            .entities()
            .get(members[0])
            .unwrap()
            .miner
            .as_ref()
            .unwrap()
            .cargo(),
        cargo.as_slice()
    );
    assert_eq!(
        restored
            .entities()
            .get(foot_id)
            .unwrap()
            .building_light
            .light(),
        Some(light_id)
    );
    assert!(restored.building_spotlight(light_id, &rules).is_some());
    restored.techno_deactivate(foot_id, &rules, FrameEffects::empty());
    assert!(
        restored
            .entities()
            .get(foot_id)
            .unwrap()
            .building_light
            .light()
            .is_none()
    );
    assert!(restored.building_spotlight(light_id, &rules).is_none());
    restored.techno_reactivate(foot_id, &rules, FrameEffects::empty());
    let recreated = restored
        .entities()
        .get(foot_id)
        .unwrap()
        .building_light
        .light()
        .unwrap();
    assert_ne!(recreated, light_id);
    restored.team_step_script(team, &rules, None, FrameEffects::empty());
    assert_eq!(restored.team_script_vm.team(team).unwrap().cursor(), 1);
    assert_eq!(
        restored
            .team_script_vm
            .team_for_member(members[0])
            .map(|(team, _)| team),
        Some(team)
    );
    assert_eq!(
        restored
            .interner
            .resolve(restored.entities().get(members[0]).unwrap().type_ref()),
        "NEW"
    );
    restored.techno_limbo_with_rules(foot_id, &rules, None);
    assert!(restored.building_spotlight(recreated, &rules).is_none());
    let fresh = restored
        .spawn_object_at_height("FOOT2", "Americans", 16, 12, 0, 0, &rules)
        .unwrap();
    assert!(
        restored
            .entities()
            .get(fresh)
            .unwrap()
            .building_light
            .light()
            .is_some(),
        "normal Foot placement uses the same light constructor"
    );
}

#[cfg(not(feature = "ares"))]
#[test]
fn default_build_keeps_script68_inert_without_type_or_count_writes() {
    let (mut sim, rules, team, members) = scene();
    sim.team_step_script(team, &rules, None, FrameEffects::empty());
    assert_eq!(
        sim.interner
            .resolve(sim.entities().get(members[0]).unwrap().type_ref()),
        "OLD"
    );
    assert_eq!(sim.entities().get(members[0]).unwrap().health.current(), 50);
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.cursor(), 0);
    assert!(!state.advance_pending());
    let owner = sim.interner.get("Americans").unwrap();
    assert_eq!(
        sim.houses
            .get(&owner)
            .unwrap()
            .tracking()
            .owned_count(EntityCategory::Unit, sim.interner.get("OLD").unwrap()),
        1
    );
    let foot = sim
        .spawn_object_at_height("FOOT2", "Americans", 16, 10, 0, 0, &rules)
        .unwrap();
    assert!(
        sim.entities()
            .get(foot)
            .unwrap()
            .building_light
            .light()
            .is_none(),
        "stock Foot ignores HasSpotlight"
    );
}
