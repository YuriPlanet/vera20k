//! Retained UnitType ART reader and SHP animation catalog projection.
//!
//! UnitType747620 reads fixed ART on each admitted Rules body. Its frame counts,
//! current defaults and resolved start offsets survive later Rules passes. The
//! process registry owns those fields; the immutable animation catalog projects
//! them for both ordinary SHP units and voxel units drawing a terrain disguise.
//!
//! On the first ordinary read, walk starts at zero, standing follows walk and
//! firing follows standing. Explicit starts and retained offsets can change that
//! layout. Original reader/postpass controls are in `mirage_disguise.json`.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on rules/art_data, sim/animation.
//! - Does NOT depend on sim/ game logic, render/, or any game module.

use crate::rules::animation_sequence::{
    FacingSlots, LoopMode, SequenceDef, SequenceKind, SequenceSet, ShpVehicleCadence,
};
use crate::rules::art_data::ArtEntry;
use crate::rules::ini_parser::IniSection;

/// UnitType7470D0/747620's retained ART layout, owned by the process-resident
/// UnitType registry. ART is fixed, but Image/Turret and current reader defaults
/// can change between reached Rules passes. The final image metadata alone
/// cannot reconstruct this state. Native controls: mirage_disguise.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub(crate) struct UnitShpReadState {
    walk_frames: i8,
    firing_frames: i8,
    standing_frames: i32,
    death_frames: i32,
    death_frame_rate: i32,
    facings: i32,
    start_stand_frame: i32,
    start_walk_frame: i32,
    start_firing_frame: i32,
    start_death_frame: i32,
    max_death_counter: i32,
}

impl Default for UnitShpReadState {
    fn default() -> Self {
        // Original constructor747167..7471B2. Start fields are sentinels,
        // resolved only by a reached UnitType reader, not asset installation.
        Self {
            walk_frames: 12,
            firing_frames: 0,
            standing_frames: 0,
            death_frames: 0,
            death_frame_rate: 1,
            facings: 8,
            start_stand_frame: -1,
            start_walk_frame: -1,
            start_firing_frame: -1,
            start_death_frame: -1,
            max_death_counter: -1,
        }
    }
}

impl UnitShpReadState {
    pub(crate) fn read_pass(&mut self, art: &IniSection, has_turret: bool) {
        // Original7477F1/74780F store AL, with each signed byte as its
        // ReadInteger default. These are not nonnegative u16 frame counts.
        self.walk_frames = art.read_int("WalkFrames", i32::from(self.walk_frames)) as i8;
        self.firing_frames = art.read_int("FiringFrames", i32::from(self.firing_frames)) as i8;
        if self.firing_frames > 0 {
            self.standing_frames = 1;
        }
        self.standing_frames = art.read_int("StandingFrames", self.standing_frames);
        self.death_frames = art.read_int("DeathFrames", self.death_frames);
        self.death_frame_rate = art.read_int("DeathFrameRate", self.death_frame_rate).max(1);
        // Original747930..74795C changes the current default, not the value
        // used on every other pass. A later turret can therefore retain1.
        if self.firing_frames == 0 && !has_turret {
            self.facings = 1;
        }
        self.facings = art.read_int("Facings", self.facings);

        // Original747967..747A11 fills only the -1 sentinels. All arithmetic
        // is wrapping signed dword arithmetic, before exact Start* reads.
        if self.start_walk_frame == -1 {
            self.start_walk_frame = 0;
        }
        if self.start_stand_frame == -1 {
            self.start_stand_frame = if self.standing_frames == 0 {
                self.start_walk_frame
            } else {
                i32::from(self.walk_frames).wrapping_mul(self.facings)
            };
        }
        if self.start_firing_frame == -1 {
            self.start_firing_frame = if self.firing_frames == 0 {
                self.start_stand_frame
            } else {
                i32::from(self.walk_frames)
                    .wrapping_add(self.standing_frames)
                    .wrapping_mul(self.facings)
            };
        }
        if self.start_death_frame == -1 {
            self.start_death_frame = if self.death_frames == 0 {
                -1
            } else {
                i32::from(self.firing_frames)
                    .wrapping_add(i32::from(self.walk_frames))
                    .wrapping_add(1)
                    .wrapping_mul(self.facings)
            };
            self.max_death_counter = self.start_death_frame.wrapping_add(self.death_frames);
        }
        self.start_stand_frame = art.read_int("StartStandFrame", self.start_stand_frame);
        self.start_walk_frame = art.read_int("StartWalkFrame", self.start_walk_frame);
        self.start_firing_frame = art.read_int("StartFiringFrame", self.start_firing_frame);
        self.start_death_frame = art.read_int("StartDeathFrame", self.start_death_frame);
        self.max_death_counter = art.read_int("MaxDeathCounter", self.max_death_counter);
    }
}

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

/// Project a retained Unit layout into the existing immutable frame catalog.
///
/// Unit callers supply the process registry's state. `None` preserves the
/// existing Aircraft metadata projection, whose reader is outside this chain.
/// Stock DLPH/DRON/SQD retain their ordinary walk/stand/fire blocks, while MGTK's
/// actual UnitType layout also serves its observer-selected terrain image.
pub(crate) fn build_shp_vehicle_sequences(
    art: &ArtEntry,
    cadence: ShpVehicleCadence,
    retained: Option<&UnitShpReadState>,
) -> SequenceSet {
    let mut set = SequenceSet::new();
    set.set_shp_vehicle_cadence(cadence);
    let (
        facings,
        walk_frames,
        firing_frames,
        standing_frames,
        start_walk_frame,
        start_stand_frame,
        start_firing_frame,
    ) = if let Some(retained) = retained {
        let (
            Ok(facings),
            Ok(walk),
            Ok(fire),
            Ok(stand),
            Ok(start_walk),
            Ok(start_stand),
            Ok(start_fire),
        ) = (
            u8::try_from(retained.facings),
            u16::try_from(retained.walk_frames),
            u16::try_from(retained.firing_frames),
            u16::try_from(retained.standing_frames),
            u16::try_from(retained.start_walk_frame),
            u16::try_from(retained.start_stand_frame),
            u16::try_from(retained.start_firing_frame),
        )
        else {
            // The native reader retains signed/wide authored values exactly.
            // This existing u16 atlas projection cannot represent arbitrary
            // negative/wide layouts; do not silently clamp them into a valid
            // unrelated frame. Those non-stock layouts are not this corpus's
            // rendering claim. The raw state still participates in rules hash.
            return set;
        };
        (
            facings,
            walk,
            fire,
            stand,
            start_walk,
            start_stand,
            start_fire,
        )
    } else {
        // Preserve the existing Aircraft metadata projection. Unit callers
        // always supply their process-resident state; an asset bind cannot
        // execute a second UnitType ReadINI or reset retained start offsets.
        let facings = art.shp_facings.max(1);
        let walk = art.walk_frames.unwrap_or(DEFAULT_WALK_FRAMES);
        let fire = art.firing_frames.unwrap_or(0);
        let stand = art
            .standing_frames
            .unwrap_or_else(|| implicit_standing_frames(fire));
        let start_stand = if stand > 0 {
            walk * u16::from(facings)
        } else {
            0
        };
        let start_fire = if fire > 0 {
            (walk + stand) * u16::from(facings)
        } else {
            start_stand
        };
        (facings, walk, fire, stand, 0, start_stand, start_fire)
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
    let (stand_count, stand_stride, stand_start): (u16, u16, u16) = if standing_frames > 0 {
        (standing_frames, standing_frames, start_stand_frame)
    } else {
        // Unit73C6E9..73C71D uses StartWalkFrame when StandFrames is zero,
        // even if an authored StartStandFrame differs from its default.
        (1, walk_frames, start_walk_frame)
    };
    set.insert(
        SequenceKind::Stand,
        SequenceDef {
            start_frame: stand_start,
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
    use crate::rules::ini_parser::IniFile;
    use crate::rules::mirage_disguise_tests::cached_sections;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    const TEST_CADENCE: ShpVehicleCadence = ShpVehicleCadence {
        walk_rate: 3,
        idle_rate: 1,
    };

    fn native_corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/mirage_disguise.json",
        ))
        .unwrap()
    }

    #[test]
    fn mirage_unit_art_reader_matches_original_retained_controls() {
        let corpus = native_corpus();
        let controls = corpus["unit_art_controls"].as_array().unwrap();
        assert_eq!(controls.len(), 17);
        for row in controls {
            let prior = &row["input"]["prior"];
            assert_eq!(prior, &row["before"], "declared native supplied boundary");
            let mut state: UnitShpReadState = serde_json::from_value(prior.clone()).unwrap();
            let art = cached_sections(&row["input"]["sections"]);
            state.read_pass(
                art.section_or_empty("RTNK"),
                prior["turret"].as_i64().unwrap() != 0,
            );
            let expected: UnitShpReadState = serde_json::from_value(row["after"].clone()).unwrap();
            assert_eq!(state, expected, "{}", row["name"]);
        }
    }

    #[test]
    fn mirage_unit_art_constructor_and_ordered_layers_match_original() {
        let corpus = native_corpus();
        let initialization = &corpus["initialization"];
        let expected: UnitShpReadState =
            serde_json::from_value(initialization["unit_art_constructor"].clone()).unwrap();
        assert_eq!(UnitShpReadState::default(), expected);
        let art = cached_sections(&initialization["art_input"]["sections"]);
        // The original fixture constructed MGTK before its recorded reads.
        // A registry-only pass produces that same no-body boundary in VERA.
        let mut layers = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=MGTK\n"));
        for source in initialization["layers"].as_array().unwrap() {
            if source["absent"] == true {
                continue;
            }
            let kind = match source["file"].as_str().unwrap() {
                "RULESMD.INI" => RulesLayerKind::RulesMd,
                "LANGRULE.INI" => RulesLayerKind::LangRule,
                "MPBattleMD.ini" => RulesLayerKind::GameMode,
                "XMP03T4.MAP" => RulesLayerKind::Scenario,
                other => panic!("unknown recorded Unit rules source {other}"),
            };
            layers.push(kind, cached_sections(&source["sections"]));
            let processed = layers.process_with_fixed_art(&art).unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            let expected: UnitShpReadState =
                serde_json::from_value(source["unit_art"].clone()).unwrap();
            assert_eq!(
                rules.unit_shp_read_state("MGTK"),
                Some(&expected),
                "{}",
                source["file"]
            );
            rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
            assert_eq!(
                rules.unit_shp_read_state("MGTK"),
                Some(&expected),
                "asset installation cannot execute another Unit ART read"
            );
            assert_eq!(
                i32::from(
                    rules
                        .animation_sequence("MGTK")
                        .unwrap()
                        .get(&SequenceKind::Stand)
                        .unwrap()
                        .facings
                ),
                expected.facings,
                "catalog uses retained type layout"
            );
        }
    }

    #[test]
    fn mirage_unit_art_retains_defaults_across_turret_image_and_absent_body_passes() {
        let art = IniFile::from_str("[FIRST]\nVoxel=yes\n[SECOND]\nVoxel=yes\n");
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nImage=FIRST\nTurret=no\n",
        ));
        let first = layers.process_with_fixed_art(&art).unwrap();
        let first_state = *first.unit_shp_read_states().next().unwrap().1;
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[UNIT]\nImage=SECOND\nTurret=yes\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Other]\nKey=1\n"),
        );
        let final_pass = layers.process_with_fixed_art(&art).unwrap();
        assert_eq!(
            final_pass.unit_shp_read_states().next().unwrap().1,
            &first_state,
            "later turret and omitted ART keys retain current defaults; absent Rules body skips ART"
        );
    }

    fn build_test(art: &ArtEntry) -> SequenceSet {
        let mut source = format!("[IMAGE]\nFacings={}\n", art.shp_facings);
        for (key, value) in [
            ("WalkFrames", art.walk_frames),
            ("FiringFrames", art.firing_frames),
            ("StandingFrames", art.standing_frames),
        ] {
            if let Some(value) = value {
                source.push_str(&format!("{key}={value}\n"));
            }
        }
        let ini = IniFile::from_str(&source);
        let mut native = UnitShpReadState::default();
        native.read_pass(ini.section_or_empty("IMAGE"), false);
        build_shp_vehicle_sequences(art, TEST_CADENCE, Some(&native))
    }

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
            turret_offset: 0,
            y_draw_offset: 0,
            x_draw_offset: 0,
            building_anims: Vec::new(),
            building_anim_power: [Default::default(); 21],
            building_gate_stages: 9,
            building_body_ranges: [[0, 1, 0]; 4],
            buildup: None,
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
            numbered_weapon_flh: [Default::default(); crate::rules::object_type::WEAPON_SLOT_COUNT],
            elite_numbered_weapon_flh: [Default::default();
                crate::rules::object_type::WEAPON_SLOT_COUNT],
            alternate_flh: Default::default(),
            second_spawn_offset: Default::default(),
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
        let set = build_test(&art);
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
        let set = build_test(&art);

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
        let set = build_test(&art);

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
        let set = build_test(&art);

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
        let set = build_test(&art);

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
        let set = build_test(&art);

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
