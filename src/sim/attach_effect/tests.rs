use super::*;

fn effect(duration: i32) -> AttachEffectType {
    let mut effect = AttachEffectType::default();
    effect.duration = duration;
    effect
}

/// The initial delay gates everything, then the duration counts down and a
/// terminal (`Delay < 0`) effect expires at zero.
#[test]
fn initial_delay_gates_the_countdown_and_a_terminal_effect_expires() {
    let effect = effect(3);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            initial_delay: 2,
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
    );
    assert_eq!(
        instance.remaining(),
        3,
        "constructor takes the type duration"
    );
    instance.tick(&effect, false);
    instance.tick(&effect, false);
    assert_eq!(instance.remaining(), 3, "initial delay holds the duration");
    for _ in 0..3 {
        instance.tick(&effect, false);
    }
    assert_eq!(instance.remaining(), 0);
    assert!(instance.has_expired(), "Delay < 0 ends the effect");
    instance.tick(&effect, false);
    assert_eq!(instance.remaining(), 0, "a terminal effect stays ended");
}

/// `Delay = 0` refills the duration in the same frame, and a re-creating
/// effect never reports expired.
#[test]
fn a_zero_delay_recreates_in_the_same_frame() {
    let effect = effect(2);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: 0,
            ..AttachEffectParams::default()
        },
        &effect,
    );
    instance.tick(&effect, false);
    assert_eq!(instance.remaining(), 1);
    instance.tick(&effect, false);
    assert_eq!(instance.remaining(), 2, "zero delay refreshes on the spot");
    assert!(!instance.should_refresh(), "no deferred refresh was queued");
    assert!(
        !instance.has_expired(),
        "a re-creating effect never expires"
    );
}

/// Behind a discard check the same zero delay is deferred instead.
#[test]
fn a_zero_delay_under_discard_refreshes_on_a_later_frame() {
    let effect = effect(1);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: 0,
            ..AttachEffectParams::default()
        },
        &effect,
    );
    instance.tick(&effect, true);
    assert_eq!(instance.remaining(), 0);
    assert!(
        instance.should_refresh(),
        "queued, not refilled while discarded"
    );
    instance.tick(&effect, false);
    assert_eq!(instance.remaining(), 1);
}

/// A pending re-creation delay counts down and then refreshes; discarding
/// freezes it, as `ShouldBeDiscardedNow` gates the native branch.
#[test]
fn a_recreation_delay_counts_down_and_discarding_freezes_it() {
    let effect = effect(1);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: 2,
            ..AttachEffectParams::default()
        },
        &effect,
    );
    instance.tick(&effect, false);
    assert_eq!(
        instance.current_delay(),
        2,
        "duration 1 -> 0 arms the delay"
    );
    instance.tick(&effect, true);
    assert_eq!(instance.current_delay(), 2, "discarded delays do not tick");
    instance.tick(&effect, false);
    assert_eq!(instance.current_delay(), 1);
    instance.tick(&effect, false);
    assert_eq!(instance.current_delay(), 0);
    assert!(instance.should_refresh());
}

/// The override wins over the type duration, and a negative recreation
/// delay refuses `ResetIfRecreatable`.
#[test]
fn override_wins_and_a_negative_recreation_delay_refuses_the_reset() {
    let effect = effect(5);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            duration_override: 7,
            recreation_delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
    );
    assert_eq!(instance.remaining(), 7);
    assert!(!instance.reset_if_recreatable());
    assert_eq!(instance.remaining(), 7, "a refused reset changes nothing");
}

fn cumulative_effect(duration: i32, max_count: i32) -> AttachEffectType {
    let mut effect = effect(duration);
    effect.cumulative = true;
    effect.cumulative_max_count = max_count;
    effect
}

fn attach_with(
    list: &mut AttachEffectList,
    effect: &AttachEffectType,
    override_duration: i32,
) -> AttachOutcome {
    list.attach(
        0,
        AttachEffectParams {
            duration_override: override_duration,
            delay: -1,
            ..AttachEffectParams::default()
        },
        effect,
        AttachPolicy::default(),
        None,
    )
}

/// The first AI frame creates the effect's animation through the animation
/// owner, attached to the techno with its loops held; the frame that ends
/// the duration removes the effect and retires the animation.
#[test]
fn the_effect_animation_is_created_attached_and_retired() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=PULSEAE\n\
         AttachEffect.Delays=-1\n\
         [AttachEffectTypes]\n0=PULSEAE\n\
         [PULSEAE]\nDuration=3\nAnimation=PULSE\n",
    );
    let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\n");
    let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .expect("rules process");
    let mut rules =
        crate::rules::ruleset::RuleSet::from_processed_rules(&processed).expect("rules");
    let mut registry = ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("PULSE", 4);
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");

    let instances = sim
        .entities()
        .get(target)
        .unwrap()
        .phobos_attach_effects()
        .instances();
    assert_eq!(instances.len(), 1);
    assert!(
        instances[0].anim().is_none(),
        "the constructor only attaches; AI creates the animation"
    );

    sim.update_phobos_attach_effects(target, &rules);
    let anim = sim
        .entities()
        .get(target)
        .unwrap()
        .phobos_attach_effects()
        .instances()[0]
        .anim()
        .expect("the first AI frame creates the animation");
    assert_eq!(
        sim.anim(anim).unwrap().owner_entity(),
        Some(target),
        "SetOwnerObject binds the animation to its techno"
    );
    assert_eq!(
        sim.anim(anim).unwrap().runtime().loop_remaining,
        u8::MAX,
        "the effect holds its animation loops"
    );
    assert!(
        sim.interner
            .resolve(sim.anim(anim).unwrap().type_id())
            .eq_ignore_ascii_case("PULSE")
    );
    let position = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(target).unwrap().position,
    );
    assert_eq!(
        sim.anim_absolute_coord(anim),
        Some(crate::sim::anim_class::AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        }),
        "an attached animation follows its owner's coordinate"
    );

    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()[0]
            .remaining(),
        1
    );
    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .is_empty(),
        "the frame that ends the duration removes the effect"
    );
    assert!(
        sim.substrate.pending_delete.contains_object(&anim),
        "the removed effect retires its animation"
    );
}

/// `Animation.HideIfAttachedWith`: a listed type being attached keeps the
/// effect's animation away; when that type expires the next frame
/// recreates it.
#[test]
fn hide_if_attached_with_hides_and_restores_the_animation() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=SHOOTER,HIDER\n\
         AttachEffect.Delays=-1\n\
         [AttachEffectTypes]\n0=SHOOTER\n1=HIDER\n\
         [SHOOTER]\nDuration=100\nAnimation=PULSE\n\
         Animation.HideIfAttachedWith=HIDER\n\
         [HIDER]\nDuration=3\n",
    );
    let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\n");
    let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .expect("rules process");
    let mut rules =
        crate::rules::ruleset::RuleSet::from_processed_rules(&processed).expect("rules");
    let mut registry = ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("PULSE", 4);
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    let shooter_anim = |sim: &Simulation| {
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()
            .iter()
            .find(|instance| instance.type_index == 0)
            .and_then(|instance| instance.anim())
    };

    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        shooter_anim(&sim).is_none(),
        "HIDER is attached, so the animation stays hidden"
    );
    sim.update_phobos_attach_effects(target, &rules);
    assert!(shooter_anim(&sim).is_none());
    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        shooter_anim(&sim).is_some(),
        "the expired HIDER no longer hides the animation"
    );
}

/// Phobos' C++ int casts: truncation toward zero, `i32::MIN` for a NaN or
/// out-of-range product.
#[test]
fn scale_i32_matches_the_cpp_cast() {
    assert_eq!(scale_i32(7, 1.0), 7);
    assert_eq!(scale_i32(7, 1.5), 10);
    assert_eq!(scale_i32(7, -0.5), -3);
    assert_eq!(scale_i32(7, f64::INFINITY), i32::MIN);
    assert_eq!(scale_i32(7, f64::NAN), i32::MIN);
    assert_eq!(scale_i32(i32::MAX, 2.0), i32::MIN);
    assert_eq!(scale_i32(i32::MIN, 1.0), i32::MIN);
}

/// `ROFMultiplier.ApplyOnCurrentTimer`: the rearm timer already running
/// when the effect first initializes is scaled; the key can turn it off.
#[test]
fn rof_multiplier_scales_the_running_rearm_timer_on_initialization() {
    use crate::rules::ini_parser::IniFile;

    for (apply, expected) in [(false, 30), (true, 60)] {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=ROFAE\n\
             AttachEffect.InitialDelays=1\n\
             [AttachEffectTypes]\n0=ROFAE\n\
             [ROFAE]\nDuration=10\nROFMultiplier=2\n\
             ROFMultiplier.ApplyOnCurrentTimer={}\n",
            if apply { "yes" } else { "no" }
        )))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        let frame = sim.session().binary_frame() as i32;
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .seldom
            .rearm_timer
            .start(frame, 30);

        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .seldom
                .rearm_timer
                .remaining(frame),
            30,
            "the initial-delay frame never initializes the effect"
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .seldom
                .rearm_timer
                .remaining(frame),
            expected,
            "ApplyOnCurrentTimer={apply}"
        );
    }
}

/// Phobos `CalculateArmorMultipliers`'s restricted arm: one chance draw
/// per active restricted effect (even when it can never pass), the
/// warhead gates, and the picked HitAnim.
#[test]
fn restricted_armor_rolls_chance_gates_and_hit_animations() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;

    let scene = |roll: &str| {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=BASE,ROLL\n\
             [Warheads]\n0=HIT\n1=OTHER\n[HIT]\n[OTHER]\n\
             [AttachEffectTypes]\n0=BASE\n1=ROLL\n\
             [BASE]\nDuration=10\nArmorMultiplier=0.5\n\
             [ROLL]\nDuration=10\nArmorMultiplier=0.5\n{roll}"
        ));
        let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\n");
        let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
            .process_with_fixed_art(&art)
            .expect("rules process");
        let mut rules =
            crate::rules::ruleset::RuleSet::from_processed_rules(&processed).expect("rules");
        let mut registry = ArtRegistry::from_ini(&art);
        registry.bind_anim_frame_count_for_test("PULSE", 4);
        rules.install_art_data(registry);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        sim.update_phobos_attach_effects(target, &rules);
        (sim, rules, target)
    };

    // A passing roll multiplies the unrestricted base and spawns the pick.
    let (mut sim, rules, target) =
        scene("ArmorMultiplier.Chance=1.0\nArmorMultiplier.HitAnim=PULSE\n");
    let hit = sim.interner.intern("HIT");
    let owner = sim.entities().get(target).unwrap().owner();
    let before = sim.scenario_rng.native_state_hex();
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(hit), Some(owner), &rules),
        0.25,
        "base 0.5 times the applied 0.5"
    );
    assert_ne!(
        before,
        sim.scenario_rng.native_state_hex(),
        "the chance and the pick both drew"
    );
    assert!(
        sim.substrate.anims.iter().any(|(_, anim)| {
            anim.owner_entity() == Some(target)
                && sim
                    .interner()
                    .resolve(anim.type_id())
                    .eq_ignore_ascii_case("PULSE")
        }),
        "the picked HitAnim is attached to the target"
    );

    // Chance 0 still draws, then rejects.
    let (mut sim, rules, target) = scene("ArmorMultiplier.Chance=0.0\n");
    let hit = sim.interner.intern("HIT");
    let before = sim.scenario_rng.native_state_hex();
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
        0.5,
        "the base product survives the failed roll"
    );
    assert_ne!(
        before,
        sim.scenario_rng.native_state_hex(),
        "the chance draw happens even at zero"
    );

    // The warhead lists gate the same multiplier.
    let (mut sim, rules, target) = scene("ArmorMultiplier.DisallowWarheads=HIT\n");
    let hit = sim.interner.intern("HIT");
    let other = sim.interner.intern("OTHER");
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
        0.5,
        "a disallowed warhead skips the effect"
    );
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(other), None, &rules),
        0.25,
        "an unlisted warhead passes"
    );

    // `ArmorMultiplier.Delay`: a real hit starts the per-instance timer,
    // which skips later rolls until it runs out.
    let (mut sim, rules, target) = scene("ArmorMultiplier.Delay=5\n");
    let hit = sim.interner.intern("HIT");
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
        0.25,
        "the first hit applies the multiplier"
    );
    let before = sim.scenario_rng.native_state_hex();
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
        0.5,
        "the running delay skips the effect"
    );
    assert_eq!(
        before,
        sim.scenario_rng.native_state_hex(),
        "a skipped effect draws nothing"
    );
    sim.session.set_binary_frame_for_test(6);
    assert_eq!(
        sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
        0.25,
        "once the delay runs out the multiplier applies again"
    );
}

/// Phobos' Unkillable arm: a hit whose warhead total damage reaches the
/// target's health leaves Health and EstimatedHealth at 1; a nonlethal
/// packet or an object without the effect is untouched.
#[test]
fn unkillable_clamps_a_lethal_receive() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n1=PLAIN\n\
         [Warheads]\n0=HIT\n\
         [HIT]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TARGET]\nStrength=500\nSpeed=6\nArmor=light\n\
         AttachEffect.AttachTypes=AE_UK\n\
         [PLAIN]\nStrength=500\nSpeed=6\nArmor=light\n\
         [AttachEffectTypes]\n0=AE_UK\n\
         [AE_UK]\nDuration=-1\nUnkillable=yes\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    sim.update_phobos_attach_effects(target, &rules);
    let warhead = sim.interner.intern("HIT");
    let event = crate::sim::combat::EntityDamageEvent::area(
        target,
        100,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
    );
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .health
        .set(50);
    assert!(sim.apply_unkillable_clamp(target, &event, 100, &rules));
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 1);
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .health
        .set(500);
    assert!(
        !sim.apply_unkillable_clamp(target, &event, 100, &rules),
        "100 point of damage does not reach 500 health"
    );
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 500);

    let plain = sim
        .spawn_object("PLAIN", "Americans", 22, 20, 0, &rules)
        .expect("spawn");
    sim.substrate
        .entities
        .get_mut(plain)
        .unwrap()
        .health
        .set(50);
    let event = crate::sim::combat::EntityDamageEvent::area(
        plain,
        100,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
    );
    assert!(
        !sim.apply_unkillable_clamp(plain, &event, 100, &rules),
        "an object without the effect takes the lethal hit"
    );
}

/// `GetRangeWithModifiers`: multipliers apply in attach order, summed
/// `ExtraRange` cells are added once after every multiplier, and the
/// weapon lists gate both. The `-512` sentinel returns before the fold.
#[test]
fn attach_effect_range_multiplies_then_adds_extra_cells() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nRange=5\nDamage=10\nROF=20\n\
         [TARGET]\nStrength=1000\nSpeed=6\nPrimary=GUN\n\
         AttachEffect.AttachTypes=A,B\n\
         [AttachEffectTypes]\n0=A\n1=B\n\
         [A]\nDuration=10\nWeaponRange.Multiplier=1.5\n\
         [B]\nDuration=10\nWeaponRange.Multiplier=2\n\
         WeaponRange.ExtraRange=1\nWeaponRange.AllowWeapons=GUN\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    sim.update_phobos_attach_effects(target, &rules);
    let entity = sim.entities().get(target).unwrap();
    let obj = rules.object("TARGET").expect("target type");
    assert_eq!(
        crate::sim::combat::combat_weapon::weapon_range(
            entity,
            obj,
            0,
            sim.entities(),
            &rules,
            sim.interner(),
        ),
        5 * 256 * 3 + 256,
        "5 cells times 1.5 times 2, then one extra cell"
    );
    let fold = |weapon: &str, range: i32| {
        crate::sim::attach_effect::range_with_attach_effects(entity, weapon, range, &rules)
    };
    assert_eq!(
        fold("OTHER", 1000),
        1500,
        "B's allow-list skips it entirely"
    );
    assert_eq!(fold("gun", 1000), 3256, "the weapon match ignores case");
    assert_eq!(
        fold("GUN", -512),
        -512,
        "the always-in-range sentinel precedes the fold"
    );
    assert_eq!(
        fold("OTHER", i32::MAX),
        i32::MAX,
        "SafeMultiply clamps instead of overflowing"
    );
    assert_eq!(
        fold("GUN", -100),
        0,
        "-100 * 1.5 * 2 + 256 = -44 floors at zero"
    );
}

/// `OpenTopped.UseTransportRangeModifiers`: inside an `OpenTopped`
/// transport that enables the key, a rider folds the transport's
/// `WeaponRange` effects instead of its own.
#[test]
fn an_open_topped_transport_lends_its_range_modifiers_to_riders() {
    use crate::rules::ini_parser::IniFile;
    let scene = |enabled: bool| {
        let key = if enabled {
            "OpenTopped.UseTransportRangeModifiers=yes\n"
        } else {
            ""
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TRUCK\n1=RIDER\n\
             [WeaponTypes]\n0=GUN\n[GUN]\nRange=5\nDamage=10\nROF=20\n\
             [TRUCK]\nStrength=500\nSpeed=6\nOpenTopped=yes\nPassengers=1\n{key}\
             AttachEffect.AttachTypes=TRANSPORT_RANGE\n\
             [RIDER]\nStrength=100\nSpeed=4\nPrimary=GUN\n\
             AttachEffect.AttachTypes=RIDER_RANGE\n\
             [AttachEffectTypes]\n0=TRANSPORT_RANGE\n1=RIDER_RANGE\n\
             [TRANSPORT_RANGE]\nDuration=100\nWeaponRange.Multiplier=2\n\
             [RIDER_RANGE]\nDuration=100\nWeaponRange.Multiplier=3\n"
        )))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let truck = sim
            .spawn_object("TRUCK", "Americans", 20, 20, 0, &rules)
            .expect("truck");
        let rider = sim
            .spawn_object("RIDER", "Americans", 21, 20, 0, &rules)
            .expect("rider");
        sim.update_phobos_attach_effects(truck, &rules);
        sim.update_phobos_attach_effects(rider, &rules);
        let range = move |sim: &crate::sim::world::Simulation| {
            crate::sim::combat::combat_weapon::weapon_range(
                sim.entities().get(rider).unwrap(),
                rules.object("RIDER").expect("rider type"),
                0,
                sim.entities(),
                &rules,
                sim.interner(),
            )
        };
        (sim, truck, rider, range)
    };

    let (mut sim, truck, rider, range) = scene(true);
    assert_eq!(
        range(&sim),
        5 * 256 * 3,
        "outside a transport the rider folds its own effects"
    );
    sim.substrate
        .entities
        .get_mut(rider)
        .unwrap()
        .set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
            truck, true,
        ));
    assert_eq!(
        range(&sim),
        5 * 256 * 2,
        "the transport's fold applies while aboard"
    );

    let (mut sim, truck, rider, range) = scene(false);
    sim.substrate
        .entities
        .get_mut(rider)
        .unwrap()
        .set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
            truck, true,
        ));
    assert_eq!(
        range(&sim),
        5 * 256 * 3,
        "the default keeps the rider's own fold"
    );
}

/// `ShouldBeDiscardedNow`'s continuous conditions over injected facts.
#[test]
fn discard_conditions_follow_their_facts() {
    use crate::rules::attach_effect::{AttachEffectType, DiscardOn};
    let mut effect = AttachEffectType::default();
    effect.duration = 10;
    let mut list = AttachEffectList::default();
    list.attach(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        AttachPolicy::default(),
        None,
    );
    let instance = list.instances()[0].clone();
    let globals = crate::rules::attach_effect::AttachEffectGlobals::default();

    // Ammo `[min, max]`, inclusive, `-1` unbounded.
    effect.discard_on = DiscardOn::AMMO;
    effect.discard_ammo_minimum = 2;
    effect.discard_ammo_maximum = 4;
    let mut facts = DiscardFacts::default();
    for (ammo, expected) in [(1, false), (2, true), (4, true), (5, false)] {
        facts.ammo = ammo;
        assert_eq!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            expected,
            "ammo {ammo}"
        );
    }
    effect.discard_ammo_maximum = -1;
    facts.ammo = 9;
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));

    // Health `(above, below]`, above inclusive at zero health.
    effect.discard_on = DiscardOn::HEALTH;
    effect.discard_health_above = 0.2;
    effect.discard_health_below = 0.5;
    facts = DiscardFacts::default();
    for (health, strength, expected) in [
        (50, 100, true),
        (20, 100, false),
        (60, 100, false),
        (0, 0, true),
        (0, 100, false),
    ] {
        facts.health = health;
        facts.strength = strength;
        assert_eq!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            expected,
            "health {health}/{strength}"
        );
    }

    // Move / stationary / harvesting.
    effect.discard_on = DiscardOn::MOVE;
    facts = DiscardFacts {
        is_foot: true,
        is_really_moving: true,
        ..DiscardFacts::default()
    };
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.is_really_moving = false;
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_move_based_on_destination = Some(true);
    facts.is_moving = true;
    assert!(
        instance.should_be_discarded_now(&effect, &facts, &globals),
        "the destination query replaces the locomotor motion query"
    );
    facts.is_moving = false;

    effect.discard_on = DiscardOn::STATIONARY;
    effect.discard_consider_harvesting_as_stationary = Some(true);
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.harvesting = true;
    assert!(
        instance.should_be_discarded_now(&effect, &facts, &globals),
        "harvesting counts as stationary on this switch"
    );
    effect.discard_consider_harvesting_as_stationary = Some(false);
    effect.discard_on = DiscardOn::HARVESTING;
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.harvesting = false;
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_on = DiscardOn::STATIONARY;
    facts.harvesting = true;
    assert!(
        !instance.should_be_discarded_now(&effect, &facts, &globals),
        "harvesting is not stationary on this switch"
    );
    facts.harvesting = false;
    facts.mission_harvest_on_tiberium = true;
    assert!(
        !instance.should_be_discarded_now(&effect, &facts, &globals),
        "the intermediate harvest state is neither"
    );

    // Selling vs undeploying split on the archive target.
    effect.discard_on = DiscardOn::SELLING;
    facts = DiscardFacts {
        is_building: true,
        selling: true,
        ..DiscardFacts::default()
    };
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_on = DiscardOn::UNDEPLOYING;
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.undeploying = true;
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));

    // Land type names.
    effect.discard_on = DiscardOn::LAND_TYPE;
    effect.discard_land_types = vec!["Clear".to_owned()];
    facts = DiscardFacts {
        land_type: Some(crate::rules::terrain_rules::LandType::Clear),
        ..DiscardFacts::default()
    };
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.land_type = Some(crate::rules::terrain_rules::LandType::Road);
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));

    // Sequence: immediate matches the current action; deferred matches
    // the previous one once it changes.
    use crate::rules::animation_sequence::SequenceKind;
    effect.discard_on = DiscardOn::SEQUENCE;
    effect.discard_sequences = vec!["walk".to_owned()];
    effect.discard_sequences_immediate = Some(true);
    facts = DiscardFacts {
        sequence: Some(SequenceKind::Walk),
        ..DiscardFacts::default()
    };
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.sequence = Some(SequenceKind::Stand);
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_sequences_immediate = Some(false);
    let mut deferred = instance.clone();
    deferred.last_sequence_check = Some(SequenceKind::Walk);
    assert!(
        deferred.should_be_discarded_now(&effect, &facts, &globals),
        "the previous matching sequence discards on the change"
    );
    deferred.last_sequence_check = Some(SequenceKind::Stand);
    assert!(!deferred.should_be_discarded_now(&effect, &facts, &globals));

    // InRange/OutOfRange use the held target's distance and the selected
    // weapon's range; RangeOverride replaces the range in cells.
    effect.discard_on = DiscardOn::IN_RANGE;
    effect.discard_range_override = None;
    facts = DiscardFacts {
        target: Some((100, 200)),
        ..DiscardFacts::default()
    };
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.target = Some((300, 200));
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_on = DiscardOn::OUT_OF_RANGE;
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.target = Some((200, 200));
    assert!(
        instance.should_be_discarded_now(&effect, &facts, &globals),
        "the exact range boundary discards either way"
    );
    effect.discard_on = DiscardOn::IN_RANGE;
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_range_override = Some(0.5);
    facts.target = Some((100, 9_999));
    assert!(
        instance.should_be_discarded_now(&effect, &facts, &globals),
        "the override replaces the weapon range"
    );
    facts.target = Some((200, 9_999));
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.target = None;
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));

    // Mission list: a human owner reads DiscardOn.Missions, an AI reads
    // DiscardOn.AIMissions when it is set.
    use crate::rules::mission_data::MissionType;
    effect.discard_on = DiscardOn::MISSION;
    effect.discard_missions = vec![MissionType::Guard.ini_section().to_owned()];
    effect.discard_ai_missions = Some(vec![MissionType::Hunt.ini_section().to_owned()]);
    facts = DiscardFacts {
        mission: Some(MissionType::Guard),
        owner_human: true,
        ..DiscardFacts::default()
    };
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.mission = Some(MissionType::Hunt);
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.owner_human = false;
    assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
    facts.mission = Some(MissionType::Guard);
    assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
    effect.discard_ai_missions = None;
    assert!(
        instance.should_be_discarded_now(&effect, &facts, &globals),
        "an absent AI list inherits DiscardOn.Missions"
    );

    // The event flag wins even with no condition list.
    let mut flagged = instance.clone();
    flagged.mark_discarded();
    effect.discard_on = DiscardOn::NONE;
    assert!(flagged.should_be_discarded_now(&effect, &DiscardFacts::default(), &globals));
}

/// The event conditions flag through their owners (a fired count, a
/// received hit, limbo entry), and the AI walk removes the effect.
#[test]
fn event_discards_remove_after_their_counts() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=500\nSpeed=6\n\
         AttachEffect.AttachTypes=FIRE,RECEIVE,ENTRY\n\
         [AttachEffectTypes]\n0=FIRE\n1=RECEIVE\n2=ENTRY\n\
         [FIRE]\nDuration=10\nDiscardOn=firing\nDiscardOn.Firing.Count=2\n\
         [RECEIVE]\nDuration=10\nDiscardOn=receiveddamage\n\
         DiscardOn.ReceivedDamage.Count=1\n\
         [ENTRY]\nDuration=10\nDiscardOn=entry\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    let count = |sim: &crate::sim::world::Simulation| {
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()
            .len()
    };
    assert_eq!(count(&sim), 3);

    sim.record_phobos_attach_firing(target, &rules);
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(count(&sim), 3, "one shot is below Firing.Count=2");
    sim.record_phobos_attach_firing(target, &rules);
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(count(&sim), 2, "the second shot discards FIRE");

    let owner = sim.entities().get(target).unwrap().owner();
    sim.record_phobos_attach_damage(target, Some(owner), &rules);
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(count(&sim), 1, "one hit discards RECEIVE");

    sim.limbo_phobos_attach_effects(
        target,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(count(&sim), 0, "limbo entry discards ENTRY");
}

/// An owner change and a dead external invoker flag through their owners.
#[test]
fn owner_change_and_invoker_death_flag_the_effect() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=500\nSpeed=6\nAttachEffect.AttachTypes=OWNER\n\
         [AttachEffectTypes]\n0=OWNER\n1=DEADINV\n\
         [OWNER]\nDuration=10\nDiscardOn=ownerchange\n\
         [DEADINV]\nDuration=10\nDiscardOn=invokerdie\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    // A second effect attached from outside names an invoker that is gone.
    let catalog = rules.attach_effect_types();
    let effect = &catalog.types()[1];
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .phobos_attach_effects_mut()
        .attach(
            1,
            AttachEffectParams {
                delay: -1,
                recreation_delay: -1,
                ..AttachEffectParams::default()
            },
            effect,
            AttachPolicy::default(),
            Some(AttachSource {
                invoker: Some(999_999),
                source: None,
                invoker_house: None,
            }),
        );
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()
            .len(),
        2
    );
    sim.update_phobos_attach_effects(target, &rules);
    let instances = sim
        .entities()
        .get(target)
        .unwrap()
        .phobos_attach_effects()
        .instances();
    assert_eq!(instances.len(), 1, "the dead invoker discards DEADINV");
    assert_eq!(instances[0].type_index, 0);

    let russians = sim.interner().get("Russians").expect("rival house");
    sim.change_owner_with_rules(
        target,
        russians,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    );
    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .is_empty(),
        "the human-to-computer owner change discards OWNER"
    );
}

/// The sequence condition reads the infantry's current Doing; the
/// immediate list form matches at once and the deferred form matches the
/// previous sequence after the next one starts.
#[test]
fn sequence_discard_reads_the_infantry_doing() {
    use crate::rules::ini_parser::IniFile;
    let scene = |immediate: &str| {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n[InfantryTypes]\n0=E1\n\
             [E1]\nStrength=100\nSpeed=4\nAttachEffect.AttachTypes=SEQ\n\
             [AttachEffectTypes]\n0=SEQ\n\
             [SEQ]\nDuration=10\nDiscardOn=sequence\n\
             DiscardOn.Sequences=walk\nDiscardOn.Sequences.Immediate={immediate}\n"
        )))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("E1", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        (sim, rules, target)
    };
    let count = |sim: &crate::sim::world::Simulation, id: u64| {
        sim.entities()
            .get(id)
            .unwrap()
            .phobos_attach_effects()
            .instances()
            .len()
    };

    let (mut sim, rules, target) = scene("yes");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(3)
        .expect("Walk action");
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(count(&sim, target), 0, "the immediate form matches Walk");

    let (mut sim, rules, target) = scene("no");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(3)
        .expect("Walk action");
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(count(&sim, target), 1, "the deferred form waits");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(0)
        .expect("Ready action");
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(
        count(&sim, target),
        0,
        "leaving the matching sequence discards"
    );
}

/// `DiscardOn=outofrange` reads the live held target; a zero-cell override
/// makes any positive distance discard.
#[test]
fn out_of_range_discard_reads_the_held_target() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nRange=5\nDamage=10\nROF=20\n\
         [TARGET]\nStrength=500\nSpeed=6\nPrimary=GUN\n\
         AttachEffect.AttachTypes=OUT\n\
         [AttachEffectTypes]\n0=OUT\n\
         [OUT]\nDuration=10\nDiscardOn=outofrange\nDiscardOn.RangeOverride=0\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    assert!(
        sim.assign_target_represented(
            target,
            Some(crate::sim::combat::TargetKind::Cell(30, 30)),
            &rules,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .is_ok()
    );
    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .is_empty(),
        "a positive distance is outside the zero range"
    );
}

/// `DiscardOn=ammo` erases the effect on the AI frame that observes the
/// target's rounds inside the interval.
#[test]
fn an_ammo_discard_removes_the_effect() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=500\nSpeed=6\nAmmo=0\n\
         AttachEffect.AttachTypes=AE_AMMO\n\
         [AttachEffectTypes]\n0=AE_AMMO\n\
         [AE_AMMO]\nDuration=10\nDiscardOn=ammo\n\
         DiscardOn.Ammo.MinimumAmount=0\nDiscardOn.Ammo.MaximumAmount=0\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()
            .len(),
        1
    );
    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .is_empty(),
        "the empty magazine discards the effect"
    );
}

/// `DiscardOn=drain` reads the victim side of the Floating Disc link
/// (`TechnoClass+0x1D0 DrainingMe`).
#[test]
fn a_drain_link_discards_the_effect() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=500\nSpeed=6\n\
         AttachEffect.AttachTypes=AE_DRAIN\n\
         [AttachEffectTypes]\n0=AE_DRAIN\n\
         [AE_DRAIN]\nDuration=10\nDiscardOn=drain\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()
            .len(),
        1,
        "no drainer keeps the effect"
    );
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .drain
        .set_draining_me_for_test(Some(999));
    sim.update_phobos_attach_effects(target, &rules);
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .is_empty(),
        "the drained victim discards the effect"
    );
}

/// `RecalculateStatMultipliers` folds every active armor multiplier into
/// the cached factor, and the receiver's extra divide consumes it after
/// the first AI frame.
#[test]
fn active_effects_fold_into_the_receiver_armor_divider() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=ARMORA,ARMORB\n\
         [AttachEffectTypes]\n0=ARMORA\n1=ARMORB\n\
         [ARMORA]\nDuration=10\nArmorMultiplier=0.5\n\
         [ARMORB]\nDuration=10\nArmorMultiplier=0.5\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let target = sim
        .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
        .expect("spawn");
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_armor_multiplier(),
        1.0,
        "the fold starts at identity until the first AI frame"
    );
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_armor_multiplier(),
        0.25,
        "two active effects stack"
    );
    let divisors = crate::sim::combat::receiver_divisors(
        sim.entities().get(target).unwrap(),
        rules.object("TARGET"),
        &rules,
        sim.interner(),
        sim.houses(),
        sim.entities()
            .get(target)
            .unwrap()
            .phobos_armor_multiplier(),
    );
    assert_eq!(divisors.ae_armor, 0.25);
    assert_eq!(
        crate::sim::combat::damage::receive::defence_divides(100, &divisors),
        400,
        "the stock divide then the effect divide"
    );
}

/// `TechnoClass::EstimateDamage`'s Phobos divide (`0x006FDC87`) reads
/// `CalculateArmorMultipliers(pTarget, warhead, pThis->Owner)`, so the
/// target's effect armor applies, not the firer's.
#[test]
fn the_damage_estimate_divides_by_the_targets_effect_armor() {
    use crate::rules::ini_parser::IniFile;
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nDamage=100\nRange=5\nROF=20\nWarhead=WH\n\
         [WarheadTypes]\n0=WH\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [FIRER]\nStrength=500\nSpeed=6\nPrimary=GUN\n\
         AttachEffect.AttachTypes=FIRER_ARMOR\n\
         [TARGET]\nStrength=500\nSpeed=6\n\
         AttachEffect.AttachTypes=TARGET_ARMOR\n\
         [AttachEffectTypes]\n0=FIRER_ARMOR\n1=TARGET_ARMOR\n\
         [FIRER_ARMOR]\nDuration=100\nArmorMultiplier=4\n\
         [TARGET_ARMOR]\nDuration=100\nArmorMultiplier=2\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Americans", 22, 20, 0, &rules)
        .expect("target");
    sim.update_phobos_attach_effects(firer, &rules);
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(
        crate::sim::combat::estimated_damage_on(
            &sim,
            &rules,
            firer,
            target,
            rules.weapon("GUN").expect("weapon"),
        ),
        50,
        "100 damage divided by the target's ArmorMultiplier=2"
    );
}

/// `TechnoExt::ExtData::HasAttachedEffects(..., requireAll=false,
/// requireAnims=true)`: a required set matches on any active attached
/// type, and a type that declares an animation only counts while its
/// instance holds one.
#[test]
fn draw_offsets_match_any_active_required_type_and_need_their_animation() {
    use crate::rules::attach_effect::AnimationDrawOffset;

    let mut host = AttachEffectType::default();
    host.name = "HOST".to_owned();
    host.duration = 10;
    host.animation_draw_offsets = vec![
        AnimationDrawOffset {
            offset: [2, -1],
            required_types: Vec::new(),
        },
        AnimationDrawOffset {
            offset: [5, 5],
            required_types: vec!["NEEDED".to_owned(), "OTHER".to_owned()],
        },
    ];
    let mut needed = AttachEffectType::default();
    needed.name = "NEEDED".to_owned();
    needed.duration = 10;
    needed.animation = Some("LOOP".to_owned());
    let mut other = AttachEffectType::default();
    other.name = "OTHER".to_owned();
    other.duration = 10;
    let effects = vec![host, needed, other];
    let attach = |list: &mut AttachEffectList, type_index| {
        list.attach(
            type_index,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effects[type_index],
            AttachPolicy::default(),
            None,
        )
    };

    let mut list = AttachEffectList::default();
    attach(&mut list, 0);
    attach(&mut list, 1);
    list.tick_all(&effects, |_, _, _| false);
    assert_eq!(
        list.draw_offset(0, &effects),
        [2, -1],
        "the unconditional rule adds on its own; a type that declares an \
         animation does not match without one"
    );
    list.instances[1].anim = Some(9);
    assert_eq!(
        list.draw_offset(0, &effects),
        [7, 4],
        "the animated instance satisfies the rule"
    );
    // `RequiredTypes` is any-of, and a second match must not add twice.
    list.instances[1].anim = None;
    attach(&mut list, 2);
    list.tick_all(&effects, |_, _, _| false);
    assert_eq!(
        list.draw_offset(0, &effects),
        [7, 4],
        "OTHER declares no animation, so it matches on its own"
    );
    list.instances[1].anim = Some(9);
    assert_eq!(
        list.draw_offset(0, &effects),
        [7, 4],
        "a rule adds once however many listed types match"
    );

    // An ended terminal effect no longer satisfies a required type.
    let mut host = AttachEffectType::default();
    host.name = "HOST".to_owned();
    host.duration = 10;
    host.animation_draw_offsets = vec![AnimationDrawOffset {
        offset: [7, 4],
        required_types: vec!["OTHER".to_owned()],
    }];
    let mut short = AttachEffectType::default();
    short.name = "OTHER".to_owned();
    short.duration = 2;
    let short_effects = vec![host, short];
    let mut list = AttachEffectList::default();
    list.attach(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &short_effects[0],
        AttachPolicy::default(),
        None,
    );
    // Attached from outside: activity follows the remaining duration, so a
    // run-out effect leaves the required set even before removal.
    list.attach(
        1,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &short_effects[1],
        AttachPolicy::default(),
        Some(AttachSource::default()),
    );
    list.tick_all(&short_effects, |_, _, _| false);
    assert_eq!(list.draw_offset(0, &short_effects), [7, 4]);
    list.tick_all(&short_effects, |_, _, _| false);
    assert_eq!(
        list.draw_offset(0, &short_effects),
        [0, 0],
        "an expired effect is not active"
    );
}

/// A creation attach reads the type's list with each paired value and
/// keeps an unknown catalog name out.
#[test]
fn creation_attaches_the_type_list_with_paired_values() {
    use crate::rules::ruleset::RuleSet;
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[AttachEffectTypes]
0=FIRST
[FIRST]
Duration=40
             [VehicleTypes]
0=MTNK
[MTNK]
Name=Apocalypse
             AttachEffect.AttachTypes=FIRST,MISSING
AttachEffect.InitialDelays=3
",
    ))
    .expect("rules");
    let object = rules.object("MTNK").expect("techno");
    let mut list = AttachEffectList::default();
    assert_eq!(
        attach_initial_effects(
            &mut list,
            crate::rules::object_type::ObjectCategory::Vehicle,
            None,
            object,
            &rules,
            0,
            None,
        ),
        1,
        "only the catalog name attaches"
    );
    assert_eq!(list.instances().len(), 1);
    assert_eq!(list.instances()[0].remaining(), 40);
}

/// `CreateAndAttach` (`AttachEffectClass.cpp:883..898`) refuses an active
/// curtain without the matching penetration flag, then applies the class
/// whitelist and the type lists. A Force Shield reads its own flag, which
/// inherits the Iron Curtain one when absent.
#[test]
fn attach_gate_follows_curtain_class_and_type_filters() {
    use crate::rules::attach_effect::AttachEffectTargets;
    use crate::rules::object_type::ObjectCategory;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::sim::timer::CdTimer;

    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nName=Apocalypse\n",
    ))
    .expect("rules");
    let object = rules.object("MTNK").expect("techno");
    let mut effect = effect(40);
    assert!(can_attach_effect(
        ObjectCategory::Vehicle,
        None,
        object,
        &effect,
        0
    ));

    effect.affects_target = AttachEffectTargets {
        infantry: false,
        units: false,
        aircraft: false,
        buildings: true,
    };
    assert!(
        !can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
        "a building-only effect rejects a vehicle"
    );
    effect.affects_target = AttachEffectTargets::ALL;

    effect.affect_types = vec!["OTHER".to_owned()];
    assert!(
        !can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
        "the whitelist rejects an unlisted type"
    );
    effect.affect_types = vec!["mtnk".to_owned()];
    assert!(
        can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
        "the whitelist matches its id case-insensitively"
    );
    effect.affect_types.clear();
    effect.ignore_types = vec!["MTNK".to_owned()];
    assert!(
        !can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
        "the blacklist wins after the whitelist"
    );
    effect.ignore_types.clear();

    let curtain = InvulnerabilityState::new(CdTimer::started(0, 100), InvulnKind::IronCurtain);
    assert!(
        !can_attach_effect(ObjectCategory::Vehicle, Some(&curtain), object, &effect, 50),
        "an active curtain blocks the attach"
    );
    effect.penetrates_iron_curtain = true;
    assert!(
        can_attach_effect(ObjectCategory::Vehicle, Some(&curtain), object, &effect, 50),
        "PenetratesIronCurtain admits the curtained target"
    );
    assert!(
        can_attach_effect(
            ObjectCategory::Vehicle,
            Some(&curtain),
            object,
            &effect,
            100
        ),
        "an expired curtain no longer blocks"
    );

    let shield = InvulnerabilityState::new(CdTimer::started(0, 100), InvulnKind::ForceShield);
    effect.penetrates_force_shield = Some(false);
    assert!(
        !can_attach_effect(ObjectCategory::Vehicle, Some(&shield), object, &effect, 50),
        "a Force Shield reads PenetratesForceShield"
    );
    effect.penetrates_force_shield = None;
    assert!(
        can_attach_effect(ObjectCategory::Vehicle, Some(&shield), object, &effect, 50),
        "an absent PenetratesForceShield inherits the iron curtain flag"
    );
}

/// A paired list uses its own index, else the last value, else the key's
/// documented default.
#[test]
fn paired_lists_take_the_last_value_for_the_remaining_types() {
    assert_eq!(paired(&[5, 7], 0, 0), 5);
    assert_eq!(paired(&[5, 7], 1, 0), 7);
    assert_eq!(paired(&[5, 7], 4, 0), 7, "the last covers the rest");
    assert_eq!(paired(&[], 0, -1), -1, "an absent list keeps the default");
}

/// A re-creating effect is inactive until its initial delay clears, and a
/// `Powered` type follows its owner's activity; a plain type does not.
#[test]
fn active_state_follows_delays_and_the_powered_owner() {
    let plain = effect(3);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            initial_delay: 1,
            delay: 0,
            ..AttachEffectParams::default()
        },
        &plain,
    );
    instance.tick(&plain, false);
    assert!(
        !instance.is_active(false),
        "the delay frame does not initialize"
    );
    instance.tick(&plain, false);
    assert!(instance.is_active(false), "initialized, no pending delay");
    instance.online_check(&plain, false);
    assert!(instance.is_online(), "a plain type ignores its owner");

    let mut powered = effect(3);
    powered.powered = true;
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: 0,
            ..AttachEffectParams::default()
        },
        &powered,
    );
    instance.tick(&powered, false);
    instance.online_check(&powered, false);
    assert!(
        !instance.is_active(false),
        "a powered type follows its owner"
    );
    instance.online_check(&powered, true);
    assert!(instance.is_active(false));
}

/// A terminal effect is active only while its duration runs; a re-creating
/// one is inactive while its re-creation delay is pending.
#[test]
fn a_terminal_effect_follows_its_duration_and_a_recreating_one_its_delay() {
    let terminal = effect(1);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &terminal,
    );
    assert!(
        instance.is_active(false),
        "a terminal effect reads its duration"
    );
    instance.tick(&terminal, false);
    assert!(!instance.is_active(false), "duration ran out");

    let cycling = effect(1);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: 2,
            ..AttachEffectParams::default()
        },
        &cycling,
    );
    instance.tick(&cycling, false);
    assert_eq!(instance.current_delay(), 2);
    assert!(
        !instance.is_active(false),
        "the pending re-creation delay keeps it inactive"
    );
    instance.tick(&cycling, false);
    instance.tick(&cycling, false);
    assert!(instance.should_refresh());
    instance.tick(&cycling, false);
    assert!(instance.is_active(false), "refreshed and counting again");
}

/// The offline and temporal actions hide, pause or freeze the animation.
#[test]
fn animation_state_maps_the_offline_and_temporal_actions() {
    let mut powered = effect(3);
    powered.powered = true;
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &powered,
    );
    instance.tick(&powered, false);
    let online = instance.animation_state(&powered, false);
    assert!(online.show && !online.paused && !online.temporal);

    instance.online_check(&powered, false);
    let offline = instance.animation_state(&powered, false);
    assert!(!offline.show, "the default OfflineAction hides");

    powered.animation_offline_action = AttachedAnimFlag::Paused;
    let offline = instance.animation_state(&powered, false);
    assert!(offline.show && offline.paused && !offline.temporal);

    powered.animation_temporal_action = AttachedAnimFlag::PausedTemporal;
    let frozen = instance.animation_state(&powered, true);
    assert!(frozen.show && frozen.paused && frozen.temporal);

    let plain = effect(3);
    let instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &plain,
    );
    let untouched = instance.animation_state(&plain, true);
    assert!(untouched.show && !untouched.paused && !untouched.temporal);
}

/// The list round-trips through serde with its instance state intact.
#[test]
fn the_list_round_trips_through_serde() {
    let effect = cumulative_effect(4, 2);
    let mut list = AttachEffectList::default();
    attach_with(&mut list, &effect, 0);
    list.tick_all(std::slice::from_ref(&effect), |_, _, _| false);
    let json = serde_json::to_string(&list).expect("serialize");
    let back: AttachEffectList = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, list);
}

/// A non-cumulative type never stacks: the existing instance takes the
/// refresh, but only when the new duration wins or the site replaces
/// longer ones.
#[test]
fn reapplying_a_non_cumulative_type_refreshes_by_the_longer_rule() {
    let effect = effect(5);
    let mut list = AttachEffectList::default();
    assert_eq!(
        attach_with(&mut list, &effect, 0),
        AttachOutcome::Attached(0)
    );
    list.tick_all(std::slice::from_ref(&effect), |_, _, _| false);
    list.tick_all(std::slice::from_ref(&effect), |_, _, _| false);
    assert_eq!(list.instances()[0].remaining(), 3);

    assert_eq!(attach_with(&mut list, &effect, 2), AttachOutcome::Refreshed);
    assert_eq!(list.instances().len(), 1, "no second instance");
    assert_eq!(
        list.instances()[0].remaining(),
        3,
        "a shorter reapply is ignored"
    );

    let mut list = AttachEffectList::default();
    attach_with(&mut list, &effect, 0);
    let replace = AttachPolicy {
        replace_longer_duration: true,
        ..AttachPolicy::default()
    };
    let outcome = list.attach(
        0,
        AttachEffectParams {
            duration_override: 2,
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        replace,
        None,
    );
    assert_eq!(outcome, AttachOutcome::Refreshed);
    assert_eq!(
        list.instances()[0].remaining(),
        2,
        "replaced even though shorter"
    );
}

/// Below `Cumulative.MaxCount` the list stacks; at the cap it refreshes
/// the shortest match instead of appending.
#[test]
fn cumulative_stacks_below_the_cap_and_refreshes_the_shortest_at_it() {
    let effect = cumulative_effect(6, 2);
    let mut list = AttachEffectList::default();
    attach_with(&mut list, &effect, 6);
    attach_with(&mut list, &effect, 2);
    assert_eq!(list.instances().len(), 2, "below the cap both attach");

    assert_eq!(attach_with(&mut list, &effect, 3), AttachOutcome::Refreshed);
    assert_eq!(list.instances().len(), 2, "the cap refuses a third");
    assert_eq!(
        list.instances()[0].remaining(),
        6,
        "the longer match is untouched"
    );
    assert_eq!(
        list.instances()[1].remaining(),
        3,
        "the shortest match refreshed"
    );
}

/// `CumulativeRefreshAll` refreshes every match at the cap.
#[test]
fn refresh_all_at_the_cap_refreshes_every_match() {
    let effect = cumulative_effect(6, 2);
    let mut list = AttachEffectList::default();
    attach_with(&mut list, &effect, 6);
    attach_with(&mut list, &effect, 2);
    let policy = AttachPolicy {
        cumulative_refresh_all: true,
        ..AttachPolicy::default()
    };
    let outcome = list.attach(
        0,
        AttachEffectParams {
            duration_override: 3,
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        policy,
        None,
    );
    assert_eq!(outcome, AttachOutcome::Refreshed);
    assert_eq!(list.instances()[0].remaining(), 3);
    assert_eq!(list.instances()[1].remaining(), 3);
}

/// `CumulativeRefreshAll.OnAttach` refreshes under the cap and still adds
/// the new instance.
#[test]
fn refresh_all_on_attach_below_the_cap_still_appends() {
    let effect = cumulative_effect(5, 3);
    let mut list = AttachEffectList::default();
    attach_with(&mut list, &effect, 5);
    let policy = AttachPolicy {
        cumulative_refresh_all: true,
        cumulative_refresh_all_on_attach: true,
        ..AttachPolicy::default()
    };
    let outcome = list.attach(
        0,
        AttachEffectParams {
            duration_override: 2,
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        policy,
        None,
    );
    assert_eq!(outcome, AttachOutcome::Attached(1));
    assert_eq!(list.instances()[0].remaining(), 2, "the match refreshed");
    assert_eq!(
        list.instances()[1].remaining(),
        2,
        "the new instance starts at its own"
    );
}

/// `CumulativeRefreshSameSourceOnly` keeps another source's instance out of
/// the match set, so the cap is judged separately per source.
#[test]
fn same_source_only_skips_other_sources() {
    let effect = cumulative_effect(4, 2);
    let mut list = AttachEffectList::default();
    let first = Some(AttachSource {
        invoker: Some(1),
        source: Some(10),
        invoker_house: None,
    });
    let second = Some(AttachSource {
        invoker: Some(2),
        source: Some(20),
        invoker_house: None,
    });
    let policy = AttachPolicy::default();
    list.attach(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        policy,
        first,
    );
    list.attach(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        policy,
        second,
    );
    assert_eq!(
        list.instances().len(),
        2,
        "a different source still attaches"
    );
    assert_eq!(
        list.instances()[1].source,
        second,
        "the instance keeps who attached it"
    );
}

/// A source cap refuses the new instance even when the type cap allows it;
/// a different retained house is still the same source.
#[test]
fn the_source_cap_refreshes_instead_of_appending() {
    let effect = cumulative_effect(4, -1);
    let mut list = AttachEffectList::default();
    let source = Some(AttachSource {
        invoker: Some(1),
        source: Some(10),
        invoker_house: None,
    });
    let policy = AttachPolicy {
        cumulative_source_max_count: 1,
        ..AttachPolicy::default()
    };
    list.attach(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        policy,
        source,
    );
    let outcome = list.attach(
        0,
        AttachEffectParams {
            delay: -1,
            ..AttachEffectParams::default()
        },
        &effect,
        policy,
        source.map(|source| AttachSource {
            invoker_house: Some(crate::sim::intern::InternedId::from_index(3)),
            ..source
        }),
    );
    assert_eq!(outcome, AttachOutcome::Refreshed);
    assert_eq!(list.instances().len(), 1, "the source cap blocks the copy");
}

/// A two-house arena built from the given rules text, with every type id
/// and rule handle interned, ready for production spawns.
fn arena_scene(text: &str, seed: u64) -> (Simulation, crate::rules::ruleset::RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(text);
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules parse");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn spawn(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    type_name: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    sim.spawn_object(type_name, owner, rx, ry, 0, rules)
        .expect("arena spawn")
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate
        .entities
        .get(id)
        .expect("live entity")
        .health
        .current()
}

fn attach_count(sim: &Simulation, id: u64) -> usize {
    sim.substrate
        .entities
        .get(id)
        .expect("live entity")
        .phobos_attach_effects()
        .instances()
        .len()
}

fn direct_hit(
    target: u64,
    damage: i32,
    attacker: u64,
    warhead: crate::sim::intern::InternedId,
) -> crate::sim::combat::EntityDamageEvent {
    crate::sim::combat::EntityDamageEvent::direct_receiver(
        target,
        damage,
        0,
        attacker,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    )
}

const ALL_HIT: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";

/// Phobos `ExpireWeapon.TriggerOn=expire`: a terminal effect fires its
/// weapon at its own owner when the duration runs out. The weapon is named
/// only by the AttachEffect key, so this also covers the catalog's type
/// factory keeping it.
#[test]
fn an_expiring_effect_fires_its_expire_weapon_at_its_owner() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
             AttachEffect.AttachTypes=EXPIRES\nAttachEffect.Delays=-1\n\
             [HIT]\n{ALL_HIT}\
             [EXPW]\nDamage=5\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=EXPIRES\n\
             [EXPIRES]\nDuration=1\nExpireWeapon=EXPW\n"
        ),
        17,
    );
    assert!(rules.weapon("EXPW").is_some(), "the AE key allocated EXPW");
    assert_eq!(
        rules.weapon("EXPW").unwrap().warhead.as_deref(),
        Some("HIT")
    );
    let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
    assert_eq!(health(&sim, target), 1000);

    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(attach_count(&sim, target), 0, "the effect ended");
    assert_eq!(
        health(&sim, target),
        995,
        "the expire weapon detonated at the owner"
    );
}

/// Phobos `BulletExt::Detonate`'s `pFiringHouse`: an expire shot whose
/// invoker Techno is gone keeps the House that attached the effect, so
/// the detonation's `AffectsAllies` gate still sees it.
#[test]
fn an_expired_invokers_shot_keeps_the_attaching_house() {
    let run = |attacker_house: &str, remove_attacker: bool| -> i32 {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n\
                 [VehicleTypes]\n0=SOURCE\n1=TARGET\n\
                 [SOURCE]\nStrength=1000\nSpeed=6\nArmor=none\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n\
                 [Warheads]\n0=DROP\n1=GATE\n\
                 [DROP]\nAttachEffect.AttachTypes=EXPIRES\n{ALL_HIT}\
                 [GATE]\nAffectsAllies=no\n{ALL_HIT}\
                 [EXPW]\nDamage=5\nRange=1\nWarhead=GATE\n\
                 [AttachEffectTypes]\n0=EXPIRES\n\
                 [EXPIRES]\nDuration=1\nExpireWeapon=EXPW\n\
                 ExpireWeapon.UseInvokerAsOwner=yes\n"
            ),
            41,
        );
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        let attacker = spawn(&mut sim, &rules, "SOURCE", attacker_house, 20, 20);
        let house = sim.interner.intern(attacker_house);
        let warhead = sim.interner.intern("DROP");
        sim.apply_warhead_attach_effects(
            target,
            warhead,
            Some(attacker),
            Some(house),
            &rules,
            &mut crate::sim::world::FrameEffects::empty(),
            None,
        );
        assert_eq!(attach_count(&sim, target), 1, "{attacker_house}: attached");
        if remove_attacker {
            sim.substrate.entities.remove(attacker);
        }
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(attach_count(&sim, target), 0, "{attacker_house}: expired");
        health(&sim, target)
    };

    assert_eq!(
        run("Russians", false),
        995,
        "a live enemy invoker's house passes the gate"
    );
    assert_eq!(
        run("Americans", true),
        1000,
        "the house retained for the expired invoker still blocks the shot"
    );
}

/// Phobos `RevengeWeapon`: a killed effect owner fires its weapon at the
/// killing source, through the ordinary receiver and detonation paths.
#[test]
fn a_dying_effect_owner_fires_its_revenge_weapon_at_the_killer() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             [TARGET]\nStrength=50\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REV\n\
             [HIT]\n{ALL_HIT}\
             [KILLW]\nDamage=100\nRange=1\nWarhead=HIT\n\
             [REVW]\nDamage=7\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=REV\n[REV]\nDuration=-1\nRevengeWeapon=REVW\n"
        ),
        23,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    sim.update_phobos_attach_effects(victim, &rules);
    assert_eq!(attach_count(&sim, victim), 1);

    let warhead = sim.interner.intern("HIT");
    let event = direct_hit(victim, 100, attacker, warhead);
    let _ = sim.commit_direct_damage_receiver(
        &rules,
        None,
        event,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert!(
        sim.substrate.entities.get(victim).is_none() || health(&sim, victim) == 0,
        "the victim died"
    );
    assert_eq!(
        health(&sim, attacker),
        993,
        "the revenge weapon hit the killer for 7"
    );
}

/// Phobos `TechnoClass_ReceiveDamage_AttachEffectExpireWeapon` (`0x702050`):
/// a dying effect owner fires `ExpireWeapon.TriggerOn=death` at itself
/// before the death weapon; two cumulative instances with
/// `CumulativeOnlyOnce` fire once, and a type without the death trigger
/// does not fire at all.
#[test]
fn a_dying_effect_owner_fires_its_death_expire_weapon_once() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=ATTACKER\n1=TARGET\n2=BYSTANDER\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             [TARGET]\nStrength=20\nSpeed=6\nArmor=none\n\
             AttachEffect.AttachTypes=DEATH,DEATH,OTHER\n\
             [BYSTANDER]\nStrength=1000\nSpeed=6\nArmor=none\n\
             [KHIT]\n{ALL_HIT}\
             [DHIT]\nCellSpread=1\nAffectsAllies=yes\n{ALL_HIT}\
             [KILLW]\nDamage=100\nRange=1\nWarhead=KHIT\n\
             [DEATHW]\nDamage=7\nRange=1\nWarhead=DHIT\n\
             [AttachEffectTypes]\n0=DEATH\n1=OTHER\n\
             [DEATH]\nDuration=-1\nExpireWeapon=DEATHW\nExpireWeapon.TriggerOn=death\n\
             Cumulative=yes\nExpireWeapon.CumulativeOnlyOnce=yes\n\
             [OTHER]\nDuration=-1\nExpireWeapon=DEATHW\n"
        ),
        31,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    let bystander = spawn(&mut sim, &rules, "BYSTANDER", "Americans", 21, 21);
    sim.update_phobos_attach_effects(victim, &rules);
    assert_eq!(attach_count(&sim, victim), 3);
    assert_eq!(health(&sim, bystander), 1000);

    let warhead = sim.interner.intern("KHIT");
    let event = direct_hit(victim, 100, attacker, warhead);
    let _ = sim.commit_direct_damage_receiver(
        &rules,
        None,
        event,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(
        health(&sim, bystander),
        993,
        "only the once-per-type death shot reached the bystander"
    );
}

/// Phobos `ReflectDamage`: the `.Override` fixed value and the
/// `.Multiplier` form of the defense-divided received damage.
#[test]
fn reflect_damage_returns_the_received_packet_by_override_or_multiplier() {
    for (keys, expected) in [
        ("ReflectDamage.Override=9\n", 991),
        ("ReflectDamage.Multiplier=0.5\n", 995),
    ] {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
                 [HIT]\n{ALL_HIT}\
                 [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=REF\n\
                 [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n{keys}"
            ),
            29,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        sim.update_phobos_attach_effects(victim, &rules);

        let warhead = sim.interner.intern("HIT");
        let event = direct_hit(victim, 10, attacker, warhead);
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            &mut crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(health(&sim, victim), 990, "{keys}");
        assert_eq!(health(&sim, attacker), expected, "{keys}");
    }
}

/// Phobos `ReflectDamage.Delay` (`Hooks.ReceiveDamage.cpp:429,461`): a
/// successful reflection starts the per-instance timer; a hit inside it is
/// not reflected, and one after it is.
#[test]
fn reflect_damage_delay_blocks_the_next_hit_until_it_runs_out() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
             [HIT]\n{ALL_HIT}\
             [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=REF\n\
             [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n\
             ReflectDamage.Override=7\nReflectDamage.Delay=5\n"
        ),
        37,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    sim.update_phobos_attach_effects(victim, &rules);
    let warhead = sim.interner.intern("HIT");
    let hit = |sim: &mut Simulation| {
        let event = direct_hit(victim, 1, attacker, warhead);
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            &mut crate::sim::world::FrameEffects::empty(),
        );
    };
    hit(&mut sim);
    assert_eq!(health(&sim, attacker), 993, "the first hit reflects");
    hit(&mut sim);
    assert_eq!(
        health(&sim, attacker),
        993,
        "the running delay blocks the second"
    );
    sim.session.set_binary_frame_for_test(6);
    hit(&mut sim);
    assert_eq!(
        health(&sim, attacker),
        986,
        "after the delay it reflects again"
    );
}

/// Phobos `SuppressReflectDamage`: a bare key suppresses every reflecting
/// effect, so the hit deals no reflected damage.
#[test]
fn suppress_reflect_damage_stops_the_return_hit() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
             [HIT]\n{ALL_HIT}SuppressReflectDamage=yes\n\
             [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=REF\n\
             [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n\
             ReflectDamage.Override=9\n"
        ),
        31,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    sim.update_phobos_attach_effects(victim, &rules);

    let warhead = sim.interner.intern("HIT");
    let event = direct_hit(victim, 10, attacker, warhead);
    let _ = sim.commit_direct_damage_receiver(
        &rules,
        None,
        event,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(health(&sim, victim), 990);
    assert_eq!(health(&sim, attacker), 1000, "the suppressor refused it");
}

/// The reflected hit's own warhead is marked `Reflected` while it lands,
/// so two reflectors carrying the same warhead cannot ping-pong.
#[test]
fn a_reflected_hit_never_reflects_again() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             AttachEffect.AttachTypes=REF\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
             [HIT]\n{ALL_HIT}\
             [ECHO]\n{ALL_HIT}\
             [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=REF\n\
             [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=ECHO\n"
        ),
        37,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    sim.update_phobos_attach_effects(attacker, &rules);
    sim.update_phobos_attach_effects(victim, &rules);

    let warhead = sim.interner.intern("HIT");
    let event = direct_hit(victim, 10, attacker, warhead);
    let _ = sim.commit_direct_damage_receiver(
        &rules,
        None,
        event,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(health(&sim, victim), 990, "the incoming hit only");
    assert_eq!(
        health(&sim, attacker),
        990,
        "one reflected ECHO hit, and its own reflection was refused"
    );
}

/// Phobos `0x701E18` runs after Object's body (`0x00701DF8`): the packet
/// it reflects is the post-Verses `*pDamage`, not the divided input.
#[test]
fn reflect_damage_returns_the_post_verses_packet() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
             [HIT]\n{ALL_HIT}\
             [HALF]\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%\n\
             [KILLW]\nDamage=1\nRange=1\nWarhead=HALF\n\
             [AttachEffectTypes]\n0=REF\n\
             [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n\
             ReflectDamage.Multiplier=1.0\n"
        ),
        29,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    sim.update_phobos_attach_effects(victim, &rules);
    let event = direct_hit(victim, 10, attacker, sim.interner.intern("HALF"));
    let _ = sim.commit_direct_damage_receiver(
        &rules,
        None,
        event,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(health(&sim, victim), 995, "50% of 10");
    assert_eq!(
        health(&sim, attacker),
        995,
        "the reflected 5, not the 10 sent"
    );
}

/// The same hook counts `DiscardOn=ReceivedDamage`: a healing packet does
/// not count, an IgnoreDefenses hit does.
#[test]
fn received_damage_discard_counts_positive_hits_with_ignore_defenses() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
             [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=RECEIVE\n\
             [HIT]\n{ALL_HIT}\
             [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=RECEIVE\n\
             [RECEIVE]\nDuration=-1\nDiscardOn=receiveddamage\n\
             DiscardOn.ReceivedDamage.Count=1\n"
        ),
        29,
    );
    let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
    let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
    sim.update_phobos_attach_effects(victim, &rules);
    let warhead = sim.interner.intern("HIT");
    let hit = |sim: &mut Simulation, damage: i32, ignore_defenses: bool| {
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            victim,
            damage,
            0,
            attacker,
            None,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses,
                arg6: false,
            },
        );
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            &mut crate::sim::world::FrameEffects::empty(),
        );
        sim.update_phobos_attach_effects(victim, &rules);
        attach_count(sim, victim)
    };
    assert_eq!(hit(&mut sim, -10, false), 1, "a heal is not damage");
    assert_eq!(hit(&mut sim, 10, true), 0, "IgnoreDefenses still counts");
}

/// Phobos `TechnoClass_Limbo_Wrapper`: a removed `DiscardOn=Entry` effect
/// leaves the cached stats and fires its `Discard` expire weapon at the
/// object before the Limbo body.
#[test]
fn limbo_entry_discard_recalculates_and_fires_the_discard_weapon() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
             AttachEffect.AttachTypes=ENTRY\n\
             [HIT]\n{ALL_HIT}\
             [EXPW]\nDamage=5\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=ENTRY\n\
             [ENTRY]\nDuration=-1\nDiscardOn=entry\nFirepowerMultiplier=2\n\
             ExpireWeapon=EXPW\nExpireWeapon.TriggerOn=discard\n"
        ),
        17,
    );
    let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
    sim.update_phobos_attach_effects(target, &rules);
    let firepower = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(target)
            .expect("live entity")
            .phobos_stat_multipliers()
            .firepower
    };
    assert_eq!(firepower(&sim), 2.0);
    sim.limbo_phobos_attach_effects(
        target,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(attach_count(&sim, target), 0);
    assert_eq!(firepower(&sim), 1.0, "the effect's factor is gone");
    assert_eq!(health(&sim, target), 995, "the discard weapon detonated");
}

/// Phobos `AttachEffect.RequiredTypes` on a weapon: without the effect on
/// the target GetFireError returns `Cant`; with it the same shot is legal.
#[test]
fn a_weapon_attach_effect_requirement_gates_the_fire_error() {
    use crate::sim::combat::fire_error::FireError;

    let build = |attach: bool, weapon_extra: &str| {
        let (sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=GATEW\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n{}\
                 [HIT]\n{ALL_HIT}\
                 [GATEW]\nDamage=10\nRange=-2\nWarhead=HIT\nOmniFire=yes\n{weapon_extra}\
                 [AttachEffectTypes]\n0=REQ\n1=OTHER\n[REQ]\nDuration=-1\n[OTHER]\nDuration=-1\n",
                if attach {
                    "AttachEffect.AttachTypes=REQ\n"
                } else {
                    ""
                }
            ),
            41,
        );
        (sim, rules)
    };
    let run = |attach: bool, weapon_extra: &str| {
        let (mut sim, rules) = build(attach, weapon_extra);
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 23, 20);
        if attach {
            sim.update_phobos_attach_effects(victim, &rules);
        }
        crate::sim::combat::fire_error_world::FireSubject {
            world: &sim,
            rules: &rules,
            fog: None,
            firer: sim.substrate.entities.get(attacker).unwrap(),
            obj: rules.object("ATTACKER").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(victim)),
            weapon_index: 0,
        }
        .fire_error(false)
    };

    assert_eq!(
        run(false, "AttachEffect.RequiredTypes=REQ\n"),
        FireError::Cant,
        "the target lacks REQ"
    );
    assert_eq!(
        run(true, "AttachEffect.RequiredTypes=REQ\n"),
        FireError::Ok,
        "the target carries REQ, so the shot is legal"
    );

    // `Required.Any` needs only one listed entry; the default needs all.
    assert_eq!(
        run(
            true,
            "AttachEffect.RequiredTypes=REQ,OTHER\nAttachEffect.Required.Any=yes\n"
        ),
        FireError::Ok,
        "one of the two required entries is present"
    );
    assert_eq!(
        run(true, "AttachEffect.RequiredTypes=REQ,OTHER\n"),
        FireError::Cant,
        "the default needs every required entry"
    );

    // `Disallowed.Any` defaults true; clearing it needs every entry.
    assert_eq!(
        run(true, "AttachEffect.DisallowedTypes=REQ,OTHER\n"),
        FireError::Cant,
        "any disallowed entry refuses the shot"
    );
    assert_eq!(
        run(
            true,
            "AttachEffect.DisallowedTypes=REQ,OTHER\nAttachEffect.Disallowed.Any=no\n"
        ),
        FireError::Ok,
        "requireAll disallowed needs the whole list"
    );

    // A self-owned effect does not come from the shot's source.
    assert_eq!(
        run(
            true,
            "AttachEffect.RequiredTypes=REQ\nAttachEffect.SameSourceOnly=yes\n"
        ),
        FireError::Cant,
        "the target's own effect is not from the firer"
    );
    // The instance's house is the target's own house for a self effect.
    assert_eq!(
        run(
            true,
            "AttachEffect.RequiredTypes=REQ\nAttachEffect.Required.Houses=enemies\n"
        ),
        FireError::Ok,
        "the target's house is the firer's enemy"
    );
    assert_eq!(
        run(
            true,
            "AttachEffect.RequiredTypes=REQ\nAttachEffect.Required.Houses=owner\n"
        ),
        FireError::Cant,
        "the target's house is not the firer's"
    );
}

/// Phobos `AttachEffect.CumulativeRemoveMinCounts` / `MaxCounts` and
/// `RemoveGroups`: the min keeps the whole stack, the max removes only
/// that many, a later pass below the min keeps the rest, and a group
/// removes what is left.
#[test]
fn warhead_removal_boundaries_follow_min_max_counts_and_groups() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
             AttachEffect.AttachTypes=STACK,STACK,STACK\nAttachEffect.Delays=-1\n\
             [Warheads]\n0=KEEPMIN\n1=LIMITMAX\n2=LATERMIN\n3=GROUPOFF\n\
             [KEEPMIN]\n{ALL_HIT}AttachEffect.RemoveTypes=STACK\n\
             AttachEffect.CumulativeRemoveMinCounts=4\n\
             [LIMITMAX]\n{ALL_HIT}AttachEffect.RemoveTypes=STACK\n\
             AttachEffect.CumulativeRemoveMaxCounts=2\n\
             [LATERMIN]\n{ALL_HIT}AttachEffect.RemoveTypes=STACK\n\
             AttachEffect.CumulativeRemoveMinCounts=3\n\
             [GROUPOFF]\n{ALL_HIT}AttachEffect.RemoveGroups=tag\n\
             [AttachEffectTypes]\n0=STACK\n\
             [STACK]\nDuration=100\nCumulative=yes\nGroups=tag\n"
        ),
        43,
    );
    let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(attach_count(&sim, target), 3, "three stacked instances");

    let detonate = |sim: &mut Simulation, warhead: &str| {
        let warhead_ref = sim.interner.intern(warhead);
        let impact = {
            let entity = sim.substrate.entities.get(target).unwrap();
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: u64::MAX,
            source_id: target,
            target: crate::sim::projectile::ProjectileTarget::Entity(target),
            impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
            payload: crate::sim::projectile::ProjectilePayload::new(1, warhead_ref, None, false),
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        };
        sim.commit_attach_effect_detonations(
            &[detonation],
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::empty(),
        );
    };

    detonate(&mut sim, "KEEPMIN");
    assert_eq!(attach_count(&sim, target), 3, "min 4 keeps the stack of 3");

    detonate(&mut sim, "LIMITMAX");
    assert_eq!(attach_count(&sim, target), 1, "max 2 removed two");

    detonate(&mut sim, "LATERMIN");
    assert_eq!(attach_count(&sim, target), 1, "min 3 keeps the last one");

    detonate(&mut sim, "GROUPOFF");
    assert_eq!(attach_count(&sim, target), 0, "the group removed the last");
}

/// Phobos `ExpireWeapon.TriggerOn=remove`: the weapon fires when a
/// warhead's `AttachEffect.RemoveTypes` takes the effect off its owner.
#[test]
fn a_removed_effect_fires_its_remove_weapon() {
    let (mut sim, rules) = arena_scene(
        &format!(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
             AttachEffect.AttachTypes=REMOVABLE\nAttachEffect.Delays=-1\n\
             [Warheads]\n0=REMWH\n\
             [REMWH]\n{ALL_HIT}AttachEffect.RemoveTypes=REMOVABLE\n\
             [HIT]\n{ALL_HIT}\
             [EXPW]\nDamage=5\nRange=1\nWarhead=HIT\n\
             [AttachEffectTypes]\n0=REMOVABLE\n\
             [REMOVABLE]\nDuration=100\nExpireWeapon=EXPW\n\
             ExpireWeapon.TriggerOn=remove\n"
        ),
        47,
    );
    let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
    sim.update_phobos_attach_effects(target, &rules);
    assert_eq!(attach_count(&sim, target), 1);

    let warhead_ref = sim.interner.intern("REMWH");
    let impact = {
        let entity = sim.substrate.entities.get(target).unwrap();
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref())
    };
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: u64::MAX,
        source_id: target,
        target: crate::sim::projectile::ProjectileTarget::Entity(target),
        impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
        payload: crate::sim::projectile::ProjectilePayload::new(1, warhead_ref, None, false),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    sim.commit_attach_effect_detonations(
        &[detonation],
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::empty(),
    );
    assert_eq!(attach_count(&sim, target), 0, "the warhead removed it");
    assert_eq!(
        health(&sim, target),
        994,
        "the remove weapon fired for 5 on top of the warhead's 1"
    );
}

/// A positive re-creation delay clears the duration and arms the delay.
#[test]
fn reset_arms_the_recreation_delay() {
    let effect = effect(4);
    let mut instance = AttachEffectInstance::new(
        0,
        AttachEffectParams {
            recreation_delay: 3,
            ..AttachEffectParams::default()
        },
        &effect,
    );
    assert!(instance.reset_if_recreatable());
    assert_eq!(instance.remaining(), 0);
    assert_eq!(instance.current_delay(), 3);
    assert!(instance.should_refresh(), "the queued refresh refills it");
}

/// `GetCritChance`'s AttachEffect fold: a multiplier scales the warhead's
/// chance, extra chances add after it, and the Allow/Disallow lists gate
/// the effect's contribution.
#[test]
fn attach_effect_crit_modifiers_fold_into_the_warhead_chance() {
    use crate::rules::ini_parser::IniFile;
    let crit_damage = |firer_ini: &str, effect_ini: &str| {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=FIRER\n1=TARGET\n\
             [WeaponTypes]\n0=GUN\n[GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
             [Warheads]\n0=WH\n\
             [WH]\nCellSpread=0\nCrit.Chance=0.05\nCrit.ExtraDamage=50\n\
             [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n{firer_ini}\
             [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n\
             [AttachEffectTypes]\n0=CRIT\n\
             [CRIT]\nDuration=100\n{effect_ini}"
        )))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let firer = sim
            .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
            .expect("firer");
        let target = sim
            .spawn_object("TARGET", "Russians", 22, 20, 0, &rules)
            .expect("target");
        sim.update_phobos_attach_effects(firer, &rules);
        // The shot's shared dice must miss the bare 0.05 chance, so only a
        // folded chance can crit.
        let mut probe = sim.scenario_rng.clone();
        let dice = probe.random_double();
        assert!(dice > 0.05, "the seed's dice must miss the base chance");
        let coords = {
            let target = sim.entities().get(target).expect("target");
            crate::sim::movement::ground_pose::object_get_coords(
                target,
                sim.resolved_terrain.as_ref(),
            )
        };
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: target,
            source_id: firer,
            target: crate::sim::projectile::ProjectileTarget::Entity(target),
            impact: crate::sim::projectile::ProjectileCoord::new(coords.x, coords.y, coords.z),
            payload: crate::sim::projectile::ProjectilePayload::new(
                0,
                sim.interner.intern("WH"),
                None,
                false,
            ),
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        };
        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
        crate::sim::combat::world_receiver::commit_projectiles(
            &mut sim,
            &mut run,
            &[detonation],
            &rules,
            None,
        );
        run.finish();
        500 - sim.entities().get(target).unwrap().health.current()
    };

    assert_eq!(crit_damage("", ""), 0, "no effect keeps the base chance");
    assert_eq!(
        crit_damage("AttachEffect.AttachTypes=CRIT\n", "Crit.ExtraChance=1.0\n"),
        50,
        "ExtraChance adds after the multiplier"
    );
    assert_eq!(
        crit_damage("AttachEffect.AttachTypes=CRIT\n", "Crit.Multiplier=30\n"),
        50,
        "a multiplier scales the chance"
    );
    assert_eq!(
        crit_damage(
            "AttachEffect.AttachTypes=CRIT\n",
            "Crit.ExtraChance=1.0\nCrit.AllowWarheads=OTHER\n",
        ),
        0,
        "an unmatched AllowWarheads list skips the effect"
    );
    assert_eq!(
        crit_damage(
            "AttachEffect.AttachTypes=CRIT\n",
            "Crit.ExtraChance=1.0\nCrit.DisallowWarheads=wh\n",
        ),
        0,
        "a matching DisallowWarheads list skips the effect"
    );
}
