//! SHP vehicle animation sequence builder.
//!
//! RA2 has a small number of SHP-based vehicles (Dolphin, Terror Drone, Giant Squid)
//! that use a tag-driven frame layout in art.ini rather than the arbitrary infantry
//! sequence system. Frame blocks are defined by count tags: `WalkFrames=6`,
//! `FiringFrames=4`, `StandingFrames=`.
//!
//! ## Block order
//! The unit type's INI reader derives each block's start frame from the counts
//! that precede it, and **walk occupies frame 0** — standing follows walk, and
//! firing follows standing. Each block holds `Facings × FramesPerFacing` frames,
//! all frames of slot 0 first, then all of slot 1, and so on.
//!
//! A firing-capable body always has a standing block even when art.ini omits
//! `StandingFrames=`, because the reader forces the count to 1 first. Skipping
//! it leaves the firing block one facing-block (8 frames) too early.
//!
//! Frame block 0 is the screen-north pose, which in cell terms is NW; the slots
//! then run clockwise. `FacingSlots::VehicleOctant` carries that conversion.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on rules/art_data, sim/animation.
//! - Does NOT depend on sim/ game logic, render/, or any game module.

use crate::rules::animation_sequence::{
    FacingSlots, LoopMode, SequenceDef, SequenceKind, SequenceSet, ShpVehicleCadence,
};
use crate::rules::art_data::ArtEntry;

/// Frames per facing the unit type constructor installs before art.ini is read.
/// An entry that declares `FiringFrames=` but no `WalkFrames=` still gets a walk
/// block of this size, which shifts the start of every block after it.
const DEFAULT_WALK_FRAMES: u16 = 12;

/// Standing frames a firing-capable body gets when art.ini declares none.
///
/// The unit type's constructor seeds `StandingFrames` to 0, but its INI reader
/// raises it to 1 for any body with `FiringFrames > 0` *before* using it as the
/// `StandingFrames=` default. No stock art section declares the key, so every
/// SHP vehicle that can fire carries this one-frame block — and it is what makes
/// the retail frame layouts come out exactly contiguous.
fn implicit_standing_frames(firing_frames: u16) -> u16 {
    if firing_frames > 0 { 1 } else { 0 }
}

/// Build a `SequenceSet` for an SHP vehicle from art.ini frame tags.
///
/// Reproduces the native start-frame derivation. All three stock SHP vehicles
/// (`DLPH`, `DRON`, `SQD`) declare `WalkFrames`/`FiringFrames` and nothing else,
/// so each gets an implicit one-frame standing block between walk and firing
/// (see [`implicit_standing_frames`]). The resulting layout is contiguous and
/// fills the retail files exactly — for `DRON`, walk 0..=47, standing 48..=55,
/// firing 56..=87, which is precisely the 88-frame body half of `DRON.SHP`.
pub fn build_shp_vehicle_sequences(art: &ArtEntry, cadence: ShpVehicleCadence) -> SequenceSet {
    let mut set = SequenceSet::new();
    set.set_shp_vehicle_cadence(cadence);
    let facings: u8 = art.shp_facings.max(1);

    let walk_frames: u16 = art.walk_frames.unwrap_or(DEFAULT_WALK_FRAMES);
    let firing_frames: u16 = art.firing_frames.unwrap_or(0);
    let standing_frames: u16 = art
        .standing_frames
        .unwrap_or_else(|| implicit_standing_frames(firing_frames));

    // Native start-frame defaults, computed in this order because each block
    // falls back to the previous block's start when its own count is zero.
    let start_walk_frame: u16 = 0;
    let start_stand_frame: u16 = if standing_frames > 0 {
        walk_frames * facings as u16
    } else {
        start_walk_frame
    };
    let start_firing_frame: u16 = if firing_frames > 0 {
        (walk_frames + standing_frames) * facings as u16
    } else {
        start_stand_frame
    };

    if walk_frames > 0 {
        set.insert(
            SequenceKind::Walk,
            SequenceDef {
                start_frame: start_walk_frame,
                frame_count: walk_frames,
                facings,
                facing_multiplier: walk_frames,
                // UnitClass SHP bodies do not use SequenceDef's relative
                // elapsed-frame clock. Foot's absolute body counter owns it.
                frame_delay: 1,
                normalized: false,
                completion_facing: None,
                loop_mode: LoopMode::Loop,
                facing_slots: FacingSlots::VehicleOctant,
            },
        );
    }

    // A body with no standing block at all (only reachable when it also cannot
    // fire) has its idle draw fall back to the *walk* block, holding the first
    // walk image of the current facing.
    let (stand_count, stand_stride): (u16, u16) = if standing_frames > 0 {
        (standing_frames, standing_frames)
    } else {
        (1, walk_frames)
    };
    set.insert(
        SequenceKind::Stand,
        SequenceDef {
            start_frame: start_stand_frame,
            frame_count: stand_count,
            facings,
            facing_multiplier: stand_stride,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::VehicleOctant,
        },
    );

    if firing_frames > 0 {
        set.insert(
            SequenceKind::Attack,
            SequenceDef {
                start_frame: start_firing_frame,
                frame_count: firing_frames,
                facings,
                facing_multiplier: firing_frames,
                // TODO(parity): the native firing counter advances one image per
                // two counter steps and is seeded counting down from
                // `FiringFrames * 2 - 1`. Reproducing that needs the fire-seeding
                // path, which lives outside this module, so the walk rate stands
                // in until then.
                frame_delay: 1,
                normalized: false,
                completion_facing: None,
                loop_mode: LoopMode::TransitionTo(SequenceKind::Stand),
                facing_slots: FacingSlots::VehicleOctant,
            },
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CADENCE: ShpVehicleCadence = ShpVehicleCadence {
        walk_rate: 3,
        idle_rate: 1,
    };

    fn make_art_entry(walk: Option<u16>, firing: Option<u16>) -> ArtEntry {
        ArtEntry {
            image: None,
            cameo: None,
            alt_cameo: None,
            new_theater: false,
            theater: false,
            scorch: false,
            crater: false,
            force_big_craters: false,
            frame_width: 30,
            frame_height: 30,
            voxel: false,
            authored_voxel: None,
            turret_offset: 0,
            y_draw_offset: 0,
            x_draw_offset: 0,
            building_anims: Vec::new(),
            building_anim_power: [Default::default(); 21],
            building_gate_stages: 9,
            building_body_ranges: [[0, 1, 0]; 4],
            foundation: None,
            to_overlay: None,
            bib_shape: None,
            palette: None,
            sequence: None,
            crawls: false,
            primary_fire_flh: Default::default(),
            secondary_fire_flh: Default::default(),
            elite_primary_fire_flh: None,
            elite_secondary_fire_flh: None,
            primary_fire_pixel_offset: None,
            secondary_fire_pixel_offset: None,
            primary_fire_dual_offset: false,
            is_anim_delayed_fire: false,
            silo_damage: false,
            delayed_fire_delay: 0,
            walk_frames: walk,
            firing_frames: firing,
            standing_frames: None,
            shp_facings: 8,
            fire_up: 0,
            fire_prone: 0,
            secondary_fire: 0,
            secondary_prone: 0,
            report: None,
            start_sound: None,
            extra_light: 0,
            terrain_palette: false,
            queueing_cell: [0, 0],
            pads: Vec::new(),
            damage_fire_offsets: Vec::new(),
            height: 0,
            muzzle_flash_positions: Vec::new(),
            add_occupy: [None; crate::rules::object_type::HIDDEN_OCCUPY_SLOT_COUNT],
            remove_occupy: [None; crate::rules::object_type::HIDDEN_OCCUPY_SLOT_COUNT],
            deploy_frames: None,
            undeploy_frames: None,
            deployed_fire_frames: None,
            z_shape_point_move: (0, 0),
            normal_z_adjust: 0,
        }
    }

    /// Last frame the derived layout occupies, i.e. the first frame of the
    /// death block. For a body with no death frames this must land exactly on
    /// the retail file's body-half boundary — the layout is contiguous, so any
    /// orphan frames left over mean a block was mis-sized.
    fn body_frames_used(set: &SequenceSet, facings: u16) -> u16 {
        let block_end = |kind: &SequenceKind| {
            set.get(kind)
                .map(|d| d.start_frame + d.facing_multiplier * facings)
                .unwrap_or(0)
        };
        block_end(&SequenceKind::Walk)
            .max(block_end(&SequenceKind::Stand))
            .max(block_end(&SequenceKind::Attack))
    }

    #[test]
    fn test_dolphin_sequences() {
        // DLPH: WalkFrames=6, FiringFrames=6, 8 facings, no StandingFrames key.
        // FiringFrames > 0 forces StandingFrames to 1, so:
        //   StartWalkFrame=0, StartStandFrame=6*8=48, StartFiringFrame=(6+1)*8=56.
        // Layout: walk 0..=47, stand 48..=55, firing 56..=103. DLPH.SHP has 232
        // frames = 116 body, leaving 104..=115 for the death block — which is
        // exactly the native StartDeathFrame of (6+6+1)*8 = 104.
        let art = make_art_entry(Some(6), Some(6));
        let set = build_shp_vehicle_sequences(&art, TEST_CADENCE);
        assert_eq!(set.shp_vehicle_cadence(), Some(TEST_CADENCE));

        let walk = set.get(&SequenceKind::Walk).expect("Walk");
        assert_eq!(walk.start_frame, 0, "walk block must occupy frame 0");
        assert_eq!(walk.frame_count, 6);
        assert_eq!(walk.facing_multiplier, 6);
        assert_eq!(walk.facings, 8);
        assert_eq!(walk.frame_delay, 1);

        let stand = set.get(&SequenceKind::Stand).expect("Stand");
        assert_eq!(stand.start_frame, 48, "WalkFrames * Facings");
        assert_eq!(stand.frame_count, 1);
        assert_eq!(stand.facing_multiplier, 1);
        assert_eq!(stand.frame_delay, 1);

        let attack = set.get(&SequenceKind::Attack).expect("Attack");
        assert_eq!(
            attack.start_frame, 56,
            "(WalkFrames + StandingFrames) * Facings"
        );
        assert_eq!(attack.frame_count, 6);
        assert_eq!(attack.facing_multiplier, 6);

        assert_eq!(body_frames_used(&set, 8), 104, "native StartDeathFrame");
    }

    #[test]
    fn test_terror_drone_sequences() {
        // DRON: WalkFrames=6, FiringFrames=4 → stand at 6*8=48, firing at
        // (6+1)*8=56. Layout is walk 0..=47, stand 48..=55, firing 56..=87,
        // which fills DRON.SHP's 88-frame body half exactly with nothing left
        // over. Dropping the standing block strands frames 80..=87.
        let art = make_art_entry(Some(6), Some(4));
        let set = build_shp_vehicle_sequences(&art, TEST_CADENCE);

        assert_eq!(set.get(&SequenceKind::Walk).expect("Walk").start_frame, 0);
        assert_eq!(
            set.get(&SequenceKind::Stand).expect("Stand").start_frame,
            48
        );
        let attack = set.get(&SequenceKind::Attack).expect("Attack");
        assert_eq!(attack.start_frame, 56);
        assert_eq!(attack.frame_count, 4);

        // 176 retail frames = 88 body + 88 shadow, and the body is fully used.
        assert_eq!(body_frames_used(&set, 8), 88);
    }

    #[test]
    fn test_squid_sequences() {
        // SQD: WalkFrames=20, FiringFrames=16 → stand at 160, firing at 21*8=168.
        // Layout runs 0..=295, and SQD.SHP is 296 frames with no shadow half.
        let art = make_art_entry(Some(20), Some(16));
        let set = build_shp_vehicle_sequences(&art, TEST_CADENCE);

        assert_eq!(set.get(&SequenceKind::Walk).expect("Walk").start_frame, 0);
        assert_eq!(
            set.get(&SequenceKind::Stand).expect("Stand").start_frame,
            160
        );
        assert_eq!(
            set.get(&SequenceKind::Attack).expect("Attack").start_frame,
            168
        );

        assert_eq!(body_frames_used(&set, 8), 296);
    }

    #[test]
    fn test_explicit_standing_frames_overrides_the_forced_one() {
        // No stock SHP vehicle declares StandingFrames, but an explicit value
        // replaces the forced 1 and widens the block between walk and firing.
        let mut art = make_art_entry(Some(6), Some(4));
        art.standing_frames = Some(2);
        let set = build_shp_vehicle_sequences(&art, TEST_CADENCE);

        let stand = set.get(&SequenceKind::Stand).expect("Stand");
        assert_eq!(stand.start_frame, 48, "WalkFrames * Facings");
        assert_eq!(stand.frame_count, 2);
        assert_eq!(stand.facing_multiplier, 2);

        let attack = set.get(&SequenceKind::Attack).expect("Attack");
        assert_eq!(attack.start_frame, 64, "(6 + 2) * 8");
    }

    #[test]
    fn test_non_firing_body_gets_no_standing_block() {
        // The standing count is only forced to 1 for bodies that can fire. With
        // FiringFrames absent it stays 0, standing falls back to the walk block,
        // and no Attack sequence is emitted.
        let art = make_art_entry(Some(4), None);
        let set = build_shp_vehicle_sequences(&art, TEST_CADENCE);

        let stand = set.get(&SequenceKind::Stand).expect("Stand");
        assert_eq!(stand.start_frame, 0);
        assert_eq!(stand.frame_count, 1);
        assert_eq!(stand.facing_multiplier, 4, "strides by WalkFrames");
        assert!(set.get(&SequenceKind::Walk).is_some());
        assert!(set.get(&SequenceKind::Attack).is_none());
    }

    #[test]
    fn test_walk_frames_absent_uses_native_default() {
        // WalkFrames absent keeps the constructor's 12, so standing lands at
        // 12*8 and firing at (12+1)*8 rather than at 0.
        let art = make_art_entry(None, Some(4));
        let set = build_shp_vehicle_sequences(&art, TEST_CADENCE);

        let walk = set.get(&SequenceKind::Walk).expect("Walk");
        assert_eq!(walk.frame_count, 12);
        assert_eq!(
            set.get(&SequenceKind::Stand).expect("Stand").start_frame,
            96
        );
        assert_eq!(
            set.get(&SequenceKind::Attack).expect("Attack").start_frame,
            104
        );
    }
}
