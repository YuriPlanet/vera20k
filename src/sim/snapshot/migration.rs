//! Explicit conversions between audited player-save layouts.

use super::{
    GameSnapshot, GameSnapshotHeader, GameSnapshotPreamble, SNAPSHOT_ENVELOPE_VERSION,
    SNAPSHOT_PRODUCT_MAGIC, SnapshotError, save_codec, snapshot_layout,
};

const MIGRATION_ENVELOPE_VERSION: u32 = 2;
const V0_3_9: u32 = 0xFD30_A463;
const V0_4_0: u32 = 0x9BB2_0D64;

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("this build cannot write v0.4.0 migration output (layout {0:#010x})")]
    UnsupportedTarget(u32),
    #[error("the save uses layout {0:#010x}; conversion expects a v0.3.9 save")]
    UnsupportedSource(u32),
    #[error("the save is already in the v0.4.0 layout")]
    AlreadyCurrent,
    #[error("unexpected bytes after the saved world")]
    TrailingBytes,
    #[error("entity {0} was saved in a map tunnel; finish transit in v0.3.9 and save again")]
    TunnelTransit(u64),
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
        && header.layout == V0_3_9
        && header.compatibility().matches_build()
        && SNAPSHOT_ENVELOPE_VERSION == MIGRATION_ENVELOPE_VERSION
        && snapshot_layout() == V0_4_0
}

/// Convert a v0.3.9 player save to the v0.4.0 layout in memory.
///
/// Appending `Mech` to `LocomotorKind` preserves the existing variant tags.
/// Keep the world bytes verbatim after decoding all of them with the target
/// reader. Tunnel movement changed meaning in v0.4.0, so refuse any saved
/// actor in transit. The game still checks the rules and assets on restore.
pub fn migrate(bytes: &mut [u8]) -> Result<(), MigrationError> {
    let target = snapshot_layout();
    if SNAPSHOT_ENVELOPE_VERSION != MIGRATION_ENVELOPE_VERSION || target != V0_4_0 {
        return Err(MigrationError::UnsupportedTarget(target));
    }

    let preamble = GameSnapshot::read_preamble(bytes)?;
    GameSnapshot::validate_version(preamble.envelope_version)?;
    match preamble.layout {
        V0_4_0 => return Err(MigrationError::AlreadyCurrent),
        V0_3_9 => {}
        layout => return Err(MigrationError::UnsupportedSource(layout)),
    }
    GameSnapshot::read_compatible_header(bytes)?;

    let (mut snapshot, remainder): (GameSnapshot, _) =
        save_codec::deserialize_with_remainder(bytes)?;
    if !remainder.is_empty() {
        return Err(MigrationError::TrailingBytes);
    }
    GameSnapshot::validate_admitted(&mut snapshot)?;
    snapshot.require_player_resources()?;
    if let Some((id, _)) = snapshot
        .sim
        .substrate
        .entities
        .iter_sorted()
        .find(|(_, entity)| entity.low_bridge_tube_state.is_some())
    {
        return Err(MigrationError::TunnelTransit(id));
    }

    let target_preamble = save_codec::serialize(&GameSnapshotPreamble {
        layout: target,
        ..preamble
    })?;
    let output = bytes
        .get_mut(..target_preamble.len())
        .ok_or(save_codec::Error::UnexpectedEnd)?;
    output.copy_from_slice(&target_preamble);
    Ok(())
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use crate::sim::world::Simulation;

    #[test]
    fn migration_leaves_unsupported_and_invalid_snapshots_byte_exact() {
        let wrong_layout = V0_3_9 ^ 1;
        let mut bytes = save_codec::serialize(&GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: wrong_layout,
        })
        .unwrap();
        let original = bytes.clone();
        let error = migrate(&mut bytes).unwrap_err();
        assert_eq!(bytes, original);
        if SNAPSHOT_ENVELOPE_VERSION != MIGRATION_ENVELOPE_VERSION || snapshot_layout() != V0_4_0 {
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
            layout: V0_3_9,
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
}
