//! Synthetic Ares regression scenes through the existing command/combat owners.
//! Expected outcomes are behavior checks, not captured native-execution goldens.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::superweapon_type::{GenericWarheadAiTargeting, SuperWeaponKind};
use crate::sim::command::Command;
use crate::sim::superweapon::SuperWeaponInstance;

const RULES: &str = "\
[SuperWeaponTypes]
0=Strike
[Strike]
Type=GenericWarhead
SW.Warhead=Payload
SW.Damage=75
RechargeTime=1
IsPowered=no
[BuildingTypes]
0=LAUNCHER
1=TARGET
[LAUNCHER]
Strength=200
Armor=wood
SuperWeapon=Strike
[TARGET]
Strength=200
Armor=wood
[Payload]
CellSpread=1.5
PercentAtMax=1
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";
const ART: &str = "[LAUNCHER]\nFoundation=1x1\n[TARGET]\nFoundation=1x1\n";

fn rules(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(ART)).unwrap()
}

fn fixture(extra: &str, owner_name: &str) -> (Simulation, RuleSet, InternedId, InternedId, u64) {
    let rules = rules(extra);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern(owner_name);
    let sw_type = sim.interner.intern("Strike");
    sim.spawn_object_at_height("LAUNCHER", owner_name, 6, 6, 0, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object_at_height("TARGET", "Americans", 15, 15, 0, 0, &rules)
        .unwrap();
    let mut instance = SuperWeaponInstance::new(sw_type, owner, 0);
    instance.is_active = true;
    instance.is_ready = true;
    instance.charge_start_tick = sim.session.binary_frame() as i32;
    instance.charge_duration = 900;
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(sw_type, instance);
    (sim, rules, owner, sw_type, target)
}

fn fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    cell: (u16, u16),
) -> bool {
    let owner_name = sim.interner.resolve(owner).to_owned();
    sim.apply_command_with_overlays(
        &owner_name,
        &Command::LaunchSuperWeapon {
            sw_type_id: sw_type,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        rules,
        None,
        FrameEffects::default(),
    )
}

#[test]
fn ares_generic_warhead_binds_unlisted_payload_and_preserves_layered_references() {
    // No [Warheads] list or WeaponType references Payload: the SW must allocate it.
    let base = IniFile::from_str(RULES);
    let mut layers = RulesLayerStack::new(base);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Strike]\nSW.Damage=40\n"),
    );
    let parsed = RuleSet::from_rules_layers(&layers).unwrap();
    let sw = parsed.super_weapon("Strike").unwrap();
    let settings = sw.generic_warhead.as_ref().unwrap();
    assert_eq!(sw.kind, SuperWeaponKind::GenericWarhead);
    assert_eq!(sw.action.as_deref(), Some("Attack"));
    assert_eq!(settings.warhead.as_deref(), Some("Payload"));
    assert_eq!(settings.damage, 40);
    assert_eq!(settings.ai_targeting, GenericWarheadAiTargeting::Offensive);
    assert!(parsed.warhead("Payload").is_some());

    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Strike]\nSW.Warhead=none\n"),
    );
    let cleared = RuleSet::from_rules_layers(&layers).unwrap();
    assert!(
        cleared
            .super_weapon("Strike")
            .unwrap()
            .generic_warhead
            .as_ref()
            .unwrap()
            .warhead
            .is_none()
    );
    assert_ne!(
        parsed.simulation_config_hash(),
        cleared.simulation_config_hash()
    );
}

#[test]
fn ares_generic_warhead_command_applies_area_damage_and_consumes_charge_once() {
    let (mut sim, rules, owner, sw_type, target) = fixture("", "Russians");
    let far = sim
        .spawn_object_at_height("TARGET", "Americans", 25, 25, 0, 0, &rules)
        .unwrap();
    assert!(fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 125);
    assert_eq!(sim.entities().get(far).unwrap().health.current(), 200);
    let instance = &sim.super_weapons[&owner][&sw_type];
    assert!(!instance.is_ready);
    assert_eq!(instance.charge_duration, 900);
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::SuperWeaponLaunched { owner: house, sw_type: kind, world }
        if *house == owner && *kind == sw_type
            && world.to_cell_sub_z().0 == 15 && world.to_cell_sub_z().1 == 15)));
    assert!(!fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 125);
}

#[test]
fn ares_generic_warhead_source_obeys_damage_self() {
    for damage_self in [false, true] {
        let extra = format!(
            "[LAUNCHER]\nDamageSelf={}\n",
            if damage_self { "yes" } else { "no" }
        );
        let (mut sim, rules, owner, sw_type, _) = fixture(&extra, "Russians");
        let source = sim.houses[&owner].base_projection().buildings()[0];
        assert!(fire(&mut sim, &rules, owner, sw_type, (6, 6)));
        assert_eq!(
            sim.entities().get(source).unwrap().health.current(),
            if damage_self { 125 } else { 200 }
        );
    }
}

#[test]
fn generic_warhead_source_destruction_keeps_single_charge_cleanup() {
    for one_time in [false, true] {
        let (mut sim, rules, owner, sw_type, _) = fixture(
            "[Strike]\nSW.Damage=250\n[LAUNCHER]\nDamageSelf=yes\n",
            "Russians",
        );
        let source = sim.houses[&owner].base_projection().buildings()[0];
        sim.super_weapons
            .map_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .get_mut(&sw_type)
            .unwrap()
            .one_time = one_time;
        assert!(fire(&mut sim, &rules, owner, sw_type, (6, 6)));
        assert!(
            sim.entities()
                .get(source)
                .is_none_or(|entity| !entity.is_object_alive())
        );
        let instance = &sim.super_weapons[&owner][&sw_type];
        assert!(!instance.is_ready);
        if one_time {
            assert!(!instance.is_active);
        }
        assert!(!fire(&mut sim, &rules, owner, sw_type, (6, 6)));
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event,
                    SimSoundEvent::SuperWeaponLaunched { sw_type: kind, .. } if *kind == sw_type
                ))
                .count(),
            1
        );
    }
}

#[test]
fn ares_generic_warhead_ai_offensive_and_none_use_normal_firing() {
    for mode in ["Offensive", "None"] {
        let (mut sim, rules, owner, sw_type, target) =
            fixture(&format!("[Strike]\nSW.AITargeting={mode}\n"), "Russians");
        let enemy = sim.interner.intern("Americans");
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_enemy_house_for_test(Some(enemy));
        crate::sim::superweapon::ai_fire::try_fire(
            &mut sim,
            &rules,
            owner,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.entities().get(target).unwrap().health.current(),
            if mode == "None" { 200 } else { 125 }
        );
        assert_eq!(sim.super_weapons[&owner][&sw_type].is_ready, mode == "None");
    }
}

// Migrated from SuperweaponTargetPreflightTests.InvalidTargetPreflight_...
// and RangeGate_UsesLiveSpecificLauncherWithInclusiveBoundaries in ra2-mono.
#[test]
fn mono_invalid_preflight_preserves_charge_health_and_observable_effects() {
    for extra in [
        "[Strike]\nSW.RequiresTarget=water\n",
        "[Strike]\nSW.RequiresHouse=owner\n",
        "[Strike]\nSW.RangeMaximum=2\n",
        "[Strike]\nSW.RangeMinimum=9\nSW.RangeMaximum=2\n",
        "[Strike]\nSW.RequiresTarget=future-content-mask\n",
    ] {
        let (mut sim, rules, owner, sw_type, target) = fixture(extra, "Russians");
        let before = sim.state_hash();
        let events = sim.sound_events.len();
        assert!(
            sim.super_weapon_target_check(&rules, owner, sw_type, (15, 15), false)
                .is_err(),
            "{extra}"
        );
        assert_eq!(sim.state_hash(), before, "preview is side-effect free");
        assert!(!fire(&mut sim, &rules, owner, sw_type, (15, 15)), "{extra}");
        assert_eq!(sim.state_hash(), before, "rejected command is atomic");
        assert_eq!(sim.sound_events.len(), events);
        assert_eq!(sim.entities().get(target).unwrap().health.current(), 200);
        assert!(sim.super_weapons[&owner][&sw_type].is_ready);
    }
}

#[test]
fn generic_warhead_range_requires_the_existing_provider_prerequisites() {
    use crate::rules::superweapon_targeting::TargetReject;

    let (mut sim, rules, owner, sw_type, _) = fixture(
        "[Strike]\nAuxBuilding=TARGET\nSW.RangeMaximum=1\n",
        "Russians",
    );
    // A trigger may grant a one-time Super independently of building grants.
    sim.super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw_type)
        .unwrap()
        .one_time = true;
    assert_eq!(
        sim.super_weapon_target_check(&rules, owner, sw_type, (6, 6), false),
        Err(TargetReject::NoEligibleLaunchSite),
    );
    assert!(!fire(&mut sim, &rules, owner, sw_type, (6, 6)));
    assert!(sim.super_weapons[&owner][&sw_type].is_ready);

    sim.spawn_object_at_height("TARGET", "Russians", 8, 8, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.super_weapon_target_check(&rules, owner, sw_type, (6, 6), false),
        Ok(())
    );
    assert!(fire(&mut sim, &rules, owner, sw_type, (6, 6)));
    assert!(!sim.super_weapons[&owner][&sw_type].is_ready);
}

#[test]
fn mono_range_gate_uses_second_live_launcher_at_inclusive_boundary() {
    let (mut sim, rules, owner, sw_type, target) = fixture(
        "[Strike]\nSW.RangeMinimum=5\nSW.RangeMaximum=5\n",
        "Russians",
    );
    assert!(!fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    sim.spawn_object_at_height("LAUNCHER", "Russians", 12, 11, 0, 0, &rules)
        .unwrap();
    assert!(fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 125);
}

#[test]
fn mono_player_and_ai_queries_keep_independent_requirements() {
    let (mut sim, rules, owner, sw_type, target) = fixture(
        "[Strike]\nSW.RequiresHouse=owner\nSW.AIRequiresHouse=enemies\nSW.AIRequiresTarget=buildings\n",
        "Russians",
    );
    assert!(!fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert!(
        sim.super_weapon_target_check(&rules, owner, sw_type, (15, 15), true)
            .is_ok()
    );
    let enemy = sim.interner.intern("Americans");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_enemy_house_for_test(Some(enemy));
    crate::sim::superweapon::ai_fire::try_fire(
        &mut sim,
        &rules,
        owner,
        None,
        FrameEffects::default(),
    );
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 125);
    assert!(!sim.super_weapons[&owner][&sw_type].is_ready);
}

#[test]
fn mono_fire_into_shroud_allows_explored_fog_but_not_never_explored_cell() {
    let (mut sim, rules, owner, sw_type, target) =
        fixture("[Strike]\nSW.FireIntoShroud=no\n", "Russians");
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    assert!(!fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert!(sim.super_weapons[&owner][&sw_type].is_ready);
    sim.fog.reveal_cells_for_owner(owner, [(15, 15)]);
    assert!(fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 125);
}

// Migrates GenericWarhead_SwAffectsHouseDoesNotOverrideWarheadHouseFilters;
// VERA has no AttachEffect receiver, so this scene observes ordinary HP damage.
#[test]
fn mono_generic_warhead_target_gate_does_not_override_payload_house_filter() {
    let (mut sim, rules, owner, sw_type, enemy) = fixture(
        "[Strike]\nSW.AffectsHouse=none\nSW.RequiresHouse=enemies\n[Payload]\nAffectsAllies=no\n",
        "Russians",
    );
    let own = sim
        .spawn_object_at_height("TARGET", "Russians", 16, 15, 0, 0, &rules)
        .unwrap();
    assert!(fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    assert_eq!(sim.entities().get(enemy).unwrap().health.current(), 125);
    assert_eq!(sim.entities().get(own).unwrap().health.current(), 200);
}

#[test]
fn ares_generic_warhead_one_time_consumption_survives_snapshot() {
    let (mut sim, rules, owner, sw_type, _) = fixture("", "Russians");
    sim.super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw_type)
        .unwrap()
        .one_time = true;
    assert!(fire(&mut sim, &rules, owner, sw_type, (15, 15)));
    let bytes = crate::sim::snapshot::GameSnapshot::save(
        &sim,
        0,
        rules.simulation_config_hash(),
        "generic",
        0,
    );
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert!(!restored.super_weapons[&owner][&sw_type].is_active);
    assert!(!restored.super_weapons[&owner][&sw_type].is_ready);
    assert!(!fire(&mut restored, &rules, owner, sw_type, (15, 15)));
}

/// Fourth native history pins count/frame writes and limit admission. This
/// scene joins the real command/area-damage owner to persistent launch history,
/// grant invalidation, and the unavailable building-Anim fallback.
#[test]
fn successful_ares_launch_history_limits_commands_survives_save_and_keeps_fallback_body() {
    let (mut sim, mut rules, owner, typ, target) = fixture("[Strike]\nSW.Shots=2\n", "Russians");
    let native = crate::test_fixture::json("tools/rules_oracle/ares_super_anims.json");
    let rows = native["availability_history"].as_array().unwrap();
    sim.session.set_binary_frame_for_test(9000);
    assert!(fire(&mut sim, &rules, owner, typ, (15, 15)));
    let first = &sim.super_weapons[&owner][&typ];
    assert_eq!(
        first.ares_launch_count as i64,
        rows[7]["counter"]["count"].as_i64().unwrap()
    );
    assert_eq!(
        first.ares_last_launch_frame.unwrap() as i64,
        rows[7]["counter"]["last_frame"].as_i64().unwrap()
    );
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 125);
    assert!(
        !fire(&mut sim, &rules, owner, typ, (15, 15)),
        "not ready does not count"
    );
    assert_eq!(sim.super_weapons[&owner][&typ].ares_launch_count, 1);
    let bytes = crate::sim::snapshot::GameSnapshot::save(
        &sim,
        0,
        rules.simulation_config_hash(),
        "ares-super-history",
        0,
    );
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        crate::util::save_codec::serialize(&restored.super_weapons).unwrap(),
        crate::util::save_codec::serialize(&sim.super_weapons).unwrap()
    );
    let hash = restored.state_hash();
    restored
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&typ)
        .unwrap()
        .ares_launch_count = 0;
    assert_ne!(restored.state_hash(), hash);
    restored
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&typ)
        .unwrap()
        .ares_launch_count = 1;
    restored.session.set_binary_frame_for_test(9001);
    restored
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&typ)
        .unwrap()
        .is_ready = true;
    restored.houses.get_mut(&owner).unwrap().take_tech_recheck();
    assert!(fire(&mut restored, &rules, owner, typ, (15, 15)));
    let second = &restored.super_weapons[&owner][&typ];
    assert_eq!(
        second.ares_launch_count as i64,
        rows[8]["counter"]["count"].as_i64().unwrap()
    );
    assert_eq!(
        second.ares_last_launch_frame.unwrap() as i64,
        rows[8]["counter"]["last_frame"].as_i64().unwrap()
    );
    assert!(restored.houses[&owner].tech_recheck_pending());
    assert!(!crate::sim::superweapon::super_weapon_available(
        &restored, &rules, owner, "Strike"
    ));
    let hp = restored.entities().get(target).unwrap().health.current();
    assert!(!fire(&mut restored, &rules, owner, typ, (15, 15)));
    assert_eq!(
        restored.entities().get(target).unwrap().health.current(),
        hp
    );
    // Original scene has no attached Art body. Bind a synthetic registered
    // slot14 to check unavailable-Super fallback through the real slot owner.
    let art = IniFile::from_str(
        "[LAUNCHER]\nFoundation=1x1\nSuperAnim=RESTING\n[TARGET]\nFoundation=1x1\n[RESTING]\nEnd=1\n",
    );
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(
        "[Strike]\nSW.Shots=2\n[Animations]\n0=RESTING\n",
    ));
    rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    // Fixed-ART parsing registers the type; real AnimClass allocation also
    // needs asset bounds. A one-frame fixture is enough to observe the slot.
    rules.bind_anim_frame_count_for_test("RESTING", 1);
    restored.intern_rule_type_ids(&rules);
    let building = restored.houses[&owner].base_projection().buildings()[0];
    restored.open_super_weapon_anims(building, false, false, &rules);
    let id = restored
        .entities()
        .get(building)
        .unwrap()
        .building_art
        .anim_slots()[14]
        .unwrap();
    assert_eq!(
        restored
            .interner
            .resolve(restored.anim(id).unwrap().type_id()),
        "RESTING"
    );
    restored.update_super_weapon_anims(building, &rules);
    assert_eq!(
        restored
            .entities()
            .get(building)
            .unwrap()
            .building_art
            .anim_slots()[14],
        Some(id)
    );
}
