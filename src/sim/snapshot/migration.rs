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
// The next writer stores a map House's Ares `Degrades=` (HouseExt, read at
// hook 0x0050114D). Older worlds hold none: their Houses convert with it
// unset and keep their country's answer, as they played when saved.
const HOUSE_DEGRADES: u32 = 0xD6C7_5D57;
// The Firestorm ice chain (docs/ts-ice.md) stores its Scenario growth timer,
// its solidify list and each unit's ice-sinking flag. Older worlds ran no
// ice: no timer, no cracks and no unit sinking through ice.
const TS_ICE: u32 = 0xAFC7_F643;
// The next writer stores terrain deformation's pending second dent step.
// Older worlds have none pending.
const TERRAIN_DENT: u32 = 0x82C3_2EA4;
// Each object stores the Tiberian Sun limpets on it (`sim::limpet`) in its
// seldom part, and its foot speed a byte saying so. Older worlds hold none:
// only a `LimpetFactor=` warhead, which no older writer read, attaches one.
const TS_LIMPETS: u32 = 0xF612_7DC1;
// Tiberian Sun's BounceSound and ExpireSound: each saved AnimType's
// references gain both names. Older worlds had neither, which reads as None
// and plays nothing (docs/ts-anim-sounds.md).
const TS_ANIM_SOUNDS: u32 = 0x9441_E8D8;
// Older writers stored neither Kratos inputs nor its infantry firing offsets.
const KRATOS_FLH: u32 = 0x9F1B_D701;
// Tiberian Sun webs (docs/ts-webs.md) store the frame an infantryman's web
// runs out with a byte saying he was webbed, a warhead's web keys and an
// infantry type's Struggle record. Older worlds hold none: no man was
// webbed, end frame 0 has passed, no warhead webs and no type has Struggle
// frames.
const TS_WEBS: u32 = 0xF215_BF3A;
// The Wasm extension modules (RFC 0001), as their branch's writer `a33fcac81`
// stored them: the world stores its modules, their modifiers, variables and
// claims, and the trigger runtime the triggers they made. Older worlds ran no
// module: both convert empty.
const WASM_MODULES: u32 = 0xE18E_A373;
// The Wasm modules' control points (RFC 0001 0.0.22, 0.0.23): projectile
// paths and data words (`Launch`, `ProjectileFrame`), running pushes
// (`Push`), the `ImmunityFlags` and `ROTMultiplier` stats appended to `Stat`
// and the native body rates the ROT modifier keeps, without the image swap.
// Older worlds hold no path, no push and no modifier of the new stats.
const CONTROL_POINTS: u32 = 0x919A_B3CD;
// Beside them, main's writer from #5205 stores each object image's Phobos
// ArtImageSwap name (`ObjectImageState::image_swap`). Older worlds swapped no
// image, as they played when saved.
const ART_IMAGE_SWAP: u32 = 0x8C73_B1B3;
// Main's writer once the modules joined it, with both the modules and the
// image swap, without the control points.
const MODULES_AND_IMAGE_SWAP: u32 = 0xB65C_0664;
// NPatch's branch writer `2e67ebaa8` stored each processed rules layer's NPatch
// key reads, which decide `RuleSet::npatch` (`rules::npatch_switch`), beside
// the webs, without the modules or the image swap. Older worlds ran no NPatch
// switch: their rules keep every NPatch key dormant, as they played when saved.
const NPATCH_INPUTS: u32 = 0xE527_9FB2;
// Main's writer stores the modules, the image swap and the NPatch key reads,
// without the control points.
const MODULES_SWAP_AND_NPATCH: u32 = 0xBA51_A42C;
// The control points' writer stores the control points, the image swap and
// the NPatch key reads, without the client modules' messages.
const CONTROL_POINTS_SWAP_AND_NPATCH: u32 = 0xC66C_E753;
// The client modules writer adds their state (RFC 0001 section 10):
// the client messages waiting for `on_message` with their rate windows, and
// the room's `AutomationAllowed`. Older worlds hold no message and did not
// allow automation.
const CLIENT_MESSAGES: u32 = 0x9909_B4E7;
// Successive module writers retained animations (#5297), door positions
// (#5298), then image overrides (#5304). Each new collection starts empty.
const HELD_ANIMS: u32 = 0xED16_B100;
const HELD_DOORS: u32 = 0x9593_BB1E;
const HELD_IMAGES: u32 = 0xAA20_1352;
// Each house keeps its external driver's economy settings. All earlier
// writers stored only command_driven. Its saved value still owns control,
// and four zero bytes restore the neutral policy for that house.
const DRIVER_POLICY: u32 = 0xD5A9_6CCA;

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
        && snapshot_layout() == DRIVER_POLICY
}

fn previous_layout(source: u32) -> Option<save_codec::PreviousLayout> {
    const IMAGE_FLAGS: (&str, &[&str]) = ("ObjectImageState", &["alternate_theater_art"]);
    const ALPHA_LISTENER: (&str, &[&str]) = ("AlphaShape", &["anim_listener"]);
    const DECK: (&str, &[&str]) = ("BridgeCellFacts", &["fixed_deck_level"]);
    // The pending cell was a bare (x, y) point, the struct's first field.
    const PENDING_DECK: (&str, &[&str]) = ("PendingBridgeCell", &["corrected_deck_level"]);
    const SPIN: (&str, &[&str]) = ("FlyRuntime", &["spinning", "spin_rate"]);
    const DEGRADES: (&str, &[&str]) = ("HouseState", &["ares_degrades"]);
    const ICE_TIMER: (&str, &[&str]) = ("ScenarioSession", &["ice_growth_timer"]);
    const ICE_CRACKS: (&str, &[&str]) = ("Simulation", &["ice_cracks"]);
    const ICE_SINK: (&str, &[&str]) = ("SinkingState", &["ice"]);
    const DENT: (&str, &[&str]) = ("Simulation", &["terrain_dent"]);
    const LIMPETS: (&str, &[&str]) = ("Seldom", &["limpets"]);
    const LIMPETED: (&str, &[&str]) = ("FootSpeedState", &["limpeted"]);
    const ANIM_SOUNDS: (&str, &[&str]) = ("AnimTypeReferences", &["bounce_sound", "expire_sound"]);
    const KRATOS_INPUTS: (&str, &[&str]) = ("ProcessedRulesLayers", &["kratos_inputs"]);
    const KRATOS_OFFSETS: (&str, &[&str]) = ("WeaponOffsets", &["crawling", "elite_crawling"]);
    const WEB: (&str, &[&str]) = ("InfantryRuntime", &["webbed"]);
    const WEB_TIMER: (&str, &[&str]) = ("Seldom", &["web"]);
    const WARHEAD_WEB: (&str, &[&str]) = ("WarheadAnimReadState", &["web"]);
    const STRUGGLE: (&str, &[&str]) = ("InfantrySequenceBank", &["struggle"]);
    const EXT_MODULES: (&str, &[&str]) = ("Simulation", &["ext_modules"]);
    const MODULE_TRIGGERS: (&str, &[&str]) = ("TriggerRuntime", &["module"]);
    const CONTROL_POINT_STATE: (&str, &[&str]) = ("SavedExtModules", &["projectiles", "motions"]);
    const NATIVE_ROTS: (&str, &[&str]) = ("ModifierStore", &["native_rots"]);
    const IMAGE_SWAP: (&str, &[&str]) = ("ObjectImageState", &["image_swap"]);
    // Worlds older than the NPatch switch stored no key reads either: their
    // rules kept the switch off, which the all-zero empty read list restores.
    const NPATCH_KEYS: (&str, &[&str]) = ("ProcessedRulesLayers", &["npatch_inputs"]);
    const MESSAGES: (&str, &[&str]) = ("SavedExtModules", &["messages", "automation_allowed"]);
    const HELD_VISUALS: (&str, &[&str]) = ("ActionHeld", &["anims", "doors", "images"]);
    const DOORS_AND_IMAGES: (&str, &[&str]) = ("ActionHeld", &["doors", "images"]);
    const IMAGES: (&str, &[&str]) = ("ActionHeld", &["images"]);
    const POLICY: (&str, &[&str]) = ("HouseState", &["driver_policy"]);
    let absent_fields: &'static [_] = match source {
        V0_3_8_AND_0_3_9 | V0_4_0 => &[
            POLICY,
            HELD_VISUALS,
            IMAGE_FLAGS,
            ALPHA_LISTENER,
            DECK,
            PENDING_DECK,
            SPIN,
            DEGRADES,
            ICE_TIMER,
            ICE_CRACKS,
            ICE_SINK,
            DENT,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        ALPHA_SHAPE_LISTENER => &[
            POLICY,
            HELD_VISUALS,
            IMAGE_FLAGS,
            DECK,
            PENDING_DECK,
            SPIN,
            DEGRADES,
            ICE_TIMER,
            ICE_CRACKS,
            ICE_SINK,
            DENT,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        ALTERNATE_THEATER_ART => &[
            POLICY,
            HELD_VISUALS,
            DECK,
            PENDING_DECK,
            SPIN,
            DEGRADES,
            ICE_TIMER,
            ICE_CRACKS,
            ICE_SINK,
            DENT,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        BRIDGE_DECK_AND_FLY_SPIN => &[
            POLICY,
            HELD_VISUALS,
            DEGRADES,
            ICE_TIMER,
            ICE_CRACKS,
            ICE_SINK,
            DENT,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        HOUSE_DEGRADES => &[
            POLICY,
            HELD_VISUALS,
            ICE_TIMER,
            ICE_CRACKS,
            ICE_SINK,
            DENT,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        TS_ICE => &[
            POLICY,
            HELD_VISUALS,
            DENT,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        TERRAIN_DENT => &[
            POLICY,
            HELD_VISUALS,
            LIMPETS,
            LIMPETED,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        TS_LIMPETS => &[
            POLICY,
            HELD_VISUALS,
            ANIM_SOUNDS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        TS_ANIM_SOUNDS => &[
            POLICY,
            HELD_VISUALS,
            KRATOS_INPUTS,
            KRATOS_OFFSETS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        KRATOS_FLH => &[
            POLICY,
            HELD_VISUALS,
            WEB,
            WEB_TIMER,
            WARHEAD_WEB,
            STRUGGLE,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        TS_WEBS => &[
            POLICY,
            HELD_VISUALS,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
            NPATCH_KEYS,
        ],
        WASM_MODULES => &[
            POLICY,
            HELD_VISUALS,
            CONTROL_POINT_STATE,
            NATIVE_ROTS,
            IMAGE_SWAP,
            NPATCH_KEYS,
            MESSAGES,
        ],
        ART_IMAGE_SWAP => &[
            POLICY,
            HELD_VISUALS,
            EXT_MODULES,
            MODULE_TRIGGERS,
            NPATCH_KEYS,
        ],
        MODULES_AND_IMAGE_SWAP => &[
            POLICY,
            HELD_VISUALS,
            CONTROL_POINT_STATE,
            NATIVE_ROTS,
            NPATCH_KEYS,
            MESSAGES,
        ],
        CONTROL_POINTS => &[POLICY, HELD_VISUALS, IMAGE_SWAP, NPATCH_KEYS, MESSAGES],
        NPATCH_INPUTS => &[
            POLICY,
            HELD_VISUALS,
            EXT_MODULES,
            MODULE_TRIGGERS,
            IMAGE_SWAP,
        ],
        MODULES_SWAP_AND_NPATCH => &[
            POLICY,
            HELD_VISUALS,
            CONTROL_POINT_STATE,
            NATIVE_ROTS,
            MESSAGES,
        ],
        CONTROL_POINTS_SWAP_AND_NPATCH => &[POLICY, HELD_VISUALS, MESSAGES],
        CLIENT_MESSAGES => &[POLICY, HELD_VISUALS],
        HELD_ANIMS => &[POLICY, DOORS_AND_IMAGES],
        HELD_DOORS => &[POLICY, IMAGES],
        HELD_IMAGES => &[POLICY],
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
    if SNAPSHOT_ENVELOPE_VERSION != MIGRATION_ENVELOPE_VERSION || target != DRIVER_POLICY {
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
            || snapshot_layout() != DRIVER_POLICY
        {
            assert!(matches!(error, MigrationError::UnsupportedTarget(_)));
            return;
        }
        assert!(
            matches!(error, MigrationError::UnsupportedSource(layout) if layout == wrong_layout)
        );

        // The empty world as the v0.3.8 writer wrote it, without the fields
        // that writer lacked.
        let current = GameSnapshot::save_validated(&Simulation::new(), 0, 0, "migration", 0);
        let snapshot: GameSnapshot = save_codec::deserialize(&current).unwrap();
        let mut bytes =
            save_codec::serialize_previous(&snapshot, &previous_layout(V0_3_8_AND_0_3_9).unwrap())
                .unwrap();
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
    fn old_command_driven_house_keeps_its_controller_and_gets_neutral_support() {
        use crate::sim::driver::DriverPolicy;
        use crate::sim::house_state::HouseState;

        let mut house = HouseState::new(Default::default(), 0, None, false, 500, 10);
        house.make_command_driven(DriverPolicy {
            wallet_floor: Some(2000),
            ..DriverPolicy::neutral()
        });
        let old = previous_layout(HELD_IMAGES).unwrap();
        let bytes = save_codec::serialize_previous(&house, &old).unwrap();
        let (restored, rest) =
            save_codec::deserialize_previous::<HouseState>(&bytes, old, &mut Vec::new()).unwrap();
        assert!(rest.is_empty());
        assert_eq!(restored.driver_policy(), Some(&DriverPolicy::neutral()));
        assert_eq!(restored.economy().credits(), 500);
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
        // That writer also predates the optional Phobos image name, which
        // now sits immediately before its retained model slots.
        let slots_size = save_codec::serialize(&image.model_slots).unwrap().len();
        let image_swap = bytes.len() - slots_size - 1;
        assert_eq!(bytes.remove(image_swap), 0); // None
        let mut offsets = Vec::new();
        let (restored, rest) = save_codec::deserialize_previous::<ObjectImageState>(
            &bytes,
            previous_layout(ALPHA_SHAPE_LISTENER).unwrap(),
            &mut offsets,
        )
        .unwrap();
        assert_eq!(restored, image);
        assert!(rest.is_empty());
        assert_eq!(offsets, [(after_image + 1, 1), (image_swap, 1)]);
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
