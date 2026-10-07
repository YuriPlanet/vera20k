//! The nuclear missile through production frames on retail rules: the
//! launch command, the silo's flight, NukeMaker's falling warhead and its
//! strike.

use super::SuperWeaponInstance;
use crate::map::resolved_terrain::test_grid;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::projectile::{Projectile, ProjectileTarget};
use crate::sim::world::{SimSoundEvent, Simulation};

const TARGET: (u16, u16) = (40, 40);

fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), None, None, 33);
}

/// Steps until `done` holds, at most `frames` times; the frames it took.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    frames: u32,
    mut done: impl FnMut(&Simulation) -> bool,
) -> u32 {
    for frame in 1..=frames {
        step(sim, rules);
        if done(sim) {
            return frame;
        }
    }
    panic!("not reached in {frames} frames");
}

fn bullets_of<'a>(sim: &'a Simulation, weapon: &str) -> Vec<(u64, &'a Projectile)> {
    sim.projectiles
        .iter()
        .filter(|(_, bullet)| sim.interner.resolve(bullet.payload.weapon) == weapon)
        .map(|(&id, bullet)| (id, bullet))
        .collect()
}

fn psi_warnings(sim: &Simulation) -> Vec<Option<u64>> {
    sim.substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "PSIWARN")
        .map(|(_, anim)| anim.attached_bullet())
        .collect()
}

#[test]
fn retail_nuclear_missile_rises_falls_and_strikes_its_target() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    // The lib suite loads no SHP: bind the two anims' frame counts.
    for name in ["PSIWARN", "NUKETO"] {
        rules.bind_anim_frame_count_for_test(name, 20);
    }
    // The retail values the chain reads.
    let nuke = rules.super_weapon("NukeSpecial").unwrap();
    assert_eq!(
        (
            nuke.kind,
            nuke.weapon_type.as_deref(),
            nuke.ai_defend_against,
            nuke.recharge_time_frames
        ),
        (
            SuperWeaponKind::MultiMissile,
            Some("NukeCarrier"),
            true,
            9000
        )
    );
    let silo_type = rules.object("NAMISL").unwrap();
    assert!(silo_type.nuke_silo);
    assert_eq!(silo_type.super_weapon.as_deref(), Some("NukeSpecial"));
    assert_eq!(silo_type.charged_anim_time, 1.0);
    assert_eq!(rules.general.nuke_take_off, "NUKETO");
    assert_eq!(rules.general.ai_super_defense_probability, vec![90, 50, 10]);
    assert_eq!(rules.general.ai_super_defense_distance, 12 * 256);
    let up = rules
        .projectile(
            rules
                .weapon("NukeCarrier")
                .unwrap()
                .projectile
                .as_deref()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(up.detonation_altitude, 20000);
    let payload = rules.weapon("NukePayload").unwrap();
    assert_eq!(
        (
            payload.damage,
            payload.warhead.as_deref(),
            payload.rad_level
        ),
        (600, Some("NUKE"), 500)
    );

    let mut sim = Simulation::with_seed(11);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let russians = sim.interner.intern("Russians");
    let americans = sim.interner.intern("Americans");
    for (id, side, human) in [(russians, 1, true), (americans, 0, false)] {
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    {
        // A hard computer house takes up the defence 90 times in 100.
        let house = sim.houses.get_mut(&americans).unwrap();
        house.base_center = Some((42, 42));
        house.difficulty = crate::sim::house_state::HouseDifficulty::Hard;
    }
    sim.session.game_options.super_weapons = true;
    sim.resolved_terrain = Some(test_grid(
        64,
        64,
        super::cell_receiver_tests::test_terrain_cell,
    ));
    sim.playfield_bounds = Some(super::cell_receiver_tests::test_playfield_bounds());
    let silo = sim
        .spawn_object_at_height("NAMISL", "Russians", 10, 10, 0, 0, &rules)
        .expect("the silo stands");
    // Power for the silo, so the recharge runs.
    for at in [(20, 10), (24, 10)] {
        sim.spawn_object_at_height("NAPOWR", "Russians", at.0, at.1, 0, 0, &rules)
            .expect("the power plant stands");
    }
    let tank = sim
        .spawn_object_at_height("MTNK", "Americans", TARGET.0, TARGET.1, 0, 0, &rules)
        .unwrap();
    let sw_type: InternedId = sim.interner.intern("NukeSpecial");
    let mut instance = SuperWeaponInstance::new(sw_type, russians);
    instance.activate(9000, sim.session.binary_frame);
    instance.is_ready = true;
    sim.super_weapons
        .entry(russians)
        .or_default()
        .insert(sw_type, instance);

    sim.queue_command(CommandEnvelope::new(
        russians,
        sim.session.tick + 1,
        Command::LaunchSuperWeapon {
            sw_type_id: sw_type,
            target_rx: TARGET.0,
            target_ry: TARGET.1,
        },
    ));
    let launch_frame = sim.session.binary_frame as i32;
    run_until(&mut sim, &rules, 3, |sim| {
        !bullets_of(sim, "NukeCarrier").is_empty()
    });
    // ClickFire restarted the recharge; Launch aimed the silo and the house.
    let instance = &sim.super_weapons[&russians][&sw_type];
    assert!(!instance.is_ready);
    assert_eq!(instance.charge_duration, 9000);
    assert!(instance.charge_start_tick >= launch_frame);
    assert_eq!(sim.houses[&russians].nuke_target(), TARGET);
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::SuperWeaponLaunched { owner, rx: 40, ry: 40, .. } if *owner == russians
    )));
    // The computer house near the target took up the defence of its base.
    assert_eq!(sim.houses[&americans].super_weapon_defense().0, (42, 42));
    let (missile, _) = bullets_of(&sim, "NukeCarrier")[0];
    assert_eq!(sim.projectiles.get(missile).unwrap().source_id, silo);
    // `FirersPalette=yes` (GiantNukeUp): Construct kept the silo's House for
    // the draw's colour scheme (Bullet+0x114).
    assert_eq!(
        sim.projectiles.get(missile).unwrap().firer_house(),
        Some(russians)
    );
    assert_eq!(psi_warnings(&sim), vec![Some(missile)]);

    // The warning shows to a house not allied with the launcher whose
    // powered Psychic Sensor (2x2, `PsychicDetectionRadius=15`) stands within
    // 15 cells of the target cell's centre.
    let detected = |sim: &Simulation, viewer| {
        sim.substrate
            .anims
            .iter()
            .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "PSIWARN")
            .any(|(_, anim)| sim.psi_warning_detected_by(viewer, anim, &rules))
    };
    let place = |sim: &mut Simulation, kind, owner, at: (u16, u16)| {
        sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, &rules)
            .expect("the building stands");
        step(sim, &rules);
    };
    place(&mut sim, "NAPOWR", "Americans", (50, 50));
    // Its centre is 5248 leptons east of the target cell's.
    place(&mut sim, "NAPSIS", "Americans", (60, 40));
    assert!(!detected(&sim, americans), "beyond the radius");
    place(&mut sim, "NAPSIS", "Russians", (50, 40));
    assert!(!detected(&sim, russians), "the launcher's own warning");
    // sqrt(3712² + 128²) = 3714 leptons, within 15 × 256.
    place(&mut sim, "NAPSIS", "Americans", (54, 40));
    assert!(detected(&sim, americans));
    assert_eq!(psi_warnings(&sim), vec![Some(missile)]);

    // The missile rises out of sight; its warning goes with it and the
    // warhead falls from 20000 leptons over the target.
    run_until(&mut sim, &rules, 600, |sim| {
        sim.projectiles.get(missile).is_none()
    });
    assert!(psi_warnings(&sim).is_empty());
    let falling = bullets_of(&sim, "NukePayload");
    assert_eq!(falling.len(), 1);
    let (warhead, bullet) = falling[0];
    assert_eq!(
        bullet.target,
        ProjectileTarget::Cell {
            rx: TARGET.0,
            ry: TARGET.1
        }
    );
    assert_eq!(bullet.source_id, silo);
    assert_eq!(bullet.firer_house(), Some(russians), "GiantNukeDown");
    assert_eq!(
        [
            bullet.launch_origin.x,
            bullet.launch_origin.y,
            bullet.launch_origin.z
        ],
        [40 * 256 + 128, 40 * 256 + 128, 20000]
    );

    // It lands on the tank.
    run_until(&mut sim, &rules, 3000, |sim| {
        sim.projectiles.get(warhead).is_none()
    });
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|entity| entity.dying || entity.health.current == 0),
        "the strike destroys the tank"
    );
    assert!(
        sim.radiation.site_at(TARGET).is_some(),
        "NukePayload's RadLevel"
    );
}
