//! Explicit conversions between audited player-save layouts.

use super::{
    GameSnapshot, GameSnapshotHeader, GameSnapshotPreamble, SNAPSHOT_ENVELOPE_VERSION,
    SNAPSHOT_PRODUCT_MAGIC, SnapshotError, save_codec, snapshot_layout,
};

const MIGRATION_ENVELOPE_VERSION: u32 = 2;
// The stock desktop v0.3.8 and v0.3.9 writers share this layout.
const V0_3_8_AND_0_3_9: u32 = 0xFD30_A463;
const V0_4_0: u32 = 0x9BB2_0D64;
// The next desktop writer stores an AlphaShape's Anim expiry listener.
const ALPHA_SHAPE_LISTENER: u32 = 0xFD54_FFFF;
// d00f8183c added the Ares alternate-theater flag to each retained object image.
const ALTERNATE_THEATER_ART: u32 = 0xCB54_5137;
// 08e1424eb (#5002) stores a high bridge's fixed deck level, and 26b2f9464
// Fly's Ares EMP spin. Older worlds hold neither: no fixed deck, so a converted
// bridge keeps its saved per-cell height until it is stamped again, and no
// spin, whose rate is read only while spinning.
const BRIDGE_DECK_AND_FLY_SPIN: u32 = 0xCADF_6067;

/// The rules digest a converted save carries: the loader takes the rules the
/// game has instead of proving the digest the old writer computed, whose
/// recipe changes with most rules ports. The world keeps its saved processed
/// rules; the next ordinary save records the current digest.
pub const CONVERTED_RULES_HASH: u64 = 0;

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("this build cannot write the audited migration target (layout {0:#010x})")]
    UnsupportedTarget(u32),
    #[error("the save uses an unsupported source layout {0:#010x}")]
    UnsupportedSource(u32),
    #[error("the save is already in the current layout")]
    AlreadyCurrent,
    #[error("unexpected bytes after the saved world")]
    TrailingBytes,
    #[error("entity {0} has an invalid saved map tunnel or path cursor")]
    InvalidTunnelTransit(u64),
    #[error(transparent)]
    HeaderOrState(#[from] SnapshotError),
    #[error(transparent)]
    Decode(#[from] save_codec::Error),
}

/// Whether the header names a supported source for this build. Eligibility
/// only uses metadata. Conversion must still validate the saved world.
pub fn supports(header: &GameSnapshotHeader) -> bool {
    header.product_magic == SNAPSHOT_PRODUCT_MAGIC
        && header.envelope_version == MIGRATION_ENVELOPE_VERSION
        && previous_layout(header.layout).is_some()
        && header.compatibility().matches_build()
        && SNAPSHOT_ENVELOPE_VERSION == MIGRATION_ENVELOPE_VERSION
        && snapshot_layout() == BRIDGE_DECK_AND_FLY_SPIN
}

fn previous_layout(source: u32) -> Option<save_codec::PreviousLayout> {
    const IMAGE_FLAGS: (&str, &[&str]) = ("ObjectImageState", &["alternate_theater_art"]);
    const ALPHA_LISTENER: (&str, &[&str]) = ("AlphaShape", &["anim_listener"]);
    const DECK: (&str, &[&str]) = ("BridgeCellFacts", &["fixed_deck_level"]);
    // The pending cell was a bare (x, y) point, the struct's first field.
    const PENDING_DECK: (&str, &[&str]) = ("PendingBridgeCell", &["corrected_deck_level"]);
    const SPIN: (&str, &[&str]) = ("FlyRuntime", &["spinning", "spin_rate"]);
    let absent_fields: &'static [_] = match source {
        V0_3_8_AND_0_3_9 | V0_4_0 => &[IMAGE_FLAGS, ALPHA_LISTENER, DECK, PENDING_DECK, SPIN],
        ALPHA_SHAPE_LISTENER => &[IMAGE_FLAGS, DECK, PENDING_DECK, SPIN],
        ALTERNATE_THEATER_ART => &[DECK, PENDING_DECK, SPIN],
        _ => return None,
    };
    Some(save_codec::PreviousLayout {
        absent_fields,
        // v0.4.0 appended Mech without changing the earlier discriminants.
        absent_variant: (source == V0_3_8_AND_0_3_9).then_some(("LocomotorKind", "Mech")),
    })
}

/// Decode the entire historical world, then publish only the audited wire
/// changes. Older AlphaShapes belonged to Technos and never listened for
/// Anim expiry. Before alternate-theater art support, each retained image had
/// only its stock Arctic setting and latch. Keep both exactly as saved.
pub fn migrate(bytes: &mut Vec<u8>) -> Result<(), MigrationError> {
    let target = snapshot_layout();
    if SNAPSHOT_ENVELOPE_VERSION != MIGRATION_ENVELOPE_VERSION || target != BRIDGE_DECK_AND_FLY_SPIN
    {
        return Err(MigrationError::UnsupportedTarget(target));
    }

    let preamble = GameSnapshot::read_preamble(bytes)?;
    GameSnapshot::validate_version(preamble.envelope_version)?;
    if preamble.layout == target {
        return Err(MigrationError::AlreadyCurrent);
    }
    let previous = previous_layout(preamble.layout)
        .ok_or(MigrationError::UnsupportedSource(preamble.layout))?;
    GameSnapshot::read_compatible_header(bytes)?;

    let mut missing_fields = Vec::new();
    {
        let (mut snapshot, remainder): (GameSnapshot, _) =
            save_codec::deserialize_previous(bytes, previous, &mut missing_fields)?;
        if !remainder.is_empty() {
            return Err(MigrationError::TrailingBytes);
        }
        GameSnapshot::validate_admitted(&mut snapshot)?;
        let resources = snapshot.require_player_resources()?;
        if preamble.layout == V0_3_8_AND_0_3_9 {
            for (id, entity) in snapshot.sim.substrate.entities.iter_sorted() {
                if let Some(transit) = entity.low_bridge_tube_state
                    && !valid_old_transit(transit, resources.terrain())
                {
                    return Err(MigrationError::InvalidTunnelTransit(id));
                }
            }
        }
    }

    let target_preamble = save_codec::serialize(&GameSnapshotPreamble {
        layout: target,
        ..preamble
    })?;
    // Offsets are produced by the successful sequential decoder. Back-fill
    // in a single pass so each original byte moves at most once.
    let added: usize = missing_fields.iter().map(|&(_, width)| width).sum();
    let mut end = bytes.len();
    bytes
        .try_reserve_exact(added)
        .map_err(|error| save_codec::Error::Custom(error.to_string()))?;
    bytes.resize(end + added, 0);
    // `shift` is the width inserted at or before the current field.
    let mut shift = added;
    for &(offset, width) in missing_fields.iter().rev() {
        bytes.copy_within(offset..end, offset + shift);
        shift -= width;
        bytes[offset + shift..offset + shift + width].fill(0);
        end = offset;
    }
    bytes
        .get_mut(..target_preamble.len())
        .ok_or(save_codec::Error::UnexpectedEnd)?
        .copy_from_slice(&target_preamble);
    Ok(())
}

/// Convert the world, then replace its variable-length browser description.
/// Keep all bytes following the metadata, including state the runtime resets
/// when a player loads the result.
pub fn migrate_named(mut bytes: Vec<u8>, description: &str) -> Result<Vec<u8>, MigrationError> {
    migrate(&mut bytes)?;
    let (mut header, world): (GameSnapshotHeader, _) =
        save_codec::deserialize_with_remainder(&bytes)?;
    let header_len = bytes.len() - world.len();
    header.description = description.to_owned();
    header.rules_hash = CONVERTED_RULES_HASH;
    let replacement = save_codec::serialize(&header)?;
    // A longer name needs only a few extra bytes, even for a very large world.
    bytes.reserve_exact(replacement.len().saturating_sub(header_len));
    bytes.splice(..header_len, replacement);
    Ok(bytes)
}

// v0.3.8 and v0.3.9's tube_word admitted only IDs 0..=127 and explicit
// nonempty paths. UnitClass::TubeMovement (0x007359F0) and its infantry
// counterpart (0x0051B350) still consume the same cursor and target XYZ.
// A blocked exit legitimately holds cursor == path length between turns.
fn valid_old_transit(
    state: crate::sim::movement::tube_movement::LowBridgeTubeMovementState,
    terrain: &crate::map::resolved_terrain::SavedTerrainTemplate,
) -> bool {
    state.tube_id().0 <= i8::MAX as u16
        && terrain.tube(state.tube_id()).is_some_and(|tube| {
            tube.source == crate::map::tube_facts::TubeSource::ExplicitMap
                && tube.path_len() > 0
                && usize::from(state.cursor()) <= tube.path_len()
        })
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use crate::sim::world::Simulation;

    #[test]
    fn old_tunnel_cursor_may_wait_at_the_exit_but_not_pass_it() {
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_loader_clear_cell};
        use crate::map::tube_facts::{TubeFact, TubeId};
        use crate::sim::components::DriveCoord;
        use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

        let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
            2,
            1,
            vec![test_loader_clear_cell(0, 0), test_loader_clear_cell(1, 0)],
            vec![TubeFact::explicit((0, 0), (1, 0), 2, vec![2])],
        )
        .capture_saved_template()
        .unwrap();
        let target = DriveCoord::cell(1, 0, 0);
        let state = |id, cursor| LowBridgeTubeMovementState::for_test(TubeId(id), cursor, target);
        assert!(valid_old_transit(state(0, 1), &terrain));
        assert!(!valid_old_transit(state(0, 2), &terrain));
        assert!(!valid_old_transit(state(1, 0), &terrain));
    }

    #[test]
    fn migration_leaves_unsupported_and_invalid_snapshots_byte_exact() {
        let wrong_layout = V0_3_8_AND_0_3_9 ^ 1;
        let mut bytes = save_codec::serialize(&GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: wrong_layout,
        })
        .unwrap();
        let original = bytes.clone();
        let error = migrate(&mut bytes).unwrap_err();
        assert_eq!(bytes, original);
        if SNAPSHOT_ENVELOPE_VERSION != MIGRATION_ENVELOPE_VERSION
            || snapshot_layout() != BRIDGE_DECK_AND_FLY_SPIN
        {
            assert!(matches!(error, MigrationError::UnsupportedTarget(_)));
            return;
        }
        assert!(
            matches!(error, MigrationError::UnsupportedSource(layout) if layout == wrong_layout)
        );

        let mut bytes = GameSnapshot::save_validated(&Simulation::new(), 0, 0, "migration", 0);
        let old_preamble = save_codec::serialize(&GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: V0_3_8_AND_0_3_9,
        })
        .unwrap();
        bytes[..old_preamble.len()].copy_from_slice(&old_preamble);
        let before = bytes.clone();
        assert!(supports(&GameSnapshot::read_metadata(&before).unwrap()));
        assert!(matches!(
            migrate(&mut bytes),
            Err(MigrationError::HeaderOrState(
                SnapshotError::MissingScenarioResources
            ))
        ));
        assert_eq!(bytes, before);

        bytes.push(0);
        let before = bytes.clone();
        assert!(matches!(
            migrate(&mut bytes),
            Err(MigrationError::TrailingBytes)
        ));
        assert_eq!(bytes, before);
    }

    #[test]
    fn old_image_keeps_the_arctic_setting_and_applied_latch() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::object_image::{ImageTheater, ObjectImageKind, ObjectImageState};

        let ini = IniFile::from_str("[TREE]\nAlternateArcticArt=yes\n");
        let mut image = ObjectImageState::new("TREE");
        image.read_ini(
            ini.section("TREE").unwrap(),
            &ini,
            ObjectImageKind::Ordinary,
            ImageTheater::Snow,
        );
        assert_eq!(image.image(), "TREEA");

        // The eb05f451e writer put two true bytes immediately after ImageFile:
        // AlternateArcticArt and the latch that had applied its 'A' suffix.
        let mut bytes = save_codec::serialize(&image).unwrap();
        let after_image = 8 + image.image().len();
        assert_eq!(&bytes[after_image..after_image + 3], &[1, 0, 1]);
        bytes.remove(after_image + 1);
        let mut offsets = Vec::new();
        let (restored, rest) = save_codec::deserialize_previous::<ObjectImageState>(
            &bytes,
            previous_layout(ALPHA_SHAPE_LISTENER).unwrap(),
            &mut offsets,
        )
        .unwrap();
        assert_eq!(restored, image);
        assert!(rest.is_empty());
        assert_eq!(offsets, [(after_image + 1, 1)]);
    }

    #[test]
    fn spin_fields_insert_a_false_byte_and_a_zero_rate() {
        use crate::sim::movement::fly_height::FlyRuntime;

        // The 41f140f93 writer ended FlyRuntime with fall_counter and the two
        // speed words; spinning and spin_rate now sit between them.
        let fly = FlyRuntime::default();
        let mut bytes = save_codec::serialize(&fly).unwrap();
        let speeds = bytes.len() - 16;
        assert_eq!(&bytes[speeds - 5..speeds], &[0; 5]);
        bytes.drain(speeds - 5..speeds);
        let mut fields = Vec::new();
        let (restored, rest) = save_codec::deserialize_previous::<FlyRuntime>(
            &bytes,
            previous_layout(ALTERNATE_THEATER_ART).unwrap(),
            &mut fields,
        )
        .unwrap();
        assert_eq!(restored, fly);
        assert!(rest.is_empty());
        assert_eq!(fields, [(speeds - 5, 1), (speeds - 5, 4)]);
    }

    #[test]
    fn mech_tag_is_valid_only_from_v0_4_0() {
        use crate::rules::locomotor_type::LocomotorKind;
        let bytes = save_codec::serialize(&LocomotorKind::Mech).unwrap();
        let decode = |layout| {
            save_codec::deserialize_previous::<LocomotorKind>(
                &bytes,
                previous_layout(layout).unwrap(),
                &mut Vec::new(),
            )
        };
        assert!(decode(V0_3_8_AND_0_3_9).is_err());
        assert_eq!(decode(V0_4_0).unwrap(), (LocomotorKind::Mech, &[][..]));
    }
}
