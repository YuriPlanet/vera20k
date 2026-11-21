//! Receiver responses to a hit: Building attack frames, the protected-Techno
//! response, infantry scatter and fear, berserk targeting and the hostile
//! Building hit latch for AI repair.

use super::*;

fn gsi_04_05_attack_frame_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=SOURCE\n1=PROTECTED\n\
         [BuildingTypes]\n0=NORMAL\n1=MOD1X1\n2=STOCK2X2\n3=SELFNO\n4=SELFYES\n5=IMMUNE\n6=PROTECTEDBLDG\n\
         [Warheads]\n0=HITWH\n1=SELFWH\n\
         [SOURCE]\nStrength=100\nArmor=heavy\n\
         [PROTECTED]\nStrength=100\nArmor=heavy\nToProtect=yes\n\
         [NORMAL]\nStrength=100\nArmor=wood\n\
         [MOD1X1]\nStrength=100\nArmor=wood\nUndeploysInto=MODUNIT\n\
         [STOCK2X2]\nStrength=100\nArmor=wood\nUndeploysInto=MODUNIT\n\
         [SELFNO]\nStrength=100\nArmor=wood\nDamageSelf=no\n\
         [SELFYES]\nStrength=100\nArmor=wood\nDamageSelf=yes\n\
         [IMMUNE]\nStrength=100\nArmor=wood\nImmune=yes\n\
         [PROTECTEDBLDG]\nStrength=100\nArmor=wood\nToProtect=yes\n\
         [HITWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SELFWH]\nAllowDamageOnSelf=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let art = IniFile::from_str(
        "[NORMAL]\nFoundation=2x2\n\
         [MOD1X1]\nFoundation=1x1\n\
         [STOCK2X2]\nFoundation=2x2\n\
         [SELFNO]\nFoundation=2x2\n\
         [SELFYES]\nFoundation=2x2\n\
         [IMMUNE]\nFoundation=2x2\n\
         [PROTECTEDBLDG]\nFoundation=2x2\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("House attack-frame rules parse")
}

#[test]
fn gsi_04_05_building_attack_frame_prelude_obeys_object_and_type_gates() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    for (id, type_name) in [
        (2, "NORMAL"),
        (3, "MOD1X1"),
        (4, "STOCK2X2"),
        (5, "SELFNO"),
        (6, "SELFYES"),
    ] {
        let mut target = make_entity_owned(id, type_name, id as u16 + 3, 4, 100, "Victim");
        target.set_category_for_test(EntityCategory::Structure);
        entities.insert(target);
    }
    let mut unit_target = make_entity_owned(7, "SOURCE", 10, 4, 100, "Victim");
    unit_target.set_category_for_test(EntityCategory::Unit);
    entities.insert(unit_target);
    entities.insert(make_entity_owned(8, "SOURCE", 11, 4, 100, "Victim"));
    let mut dead_target = make_entity_owned(9, "NORMAL", 12, 4, 0, "Victim");
    dead_target.set_category_for_test(EntityCategory::Structure);
    entities.insert(dead_target);

    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = computer_houses(&[victim_owner]);
    let frame = |houses: &crate::sim::houses::Houses| {
        houses[&victim_owner]
            .strategy_emergency()
            .last_building_attack_frame()
    };

    let event = EntityDamageEvent::area(2, -7, 0, 1, Some(source_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&event, &entities, &rules, &interner, &mut houses, 41),
        BuildingReceivePrelude::Respond
    );
    assert_eq!(
        frame(&houses),
        41,
        "negative damage still records the frame"
    );

    let event = EntityDamageEvent::area(2, 0, 0, 1, Some(victim_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&event, &entities, &rules, &interner, &mut houses, 42),
        BuildingReceivePrelude::Respond
    );
    assert_eq!(frame(&houses), 42, "alliance and zero damage are not gates");

    let null_attacker = EntityDamageEvent::area(2, 10, 0, RAD_NO_ATTACKER, Some(source_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(
            &null_attacker,
            &entities,
            &rules,
            &interner,
            &mut houses,
            43,
        ),
        BuildingReceivePrelude::Continue,
        "0x0044227E skips the +0x54DC write and RespondToBaseAttack too"
    );
    assert_eq!(
        frame(&houses),
        42,
        "source House cannot replace a null object"
    );

    let removed_attacker = EntityDamageEvent::area(2, 10, 0, 999, None, wh);
    apply_building_receive_prelude(
        &removed_attacker,
        &entities,
        &rules,
        &interner,
        &mut houses,
        43,
    );
    assert_eq!(
        frame(&houses),
        43,
        "the retained non-null object argument does not require a live lookup"
    );

    let no_source_house = EntityDamageEvent::area(2, 10, 0, 1, None, wh);
    apply_building_receive_prelude(
        &no_source_house,
        &entities,
        &rules,
        &interner,
        &mut houses,
        44,
    );
    assert_eq!(
        frame(&houses),
        44,
        "source House is not required for the write"
    );

    let allied = EntityDamageEvent::area(2, 10, 0, 8, Some(victim_owner), wh);
    apply_building_receive_prelude(&allied, &entities, &rules, &interner, &mut houses, 45);
    assert_eq!(
        frame(&houses),
        45,
        "a distinct attacker owned by the victim House still writes"
    );

    let modded_skip = EntityDamageEvent::area(3, 10, 0, 1, Some(source_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&modded_skip, &entities, &rules, &interner, &mut houses, 46),
        BuildingReceivePrelude::Continue,
        "the vtable +0x80 skip covers +0x54DC and RespondToBaseAttack"
    );
    assert_eq!(
        frame(&houses),
        45,
        "a 1x1 undeployer takes vtable +0x80 skip"
    );

    let stock_shape = EntityDamageEvent::area(4, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&stock_shape, &entities, &rules, &interner, &mut houses, 47);
    assert_eq!(
        frame(&houses),
        47,
        "larger retail-shaped undeployer records"
    );

    let already_dead = EntityDamageEvent::area(9, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&already_dead, &entities, &rules, &interner, &mut houses, 48);
    assert_eq!(frame(&houses), 48, "Health zero is later than this prelude");

    let self_no = EntityDamageEvent::area(5, 10, 0, 5, Some(victim_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&self_no, &entities, &rules, &interner, &mut houses, 49,),
        BuildingReceivePrelude::ReturnZero
    );
    assert_eq!(frame(&houses), 48);

    let self_yes = EntityDamageEvent::area(6, 10, 0, 6, Some(victim_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(
            &self_yes,
            &entities,
            &rules,
            &interner,
            &mut houses,
            0x1_8000_0001,
        ),
        BuildingReceivePrelude::Respond
    );
    assert_eq!(
        frame(&houses),
        i32::MIN + 1,
        "native stores the raw low dword"
    );

    let unit = EntityDamageEvent::area(7, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&unit, &entities, &rules, &interner, &mut houses, 47);
    assert_eq!(frame(&houses), i32::MIN + 1, "the wrapper is Building-only");

    // Phobos `BuildingClass_ReceiveDamage_DamageSelf @ 0x0044224F` skips the
    // `0x00442262` self return when the warhead admits the firer.
    #[cfg(feature = "phobos")]
    {
        let allow_wh = interner.intern("SELFWH");
        let allow_self = EntityDamageEvent::area(5, 10, 0, 5, Some(victim_owner), allow_wh);
        assert_eq!(
            apply_building_receive_prelude(
                &allow_self,
                &entities,
                &rules,
                &interner,
                &mut houses,
                50,
            ),
            BuildingReceivePrelude::Respond,
            "DamageSelf=no building admits its own AllowDamageOnSelf warhead"
        );
    }
}

#[test]
fn gsi_04_05_building_attack_frame_precedes_immune_receiver_exit() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    let mut target = make_entity_owned(2, "IMMUNE", 5, 4, 100, "Victim");
    target.set_category_for_test(EntityCategory::Structure);
    entities.insert(target);
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = computer_houses(&[victim_owner, source_owner]);
    let mut trace_hook = FixtureTrace::default();

    commit_hits(
        &[EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh)],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        77,
        Some(&mut trace_hook),
    );

    assert_eq!(entities.get(2).unwrap().health.current(), 100);
    assert_eq!(
        trace_hook.entries,
        vec![BaseDefenseResponseTraceEntry {
            site: BaseDefenseResponseCallSite::BuildingPrelude,
            victim_id: 2,
            health: 100,
            last_attacker_house_index: 1,
        }],
        "Building response runs after the attacker-House index write and before immunity"
    );
    assert_eq!(
        houses[&victim_owner]
            .strategy_emergency()
            .last_building_attack_frame(),
        77,
        "the House write happens before ObjectClass immunity rejects damage"
    );
}

/// `BuildingClass::ReceiveDamage @ 0x00442230` skips `House+0x54D8`,
/// `+0x54DC` and `RespondToBaseAttack @ 0x00708080` for a null attacker
/// (`0x0044227E`), as BlowUpBridge passes: the victim House keeps its last
/// attacker index and no Building response runs.
#[test]
fn gsi_04_05_null_attacker_building_hit_keeps_the_attacker_index() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    let mut target = make_entity_owned(2, "NORMAL", 5, 4, 100, "Victim");
    target.set_category_for_test(EntityCategory::Structure);
    entities.insert(target);
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = computer_houses(&[victim_owner, source_owner]);
    let mut trace_hook = FixtureTrace::default();

    commit_hits(
        &[EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh)],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        60,
        Some(&mut trace_hook),
    );
    commit_hits(
        &[EntityDamageEvent::area(
            2,
            10,
            0,
            RAD_NO_ATTACKER,
            Some(source_owner),
            wh,
        )],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        61,
        Some(&mut trace_hook),
    );

    assert_eq!(entities.get(2).unwrap().health.current(), 80);
    let emergency = houses[&victim_owner].strategy_emergency();
    assert_eq!(emergency.last_attacker_house_index(), 1);
    assert_eq!(emergency.last_building_attack_frame(), 60);
    assert_eq!(
        trace_hook
            .entries
            .iter()
            .filter(|entry| entry.site == BaseDefenseResponseCallSite::BuildingPrelude)
            .count(),
        1,
        "only the sourced hit runs the Building response"
    );
}

/// `BuildingClass::ReceiveDamage @ 0x00442230` returns 0 at `0x004423A3`
/// for the retail CABHUT (`BridgeRepairHut=yes`, `Immune=yes`) before
/// `TechnoClass::ReceiveDamage @ 0x00701900`: an ordinary sourced area hit,
/// even under an Iron Curtain, resolves no receiver outcome (no Techno
/// gates, impact light, anger or threat tail), and a WaveClass caller's
/// shared damage stays unchanged.
#[test]
fn retail_bridge_repair_hut_area_hit_returns_before_the_techno_receiver() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let mut sim = crate::sim::world::Simulation::new();
    let source = sim
        .spawn_object("MTNK", "Russians", 4, 4, 0, &rules)
        .expect("retail MTNK spawns");
    let source_owner = sim.substrate.entities.get(source).unwrap().owner();
    let hut = sim
        .spawn_object("CABHUT", "Neutral", 8, 4, 0, &rules)
        .expect("retail CABHUT spawns");
    let strength = rules.object("CABHUT").unwrap().strength;
    let warhead = sim.interner.intern("AP");
    assert!(rules.warhead("AP").is_some());
    let event = EntityDamageEvent::area(hut, 100, 0, source, Some(source_owner), warhead);
    for iron_curtain in [false, true] {
        sim.substrate.entities.get_mut(hut).unwrap().invulnerability = iron_curtain.then(|| {
            InvulnerabilityState::new(
                crate::sim::timer::CdTimer::started(0, 100),
                InvulnKind::IronCurtain,
            )
        });
        let resolve = |sim: &crate::sim::world::Simulation| {
            resolve_receive_damage(
                &event,
                &sim.substrate.entities,
                &rules,
                &sim.interner,
                &sim.houses,
                &sim.house_alliances,
                false,
                0,
                None,
                None,
            )
            .is_none()
        };
        assert!(resolve(&sim), "IC {iron_curtain}: no receiver outcome");
        let (_, packet) = world_receiver::commit_receiver(
            &mut sim,
            &mut world_receiver::ReceiverRun::default(),
            combat_aoe::AreaDamageReceiver::Entity(event),
            &rules,
            None,
            &mut Vec::new(),
        );
        assert_eq!(
            packet, 100,
            "IC {iron_curtain}: the hut kept the incoming packet"
        );
    }
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[event],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate.entities.get(hut).unwrap().health.current(),
        strength
    );
}

/// `0x00442358..0x004423A0`: a `LaserFence=` Building returns 0 before
/// Techno damage unless ignoreDefenses is set. Dormant in retail.
#[test]
fn laser_fence_building_returns_before_techno_damage_without_ignore_defenses() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[VehicleTypes]\n0=SOURCE\n[BuildingTypes]\n0=FENCE\n[Warheads]\n0=HITWH\n\
             [SOURCE]\nStrength=100\nArmor=heavy\n\
             [FENCE]\nStrength=100\nArmor=wood\nLaserFence=yes\n\
             [HITWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ),
        &IniFile::from_str("[FENCE]\nFoundation=1x1\n"),
    )
    .expect("LaserFence rules parse");
    assert!(rules.object("FENCE").unwrap().laser_fence);
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    let mut fence = make_entity_owned(2, "FENCE", 5, 4, 100, "Victim");
    fence.set_category_for_test(EntityCategory::Structure);
    entities.insert(fence);
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = computer_houses(&[victim_owner, source_owner]);

    commit_hits(
        &[EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh)],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        70,
        None,
    );
    assert_eq!(entities.get(2).unwrap().health.current(), 100);
    assert_eq!(
        houses[&victim_owner]
            .strategy_emergency()
            .last_building_attack_frame(),
        70,
        "the attacker bookkeeping precedes the gate"
    );

    commit_hits(
        &[EntityDamageEvent::direct_receiver(
            2,
            10,
            0,
            1,
            Some(source_owner),
            wh,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        )],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        71,
        None,
    );
    assert_eq!(entities.get(2).unwrap().health.current(), 90);
}

#[test]
fn gsi_04_05_protected_techno_response_runs_after_object_health_commit() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    entities.insert(make_entity_owned(2, "PROTECTED", 5, 4, 100, "Victim"));
    let mut protected_building = make_entity_owned(3, "PROTECTEDBLDG", 6, 4, 100, "Victim");
    protected_building.set_category_for_test(EntityCategory::Structure);
    entities.insert(protected_building);
    entities.insert(make_entity_owned(4, "SOURCE", 7, 4, 100, "Victim"));

    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = computer_houses(&[victim_owner, source_owner]);
    let mut trace_hook = FixtureTrace::default();

    commit_hits(
        &[
            EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(3, 10, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(4, 10, 0, 1, Some(source_owner), wh),
        ],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        91,
        Some(&mut trace_hook),
    );

    assert_eq!(
        trace_hook.entries,
        vec![
            BaseDefenseResponseTraceEntry {
                site: BaseDefenseResponseCallSite::ProtectedTechno,
                victim_id: 2,
                health: 90,
                last_attacker_house_index: -1,
            },
            BaseDefenseResponseTraceEntry {
                site: BaseDefenseResponseCallSite::BuildingPrelude,
                victim_id: 3,
                health: 100,
                last_attacker_house_index: 1,
            },
            BaseDefenseResponseTraceEntry {
                site: BaseDefenseResponseCallSite::ProtectedTechno,
                victim_id: 3,
                health: 90,
                last_attacker_house_index: 1,
            },
        ],
        "ToProtect calls once after Object health; Buildings first run their wrapper response"
    );
    assert_eq!(entities.get(2).unwrap().health.current(), 90);
    assert_eq!(entities.get(3).unwrap().health.current(), 90);
    assert_eq!(entities.get(4).unwrap().health.current(), 90);
}

/// The protected-Techno call is the tail of result 1's own arm (switch
/// `0x00702D24` entry 1, `0x007027AE..0x007027EE`): a hit that crosses into
/// yellow or red, or kills, never reaches it (#1422). Nor does a result-1
/// hit on an object whose owner a human controls (`0x0050B730` at
/// `0x007027D2`): in GameMode 0 that includes a `PlayerControl=` house.
#[test]
fn gsi_04_05_protected_techno_response_runs_only_on_result_one() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    for id in 2..=6 {
        let owner = if id == 6 { "Ally" } else { "Victim" };
        let mut victim = make_entity_owned(id, "PROTECTED", id as u16 + 3, 4, 100, owner);
        // A survivor's scatter tail asks its locomotor whether it moves.
        victim.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        entities.insert(victim);
    }
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let ally_owner = interner.intern("Ally");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut ally = HouseState::new(ally_owner, 0, None, false, 0, 10);
    ally.set_player_control_for_test(true);
    let mut houses = [
        HouseState::new(victim_owner, 0, None, false, 0, 10),
        ally,
        HouseState::new(source_owner, 1, None, false, 0, 10),
    ]
    .into_iter()
    .collect::<crate::sim::houses::Houses>();
    let mut trace_hook = FixtureTrace::default();

    // Result 1, a yellow crossing, a red crossing, a kill, and result 1 on
    // the PlayerControl house's object.
    commit_hits(
        &[
            EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(3, 60, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(4, 80, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(5, 100, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(6, 10, 0, 1, Some(source_owner), wh),
        ],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        91,
        Some(&mut trace_hook),
    );

    assert_eq!(
        trace_hook.entries,
        vec![BaseDefenseResponseTraceEntry {
            site: BaseDefenseResponseCallSite::ProtectedTechno,
            victim_id: 2,
            health: 90,
            last_attacker_house_index: -1,
        }],
        "only the computer's result-1 hit reaches the protected-Techno call"
    );
    let health = |id| entities.get(id).map_or(0, |entity| entity.health.current());
    assert_eq!(
        [health(2), health(3), health(4), health(5), health(6)],
        [90, 40, 20, 0, 90]
    );
}

/// A hit without a source takes `TechnoClass::ReceiveDamage`'s refused
/// scatter arm (`ShouldRetaliate` answers 0; `0x00702BFE`): a computer's
/// guarding man scatters at once (`Scatter(null, 1, 0)`, whose null arm
/// spends a Scenario draw), a human's rookie does not. Either way
/// InfantryClass then skips its source Scatter and takes the fear ladder
/// (`0x00518C47`): 12 at 90% health, not the source latch's 100.
#[test]
fn sourceless_infantry_hit_takes_the_refused_scatter_arm_and_the_fear_ladder() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=GI\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=HITWH\n[GI]\nStrength=100\nArmor=none\n\
         [HITWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    for human in [false, true] {
        let mut victim = make_infantry_entity(2, "GI", 5, 5, 100);
        victim.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ),
        );
        set_mission(&mut victim, MissionType::Guard, MissionId::NONE);
        let mut entities = EntityStore::new();
        entities.insert(victim);
        let mut interner = test_interner();
        let owner = interner.intern("Test");
        let wh = interner.intern("HITWH");
        let mut houses = [HouseState::new(owner, 0, None, human, 0, 10)]
            .into_iter()
            .collect::<crate::sim::houses::Houses>();
        let mut main_rng = SimRng::new(5);
        let mut scenario_rng = SimRng::new(7);
        let scenario_before = scenario_rng.state();

        let _ = commit_damage_events(
            &[EntityDamageEvent::area(2, 10, 0, RAD_NO_ATTACKER, None, wh)],
            &mut entities,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            &mut houses,
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut Vec::new(),
            None,
            None,
            None,
            40,
            &mut None,
            &mut None,
        );

        let victim = entities.get(2).unwrap();
        assert_eq!(victim.health.current(), 90);
        assert_eq!(
            victim.infantry.as_ref().unwrap().fear_level(),
            12,
            "human={human}"
        );
        assert_eq!(
            scenario_rng.state() != scenario_before,
            !human,
            "human={human}"
        );
    }
}

/// Stock rules through the production receiver (#1327). Retail
/// `PlayerScatter=no` and `[TANY] VeteranAbilities=...SCATTER`: a human's
/// veteran Tanya hit without a source passes the refused arm's rank gate
/// (`0x00702CA2..0x00702CFC`) and steps to a nearby cell; as a rookie, or
/// with a NavCom, she stays. A computer's rookie GI passes by ownership
/// alone (`0x00702C8A`). Each Scatter spends the null arm's Scenario draw.
#[test]
fn retail_damage_scatter_tail_admits_by_rank_ability_and_ownership() {
    use crate::sim::components::NavTargetRef;
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    assert!(!rules.general.player_scatter);
    let run = |type_id: &str, human: bool, rank: u16, nav: bool| {
        let mut sim = crate::sim::world::Simulation::with_seed(0x5CA7);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, None, human, 0, 10));
        let id = sim
            .spawn_object(type_id, "Americans", 10, 10, 0, &rules)
            .unwrap();
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        assert_eq!(
            actor.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );
        actor.veterancy.set_rank_for_test(rank);
        if nav {
            actor
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::cell(14, 10)));
        }
        let rng = sim.scenario_rng.state();
        let warhead = sim.interner.intern("SA");
        crate::sim::combat::world_receiver::commit_entities(
            &mut sim,
            &mut crate::sim::combat::world_receiver::ReceiverRun::default(),
            &[EntityDamageEvent::area(
                id,
                10,
                0,
                RAD_NO_ATTACKER,
                None,
                warhead,
            )],
            None,
            &rules,
            None,
            &mut Vec::new(),
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        assert!(actor.health.current() < rules.object(type_id).unwrap().strength);
        (sim.scenario_rng.state() != rng, actor.navigation.nav_com())
    };
    let (drew, nav) = run("TANY", true, 100, false);
    assert!(drew && nav.is_some(), "veteran Tanya steps aside: {nav:?}");
    assert_eq!(run("TANY", true, 0, false), (false, None), "rookie Tanya");
    assert_eq!(
        run("TANY", true, 100, true),
        (false, Some(NavTargetRef::cell(14, 10))),
        "a NavCom refuses"
    );
    let (drew, nav) = run("E1", false, 0, false);
    assert!(
        drew && nav.is_some(),
        "a computer's GI steps aside: {nav:?}"
    );
}

/// One 10-point `SA` hit on stock rules through the production receiver. The
/// `actors` spawn on row 10 of a flat arena: the first is the victim and, when
/// `sourced`, the second is the source. `setup` adjusts the world before the
/// hit. Answers the world, the actors' ids and the Scenario RNG state just
/// before the hit.
fn retail_arena_hit(
    rules: &RuleSet,
    actors: &[(&str, &str, bool, u16)],
    sourced: bool,
    setup: impl FnOnce(&mut Simulation, &[u64]),
) -> (Simulation, Vec<u64>, u64) {
    let (sim, ids, rng, _) = retail_arena_hit_by(rules, actors, sourced, ("SA", 10), setup);
    (sim, ids, rng)
}

/// [`retail_arena_hit`] with a hit of `damage` by `warhead`, also answering
/// the hit's under-attack pings.
fn retail_arena_hit_by(
    rules: &RuleSet,
    actors: &[(&str, &str, bool, u16)],
    sourced: bool,
    (warhead, damage): (&str, i32),
    setup: impl FnOnce(&mut Simulation, &[u64]),
) -> (Simulation, Vec<u64>, u64, Vec<UnderAttackEvent>) {
    let mut sim = Simulation::with_seed(0x5CA7);
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let mut ids = Vec::new();
    for &(type_id, house, human, rx) in actors {
        let owner = sim.interner.intern(house);
        sim.houses
            .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, human, 0, 10));
        ids.push(sim.spawn_object(type_id, house, rx, 10, 0, rules).unwrap());
    }
    setup(&mut sim, &ids);
    let (source, source_owner) = if sourced {
        (
            ids[1],
            Some(sim.substrate.entities.get(ids[1]).unwrap().owner()),
        )
    } else {
        (RAD_NO_ATTACKER, None)
    };
    let rng = sim.scenario_rng.state();
    let warhead = sim.interner.intern(warhead);
    let mut pings = Vec::new();
    crate::sim::combat::world_receiver::commit_entities(
        &mut sim,
        &mut crate::sim::combat::world_receiver::ReceiverRun::default(),
        &[EntityDamageEvent::area(
            ids[0],
            damage,
            0,
            source,
            source_owner,
            warhead,
        )],
        None,
        rules,
        None,
        &mut pings,
    );
    (sim, ids, rng, pings)
}

/// A source the victim answers takes the scatter tail's accepted arm
/// (`0x00702B47`). In reach, `Override_Mission` (`0x00702B41`) first gives a
/// human's veteran Tanya her Target, which refuses the tail. Beyond weapon
/// range and Sight (`0x00702A58..0x00702B2F`) she has none, and her rank's
/// SCATTER admits the Scatter and its draw; a rookie's rank does not.
#[test]
fn retail_damage_scatter_tail_follows_the_retaliation_answer() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let run = |source_x: u16, rank: u16| {
        let (sim, ids, rng) = retail_arena_hit(
            &rules,
            &[
                ("TANY", "Americans", true, 10),
                ("E2", "Russians", false, source_x),
            ],
            true,
            |sim, ids| {
                sim.substrate
                    .entities
                    .get_mut(ids[0])
                    .unwrap()
                    .veterancy
                    .set_rank_for_test(rank)
            },
        );
        let tanya = sim.substrate.entities.get(ids[0]).unwrap();
        (
            tanya
                .attack_target
                .target()
                .is_some_and(|target| target == TargetKind::Entity(ids[1])),
            tanya.navigation.nav_com().is_some(),
            sim.scenario_rng.state() != rng,
        )
    };
    let (target, nav, _) = run(12, 100);
    assert!(target && !nav, "in reach she attacks: {target} {nav}");
    assert_eq!(
        run(30, 100),
        (false, true, true),
        "out of reach she steps aside"
    );
    assert_eq!(run(30, 0), (false, false, false), "a rookie stays");
}

/// A computer's Fraidycat civilian hit by a source scatters twice in one
/// receiver: the Techno tail's `Scatter(null, 1, 0)` (`0x00702D0B`; an
/// unarmed man refuses retaliation), then InfantryClass's
/// `Scatter(source, 0, 0)` (`0x00518C56`), whose `Queue_Mission(Move)` and
/// away-from-the-source destination replace the first. Each spends a Scenario
/// draw. Without a source only the first runs.
#[test]
fn retail_fraidycat_hit_by_a_source_scatters_twice() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    assert!(rules.object("CIV1").unwrap().fraidycat);
    let run = |sourced: bool| {
        let (sim, ids, rng) = retail_arena_hit(
            &rules,
            &[
                ("CIV1", "Neutral", false, 10),
                ("E2", "Russians", false, 14),
            ],
            sourced,
            |_, _| {},
        );
        let civilian = sim.substrate.entities.get(ids[0]).unwrap();
        (
            civilian.navigation.nav_com(),
            civilian.mission.queued().known(),
            sim.scenario_rng.state(),
            rng,
        )
    };
    let (tail_nav, tail_queued, tail_rng, rng) = run(false);
    assert!(tail_nav.is_some() && tail_rng != rng, "the tail scatters");
    assert_eq!(tail_queued, None, "the found-cell arm queues nothing");
    let (nav, queued, both_rng, _) = run(true);
    assert!(nav.is_some());
    assert_eq!(
        queued,
        Some(MissionType::Move),
        "the source Scatter queues Move"
    );
    assert_ne!(both_rng, tail_rng, "a second draw");
}

/// What the Foot and Infantry receivers add after the Techno tail, on
/// stock rules:
/// - a computer's `Engineer=` man in Guard queues Hunt
///   (`0x00518BF2..0x00518C3F`), with or without a source; a human's, or one
///   in Sleep, does not;
/// - a teamless Foot in `[Harmless]` (NoThreat, not Zombie) hit by a source
///   takes `Enter_Idle_Mode(0, 1)` (`0x004D749F..0x004D74C7`): the tail's
///   Scatter gave it a NavCom, so it queues Move. A team member, or a hit
///   without a source, keeps Harmless.
#[test]
fn retail_infantry_postlude_hunts_and_idles() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    assert!(rules.object("ENGINEER").unwrap().engineer);
    let harmless = rules.mission_control.entry(MissionType::Harmless).unwrap();
    assert!(harmless.no_threat && !harmless.zombie && !harmless.retaliate);
    let queued = |type_id: &str, human: bool, mission: MissionType, sourced: bool, team: bool| {
        let (sim, ids, _) = retail_arena_hit(
            &rules,
            &[
                (type_id, "Neutral", human, 10),
                ("E2", "Russians", false, 14),
            ],
            sourced,
            |sim, ids| {
                let victim = sim.substrate.entities.get_mut(ids[0]).unwrap();
                set_mission(victim, mission, MissionId::NONE);
                if team {
                    let owner = victim.owner();
                    let script = sim.interner.intern("Script");
                    sim.team_script_vm
                        .create_team(owner, script, vec![ids[0]], 0);
                }
            },
        );
        sim.substrate
            .entities
            .get(ids[0])
            .unwrap()
            .mission
            .queued()
            .known()
    };
    use MissionType::{Guard, Harmless, Hunt, Move, Sleep};
    assert_eq!(queued("ENGINEER", false, Guard, false, false), Some(Hunt));
    assert_eq!(queued("ENGINEER", false, Guard, true, false), Some(Hunt));
    assert_eq!(queued("ENGINEER", true, Guard, false, false), None);
    assert_eq!(queued("ENGINEER", false, Sleep, false, false), None);
    assert_eq!(queued("E1", false, Harmless, true, false), Some(Move));
    assert_eq!(queued("E1", false, Harmless, false, false), None);
    assert_eq!(queued("E1", false, Harmless, true, true), None);
}

/// A Chaos Drone's gas (`PsychGasCreate`, `Psychedelic=yes`) returns result 1
/// from TechnoClass's Psychedelic arm (`0x00701DBA`), and the Infantry
/// receiver's tail follows (`0x00518BB2..`). With a source a Fraidycat
/// scatters from it (`0x00518C56`; the unforced Scatter refuses anyone
/// else), whose Move replaces the berserk Hunt, and fear latches (100, or
/// 300 for a Fraidycat); without one it climbs the health ladder (12 at full
/// strength). Before them the Foot tail runs: a teamless man on a NoThreat
/// mission (`[Harmless]`) takes `Enter_Idle_Mode(0, 1)` (`0x004D749F`), whose
/// Guard replaces the Hunt. The Unit tail pings for a `Harvester=` vehicle
/// (`0x007384B9`); only the map-placed miners (HORV, CMON, SMON) lack
/// `ImmuneToPsionics=`.
#[test]
fn retail_gassed_infantry_scatters_and_fears() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    assert!(rules.object("CIV1").unwrap().fraidycat);
    let gassed = |type_id: &str, sourced: bool, mission: Option<MissionType>| {
        let (sim, ids, _, pings) = retail_arena_hit_by(
            &rules,
            &[
                (type_id, "Russians", false, 10),
                ("CAOS", "YuriCountry", false, 14),
            ],
            sourced,
            ("PsychGasCreate", 600),
            |sim, ids| {
                if let Some(mission) = mission {
                    let victim = sim.substrate.entities.get_mut(ids[0]).unwrap();
                    set_mission(victim, mission, MissionId::NONE);
                }
            },
        );
        let victim = sim.substrate.entities.get(ids[0]).unwrap();
        assert!(victim.berserk.is_active(), "{type_id} goes berserk");
        assert_eq!(
            victim.health.current(),
            rules.object(type_id).unwrap().strength,
            "gas does no damage"
        );
        assert!(pings.is_empty(), "miner warnings are submitted inline");
        (
            victim.infantry.as_ref().map(|man| man.fear_level()),
            victim.navigation.nav_com().is_some(),
            victim.mission.queued().known(),
            sim.sound_events().iter().any(|event| {
                matches!(
                    event,
                    crate::sim::world::SimSoundEvent::UnderAttack { miner: true, .. }
                )
            }),
        )
    };
    use MissionType::{Guard, Harmless, Hunt, Move};
    assert_eq!(
        gassed("E1", true, None),
        (Some(100), false, Some(Hunt), false)
    );
    assert_eq!(
        gassed("CIV1", true, None),
        (Some(300), true, Some(Move), false)
    );
    assert_eq!(
        gassed("E1", false, None),
        (Some(12), false, Some(Hunt), false)
    );
    assert_eq!(
        gassed("E1", true, Some(Harmless)),
        (Some(100), false, Some(Guard), false)
    );
    assert!(gassed("HORV", false, None).3, "a gassed miner pings");
}

/// The gas's target clear is the class setter (`vt+0x3C8(0)` at
/// `0x00701DA6`): Infantry's (`0x0051B1F0`) also drops a firing man's
/// action and his path head.
#[test]
fn retail_gassed_infantry_drops_its_fire_and_path() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let (sim, ids, _, _) = retail_arena_hit_by(
        &rules,
        &[
            ("E1", "Russians", false, 10),
            ("CAOS", "YuriCountry", false, 14),
        ],
        false,
        ("PsychGasCreate", 600),
        |sim, ids| {
            let man = sim.substrate.entities.get_mut(ids[0]).unwrap();
            man.attack_target
                .set_for_test(Some(TargetKind::Entity(ids[1])));
            man.mission_leaf.set_infantry_doing_verified(0).unwrap();
            man.mission_leaf.set_foot_firing_sequence(1);
            man.navigation
                .path_replay_mut()
                .set_directions_for_test(vec![0, 1, 2]);
            man.navigation.path_replay_mut().set_cursor_for_test(0);
        },
    );
    let man = sim.substrate.entities.get(ids[0]).unwrap();
    assert!(man.berserk.is_active());
    assert!(man.attack_target.target().is_none());
    assert_eq!(man.mission_leaf.foot_firing_sequence_latch(), 0);
    assert!(
        man.navigation
            .path_replay()
            .remaining_directions()
            .is_empty()
    );
}

/// Through the frame's object AI: a berserk computer GI's time runs down one
/// per frame (`0x006F9F0D`), and in the frame it ends the GI calms down and
/// queues Hunt.
#[test]
fn retail_berserk_gi_calms_down_in_the_frame_its_time_ends() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let mut sim = Simulation::with_seed(0x5CA7);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let owner = sim.interner.intern("Russians");
    register_computer_houses(&mut sim.houses, &[owner]);
    let gi = sim
        .spawn_object("E1", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let berserk = |active, timer| crate::sim::combat::BerserkState::for_test(active, timer);
    sim.substrate.entities.get_mut(gi).unwrap().berserk = berserk(true, 2);
    let frame = |sim: &mut Simulation| {
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
        let gi = sim.substrate.entities.get(gi).unwrap();
        (
            gi.berserk,
            gi.mission
                .queued()
                .known()
                .or_else(|| gi.mission.current().known()),
        )
    };
    assert_eq!(frame(&mut sim).0, berserk(true, 1));
    assert_eq!(
        frame(&mut sim),
        (berserk(false, 0), Some(MissionType::Hunt))
    );
}

/// A berserk GI takes its own house's man as it takes an enemy, on stock
/// rules: the ring walk's cell walk passes the first Techno, ally or not
/// (`0x006F8AAA`, `0x006F8BD0`), Hunt's Techno walk admits allies
/// (`0x006F9C92..`), and Evaluate_Candidate skips its ally test
/// (`0x006F7F0C`). Calm, it finds nothing.
#[test]
fn retail_berserk_gi_targets_its_own_house() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let mut sim = Simulation::with_seed(0x5CA7);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.interner.intern("Russians");
    register_computer_houses(&mut sim.houses, &[owner]);
    let gi = sim
        .spawn_object("E1", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let ally = sim
        .spawn_object("E1", "Russians", 11, 10, 0, &rules)
        .unwrap();
    let scan = |sim: &mut Simulation, mask| {
        sim.greatest_threat_represented(&rules, None, gi, mask, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    let masks = [ScanMission::Guard, ScanMission::Hunt];
    for mask in masks {
        assert_eq!(scan(&mut sim, mask), None, "calm, {mask:?}");
    }
    sim.substrate
        .entities
        .get_mut(gi)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    for mask in masks {
        assert_eq!(scan(&mut sim, mask), Some(ally), "berserk, {mask:?}");
    }
}

/// The scan's ally test is the scanner's house asking (`0x006F7F22`): a GI
/// takes a man whose house allies its own only one way, as a paranoid
/// computer's GIs take the Neutral house's, and spares one its own house
/// allies.
#[test]
fn retail_gi_scan_asks_only_its_own_house_about_allies() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let mut sim = Simulation::with_seed(0x5CA7);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for name in ["Russians", "Americans"] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, None, false, 0, 10));
    }
    let gi = sim
        .spawn_object("E1", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let other = sim
        .spawn_object("E1", "Americans", 11, 10, 0, &rules)
        .unwrap();
    let one_way = |sim: &mut Simulation, asker: &str, ally: &str| {
        let alliances = crate::map::houses::HouseAllianceMap::from([(
            asker.to_ascii_uppercase(),
            std::collections::BTreeSet::from([ally.to_ascii_uppercase()]),
        )]);
        sim.install_house_alliances(
            crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(alliances),
            &rules,
        );
    };
    let scan = |sim: &mut Simulation, mask| {
        sim.greatest_threat_represented(&rules, None, gi, mask, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    let masks = [ScanMission::Guard, ScanMission::Hunt];
    one_way(&mut sim, "Americans", "Russians");
    for mask in masks {
        assert_eq!(
            scan(&mut sim, mask),
            Some(other),
            "allied one way, {mask:?}"
        );
    }
    one_way(&mut sim, "Russians", "Americans");
    for mask in masks {
        assert_eq!(scan(&mut sim, mask), None, "its own ally, {mask:?}");
    }
}

/// Through the frame: a berserk computer GI on the Hunt the gas queued shoots
/// its own house's man. Hunt's scan takes him, the allied-target drop spares
/// a berserk owner (`0x006FA45D`) and GetFireError admits the shot
/// (`0x006FC1CC`).
#[test]
fn retail_berserk_gi_shoots_its_own_house() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = Simulation::with_seed(0x5CA7);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let owner = sim.interner.intern("Russians");
    register_computer_houses(&mut sim.houses, &[owner]);
    let gi = sim
        .spawn_object("E1", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let friend = sim
        .spawn_object("E1", "Russians", 12, 10, 0, &rules)
        .unwrap();
    let strength = rules.object("E1").unwrap().strength;
    sim.substrate
        .entities
        .get_mut(gi)
        .unwrap()
        .berserk
        .start(900);
    let _ = sim.mission_queue_exact(
        gi,
        MissionId::from_known(MissionType::Hunt),
        0,
        sim.session.binary_frame(),
        &crate::sim::mission::authority::LiveReadyInputProvider { rules: &rules },
    );
    let shot = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(friend)
            .is_none_or(|man| man.health.current() < strength)
    };
    for _ in 0..150 {
        if shot(&sim) {
            break;
        }
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
    }
    assert!(shot(&sim), "the berserk GI shoots his friend");
}

#[test]
fn gsi_04_05_building_self_damage_return_zero_stops_receiver_commit() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    let mut target = make_entity_owned(5, "SELFNO", 5, 4, 100, "Victim");
    target.set_category_for_test(EntityCategory::Structure);
    entities.insert(target);
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let wh = interner.intern("HITWH");
    let mut houses = computer_houses(&[victim_owner]);
    commit_hits(
        &[EntityDamageEvent::direct_receiver(
            5,
            10,
            0,
            5,
            Some(victim_owner),
            wh,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        )],
        &mut entities,
        &rules,
        &mut interner,
        &mut houses,
        88,
        None,
    );

    assert_eq!(entities.get(5).unwrap().health.current(), 100);
    assert_eq!(
        houses[&victim_owner]
            .strategy_emergency()
            .last_building_attack_frame(),
        0
    );
}

#[test]
fn gsi_04_05_building_attack_frame_remains_live_after_world_receiver_dispatch() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let source_id = sim
        .spawn_object("SOURCE", "Enemy", 4, 4, 0, &rules)
        .expect("source spawns");
    let target_id = sim
        .spawn_object("NORMAL", "Victim", 6, 4, 0, &rules)
        .expect("Building target spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner();
    let victim_owner = sim.substrate.entities.get(target_id).unwrap().owner();
    register_computer_houses(&mut sim.houses, &[source_owner, victim_owner]);
    sim.session.set_binary_frame_for_test(77);
    let warhead = sim.interner.intern("HITWH");

    let event = EntityDamageEvent::area(target_id, 10, 0, source_id, Some(source_owner), warhead);
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[event, event],
        &mut crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(
        sim.houses[&victim_owner]
            .strategy_emergency()
            .last_building_attack_frame(),
        77,
        "receiver dispatch writes the world's authoritative House timestamp"
    );
    assert_eq!(
        sim.houses[&victim_owner]
            .strategy_emergency()
            .last_attacker_house_index(),
        0,
        "receiver dispatch retains the attacker-House index with the frame stamp"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(target_id)
            .unwrap()
            .health
            .current(),
        80,
        "repeated qualifying receiver records in one frame remain admitted"
    );
}

#[test]
fn gsi_04_07_damage_hostile_building_hit_latches_was_attacked_for_ai_repair() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
         FixtureOnly=1\n\
         [AI]\nCreditReserve=100\n\
         [IQ]\nMaxIQLevels=5\nRepairSell=2\nSellBack=2\n\
         [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n\
         [Warheads]\n0=HITWH\n\
         [MTNK]\nStrength=300\nArmor=heavy\n\
         [GAPOWR]\nStrength=1000\nArmor=wood\nCost=800\nCrewed=no\n\
         [HITWH]\nCellSpread=0\nPercentAtMax=1\nAffectsAllies=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("hostile-hit rules");
    // A Buildup SHP, without which Sell_Back refuses the sale.
    rules.set_buildup_control_for_test("GAPOWR", [0, 25, 2]);
    let mut sim = crate::sim::world::Simulation::new();
    let ai_owner = sim.interner.intern("AI");
    let enemy_owner = sim.interner.intern("ENEMY");
    let ally_owner = sim.interner.intern("ALLY");
    let scenario_ini = IniFile::from_str("[Houses]\n0=AI\n[AI]\nIQ=1\n");
    let scenario_houses =
        crate::map::houses::parse_house_roster(&scenario_ini, &rules.color_schemes, Some(&rules));
    let mut ai_house = HouseState::new(ai_owner, 0, None, false, 0, 51);
    // HouseClass::Read_Scenario_INI stores `IQ=` as both the authored IQ
    // (+0x1D0) and CurrentIQ (+0x24C).
    ai_house.set_current_iq_for_test(
        scenario_houses.houses[0].scenario_current_iq(rules.general.max_iq_levels),
    );
    ai_house.set_authored_iq_for_test(ai_house.current_iq());
    sim.houses.register(ai_house);
    let hostile_target = sim
        .spawn_object("GAPOWR", "AI", 5, 5, 0, &rules)
        .expect("hostile target");
    let allied_target = sim
        .spawn_object("GAPOWR", "AI", 7, 5, 0, &rules)
        .expect("allied target");
    let null_target = sim
        .spawn_object("GAPOWR", "AI", 9, 5, 0, &rules)
        .expect("null-source target");
    let hostile_source = sim
        .spawn_object("MTNK", "ENEMY", 5, 6, 0, &rules)
        .expect("hostile source");
    let allied_source = sim
        .spawn_object("MTNK", "ALLY", 7, 6, 0, &rules)
        .expect("allied source");
    for target_id in [hostile_target, allied_target, null_target] {
        sim.substrate
            .entities
            .get_mut(target_id)
            .unwrap()
            .health
            .set_for_test(200);
    }
    let warhead_ref = sim.interner.intern("HITWH");
    let events = [
        EntityDamageEvent::area(
            hostile_target,
            10,
            0,
            hostile_source,
            Some(enemy_owner),
            warhead_ref,
        ),
        EntityDamageEvent::area(
            allied_target,
            10,
            0,
            allied_source,
            Some(ally_owner),
            warhead_ref,
        ),
        EntityDamageEvent::area(null_target, 10, 0, RAD_NO_ATTACKER, None, warhead_ref),
    ];
    let mut alliances = HouseAllianceMap::new();
    alliances
        .entry("AI".to_string())
        .or_default()
        .insert("ALLY".to_string());
    let mut main_rng = SimRng::new(3);
    let mut handled_deaths = Vec::new();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;
    let _ = commit_damage_events(
        &events,
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        &rules,
        &mut sim.interner,
        &mut sim.houses,
        &alliances,
        &mut main_rng,
        &mut sim.scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert!(
        sim.substrate
            .entities
            .get(hostile_target)
            .unwrap()
            .seldom
            .ai_sell_repair
            .was_attacked_by_enemy(),
        "surviving hostile source sets the persistent Techno tail byte"
    );
    assert!(
        !sim.substrate
            .entities
            .get(allied_target)
            .unwrap()
            .seldom
            .ai_sell_repair
            .was_attacked_by_enemy(),
        "target-owner alliance suppresses the hostile latch"
    );
    assert!(
        !sim.substrate
            .entities
            .get(null_target)
            .unwrap()
            .seldom
            .ai_sell_repair
            .was_attacked_by_enemy(),
        "null radiation/environment source cannot set it"
    );
    let latched_hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(hostile_target)
        .unwrap()
        .seldom
        .ai_sell_repair
        .set_was_attacked_by_enemy_for_test(false);
    assert_ne!(
        sim.state_hash(),
        latched_hash,
        "the persistent byte is hashed"
    );
    sim.substrate
        .entities
        .get_mut(hostile_target)
        .unwrap()
        .seldom
        .ai_sell_repair
        .set_was_attacked_by_enemy_for_test(true);

    let selling = |sim: &crate::sim::world::Simulation, id: u64| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .effective()
            .known()
            == Some(crate::sim::mission::MissionType::Selling)
    };
    // Each building's UpdateRepairAndPower, in its LogicVector visit.
    let repair_and_power = |sim: &mut crate::sim::world::Simulation| {
        for id in [hostile_target, allied_target, null_target] {
            crate::sim::production::update_repair_and_power(
                sim,
                &rules,
                id,
                None,
                &mut crate::sim::world::FrameEffects::default(),
            );
        }
    };
    let low_iq_rng = sim.scenario_rng.logical_state();
    repair_and_power(&mut sim);
    assert!(
        !selling(&sim, hostile_target),
        "scenario CurrentIQ 1 stays below RepairSell/SellBack 2"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        low_iq_rng,
        "an IQ-gated-out building draws no low-credit sale RNG"
    );

    let house = sim.houses.get_mut(&ai_owner).unwrap();
    house.set_current_iq_for_test(2);
    house.set_authored_iq_for_test(2);
    let mut expected_rng = sim.scenario_rng.clone();
    assert!(
        expected_rng.next_range_u32_inclusive(0, 0x32) < 51,
        "TechLevel 51 makes every inclusive native roll win"
    );
    repair_and_power(&mut sim);
    assert!(
        selling(&sim, hostile_target),
        "the computer's Sell_Back(1) starts the Selling mission"
    );
    assert!(!selling(&sim, allied_target));
    assert!(!selling(&sim, null_target));
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state(),
        "the single qualifying building consumes exactly one inclusive roll"
    );
}
