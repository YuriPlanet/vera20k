//! The `[CombatDamage] Shrapnel.*` globals (`Ext/Rules/Body.cpp`), whose
//! `Nullable` default reads `no`.
use super::PhobosGlobals;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosGlobals {
    PhobosGlobals::from_ini(&IniFile::from_str(text))
}

/// `[General]` force-weapon fallbacks (`Ext/Rules/Body.cpp:230-232`,
/// `Ext/Rules/Body.h:674-676`).
#[test]
fn the_force_weapon_globals_default_and_read_from_general() {
    let defaults = PhobosGlobals::default();
    assert!(
        defaults.force_weapon_in_range_techno_only,
        "TechnoOnly constructs yes"
    );
    assert!(!defaults.force_weapon_in_range_apply_range_modifiers);
    assert!(!defaults.force_aa_weapon_in_range_apply_range_modifiers);

    let set = read(
        "[General]\nForceWeapon.InRange.TechnoOnly=no\n\
         ForceWeapon.InRange.ApplyRangeModifiers=yes\n\
         ForceAAWeapon.InRange.ApplyRangeModifiers=yes\n",
    );
    assert!(!set.force_weapon_in_range_techno_only);
    assert!(set.force_weapon_in_range_apply_range_modifiers);
    assert!(set.force_aa_weapon_in_range_apply_range_modifiers);
}

/// `[General] ApplyModifiersOnNegativeDamage=` (`Ext/Rules/Body.cpp:492`,
/// constructor false `Body.h:907`) is the fallback for the warhead
/// `Nullable` key (`Ext/WarheadType/Body.cpp:185`, hooks `0x4891AF`).
#[test]
fn the_negative_damage_modifier_fallback() {
    assert!(!PhobosGlobals::default().apply_modifiers_on_negative_damage);
    assert!(
        read(
            "[General]
ApplyModifiersOnNegativeDamage=yes
"
        )
        .apply_modifiers_on_negative_damage
    );
    assert!(
        !read(
            "[CombatDamage]
ApplyModifiersOnNegativeDamage=yes
"
        )
        .apply_modifiers_on_negative_damage
    );
    #[cfg(feature = "phobos")]
    {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Warheads]
0=KEYED
1=UNSET
             [KEYED]
ApplyModifiersOnNegativeDamage=yes
[UNSET]
",
        ))
        .unwrap();
        assert!(
            rules
                .warhead("KEYED")
                .unwrap()
                .apply_modifiers_on_negative_damage(&rules)
        );
        assert!(
            !rules
                .warhead("UNSET")
                .unwrap()
                .apply_modifiers_on_negative_damage(&rules)
        );
    }
}

/// `[CombatDamage] Warhead.PreventScatter=` (`Ext/Rules/Body.cpp:648`,
/// constructor false `Body.h:1042`) is the fallback for the warhead
/// `Nullable<bool>` key (`Ext/WarheadType/Body.cpp:491`) in Phobos' re-hook of
/// the native `FootClass::Scatter` call site (`Misc/Hooks.Ares.cpp:273-281`,
/// returns `0x702D11`). An authored `no`, like a `<default>` reset, is a read
/// value and therefore outranks a `yes` global.
#[cfg(feature = "phobos")]
#[test]
fn the_warhead_prevent_scatter_fallback() {
    let global_yes = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nWarhead.PreventScatter=yes\n\
         [Warheads]\n0=UNSET\n1=OFF\n2=ON\n\
         [UNSET]\n[OFF]\nPreventScatter=no\n[ON]\nPreventScatter=yes\n",
    ))
    .unwrap();
    assert!(
        global_yes
            .warhead("UNSET")
            .unwrap()
            .prevents_scatter(&global_yes),
        "an absent key consults the global"
    );
    assert!(
        !global_yes
            .warhead("OFF")
            .unwrap()
            .prevents_scatter(&global_yes),
        "an authored no scatters even under a yes global"
    );
    assert!(
        global_yes
            .warhead("ON")
            .unwrap()
            .prevents_scatter(&global_yes)
    );

    let global_off = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nWarhead.PreventScatter=no\n\
         [Warheads]\n0=UNSET\n1=RESET\n2=ON\n\
         [UNSET]\n[RESET]\nPreventScatter=<default>\n[ON]\nPreventScatter=yes\n",
    ))
    .unwrap();
    assert!(
        !global_off
            .warhead("UNSET")
            .unwrap()
            .prevents_scatter(&global_off)
    );
    assert!(
        !global_off
            .warhead("RESET")
            .unwrap()
            .prevents_scatter(&global_off),
        "`<default>` clears the nullable back to the global"
    );
    assert!(
        global_off
            .warhead("ON")
            .unwrap()
            .prevents_scatter(&global_off)
    );
}

/// `[CombatDamage] AllowBerzerkOnAllies=` (`Ext/Rules/Body.cpp:421`,
/// constructor `Body.h:339,847` no): the switch on Phobos' re-added Psychedelic
/// ally test (`0x701D2E`, `Ext/Techno/Hooks.ReceiveDamage.cpp:535`). It sits
/// next to `BerzerkTargeting` in the same section.
#[cfg(feature = "phobos")]
#[test]
fn allow_berzerk_on_allies_constructs_false_and_reads_from_combat_damage() {
    assert!(!PhobosGlobals::default().allow_berzerk_on_allies);
    assert!(read("[CombatDamage]\nAllowBerzerkOnAllies=yes\n").allow_berzerk_on_allies);
    assert!(!read("[CombatDamage]\nAllowBerzerkOnAllies=no\n").allow_berzerk_on_allies);
    assert!(
        !read("[General]\nAllowBerzerkOnAllies=yes\n").allow_berzerk_on_allies,
        "the key is a `[CombatDamage]` read"
    );
}

/// `[General] AIChronoSphereSW=` / `AIChronoWarpSW=` (`ValueableIdx`, i.e. a
/// superweapon type ID, constructors the unset index `-1`,
/// `Ext/Rules/Body.cpp:122-123`, `Body.h:41-42,581-582`): the supers
/// `HouseExt::GetAIChronoshiftSupers` (`Ext/House/Body.cpp:333-368`) uses for
/// the computer's Chronoshift actions.
#[cfg(feature = "phobos")]
#[test]
fn the_ai_chronoshift_supers_default_and_read_from_general() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.ai_chrono_sphere_sw, "");
    assert_eq!(defaults.ai_chrono_warp_sw, "");
    assert_eq!(read(""), defaults);

    let set = read("[General]\nAIChronoSphereSW=EUCLSF\nAIChronoWarpSW=CAL001\n");
    assert_eq!(set.ai_chrono_sphere_sw, "EUCLSF");
    assert_eq!(set.ai_chrono_warp_sw, "CAL001");

    // `none` is the read's blank: it stores the unset index, not a name.
    let cleared = read("[General]\nAIChronoSphereSW=none\nAIChronoWarpSW=<none>\n");
    assert_eq!(cleared, defaults);

    assert_eq!(
        read("[CombatDamage]\nAIChronoSphereSW=EUCLSF\nAIChronoWarpSW=CAL001\n"),
        defaults,
        "both keys live in `[General]`"
    );
}

/// `[General] AnimCraterDestroyTiberium=` (`Ext/Rules/Body.cpp:418`,
/// constructor `Ext/Rules/Body.h:844`) defaults yes and is the fallback for an
/// AnimType's unset art `Crater.DestroyTiberium=` (`Ext/AnimType/Body.cpp:126`).
#[test]
fn the_anim_crater_destroy_tiberium_fallback() {
    assert!(PhobosGlobals::default().anim_crater_destroy_tiberium);
    assert_eq!(read(""), PhobosGlobals::default());
    assert!(!read("[General]\nAnimCraterDestroyTiberium=no\n").anim_crater_destroy_tiberium);
    assert!(
        read("[CombatDamage]\nAnimCraterDestroyTiberium=no\n").anim_crater_destroy_tiberium,
        "a [General] read, not CombatDamage"
    );
    #[cfg(feature = "phobos")]
    {
        // Phobos reads under the AnimType's own section (pID), so an ART
        // `Image=` redirect never supplies the key.
        let art = IniFile::from_str(
            "[MARK]\nCrater.DestroyTiberium=no\n[ALIAS]\nImage=MARK\nStart=1\n[PLAIN]\nStart=1\n",
        );
        let registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
        assert_eq!(
            registry
                .anim_runtime_config("MARK")
                .unwrap()
                .crater_destroy_tiberium,
            Some(false)
        );
        assert_eq!(
            registry
                .anim_runtime_config("PLAIN")
                .unwrap()
                .crater_destroy_tiberium,
            None
        );
        assert_eq!(
            registry
                .anim_runtime_config("ALIAS")
                .unwrap()
                .crater_destroy_tiberium,
            None,
            "the section's own Image= is not followed"
        );
        assert!(registry.anim_runtime_config("MISSING").is_none());
    }
}

/// `[CombatDamage] PenetratesTransport.Level=`, default 10
/// (`Ext/Rules/Body.h:945`).
#[test]
fn the_penetrates_transport_level_defaults_and_reads() {
    assert_eq!(PhobosGlobals::default().penetrates_transport_level, 10);
    assert_eq!(
        read("[CombatDamage]\nPenetratesTransport.Level=3\n").penetrates_transport_level,
        3
    );
}

#[test]
fn the_shrapnel_globals_default_to_the_stock_admission() {
    // An absent section, an absent key and a later layer that names nothing all
    // leave the stock behaviour, which is why the unset value is `no`.
    assert_eq!(read(""), PhobosGlobals::default());
    assert_eq!(read("[CombatDamage]\n"), PhobosGlobals::default());
    assert!(!PhobosGlobals::default().shrapnel_affects_ground);
    assert!(!PhobosGlobals::default().shrapnel_affects_buildings);

    let set = read("[CombatDamage]\nShrapnel.AffectsGround=yes\nShrapnel.AffectsBuildings=yes\n");
    assert!(set.shrapnel_affects_ground);
    assert!(set.shrapnel_affects_buildings);

    // A later layer naming only one key keeps the other at its default.
    let half = read("[CombatDamage]\nShrapnel.AffectsBuildings=yes\n");
    assert!(!half.shrapnel_affects_ground);
    assert!(half.shrapnel_affects_buildings);

    // The AutoDeath scan layers default to the same `no`.
    assert!(!PhobosGlobals::default().auto_death_technos_exist_allow_limboed);
    assert!(!PhobosGlobals::default().auto_death_technos_dont_exist_allow_limboed);
    // `Ext/Rules/Body.h:1027` constructs this one `true`.
    assert!(PhobosGlobals::default().auto_death_allow_limboed);
    // `Ext/Rules/Body.h:923` constructs the invoker-state fallback `true`.
    assert!(PhobosGlobals::default().affects_invoker_only_ignore_invoker_state);
    assert!(
        !read(
            "[CombatDamage]
AffectsInvokerOnly.IgnoreInvokerState=no
"
        )
        .affects_invoker_only_ignore_invoker_state
    );
    let limbo = read(
        "[CombatDamage]\nAutoDeath.TechnosExist.AllowLimboed=yes\n\
         AutoDeath.TechnosDontExist.AllowLimboed=yes\n",
    );
    assert!(limbo.auto_death_technos_exist_allow_limboed);
    assert!(limbo.auto_death_technos_dont_exist_allow_limboed);
}

/// `[AudioVisual]` block (`Ext/Rules/Body.cpp:291-304`), whose `Valueable`
/// defaults come from `Ext/Rules/Body.h:725-732`.
#[test]
fn the_drain_display_globals_default_to_phobos_constructor_values() {
    assert_eq!(read(""), PhobosGlobals::default());
    let defaults = PhobosGlobals::default();
    assert!(!defaults.drain_money_display);
    assert_eq!(
        defaults.drain_money_display_houses,
        crate::rules::phobos_shields::AffectedHouse::ALL
    );
    assert!(!defaults.drain_money_display_on_target);
    assert!(
        defaults.drain_money_display_on_target_use_display_income,
        "UseDisplayIncome constructs yes"
    );
    assert_eq!(
        defaults.display_income_houses,
        crate::rules::phobos_shields::AffectedHouse::ALL
    );

    let set = read(
        "[AudioVisual]\nDrainMoneyDisplay=yes\nDrainMoneyDisplay.Houses=owner,enemies\n\
         DrainMoneyDisplay.OnTarget=yes\nDrainMoneyDisplay.OnTarget.UseDisplayIncome=no\n\
         DisplayIncome.Houses=allies\n",
    );
    assert!(set.drain_money_display);
    assert!(
        set.drain_money_display_houses
            .contains(crate::rules::phobos_shields::AffectedHouse::OWNER)
    );
    assert!(
        set.drain_money_display_houses
            .contains(crate::rules::phobos_shields::AffectedHouse::ENEMIES)
    );
    assert!(
        !set.drain_money_display_houses
            .contains(crate::rules::phobos_shields::AffectedHouse::ALLIES)
    );
    assert!(set.drain_money_display_on_target);
    assert!(!set.drain_money_display_on_target_use_display_income);
    assert_eq!(
        set.display_income_houses,
        crate::rules::phobos_shields::AffectedHouse::ALLIES
    );

    // The `[CombatDamage]` block keeps its own defaults in a layer that names
    // only `[AudioVisual]` keys.
    let audio_only = read("[AudioVisual]\nDrainMoneyDisplay=yes\n");
    assert!(audio_only.drain_money_display);
    assert!(!audio_only.shrapnel_affects_ground);
}

/// `[General] Convert.ResetMindControl=` (`Ext/Rules/Body.cpp:222`) and the
/// two `[General] Explodes.*` globals (`:470-471`, constructors
/// `Ext/Rules/Body.h:885-886`): all three read from General, and the two
/// Explodes booleans construct `true`.
#[test]
fn the_general_convert_and_explodes_globals_read_from_general() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.convert_reset_mind_control);
    assert!(defaults.explodes_kill_passengers);
    assert!(defaults.explodes_during_buildup);

    let set = read(
        "[General]\nConvert.ResetMindControl=yes\n\
         Explodes.KillPassengers=no\nExplodes.DuringBuildup=no\n",
    );
    assert!(set.convert_reset_mind_control);
    assert!(!set.explodes_kill_passengers);
    assert!(!set.explodes_during_buildup);

    // The keys belong to General; a CombatDamage read does not see them.
    let wrong = read(
        "[CombatDamage]\nConvert.ResetMindControl=yes\n\
         Explodes.KillPassengers=no\nExplodes.DuringBuildup=no\n",
    );
    assert!(!wrong.convert_reset_mind_control);
    assert!(wrong.explodes_kill_passengers);
    assert!(wrong.explodes_during_buildup);
}

/// `[CombatDamage]` interceptor globals (`Ext/Rules/Body.cpp:497-499`,
/// `Body.h:912-914`): two construct `false`, `ApplyFirepowerMult` constructs
/// `true`.
#[test]
fn the_interceptor_globals_default_and_read_from_combat_damage() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.projectile_interceptable);
    assert!(!defaults.interceptor_guard_range_is_cylindrical);
    assert!(defaults.interceptor_apply_firepower_mult);

    let set = read(
        "[CombatDamage]\nProjectileInterceptable=yes\n\
         Interceptor.GuardRange.IsCylindrical=yes\n\
         Interceptor.ApplyFirepowerMult=no\n",
    );
    assert!(set.projectile_interceptable);
    assert!(set.interceptor_guard_range_is_cylindrical);
    assert!(!set.interceptor_apply_firepower_mult);

    let wrong = read(
        "[General]\nProjectileInterceptable=yes\n\
         Interceptor.GuardRange.IsCylindrical=yes\n\
         Interceptor.ApplyFirepowerMult=no\n",
    );
    assert!(!wrong.projectile_interceptable);
    assert!(wrong.interceptor_apply_firepower_mult);
}

/// `[AudioVisual] VisualScatter.Min/Max` (`Ext/Rules/Body.cpp:325-326`),
/// constructed 8/32 leptons (`Ext/Rules/Body.h:758-759`).
/// `[AudioVisual] AnimRemapDefaultColorScheme=` (`ValueableIdx<ColorScheme>`,
/// constructor 0 at `Ext/Rules/Body.h:750`, read at `Ext/Rules/Body.cpp:174`):
/// the `ColorScheme::Array` index an ownerless `AltPalette` anim draws with
/// before `0x4232E2` adds `AltPalette.ApplyLighting`.
#[test]
fn the_alt_palette_default_scheme_reads_audio_visual() {
    assert_eq!(PhobosGlobals::default().anim_remap_default_color_scheme, 0);
    let set = read("[AudioVisual]\nAnimRemapDefaultColorScheme=3\n");
    assert_eq!(set.anim_remap_default_color_scheme, 3);
    let later = read("[AudioVisual]\nAnimRemapDefaultColorScheme=3\n[AudioVisual]\n");
    assert_eq!(later.anim_remap_default_color_scheme, 3);
}

#[test]
fn the_visual_scatter_bounds_default_and_read_from_audio_visual() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.visual_scatter_min, 8);
    assert_eq!(defaults.visual_scatter_max, 32);

    let set = read("[AudioVisual]\nVisualScatter.Min=2\nVisualScatter.Max=5\n");
    assert_eq!(set.visual_scatter_min, 2);
    assert_eq!(set.visual_scatter_max, 5);

    // A later layer naming only one keeps the other at its constructor value.
    let half = read("[AudioVisual]\nVisualScatter.Max=5\n");
    assert_eq!(half.visual_scatter_min, 8);
    assert_eq!(half.visual_scatter_max, 5);
}

/// `[CombatDamage] Damage*Multiplier` (`Ext/Rules/Body.cpp:340-347`): the
/// three plain columns construct 1.0 and the five `.NotAffectsEnemies`/
/// `.Berzerk` columns construct unset (`Ext/Rules/Body.h:770-777`).
#[test]
fn the_damage_multiplier_globals_default_and_read_from_combat_damage() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.damage_owner_multiplier(), 1.0);
    assert_eq!(defaults.damage_allies_multiplier(), 1.0);
    assert_eq!(defaults.damage_enemies_multiplier(), 1.0);
    assert_eq!(defaults.damage_owner_multiplier_not_affects_enemies(), None);
    assert_eq!(
        defaults.damage_allies_multiplier_not_affects_enemies(),
        None
    );
    assert_eq!(defaults.damage_owner_multiplier_berzerk(), None);
    assert_eq!(defaults.damage_allies_multiplier_berzerk(), None);
    assert_eq!(defaults.damage_enemies_multiplier_berzerk(), None);
    assert_eq!(read(""), defaults);

    let set = read(
        "[CombatDamage]\nDamageOwnerMultiplier=0.5\nDamageAlliesMultiplier=0.25\n\
         DamageEnemiesMultiplier=2\nDamageOwnerMultiplier.NotAffectsEnemies=0.1\n\
         DamageAlliesMultiplier.NotAffectsEnemies=0.2\nDamageOwnerMultiplier.Berzerk=3\n\
         DamageAlliesMultiplier.Berzerk=4\nDamageEnemiesMultiplier.Berzerk=5\n",
    );
    assert_eq!(set.damage_owner_multiplier(), 0.5);
    assert_eq!(set.damage_allies_multiplier(), 0.25);
    assert_eq!(set.damage_enemies_multiplier(), 2.0);
    // Phobos `Parser<double>` stores `static_cast<float>(value)` widened back
    // to double (`Utilities/Parser.h:261-283`).
    assert_eq!(
        set.damage_owner_multiplier_not_affects_enemies(),
        Some(f64::from(0.1_f32))
    );
    assert_eq!(
        set.damage_allies_multiplier_not_affects_enemies(),
        Some(f64::from(0.2_f32))
    );
    assert_eq!(set.damage_owner_multiplier_berzerk(), Some(3.0));
    assert_eq!(set.damage_allies_multiplier_berzerk(), Some(4.0));
    assert_eq!(set.damage_enemies_multiplier_berzerk(), Some(5.0));

    // The keys belong to `[CombatDamage]`, not `[General]`.
    let wrong = read("[General]\nDamageEnemiesMultiplier=9\n");
    assert_eq!(wrong.damage_enemies_multiplier(), 1.0);
}

/// `[AudioVisual] RemoveMindControl.Silent=` and
/// `[CombatDamage] MergeBuildingDamage=` (`Ext/Rules/Body.cpp:597`, `:503`),
/// both constructed `no` (`Ext/Rules/Body.h:998`, `:918`).
#[test]
fn the_mind_control_silent_and_merge_building_damage_globals() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.remove_mind_control_silent);
    assert!(!defaults.merge_building_damage);
    assert_eq!(read(""), defaults);

    let set = read(
        "[AudioVisual]\nRemoveMindControl.Silent=yes\n[CombatDamage]\nMergeBuildingDamage=yes\n",
    );
    assert!(set.remove_mind_control_silent);
    assert!(set.merge_building_damage);

    // Each key belongs to its own section.
    let wrong = read(
        "[CombatDamage]\nRemoveMindControl.Silent=yes\n[AudioVisual]\nMergeBuildingDamage=yes\n",
    );
    assert!(!wrong.remove_mind_control_silent);
    assert!(!wrong.merge_building_damage);
}

/// `[AudioVisual] MindControl.Permanent.ReplaceSilent=`
/// (`Ext/Rules/Body.cpp:598`, constructor `no` `Body.h:503,999`): the fallback
/// of the warhead silent flag only where an Ares permanent capture replaces an
/// already-controlled victim (`Misc/Hooks.Ares.cpp:120-124`).
#[cfg(feature = "phobos")]
#[test]
fn the_permanent_replace_silent_defaults_false_and_reads_from_audio_visual() {
    assert!(!PhobosGlobals::default().mind_control_permanent_replace_silent);
    assert!(
        read(
            "[AudioVisual]
MindControl.Permanent.ReplaceSilent=yes
"
        )
        .mind_control_permanent_replace_silent
    );
    assert!(
        !read(
            "[AudioVisual]
MindControl.Permanent.ReplaceSilent=no
"
        )
        .mind_control_permanent_replace_silent
    );
    assert!(
        !read(
            "[CombatDamage]
MindControl.Permanent.ReplaceSilent=yes
"
        )
        .mind_control_permanent_replace_silent,
        "the key is an `[AudioVisual]` read"
    );
}

/// `[General] ShakeIsLocal=` (`Ext/Rules/Body.cpp:491`), constructed `no`
/// (`Ext/Rules/Body.h:906`): the fallback a warhead's own `ShakeIsLocal=` reads.
#[test]
fn the_shake_is_local_global_defaults_off_and_reads_from_general() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.shake_is_local);
    assert!(!read("").shake_is_local);
    assert!(read("[General]\nShakeIsLocal=yes\n").shake_is_local);
    // A same-named key in another section is not this read.
    assert!(!read("[CombatDamage]\nShakeIsLocal=yes\n").shake_is_local);
}

/// `[General] RadarJamHouses=` / `RadarJamDelay=` (`Ext/Rules/Body.cpp:213-214`,
/// constructors `Ext/Rules/Body.h:657-658`) default `Enemies` and `30`, which
/// is the Ares scan interval they replace.
#[test]
fn the_radar_jam_globals_default_and_read_from_general() {
    use crate::rules::phobos_shields::AffectedHouse;

    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.radar_jam_houses, AffectedHouse::ENEMIES);
    assert_eq!(defaults.radar_jam_delay, 30);
    assert_eq!(read(""), defaults);

    let set = read("[General]\nRadarJamHouses=all\nRadarJamDelay=7\n");
    assert_eq!(set.radar_jam_houses, AffectedHouse::ALL);
    assert_eq!(set.radar_jam_delay, 7);

    // `RadarJamHouses` is a `Valueable<AffectedHouse>` here: an unknown token
    // keeps the live value instead of clearing it.
    assert_eq!(
        read("[General]\nRadarJamHouses=bogus\n").radar_jam_houses,
        AffectedHouse::ENEMIES
    );

    // The keys belong to `[General]`, not `[AudioVisual]`.
    assert_eq!(read("[AudioVisual]\nRadarJamDelay=7\n").radar_jam_delay, 30);
}

/// `[General] NoQueueUpToEnter*` (`Ext/Rules/Body.cpp:197-200`) and
/// `[General] VeteranReload*` (`:388-389`), constructors `Ext/Rules/Body.h:640-643`
/// and `:808-809`.
#[test]
fn the_board_queue_and_veteran_reload_globals() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.no_queue_up_to_enter);
    assert_eq!(defaults.no_queue_up_to_enter_board_distance, 384);
    assert_eq!(defaults.no_queue_up_to_enter_buildings, None);
    assert_eq!(defaults.veteran_reload(), 1.0);
    assert_eq!(defaults.veteran_empty_reload(), None);
    assert_eq!(read(""), defaults);

    let set = read(
        "[General]\nNoQueueUpToEnter=yes\nNoQueueUpToEnter.BoardDistance=512\n\
         NoQueueUpToEnter.Buildings=yes\nVeteranReload=0.5\nVeteranEmptyReload=2\n",
    );
    assert!(set.no_queue_up_to_enter);
    assert_eq!(set.no_queue_up_to_enter_board_distance, 512);
    assert_eq!(set.no_queue_up_to_enter_buildings, Some(true));
    assert_eq!(set.veteran_reload(), 0.5);
    assert_eq!(set.veteran_empty_reload(), Some(2.0));
}

/// `[General] UseDisguiseMovementSpeed=` (`Ext/Rules/Body.cpp:221`) and
/// `[AudioVisual] IsVoiceCreatedGlobal=` (`:306`) default `no`, and
/// `[General] HoverDrownable=` (`:475`, constructor `Ext/Rules/Body.h:890`)
/// defaults `yes`.
#[test]
fn the_disguise_voice_and_hover_globals() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.use_disguise_movement_speed, None);
    assert!(!defaults.is_voice_created_global);
    assert!(defaults.hover_drownable, "HoverDrownable constructs yes");
    assert_eq!(read(""), defaults);

    let set = read(
        "[General]\nUseDisguiseMovementSpeed=yes\nHoverDrownable=no\n\
         [AudioVisual]\nIsVoiceCreatedGlobal=yes\n",
    );
    assert_eq!(set.use_disguise_movement_speed, Some(true));
    assert!(!set.hover_drownable);
    assert!(set.is_voice_created_global);

    // `IsVoiceCreatedGlobal` is an `[AudioVisual]` read, not a `[General]` one.
    assert!(!read("[General]\nIsVoiceCreatedGlobal=yes\n").is_voice_created_global);
}

/// `[CombatDamage] ReturnWeapon.ApplyFirepowerMult=` (`Ext/Rules/Body.cpp:559`,
/// constructor `Ext/Rules/Body.h:973`) defaults `no`, and
/// `[General] OnlyUseLandSequences=` (`:622`, `Body.h:1017`) too.
#[test]
fn the_return_weapon_and_land_sequence_globals() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.return_weapon_apply_firepower_mult);
    assert!(!defaults.only_use_land_sequences);
    assert_eq!(read(""), defaults);

    let set = read(
        "[CombatDamage]\nReturnWeapon.ApplyFirepowerMult=yes\n\
         [General]\nOnlyUseLandSequences=yes\n",
    );
    assert!(set.return_weapon_apply_firepower_mult);
    assert!(set.only_use_land_sequences);

    // Each key lives in its own section; a read from the other one is a no-op.
    assert!(
        !read("[General]\nReturnWeapon.ApplyFirepowerMult=yes\n")
            .return_weapon_apply_firepower_mult
    );
    assert!(!read("[CombatDamage]\nOnlyUseLandSequences=yes\n").only_use_land_sequences);
}

/// `[General] ArtImageSwap=`, default `no` (`Phobos.INI.cpp:262`). VERA's
/// body image already applies the ART `Image=` redirect; the flag is read for
/// parity and changes no SHP UnitType turret name (see
/// `docs/phobos-shp-turret.md`).
#[test]
fn art_image_swap_defaults_off_and_reads_from_general() {
    assert!(!PhobosGlobals::default().art_image_swap);
    assert!(read("[General]\nArtImageSwap=yes\n").art_image_swap);
    assert!(!read("[CombatDamage]\nArtImageSwap=yes\n").art_image_swap);
    assert!(!read("[General]\nArtImageSwap=no\n").art_image_swap);
}

/// Package 45 G1 `[CombatDamage]` gates (`Ext/Rules/Body.cpp:477,583-584`):
/// `Arcing.AllowElevationInaccuracy` constructs **yes** (off means apply
/// `BulletExt::ApplyArcingFix`), while both `Temporal.Apply*` globals
/// construct **no**. The three share the CombatDamage section with the
/// other Phobos combat gates, not `[General]`.
#[cfg(feature = "phobos")]
#[test]
fn the_g1_combat_damage_gates_construct_and_read_from_their_own_section() {
    let defaults = PhobosGlobals::default();
    assert!(defaults.arcing_allow_elevation_inaccuracy);
    assert!(!defaults.temporal_apply_multiplier);
    assert!(!defaults.temporal_apply_versus);
    assert_eq!(read(""), PhobosGlobals::default());

    let set = read(
        "[CombatDamage]\nArcing.AllowElevationInaccuracy=no\n\
         Temporal.ApplyMultiplier=yes\nTemporal.ApplyVersus=yes\n",
    );
    assert!(!set.arcing_allow_elevation_inaccuracy);
    assert!(set.temporal_apply_multiplier);
    assert!(set.temporal_apply_versus);

    // A `[General]` spelling belongs to no reader, so the constructors stand.
    let wrong_section = read(
        "[General]\nArcing.AllowElevationInaccuracy=no\n\
         Temporal.ApplyMultiplier=yes\nTemporal.ApplyVersus=yes\n",
    );
    assert!(wrong_section.arcing_allow_elevation_inaccuracy);
    assert!(!wrong_section.temporal_apply_multiplier);
    assert!(!wrong_section.temporal_apply_versus);
}

/// `[CombatDamage] Warhead.PreventScatter` (`Ext/Rules/Body.cpp:648`,
/// constructed false at `Body.h:1042`): the global fallback Phobos' re-hook of
/// the native scatter call site (`Misc/Hooks.Ares.cpp:278`) uses for a warhead
/// that does not author `PreventScatter=`.
#[cfg(feature = "phobos")]
#[test]
fn warhead_prevent_scatter_constructs_false_and_reads_from_combat_damage() {
    assert!(!PhobosGlobals::default().warhead_prevent_scatter);
    assert!(read("[CombatDamage]\nWarhead.PreventScatter=yes\n").warhead_prevent_scatter);
    assert!(!read("[CombatDamage]\nWarhead.PreventScatter=no\n").warhead_prevent_scatter);
    assert!(
        !read("[General]\nWarhead.PreventScatter=yes\n").warhead_prevent_scatter,
        "the key lives in `[CombatDamage]`, so a `[General]` spelling is invisible"
    );
}

/// `[General] AIGuardAreaTargetingDelay` (`Nullable<int>`, unset) and
/// `CrushSlowdownMultiplier` (`Valueable<double>`, 0.2).
#[test]
fn ai_guard_delay_and_crush_multiplier_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.ai_guard_area_targeting_delay, None);
    assert_eq!(
        f64::from_bits(defaults.crush_slowdown_multiplier.bits()),
        0.2
    );
    let set = read(
        "[General]
AIGuardAreaTargetingDelay=45
CrushSlowdownMultiplier=0.5
",
    );
    assert_eq!(set.ai_guard_area_targeting_delay, Some(45));
    assert_eq!(f64::from_bits(set.crush_slowdown_multiplier.bits()), 0.5);
}

/// `[General] Sinkable` (`Nullable<bool>`, unset) and `SinkSpeed`
/// (`Valueable<int>`, 5).
#[test]
fn sinkable_and_sink_speed_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.sinkable, None);
    assert_eq!(defaults.sink_speed, 5);
    let set = read("[General]\nSinkable=yes\nSinkSpeed=2\n");
    assert_eq!(set.sinkable, Some(true));
    assert_eq!(set.sink_speed, 2);
}

/// `[General] ParadropMission` (Guard) and `AIParadropMission` (Hunt), read
/// through the same mission-name table as map `MISSION=`; an unknown name
/// keeps the constructor value (`Ext/Rules/Body.h:934-935`).
#[test]
fn paradrop_mission_globals_default_and_read() {
    use crate::rules::mission_data::MissionType;
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.paradrop_mission, MissionType::Guard);
    assert_eq!(defaults.ai_paradrop_mission, MissionType::Hunt);
    let set = read("[General]\nParadropMission=Move\nAIParadropMission=Area Guard\n");
    assert_eq!(set.paradrop_mission, MissionType::Move);
    assert_eq!(set.ai_paradrop_mission, MissionType::AreaGuard);
    let invalid = read("[General]\nParadropMission=Wibble\n");
    assert_eq!(invalid.paradrop_mission, MissionType::Guard);
}

/// `[General] AircraftWeapon.KickOutPassengers` (`Valueable<bool>`, true).
#[test]
fn aircraft_weapon_kick_out_passengers_defaults_and_reads() {
    assert!(PhobosGlobals::default().aircraft_weapon_kick_out_passengers);
    assert!(
        !read("[General]\nAircraftWeapon.KickOutPassengers=no\n")
            .aircraft_weapon_kick_out_passengers
    );
}

/// `[General] SecondaryFireSequenceLandOnly` (`Valueable<bool>`, true).
#[test]
fn secondary_fire_sequence_land_only_defaults_and_reads() {
    assert!(
        PhobosGlobals::default().secondary_fire_sequence_land_only,
        "the global constructs yes (stock remap)"
    );
    assert!(
        !read("[General]\nSecondaryFireSequenceLandOnly=no\n").secondary_fire_sequence_land_only
    );
}

/// `[General] DamagedSpeed` (`Valueable<double>`, 0.75).
#[test]
fn damaged_speed_defaults_and_reads() {
    assert_eq!(
        PhobosGlobals::default().damaged_speed.bits(),
        0.75f64.to_bits(),
        "the constructor keeps the stock factor"
    );
    assert_eq!(
        read("[General]\nDamagedSpeed=0.5\n").damaged_speed.bits(),
        0.5f64.to_bits()
    );
}

/// `[General] NoQueueUpToUnload` (false) and its nullable Building override.
#[test]
fn no_queue_up_to_unload_globals_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.no_queue_up_to_unload);
    assert_eq!(defaults.no_queue_up_to_unload_buildings, None);
    let set = read("[General]\nNoQueueUpToUnload=yes\nNoQueueUpToUnload.Buildings=no\n");
    assert!(set.no_queue_up_to_unload);
    assert_eq!(set.no_queue_up_to_unload_buildings, Some(false));
}

/// `[General] ReloadInTransport` (`Valueable<bool>`, false).
#[test]
fn reload_in_transport_defaults_and_reads() {
    assert!(!PhobosGlobals::default().reload_in_transport);
    assert!(read("[General]\nReloadInTransport=yes\n").reload_in_transport);
}

/// `[General] HarvesterDumpAmount=`, `HarvesterScanAfterUnload=`,
/// `SubterraneanSpeed/Height=`, `VeteranRange=`, `SetRecruitableOnLiberate=`
/// (`Ext/Rules/Body.cpp:110`, `:125-126`, `:309`, `:390`, `:416`).
#[test]
fn the_harvester_subterranean_and_veteran_globals_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert_eq!(f32::from_bits(defaults.harvester_dump_amount.bits()), 0.0);
    assert!(!defaults.harvester_scan_after_unload);
    assert_eq!(defaults.subterranean_speed, 19);
    assert_eq!(defaults.subterranean_height, -256);
    assert_eq!(f64::from_bits(defaults.veteran_range.bits()), 1.0);
    assert_eq!(defaults.set_recruitable_on_liberate, -1);

    let set = read(
        "[General]\nHarvesterDumpAmount=25.5\nHarvesterScanAfterUnload=yes\n\
         SubterraneanSpeed=50\nSubterraneanHeight=-100\nVeteranRange=1.25\n\
         SetRecruitableOnLiberate=2\n",
    );
    assert_eq!(f32::from_bits(set.harvester_dump_amount.bits()), 25.5);
    assert!(set.harvester_scan_after_unload);
    // `ReadSpeed`: (50*256)/100 = 128.
    assert_eq!(set.subterranean_speed, 128);
    assert_eq!(set.subterranean_height, -100);
    assert_eq!(f64::from_bits(set.veteran_range.bits()), 1.25);
    assert_eq!(set.set_recruitable_on_liberate, 2);
}

/// A negative `SubterraneanSpeed=` keeps the ctor value
/// (`Utilities/INIParser.h:157-163`).
#[test]
fn a_negative_subterranean_speed_keeps_the_default() {
    assert_eq!(
        read("[General]\nSubterraneanSpeed=-5\n").subterranean_speed,
        19
    );
}

/// `[AudioVisual] DisplayIncome=` + `.Delay=` and `CombatLightDetailLevel=`
/// (`Ext/Rules/Body.cpp:291-297`, `:404`); a zero Delay is forced to 1.
#[test]
fn the_income_and_light_globals_read_and_clamp() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.display_income);
    assert_eq!(defaults.display_income_delay, 15);
    assert_eq!(defaults.combat_light_detail_level, 0);
    let set = read(
        "[AudioVisual]\nDisplayIncome=yes\nDisplayIncome.Delay=0\n\
         CombatLightDetailLevel=3\n",
    );
    assert!(set.display_income);
    assert_eq!(set.display_income_delay, 1);
    assert_eq!(set.combat_light_detail_level, 3);
}

/// `[CombatDamage] AllowWeaponSelectAgainstWalls=` (`Ext/Rules/Body.cpp:256`,
/// constructor false `Body.h:702`), not a `[General]` key.
#[test]
fn allow_weapon_select_against_walls_is_combat_damage() {
    assert!(!PhobosGlobals::default().allow_weapon_select_against_walls);
    assert!(
        read("[CombatDamage]\nAllowWeaponSelectAgainstWalls=yes\n")
            .allow_weapon_select_against_walls
    );
    assert!(
        !read("[General]\nAllowWeaponSelectAgainstWalls=yes\n").allow_weapon_select_against_walls
    );
}

/// `[AudioVisual] NoAlphaImageOnBuildup=` (`Ext/Rules/Body.cpp:645`,
/// constructor false `Body.h:1038`) and `[General] ParadropDelay=`
/// (`:522`, constructor 5 `Body.h:936`).
#[test]
fn the_buildup_alpha_and_paradrop_delay_globals_read() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.no_alpha_image_on_buildup);
    assert_eq!(defaults.paradrop_delay, 5);

    let set = read("[AudioVisual]\nNoAlphaImageOnBuildup=yes\n[General]\nParadropDelay=9\n");
    assert!(set.no_alpha_image_on_buildup);
    assert_eq!(set.paradrop_delay, 9);

    // Each key has one section: the sibling section must not satisfy it.
    let wrong = read("[General]\nNoAlphaImageOnBuildup=yes\n[AudioVisual]\nParadropDelay=9\n");
    assert!(!wrong.no_alpha_image_on_buildup);
    assert_eq!(wrong.paradrop_delay, 5);
}

/// `[General]` gained-self-heal caps and gates (`Ext/Rules/Body.h:30-34`).
/// `[AudioVisual]` aircraft landing-direction keys (`Ext/Rules/Body.h:533-535`).
#[test]
fn aircraft_landing_direction_globals_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert!(defaults.aircraft_docking_dir_default_to_pose_dir);
    assert_eq!(defaults.pose_dir_production, None);
    assert_eq!(defaults.pose_dir_field, None);
    let set = read(
        "[AudioVisual]\nAircraftDockingDir.DefaultToPoseDir=no\n\
         PoseDir.Production=64\nPoseDir.Field=200\n",
    );
    assert!(!set.aircraft_docking_dir_default_to_pose_dir);
    assert_eq!(set.pose_dir_production, Some(64));
    assert_eq!(set.pose_dir_field, Some(200));
}

#[test]
fn self_heal_caps_and_gates_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.infantry_gain_self_heal_cap, None);
    assert_eq!(defaults.units_gain_self_heal_cap, None);
    assert!(defaults.gain_self_heal_allow_multiplay_passive);
    assert!(!defaults.gain_self_heal_from_player_control);
    assert!(!defaults.gain_self_heal_from_allies);
    let set = read(
        "[General]\nInfantryGainSelfHealCap=3\nUnitsGainSelfHealCap=2\n\
         GainSelfHealAllowMultiplayPassive=no\nGainSelfHealFromPlayerControl=yes\n\
         GainSelfHealFromAllies=yes\n",
    );
    assert_eq!(set.infantry_gain_self_heal_cap, Some(3));
    assert_eq!(set.units_gain_self_heal_cap, Some(2));
    assert!(!set.gain_self_heal_allow_multiplay_passive);
    assert!(set.gain_self_heal_from_player_control);
    assert!(set.gain_self_heal_from_allies);
}

/// `[General] BunkerStateUpdateDelay=` (`Ext/Rules/Body.cpp:617`,
/// constructor **15** `Body.h:1012`).
#[test]
fn the_bunker_state_update_delay_defaults_to_fifteen() {
    assert_eq!(PhobosGlobals::default().bunker_state_update_delay, 15);
    assert_eq!(
        read("[General]\nBunkerStateUpdateDelay=33\n").bunker_state_update_delay,
        33
    );
    // `[General]` only.
    assert_eq!(
        read("[AudioVisual]\nBunkerStateUpdateDelay=33\n").bunker_state_update_delay,
        15
    );
}

/// `[General] ChronoSpherePreDelay=` (`Ext/Rules/Body.cpp:120`,
/// constructor **60** `Body.h:579`).
#[test]
fn the_chrono_sphere_pre_delay_defaults_to_sixty() {
    assert_eq!(PhobosGlobals::default().chrono_sphere_pre_delay, 60);
    assert_eq!(
        read("[General]\nChronoSpherePreDelay=45\n").chrono_sphere_pre_delay,
        45
    );
    assert_eq!(
        read("[AudioVisual]\nChronoSpherePreDelay=45\n").chrono_sphere_pre_delay,
        60
    );
}

/// `[AudioVisual] UseFixedVoxelLighting=` (`Ext/Rules/Body.cpp:368`,
/// constructor **no** `Body.h:273,793`): the voxel initializer's secondary
/// light vector becomes `{0,0,0}` (hook `0x753D86`) instead of `{0,0,1}`.
#[cfg(feature = "phobos")]
#[test]
fn the_fixed_voxel_lighting_defaults_false_and_reads_from_audio_visual() {
    assert!(!PhobosGlobals::default().use_fixed_voxel_lighting);
    assert!(
        read(
            "[AudioVisual]
UseFixedVoxelLighting=yes
"
        )
        .use_fixed_voxel_lighting
    );
    assert!(
        !read(
            "[AudioVisual]
UseFixedVoxelLighting=no
"
        )
        .use_fixed_voxel_lighting
    );
    assert!(
        !read(
            "[General]
UseFixedVoxelLighting=yes
"
        )
        .use_fixed_voxel_lighting,
        "the key is an `[AudioVisual]` read"
    );
}

/// `[JumpjetControls] RotateOnCrash=` (`Ext/Rules/Body.cpp:1355`,
/// constructor **true** `Body.h:596`).
#[test]
fn the_jumpjet_rotate_on_crash_defaults_true() {
    assert!(PhobosGlobals::default().jumpjet_rotate_on_crash);
    assert!(!read("[JumpjetControls]\nRotateOnCrash=no\n").jumpjet_rotate_on_crash);
    // `[JumpjetControls]` only.
    assert!(read("[General]\nRotateOnCrash=no\n").jumpjet_rotate_on_crash);
}

/// `[JumpjetControls] Crash=` (`Ext/Rules/Body.cpp:1353`, constructor **5.0**
/// `Body.h:594`) and `NoWobbles=` (`:1354`, constructor **false**
/// `Body.h:595`): the two Phobos-only seeds of the techno constructor hook.
#[test]
fn the_jumpjet_crash_and_no_wobbles_default_and_read() {
    let defaults = PhobosGlobals::default();
    assert_eq!(f64::from_bits(defaults.jumpjet_crash.bits()), 5.0);
    assert!(!defaults.jumpjet_no_wobbles);

    let set = read("[JumpjetControls]\nCrash=9.5\nNoWobbles=yes\n");
    assert_eq!(f64::from_bits(set.jumpjet_crash.bits()), 9.5);
    assert!(set.jumpjet_no_wobbles);

    // `[JumpjetControls]` only: a `[General]` or `[CombatDamage]` spelling is
    // invisible.
    let wrong = read("[General]\nCrash=9.5\nNoWobbles=yes\n[CombatDamage]\nCrash=9.5\n");
    assert_eq!(wrong, defaults);
}

/// `[General] JumpjetClimbIgnoreBuilding=` (`Ext/Rules/Body.cpp:338`,
/// constructor **false** `Body.h:768`).
#[test]
fn the_jumpjet_climb_ignore_building_global_defaults_false() {
    assert!(!PhobosGlobals::default().jumpjet_climb_ignore_building);
    assert!(read("[General]\nJumpjetClimbIgnoreBuilding=yes\n").jumpjet_climb_ignore_building);
    // `[JumpjetControls]` does not own this key.
    assert!(
        !read("[JumpjetControls]\nJumpjetClimbIgnoreBuilding=yes\n").jumpjet_climb_ignore_building
    );
}

/// `[General] AircraftFiringForceScatter=` (`Ext/Rules/Body.cpp:473`,
/// constructor **true** `Body.h:888`).
#[test]
fn the_aircraft_firing_force_scatter_global_defaults_true() {
    assert!(PhobosGlobals::default().aircraft_firing_force_scatter);
    assert!(!read("[General]\nAircraftFiringForceScatter=no\n").aircraft_firing_force_scatter);
}

/// `[General] SkipCrushSlowdown=` (`Ext/Rules/Body.cpp:274`, constructor
/// **false** `Body.h:716`) and its `[General]`-only placement.
#[test]
fn the_skip_crush_slowdown_global_defaults_false_and_reads_general() {
    assert!(!PhobosGlobals::default().skip_crush_slowdown);
    assert!(read("[General]\nSkipCrushSlowdown=yes\n").skip_crush_slowdown);
    assert!(!read("[AudioVisual]\nSkipCrushSlowdown=yes\n").skip_crush_slowdown);
}

/// `[General] AircraftSpawnFromEdge=` (`Ext/Rules/Body.cpp:193`, constructor
/// `Owner` `Body.h:636`): the fallback for an AircraftType's own nullable.
#[test]
fn the_aircraft_spawn_from_edge_global_defaults_to_owner_and_reads_the_names() {
    use crate::rules::phobos_aircraft_edge::PhobosEdgeType;

    assert_eq!(
        PhobosGlobals::default().aircraft_spawn_from_edge,
        PhobosEdgeType::Owner
    );
    let set = read("[General]\nAircraftSpawnFromEdge=random\n");
    assert_eq!(set.aircraft_spawn_from_edge, PhobosEdgeType::Random);
    let closest = read("[General]\nAircraftSpawnFromEdge=closest\n");
    assert_eq!(closest.aircraft_spawn_from_edge, PhobosEdgeType::Closest);
    // An unknown name keeps the constructor value.
    let bad = read("[General]\nAircraftSpawnFromEdge=nearest\n");
    assert_eq!(bad.aircraft_spawn_from_edge, PhobosEdgeType::Owner);
}

/// `ToolTip.Background.*` (`[AudioVisual]`, `Ext/Rules/Body.cpp:169-171`,
/// constructor `Body.h:722-724`): black at 100 opacity and zero blur, each
/// key replacing its own value.
#[test]
fn the_tooltip_background_globals_default_and_read() {
    let defaults = read("");
    assert_eq!(defaults.tooltip_background_color, [0, 0, 0]);
    assert_eq!(defaults.tooltip_background_opacity, 100);
    assert_eq!(
        defaults.tooltip_background_blur_size.bits(),
        0.0f32.to_bits()
    );

    let set = read(
        "[AudioVisual]\nToolTip.Background.Color=10,20,30\n\
         ToolTip.Background.Opacity=40\nToolTip.Background.BlurSize=2.5\n",
    );
    assert_eq!(set.tooltip_background_color, [10, 20, 30]);
    assert_eq!(set.tooltip_background_opacity, 40);
    assert_eq!(set.tooltip_background_blur_size.bits(), 2.5f32.to_bits());

    // Phobos parsers retain the current value on junk: the colour is a
    // three-byte read whose every component must fit, the int rejects text,
    // the float uses strtod.
    let junk = read(
        "[AudioVisual]
ToolTip.Background.Color=300,0,0
         ToolTip.Background.Opacity=abc
ToolTip.Background.BlurSize=junk
",
    );
    assert_eq!(junk.tooltip_background_color, [0, 0, 0]);
    assert_eq!(junk.tooltip_background_opacity, 100);
    assert_eq!(junk.tooltip_background_blur_size.bits(), 0.0f32.to_bits());

    // Above 100 is stored; `FillRectTrans` clamps at draw time.
    let over = read(
        "[AudioVisual]
ToolTip.Background.Opacity=101
",
    );
    assert_eq!(over.tooltip_background_opacity, 101);
}

/// `[AudioVisual] Pips.{Generic,Ammo}[.Buildings].Size` are
/// `Valueable<Point2D>` pairs (`Ext/Rules/Body.cpp:160..163`) whose
/// constructor defaults reproduce the stock `4 / (building ? 2 : 0)` advance
/// (`Body.h:620..623`).
#[test]
fn the_pip_size_globals_default_and_read_from_audio_visual() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.pips_generic_size, [4, 0]);
    assert_eq!(defaults.pips_generic_buildings_size, [4, 2]);
    assert_eq!(defaults.pips_ammo_size, [4, 0]);
    assert_eq!(defaults.pips_ammo_buildings_size, [4, 2]);

    let set = read(
        "[AudioVisual]\nPips.Generic.Size=7,3\nPips.Generic.Buildings.Size=8,4\n\
         Pips.Ammo.Size=9,5\nPips.Ammo.Buildings.Size=10,6\n",
    );
    assert_eq!(set.pips_generic_size, [7, 3]);
    assert_eq!(set.pips_generic_buildings_size, [8, 4]);
    assert_eq!(set.pips_ammo_size, [9, 5]);
    assert_eq!(set.pips_ammo_buildings_size, [10, 6]);

    // `Parser<int, 2>`: a short read fills only the leading slot, and a value
    // that converts nothing keeps the whole previous pair.
    let partial = read("[AudioVisual]\nPips.Generic.Size=7\nPips.Ammo.Size=bad\n");
    assert_eq!(partial.pips_generic_size, [7, 0]);
    assert_eq!(partial.pips_ammo_size, [4, 0]);
}

/// `[AudioVisual] DefaultLandingAnim[.Dropship/.Carryall]`
/// (`Ext/Rules/Body.cpp:602-604`, constructors `Body.h:1003-1005`): the plain
/// key is a `Valueable` anim pointer whose `<default>` keeps the previous
/// name, while the two variants are `Nullable` and clear on it or `none`.
#[test]
fn the_landing_anim_globals_default_and_read_from_audio_visual() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.default_landing_anim, None);
    assert_eq!(defaults.default_landing_anim_dropship, None);
    assert_eq!(defaults.default_landing_anim_carryall, None);

    let set = read(
        "[AudioVisual]\nDefaultLandingAnim=LAND\n\
         DefaultLandingAnim.Dropship=DSHIP\nDefaultLandingAnim.Carryall=CARY\n",
    );
    assert_eq!(set.default_landing_anim.as_deref(), Some("LAND"));
    assert_eq!(set.default_landing_anim_dropship.as_deref(), Some("DSHIP"));
    assert_eq!(set.default_landing_anim_carryall.as_deref(), Some("CARY"));

    let cleared = read(
        "[AudioVisual]\nDefaultLandingAnim=<default>\n\
         DefaultLandingAnim.Dropship=<default>\nDefaultLandingAnim.Carryall=none\n",
    );
    assert_eq!(
        cleared.default_landing_anim, None,
        "<default> leaves the null constructor value"
    );
    assert_eq!(cleared.default_landing_anim_dropship, None);
    assert_eq!(cleared.default_landing_anim_carryall, None);

    // A later rules pass supplies the keep-previous case: the `Valueable`
    // pointer retains an earlier name when `<default>` fails its lookup.
    let layered = read("[AudioVisual]\nDefaultLandingAnim=LAND\nDefaultLandingAnim=<default>\n");
    assert_eq!(layered.default_landing_anim.as_deref(), Some("LAND"));

    // Names are stored unresolved; the landing consumer rejects a name the
    // anim catalog lacks, as the native pointer read would.
    let unknown = read("[AudioVisual]\nDefaultLandingAnim=TYPO\n");
    assert_eq!(unknown.default_landing_anim.as_deref(), Some("TYPO"));
}

/// `[AudioVisual] Pips.Tiberiums.*` (`Ext/Rules/Body.cpp:164-168`,
/// constructors `Body.h:624-628`): the per-class frame list, empty frame,
/// draw order and both weeder frames.
#[test]
fn the_tiberium_pip_globals_default_and_read_from_audio_visual() {
    let defaults = PhobosGlobals::default();
    assert!(defaults.pips_tiberiums_frames.is_empty());
    assert_eq!(defaults.pips_tiberiums_empty_frame, 0);
    assert!(defaults.pips_tiberiums_display_order.is_empty());
    assert_eq!(defaults.pips_tiberiums_weed_frame, 1);
    assert_eq!(defaults.pips_tiberiums_weed_empty_frame, 0);

    let set = read(
        "[AudioVisual]\nPips.Tiberiums.Frames=3,4\nPips.Tiberiums.EmptyFrame=9\n\
         Pips.Tiberiums.DisplayOrder=1,0\nPips.Tiberiums.WeedFrame=7\n\
         Pips.Tiberiums.WeedEmptyFrame=8\n",
    );
    assert_eq!(set.pips_tiberiums_frames, [3, 4]);
    assert_eq!(set.pips_tiberiums_empty_frame, 9);
    assert_eq!(set.pips_tiberiums_display_order, [1, 0]);
    assert_eq!(set.pips_tiberiums_weed_frame, 7);
    assert_eq!(set.pips_tiberiums_weed_empty_frame, 8);
}

/// `[General] KeepAlive.{Infantry,Units,Aircraft,Buildings,Defenses}`
/// (`Ext/Rules/Body.cpp:652-656`, constructors `Body.h:1046-1050`): the
/// Ares house keep-alive category defaults, buildings and defenses `yes`.
#[test]
fn the_keep_alive_globals_default_and_read_from_general() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.keep_alive_infantry);
    assert!(!defaults.keep_alive_units);
    assert!(!defaults.keep_alive_aircraft);
    assert!(defaults.keep_alive_buildings);
    assert!(defaults.keep_alive_defenses);

    let set = read(
        "[General]\nKeepAlive.Infantry=yes\nKeepAlive.Units=yes\n\
         KeepAlive.Aircraft=yes\nKeepAlive.Buildings=no\nKeepAlive.Defenses=no\n",
    );
    assert!(set.keep_alive_infantry);
    assert!(set.keep_alive_units);
    assert!(set.keep_alive_aircraft);
    assert!(!set.keep_alive_buildings);
    assert!(!set.keep_alive_defenses);
}

/// `[AudioVisual] Pips.SelfHeal.*` (`Ext/Rules/Body.cpp:154-159`,
/// constructors `Body.h:614-619`): three frame pairs and three native draw
/// offsets.
#[test]
fn the_self_heal_pip_globals_default_and_read_from_audio_visual() {
    let defaults = PhobosGlobals::default();
    assert_eq!(defaults.pips_self_heal_infantry, [13, 20]);
    assert_eq!(defaults.pips_self_heal_units, [13, 20]);
    assert_eq!(defaults.pips_self_heal_buildings, [13, 20]);
    assert_eq!(defaults.pips_self_heal_infantry_offset, [25, -35]);
    assert_eq!(defaults.pips_self_heal_units_offset, [33, -32]);
    assert_eq!(defaults.pips_self_heal_buildings_offset, [15, 10]);

    let set = read(
        "[AudioVisual]\nPips.SelfHeal.Infantry=1,2\nPips.SelfHeal.Units=3,4\n\
         Pips.SelfHeal.Buildings=5,6\nPips.SelfHeal.Infantry.Offset=7,8\n\
         Pips.SelfHeal.Units.Offset=9,10\nPips.SelfHeal.Buildings.Offset=11,12\n",
    );
    assert_eq!(set.pips_self_heal_infantry, [1, 2]);
    assert_eq!(set.pips_self_heal_units, [3, 4]);
    assert_eq!(set.pips_self_heal_buildings, [5, 6]);
    assert_eq!(set.pips_self_heal_infantry_offset, [7, 8]);
    assert_eq!(set.pips_self_heal_units_offset, [9, 10]);
    assert_eq!(set.pips_self_heal_buildings_offset, [11, 12]);
}

/// `[AudioVisual] ConditionYellow.Terrain=` (`Ext/Rules/Body.cpp:147`,
/// constructed unset `Body.h:70`): the terrain damaged-frame threshold.
#[test]
fn the_terrain_condition_yellow_defaults_and_reads() {
    assert_eq!(PhobosGlobals::default().condition_yellow_terrain, None);
    // 0.1 is not exact in binary: the Phobos Parser<double> stores a
    // binary32 first, so the widened value differs from an exact double.
    let set = read("[AudioVisual]\nConditionYellow.Terrain=0.1\n");
    assert_eq!(
        set.condition_yellow_terrain
            .map(|bits| f64::from_bits(bits.bits())),
        Some(f64::from(0.1f32))
    );
    let set = read("[AudioVisual]\nConditionYellow.Terrain=50%\n");
    assert_eq!(
        set.condition_yellow_terrain
            .map(|bits| f64::from_bits(bits.bits())),
        Some(0.5)
    );
    for token in ["<default>", "none", "<none>"] {
        let cleared = read(&format!("[AudioVisual]\nConditionYellow.Terrain={token}\n"));
        assert_eq!(cleared.condition_yellow_terrain, None, "{token}");
    }
}

/// `[CombatDamage] AdjacentWallDamage=` (`Ext/Rules/Body.cpp:443`,
/// constructor 200 `Body.h:858`).
#[test]
fn the_adjacent_wall_damage_defaults_and_reads() {
    assert_eq!(PhobosGlobals::default().adjacent_wall_damage, 200);
    assert_eq!(
        read("[CombatDamage]\nAdjacentWallDamage=0\n").adjacent_wall_damage,
        0
    );
    assert_eq!(
        read("[CombatDamage]\nAdjacentWallDamage=350\n").adjacent_wall_damage,
        350
    );
}

/// `[General] AIAirTargetingFix=` (`Ext/Rules/Body.cpp:452`, constructor
/// `false` `Body.h:867`) and `[General] DisableOveroptimizationInTargeting=`
/// (`Ext/Rules/Body.cpp:531`, constructor `false` `Body.h:951`): the two
/// targeting switches, both `[General]` reads.
#[test]
fn the_targeting_switches_default_false_and_read_from_general() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.ai_air_targeting_fix);
    assert!(!defaults.disable_overoptimization_in_targeting);
    let set = read(
        "[General]
AIAirTargetingFix=yes
DisableOveroptimizationInTargeting=yes
",
    );
    assert!(set.ai_air_targeting_fix);
    assert!(set.disable_overoptimization_in_targeting);
    let other = read(
        "[AI]
AIAirTargetingFix=yes
DisableOveroptimizationInTargeting=yes
",
    );
    assert!(!other.ai_air_targeting_fix);
    assert!(!other.disable_overoptimization_in_targeting);
}

/// `[CrateRules] CrateOnlyOnLand=` and `UnitCrateVehicleCap=`
/// (`Ext/Rules/Body.cpp:281-283`, constructors `Body.h:733-735`).
#[test]
fn the_crate_globals_default_and_read_from_crate_rules() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.crate_only_on_land);
    assert_eq!(defaults.unit_crate_vehicle_cap, 50);
    assert_eq!(defaults.free_mcv_credits_threshold, 1500);
    let set = read(
        "[CrateRules]\nCrateOnlyOnLand=yes\nUnitCrateVehicleCap=-1\n\
         FreeMCV.CreditsThreshold=6000\n",
    );
    assert!(set.crate_only_on_land);
    assert_eq!(set.unit_crate_vehicle_cap, -1);
    assert_eq!(set.free_mcv_credits_threshold, 6000);
}

/// `[General] Temporal.KillPoweredAnim=` (`Ext/Rules/Body.cpp:585`,
/// constructor `true` `Body.h:993`).
#[test]
fn the_temporal_kill_powered_anim_defaults_and_reads() {
    assert!(PhobosGlobals::default().temporal_kill_powered_anim);
    assert!(!read("[General]\nTemporal.KillPoweredAnim=no\n").temporal_kill_powered_anim);
}

/// `[General] Storage.TiberiumIndex=` (`Ext/Rules/Body.cpp:109`,
/// constructor `-1` `Body.h:568`).
#[test]
fn the_storage_tiberium_index_defaults_and_reads() {
    assert_eq!(PhobosGlobals::default().storage_tiberium_index, -1);
    assert_eq!(
        read("[General]\nStorage.TiberiumIndex=1\n").storage_tiberium_index,
        1
    );
}

/// `[CombatDamage] IvanBombAttachToCenter=` (`Ext/Rules/Body.cpp:447`,
/// constructor `false` `Body.h:862`).
#[test]
fn the_ivan_bomb_attach_to_center_defaults_and_reads() {
    assert!(!PhobosGlobals::default().ivan_bomb_attach_to_center);
    assert!(read("[CombatDamage]\nIvanBombAttachToCenter=yes\n").ivan_bomb_attach_to_center);
}

/// The rest of batch 56a's readers (`Ext/Rules/Body.cpp:323,370-375,449,451,
/// 540,606`): defaults from the constructor, `[AI]` for the placement
/// switches, `[CombatDamage]` for the missile spawn cell, and the enum's
/// name table.
#[test]
fn the_ai_placement_and_bugfix_keys_read_from_their_own_sections() {
    use crate::rules::ini_value::PhobosDynamicTeamDelayType;

    let defaults = PhobosGlobals::default();
    assert!(defaults.ai_auto_deploy_mcv);
    assert!(defaults.ai_set_base_center);
    assert!(!defaults.ai_bias_spawn_cell);
    assert!(!defaults.ai_forbid_conyard);
    assert!(!defaults.ai_node_walls_only);
    assert!(!defaults.ai_clean_wall_node);
    assert!(!defaults.building_waypoints);
    assert!(defaults.missile_spawn_attack_cell);
    assert!(!defaults.falling_down_targeting_fix);
    assert!(!defaults.balloon_hover_pathing_fix);
    assert_eq!(
        defaults.team_delays_dynamic_type,
        PhobosDynamicTeamDelayType::StartingPoint
    );

    let set = read(
        "[AI]
AIAutoDeployMCV=no
AISetBaseCenter=no
AIBiasSpawnCell=yes
AIForbidConYard=yes
AINodeWallsOnly=yes
AICleanWallNode=yes
[General]
BuildingWaypoints=yes
FallingDownTargetingFix=yes
BalloonHoverPathingFix=yes
TeamDelays.DynamicType=AliveEnemies
[CombatDamage]
MissileSpawnAttackCell=no
",
    );
    assert!(!set.ai_auto_deploy_mcv);
    assert!(!set.ai_set_base_center);
    assert!(set.ai_bias_spawn_cell);
    assert!(set.ai_forbid_conyard);
    assert!(set.ai_node_walls_only);
    assert!(set.ai_clean_wall_node);
    assert!(set.building_waypoints);
    assert!(!set.missile_spawn_attack_cell);
    assert!(set.falling_down_targeting_fix);
    assert!(set.balloon_hover_pathing_fix);
    assert_eq!(
        set.team_delays_dynamic_type,
        PhobosDynamicTeamDelayType::AliveEnemies
    );

    // Each key is read from its one section only.
    let wrong = read(
        "[General]
AIAutoDeployMCV=no
AISetBaseCenter=no
AIBiasSpawnCell=yes
AIForbidConYard=yes
AINodeWallsOnly=yes
AICleanWallNode=yes
MissileSpawnAttackCell=no
[AI]
BuildingWaypoints=yes
TeamDelays.DynamicType=AliveEnemies
[CombatDamage]
FallingDownTargetingFix=yes
",
    );
    assert!(wrong.ai_auto_deploy_mcv);
    assert!(wrong.ai_set_base_center);
    assert!(!wrong.ai_bias_spawn_cell);
    assert!(!wrong.ai_forbid_conyard);
    assert!(!wrong.ai_node_walls_only);
    assert!(!wrong.ai_clean_wall_node);
    assert!(wrong.missile_spawn_attack_cell);
    assert!(!wrong.building_waypoints);
    assert!(!wrong.falling_down_targeting_fix);
    assert_eq!(
        wrong.team_delays_dynamic_type,
        PhobosDynamicTeamDelayType::StartingPoint
    );
}

/// `[General]`/`[AI]`/`[CombatDamage]` keys of the remaining Phobos globals
/// (`Ext/Rules/Body.cpp:118-119,136,243-246,299,307,336-337,401,549,619-625,
/// 646,648,658`), with their constructor defaults.
#[test]
fn the_remaining_general_ai_and_combatdamage_globals() {
    let defaults = PhobosGlobals::default();
    assert!(!defaults.sort_cameo_by_name);
    assert!(!defaults.set_tab_by_selecting_factory);
    assert!(!defaults.allow_chat_box_in_single_player);
    assert!(!defaults.allow_beacon_hotkey_in_single_player);
    assert!(!defaults.auto_remove_earliest_beacon);
    assert!(!defaults.allow_deploy_controlled_mcv);
    assert!(!defaults.fix_repair_step_cost);
    assert!(!defaults.extended_player_repair);
    assert!(!defaults.enable_power_surplus);
    assert_eq!(defaults.power_surplus, 50, "stock [AI] PowerSurplus");
    assert_eq!(defaults.power_surplus_scale_to_drain_amount, 0);
    assert!(defaults.display_income_allow_ai, "AllowAI constructs yes");
    assert!(
        defaults.auto_target_insignificant_when_mind_controlled,
        "InsignificantWhenMindControlled constructs yes"
    );
    assert!(!defaults.ready_to_next_mission_moving_check);
    assert_eq!(defaults.chrono_sparkle_display_delay, 24);
    assert_eq!(
        defaults.chrono_sparkle_building_display_positions.bits(),
        0x4,
        "OccupantSlots"
    );
    assert!(defaults.veinhole_warhead.is_none());

    let set = read(
        "[General]
SortCameoByName=yes
SetTabBySelectingFactory=yes
AllowChatBoxInSinglePlayer=yes
AllowBeaconHotKeyInSinglePlayer=yes
AutoRemoveEarliestBeacon=yes
AllowDeployControlledMCV=yes
FixRepairStepCost=yes
ExtendedPlayerRepair=yes
JumpjetClimbPredictHeight=yes
JumpjetClimbWithoutCutOut=yes
ReadyToNextMission.MovingCheck=yes
ChronoSparkleDisplayDelay=7
ChronoSparkleBuildingDisplayPositions=Building, Occupants
[AI]
EnablePowerSurplus=yes
PowerSurplus=40
PowerSurplus.ScaleToDrainAmount=40
[AudioVisual]
DisplayIncome.AllowAI=no
[CombatDamage]
AutoTarget.InsignificantWhenMindControlled=no
VeinholeWarhead=WH_VEIN
",
    );
    assert!(set.sort_cameo_by_name);
    assert!(set.set_tab_by_selecting_factory);
    assert!(set.allow_chat_box_in_single_player);
    assert!(set.allow_beacon_hotkey_in_single_player);
    assert!(set.auto_remove_earliest_beacon);
    assert!(set.allow_deploy_controlled_mcv);
    assert!(set.fix_repair_step_cost);
    assert!(set.extended_player_repair);
    assert!(set.jumpjet_climb_predict_height);
    assert!(set.jumpjet_climb_without_cut_out);
    assert!(set.ready_to_next_mission_moving_check);
    assert_eq!(set.chrono_sparkle_display_delay, 7);
    assert_eq!(
        set.chrono_sparkle_building_display_positions.bits(),
        0x1 | 0x2
    );
    assert!(set.enable_power_surplus);
    assert_eq!(set.power_surplus, 40);
    assert_eq!(set.power_surplus_scale_to_drain_amount, 40);
    assert!(!set.display_income_allow_ai);
    assert!(!set.auto_target_insignificant_when_mind_controlled);
    assert_eq!(set.veinhole_warhead.as_deref(), Some("WH_VEIN"));

    // Every key above belongs to exactly one section.
    let wrong = read(
        "[AudioVisual]
SortCameoByName=yes
EnablePowerSurplus=yes
ChronoSparkleDisplayDelay=7
[General]
DisplayIncome.AllowAI=no
PowerSurplus.ScaleToDrainAmount=40
[CombatDamage]
ChronoSparkleBuildingDisplayPositions=All
",
    );
    assert!(!wrong.sort_cameo_by_name);
    assert!(!wrong.enable_power_surplus);
    assert_eq!(wrong.chrono_sparkle_display_delay, 24);
    assert!(wrong.display_income_allow_ai);
    assert_eq!(wrong.power_surplus_scale_to_drain_amount, 0);
    assert_eq!(
        wrong.chrono_sparkle_building_display_positions.bits(),
        0x4,
        "not read from CombatDamage"
    );
}

/// `[AudioVisual]` light multipliers, radial-indicator visibility,
/// `UseRetintFix`, `ColorAddUse8BitRGB` and the nullable `VoxelLightSource`
/// (`Ext/Rules/Body.cpp:172,260,349-352,409`).
#[test]
fn the_audiovisual_light_and_color_globals() {
    let defaults = PhobosGlobals::default();
    assert_eq!(
        defaults.aircraft_level_light_multiplier.bits(),
        1.0f64.to_bits()
    );
    assert_eq!(
        defaults.jumpjet_level_light_multiplier.bits(),
        0.0f64.to_bits()
    );
    assert_eq!(
        defaults.radial_indicator_visibility,
        crate::rules::phobos_shields::AffectedHouse::ALLIES
    );
    assert!(defaults.use_retint_fix, "UseRetintFix constructs yes");
    assert!(!defaults.color_add_use_8bit_rgb);
    assert!(defaults.voxel_light_source.is_none());

    let set = read(
        "[AudioVisual]
AircraftLevelLightMultiplier=0.5
JumpjetLevelLightMultiplier=2.0
RadialIndicatorVisibility=owner
UseRetintFix=no
ColorAddUse8BitRGB=yes
VoxelLightSource=1.0,2.0,3.0
",
    );
    assert_eq!(set.aircraft_level_light_multiplier.bits(), 0.5f64.to_bits());
    assert_eq!(set.jumpjet_level_light_multiplier.bits(), 2.0f64.to_bits());
    assert_eq!(
        set.radial_indicator_visibility,
        crate::rules::phobos_shields::AffectedHouse::OWNER
    );
    assert!(!set.use_retint_fix);
    assert!(set.color_add_use_8bit_rgb);
    let light = set.voxel_light_source.expect("three tokens set it");
    assert_eq!(
        light.map(|value| f32::from_bits(value.bits())),
        [1.0, 2.0, 3.0]
    );

    // `<none>` clears a previously set value; a short vector keeps it.
    let cleared = read(
        "[AudioVisual]
VoxelLightSource=1.0,2.0,3.0
",
    );
    assert!(cleared.voxel_light_source.is_some());
    let malformed = read(
        "[AudioVisual]
VoxelLightSource=1.0
",
    );
    assert!(malformed.voxel_light_source.is_none());
}
