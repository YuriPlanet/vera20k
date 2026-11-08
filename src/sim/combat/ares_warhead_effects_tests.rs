//! Ares 3.0 WarheadTypeExt effects part 2 through the production detonation,
//! receiver, curtain and fire-error owners: IronCurtain.Duration/Cap with the
//! TechnoType curtain modifiers, MindControl.Permanent,
//! Sonar.Duration, DisableWeapons.Duration and InfDeathAnim.

use super::fire_error::FireError;
use super::world_receiver::{ReceiverRun, commit_projectiles};
use super::{EntityDamageEvent, ReceiverCallFlags, TargetKind};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::{FrameEffects, Simulation};

/// A firer and a Strength-1000 `TARGET` (with `target_ini`), the weapon `GUN`
/// (Damage 30, warhead `WH` with `wh_ini`), the curtain-stopping warhead
/// `STOP` and `extra` sections.
fn rules(wh_ini: &str, target_ini: &str, extra: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=30\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n1=STOP\n\
         [STOP]\nCellSpread=1\nIronCurtain.Duration=-100\n\
         [WH]\nCellSpread=1\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%\n{wh_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [TARGET]\nStrength=1000\nArmor=none\nSpeed=6\nPrimary=GUN\n{target_ini}\n{extra}"
    )))
    .expect("warhead effect rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x1C_AA);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 23, 20, 0, rules)
        .expect("target");
    (sim, firer, target)
}

/// One bullet of `GUN` (payload `damage`) from `firer` detonating on `target`.
fn shoot(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64, damage: i32) {
    let coords = object_get_coords(sim.entities().get(target).unwrap(), sim.resolved_terrain());
    let payload = ProjectilePayload::new(damage, sim.interner.intern("WH"), None, false)
        .with_installed_weapon(Some(sim.interner.intern("GUN")));
    let detonation = ProjectileDetonation {
        projectile_id: 9_999,
        source_id: firer,
        target: ProjectileTarget::Entity(target),
        impact: ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload,
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = ReceiverRun::default();
    let _ = commit_projectiles(sim, &mut run, &[detonation], rules, None);
    run.finish();
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.entities().get(id).unwrap().health.current()
}

fn curtain_left(sim: &Simulation, id: u64) -> i32 {
    let frame = sim.session.binary_frame() as i32;
    sim.entities()
        .get(id)
        .unwrap()
        .invulnerability
        .as_ref()
        .map_or(0, |state| state.timer().remaining(frame))
}

/// WarheadTypeExt::applyIronCurtain (10052890) from BulletClass_Detonate
/// (100536E0): off a curtain the weapon's Damage lands first, ignoring the
/// 50% Verses; the duration scales by `IronCurtain.Modifier` once although
/// the Techno curtain (hook 70E2B0) scales again; on a curtain a
/// non-stacking refresh deals nothing; a negative duration stops it.
#[test]
fn iron_curtain_warhead_damages_then_curtains_once_scaled() {
    let rules = rules(
        "IronCurtain.Duration=100\n",
        "IronCurtain.Modifier=50%\n",
        "",
    );
    assert_eq!(
        rules
            .object("TARGET")
            .unwrap()
            .ares_curtain_modifiers()
            .map(|bits| f64::from_bits(bits.bits())),
        [0.5, 1.0]
    );
    let (mut sim, firer, target) = scene(&rules);
    shoot(&mut sim, &rules, firer, target, 0);
    assert_eq!(health(&sim, target), 970);
    assert_eq!(curtain_left(&sim, target), 50);

    shoot(&mut sim, &rules, firer, target, 0);
    assert_eq!(health(&sim, target), 970, "a refresh deals no damage");
    assert_eq!(curtain_left(&sim, target), 50, "Cap -1 does not stack");

    let stop = rules.warhead("STOP").unwrap();
    let source_house = sim.entities().get(firer).map(|firer| firer.owner());
    let coords = object_get_coords(sim.entities().get(target).unwrap(), sim.resolved_terrain());
    let mut run = ReceiverRun::default();
    let mut effects = super::DeathEffects::default();
    sim.apply_ares_iron_curtain_detonation(
        &mut run,
        ProjectileCoord::new(coords.x, coords.y, coords.z),
        source_house,
        stop,
        0,
        &rules,
        None,
        &mut effects,
    );
    run.finish();
    assert_eq!(curtain_left(&sim, target), 0);
}

/// Ares TechnoClass_IronCurtain (hook 70E2B0, 1004A880): the vt+0x154
/// curtain every Iron Curtain source dispatches scales its duration by the
/// type's `IronCurtain.Modifier`.
#[test]
fn techno_curtain_scales_by_the_type_modifier() {
    let rules = rules("", "IronCurtain.Modifier=1.5\n", "");
    let (mut sim, _, target) = scene(&rules);
    crate::sim::superweapon::iron_curtain::curtain_object(
        &mut sim,
        &rules,
        None,
        target,
        EntityCategory::Unit,
        None,
        101,
        FrameEffects::default(),
    );
    assert_eq!(curtain_left(&sim, target), 151);
}

/// applyPermaMC (10052CD0) -> TechnoExt::ApplyPermaMC (10045E30): the target
/// joins the firer's house for good and the vanilla area damage is skipped
/// (0x469AA4); an ImmuneToPsionics type refuses and takes the damage.
#[test]
fn permanent_mind_control_captures_and_skips_the_area_damage() {
    let rules = rules("MindControl.Permanent=yes\n", "", "");
    let (mut sim, firer, target) = scene(&rules);
    shoot(&mut sim, &rules, firer, target, 100);
    let captured = sim.entities().get(target).unwrap();
    assert_eq!(captured.owner(), sim.entities().get(firer).unwrap().owner());
    assert!(captured.mind_control.permanent());
    assert_eq!(captured.health.current(), 1000);

    let immune = self::rules("MindControl.Permanent=yes\n", "ImmuneToPsionics=yes\n", "");
    let (mut sim, firer, target) = scene(&immune);
    let owner = sim.entities().get(target).unwrap().owner();
    shoot(&mut sim, &immune, firer, target, 100);
    assert_eq!(sim.entities().get(target).unwrap().owner(), owner);
    assert_eq!(health(&sim, target), 950);
}

/// TechnoClass_ReceiveDamage_Aftermath (1004AB50) starts DisableWeaponTimer
/// with `DisableWeapons.Duration`; TechnoClass_CanFire_DisableWeapons
/// (10049EA0, hook 0x6FC0D3) then answers Rearm until it runs out.
#[test]
fn disable_weapons_duration_holds_fire_with_rearm() {
    let rules = rules("DisableWeapons.Duration=40\n", "", "");
    let (mut sim, firer, target) = scene(&rules);
    let fire_error = |sim: &Simulation| {
        let actor = sim.entities().get(target).unwrap();
        super::fire_error_world::FireSubject {
            world: sim,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer: actor,
            obj: rules.object("TARGET").unwrap(),
            target: Some(TargetKind::Entity(firer)),
            weapon_index: 0,
        }
        .fire_error(false)
    };
    assert_ne!(fire_error(&sim), FireError::Rearm);
    let event = EntityDamageEvent::direct_receiver(
        target,
        10,
        0,
        firer,
        None,
        sim.interner.intern("WH"),
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(&rules, None, event, FrameEffects::default());
    assert_eq!(fire_error(&sim), FireError::Rearm);
    let timer = sim.entities().get(target).unwrap().ares_disable_weapons;
    assert_eq!(timer.remaining(sim.session.binary_frame() as i32), 40);
}

/// The same aftermath (1004AB50) restarts the SonarTimer with
/// `Sonar.Duration` only while that is longer than the time left, which
/// CloakDisallowed (10043A40) then reads.
#[test]
fn sonar_duration_extends_the_sonar_timer() {
    let rules = rules("Sonar.Duration=60\n", "", "");
    let (mut sim, firer, target) = scene(&rules);
    let hit = |sim: &mut Simulation| {
        let event = EntityDamageEvent::direct_receiver(
            target,
            10,
            0,
            firer,
            None,
            sim.interner.intern("WH"),
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        let _ = sim.commit_direct_damage_receiver(&rules, None, event, FrameEffects::default());
    };
    let left = |sim: &Simulation| {
        let frame = sim.session.binary_frame() as i32;
        sim.entities()
            .get(target)
            .unwrap()
            .sonar_timer
            .remaining(frame)
    };
    hit(&mut sim);
    assert_eq!(left(&sim), 60);
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .sonar_timer
        .start(sim.session.binary_frame() as i32, 90);
    hit(&mut sim);
    assert_eq!(left(&sim), 90, "a longer running timer is kept");
}

/// InfantryClass_ReceiveDamage_InfDeath (10025DE0, hook 0x5185C8): a killed
/// infantryman on the ground builds the warhead's `InfDeathAnim` instead of
/// the InfDeath table's arm, owned by `MakeInfantryOwner` (Ares default
/// Invoker: the killer's house) and remapped for a MakeInfantry type.
#[test]
fn inf_death_anim_replaces_the_table_and_takes_the_invoker_house() {
    let mut rules = rules(
        "InfDeathAnim=GOO\nInfDeath=2\n",
        "",
        "[InfantryTypes]\n0=TROOP\n[TROOP]\nStrength=100\nArmor=none\nSpeed=4\n\
         [Animations]\n0=GOO\n",
    );
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[GOO]\nRate=900\nMakeInfantry=0\n",
    ));
    art.bind_anim_frame_count_for_test("GOO", 20);
    rules.replace_art_registry_for_test(art);
    let (mut sim, firer, _) = scene(&rules);
    let troop = sim
        .spawn_object("TROOP", "Russians", 25, 20, 0, &rules)
        .unwrap();
    let event = EntityDamageEvent::direct_receiver(
        troop,
        1_000,
        0,
        firer,
        None,
        sim.interner.intern("WH"),
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    let _ = sim.commit_direct_damage_receiver(&rules, None, event, FrameEffects::default());
    let goo = sim.interner.get("GOO").expect("anim type interned");
    let anims: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| anim)
        .filter(|anim| anim.type_id() == goo)
        .collect();
    let [anim] = anims[..] else {
        panic!("one InfDeathAnim: {}", anims.len());
    };
    let killer = sim.entities().get(firer).unwrap().owner();
    assert_eq!(anim.owner_house(), Some(killer));
    assert_eq!(
        anim.remap(),
        Some(crate::sim::anim_class::AnimRemap::House(killer))
    );
    assert!(
        sim.entities()
            .get(troop)
            .is_none_or(|troop| !troop.lifecycle.object_alive()),
        "the anim arm UnInits instead of the Die2 sequence"
    );
}

/// InfantryClass_ReceiveDamage_Anims (10025D20, hook 0x5186F2): the
/// FlamingInfantry arm's anim takes SetMakeInfOwner with no invoker, so the
/// default `MakeInfantryOwner` (invoker) leaves it unowned, and `victim`
/// gives it the burnt infantryman's house.
#[test]
fn inf_death_table_anim_takes_its_make_infantry_owner() {
    for (owner_kind, expect_victim) in [("", false), ("MakeInfantryOwner=victim\n", true)] {
        let mut rules = rules(
            "InfDeath=4\n",
            "",
            "[InfantryTypes]\n0=TROOP\n[TROOP]\nStrength=100\nArmor=none\nSpeed=4\n\
             [Animations]\n0=FLAME\n[General]\nFlamingInfantry=FLAME\n",
        );
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(&format!(
            "[FLAME]\nRate=900\n{owner_kind}"
        )));
        art.bind_anim_frame_count_for_test("FLAME", 20);
        rules.replace_art_registry_for_test(art);
        let (mut sim, firer, _) = scene(&rules);
        let troop = sim
            .spawn_object("TROOP", "Russians", 25, 20, 0, &rules)
            .unwrap();
        let event = EntityDamageEvent::direct_receiver(
            troop,
            1_000,
            0,
            firer,
            None,
            sim.interner.intern("WH"),
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        let _ = sim.commit_direct_damage_receiver(&rules, None, event, FrameEffects::default());
        let flame = sim.interner.get("FLAME").expect("anim type interned");
        let anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .filter(|anim| anim.type_id() == flame)
            .collect();
        let [anim] = anims[..] else {
            panic!("one FlamingInfantry anim: {}", anims.len());
        };
        let victim = sim.interner.get("Russians").unwrap();
        assert_eq!(
            anim.owner_house(),
            expect_victim.then_some(victim),
            "{owner_kind}"
        );
    }
}

/// Ares TechnoClass_CanFire_PsionicsImmune (10049EF0, hook 0x6FC417):
/// `ImmuneToBerserk` overrides `ImmuneToPsionics` for a Psychedelic weapon,
/// both ways.
#[test]
fn immune_to_berserk_overrides_the_psychedelic_fire_error() {
    for (target_ini, illegal) in [
        ("ImmuneToPsionics=yes\n", true),
        ("ImmuneToPsionics=yes\nImmuneToBerserk=no\n", false),
        ("ImmuneToBerserk=yes\n", true),
    ] {
        let rules = rules("Psychedelic=yes\n", target_ini, "");
        let (sim, firer, target) = scene(&rules);
        let code = super::fire_error_world::FireSubject {
            world: &sim,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer: sim.entities().get(firer).unwrap(),
            obj: rules.object("FIRER").unwrap(),
            target: Some(TargetKind::Entity(target)),
            weapon_index: 0,
        }
        .fire_error(false);
        assert_eq!(code == FireError::Illegal, illegal, "{target_ini}");
    }
}

/// Ares TechnoClass_ReceiveDamage_Abilities (1004A950, hook 0x701BFE): a
/// veteran with the `RADIMMUNE` rank ability takes no radiation damage; the
/// same type below veteran does.
#[test]
fn rad_immune_rank_ability_refuses_radiation_damage() {
    let rules = rules("Radiation=yes\n", "VeteranAbilities=RADIMMUNE\n", "");
    for (experience, damaged) in [(0.0f32, true), (1.0, false)] {
        let (mut sim, firer, target) = scene(&rules);
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .veterancy
            .set_raw_for_test(crate::util::native_x87::NativeF32Bits::from_bits(
                experience.to_bits(),
            ));
        shoot(&mut sim, &rules, firer, target, 30);
        assert_eq!(health(&sim, target) < 1000, damaged, "{experience}");
    }
}

/// Helpers::Alex::getCellSpreadItems (Ares.dll 10076660) with includeInAir,
/// the victim collector of applyIronCurtain, applyAttachedEffect and
/// CreateEMPulse: a FootClass object on no cell list within CellSpread of the
/// impact is collected while it is above the ground (GetHeight > 0), not at
/// ground height.
#[test]
fn cell_spread_items_collect_airborne_foot_objects() {
    let rules = rules("IronCurtain.Duration=100\n", "", "");
    let (mut sim, _, flying) = scene(&rules);
    let grounded = sim
        .spawn_object("TARGET", "Russians", 23, 21, 0, &rules)
        .expect("grounded");
    let ground = object_get_coords(sim.entities().get(flying).unwrap(), sim.resolved_terrain());
    for id in [flying, grounded] {
        let position = &sim.entities().get(id).unwrap().position;
        let (rx, ry) = (position.rx(), position.ry());
        sim.substrate.occupancy.remove(rx, ry, id);
    }
    sim.substrate
        .entities
        .get_mut(flying)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(ground.z + 100));
    // 137 leptons from both: halfway between the cells, 50 up.
    let impact = ProjectileCoord::new(ground.x, ground.y + 128, ground.z + 50);
    assert_eq!(
        sim.ares_cell_spread_items(impact, 1.0, &rules),
        vec![flying]
    );
}
