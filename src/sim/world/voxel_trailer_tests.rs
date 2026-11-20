//! Phobos `Trailer.SpawnDelay` / `TrailerAnim` on a VoxelAnimType:
//! `VoxelAnimClass_AI_Trailer @ 0x74A70E` (`Ext/VoxelAnim/Hooks.cpp:47-67`)
//! replaces the stock trailer block with a restartable spawn timer.

use glam::IVec3;

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::voxel_anim_type::VoxelAnimType;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

fn puff_anims(sim: &Simulation) -> Vec<&crate::sim::anim_class::AnimObject> {
    sim.anims()
        .filter(|(_, anim)| sim.interner().resolve(anim.type_id()) == "PUFF")
        .map(|(_, anim)| anim)
        .collect()
}

fn scene(delay_key: &str, with_owner: bool) -> (Simulation, RuleSet, u64, Option<InternedId>) {
    let art = IniFile::from_str("[PUFF]\nImage=PUFF\nTransparent=yes\n");
    let animations = "[Animations]\n0=PUFF\n";
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&format!(
            "[VoxelAnims]\n0=PIECE\n[PIECE]\nDuration=100\nTrailerAnim=PUFF\n{delay_key}{animations}"
        )),
        &art,
    )
    .expect("trailer rules");
    rules.install_art_fixture(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_anim_frame_count_for_test("PUFF", 10);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let type_id = rules
        .voxel_anim_type_id_by_name("PIECE")
        .expect("PIECE is listed in [VoxelAnims]");
    let kind = rules.voxel_anim_type(type_id);
    assert_eq!(
        kind.trailer_anim.as_deref(),
        Some("PUFF"),
        "the fixture resolves TrailerAnim through the ordered Rules owner"
    );
    let house = with_owner.then(|| sim.intern("Americans"));
    sim.emit_debris_voxel(
        type_id,
        kind,
        house,
        IVec3::new(10 * 256, 10 * 256, 0),
        true,
        &mut Vec::new(),
        &rules,
    )
    .expect("the piece stays inside the launch domain");
    let id = sim.substrate.voxel_anims.ids()[0];
    (sim, rules, id, house)
}

fn visit_frames(mut sim: Simulation, rules: &RuleSet, id: u64, end: u32) -> Vec<usize> {
    let mut counts = Vec::new();
    for frame in 0..end {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_voxel_anim(id, rules, FrameEffects::default())
            .expect("the flight visit stays in domain");
        counts.push(puff_anims(&sim).len());
    }
    counts
}

/// The key reads as `Valueable<int>` with the constructor's 2, and a written
/// value replaces it.
#[test]
fn trailer_spawn_delay_reads_with_a_two_frame_default() {
    let read = |text: &str| {
        let ini = IniFile::from_str(text);
        VoxelAnimType::from_ini_section("PIECE", ini.section("PIECE").expect("section"))
    };
    assert_eq!(read("[PIECE]\nDuration=100\n").trailer_spawn_delay, 2);
    assert_eq!(
        read("[PIECE]\nTrailer.SpawnDelay=5\n").trailer_spawn_delay,
        5
    );
}

/// `0x74A70E`: the timer starts expired, so the first flight visit spawns;
/// each spawn restarts it for `Trailer.SpawnDelay` frames, and the piece's
/// owner house is installed on every trailer.
#[test]
fn the_trailer_spawns_every_delay_frames() {
    let (sim, rules, id, _house) = scene("Trailer.SpawnDelay=3\n", true);
    let counts = visit_frames(sim, &rules, id, 7);
    assert_eq!(counts, vec![1, 1, 1, 2, 2, 2, 3], "spawns at 0, 3 and 6");
}

/// An unset key keeps the two-frame cadence.
#[test]
fn the_unset_key_spawns_every_two_frames() {
    let (sim, rules, id, _house) = scene("", true);
    let counts = visit_frames(sim, &rules, id, 6);
    assert_eq!(counts, vec![1, 1, 2, 2, 3, 3], "spawns at 0, 2 and 4");
}

/// A zero delay expires on every visit, so each flight frame spawns.
#[test]
fn a_zero_delay_spawns_on_every_visit() {
    let (sim, rules, id, _house) = scene("Trailer.SpawnDelay=0\n", true);
    let counts = visit_frames(sim, &rules, id, 4);
    assert_eq!(counts, vec![1, 2, 3, 4]);
}

/// `SetAnimOwnerHouseKind(OwnerHouse, nullptr, false, true)` with the null
/// owner the bullet-debris producer passes: the trailer stays unowned.
#[test]
fn an_unowned_piece_leaves_the_trailer_unowned() {
    let (mut sim, rules, id, house) = scene("", false);
    assert_eq!(house, None, "the unowned scene passes no house");
    sim.session.set_binary_frame_for_test(0);
    sim.visit_voxel_anim(id, &rules, FrameEffects::default())
        .expect("the flight visit stays in domain");
    let trailers = puff_anims(&sim);
    assert_eq!(trailers.len(), 1);
    assert_eq!(trailers[0].owner_house(), None);
}

/// `SetAnimOwnerHouseKind(OwnerHouse, nullptr, false, true)`: every trailer
/// carries the piece's owner house.
#[test]
fn the_trailer_inherits_the_piece_owner_house() {
    let (mut sim, rules, id, house) = scene("", true);
    let house = house.expect("owned scene");
    sim.session.set_binary_frame_for_test(0);
    sim.visit_voxel_anim(id, &rules, FrameEffects::default())
        .expect("the flight visit stays in domain");
    let trailers = puff_anims(&sim);
    assert_eq!(trailers.len(), 1);
    assert_eq!(trailers[0].owner_house(), Some(house));
}
