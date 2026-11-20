//! Phobos `RemoveMindControl` through the production detonation path
//! (`Ext/WarheadType/Detonate.cpp:207-208, 477-486`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::SimSoundEvent;
use crate::sim::world::Simulation;

/// `[AudioVisual] MindClearedSound` so the `play_sound` latch has something to
/// emit, and a mind-control range so the capture is not refused outright.
fn rules_with(wh_ini: &str, global_ini: &str) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n1=CAPTURE\n\
         [WH]\nCellSpread=0\n{wh_ini}\
         [CAPTURE]\nCellSpread=0\nChangeOwner=yes\nChangeOwner.SetAsMindControl=yes\n\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n\
         [AudioVisual]\nMindClearedSound=CLEARME\n{global_ini}"
    ));
    // `MindClearedSound=` resolves through the sound registry, as production
    // binds it.
    RuleSet::from_ini_with_sounds_for_test(&ini, &["CLEARME"])
        .expect("remove-mind-control test ruleset")
}

fn scene(wh_ini: &str, global_ini: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = rules_with(wh_ini, global_ini);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 22, 20, 0, &rules)
        .expect("target");
    (sim, rules, firer, target)
}

/// One `WarheadTypeExt::DetonateOnOneUnit` pass of `warhead` at `target`.
fn detonate_once(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64, warhead: &str) {
    let warhead_type = rules.warhead(warhead).expect("warhead");
    let warhead_ref = sim.interner.intern(warhead);
    let source_house = sim.entities().get(firer).map(|entity| entity.owner());
    let impact = {
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            sim.entities().get(target).expect("target"),
            sim.resolved_terrain.as_ref(),
        );
        crate::sim::projectile::ProjectileCoord::new(coords.x, coords.y, coords.z)
    };
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    let mut effects = crate::sim::combat::effects::DeathEffects::default();
    super::detonate(
        sim,
        &mut run,
        rules,
        None,
        warhead_type,
        warhead_ref,
        firer,
        source_house,
        impact,
        Some(target),
        0,
        false,
        &mut effects,
    );
    run.finish();
}

/// The `ChangeOwner.SetAsMindControl` capture every test starts from: the
/// firing House owns the target and the link is live.
fn captured(
    sim: &mut Simulation,
    rules: &RuleSet,
    firer: u64,
    target: u64,
) -> (
    crate::sim::intern::InternedId,
    crate::sim::intern::InternedId,
) {
    let _ = rules;
    let americans = sim.interner.get("Americans").expect("house");
    let russians = sim.interner.get("Russians").expect("house");
    // The production capture's retained state: the controller's manager holds
    // the victim with its original owner and the victim keeps the controller
    // link. Phobos' `ApplyRemoveMindControl` frees through `MindControlledBy`,
    // which only a CaptureManager sets (`ChangeOwner.SetAsMindControl` writes
    // just the permanent byte).
    {
        let firer_entity = sim.entities_mut_for_test().get_mut(firer).expect("firer");
        firer_entity.capture_manager = Some(Box::new(
            crate::sim::capture_manager::CaptureManagerState::with_nodes_for_test(&[(
                target, russians,
            )]),
        ));
    }
    {
        let entity = sim.entities_mut_for_test().get_mut(target).expect("target");
        entity.set_owner_for_test(americans);
        entity.mind_control =
            crate::sim::capture_manager::MindControlLink::controlled_by_for_test(firer);
    }
    (americans, russians)
}

/// `ApplyRemoveMindControl` frees the victim before every other per-target
/// arm (`Detonate.cpp:207-208, 477-486`), restoring the owner
/// `FreeUnit` had recorded and clearing the controller link.
#[test]
fn remove_mind_control_frees_the_victim() {
    let (mut sim, rules, firer, target) = scene("RemoveMindControl=yes\n", "");
    let (americans, russians) = captured(&mut sim, &rules, firer, target);
    detonate_once(&mut sim, &rules, firer, target, "WH");

    let entity = sim.entities().get(target).expect("target");
    assert_eq!(
        entity.owner(),
        russians,
        "the freed unit goes back to its original House"
    );
    assert!(
        !entity.mind_control.is_mind_controlled(),
        "the mind-control link is gone"
    );
    assert_ne!(entity.owner(), americans);
}

/// Without the key the link survives the detonation entirely.
#[test]
fn an_unset_key_keeps_the_victim_controlled() {
    let (mut sim, rules, firer, target) = scene("", "");
    let (americans, _russians) = captured(&mut sim, &rules, firer, target);
    detonate_once(&mut sim, &rules, firer, target, "WH");

    let entity = sim.entities().get(target).expect("target");
    assert!(entity.mind_control.is_mind_controlled(), "still controlled");
    assert_eq!(entity.owner(), americans);
}

/// The ordinary free rings `MindClearedSound` — the type's own, else
/// `[AudioVisual]`'s — as `CaptureManagerExt::FreeUnit` does.
#[test]
fn the_free_rings_the_mind_cleared_sound() {
    let (mut sim, rules, firer, target) = scene("RemoveMindControl=yes\n", "");
    captured(&mut sim, &rules, firer, target);
    sim.sound_events.clear_for_test();
    detonate_once(&mut sim, &rules, firer, target, "WH");

    assert!(
        sim.sound_events.iter().any(
            |event| matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "CLEARME")
        ),
        "the plain free rings the cue"
    );
}

/// `RemoveMindControl.Silent` passes `play_sound = false` to
/// `CaptureManagerExt::FreeUnit`, the only thing the flag changes there: the
/// victim is freed exactly as before, minus the feedback.
#[test]
fn the_silent_variant_frees_without_the_feedback() {
    let (mut sim, rules, firer, target) =
        scene("RemoveMindControl=yes\nRemoveMindControl.Silent=yes\n", "");
    captured(&mut sim, &rules, firer, target);
    sim.sound_events.clear_for_test();
    detonate_once(&mut sim, &rules, firer, target, "WH");

    let entity = sim.entities().get(target).expect("target");
    assert!(
        !entity.mind_control.is_mind_controlled(),
        "the silent variant still frees"
    );
    assert_eq!(entity.owner(), sim.interner.get("Russians").expect("house"));
    assert!(
        !sim.sound_events.iter().any(|event| matches!(
            event,
            SimSoundEvent::VocAt { sound_id, .. } if sound_id == "CLEARME"
        )),
        "no MindClearedSound while silent"
    );
}

/// `[AudioVisual] RemoveMindControl.Silent` is the fallback
/// `mind_control_silent(global)` consults when the warhead leaves its own
/// `.Silent` unset (`Ext/Rules/Body.cpp:597`).
#[test]
fn the_global_default_applies_when_the_warhead_is_unset() {
    let (mut sim, rules, firer, target) =
        scene("RemoveMindControl=yes\n", "RemoveMindControl.Silent=yes\n");
    captured(&mut sim, &rules, firer, target);
    sim.sound_events.clear_for_test();
    detonate_once(&mut sim, &rules, firer, target, "WH");

    assert!(
        !sim.entities()
            .get(target)
            .expect("target")
            .mind_control
            .is_mind_controlled(),
        "the arm still runs"
    );
    assert!(
        !sim.sound_events.iter().any(|event| matches!(
            event,
            SimSoundEvent::VocAt { sound_id, .. } if sound_id == "CLEARME"
        )),
        "the global silence applies when the warhead is unset"
    );
}

/// The arm is skipped for a target nothing controls: `controller()` is `None`
/// and no free is attempted.
#[test]
fn an_uncontrolled_target_triggers_no_free() {
    let (mut sim, rules, firer, target) = scene("RemoveMindControl=yes\n", "");
    sim.sound_events.clear_for_test();
    detonate_once(&mut sim, &rules, firer, target, "WH");
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::VocAt { .. })),
        "nothing is freed, so nothing rings"
    );
}
