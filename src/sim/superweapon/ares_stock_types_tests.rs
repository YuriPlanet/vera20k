//! The Ares handlers of the stock superweapon Types (`src/Misc/SWTypes/`):
//! their SWTypeExt reads and their Activate bodies, through the production
//! rules reader and launch transaction.

use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_targeting::{HouseMask, TargetMask};
use crate::sim::superweapon::chronosphere_tests::{charge_chronosphere, click, step, world_with};
use crate::sim::world::{SimSoundEvent, Simulation};

/// Retail rules with `patch` merged over RULESMD, the Chronosphere anims
/// bound (the lib suite loads no SHP).
fn retail_ares_rules(patch: &str) -> Option<RuleSet> {
    let (mut ini, art) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    ini.merge(&IniFile::from_str(patch));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("retail rules");
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
    for name in ["CHRONOAR", "CHRONOTG", "CHRONOFD", "WARPOUT", "CHRONOSK"] {
        rules.bind_anim_frame_count_for_test(name, 20);
    }
    Some(rules)
}

/// SW_ChronoSphere::Initialize (1006EF10) and LoadFromINI (1006EF90) over
/// two layers, the Flags-forced click tags (10033370's tail) and TechnoTypeExt
/// `Chronoshift.*` (1003DFB0).
#[test]
fn ares_chronosphere_reads_its_handler_keys_over_layers() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[Animations]\n0=BLAST\n[BLAST]\nRate=1\n[InfantryTypes]\n0=E1\n\
         [E1]\nChronoshift.Allow=no\n\
         [SuperWeaponTypes]\n0=Sphere\n1=Plain\n2=Warp\n3=Second\n\
         [Sphere]\nType=ChronoSphere\nPreClick=no\nAction=Nuke\nSW.PostDependent=Second\n\
         Chronosphere.KillCargo=yes\nChronosphere.BlastSrc=BLAST\n\
         SW.AffectsTarget=infantry,buildings\n\
         [Plain]\nType=ChronoSphere\n\
         [Warp]\nType=ChronoWarp\nPostClick=no\nPreDependent=ChronoSphere\n\
         [Second]\nType=ChronoWarp\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[Sphere]\nChronosphere.KillOrganic=no\nChronosphere.BlastDest=none\n\
             [E1]\nChronoshift.IsVehicle=yes\n",
        ),
    );
    let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let sphere = rules.super_weapon("Sphere").unwrap();
    let warp = rules.super_weapon("Warp").unwrap();
    assert_eq!(
        (
            sphere.pre_click,
            sphere.post_click,
            sphere.action.as_deref()
        ),
        (true, false, Some("ChronoSphere"))
    );
    assert_eq!(
        (warp.pre_click, warp.post_click, warp.pre_dependent),
        (false, true, -1)
    );
    let metadata = rules.super_weapon_metadata("Sphere").unwrap();
    let chrono = metadata.launch().chronosphere();
    assert_eq!(
        (
            chrono.kill_organic,
            chrono.kill_cargo,
            chrono.affect_unwarpable,
            chrono.blow_unplaceable,
            chrono.reconsider_buildings,
            chrono.kill_teleporters,
        ),
        (false, true, true, true, true, false),
        "Initialize's bytes under both passes' reads"
    );
    assert_eq!(chrono.blast_src, Some(Some("BLAST".to_owned())));
    assert_eq!(chrono.blast_dest, Some(None), "an explicit none");
    // Initialize's 0x18, the authored infantry+buildings, the Building bit
    // split off and set again; the scenario pass keeps the answer.
    let targeting = metadata.targeting();
    let kinds = |mask: TargetMask| {
        [TargetMask::INFANTRY, TargetMask::UNIT, TargetMask::BUILDING]
            .map(|kind| mask.intersects(kind))
    };
    assert!(targeting.chrono_affect_buildings());
    assert_eq!(kinds(targeting.affects_target()), [true, false, true]);
    let plain = rules.super_weapon_metadata("Plain").unwrap().targeting();
    assert!(!plain.chrono_affect_buildings());
    assert_eq!(kinds(plain.affects_target()), [true, true, true]);
    assert_eq!(
        (sphere.presentation.height, sphere.presentation.visibility),
        (5, HouseMask::TEAM)
    );
    assert_eq!(
        crate::sim::superweapon::ares_chrono_warp_for(&rules, sphere),
        Some("Second")
    );
    assert_eq!(
        crate::sim::superweapon::ares_chrono_warp_for(&rules, rules.super_weapon("Plain").unwrap()),
        Some("Warp"),
        "without SW.PostDependent, the first ChronoWarp"
    );
    let e1 = rules.object("E1").unwrap().ares_chronoshift();
    assert_eq!((e1.allow, e1.is_vehicle, e1.crushable), (false, true, true));
}

/// SW_ChronoWarp::Activate (1006FB60) and its lambda (1006F4D0) on retail
/// rules: the source is the House's last Super, whose `SW.Range` and
/// `Chronosphere.KillTeleporters` apply; `Chronoshift.Allow=no` stays; the
/// destination is the Foot's coordinates moved by the cell offset. The
/// ChronoSphere's Flags (1006EE70) leave its EVA, message and cleanup out;
/// the warp's come from the source's type.
#[test]
fn ares_chrono_warp_uses_the_last_super_and_its_keys() {
    let Some(rules) = retail_ares_rules(
        "[ChronoSphereSpecial]\nChronosphere.KillTeleporters=yes\nSW.Range=5,5\n\
         [HTNK]\nChronoshift.Allow=no\n",
    ) else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let spawn = |sim: &mut Simulation, kind: &str, (x, y): (u16, u16)| {
        sim.spawn_object_at_height(kind, "Americans", x, y, 0, 0, &rules)
            .unwrap()
    };
    let source = crate::sim::superweapon::chronosphere_tests::SOURCE;
    let far_tank = spawn(&mut sim, "MTNK", (source.0 + 2, source.1));
    let rhino = spawn(&mut sim, "HTNK", source);
    let legionnaire = spawn(&mut sim, "CLEG", (source.0 - 1, source.1));
    let here = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(far_tank).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let sphere = charge_chronosphere(&mut sim, americans);
    sim.take_sound_events_for_test();
    click(&mut sim, &rules, americans, "ChronoSphereSpecial", source);
    assert_eq!(sim.houses[&americans].ares_last_super(), 3);
    assert!(!sim.take_sound_events_for_test().iter().any(|event| {
        matches!(
            event,
            SimSoundEvent::AresSuperWeaponActivatedEva { .. }
                | SimSoundEvent::AresSuperWeaponFinished { .. }
        )
    }));
    let target = (40, 40);
    click(&mut sim, &rules, americans, "ChronoWarpSpecial", target);
    let events = sim.take_sound_events_for_test();
    assert!(events.iter().any(|event| matches!(
        event,
        SimSoundEvent::AresSuperWeaponActivatedEva { sw_type } if *sw_type == sphere
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        SimSoundEvent::AresSuperWeaponFinished { owner, sw_type, message: true }
            if *owner == americans && *sw_type == sphere
    )));
    assert!(
        sim.substrate
            .entities
            .get(legionnaire)
            .is_none_or(|entity| entity.health.current() == 0 || entity.death.is_dying()),
        "KillTeleporters kills the Teleporter"
    );
    assert!(
        sim.substrate
            .entities
            .get(rhino)
            .unwrap()
            .chrono_warp()
            .is_none()
    );
    let warp = sim
        .substrate
        .entities
        .get(far_tank)
        .and_then(|entity| entity.chrono_warp())
        .expect("SW.Range 5x5 reaches two cells out");
    let offset = (i32::from(target.0) - i32::from(source.0)) * 256;
    assert_eq!(
        (
            warp.destination().x,
            warp.destination().y,
            warp.destination().z
        ),
        (here.x + offset, here.y + offset, here.z)
    );
}

/// A one-silo-less nuke world: `extra` merged over the fixture rules.
fn nuke_rules(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(
        "[General]\nNukeTakeOff=NUKETO\n[AudioVisual]\nDigSound=DigCue\n\
         [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [SuperWeaponTypes]\n0=NukeSpecial\n\
         [NukeSpecial]\nType=MultiMissile\nWeaponType=NukeCarrier\nIsPowered=no\n\
         [NukeCarrier]\nDamage=1000\nProjectile=GiantNukeUp\nSpeed=100\nWarhead=NukeMaker\n\
         [NukePayload]\nDamage=1000\nProjectile=GiantNukeDown\nSpeed=50\nWarhead=NUKE\n\
         [GiantNukeUp]\nVertical=yes\n[GiantNukeDown]\nVertical=yes\n\
         [Warheads]\n0=NukeMaker\n1=NUKE\n2=Blast\n[NukeMaker]\nNukeMaker=yes\n\
         [Animations]\n0=PSIWARN\n1=NUKETO\n2=NUKEBALL\n3=FLAME\n",
    );
    ini.merge(&IniFile::from_str(extra));
    let art = IniFile::from_str("[PSIWARN]\n[NUKETO]\n[NUKEBALL]\n[FLAME]\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

/// SW_NuclearMissile::Initialize (10073360) and LoadFromINI (10073410),
/// the common `SW.Warhead`/`SW.Damage`/`Light.*` reads, and the NUKE warhead's
/// Initialize (10051D10) with WarheadTypeExt's nuke keys (10051D80).
#[test]
fn ares_nuke_reads_its_handler_and_warhead_keys() {
    let rules = nuke_rules(
        "[NukeSpecial]\nNuke.TakeOff=FLAME\nNuke.PsiWarning=none\nSW.Warhead=Blast\n\
         SW.Damage=77\nLight.Enabled=no\nLight.Red=5\n\
         [Blast]\nNukeFlash.Duration=12\nPreImpactAnim=FLAME\nPreImpactAnim.Moves=yes\n",
    );
    let nuke = rules.super_weapon("NukeSpecial").unwrap();
    assert_eq!(nuke.action.as_deref(), Some("Nuke"));
    let launch = rules.super_weapon_metadata("NukeSpecial").unwrap().launch();
    let fields = launch.nuke();
    assert_eq!(fields.payload.as_deref(), Some("NukePayload"));
    assert_eq!(fields.psi_warning, None, "an explicit none over PSIWARN");
    assert_eq!(fields.take_off, Some(Some("FLAME".to_owned())));
    assert!(fields.silo_launch);
    assert_eq!(
        (launch.warhead(), launch.damage()),
        (Some(Some("Blast")), Some(77))
    );
    assert!(!launch.lighting().enabled());
    assert_eq!(launch.lighting().overrides(), [None, Some(5), None, None]);
    let combat = |name| rules.warhead(name).unwrap().ares_rules().unwrap().combat();
    assert_eq!(
        (
            combat("NUKE").nuke_flash_duration(),
            combat("NUKE").pre_impact_anim()
        ),
        (Some(30), Some("NUKEBALL"))
    );
    assert_eq!(
        (
            combat("Blast").nuke_flash_duration(),
            combat("Blast").pre_impact_anim(),
            combat("Blast").pre_impact_anim_moves()
        ),
        (Some(12), Some("FLAME"), true)
    );
    let (payload, warhead, damage) =
        crate::sim::superweapon::nuke::ares_payload_hit(&rules, nuke).unwrap();
    assert_eq!(
        (
            payload.id.as_str(),
            warhead.map(|warhead| warhead.id.as_str()),
            damage
        ),
        ("NukePayload", Some("Blast"), 77)
    );
}

/// SW_NuclearMissile::Activate (10072FF0) without `Nuke.SiloLaunch`: the
/// payload drops at once over the cell, with the type's damage, the
/// PsiWarn anim, BulletExt NukeSW, a vertical start 20000 leptons up at
/// (0, 0, -100), the ally-only radar event and DigSound.
#[test]
fn ares_nuke_without_a_silo_drops_its_payload() {
    let rules = nuke_rules("[NukeSpecial]\nNuke.SiloLaunch=no\nSW.Damage=77\n");
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let typ = crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut sim,
        americans,
        "NukeSpecial",
    );
    sim.take_sound_events_for_test();
    let cell = (30, 30);
    assert!(sim.fire_super_weapon(
        &rules,
        americans,
        typ,
        cell,
        None,
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let bullets: Vec<_> = sim.projectiles.iter().collect();
    assert_eq!(bullets.len(), 1);
    let (&bullet_id, bullet) = bullets[0];
    assert_eq!(bullet.payload().base_damage, 77);
    assert_eq!(sim.interner.resolve(bullet.payload().warhead), "NUKE");
    assert_eq!(bullet.ares_nuke_sw(), Some(typ));
    let deck = super::deck_coords(&sim, cell);
    assert_eq!(
        [
            bullet.launch_origin().x,
            bullet.launch_origin().y,
            bullet.launch_origin().z
        ],
        [deck[0], deck[1], deck[2] + 20000]
    );
    assert_eq!(
        bullet.velocity().native().map(|value| value.bits()),
        [0.0_f64, 0.0, -100.0].map(f64::to_bits)
    );
    assert!(
        sim.substrate
            .anims
            .iter()
            .any(|(_, anim)| anim.attached_bullet() == Some(bullet_id)),
        "the PsiWarn anim follows the bullet"
    );
    let events = sim.take_sound_events_for_test();
    assert!(events.iter().any(|event| matches!(
        event,
        SimSoundEvent::AresAlliedRadarEvent { owner, .. } if *owner == americans
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        SimSoundEvent::VocAtCoord { sound_id, .. } if sound_id == "DigCue"
    )));
}

/// The impact hook's decision (467E59, 10036960): a stock NUKE bullet keeps
/// its 30-frame flash and NUKEBALL but no superweapon arm; a superweapon's
/// bullet rises, raises the radar event and loses its flash to
/// `Light.Enabled=no`; a NukeMaker warhead is never the superweapon arm; an
/// ordinary warhead makes no impact.
#[test]
fn ares_nuke_impact_follows_the_bullet_and_its_warhead() {
    let rules = nuke_rules("[NukeSpecial]\nLight.Enabled=no\n[Blast]\nVerses=100%\n");
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    let typ = sim.interner.intern("NukeSpecial");
    let ball = |sim: &mut Simulation, warhead: &str, nuke_sw| {
        let warhead = sim.interner.intern(warhead);
        crate::sim::superweapon::nuke::ares_nuke_ball(&rules, &sim.interner, warhead, nuke_sw)
    };
    let stock = ball(&mut sim, "NUKE", None);
    assert_eq!(
        (
            stock.sw_bullet,
            stock.radar,
            stock.flash,
            stock.pre_impact_anim
        ),
        (false, false, Some(30), true)
    );
    let launched = ball(&mut sim, "NUKE", Some(typ));
    assert_eq!(
        (
            launched.sw_bullet,
            launched.radar,
            launched.flash,
            launched.pre_impact_anim
        ),
        (true, true, None, true)
    );
    assert!(!ball(&mut sim, "NukeMaker", Some(typ)).sw_bullet);
    let plain = ball(&mut sim, "Blast", None);
    assert_eq!((plain.flash, plain.pre_impact_anim), (None, false));
}

/// SW_PsychicDominator::Initialize (10070990) and LoadFromINI (10070A40),
/// SW_GeneticMutator's (10072090, 10072100), and the common `SW.Deferment`.
#[test]
fn ares_dominator_and_mutator_read_their_handler_keys() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Animations]\n0=RING\n[RING]\nRate=1\n\
         [SuperWeaponTypes]\n0=Dom\n1=Mut\n\
         [Dom]\nType=PsychicDominator\nDominator.FirstAnimHeight=300\n\
         Dominator.ControlAnim=RING\nDominator.FireAtPercentage=40\n\
         Dominator.PermanentCapture=no\nSW.Deferment=6\n\
         [Mut]\nType=GeneticConverter\nMutate.Explosion=yes\nMutate.IgnoreCyborg=yes\n\
         SW.AffectsTarget=land\n",
    ))
    .process_with_fixed_art(&IniFile::empty())
    .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let dom = rules.super_weapon_metadata("Dom").unwrap();
    let fields = dom.launch().dominator();
    assert_eq!(
        (
            fields.first_anim_height,
            fields.second_anim_height,
            fields.fire_at_percentage,
            fields.control_anim.clone(),
            fields.first_anim.clone(),
        ),
        (300, 0, Some(40), Some(Some("RING".to_owned())), None)
    );
    assert_eq!(
        (
            fields.capture,
            fields.ripple,
            fields.capture_mind_controlled,
            fields.capture_perma_mind_controlled,
            fields.capture_immune_to_psionics,
            fields.permanent_capture,
        ),
        (true, true, true, true, false, false)
    );
    assert_eq!(dom.launch().deferment(), Some(6));
    let dominator = rules.super_weapon("Dom").unwrap();
    assert_eq!(
        dominator
            .ares_messages()
            .label(crate::rules::superweapon_type::AresSuperWeaponMessage::Abort),
        "Msg:DominatorActive"
    );
    let kinds = |mask: TargetMask| {
        [TargetMask::INFANTRY, TargetMask::UNIT, TargetMask::BUILDING]
            .map(|kind| mask.intersects(kind))
    };
    assert_eq!(kinds(dom.targeting().affects_target()), [true, true, false]);
    let mutator = rules.super_weapon_metadata("Mut").unwrap();
    let mutate = mutator.launch().mutate();
    assert_eq!(
        (
            mutate.explosion,
            mutate.ignore_cyborg,
            mutate.ignore_not_human,
            mutate.kill_natural
        ),
        (Some(true), true, false, true)
    );
    assert!(
        mutator
            .targeting()
            .affects_target()
            .intersects(TargetMask::LAND)
            && kinds(mutator.targeting().affects_target()) == [true, true, true],
        "LoadFromINI adds every Techno to the authored land"
    );
    assert_eq!(rules.super_weapon("Mut").unwrap().presentation.height, 5);
}

/// SW_PsychicDominator::Activate (100707C0), its state machine (10070BA0)
/// and the PsyDom_Fire hook (10037840) on retail rules: without anims the
/// strike follows at once; an enemy tank joins the firer without permanent
/// control, an ImmuneToPsionics one stays, and a second launch while active
/// is refused by AbortFire (10070780). `Light.*` tints the scenario.
#[test]
fn ares_dominator_captures_through_its_state_machine() {
    let Some(rules) = retail_ares_rules(
        "[PsychicDominatorSpecial]\nDominator.FirstAnim=none\nDominator.SecondAnim=none\n\
         Dominator.Ripple=no\nDominator.PermanentCapture=no\nSW.Damage=0\n\
         Light.Ambient=12\nLight.Red=7\n\
         [HTNK]\nImmuneToPsionics=yes\n",
    ) else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let russians = sim.interner.intern("Russians");
    let cell = (30, 30);
    let spawn = |sim: &mut Simulation, kind: &str, at: (u16, u16)| {
        sim.spawn_object_at_height(kind, "Russians", at.0, at.1, 0, 0, &rules)
            .unwrap()
    };
    let tank = spawn(&mut sim, "MTNK", cell);
    let immune = spawn(&mut sim, "HTNK", (cell.0 + 1, cell.1));
    let typ = crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut sim,
        americans,
        "PsychicDominatorSpecial",
    );
    let fire = |sim: &mut Simulation| {
        sim.fire_super_weapon(
            &rules,
            americans,
            typ,
            cell,
            None,
            crate::sim::world::FrameEffects::default(),
            false,
        )
    };
    assert!(fire(&mut sim));
    let tick = |sim: &mut Simulation| {
        crate::sim::superweapon::tick_active_superweapon_effects(
            sim,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
    };
    tick(&mut sim);
    // UpdateLighting (hook 53C280): CurrentPsyDom's Light.* over the
    // Dominator profile.
    let lighting = sim.session.lighting();
    assert_eq!(
        lighting.selected_profile(),
        crate::sim::scenario_session::ScenarioLightingProfile::Dominator
    );
    assert_eq!(lighting.target_ambient(), 12);
    assert_eq!(lighting.alternate_rgb().unwrap()[0], 70);
    tick(&mut sim);
    assert_eq!(sim.psychic_dominator.status_number(), 3);
    let captured = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(captured.owner(), americans);
    assert!(!captured.mind_control.permanent());
    assert_eq!(
        sim.substrate.entities.get(immune).unwrap().owner(),
        russians
    );
    crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut sim,
        americans,
        "PsychicDominatorSpecial",
    );
    sim.take_sound_events_for_test();
    assert!(!fire(&mut sim), "AbortFire refuses while one is active");
    assert!(
        sim.take_sound_events_for_test()
            .iter()
            .any(|event| matches!(
                event,
                SimSoundEvent::AresSuperWeaponAborted { owner, sw_type }
                    if *owner == americans && *sw_type == typ
            ))
    );
}

/// SW_GeneticMutator::Activate (10071C60) on retail rules in its ranged
/// mode (`Mutate.Explosion=no` over retail's `MutateExplosion=yes`): a `Natural=` infantryman dies to `C4Warhead=` under
/// `Mutate.KillNatural`, a `Cyborg=` one is spared by `Mutate.IgnoreCyborg`.
#[test]
fn ares_mutator_spares_cyborgs_and_kills_natural_infantry() {
    let Some(rules) = retail_ares_rules(
        "[GeneticConverterSpecial]\nMutate.Explosion=no\nMutate.IgnoreCyborg=yes\n\
         [E1]\nNatural=yes\n[ENGINEER]\nCyborg=yes\n",
    ) else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let cell = (30, 30);
    let spawn = |sim: &mut Simulation, kind: &str, at: (u16, u16)| {
        sim.spawn_object_at_height(kind, "Russians", at.0, at.1, 0, 0, &rules)
            .unwrap()
    };
    let natural = spawn(&mut sim, "E1", cell);
    let cyborg = spawn(&mut sim, "ENGINEER", (cell.0 + 1, cell.1));
    let strength = sim.substrate.entities.get(cyborg).unwrap().health.current();
    let typ = crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut sim,
        americans,
        "GeneticConverterSpecial",
    );
    assert!(sim.fire_super_weapon(
        &rules,
        americans,
        typ,
        cell,
        None,
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    assert!(
        sim.substrate
            .entities
            .get(natural)
            .is_none_or(|entity| entity.health.current() == 0 || entity.death.is_dying())
    );
    assert_eq!(
        sim.substrate.entities.get(cyborg).unwrap().health.current(),
        strength
    );
}

/// Whether `id` is gone or dying.
fn dead(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_none_or(|entity| entity.health.current() == 0 || entity.death.is_dying())
}

/// The ChronoSphere then the ChronoWarp from the fixture source to `target`.
fn chronoshift(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: crate::sim::intern::InternedId,
    target: (u16, u16),
) {
    let source = crate::sim::superweapon::chronosphere_tests::SOURCE;
    charge_chronosphere(sim, owner);
    click(sim, rules, owner, "ChronoSphereSpecial", source);
    click(sim, rules, owner, "ChronoWarpSpecial", target);
}

/// TeleportLocomotionClass_MakeRoom (1004B980, hook 0x718275): a Unit
/// arriving on a `Chronoshift.Crushable=no` tank dies instead of it, and
/// with `ChronoInfantryCrush=no` arriving infantry dies on a tank.
#[test]
fn ares_make_room_spares_uncrushable_units_and_kills_crush_shy_infantry() {
    let Some(rules) = retail_ares_rules(
        "[General]\nChronoInfantryCrush=no\n[HTNK]\nChronoshift.Crushable=no\n\
         [ChronoSphereSpecial]\nChronosphere.KillOrganic=no\n",
    ) else {
        return;
    };
    assert!(!rules.ares_chrono_infantry_crush());
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let spawn = |sim: &mut Simulation, kind: &str, owner: &str, (x, y): (u16, u16)| {
        sim.spawn_object_at_height(kind, owner, x, y, 0, 0, &rules)
            .unwrap()
    };
    let source = crate::sim::superweapon::chronosphere_tests::SOURCE;
    let target = (40, 40);
    let tank = spawn(&mut sim, "MTNK", "Americans", source);
    let gi = spawn(&mut sim, "E1", "Americans", (source.0 - 1, source.1));
    let uncrushable = spawn(&mut sim, "HTNK", "Russians", target);
    let under_gi = spawn(&mut sim, "MTNK", "Russians", (target.0 - 1, target.1));
    chronoshift(&mut sim, &rules, americans, target);
    for _ in 0..200 {
        step(&mut sim, &rules);
        if [tank, gi].iter().all(|&id| {
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|entity| entity.chrono_warp().is_none())
        }) {
            break;
        }
    }
    assert!(dead(&sim, tank) && !dead(&sim, uncrushable));
    assert!(dead(&sim, gi) && !dead(&sim, under_gi));
}

/// SW_ChronoWarp's Building arm and ChronoWarpStateMachine (1006F4D0,
/// 10070230): a building in `SW.AffectsTarget` is warped out at once and
/// moves by the target's offset `ChronoDelay` frames after the launch.
#[test]
fn ares_chrono_warp_moves_a_building_after_chrono_delay() {
    let Some(rules) = retail_ares_rules("[ChronoSphereSpecial]\nSW.AffectsTarget=all\n") else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let source = crate::sim::superweapon::chronosphere_tests::SOURCE;
    let target = (40, 40);
    let plant = sim
        .spawn_object_at_height("GAPOWR", "Americans", source.0, source.1, 0, 0, &rules)
        .unwrap();
    chronoshift(&mut sim, &rules, americans, target);
    let at = |sim: &Simulation| {
        let entity = sim.substrate.entities.get(plant).unwrap();
        (
            entity.position.rx(),
            entity.position.ry(),
            entity.is_warped_out(),
        )
    };
    assert_eq!(at(&sim), (source.0, source.1, true));
    let delay = rules.general.chrono_delay as usize;
    for _ in 1..delay {
        step(&mut sim, &rules);
    }
    assert_eq!(at(&sim), (source.0, source.1, true));
    step(&mut sim, &rules);
    assert_eq!(at(&sim), (target.0, target.1, false));
    assert!(!dead(&sim, plant));
}

/// An unplaceable building goes back to its origin and, under the
/// `Chronosphere.BlowUnplaceable` default, takes its Strength as C4 damage.
#[test]
fn ares_chrono_warp_blows_up_an_unplaceable_building() {
    let Some(rules) = retail_ares_rules("[ChronoSphereSpecial]\nSW.AffectsTarget=all\n") else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let source = crate::sim::superweapon::chronosphere_tests::SOURCE;
    let target = (40, 40);
    let plant = sim
        .spawn_object_at_height("GAPOWR", "Americans", source.0, source.1, 0, 0, &rules)
        .unwrap();
    sim.spawn_object_at_height("NAPOWR", "Russians", target.0, target.1, 0, 0, &rules)
        .unwrap();
    chronoshift(&mut sim, &rules, americans, target);
    for _ in 0..=rules.general.chrono_delay {
        step(&mut sim, &rules);
    }
    let entity = sim.substrate.entities.get(plant);
    assert!(entity.is_none_or(|entity| (entity.position.rx(), entity.position.ry()) == source));
    assert!(dead(&sim, plant));
}

/// The lambda's `Force_Track(-1, coordsUnitSource)` (1006F4D0; Drive
/// 0x4B0C40 stores it as head and destination) outlives the warp: nothing
/// in the Teleport states (0x7192F0), its END (0x719EE0) or the Foot AI
/// tail resets the piggybacked Drive, so once it is active again its
/// Process (0x4B0500 -> Process_Movement 0x4B2630) paths to the source
/// cell and the tank drives back, as gamemd with Ares does.
#[test]
fn ares_warped_tank_heads_back_to_its_source() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::track_process::TrackFamily;
    let Some(rules) = retail_ares_rules("") else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let source = crate::sim::superweapon::chronosphere_tests::SOURCE;
    let target = (40, 40);
    let tank = sim
        .spawn_object_at_height("MTNK", "Americans", source.0, source.1, 0, 0, &rules)
        .unwrap();
    let here = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(tank).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    chronoshift(&mut sim, &rules, americans, target);
    let drive = |sim: &Simulation| {
        let entity = sim.substrate.entities.get(tank).unwrap();
        let locomotor = entity.locomotor.as_ref().unwrap();
        (locomotor.active_kind() == LocomotorKind::Drive && entity.chrono_warp().is_none())
            .then(|| locomotor.track_destination(TrackFamily::Drive))
    };
    let mut frames = 0;
    while drive(&sim).is_none() && frames < 200 {
        step(&mut sim, &rules);
        frames += 1;
    }
    let entity = sim.substrate.entities.get(tank).unwrap();
    assert_eq!((entity.position.rx(), entity.position.ry()), target);
    assert_eq!(drive(&sim), Some(Some(here)));
    for _ in 0..60 {
        step(&mut sim, &rules);
    }
    let entity = sim.substrate.entities.get(tank).unwrap();
    assert_ne!((entity.position.rx(), entity.position.ry()), target);
}

/// SW_SpyPlane (Initialize 10075800, LoadFromINI 10075880, Activate
/// 10075780): `SpyPlane.Count` planes of the found `SpyPlane.Type` take off
/// (in `SpyPlane.Mission`); Initialize clears `SW.CreateRadarEvent`.
#[test]
fn ares_spy_plane_sends_its_count_of_its_type() {
    let Some(rules) = retail_ares_rules(
        "[SpyPlaneSpecial]\nSpyPlane.Count=2\nSpyPlane.Type=PDPLANE\nSpyPlane.Mission=Move\n",
    ) else {
        return;
    };
    let metadata = rules.super_weapon_metadata("SpyPlaneSpecial").unwrap();
    let spy_plane = metadata.launch().spy_plane();
    assert_eq!(spy_plane.plane.as_deref(), Some("PDPLANE"));
    assert_eq!(spy_plane.mission, crate::sim::mission::MissionType::Move);
    assert!(!metadata.launch().radar_event());
    let (rules, mut sim, americans) = super::spy_plane_tests::world(rules);
    crate::sim::superweapon::chronosphere_tests::charge_super(
        &mut sim,
        americans,
        "SpyPlaneSpecial",
    );
    click(&mut sim, &rules, americans, "SpyPlaneSpecial", (40, 40));
    let planes: Vec<_> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "PDPLANE")
        .map(|entity| (entity.owner(), entity.is_mission_only()))
        .collect();
    assert_eq!(planes, [(americans, true); 2]);
}
