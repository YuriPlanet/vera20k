//! Tiberian Sun's `BounceSound=` and `ExpireSound=` on a bouncing SHP chunk
//! and a `[VoxelAnims]` piece, read by the production rules processor and
//! played where Firestorm 2.0 plays them. gamemd reads neither key.

use super::{AnimClassSpawnDescriptor, AnimWorldCoord};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// The sound names, upper-cased as the SHP anim slots hold them, of every
/// positional one-shot the frame emitted, in order.
fn played(sim: &Simulation, rules: &RuleSet) -> Vec<String> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt { sound_id, .. } => {
                Some(sound_id.name_in(rules).to_ascii_uppercase())
            }
            _ => None,
        })
        .collect()
}

fn rules_with(rules_text: &str, art_text: &str) -> RuleSet {
    RuleSet::from_ini_with_fixed_art_and_sounds_for_test(
        &IniFile::from_str(&format!(
            "[General]\nDamageFireTypes=\n[AudioVisual]\nConditionYellow=50%\n\
             ConditionRed=25%\n{rules_text}"
        )),
        &IniFile::from_str(art_text),
        &["Thud", "Crunch"],
    )
    .unwrap()
}

/// A springy chunk with neither `BounceAnim=` nor `ExpireAnim=` plays its
/// `BounceSound=` on each bounce and its `ExpireSound=` once when it comes
/// down on dry ground (Firestorm 0x00414CE8, 0x004150D3).
#[test]
fn a_bouncing_chunk_plays_its_tiberian_sun_sounds() {
    let art_text = "[CHUNK]\nBouncer=yes\nElasticity=0.5\nMaxXYVel=0.5\nMinZVel=25.0\n\
                    BounceSound=Thud\nExpireSound=Crunch\nLoopEnd=15\nLoopCount=-1\n";
    let mut rules = rules_with("[Animations]\n0=CHUNK\n", art_text);
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(art_text));
    art.bind_anim_frame_count_for_test("CHUNK", 30);
    rules.install_art_fixture(art);
    let mut sim = Simulation::with_seed(3);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let chunk_type = sim.interner.intern("CHUNK");
    let center = crate::util::lepton::CELL_CENTER_LEPTON;
    let chunk = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor {
                loop_count: 1,
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(chunk_type, 8, 8, center, center, 0)
            },
            AnimWorldCoord {
                x: 8 * 256 + 128,
                y: 8 * 256 + 128,
                z: 20,
            },
        )
        .unwrap();
    let mut sounds = Vec::new();
    for frame in 1..400 {
        sim.session.set_binary_frame_for_test(frame);
        sim.sound_events.clear_for_test();
        sim.visit_anim(chunk, &rules, None, &mut FrameEffects::default())
            .unwrap();
        sounds.extend(played(&sim, &rules));
        if sim.substrate.pending_delete.contains_object(&chunk) {
            break;
        }
    }
    let (expire, bounces) = sounds.split_last().expect("the chunk comes down");
    assert_eq!(expire, "CRUNCH");
    assert!(!bounces.is_empty(), "it bounces before it rests");
    assert!(bounces.iter().all(|sound| sound == "THUD"), "{sounds:?}");
}

/// A voxel piece plays `BounceSound=` on each dry bounce and `ExpireSound=`
/// when its Duration runs out on dry ground (Firestorm 0x0065EC59,
/// 0x0065E482).
#[test]
fn a_voxel_piece_plays_its_tiberian_sun_sounds() {
    let rules = rules_with(
        "[VoxelAnims]\n0=ROCK\n\
         [ROCK]\nElasticity=0.5\nMinZVel=20\nMaxZVel=20\nMaxXYVel=0.5\nDuration=120\n\
         BounceSound=Thud\nExpireSound=Crunch\n",
        "",
    );
    let mut sim = Simulation::with_seed(3);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let rock = rules.voxel_anim_type_id_by_name("ROCK").unwrap();
    sim.emit_debris_voxel(
        rock,
        rules.voxel_anim_type(rock),
        None,
        glam::IVec3::new(8 * 256 + 128, 8 * 256 + 128, 0),
        true,
        &mut Vec::new(),
        &rules,
    )
    .unwrap();
    let id = sim.voxel_anims().ids()[0];
    let mut sounds = Vec::new();
    for _ in 0..400 {
        sim.sound_events.clear_for_test();
        sim.visit_voxel_anim(id, &rules, &mut FrameEffects::default())
            .unwrap();
        sounds.extend(played(&sim, &rules));
        if !sim
            .voxel_anims()
            .get(id)
            .is_some_and(|piece| piece.lifecycle().object_alive())
        {
            break;
        }
    }
    let (expire, bounces) = sounds.split_last().expect("the piece expires");
    assert_eq!(expire, "CRUNCH");
    assert!(!bounces.is_empty(), "it bounces before it expires");
    assert!(bounces.iter().all(|sound| sound == "THUD"), "{sounds:?}");
}
