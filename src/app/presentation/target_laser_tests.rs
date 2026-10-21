//! Techno6FF960's target laser through production FireAt on retail rules:
//! a human player's `TargetLaser=` shot starts the 15-frame +3BC timer
//! (`0x006FF48D..0x006FF4C9`), and `UnitClass::Draw_It`'s last call
//! (`0x0073D415..0x0073D43A`) draws the laser while it runs.

use super::*;
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::sim::command::Command;

fn view() -> SurfaceLineViewport {
    SurfaceLineViewport {
        camera: [-1000, 0],
        clip: [0, 0, 2000, 2000],
        z_origin_y: 0,
        zoom: 1.,
    }
}

/// Entry 8 stands apart from the rest of an otherwise black palette.
fn palette() -> crate::assets::pal_file::Palette {
    let mut bytes = vec![0u8; 768];
    bytes[24..27].copy_from_slice(&[63, 8, 8]);
    crate::assets::pal_file::Palette::from_bytes(&bytes).unwrap()
}

/// A skirmish arena with the player (Americans) and a computer house, each
/// holding a building, and `shooter`'s `kind` facing a Rhino of the other
/// house four cells east.
fn duel(rules: &RuleSet, kind: &str, shooter: &str) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x23);
    let player = sim.interner().get("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(player);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    sim.spawn_object("GAPOWR", "Americans", 3, 24, 0, rules)
        .expect("american power plant");
    let victim = if shooter == "Americans" {
        "Russians"
    } else {
        "Americans"
    };
    let firer = sim
        .spawn_object(kind, shooter, 12, 16, 64, rules)
        .expect("firer");
    let target = sim
        .spawn_object("HTNK", victim, 16, 16, 192, rules)
        .expect("target");
    (sim, firer, target)
}

/// Orders the attack and runs frames until the firer's first shot. Every
/// frame before it draws no laser.
fn first_shot(
    sim: &mut Simulation,
    rules: &RuleSet,
    shooter: &str,
    firer: u64,
    target: u64,
) -> u32 {
    assert!(sim.apply_command(
        shooter,
        &Command::Attack {
            attacker_id: firer,
            target_id: target,
        },
        Some(rules),
    ));
    let palette = palette();
    for _ in 0..300 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        let entity = sim.entities().get(firer).unwrap();
        if entity.last_fire_frame.frame() >= 0 {
            return entity.last_fire_frame.frame() as u32;
        }
        assert!(target_laser_draws(entity, sim, rules, || Some(&palette), view()).is_empty());
    }
    panic!("{shooter}'s {firer} never fired");
}

/// The player's Apocalypse draws its laser on the 14 renders after its shot
/// (elapsed 1..14; Main_Tick renders before the logic that fired and its
/// frame increment): two 3x3 boxes at the attack line's endpoints, then the
/// clipped [1,0] dash, all in the attack line's colour (Palette87F6C4 index
/// 8). The dash phase `(frame * 7) % 16` lights the other half of the walk on
/// each next frame.
#[test]
fn the_players_apocalypse_marks_its_target_for_fourteen_frames() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert!(rules.object("APOC").unwrap().target_laser);
    assert!(!rules.object("HTNK").unwrap().target_laser);
    let (mut sim, apoc, rhino) = duel(rules, "APOC", "Americans");
    let shot = first_shot(&mut sim, rules, "Americans", apoc, rhino);
    let palette = palette();
    let entity = sim.entities().get(apoc).unwrap();
    // The first render after the shot reads the committed frame.
    assert_eq!(sim.session().binary_frame(), shot + 1);
    let at = |frame| target_laser_draws_at(entity, &sim, rules, || Some(&palette), view(), frame);
    assert_eq!(
        target_laser_draws(entity, &sim, rules, || Some(&palette), view()),
        at(shot + 1)
    );

    let word = action_line_word(&palette, 8);
    let [start, end] = attack_line_points(entity, &sim, rules)
        .unwrap()
        .map(|p| [p.x as i32 + 1000, p.y as i32]);
    let lit = |frame| {
        let draws = at(frame);
        assert_eq!(
            draws[..2],
            [start, end].map(|[x, y]| SurfaceDraw::OpaqueRect {
                rect: [x - 2, y - 2, 3, 3],
                word,
            })
        );
        draws[2..]
            .iter()
            .map(|draw| match *draw {
                SurfaceDraw::OpaqueRect {
                    rect: [x, y, 1, 1],
                    word: stored,
                } if stored == word => [x, y],
                other => panic!("{other:?}"),
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    let mut walk = std::collections::BTreeSet::new();
    crate::render::surface_line::patterned_line(start, end, &[1; 16], 0, |point| {
        walk.insert(point);
    });
    assert!(walk.len() > 20, "the laser spans the four cells");
    let (first, second) = (lit(shot + 1), lit(shot + 2));
    assert!(first.is_disjoint(&second));
    assert_eq!(&first | &second, walk);
    assert_eq!(lit(shot + 14), second, "the 14th render");
    assert!(at(shot + 15).is_empty(), "the timer ran out");

    sim.entities_mut_for_test()
        .get_mut(apoc)
        .unwrap()
        .attack_target
        .set_for_test(None);
    let entity = sim.entities().get(apoc).unwrap();
    assert!(
        target_laser_draws_at(entity, &sim, rules, || Some(&palette), view(), shot + 1).is_empty(),
        "6FF960 needs a Target"
    );
}

/// A computer player's Apocalypse starts no timer (House50B6F0 at
/// `0x006FF4A7`), and a Rhino (`TargetLaser=no`) none either.
#[test]
fn neither_a_computer_apocalypse_nor_the_players_rhino_draws_a_laser() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let palette = palette();
    for (kind, shooter) in [("APOC", "Russians"), ("HTNK", "Americans")] {
        let (mut sim, firer, target) = duel(rules, kind, shooter);
        first_shot(&mut sim, rules, shooter, firer, target);
        let entity = sim.entities().get(firer).unwrap();
        assert!(entity.attack_target.target().is_some());
        assert!(
            target_laser_draws(entity, &sim, rules, || Some(&palette), view()).is_empty(),
            "{shooter}'s {kind}"
        );
    }
}

/// An infantryman's selected Attack line starts at its gun: InfantryClass's
/// +300 (`0x00521D30`) is GetFLH(0), the coordinate its primary shot leaves
/// from (retail GI `PrimaryFireFLH=80,0,105`), not its feet.
#[test]
fn an_infantry_attack_line_starts_where_its_shot_leaves() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, gi, rhino) = duel(rules, "E1", "Americans");
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: gi,
            target_id: rhino,
        },
        Some(rules),
    ));
    let mut muzzle = None;
    for _ in 0..300 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if let Some(shot) = sim.fire_events().iter().find(|shot| shot.attacker_id == gi) {
            muzzle = Some(shot.fire_coord);
            break;
        }
    }
    let muzzle = muzzle.expect("the GI fires at the Rhino");
    let entity = sim.entities().get(gi).unwrap();
    let [start, _] = attack_line_points(entity, &sim, rules).unwrap();
    let at = |x, y, z| -> ScreenPoint {
        crate::util::lepton::absolute_leptons_to_screen(x, y, z).into()
    };
    assert_eq!(start, at(muzzle.x, muzzle.y, muzzle.z));
    let feet = crate::sim::movement::ground_pose::object_location(entity, sim.resolved_terrain());
    assert_ne!(start, at(feet.x, feet.y, feet.z));
}
