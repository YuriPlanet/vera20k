//! art.ini parsing and art-resolution helpers.
//!
//! This module is intentionally split into three layers:
//! - explicit rules/art data resolution
//! - original-engine filename conventions we intentionally keep
//! - repo-only fallback hooks that stay outside this module
//!
//! `ArtRegistry` owns only parsed metadata and exact ID/section lookups.
//! Filename candidate generation lives in free functions below so render code can
//! use convention helpers without re-mixing them into metadata resolution.

use std::collections::{BTreeSet, HashMap, VecDeque};

use thiserror::Error;

use crate::rules::flh::{Flh, parse_flh};
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::object_type::{BuildingHiddenOccupancyProfile, HIDDEN_OCCUPY_SLOT_COUNT};
use crate::util::native_x87::NativeF64Bits;

/// Per-object art configuration parsed from an art.ini section.
#[derive(Debug, Clone)]
pub struct ArtEntry {
    /// Image filename override (from art.ini `Image=` key).
    /// None means use the section name as the filename base.
    pub image: Option<String>,
    /// Sidebar cameo/image override from `Cameo=`.
    pub cameo: Option<String>,
    /// Alternate sidebar cameo/image override from `AltCameo=`.
    pub alt_cameo: Option<String>,
    /// Replace 2nd char of filename with theater-specific letter.
    pub new_theater: bool,
    /// Use theater file extension (e.g., `.tem`) instead of `.shp`.
    /// Set by `Theater=yes` in art.ini. Distinct from `NewTheater=`.
    pub theater: bool,
    pub scorch: bool,
    pub crater: bool,
    pub force_big_craters: bool,
    /// SHP frame 0's visible-content bounding-rect width, in pixels.
    /// Used by the smudge dispatcher as a damage-tier proxy for size selection.
    /// Default 30 — matches the original engine's uncached first-call fallback;
    /// replaced with the actual SHP frame width by `populate_anim_frame_dims`
    /// for anims with a Crater/Scorch/ForceBigCraters spawn flag.
    pub frame_width: u16,
    /// SHP frame 0's visible-content bounding-rect height, in pixels.
    /// See `frame_width`.
    pub frame_height: u16,
    /// Render as VXL+HVA model (true) or SHP sprite (false).
    pub voxel: bool,
    /// Preserve an absent Voxel key for ObjectType's retained-default reader.
    pub authored_voxel: Option<bool>,
    /// Optional voxel turret/barrel forward/backward alignment tweak.
    pub turret_offset: i32,
    /// Extra Y pixel offset for sprite rendering.
    pub y_draw_offset: i32,
    /// Extra X pixel offset for sprite rendering.
    pub x_draw_offset: i32,
    /// Building animation overlays (ActiveAnim, IdleAnim, etc.).
    pub building_anims: Vec<BuildingAnimConfig>,
    /// All21 native records exist even when their selected names are empty.
    pub building_anim_power: [BuildingAnimPowerFlags; 21],
    /// Signed GateStages (BuildingType+16F8): constructor45E1F0 defaults to9;
    /// art ReadInt4612D1 preserves signed32 values used by body frame43EF90.
    pub building_gate_stages: i32,
    /// Signed start/count/rate triples for AnimIdle, AnimActive, AnimAux1,
    /// AnimAux2. Native4615CA..4617B8 partially assigns each scanf triple.
    pub building_body_ranges: [[i32; 3]; 4],
    /// Building foundation footprint (e.g., "4x4", "2x2").
    pub foundation: Option<String>,
    /// Overlay type produced by this BuildingType's art (`ToOverlay=`).
    pub to_overlay: Option<String>,
    /// BibShape: separate SHP for the ground-level pad/bib under a building.
    pub bib_shape: Option<String>,
    /// Custom palette override from art.ini `Palette=`.
    /// Stored as a palette base name without `.pal`.
    pub palette: Option<String>,
    /// Infantry animation sequence definition name (e.g., "ConSequence").
    /// Points to a `[ConSequence]`-style section in art.ini with frame layouts.
    pub sequence: Option<String>,
    /// Infantry `Crawls=` art flag. Merged into ObjectType for sim stance speed.
    pub crawls: bool,
    /// Muzzle offset for primary weapon fire (from art.ini `PrimaryFireFLH=`).
    pub primary_fire_flh: Flh,
    /// Muzzle offset for secondary weapon fire (from art.ini `SecondaryFireFLH=`).
    pub secondary_fire_flh: Flh,
    /// Elite-rank override for primary fire offset (from art.ini `ElitePrimaryFireFLH=`).
    /// None means use `primary_fire_flh`.
    pub elite_primary_fire_flh: Option<Flh>,
    /// Elite-rank override for secondary fire offset (from art.ini `EliteSecondaryFireFLH=`).
    /// None means use `secondary_fire_flh`.
    pub elite_secondary_fire_flh: Option<Flh>,
    /// Fixed building primary fire screen-pixel offset.
    /// Used by non-turret buildings before converting the pixel delta to world leptons.
    pub primary_fire_pixel_offset: Option<(i32, i32)>,
    /// Fixed building secondary fire screen-pixel offset.
    pub secondary_fire_pixel_offset: Option<(i32, i32)>,
    /// Building primary fire alternates the X pixel offset by burst side.
    pub primary_fire_dual_offset: bool,
    /// Building fire is held behind the SpecialAnim before weapon emission.
    pub is_anim_delayed_fire: bool,
    /// BuildingType+16A8, ART ReadBool461169, constructor45E0BA false.
    pub silo_damage: bool,
    /// Signed number of Building AI visits used by the delayed-fire latch.
    pub delayed_fire_delay: i32,
    /// SHP vehicle: walk animation frame count per facing (from `WalkFrames=`).
    pub walk_frames: Option<u16>,
    /// SHP vehicle: firing animation frame count per facing (from `FiringFrames=`).
    pub firing_frames: Option<u16>,
    /// SHP vehicle: standing animation frame count per facing (from `StandingFrames=`).
    pub standing_frames: Option<u16>,
    /// SHP vehicle: number of facing directions (from `Facings=`, default 8).
    pub shp_facings: u8,
    /// Weapon discharge delay in animation frames (from `FireUp=`, default 0).
    /// Distinct from the `FireUp` sequence action in infantry sequences.
    pub fire_up: u8,
    /// Infantry primary prone discharge frame (`FireProne=`).
    /// Defaults to `FireUp` when absent, matching the InfantryType read fallback.
    pub fire_prone: u8,
    /// Infantry secondary standing discharge frame (`SecondaryFire=`).
    /// Defaults to `FireUp` when absent.
    pub secondary_fire: u8,
    /// Infantry secondary prone/deploy discharge frame (`SecondaryProne=`).
    /// Defaults to `SecondaryFire` when absent.
    pub secondary_prone: u8,
    /// Animation `Report=` sound ID. Used as a fallback when `StartSound=`
    /// is absent.
    pub report: Option<String>,
    /// Animation `StartSound=` sound ID. Takes priority over `Report=`.
    pub start_sound: Option<String>,
    /// Signed building brightness addition (ExtraLight= in art.ini).
    /// DrawBody 0043D824 adds signed BuildingType+1548 to top-cell brightness;
    /// buildup 0043D644 omits it. It is separate from NormalZAdjust.
    /// Retail values: GADPSA=350, GAICBM=-100.
    pub extra_light: i32,
    /// BuildingType +0x1702, art parser 004612B4: use the cell ISO Convert.
    pub terrain_palette: bool,
    /// Harvester queueing cell offset from building origin (QueueingCell= in art.ini).
    /// Where miners wait outside the dock when it is occupied. e.g. `(4, 1)` for GAREFN.
    pub queueing_cell: Option<(u16, u16)>,
    /// All `DockingOffset%d` entries actually present in this art.ini section,
    /// in index order. Up to 8 (defensive ceiling for mod safety; retail uses
    /// up to 4). The art→rules merge in [`crate::rules::ruleset`] is what
    /// applies `NumberOfDocks`-aware sizing — see `ObjectType::pads` for the
    /// merged shape.
    pub pads: Vec<crate::rules::object_type::DockPad>,
    /// Pixel offsets where fire/smoke overlays appear when building health < ConditionYellow.
    /// Parsed from DamageFireOffset0=X,Y .. DamageFireOffset7=X,Y in art.ini. Max 8.
    pub damage_fire_offsets: Vec<DamageFireOffset>,
    /// Building height in cell-height units (from `Height=` in art.ini).
    /// Used for health bar vertical positioning: Dimension2.Z = (fh + Height) * 256
    /// leptons, projected via CoordsToScreen as z_screen = (fh + Height) * 7.5 px.
    pub height: i32,
    /// Fire port pixel offsets for garrison muzzle flashes.
    /// Parsed from `MuzzleFlash0=X,Y` through `MuzzleFlash9=X,Y` in art.ini.
    /// Each entry is a screen-space offset from the building's center.
    pub muzzle_flash_positions: Vec<(i32, i32)>,
    /// Valid cells from art.ini AddOccupy1..8, scanned by slot number.
    /// Signed offsets from the building's origin (rx, ry) — negative = west/north.
    pub add_occupy: [Option<(i16, i16)>; HIDDEN_OCCUPY_SLOT_COUNT],
    /// Valid cells from art.ini RemoveOccupy1..8, scanned by slot number.
    pub remove_occupy: [Option<(i16, i16)>; HIDDEN_OCCUPY_SLOT_COUNT],
    /// Middle integer of `Deploy=<start>,<frames>,<rate>` in the infantry
    /// sequence section referenced by `sequence`. `None` when the sequence
    /// is undefined or doesn't have a `Deploy` entry. Drives the per-type
    /// Deploying-phase duration via `sim::deploy::compute_anim_ticks`.
    pub deploy_frames: Option<u16>,
    /// Middle integer of `Undeploy=<start>,<frames>,<rate>` in the sequence.
    pub undeploy_frames: Option<u16>,
    /// Middle integer of `DeployedFire=<start>,<frames>,<rate>` in the sequence.
    pub deployed_fire_frames: Option<u16>,
    /// `ZShapePointMove=X,Y` (`BuildingTypeClass +0x1530/+0x1534`): pixel
    /// shift of the BUILDNGZ z-shape a building body writes its depth through
    /// (`BuildingClass_DrawBody 0x0043D6EF..0x0043D77A`). Retail sets it on
    /// ten tall structures; default 0,0.
    pub z_shape_point_move: (i32, i32),
    /// `NormalZAdjust=` (`BuildingTypeClass +0x1520`, read at `0x004613A6`):
    /// the building body's own Z term before the height lift is cancelled.
    /// Retail sets it only on GAFSDF (-10); default 0.
    pub normal_z_adjust: i32,
}

/// One native building-damage-fire art offset.
///
/// The source pair remains available for z-adjust. The world delta is bound once
/// during rules initialization so authoritative simulation never performs float
/// coordinate conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DamageFireOffset {
    pub pixel_x: i32,
    pub pixel_y: i32,
    pub world_dx: i32,
    pub world_dy: i32,
}

/// Parse a sequence entry value of the form `<start>,<frames>,<rate>` and
/// return the middle integer (frame count). Returns `None` on malformed input.
fn parse_sequence_frames(value: &str) -> Option<u16> {
    let mut parts = value.split(',').map(str::trim);
    let _start = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

/// Which category of building animation this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildingAnimKind {
    Active,
    Idle,
    Super,
    Special,
    Production,
}

/// Animation name plus the frame-timing metadata from that animation section.
#[derive(Debug, Clone, Hash)]
pub struct BuildingAnimVariantConfig {
    pub anim_type: String,
    pub loop_start: u16,
    pub loop_end: u16,
    pub loop_count: i32,
    pub rate: u16,
    pub start_frame: u16,
    pub ping_pong: bool,
}

/// Native-like metadata used by app-side `AnimClass` runtime slices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnimTypeRuntimeConfig {
    /// False means the allocated native type has never completed ART ReadINI.
    /// It retains constructor bounds and must not load an orphan SHP.
    pub art_body_read: bool,
    pub start: i32,
    pub loop_start: i32,
    pub loop_end: i32,
    pub end: i32,
    /// Whether `End=` was explicitly present. Explicit zero differs from omission.
    pub explicit_end: Option<i32>,
    /// Whether `LoopEnd=` was explicitly present.
    pub explicit_loop_end: Option<i32>,
    /// Signed SHP frame count read from header offset +6 during asset binding.
    pub raw_shp_frame_count: Option<i32>,
    pub loop_count: i32,
    pub rate_logic_frames: u16,
    pub normalized: bool,
    pub tiberium_chain_reaction: bool,
    pub is_tiberium: bool,
    pub hide_if_no_ore: bool,
    pub is_animated_tiberium: bool,
    pub tiberium_spread_radius: i32,
    pub tiberium_spawn_type: Option<String>,
    /// Raw `MakeInfantry=` index. Native defaults to -1 and treats every
    /// other value as enabling the AnimClass cell-occupation lifecycle.
    pub make_infantry: i32,
    pub bouncer: bool,
    pub spawns: Option<String>,
    pub spawn_count: i32,
    pub running_frames: i32,
    /// art.ini `Damage=`, `AnimTypeClass+0x2A8`.
    ///
    /// gamemd-derived: `AnimTypeClass::ReadINI @ 0x00427D00` reads it through
    /// `CCINIClass::ReadDouble` into `param_1[0xaa]` (= byte offset `0x2A8`), so
    /// this is a **double**, not an integer — the stock population is dominated
    /// by fractional values such as `.003`. The constructor
    /// (`AnimTypeClass::AnimTypeClass @ 0x00427530`, `param_1[0xaa]/[0xab] = 0`)
    /// leaves it `0.0` when the key is absent, and the read passes the current
    /// value back as its own default.
    ///
    /// UNCHECKED consumer (M11b): the per-frame damage arm at
    /// `AnimClass::AI 0x00424507..0x0042464C` accumulates this into
    /// `AnimClass+0x188`, which the constructor seeds to **`1.0`**
    /// (`0x00421FA4` zeroes the low dword, `0x00421FC8` writes `0x3FF00000` to
    /// the high dword), so the first armed frame always emits at least one
    /// point. Nothing reads this field yet.
    pub damage: NativeF64Bits,
    /// art.ini `Warhead=`, `AnimTypeClass+0x330`, resolved by
    /// `WarheadTypeClass::FindOrAllocate`. Bouncer-landing only — the per-frame
    /// damage arm hard-selects `[CombatDamage] C4Warhead`/`FlameDamage2`
    /// instead. Not consumed yet (M11b).
    pub warhead: Option<String>,
    /// art.ini `DamageRadius=`, `AnimTypeClass+0x334`. Bouncer-landing only.
    /// Not consumed yet (M11b).
    pub damage_radius: i32,
    /// art.ini `Elasticity=`, `AnimTypeClass+0x310`, a `ReadDouble`.
    /// Constructor default `0x3FE99999_A0000000` — the single-precision `0.8f`
    /// widened, **not** the nearest double to 0.8.
    pub elasticity: NativeF64Bits,
    /// art.ini `MinZVel=`, `AnimTypeClass+0x318`, a `ReadDouble`.
    /// Constructor default `0x400C0000_00000000` = **3.5**.
    pub min_z_vel: NativeF64Bits,
    /// art.ini `MaxXYVel=`, `AnimTypeClass+0x328`, a `ReadDouble`.
    /// Constructor default `0x402E0000_00000000` = **15.0**.
    pub max_xy_vel: NativeF64Bits,
    /// art.ini `IsMeteor=`, `AnimTypeClass+0x356`. Shares the constructor's
    /// `BounceClass` fork with `Bouncer=`. Not consumed yet (M11b).
    pub is_meteor: bool,
    /// art.ini `IsVeins=`, `AnimTypeClass+0x355`. Zero stock sections author it.
    pub is_veins: bool,
    /// art.ini `IsFlamingGuy=`, `AnimTypeClass+0x354`. Gates
    /// `AnimClass::BounceAI @ 0x00425670` — the flaming-infantry runner — from
    /// `AnimClass::AI`'s third statement. One stock section (`FLAMEGUY`).
    pub is_flaming_guy: bool,
    /// art.ini `Scorch=`, `AnimTypeClass+0x36B`, read by
    /// `AnimClass::Middle @ 0x00424F00`.
    pub scorch: bool,
    /// art.ini `Crater=`, `AnimTypeClass+0x36D`, read by
    /// `AnimClass::Middle @ 0x00424F00`.
    pub crater: bool,
    /// art.ini `ForceBigCraters=`, `AnimTypeClass+0x36E`, read by
    /// `AnimClass::Middle @ 0x00424F00`.
    pub force_big_craters: bool,
    /// art.ini `Sticky=`, `AnimTypeClass+0x36F`.
    pub sticky: bool,
    /// art.ini `PsiWarning=`, `AnimTypeClass+0x373`, read by `AnimClass::AI`.
    pub psi_warning: bool,
    /// art.ini `DoubleThick=`, `AnimTypeClass+0x368`.
    pub double_thick: bool,
    /// art.ini `Flamer=`, `AnimTypeClass+0x36C`. Zero stock sections author it.
    pub flamer: bool,
    /// art.ini `SpawnsParticle=`, `AnimTypeClass+0x2CC`, resolved by
    /// `ParticleTypeClass::FindOrCreate`. Read by
    /// `AnimClass::Middle @ 0x00424F00`, `NumParticles` times.
    ///
    /// Unlike every other key in this parser, `ReadINI` writes the field
    /// **only when the key is present** (`0x00427D00`: the store sits inside
    /// the `if (ReadString != 0)` arm), so an absent key keeps the
    /// constructor's `-1`. `None` here is that `-1`.
    pub spawns_particle: Option<String>,
    /// art.ini `NumParticles=`, `AnimTypeClass+0x2D0`.
    pub num_particles: i32,
    /// art.ini `ShouldFogRemove=`, `AnimTypeClass+0x374`.
    /// Constructor default **true**.
    pub should_fog_remove: bool,
    /// art.ini `ShouldUseCellDrawer=`, `AnimTypeClass+0x35C`.
    /// Constructor default **true**.
    pub should_use_cell_drawer: bool,
    pub detail_level: i32,
    pub next: Option<String>,
    pub bounce_anim: Option<String>,
    pub expire_anim: Option<String>,
    pub trailer_anim: Option<String>,
    pub trailer_seperation: i32,
    pub random_loop_delay: Option<(u16, u16)>,
    pub random_rate_logic_frames: Option<(u16, u16)>,
    pub y_draw_offset: i32,
    pub z_adjust: i32,
    pub y_sort_adjust: i32,
    pub layer: AnimLayer,
    pub flat: bool,
    pub tiled: bool,
    /// Raw art.ini `Translucency=` value; zero when omitted.
    pub translucency: i32,
    /// Raw art.ini `TranslucencyDetailLevel=` value; zero when omitted.
    pub translucency_detail_level: i32,
    pub translucent: bool,
    /// art.ini `AltPalette=`. gamemd's animation draw path picks the palette in a
    /// cascade: an explicit per-instance palette wins, otherwise the global
    /// ANIM.PAL conversion is used — except when this flag is set, which swaps in
    /// the first colour scheme's converted unit palette instead. **31** stock
    /// art.ini sections set it — every one of them `yes` — including the Battle
    /// Bunker, the `CABUNK0x` bunkers, the Weather Storm clouds and the squid
    /// grapple; 23 of the 31 are also declared in `[Animations]`. (Derived by
    /// section-scanning `artmd.ini` with the exact spelling `AltPalette`;
    /// gamemd's `CCINIClass` key lookup is case-sensitive, so only that spelling
    /// counts. The doc previously said 41, which no scan of the retail file
    /// reproduces.)
    pub alt_palette: bool,
    /// art.ini `UseNormalLight=`. The retail art.ini documents this as "does this
    /// anim always draw at 100% brightness? (def=no)", and gamemd's animation draw
    /// matches: it initialises the shape's brightness argument to full
    /// (1000 = 1.0) and, in each of that function's branches, only replaces it
    /// with a scalar read off the animation's cell when this flag is clear. So a
    /// set flag makes the animation ignore map lighting entirely rather than
    /// merely brightening it. At least one further native site reads the same
    /// gate outside that function and was not traced, and the branch taken when
    /// an animation instance carries its own palette convert takes brightness
    /// from an instance field rather than a cell scalar — its stock reachability
    /// is UNCHECKED.
    /// 43 stock sections set it, all the explosion/fire/flash families —
    /// `TWLT*`, `S_BANG*`, `S_BRNL*`, `S_CLSN*`, `S_TUMU*`, `BURN-S/M/L`, `FIRE3`,
    /// `EXPLOSML/MED/LRG/LB`, `BRRLEXP*`, `CRIVEXP*`, `APOCEXP`, `VTEXPLOD`,
    /// `KTSTLEXP`, `PULSEFX1/2`, `EMP_FX01`, `BEHIND`.
    pub use_normal_light: bool,
    pub shadow: bool,
    pub ping_pong: bool,
    pub reverse: bool,
    pub report: Option<String>,
    pub start_sound: Option<String>,
    pub stop_sound: Option<String>,
}

/// Failures while binding native AnimType metadata to retail SHP assets.
#[derive(Debug, Error)]
pub enum AnimAssetBindError {
    #[error("damage-fire art contains an offset outside the verified native conversion domain")]
    InvalidDamageFireOffset,
    #[error("required animation type [{0}] is absent from merged art data")]
    MissingAnimType(String),
    #[error("required SHP for animation type [{0}] was not found")]
    MissingShp(String),
    #[error("animation SHP [{name}] has an invalid signed frame count {count}")]
    InvalidFrameCount { name: String, count: i32 },
    #[error(
        "animation type [{name}] has invalid loaded {field} value {value} for {raw_count} frames"
    )]
    InvalidLoadedBound {
        name: String,
        field: &'static str,
        value: i32,
        raw_count: i32,
    },
}

/// Display layer used by native object submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimLayer {
    Ground,
    Top,
    Other(i32),
}

impl AnimLayer {
    fn from_ini(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()) {
            Some(v) if v == "ground" => Self::Ground,
            Some(v) if v == "top" => Self::Top,
            Some(v) => v.parse::<i32>().map(Self::Other).unwrap_or(Self::Top),
            None => Self::Top,
        }
    }
}

/// Configuration for a building animation overlay (ActiveAnim, IdleAnim, etc.).
#[derive(Debug, Clone, Hash)]
pub struct BuildingAnimConfig {
    /// BuildingType native slot at F4C + 44 * slot (ReadINI4617C4..464727).
    pub native_slot: u8,
    pub anim_type: String,
    /// Replacement animation for this slot when the building is damaged.
    pub damaged_variant: Option<BuildingAnimVariantConfig>,
    /// Replacement animation for this slot when a garrisoned building is healthy.
    pub garrisoned_variant: Option<BuildingAnimVariantConfig>,
    pub kind: BuildingAnimKind,
    /// True for the base key (e.g., `ActiveAnim`), false for suffixed variants
    /// (`ActiveAnimTwo`, `Three`, `Four`).  Used to gate the primary active anim
    /// on building ownership while letting secondary anims (flags) play always.
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
    pub y_sort: i32,
    pub z_adjust: i32,
    pub loop_start: u16,
    pub loop_end: u16,
    pub loop_count: i32,
    pub rate: u16,
    pub start_frame: u16,
    pub ping_pong: bool,
}

/// BuildingType F8C..F8F, stride44. Constructor45E3BE..45E416 seeds
/// Powered=true and the other three flags false; ReadINI has60 authored stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuildingAnimPowerFlags {
    pub powered: bool,
    pub powered_light: bool,
    pub powered_effect: bool,
    pub powered_special: bool,
}

impl Default for BuildingAnimPowerFlags {
    fn default() -> Self {
        Self {
            powered: true,
            powered_light: false,
            powered_effect: false,
            powered_special: false,
        }
    }
}

fn read_building_anim_power(section: &IniSection, records: &mut [BuildingAnimPowerFlags; 21]) {
    for &(base, suffixes, first_slot) in BUILDING_ANIM_KEYS {
        for (ordinal, suffix) in suffixes.iter().enumerate() {
            let slot = usize::from(first_slot) + ordinal;
            // No power ReadINI stores exist for PowerUps, Pre/Production or Turret.
            if !matches!(slot, 3..=6 | 10..=20) {
                continue;
            }
            let key = format!("{base}{suffix}");
            let record = &mut records[slot];
            for (suffix, field) in [
                ("Powered", &mut record.powered),
                ("PoweredLight", &mut record.powered_light),
                ("PoweredEffect", &mut record.powered_effect),
                ("PoweredSpecial", &mut record.powered_special),
            ] {
                if let Some(value) = section.get_bool(&format!("{key}{suffix}")) {
                    *field = value;
                }
            }
        }
    }
}

/// Convert art.ini `Rate=` value to native game-logic frame delay.
///
/// gamemd stores `900 / INI_Rate` as the AnimType frame delay when
/// `Rate > 0`; `Rate <= 0` stores zero.
pub fn art_rate_to_logic_frames(ini_rate: i32) -> u16 {
    if ini_rate < 1 {
        return 0;
    }
    (900 / ini_rate as u32) as u16
}

/// Convert art.ini `Rate=` value to milliseconds per frame.
///
/// This is an app/render convenience wrapper over the native logic-frame delay.
/// Garrison `OccupantAnim` playback uses `art_rate_to_logic_frames` directly so
/// the default remains one sim logic tick rather than a rounded wall-clock value.
pub fn art_rate_to_delay_ms(ini_rate: i32) -> u32 {
    let delay_frames: u32 = u32::from(art_rate_to_logic_frames(ini_rate));
    if delay_frames == 0 {
        return 0;
    }
    (delay_frames * 1000 / 15).max(1)
}

/// Draw-flag bits for an animation's translucency stage.
///
/// These are the low bits of the draw-flag word gamemd's shape drawer hands to
/// its blitter table. The table maps them to three fixed integer blends of the
/// 16-bit source and destination pixels — read out of the blitter scanline
/// routines themselves, not inferred from the key names:
///
/// | bits | blend                       | source weight |
/// |------|-----------------------------|---------------|
/// | 0x2  | `3*(src>>2) + (dst>>2)`      | 3/4           |
/// | 0x4  | `(src>>1)   + (dst>>1)`      | 1/2           |
/// | 0x6  | `(src>>2)   + 3*(dst>>2)`    | 1/4           |
///
/// **`Translucency=N` therefore reads as "N percent *transparent*".** The source
/// weight is `1 - N/100`, so `Translucency=25` is the *most* opaque of the three
/// and `Translucency=75` the faintest. Reading the key as "N percent opaque"
/// inverts every explosion, fire and wake in the game.
pub const ANIM_DRAW_BITS_OPAQUE: u32 = 0x0;
/// `Translucency=25` — source contributes three quarters.
pub const ANIM_DRAW_BITS_TRANSLUCENT_25: u32 = 0x2;
/// `Translucency=50` — source and destination contribute equally.
pub const ANIM_DRAW_BITS_TRANSLUCENT_50: u32 = 0x4;
/// `Translucency=75` — source contributes one quarter.
pub const ANIM_DRAW_BITS_TRANSLUCENT_75: u32 = 0x6;

/// Return the fixed draw-bit contribution for an art.ini `Translucency=` value.
///
/// `Translucent=` is an independent boolean and is not interpreted by this helper.
pub const fn anim_fixed_translucency_draw_bits(translucency: i32) -> Option<u32> {
    match translucency {
        25 => Some(ANIM_DRAW_BITS_TRANSLUCENT_25),
        50 => Some(ANIM_DRAW_BITS_TRANSLUCENT_50),
        75 => Some(ANIM_DRAW_BITS_TRANSLUCENT_75),
        _ => None,
    }
}

/// Which palette an animation type's frames are baked/sampled against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimDrawPalette {
    /// The global ANIM.PAL conversion — the default for animation draws.
    Anim,
    /// The first colour scheme's converted unit palette, selected by
    /// `AltPalette=yes`.
    Unit,
}

/// Palette an animation type draws through, per gamemd's draw-time cascade.
///
/// The native cascade tries, in order: a Tiberian Sun veins branch, a per-instance
/// cell-owned palette, an explicit per-instance palette override, and finally the
/// global ANIM.PAL conversion — with `AltPalette=yes` swapping the unit palette in
/// at that last step. Only the last step is decidable from the art type alone, and
/// it is the only one a stock skirmish reaches: the veins branch is Tiberian Sun
/// legacy with no stock YR trigger, and no stock art section sets the per-instance
/// flags the other two key on. Those are recorded as unmodelled rather than
/// approximated here.
pub const fn anim_draw_palette(config: &AnimTypeRuntimeConfig) -> AnimDrawPalette {
    if config.alt_palette {
        AnimDrawPalette::Unit
    } else {
        AnimDrawPalette::Anim
    }
}

/// Progressive translucency ladder for `Translucent=yes` animation types.
///
/// `Translucent=yes` is **not** a constant blend: gamemd's animation draw path
/// compares the current frame against fractions of the type's `End` and fades the
/// animation out as it plays. An animation is opaque for its first fifth, then
/// steps through three-quarters, one-half and one-quarter source weight. This is
/// what makes a fire or a ship wake dissipate instead of vanishing in one frame.
///
/// The fraction comparisons are evaluated in native double precision against the
/// literals 0.2 / 0.4 / 0.6 and are reproduced here rather than rewritten as
/// integer cross-multiplication, because the two disagree at boundary frames for
/// some `End` values. This is render material selection only — no simulation
/// state, ordering or RNG depends on it.
pub fn anim_progressive_translucency_draw_bits(current_frame: i32, end_frame: i32) -> u32 {
    const OPAQUE_FRACTION: f64 = 0.2;
    const QUARTER_FRACTION: f64 = 0.4;
    const HALF_FRACTION: f64 = 0.6;

    let frame = f64::from(current_frame);
    let end = f64::from(end_frame);
    if frame > end * HALF_FRACTION {
        ANIM_DRAW_BITS_TRANSLUCENT_75
    } else if frame > end * QUARTER_FRACTION {
        ANIM_DRAW_BITS_TRANSLUCENT_50
    } else if frame > end * OPAQUE_FRACTION {
        ANIM_DRAW_BITS_TRANSLUCENT_25
    } else {
        ANIM_DRAW_BITS_OPAQUE
    }
}

/// Draw-time `End` for the progressive translucency ladder.
///
/// gamemd's AnimType loader seeds `End` from the SHP header frame count, halving
/// it for `Shadow=yes` (the back half of such a SHP holds the shadow frames), and
/// an explicit art.ini `End=` overrides it. `bind_scheduler_anim_assets`
/// reproduces that for the scheduler closure, so a bound config already carries
/// the native value; for a type that has not been through asset binding, derive
/// the same value from the caller's SHP frame count rather than reading the
/// unbound zero.
fn anim_draw_end_frame(config: &AnimTypeRuntimeConfig, shp_frame_count: i32) -> i32 {
    if config.raw_shp_frame_count.is_some() {
        return config.end;
    }
    if let Some(end) = config.explicit_end {
        return end;
    }
    if config.shadow {
        shp_frame_count / 2
    } else {
        shp_frame_count
    }
}

/// Resolve an animation type's translucency draw bits for one drawn frame.
///
/// `Translucent=yes` takes the progressive ladder; otherwise an exact
/// `Translucency=25/50/75` selects a fixed stage and anything else draws opaque.
/// `shp_frame_count` is only consulted on the progressive path, and only when the
/// type has not been bound to its SHP yet.
///
/// Two native gates are deliberately not modelled and are recorded here rather
/// than approximated. gamemd skips this whole selection when the type's
/// `TranslucencyDetailLevel` exceeds the extra-animations video setting; omitting
/// the gate is equivalent to that setting being at maximum, which is its normal
/// value in play, and the runtime value is UNCHECKED. gamemd also aborts the draw
/// outright above a per-instance byte whose source was not traced; it is zero on a
/// freshly constructed animation and Rust has no counterpart, so this behaves as
/// that zero — identical for every path that reaches it with zero.
pub fn anim_translucency_draw_bits(
    config: &AnimTypeRuntimeConfig,
    current_frame: i32,
    shp_frame_count: i32,
) -> u32 {
    if config.translucent {
        return anim_progressive_translucency_draw_bits(
            current_frame,
            anim_draw_end_frame(config, shp_frame_count),
        );
    }
    anim_fixed_translucency_draw_bits(config.translucency).unwrap_or(ANIM_DRAW_BITS_OPAQUE)
}

/// Bits of a `CC_Draw_Shape` flag word that name the translucency family.
///
/// gamemd-derived: `AnimClass::DrawIt` loads the producer's whole flag word out
/// of `AnimClass+0x190` (`0x0042304B MOV EBX,[ESI+0x190]`) and then ORs the
/// selector INTO it — `0x00423075 OR EBX,0x6`, `0x0042307A OR EBX,0x4`, and the
/// same pattern for the `Translucency=`/`Translucent=` arms further down. So a
/// flag word carries the producer's bits (`0x600` for a trailer, `0x2600` for a
/// combat explosion) alongside the family, and the family must be read as a
/// masked field, never by matching the whole word.
pub const ANIM_DRAW_BITS_TRANSLUCENCY_MASK: u32 = 0x6;

/// Source-pixel weight implied by a set of translucency draw bits.
///
/// This is the renderer-facing form of the blitter table above: the weight the
/// native blend gives the incoming sprite pixel. Only
/// [`ANIM_DRAW_BITS_TRANSLUCENCY_MASK`] is consulted, so a caller may pass the
/// complete flag word; an unset family draws opaque. Float is correct here — it
/// is a colour-target blend factor consumed only by the render layer, never by
/// simulation math.
pub const fn anim_translucency_source_alpha(draw_bits: u32) -> f32 {
    match draw_bits & ANIM_DRAW_BITS_TRANSLUCENCY_MASK {
        ANIM_DRAW_BITS_TRANSLUCENT_25 => 0.75,
        ANIM_DRAW_BITS_TRANSLUCENT_50 => 0.5,
        ANIM_DRAW_BITS_TRANSLUCENT_75 => 0.25,
        _ => 1.0,
    }
}

/// Alpha an animation sprite instance should carry for one drawn frame.
///
/// Single entry point for instance builders: resolve the draw bits for this
/// frame, then convert them to the source weight the native blend applies. An
/// animation with neither `Translucent=` nor a recognised `Translucency=` returns
/// `1.0`, so wiring this in cannot change an opaque animation.
pub fn anim_frame_source_alpha(
    config: &AnimTypeRuntimeConfig,
    current_frame: i32,
    shp_frame_count: i32,
) -> f32 {
    anim_translucency_source_alpha(anim_translucency_draw_bits(
        config,
        current_frame,
        shp_frame_count,
    ))
}

fn parse_anim_runtime_config(section: &IniSection) -> AnimTypeRuntimeConfig {
    let explicit_end = section.get_i32("End");
    let explicit_loop_end = section.get_i32("LoopEnd");
    AnimTypeRuntimeConfig {
        art_body_read: true,
        start: section.get_i32("Start").unwrap_or(0),
        loop_start: section.get_i32("LoopStart").unwrap_or(0),
        loop_end: explicit_loop_end.unwrap_or(0),
        end: explicit_end.unwrap_or(0),
        explicit_end,
        explicit_loop_end,
        raw_shp_frame_count: None,
        loop_count: section.get_i32("LoopCount").unwrap_or(0),
        rate_logic_frames: section
            .get_i32("Rate")
            .map(art_rate_to_logic_frames)
            .unwrap_or(DEFAULT_ART_RATE_LOGIC_FRAMES),
        normalized: section.get_bool("Normalized").unwrap_or(false),
        tiberium_chain_reaction: section.get_bool("TiberiumChainReaction").unwrap_or(false),
        is_tiberium: section.get_bool("IsTiberium").unwrap_or(false),
        hide_if_no_ore: section.get_bool("HideIfNoOre").unwrap_or(false),
        is_animated_tiberium: section.get_bool("IsAnimatedTiberium").unwrap_or(false),
        tiberium_spread_radius: section.get_i32("TiberiumSpreadRadius").unwrap_or(0),
        tiberium_spawn_type: parse_anim_ref(section, "TiberiumSpawnType"),
        make_infantry: section.get_i32("MakeInfantry").unwrap_or(-1),
        bouncer: section.get_bool("Bouncer").unwrap_or(false),
        spawns: parse_anim_ref(section, "Spawns"),
        spawn_count: section.get_i32("SpawnCount").unwrap_or(0),
        running_frames: section.get_i32("RunningFrames").unwrap_or(0),
        // gamemd-derived: `AnimTypeClass::ReadINI @ 0x00427D00` reads these
        // through `CCINIClass::ReadDouble` and each defaults to the value the
        // constructor (`AnimTypeClass::AnimTypeClass @ 0x00427530`) left.
        damage: read_anim_double(section, "Damage", ANIM_DAMAGE_DEFAULT),
        warhead: parse_anim_ref(section, "Warhead"),
        damage_radius: section.get_i32("DamageRadius").unwrap_or(0),
        elasticity: read_anim_double(section, "Elasticity", ANIM_ELASTICITY_DEFAULT),
        min_z_vel: read_anim_double(section, "MinZVel", ANIM_MIN_Z_VEL_DEFAULT),
        max_xy_vel: read_anim_double(section, "MaxXYVel", ANIM_MAX_XY_VEL_DEFAULT),
        is_meteor: section.get_bool("IsMeteor").unwrap_or(false),
        is_veins: section.get_bool("IsVeins").unwrap_or(false),
        is_flaming_guy: section.get_bool("IsFlamingGuy").unwrap_or(false),
        scorch: section.get_bool("Scorch").unwrap_or(false),
        crater: section.get_bool("Crater").unwrap_or(false),
        force_big_craters: section.get_bool("ForceBigCraters").unwrap_or(false),
        sticky: section.get_bool("Sticky").unwrap_or(false),
        psi_warning: section.get_bool("PsiWarning").unwrap_or(false),
        double_thick: section.get_bool("DoubleThick").unwrap_or(false),
        flamer: section.get_bool("Flamer").unwrap_or(false),
        spawns_particle: parse_anim_ref(section, "SpawnsParticle"),
        num_particles: section.get_i32("NumParticles").unwrap_or(0),
        // Both constructor defaults are 1, not 0.
        should_fog_remove: section.get_bool("ShouldFogRemove").unwrap_or(true),
        should_use_cell_drawer: section.get_bool("ShouldUseCellDrawer").unwrap_or(true),
        detail_level: section.get_i32("DetailLevel").unwrap_or(0),
        next: section.get("Next").map(|s| s.trim().to_ascii_uppercase()),
        bounce_anim: parse_anim_ref(section, "BounceAnim"),
        expire_anim: parse_anim_ref(section, "ExpireAnim"),
        trailer_anim: parse_anim_ref(section, "TrailerAnim"),
        trailer_seperation: section.get_i32("TrailerSeperation").unwrap_or(0),
        random_loop_delay: section.get("RandomLoopDelay").and_then(parse_u16_pair),
        random_rate_logic_frames: section.get("RandomRate").and_then(parse_random_rate_pair),
        y_draw_offset: section.get_i32("YDrawOffset").unwrap_or(0),
        z_adjust: section.get_i32("ZAdjust").unwrap_or(0),
        y_sort_adjust: section.get_i32("YSortAdjust").unwrap_or(0),
        layer: AnimLayer::from_ini(section.get("Layer")),
        flat: section.get_bool("Flat").unwrap_or(false),
        tiled: section.get_bool("Tiled").unwrap_or(false),
        translucency: section.get_i32("Translucency").unwrap_or(0),
        translucency_detail_level: section.get_i32("TranslucencyDetailLevel").unwrap_or(0),
        translucent: section.get_bool("Translucent").unwrap_or(false),
        alt_palette: section.get_bool("AltPalette").unwrap_or(false),
        // AnimTypeClass's constructor zeroes this field before the INI read and
        // the read passes the current field back as its own default, so an
        // omitted key leaves it false.
        use_normal_light: section.get_bool("UseNormalLight").unwrap_or(false),
        shadow: section.get_bool("Shadow").unwrap_or(false),
        ping_pong: section.get_bool("PingPong").unwrap_or(false),
        reverse: section.get_bool("Reverse").unwrap_or(false),
        report: parse_anim_ref(section, "Report"),
        start_sound: parse_anim_ref(section, "StartSound"),
        stop_sound: parse_anim_ref(section, "StopSound"),
    }
}

/// `AnimTypeClass::AnimTypeClass @ 0x00427530` constructor defaults for the four
/// `ReadDouble` animation keys, taken from the literal dword pairs the
/// constructor stores.
///
/// - `Damage`   `param_1[0xaa] = 0; param_1[0xab] = 0`            -> `0.0`
/// - `Elasticity` `[0xc4] = 0xA0000000; [0xc5] = 0x3FE99999`      -> `0.8f` as f64
/// - `MinZVel`  `[0xc6] = 0; [0xc7] = 0x400C0000`                 -> `3.5`
/// - `MaxXYVel` `[0xca] = 0; [0xcb] = 0x402E0000`                 -> `15.0`
///
/// `Elasticity` is worth spelling out: `0x3FE99999_A0000000` is the **single**
/// precision `0.8f` (`0x3F4CCCCD`) widened, i.e. `0.800000011920929`, not the
/// nearest double to 0.8 (`0x3FE99999_99999999A`). That is consistent with the
/// authored path — `CCINIClass::ReadDouble` also parses `%f` and widens — so a
/// port that writes `0.8_f64` here starts every unauthored bouncer from a
/// different bit pattern than gamemd.
const ANIM_DAMAGE_DEFAULT: NativeF64Bits = NativeF64Bits::POSITIVE_ZERO;
const ANIM_ELASTICITY_DEFAULT: NativeF64Bits = NativeF64Bits::from_bits(0x3fe9_9999_a000_0000);
const ANIM_MIN_Z_VEL_DEFAULT: NativeF64Bits = NativeF64Bits::from_bits(0x400c_0000_0000_0000);
const ANIM_MAX_XY_VEL_DEFAULT: NativeF64Bits = NativeF64Bits::from_bits(0x402e_0000_0000_0000);

/// One `CCINIClass::ReadDouble` animation key, retained as native bit pattern.
///
/// The bits are kept rather than an `f64` so `AnimTypeRuntimeConfig` stays
/// `Eq`/`Hash`, and so a consumer that must reproduce a native x87 sequence has
/// the exact stored value rather than a re-parsed approximation.
fn read_anim_double(section: &IniSection, key: &str, default: NativeF64Bits) -> NativeF64Bits {
    NativeF64Bits::from_bits(
        section
            .read_double(key, f64::from_bits(default.bits()))
            .to_bits(),
    )
}

fn parse_anim_ref(section: &IniSection, key: &str) -> Option<String> {
    section
        .get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_uppercase)
}

fn parse_u16_pair(value: &str) -> Option<(u16, u16)> {
    let mut parts = value.split(',').map(str::trim);
    let a = parts.next()?.parse::<i32>().ok()?.max(0) as u16;
    let b = parts.next()?.parse::<i32>().ok()?.max(0) as u16;
    Some((a, b))
}

fn parse_random_rate_pair(value: &str) -> Option<(u16, u16)> {
    let mut parts = value.split(',').map(str::trim);
    let mut low = art_rate_to_logic_frames(parts.next()?.parse::<i32>().ok()?);
    let high = art_rate_to_logic_frames(parts.next()?.parse::<i32>().ok()?);
    if high < low {
        low = high;
    }
    Some((low, high))
}

/// Default native frame delay when art.ini section has no `Rate=` key.
pub const DEFAULT_ART_RATE_LOGIC_FRAMES: u16 = 1;

/// Default ms-per-frame when art.ini section has no `Rate=` key.
/// Matches gamemd constructor default of 1 game frame at ~15fps.
pub const DEFAULT_ART_RATE_MS: u16 = 67;

/// Exact object-art resolution from rules + art metadata.
///
/// This is the data-driven layer only. It does not generate filenames.
#[derive(Debug, Clone)]
pub struct ResolvedObjectArt<'a> {
    /// Base art identity from rules `Image=` or the object type id.
    pub base_art_id: String,
    /// Final image id after art.ini `Image=` override.
    pub image_id: String,
    /// Section id whose metadata should be used for overlays/bibs/anims.
    pub metadata_section_id: String,
    /// Parsed art entry for `metadata_section_id`, if present.
    pub entry: Option<&'a ArtEntry>,
}

/// Lookup table for art.ini rendering data.
#[derive(Debug, Clone)]
pub struct ArtRegistry {
    /// image_id (uppercase) -> ArtEntry.
    entries: HashMap<String, ArtEntry>,
    /// section ID (uppercase) -> `CanHideThings=`. Missing sections default true.
    can_hide_things: HashMap<String, bool>,
    /// section ID (uppercase) -> `OccupyHeight=`, defaulting to art `Height=`.
    occupy_heights: HashMap<String, i32>,
    /// section ID (uppercase) -> generic AnimType frame delay from `Rate=`.
    /// Missing `Rate=` keeps the gamemd constructor default of one logic frame.
    rates_ms: HashMap<String, u16>,
    /// section ID (uppercase) -> native AnimType frame delay in logic frames.
    rates_logic_frames: HashMap<String, u16>,
    /// section ID (uppercase) -> generic AnimType runtime metadata.
    anim_runtime_configs: HashMap<String, AnimTypeRuntimeConfig>,
    /// Full deterministic Next/Trailer closure reachable from scheduler-owned roots.
    scheduler_anim_types: BTreeSet<String>,
    damage_fire_offsets_valid: bool,
}

fn validate_loaded_bound(
    name: &str,
    field: &'static str,
    value: i32,
    raw_count: i32,
) -> Result<(), AnimAssetBindError> {
    if value < -1 || value > raw_count {
        return Err(AnimAssetBindError::InvalidLoadedBound {
            name: name.to_string(),
            field,
            value,
            raw_count,
        });
    }
    Ok(())
}

fn resolve_loaded_bounds(
    name: &str,
    raw_count: i32,
    shadow: bool,
    explicit_end: Option<i32>,
    explicit_loop_end: Option<i32>,
) -> Result<(i32, i32), AnimAssetBindError> {
    if raw_count <= 0 {
        return Err(AnimAssetBindError::InvalidFrameCount {
            name: name.to_string(),
            count: raw_count,
        });
    }
    let header_end = if shadow { raw_count / 2 } else { raw_count };
    // Native copies the loader-derived End into LoopEnd before applying the
    // explicit INI End and LoopEnd keys.
    let loaded_end = explicit_end.unwrap_or(header_end);
    let loaded_loop_end = explicit_loop_end.unwrap_or(header_end);
    validate_loaded_bound(name, "End", loaded_end, raw_count)?;
    validate_loaded_bound(name, "LoopEnd", loaded_loop_end, raw_count)?;
    Ok((loaded_end, loaded_loop_end))
}

/// Verified `TacticalClass::IsometricPixelToWorld` transform for the initialized
/// retail tactical matrix. Native evaluates the row in extended precision, stores
/// one f32 result, then truncates toward zero in `Math::ftol`.
fn damage_fire_world_delta(pixel_x: i32, pixel_y: i32) -> Option<(i32, i32)> {
    const EXACT_F32_INTEGER_LIMIT: i32 = 1 << 24;
    const ISO_PIXEL_TO_WORLD_A: f32 = f32::from_bits(0x4088_88CE);

    let twice_y = pixel_y.checked_mul(2)?;
    let native_x = pixel_x.checked_add(twice_y)?;
    let native_y = pixel_x.checked_neg()?.checked_add(twice_y)?;
    for value in [pixel_x, pixel_y, native_x, native_y] {
        if value.unsigned_abs() > EXACT_F32_INTEGER_LIMIT as u32 {
            return None;
        }
    }
    let world_x = ISO_PIXEL_TO_WORLD_A * native_x as f32;
    let world_y = ISO_PIXEL_TO_WORLD_A * native_y as f32;
    if !world_x.is_finite() || !world_y.is_finite() {
        return None;
    }
    Some((world_x.trunc() as i32, world_y.trunc() as i32))
}

/// Hardcoded filename prefixes that always receive `NewTheater` treatment
/// regardless of the `NewTheater=` INI key.
const NEW_THEATER_PREFIXES: &[&str] = &["GA", "GT", "NA", "NT", "CA", "CT"];

/// `repo-derived`: theater name -> replacement letter for `NewTheater`.
const THEATER_LETTERS: &[(&str, char)] = &[
    ("TEMPERATE", 'T'),
    ("SNOW", 'A'),
    ("URBAN", 'U'),
    ("DESERT", 'D'),
    ("LUNAR", 'L'),
    ("NEWURBAN", 'N'),
];

/// `repo-derived`: generic fallback letter used by original-style building art.
const NEW_THEATER_GENERIC_LETTER: char = 'G';

impl ArtRegistry {
    /// Parse all sections from an art.ini IniFile into the registry.
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut entries: HashMap<String, ArtEntry> = HashMap::new();
        let mut can_hide_things: HashMap<String, bool> = HashMap::new();
        let mut occupy_heights: HashMap<String, i32> = HashMap::new();
        let mut rates_ms: HashMap<String, u16> = HashMap::new();
        let mut rates_logic_frames: HashMap<String, u16> = HashMap::new();
        let mut anim_runtime_configs: HashMap<String, AnimTypeRuntimeConfig> = HashMap::new();
        let mut damage_fire_offsets_valid = true;

        for section_name in ini.section_names() {
            let section = match ini.section(section_name) {
                Some(s) => s,
                None => continue,
            };

            let image: Option<String> = section.get("Image").map(|s| s.to_string());
            let cameo: Option<String> = section.get("Cameo").map(|s| s.to_string());
            let alt_cameo: Option<String> = section.get("AltCameo").map(|s| s.to_string());
            let new_theater: bool = section.get_bool("NewTheater").unwrap_or(false);
            let theater: bool = section.get_bool("Theater").unwrap_or(false);
            let scorch: bool = section.get_bool("Scorch").unwrap_or(false);
            let crater: bool = section.get_bool("Crater").unwrap_or(false);
            let force_big_craters: bool = section.get_bool("ForceBigCraters").unwrap_or(false);
            let voxel: bool = section.get_bool("Voxel").unwrap_or(false);
            let turret_offset: i32 = section.get_i32("TurretOffset").unwrap_or(0);
            let y_draw_offset: i32 = section.get_i32("YDrawOffset").unwrap_or(0);
            let x_draw_offset: i32 = section.get_i32("XDrawOffset").unwrap_or(0);
            let building_anims: Vec<BuildingAnimConfig> = parse_building_anims(section, ini);
            let foundation: Option<String> = section
                .get("Foundation")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let to_overlay: Option<String> = section
                .get("ToOverlay")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let bib_shape: Option<String> = section
                .get("BibShape")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let palette: Option<String> = section
                .get("Palette")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase());
            let sequence: Option<String> = section
                .get("Sequence")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            // Pull per-phase frame counts from the referenced sequence section
            // (e.g. GuardianGISequence::Deploy=300,15,0 -> deploy_frames=15).
            let (deploy_frames, undeploy_frames, deployed_fire_frames) = sequence
                .as_deref()
                .and_then(|seq_name| ini.section(seq_name))
                .map(|seq_section| {
                    (
                        seq_section.get("Deploy").and_then(parse_sequence_frames),
                        seq_section.get("Undeploy").and_then(parse_sequence_frames),
                        seq_section
                            .get("DeployedFire")
                            .and_then(parse_sequence_frames),
                    )
                })
                .unwrap_or((None, None, None));
            let crawls = section.get_bool("Crawls").unwrap_or(false);
            let primary_fire_flh: Flh = parse_flh(section.get("PrimaryFireFLH"));
            let secondary_fire_flh: Flh = parse_flh(section.get("SecondaryFireFLH"));
            let elite_primary_fire_flh: Option<Flh> = section
                .get("ElitePrimaryFireFLH")
                .map(|v| parse_flh(Some(v)));
            let elite_secondary_fire_flh: Option<Flh> = section
                .get("EliteSecondaryFireFLH")
                .map(|v| parse_flh(Some(v)));
            let primary_fire_pixel_offset = section
                .get("PrimaryFirePixelOffset")
                .and_then(parse_i32_pair);
            let secondary_fire_pixel_offset = section
                .get("SecondaryFirePixelOffset")
                .and_then(parse_i32_pair);
            let primary_fire_dual_offset =
                section.get_bool("PrimaryFireDualOffset").unwrap_or(false);
            // BuildingTypeClass's constructor supplies false/0 and ReadINI
            // preserves the signed delay verbatim. The runtime consumer is
            // BuildingClass::Mission_Attack @ 0x0044ACF0.
            let is_anim_delayed_fire = section.get_bool("IsAnimDelayedFire").unwrap_or(false);
            let delayed_fire_delay = section.get_i32("DelayedFireDelay").unwrap_or(0);

            // SHP vehicle frame tags (only meaningful when Voxel=no for vehicles).
            let walk_frames: Option<u16> = section.get_i32("WalkFrames").map(|v| v.max(0) as u16);
            let firing_frames: Option<u16> =
                section.get_i32("FiringFrames").map(|v| v.max(0) as u16);
            let standing_frames: Option<u16> =
                section.get_i32("StandingFrames").map(|v| v.max(0) as u16);
            let shp_facings: u8 = section
                .get_i32("Facings")
                .map(|v| v.clamp(1, 32) as u8)
                .unwrap_or(8);
            let fire_up: u8 = section
                .get_i32("FireUp")
                .map(|v| v.max(0) as u8)
                .unwrap_or(0);
            let fire_prone: u8 = section
                .get_i32("FireProne")
                .map(|v| v.max(0) as u8)
                .unwrap_or(fire_up);
            let secondary_fire: u8 = section
                .get_i32("SecondaryFire")
                .map(|v| v.max(0) as u8)
                .unwrap_or(fire_up);
            let secondary_prone: u8 = section
                .get_i32("SecondaryProne")
                .map(|v| v.max(0) as u8)
                .unwrap_or(secondary_fire);
            let report = section.get("Report").map(|s| s.to_string());
            let start_sound = section.get("StartSound").map(|s| s.to_string());
            let rate_ms: u16 = section
                .get_i32("Rate")
                .map(|r| art_rate_to_delay_ms(r) as u16)
                .unwrap_or(DEFAULT_ART_RATE_MS);
            let rate_logic_frames: u16 = section
                .get_i32("Rate")
                .map(art_rate_to_logic_frames)
                .unwrap_or(DEFAULT_ART_RATE_LOGIC_FRAMES);
            let anim_runtime_config = parse_anim_runtime_config(section);
            let extra_light: i32 = section.get_i32("ExtraLight").unwrap_or(0);
            let queueing_cell: Option<(u16, u16)> = section.get("QueueingCell").and_then(|s| {
                let mut parts = s.split(',');
                let x = parts.next()?.trim().parse::<u16>().ok()?;
                let y = parts.next()?.trim().parse::<u16>().ok()?;
                Some((x, y))
            });
            // Multi-pad parser: read DockingOffset0..7 from art.ini.
            // Over-reads here; the art→rules merge in ruleset.rs truncates or
            // zero-pads to match rules.ini NumberOfDocks. 8 is a defensive
            // ceiling for mod safety (retail uses up to 4).
            let pads: Vec<crate::rules::object_type::DockPad> = (0..8)
                .filter_map(|i| {
                    let key = format!("DockingOffset{}", i);
                    section.get(&key).and_then(|s| {
                        let mut parts = s.split(',');
                        let x = parts.next()?.trim().parse::<i32>().ok()?;
                        let y = parts.next()?.trim().parse::<i32>().ok()?;
                        let z = parts
                            .next()
                            .and_then(|v| v.trim().parse::<i32>().ok())
                            .unwrap_or(0);
                        Some(crate::rules::object_type::DockPad {
                            lepton_offset: (x, y, z),
                        })
                    })
                })
                .collect();
            let damage_fire_offsets: Vec<DamageFireOffset> = {
                let mut offsets = Vec::new();
                for i in 0..8 {
                    let key = format!("DamageFireOffset{}", i);
                    if let Some(val) = section.get(&key) {
                        let mut parts = val.split(',');
                        let parsed = (
                            parts.next().and_then(|s| s.trim().parse::<i32>().ok()),
                            parts.next().and_then(|s| s.trim().parse::<i32>().ok()),
                        );
                        if let (Some(x), Some(y)) = parsed {
                            match damage_fire_world_delta(x, y) {
                                Some((world_dx, world_dy)) => offsets.push(DamageFireOffset {
                                    pixel_x: x,
                                    pixel_y: y,
                                    world_dx,
                                    world_dy,
                                }),
                                None => {
                                    damage_fire_offsets_valid = false;
                                    offsets.push(DamageFireOffset {
                                        pixel_x: x,
                                        pixel_y: y,
                                        world_dx: 0,
                                        world_dy: 0,
                                    });
                                }
                            }
                        } else {
                            damage_fire_offsets_valid = false;
                            break;
                        }
                    } else {
                        break;
                    }
                }
                offsets
            };
            // An absent `Height=` leaves the BuildingTypeClass constructor's own
            // value, which is 2 — the ctor stores a literal 2 into the height
            // field, and ReadINI passes the current field as the INI default, so
            // a section that omits the key keeps it. 187 of the 568 stock art
            // sections carrying a `Foundation=` omit `Height=`, including every
            // garrisonable city building, so defaulting to 0 collapsed their
            // selection bracket box onto the ground plane and put their health
            // bar and pips at ground level instead of roof level.
            //
            // No stock art section authors `Height=0`, so this changes only the
            // absent case.
            let height: i32 = section.get_i32("Height").unwrap_or(2);
            let can_hide: bool = section.get_bool("CanHideThings").unwrap_or(true);
            // BuildingTypeClass reads Height first, then passes that resolved
            // field value as the default for OccupyHeight.
            let occupy_height: i32 = section.get_i32("OccupyHeight").unwrap_or(height);
            let muzzle_flash_positions: Vec<(i32, i32)> = {
                let mut positions = Vec::new();
                for i in 0..10 {
                    let key = format!("MuzzleFlash{}", i);
                    if let Some(val) = section.get(&key) {
                        let mut parts = val.split(',');
                        if let (Some(x), Some(y)) = (
                            parts.next().and_then(|s| s.trim().parse::<i32>().ok()),
                            parts.next().and_then(|s| s.trim().parse::<i32>().ok()),
                        ) {
                            positions.push((x, y));
                        }
                    } else {
                        break;
                    }
                }
                positions
            };
            let add_occupy = parse_numbered_cell_offsets(section, "AddOccupy");
            let remove_occupy = parse_numbered_cell_offsets(section, "RemoveOccupy");
            let z_shape_point_move: (i32, i32) = section
                .get("ZShapePointMove")
                .and_then(parse_i32_pair)
                .unwrap_or((0, 0));
            let normal_z_adjust: i32 = section.get_i32("NormalZAdjust").unwrap_or(0);

            let section_key = section_name.to_uppercase();
            can_hide_things.insert(section_key.clone(), can_hide);
            occupy_heights.insert(section_key.clone(), occupy_height);
            rates_ms.insert(section_key.clone(), rate_ms);
            rates_logic_frames.insert(section_key.clone(), rate_logic_frames);
            anim_runtime_configs.insert(section_key.clone(), anim_runtime_config);
            entries.insert(
                section_key,
                ArtEntry {
                    image,
                    cameo,
                    alt_cameo,
                    new_theater,
                    theater,
                    scorch,
                    crater,
                    force_big_craters,
                    frame_width: 30,
                    frame_height: 30,
                    voxel,
                    authored_voxel: section.get_bool("Voxel"),
                    turret_offset,
                    y_draw_offset,
                    x_draw_offset,
                    building_anims,
                    building_anim_power: {
                        let mut records = [BuildingAnimPowerFlags::default(); 21];
                        read_building_anim_power(section, &mut records);
                        records
                    },
                    building_gate_stages: section.get_i32("GateStages").unwrap_or(9),
                    building_body_ranges: ["AnimIdle", "AnimActive", "AnimAux1", "AnimAux2"]
                        .map(|key| read_building_body_range(section, key)),
                    foundation,
                    to_overlay,
                    bib_shape,
                    palette,
                    sequence,
                    crawls,
                    primary_fire_flh,
                    secondary_fire_flh,
                    elite_primary_fire_flh,
                    elite_secondary_fire_flh,
                    primary_fire_pixel_offset,
                    secondary_fire_pixel_offset,
                    primary_fire_dual_offset,
                    is_anim_delayed_fire,
                    silo_damage: section.get_bool("SiloDamage").unwrap_or(false),
                    delayed_fire_delay,
                    walk_frames,
                    firing_frames,
                    standing_frames,
                    shp_facings,
                    fire_up,
                    fire_prone,
                    secondary_fire,
                    secondary_prone,
                    report,
                    start_sound,
                    extra_light,
                    terrain_palette: section.get_bool("TerrainPalette").unwrap_or(false),
                    queueing_cell,
                    pads,
                    damage_fire_offsets,
                    height,
                    muzzle_flash_positions,
                    add_occupy,
                    remove_occupy,
                    deploy_frames,
                    undeploy_frames,
                    deployed_fire_frames,
                    z_shape_point_move,
                    normal_z_adjust,
                },
            );
        }

        log::info!("ArtRegistry: {} entries loaded from art.ini", entries.len());
        ArtRegistry {
            entries,
            can_hide_things,
            occupy_heights,
            rates_ms,
            rates_logic_frames,
            anim_runtime_configs,
            scheduler_anim_types: BTreeSet::new(),
            damage_fire_offsets_valid,
        }
    }

    /// Create an empty registry (used when art.ini is unavailable).
    pub fn empty() -> Self {
        ArtRegistry {
            entries: HashMap::new(),
            can_hide_things: HashMap::new(),
            occupy_heights: HashMap::new(),
            rates_ms: HashMap::new(),
            rates_logic_frames: HashMap::new(),
            anim_runtime_configs: HashMap::new(),
            scheduler_anim_types: BTreeSet::new(),
            damage_fire_offsets_valid: true,
        }
    }

    /// Look up art entry for an image ID (case-insensitive).
    pub fn get(&self, image_id: &str) -> Option<&ArtEntry> {
        self.entries.get(&image_id.to_uppercase())
    }

    /// Generic AnimType frame delay for an art section, from `Rate=`.
    pub fn rate_ms(&self, image_id: &str) -> Option<u16> {
        self.rates_ms.get(&image_id.to_uppercase()).copied()
    }

    /// Generic AnimType frame delay in native logic frames for an art section.
    pub fn rate_logic_frames(&self, image_id: &str) -> Option<u16> {
        self.rates_logic_frames
            .get(&image_id.to_uppercase())
            .copied()
    }

    /// Generic native-like AnimType runtime metadata for an art section.
    pub fn anim_runtime_config(&self, image_id: &str) -> Option<&AnimTypeRuntimeConfig> {
        self.anim_runtime_configs.get(&image_id.to_uppercase())
    }

    /// Apply the canonical RulesClass processing receipt before asset binding.
    /// Building/weapon references allocated after the AnimType sweep retain
    /// constructor state even when a same-named ART section exists.
    pub fn apply_anim_type_read_states(&mut self, states: &[(String, bool)]) {
        let mut seen = BTreeSet::new();
        for (name, read) in states {
            let key = name.to_ascii_uppercase();
            // Find returns the first slot when native truncated IDs collide.
            if !seen.insert(key.clone()) || *read {
                continue;
            }
            let mut config = parse_anim_runtime_config(&IniSection::new(name.clone()));
            config.art_body_read = false;
            self.anim_runtime_configs.insert(key, config);
        }
    }

    /// Animation types whose complete raw SHP ranges must be available to the
    /// scheduler-owned runtime.
    pub fn scheduler_anim_types(&self) -> &BTreeSet<String> {
        &self.scheduler_anim_types
    }

    #[cfg(test)]
    pub(crate) fn bind_anim_frame_count_for_test(&mut self, name: &str, raw_count: i32) {
        let key = name.to_ascii_uppercase();
        let config = self
            .anim_runtime_configs
            .get_mut(&key)
            .expect("test animation section must exist");
        let (loaded_end, loaded_loop_end) = resolve_loaded_bounds(
            &key,
            raw_count,
            config.shadow,
            config.explicit_end,
            config.explicit_loop_end,
        )
        .expect("test animation bounds must be valid");
        config.raw_shp_frame_count = Some(raw_count);
        config.end = loaded_end;
        config.loop_end = loaded_loop_end;
        self.scheduler_anim_types.insert(key);
    }

    /// Bind registered combat-explosion roots to the common animation runtime.
    /// Canonical processing records whether each AnimType reached fixed-ART
    /// ReadINI. Unread types retain constructor End0/LoopEnd0/Rate1:427D22 exits
    /// before427DDD's image loader, and the lazy bounds gates require End=-1.
    /// They still construct real AnimClass identities and enter Logic; the
    /// first-AI guard runs before their later expiration. Orphan SHP files do
    /// not authorize asset loading or drawing for these types.
    ///
    /// Missing registered ART is therefore handled by the canonical receipt,
    /// not this loader's error policy. Other failed asset bindings are counted
    /// for the caller; they do not create a second AnimType registry.
    ///
    /// Follows each bound type's `Next=`/`TrailerAnim=` like the strict binder,
    /// so a chained type has loader-derived bounds when the store switches to
    /// it. A type that fails to bind ends its chain here.
    pub fn bind_anim_class_assets(
        &mut self,
        roots: &[String],
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> usize {
        let mut skipped = 0;
        let mut pending: VecDeque<String> = roots
            .iter()
            .map(|root| root.trim().to_ascii_uppercase())
            .filter(|name| !name.is_empty())
            .collect();
        let mut visited = BTreeSet::new();
        while let Some(name) = pending.pop_front() {
            if !visited.insert(name.clone()) || self.scheduler_anim_types.contains(&name) {
                continue;
            }
            match self.bind_one_anim_asset(&name, asset_manager, theater_ext, theater_name) {
                Ok(()) => {
                    if let Some(config) = self.anim_runtime_configs.get(&name) {
                        pending.extend(config.next.iter().cloned());
                        pending.extend(config.trailer_anim.iter().cloned());
                    }
                    self.scheduler_anim_types.insert(name);
                }
                Err(error) => {
                    skipped += 1;
                    log::warn!(
                        "AnimClass animation [{name}] did not bind and will draw nothing: {error}"
                    );
                }
            }
        }
        skipped
    }

    /// Resolve one animation's SHP and write the loader-derived bounds back into
    /// its runtime config. Shared by the strict and tolerant binders.
    fn bind_one_anim_asset(
        &mut self,
        name: &str,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Result<(), AnimAssetBindError> {
        let config = self
            .anim_runtime_configs
            .get(name)
            .cloned()
            .ok_or_else(|| AnimAssetBindError::MissingAnimType(name.to_string()))?;
        if !config.art_body_read {
            return Ok(());
        }
        let image_id = self.resolve_effective_image_id(name, name);
        let candidates =
            anim_shp_candidates(Some(self), name, &image_id, theater_ext, theater_name);
        let data = candidates
            .iter()
            .find_map(|candidate| asset_manager.get_ref(candidate))
            .ok_or_else(|| AnimAssetBindError::MissingShp(name.to_string()))?;
        let raw_count = data
            .get(6..8)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as i32)
            .unwrap_or(0);
        let (loaded_end, loaded_loop_end) = resolve_loaded_bounds(
            name,
            raw_count,
            config.shadow,
            config.explicit_end,
            config.explicit_loop_end,
        )?;
        let bound = self
            .anim_runtime_configs
            .get_mut(name)
            .expect("configuration was resolved above");
        bound.raw_shp_frame_count = Some(raw_count);
        bound.end = loaded_end;
        bound.loop_end = loaded_loop_end;
        Ok(())
    }

    /// Bind native loader-derived End/LoopEnd values for the transitive
    /// Next/Trailer closure rooted at the supplied animation names.
    pub fn bind_scheduler_anim_assets(
        &mut self,
        roots: &[String],
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Result<(), AnimAssetBindError> {
        if !self.damage_fire_offsets_valid {
            return Err(AnimAssetBindError::InvalidDamageFireOffset);
        }

        let mut pending: VecDeque<String> = roots
            .iter()
            .map(|name| name.trim().to_ascii_uppercase())
            .filter(|name| !name.is_empty())
            .collect();
        let mut resolved = BTreeSet::new();

        while let Some(name) = pending.pop_front() {
            if !resolved.insert(name.clone()) {
                continue;
            }
            self.bind_one_anim_asset(&name, asset_manager, theater_ext, theater_name)?;
            let config = self
                .anim_runtime_configs
                .get(&name)
                .cloned()
                .expect("binding above resolved this configuration");

            if let Some(next) = config.next {
                pending.push_back(next);
            }
            if let Some(trailer) = config.trailer_anim {
                pending.push_back(trailer);
            }
        }

        self.scheduler_anim_types = resolved;
        Ok(())
    }

    /// Hidden-occupancy gate from the art.ini object-type-ID section.
    /// The building type constructor defaults this to true.
    pub fn can_hide_things(&self, type_id: &str) -> bool {
        self.can_hide_things
            .get(&type_id.to_uppercase())
            .copied()
            .unwrap_or(true)
    }

    /// Hidden-occupancy height from art.ini `OccupyHeight=`.
    /// art.ini comments define the absent-key fallback as the section `Height=`;
    /// missing sections fall back to 2, the documented building-art default.
    pub fn occupy_height(&self, image_id: &str) -> i32 {
        self.occupy_heights
            .get(&image_id.to_uppercase())
            .copied()
            .unwrap_or(2)
    }

    /// Resolve the hidden-counter profile from its two distinct ART sections:
    /// `CanHideThings` belongs to the object type ID, while height and numbered
    /// offsets belong to the effective rules `Image=` section.
    pub fn building_hidden_occupancy_profile(
        &self,
        type_id: &str,
        image_id: &str,
    ) -> BuildingHiddenOccupancyProfile {
        let image_entry = self.get(image_id);
        BuildingHiddenOccupancyProfile {
            can_hide_things: self.can_hide_things(type_id),
            occupy_height: self.occupy_height(image_id),
            add_occupy: image_entry
                .map(|entry| entry.add_occupy)
                .unwrap_or([None; HIDDEN_OCCUPY_SLOT_COUNT]),
            remove_occupy: image_entry
                .map(|entry| entry.remove_occupy)
                .unwrap_or([None; HIDDEN_OCCUPY_SLOT_COUNT]),
        }
    }

    /// Number of entries in the registry.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get (x_draw_offset, y_draw_offset) for an object type. Returns (0, 0) if unknown.
    pub fn draw_offsets(&self, type_id: &str) -> (i32, i32) {
        self.get(type_id)
            .map(|e| (e.x_draw_offset, e.y_draw_offset))
            .unwrap_or((0, 0))
    }

    /// Resolve exact object-art identities from rules/art data only.
    pub fn resolve_object_art<'a>(
        &'a self,
        type_id: &str,
        rules_image: &str,
    ) -> ResolvedObjectArt<'a> {
        let type_upper: String = type_id.to_uppercase();
        let base_art_id: String = normalize_id(rules_image).unwrap_or_else(|| type_upper.clone());
        let image_id: String = self
            .get(&base_art_id)
            .and_then(|entry| normalize_id(entry.image.as_deref().unwrap_or_default()))
            .unwrap_or_else(|| base_art_id.clone());

        let metadata_section_id: String = if self.get(&base_art_id).is_some() {
            base_art_id.clone()
        } else if self.get(&type_upper).is_some() {
            type_upper.clone()
        } else {
            image_id.clone()
        };
        let entry: Option<&ArtEntry> = self.get(&metadata_section_id);

        ResolvedObjectArt {
            base_art_id,
            image_id,
            metadata_section_id,
            entry,
        }
    }

    /// Resolve the art metadata section for an object.
    pub fn resolve_metadata_entry<'a>(
        &'a self,
        type_id: &str,
        rules_image: &str,
    ) -> Option<&'a ArtEntry> {
        self.resolve_object_art(type_id, rules_image).entry
    }

    /// Resolve the effective image id for an object.
    pub fn resolve_effective_image_id(&self, type_id: &str, rules_image: &str) -> String {
        self.resolve_object_art(type_id, rules_image).image_id
    }

    /// Resolve the declared cameo id for an object.
    ///
    /// This stays in the exact-resolution layer: it only reads declared keys and
    /// falls back to the resolved image id. `ICON` filename guessing lives elsewhere.
    pub fn resolve_declared_cameo_id(&self, type_id: &str, rules_image: &str) -> String {
        let resolved: ResolvedObjectArt<'_> = self.resolve_object_art(type_id, rules_image);
        let type_upper: String = type_id.to_uppercase();

        // Check type-specific section first — e.g. [BFRT] declares its own Cameo
        // even though Image=SREF points to the Prism Tank's art section.
        for key in [type_upper.as_str(), resolved.image_id.as_str()] {
            if let Some(entry) = self.get(key) {
                if let Some(cameo) = normalize_id(entry.cameo.as_deref().unwrap_or_default()) {
                    return cameo;
                }
                if let Some(alt_cameo) =
                    normalize_id(entry.alt_cameo.as_deref().unwrap_or_default())
                {
                    return alt_cameo;
                }
            }
        }

        resolved.image_id
    }

    /// Resolve the declared palette id for an asset, if any.
    pub fn resolve_declared_palette_id(&self, type_id: &str, rules_image: &str) -> Option<String> {
        let resolved: ResolvedObjectArt<'_> = self.resolve_object_art(type_id, rules_image);
        let type_upper: String = type_id.to_uppercase();

        for key in [type_upper.as_str(), resolved.image_id.as_str()] {
            if let Some(entry) = self.get(key) {
                if let Some(ref pal) = entry.palette {
                    return Some(pal.clone());
                }
            }
        }

        None
    }

    /// Resolve the effective image id for an overlay.
    ///
    /// Follows the overlay resolution order: art `[NAME].Image=` first, then
    /// rules `[NAME].Image=`.
    pub fn resolve_overlay_image_id(&self, overlay_name: &str, rules_ini: &IniFile) -> String {
        let upper_name: String = overlay_name.to_uppercase();
        let mut image_id: String = upper_name.clone();

        if let Some(art_image) = self
            .get(&upper_name)
            .and_then(|entry| normalize_id(entry.image.as_deref().unwrap_or_default()))
        {
            image_id = art_image;
        }
        if let Some(rules_image) = rules_ini
            .section(overlay_name)
            .and_then(|section| section.get("Image"))
            .and_then(normalize_id)
        {
            image_id = rules_image;
        }

        image_id
    }

    /// Exact overlay convention flags used by filename generation.
    pub fn overlay_convention_flags(&self, overlay_name: &str, image_id: &str) -> (bool, bool) {
        let name_entry: Option<&ArtEntry> = self.get(overlay_name);
        let image_entry: Option<&ArtEntry> = self.get(image_id);

        let uses_theater: bool = image_entry.map(|e| e.theater).unwrap_or(false)
            || name_entry.map(|e| e.theater).unwrap_or(false);
        let uses_new_theater: bool = image_entry.map(|e| e.new_theater).unwrap_or(false)
            || name_entry.map(|e| e.new_theater).unwrap_or(false)
            || self.should_use_new_theater(image_id);

        (uses_theater, uses_new_theater)
    }

    /// Check whether `NewTheater` substitution should be applied.
    fn should_use_new_theater(&self, upper_image: &str) -> bool {
        if has_hardcoded_new_theater_prefix(upper_image) {
            return true;
        }
        self.get(upper_image)
            .map(|e| e.new_theater)
            .unwrap_or(false)
    }

    /// Iterate all entries with their canonical (uppercase) name keys.
    pub fn iter_entries(&self) -> impl Iterator<Item = (&str, &ArtEntry)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Every normal/damaged/garrisoned name reachable from an instantiated
    /// Building+55C slot. Bind these through the ordinary AnimClass asset owner.
    pub fn building_anim_roots(&self) -> Vec<String> {
        let mut roots = BTreeSet::new();
        for (_, entry) in self.iter_entries() {
            for config in &entry.building_anims {
                for name in [
                    Some(config.anim_type.as_str()),
                    config
                        .damaged_variant
                        .as_ref()
                        .map(|v| v.anim_type.as_str()),
                    config
                        .garrisoned_variant
                        .as_ref()
                        .map(|v| v.anim_type.as_str()),
                ]
                .into_iter()
                .flatten()
                .filter(|name| !name.is_empty())
                {
                    roots.insert(name.to_ascii_uppercase());
                }
            }
        }
        roots.into_iter().collect()
    }

    /// Mutable lookup; case-insensitive on the key.
    pub fn get_mut(&mut self, image_id: &str) -> Option<&mut ArtEntry> {
        self.entries.get_mut(&image_id.to_uppercase())
    }

    /// Eagerly populate `frame_width`/`frame_height` on entries whose anim has
    /// a smudge-spawn flag (Crater/Burn/ForceBigCraters). Reads frame 0 of
    /// each anim's SHP via the shared `anim_shp_candidates` filename
    /// pipeline. Anims without a loadable SHP keep the (30, 30) defaults
    /// from their initial parse.
    ///
    /// Returns `(populated, fallback)` for diagnostic logging:
    ///   `populated` = anims whose SHP was found and dims were stored
    ///   `fallback`  = smudge-flagged anims whose SHP failed to load
    pub fn populate_anim_frame_dims(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> (u32, u32) {
        // Two-pass: collect (name, image_id) under &self, then mutate via
        // get_mut. Direct iter_mut would conflict with the &self call to
        // resolve_effective_image_id.
        let pending: Vec<(String, String)> = self
            .iter_entries()
            .filter(|(_name, entry)| entry.crater || entry.scorch || entry.force_big_craters)
            .map(|(name, _entry)| {
                let image_id: String = self.resolve_effective_image_id(name, name);
                (name.to_string(), image_id)
            })
            .collect();

        let mut populated: u32 = 0;
        let mut fallback: u32 = 0;
        for (name, image_id) in pending {
            let candidates: Vec<String> =
                anim_shp_candidates(Some(self), &name, &image_id, theater_ext, theater_name);
            let shp_bytes: Option<&[u8]> = candidates.iter().find_map(|c| asset_manager.get_ref(c));
            let Some(data) = shp_bytes else {
                fallback += 1;
                continue;
            };
            let Ok(shp) = crate::assets::shp_file::ShpFile::from_bytes(data) else {
                fallback += 1;
                continue;
            };
            let Some(frame) = shp.frames.first() else {
                fallback += 1;
                continue;
            };
            let fw = frame.frame_width;
            let fh = frame.frame_height;
            if let Some(entry) = self.get_mut(&name) {
                entry.frame_width = fw;
                entry.frame_height = fh;
                populated += 1;
            } else {
                fallback += 1;
            }
        }
        (populated, fallback)
    }
}

/// Generate filename candidates for standard SHP objects.
///
/// `repo-derived`: candidate ordering mirrors the original-style behavior already
/// used by the repo. Inputs must already be exact resolved ids.
pub fn object_shp_candidates(
    art: Option<&ArtRegistry>,
    image_id: &str,
    theater_ext: &str,
    theater_name: &str,
) -> Vec<String> {
    let upper: String = image_id.to_uppercase();
    let mut candidates: Vec<String> = Vec::with_capacity(6);
    let use_new_theater: bool = art
        .map(|registry| registry.should_use_new_theater(&upper))
        .unwrap_or_else(|| has_hardcoded_new_theater_prefix(&upper));

    if use_new_theater {
        let subbed: String = apply_theater_letter(&upper, theater_name);
        push_shp_pair(&mut candidates, &subbed, theater_ext);

        let generic: String = apply_generic_letter(&upper);
        if generic != subbed && generic != upper {
            push_shp_pair(&mut candidates, &generic, theater_ext);
        }
    }

    push_shp_pair(&mut candidates, &upper, theater_ext);
    candidates
}

/// Generate filename candidates for building make/build-up art.
pub fn make_shp_candidates(
    art: Option<&ArtRegistry>,
    image_id: &str,
    theater_ext: &str,
    theater_name: &str,
) -> Vec<String> {
    let upper: String = image_id.to_uppercase();
    let mut candidates: Vec<String> = Vec::with_capacity(6);
    let use_new_theater: bool = art
        .map(|registry| registry.should_use_new_theater(&upper))
        .unwrap_or_else(|| has_hardcoded_new_theater_prefix(&upper));

    if use_new_theater {
        let subbed: String = apply_theater_letter(&upper, theater_name);
        push_shp_pair(&mut candidates, &format!("{}MK", subbed), theater_ext);

        let generic: String = apply_generic_letter(&upper);
        if generic != subbed && generic != upper {
            push_shp_pair(&mut candidates, &format!("{}MK", generic), theater_ext);
        }
    }

    push_shp_pair(&mut candidates, &format!("{}MK", upper), theater_ext);
    candidates
}

/// Generate filename candidates for building animation SHPs.
///
/// `repo-derived`: uses the anim section's own `Theater=` / `NewTheater=` flags.
pub fn anim_shp_candidates(
    art: Option<&ArtRegistry>,
    anim_type: &str,
    image_id: &str,
    theater_ext: &str,
    theater_name: &str,
) -> Vec<String> {
    let upper_anim: String = anim_type.to_uppercase();
    let upper_image: String = image_id.to_uppercase();
    let entry: Option<&ArtEntry> = art.and_then(|registry| registry.get(&upper_anim));
    let uses_new_theater: bool = entry.map(|e| e.new_theater).unwrap_or(false);
    let uses_theater: bool = entry.map(|e| e.theater).unwrap_or(false);
    let mut candidates: Vec<String> = Vec::with_capacity(6);

    if uses_new_theater {
        let subbed: String = apply_theater_letter(&upper_image, theater_name);
        push_shp_pair(&mut candidates, &subbed, theater_ext);

        let generic: String = apply_generic_letter(&upper_image);
        if generic != subbed && generic != upper_image {
            push_candidate(&mut candidates, format!("{}.SHP", generic));
        }
    }

    if uses_theater {
        push_candidate(
            &mut candidates,
            format!("{}.{}", upper_image, theater_ext.to_ascii_uppercase()),
        );
    }
    push_candidate(&mut candidates, format!("{}.SHP", upper_image));
    if !uses_theater {
        push_candidate(
            &mut candidates,
            format!("{}.{}", upper_image, theater_ext.to_ascii_uppercase()),
        );
    }

    candidates
}

/// Generate filename candidates for overlay SHPs.
///
/// This function only applies conventions. Callers should resolve `image_id`
/// through `ArtRegistry::resolve_overlay_image_id()` first.
pub fn overlay_shp_candidates(
    art: Option<&ArtRegistry>,
    overlay_name: &str,
    image_id: &str,
    theater_ext: &str,
    theater_name: &str,
) -> Vec<String> {
    let upper_name: String = overlay_name.to_uppercase();
    let upper_image: String = image_id.to_uppercase();
    let (uses_theater, uses_new_theater): (bool, bool) = art
        .map(|registry| registry.overlay_convention_flags(&upper_name, &upper_image))
        .unwrap_or((false, has_hardcoded_new_theater_prefix(&upper_image)));
    let mut candidates: Vec<String> = Vec::with_capacity(6);

    if uses_new_theater {
        let subbed: String = apply_theater_letter(&upper_image, theater_name);
        push_candidate(
            &mut candidates,
            format!("{}.{}", subbed, theater_ext.to_ascii_uppercase()),
        );
        push_candidate(&mut candidates, format!("{}.SHP", subbed));

        let generic: String = apply_generic_letter(&upper_image);
        if generic != subbed && generic != upper_image {
            push_candidate(
                &mut candidates,
                format!("{}.{}", generic, theater_ext.to_ascii_uppercase()),
            );
            push_candidate(&mut candidates, format!("{}.SHP", generic));
        }
    }

    if uses_theater {
        push_candidate(
            &mut candidates,
            format!("{}.{}", upper_image, theater_ext.to_ascii_uppercase()),
        );
        push_candidate(&mut candidates, format!("{}.SHP", upper_image));
    } else {
        push_candidate(&mut candidates, format!("{}.SHP", upper_image));
        push_candidate(
            &mut candidates,
            format!("{}.{}", upper_image, theater_ext.to_ascii_uppercase()),
        );
    }

    candidates
}

/// Build the one primary filename and one generic-letter retry used by
/// `OverlayTypeClass::GetSHP` for map-pack placement eligibility.
///
/// Keep this separate from [`overlay_shp_candidates`]: render loading probes a
/// deliberately broader compatibility list, while the native object-type
/// loader constructs exactly one name and only retries after forcing its
/// second character to `G`.
pub(crate) fn native_overlay_shp_names(
    art: &ArtRegistry,
    image_id: &str,
    theater_ext: &str,
    theater_name: &str,
) -> [String; 2] {
    let upper_image = image_id.to_ascii_uppercase();
    // ObjectTypeClass has already resolved `Image=` at this point; its two
    // filename flags come from that resolved art section, not from the broad
    // name/image convention union used by render probing.
    let image_entry = art.get(&upper_image);
    let uses_theater = image_entry.is_some_and(|entry| entry.theater);
    let uses_new_theater = image_entry.is_some_and(|entry| entry.new_theater);

    let (primary_stem, extension) = if uses_theater {
        (upper_image, theater_ext.to_ascii_uppercase())
    } else {
        let stem = if uses_new_theater {
            apply_theater_letter(&upper_image, theater_name)
        } else {
            upper_image
        };
        (stem, "SHP".to_string())
    };
    let primary = format!("{primary_stem}.{extension}");
    let generic_retry = apply_generic_letter(&primary);
    [primary, generic_retry]
}

/// Generate VXL/HVA filenames for a voxel model.
pub fn voxel_asset_names(image_id: &str) -> (String, String) {
    let upper: String = image_id.to_uppercase();
    (format!("{}.VXL", upper), format!("{}.HVA", upper))
}

/// Building animation key names and their suffixes.
const BUILDING_ANIM_KEYS: &[(&str, &[&str], u8)] = &[
    ("PowerUp1Anim", &[""], 0),
    ("PowerUp2Anim", &[""], 1),
    ("PowerUp3Anim", &[""], 2),
    ("ActiveAnim", &["", "Two", "Three", "Four"], 3),
    ("PreProductionAnim", &[""], 7),
    ("ProductionAnim", &[""], 8),
    ("TurretAnim", &[""], 9),
    ("SpecialAnim", &["", "Two", "Three", "Four"], 10),
    ("SuperAnim", &["", "Two", "Three", "Four"], 14),
    ("IdleAnim", &[""], 18),
    ("LowPower", &[""], 19),
    ("SuperLowPower", &[""], 20),
];

fn parse_building_anims(section: &IniSection, ini: &IniFile) -> Vec<BuildingAnimConfig> {
    let mut anims: Vec<BuildingAnimConfig> = Vec::new();

    for &(base, suffixes, first_slot) in BUILDING_ANIM_KEYS {
        let kind: BuildingAnimKind = match base {
            "ActiveAnim" => BuildingAnimKind::Active,
            "IdleAnim" => BuildingAnimKind::Idle,
            "SuperAnim" => BuildingAnimKind::Super,
            "SpecialAnim" => BuildingAnimKind::Special,
            "ProductionAnim" => BuildingAnimKind::Production,
            _ => BuildingAnimKind::Special,
        };
        for (ordinal, &suffix) in suffixes.iter().enumerate() {
            let key: String = format!("{}{}", base, suffix);
            let damaged_key = if first_slot < 3 {
                format!("PowerUp{}DamagedAnim", first_slot + 1)
            } else {
                format!("{key}Damaged")
            };
            let anim_type = section.get(&key).unwrap_or("").to_string();
            let damaged = section.get(&damaged_key).filter(|v| !v.is_empty());
            let garrisoned = section
                .get(&format!("{key}Garrisoned"))
                .filter(|v| !v.is_empty());
            if anim_type.is_empty() && damaged.is_none() && garrisoned.is_none() {
                continue;
            }

            let x_key = if first_slot < 3 {
                format!("PowerUp{}LocXX", first_slot + 1)
            } else {
                format!("{key}X")
            };
            let y_key = if first_slot < 3 {
                format!("PowerUp{}LocYY", first_slot + 1)
            } else {
                format!("{key}Y")
            };
            let z_key = if first_slot < 3 {
                format!("PowerUp{}LocZZ", first_slot + 1)
            } else {
                format!("{key}ZAdjust")
            };
            let sort_key = if first_slot < 3 {
                format!("PowerUp{}YSort", first_slot + 1)
            } else {
                format!("{key}YSort")
            };
            let x: i32 = section.get_i32(&x_key).unwrap_or(0);
            let y: i32 = section.get_i32(&y_key).unwrap_or(0);
            let y_sort: i32 = section.get_i32(&sort_key).unwrap_or(0);
            let z_adjust: i32 = section.get_i32(&z_key).unwrap_or(0);

            let base_variant = parse_building_anim_variant(anim_type.clone(), ini);

            anims.push(BuildingAnimConfig {
                native_slot: first_slot + ordinal as u8,
                anim_type: base_variant.anim_type,
                damaged_variant: damaged.map(|v| parse_building_anim_variant(v.to_string(), ini)),
                garrisoned_variant: garrisoned
                    .map(|v| parse_building_anim_variant(v.to_string(), ini)),
                kind,
                is_primary: suffix.is_empty(),
                x,
                y,
                y_sort,
                z_adjust,
                loop_start: base_variant.loop_start,
                loop_end: base_variant.loop_end,
                loop_count: base_variant.loop_count,
                rate: base_variant.rate,
                start_frame: base_variant.start_frame,
                ping_pong: base_variant.ping_pong,
            });
        }
    }

    anims
}

fn parse_building_anim_variant(anim_type: String, ini: &IniFile) -> BuildingAnimVariantConfig {
    let anim_section = ini.section(&anim_type);
    BuildingAnimVariantConfig {
        anim_type,
        loop_start: anim_section
            .and_then(|s| s.get_i32("LoopStart"))
            .unwrap_or(0) as u16,
        loop_end: anim_section.and_then(|s| s.get_i32("LoopEnd")).unwrap_or(0) as u16,
        loop_count: anim_section
            .and_then(|s| s.get_i32("LoopCount"))
            .unwrap_or(0),
        rate: anim_section
            .and_then(|s| s.get_i32("Rate"))
            .map(|r| art_rate_to_delay_ms(r) as u16)
            .unwrap_or(DEFAULT_ART_RATE_MS),
        start_frame: anim_section.and_then(|s| s.get_i32("Start")).unwrap_or(0) as u16,
        ping_pong: anim_section
            .and_then(|s| s.get_bool("PingPong"))
            .unwrap_or(false),
    }
}

fn parse_numbered_cell_offsets(
    section: &IniSection,
    prefix: &str,
) -> [Option<(i16, i16)>; HIDDEN_OCCUPY_SLOT_COUNT] {
    let mut offsets = [None; HIDDEN_OCCUPY_SLOT_COUNT];
    for i in 1..=8 {
        let key = format!("{}{}", prefix, i);
        let Some(val) = section.get(&key) else {
            continue;
        };
        let mut parts = val.split(',');
        if let (Some(x), Some(y)) = (
            parts.next().and_then(|s| s.trim().parse::<i16>().ok()),
            parts.next().and_then(|s| s.trim().parse::<i16>().ok()),
        ) {
            offsets[i - 1] = Some((x, y));
        }
    }
    offsets
}

/// BuildingType art reader4615CA..4617B8; original executable comparison in
/// tools/spatial_oracle/building_body_rules.{py,json,meta.json}.
fn read_building_body_range(section: &IniSection, key: &str) -> [i32; 3] {
    let mut range = [0, 1, 0];
    if let Some(values) = section.projected_values(key) {
        for value in values {
            read_building_body_range_value(value, &mut range);
        }
    } else if let Some(value) = section.get(key) {
        read_building_body_range_value(value, &mut range);
    }
    range
}

fn read_building_body_range_value(value: &str, range: &mut [i32; 3]) {
    // ReadString copies at most63 bytes then trims; sscanf decimal conversions
    // skip C whitespace, but the literal commas cannot skip whitespace.
    let value = crate::rules::ini_value::truncate_bytes(value, 63);
    let mut bytes = crate::rules::ini_value::strtrim_ascii(value).as_bytes();
    for slot in range {
        let Some(value) = crate::rules::ini_value::scan_decimal_i32(&mut bytes) else {
            return;
        };
        *slot = value;
        if bytes.first() != Some(&b',') {
            return;
        }
        bytes = &bytes[1..];
    }
}

fn parse_i32_pair(value: &str) -> Option<(i32, i32)> {
    let mut parts = value.split(',');
    let x = parts.next()?.trim().parse::<i32>().ok()?;
    let y = parts.next()?.trim().parse::<i32>().ok()?;
    Some((x, y))
}

/// Replace the 2nd character of a filename with the theater-specific letter.
fn apply_theater_letter(name: &str, theater_name: &str) -> String {
    if name.len() < 2 {
        return name.to_string();
    }

    let upper_theater: String = theater_name.to_ascii_uppercase();
    let letter: char = match THEATER_LETTERS.iter().find(|(t, _)| *t == upper_theater) {
        Some((_, ch)) => *ch,
        None => return name.to_string(),
    };
    let mut chars: Vec<char> = name.chars().collect();
    chars[1] = letter;
    chars.into_iter().collect()
}

/// Replace the 2nd character of a filename with the generic letter `G`.
fn apply_generic_letter(name: &str) -> String {
    if name.len() < 2 {
        return name.to_string();
    }

    let mut chars: Vec<char> = name.chars().collect();
    chars[1] = NEW_THEATER_GENERIC_LETTER;
    chars.into_iter().collect()
}

fn normalize_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_uppercase())
    }
}

fn has_hardcoded_new_theater_prefix(upper_image: &str) -> bool {
    upper_image.len() >= 2 && NEW_THEATER_PREFIXES.iter().any(|&p| p == &upper_image[..2])
}

fn push_shp_pair(candidates: &mut Vec<String>, base_name: &str, theater_ext: &str) {
    push_candidate(candidates, format!("{}.SHP", base_name));
    push_candidate(
        candidates,
        format!("{}.{}", base_name, theater_ext.to_ascii_uppercase()),
    );
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

#[cfg(test)]
mod anim_runtime_metadata_tests {
    use super::*;
    use crate::assets::asset_manager::AssetManager;

    #[test]
    fn building_body_fields_match_original_native_readers() {
        #[derive(serde::Deserialize)]
        struct Triple {
            raw: Option<String>,
            initial: [i32; 3],
            output: [i32; 3],
        }
        #[derive(serde::Deserialize)]
        struct Scalar {
            raw: Option<String>,
            output: i32,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            triples: Vec<Triple>,
            gate_stages: Vec<Scalar>,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_body_rules.json"
        ))
        .unwrap();
        let keys = ["AnimIdle", "AnimActive", "AnimAux1", "AnimAux2"];
        for row in fixture.triples {
            let mut ini = IniFile::from_str("[TEST]\nFixtureOnly=1\n");
            let section = ini.projection_section_mut("TEST");
            let mut previous = IniSection::new("TEST".to_string());
            for key in keys {
                previous.set(
                    key,
                    &format!("{},{},{}", row.initial[0], row.initial[1], row.initial[2]),
                );
            }
            section.overlay_rules_pass(&previous);
            if let Some(raw) = &row.raw {
                let mut patch = IniSection::new("TEST".to_string());
                for key in keys {
                    patch.set(key, raw);
                }
                section.overlay_rules_pass(&patch);
            }
            let registry = ArtRegistry::from_ini(&ini);
            assert_eq!(
                registry.get("TEST").unwrap().building_body_ranges,
                [row.output; 4],
                "raw={:?} previous={:?}",
                row.raw,
                row.initial
            );
        }
        for row in fixture.gate_stages {
            let mut ini = IniFile::from_str("[TEST]\nFixtureOnly=1\n");
            if let Some(raw) = &row.raw {
                ini.projection_section_mut("TEST").set("GateStages", raw);
            }
            let registry = ArtRegistry::from_ini(&ini);
            assert_eq!(
                registry.get("TEST").unwrap().building_gate_stages,
                row.output,
                "{:?}",
                row.raw
            );
        }
        let ini = IniFile::from_str("[TEST]\nFixtureOnly=1\n");
        let registry = ArtRegistry::from_ini(&ini);
        assert_eq!(
            registry.get("TEST").unwrap().building_body_ranges,
            [[0, 1, 0]; 4]
        );
    }

    #[test]
    fn normalizes_anim_spawn_metadata_refs_to_uppercase() {
        let ini = IniFile::from_str(
            "[DBRIS1LG]\n\
             TrailerAnim=smokey2\n\
             BounceAnim=twlt026\n\
             ExpireAnim=twlt036\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let config = reg
            .anim_runtime_config("DBRIS1LG")
            .expect("DBRIS1LG runtime metadata");
        assert_eq!(config.trailer_anim.as_deref(), Some("SMOKEY2"));
        assert_eq!(config.bounce_anim.as_deref(), Some("TWLT026"));
        assert_eq!(config.expire_anim.as_deref(), Some("TWLT036"));
    }

    #[test]
    fn parses_signed_trailer_seperation_without_clamping() {
        let ini = IniFile::from_str(
            "[DBRIS1LG]\n\
             TrailerAnim=SMOKEY2\n\
             TrailerSeperation=-2\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let config = reg
            .anim_runtime_config("DBRIS1LG")
            .expect("DBRIS1LG runtime metadata");
        assert_eq!(config.trailer_seperation, -2);
    }

    #[test]
    fn anim_spawn_metadata_defaults_to_native_nulls_and_zero_seperation() {
        let ini = IniFile::from_str("[SMOKEY2]\nRate=600\n");
        let reg = ArtRegistry::from_ini(&ini);

        let config = reg
            .anim_runtime_config("SMOKEY2")
            .expect("SMOKEY2 runtime metadata");
        assert_eq!(config.trailer_anim, None);
        assert_eq!(config.trailer_seperation, 0);
        assert_eq!(config.bounce_anim, None);
        assert_eq!(config.expire_anim, None);
    }

    #[test]
    fn anim_spawn_metadata_preserves_existing_next_random_and_rate_fields() {
        let ini = IniFile::from_str(
            "[METSTRAL]\n\
             Rate=300\n\
             Normalized=yes\n\
             Next=smokey\n\
             RandomLoopDelay=2,5\n\
             RandomRate=300,900\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let config = reg
            .anim_runtime_config("METSTRAL")
            .expect("METSTRAL runtime metadata");
        assert_eq!(config.rate_logic_frames, 3);
        assert!(config.normalized);
        assert_eq!(config.next.as_deref(), Some("SMOKEY"));
        assert_eq!(config.random_loop_delay, Some((2, 5)));
        assert_eq!(config.random_rate_logic_frames, Some((1, 1)));
        assert_eq!(config.trailer_anim, None);
        assert_eq!(config.trailer_seperation, 0);
        assert_eq!(config.bounce_anim, None);
        assert_eq!(config.expire_anim, None);
    }

    #[test]
    fn reversed_random_rate_conversion_collapses_to_the_second_endpoint() {
        let ini = IniFile::from_str("[DBRIS1LG]\nRandomRate=220,600\n");
        let reg = ArtRegistry::from_ini(&ini);

        assert_eq!(
            reg.anim_runtime_config("DBRIS1LG")
                .unwrap()
                .random_rate_logic_frames,
            Some((1, 1))
        );
    }

    #[test]
    fn normalized_defaults_false_and_parses_explicit_values() {
        let ini = IniFile::from_str(
            "[DEFAULT]\nRate=900\n\
             [YES]\nNormalized=yes\n\
             [NO]\nNormalized=no\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        assert!(!reg.anim_runtime_config("DEFAULT").unwrap().normalized);
        assert!(reg.anim_runtime_config("YES").unwrap().normalized);
        assert!(!reg.anim_runtime_config("NO").unwrap().normalized);
    }

    #[test]
    fn use_normal_light_defaults_false_and_parses_the_stock_explosion_families() {
        // The first six sections carry their stock artmd.ini names and the stock
        // spelling of the keys shown. Every one of the 43 stock hits is a bare
        // `UseNormalLight=yes`; no stock section writes `=no`, so DEFAULTS_NO is
        // synthetic and exists only to prove an explicit negative parses.
        //
        // TUNTOP01 is one of the twenty `Tile##Anim=` theater-tileset animations
        // (four tunnel tops and sixteen waterfall frames across the six theater
        // INIs). Those are the animations gamemd attaches to a cell, and the
        // attached branch keeps the cell's convert even when the flag is set —
        // so a cell-attached type that ever set the key would want the cell's
        // hue at full brightness rather than a fully neutral tint. None of the
        // twenty sets it, which is what makes the neutral return safe today.
        let ini = IniFile::from_str(
            "[TWLT070]\nUseNormalLight=yes\nNormalized=yes\n\
             [S_BANG24]\nUseNormalLight=yes\nNormalized=yes\n\
             [BURN-M]\nUseNormalLight=yes\nLayer=ground\n\
             [EXPLOLRG]\nUseNormalLight=yes\nTranslucent=yes\n\
             [TUNTOP01]\nTheater=yes\nNormalized=yes\n\
             [GCMUZZLE]\nNormalized=yes\n\
             [DEFAULTS_NO]\nUseNormalLight=no\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        for lit_at_full_brightness in ["TWLT070", "S_BANG24", "BURN-M", "EXPLOLRG"] {
            assert!(
                reg.anim_runtime_config(lit_at_full_brightness)
                    .unwrap()
                    .use_normal_light,
                "{lit_at_full_brightness} must draw at full brightness"
            );
        }
        for cell_lit in ["TUNTOP01", "GCMUZZLE", "DEFAULTS_NO"] {
            assert!(
                !reg.anim_runtime_config(cell_lit).unwrap().use_normal_light,
                "{cell_lit} must stay on the cell-lit path"
            );
        }
    }

    #[test]
    fn gsi_02_13_parses_fixed_translucency_metadata_and_omitted_zero_defaults() {
        let ini = IniFile::from_str(
            "[TWENTY_FIVE]\n\
             Translucency=25\n\
             TranslucencyDetailLevel=-3\n\
             [FIFTY]\n\
             Translucency=50\n\
             [SEVENTY_FIVE]\n\
             Translucency=75\n\
             [OMITTED]\n\
             FixtureOnly=1\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let twenty_five = reg.anim_runtime_config("TWENTY_FIVE").unwrap();
        assert_eq!(twenty_five.translucency, 25);
        assert_eq!(twenty_five.translucency_detail_level, -3);
        assert!(!twenty_five.translucent);
        assert_eq!(reg.anim_runtime_config("FIFTY").unwrap().translucency, 50);
        assert_eq!(
            reg.anim_runtime_config("SEVENTY_FIVE")
                .unwrap()
                .translucency,
            75
        );

        let omitted = reg.anim_runtime_config("OMITTED").unwrap();
        assert_eq!(omitted.translucency, 0);
        assert_eq!(omitted.translucency_detail_level, 0);
    }

    #[test]
    fn alt_palette_is_parsed_for_every_anim_type_not_just_the_parachute() {
        // Stock sections that carry the flag; the palette cascade must be able to
        // ask any AnimType, not only the one hardcoded section.
        let ini = IniFile::from_str(
            "[NABNKR]\n\
             AltPalette=yes\n\
             [WCCLOUD1]\n\
             AltPalette=true\n\
             [PARACH]\n\
             AltPalette=yes\n\
             [FBALL1]\n\
             Layer=ground\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        assert!(reg.anim_runtime_config("NABNKR").unwrap().alt_palette);
        assert!(reg.anim_runtime_config("WCCLOUD1").unwrap().alt_palette);
        assert!(reg.anim_runtime_config("PARACH").unwrap().alt_palette);
        assert!(!reg.anim_runtime_config("FBALL1").unwrap().alt_palette);
    }

    #[test]
    fn alt_palette_selects_the_unit_palette_and_everything_else_the_anim_palette() {
        let ini = IniFile::from_str(
            "[NABNKR]\n\
             AltPalette=yes\n\
             [FBALL1]\n\
             Layer=ground\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        assert_eq!(
            anim_draw_palette(reg.anim_runtime_config("NABNKR").unwrap()),
            AnimDrawPalette::Unit
        );
        assert_eq!(
            anim_draw_palette(reg.anim_runtime_config("FBALL1").unwrap()),
            AnimDrawPalette::Anim
        );
    }

    #[test]
    fn gsi_02_13_maps_only_exact_fixed_translucency_values_to_draw_bits() {
        assert_eq!(anim_fixed_translucency_draw_bits(25), Some(0x2));
        assert_eq!(anim_fixed_translucency_draw_bits(50), Some(0x4));
        assert_eq!(anim_fixed_translucency_draw_bits(75), Some(0x6));
        assert_eq!(anim_fixed_translucency_draw_bits(1), None);
        assert_eq!(anim_fixed_translucency_draw_bits(26), None);
        assert_eq!(anim_fixed_translucency_draw_bits(100), None);
        assert_eq!(anim_fixed_translucency_draw_bits(0), None);
        assert_eq!(anim_fixed_translucency_draw_bits(-25), None);
    }

    #[test]
    fn translucency_draw_bits_invert_the_ini_percentage_into_source_weight() {
        // Translucency=N is N percent TRANSPARENT: 25 is the most opaque stage.
        assert_eq!(
            anim_translucency_source_alpha(ANIM_DRAW_BITS_TRANSLUCENT_25),
            0.75
        );
        assert_eq!(
            anim_translucency_source_alpha(ANIM_DRAW_BITS_TRANSLUCENT_50),
            0.5
        );
        assert_eq!(
            anim_translucency_source_alpha(ANIM_DRAW_BITS_TRANSLUCENT_75),
            0.25
        );
        assert_eq!(anim_translucency_source_alpha(ANIM_DRAW_BITS_OPAQUE), 1.0);
        // Bits outside the family mask are not a translucency stage.
        assert_eq!(anim_translucency_source_alpha(0x800), 1.0);

        // `AnimClass::DrawIt` ORs the family into the producer's own flag word
        // (`0x0042304B` loads `Anim+0x190`, `0x00423075`/`0x0042307A` OR into
        // it), so the carried producer bits must not hide the stage. `0x600` is
        // the trailer/damage-fire word and `0x2600` the combat-explosion word.
        for carried in [0x0, 0x600, 0x2600] {
            assert_eq!(
                anim_translucency_source_alpha(carried | ANIM_DRAW_BITS_TRANSLUCENT_25),
                0.75
            );
            assert_eq!(
                anim_translucency_source_alpha(carried | ANIM_DRAW_BITS_TRANSLUCENT_50),
                0.5
            );
            assert_eq!(
                anim_translucency_source_alpha(carried | ANIM_DRAW_BITS_TRANSLUCENT_75),
                0.25
            );
            assert_eq!(anim_translucency_source_alpha(carried), 1.0);
        }
    }

    #[test]
    fn stock_fixed_translucency_sections_resolve_to_their_blend_and_others_stay_opaque() {
        // BURN-S/M/L are the burning-building fires: Translucency=25, no
        // Translucent=, so every frame draws at three-quarter source weight.
        let ini = IniFile::from_str(
            "[BURN-S]\n\
             Layer=ground\n\
             Translucency=25\n\
             [FIRE3]\n\
             Translucency=50\n\
             [PLAIN]\n\
             Layer=ground\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let burn = reg.anim_runtime_config("BURN-S").unwrap();
        for frame in [0, 1, 30, 61] {
            assert_eq!(anim_frame_source_alpha(burn, frame, 62), 0.75);
        }

        let fire = reg.anim_runtime_config("FIRE3").unwrap();
        assert_eq!(anim_frame_source_alpha(fire, 3, 8), 0.5);

        // An animation carrying neither key must be untouched by this path.
        let plain = reg.anim_runtime_config("PLAIN").unwrap();
        assert_eq!(anim_frame_source_alpha(plain, 3, 8), 1.0);
    }

    #[test]
    fn translucent_yes_fades_progressively_instead_of_holding_one_blend() {
        let ini = IniFile::from_str("[WAKE1]\nFlat=yes\nTranslucent=yes\n");
        let mut reg = ArtRegistry::from_ini(&ini);
        // End comes from the SHP header once the type is bound.
        reg.bind_anim_frame_count_for_test("WAKE1", 10);
        let wake = reg.anim_runtime_config("WAKE1").unwrap();

        // (current frame, expected source weight) against End = 10.
        let ladder = [
            (0, 1.0),
            (1, 1.0),
            (2, 1.0),
            (3, 0.75),
            (4, 0.75),
            (5, 0.5),
            (6, 0.5),
            (7, 0.25),
            (10, 0.25),
        ];
        for (frame, expected) in ladder {
            assert_eq!(
                anim_frame_source_alpha(wake, frame, 10),
                expected,
                "frame {frame} of a Translucent=yes animation"
            );
        }
    }

    #[test]
    fn translucent_yes_takes_precedence_over_a_numeric_translucency_on_the_same_section() {
        // Stock SMKPUFF and NUKETO carry both keys; the boolean wins, so their
        // numeric value never selects a blend and frame 0 is opaque.
        let ini = IniFile::from_str("[SMKPUFF]\nTranslucent=yes\nTranslucency=50\n");
        let mut reg = ArtRegistry::from_ini(&ini);
        reg.bind_anim_frame_count_for_test("SMKPUFF", 10);
        let smoke = reg.anim_runtime_config("SMKPUFF").unwrap();

        assert_eq!(anim_frame_source_alpha(smoke, 0, 10), 1.0);
        assert_eq!(anim_frame_source_alpha(smoke, 9, 10), 0.25);
    }

    #[test]
    fn progressive_end_uses_the_shp_frame_count_until_the_type_is_bound() {
        let ini = IniFile::from_str("[WAKE2]\nTranslucent=yes\n");
        let reg = ArtRegistry::from_ini(&ini);
        let wake = reg.anim_runtime_config("WAKE2").unwrap();

        // Unbound: End is derived from the caller's SHP frame count, so the
        // ladder still runs instead of collapsing onto End = 0.
        assert_eq!(anim_frame_source_alpha(wake, 1, 10), 1.0);
        assert_eq!(anim_frame_source_alpha(wake, 5, 10), 0.5);
    }

    #[test]
    fn progressive_end_honours_explicit_end_and_the_shadow_half_split() {
        let ini = IniFile::from_str(
            "[SHADOWED]\n\
             Translucent=yes\n\
             Shadow=yes\n\
             [CAPPED]\n\
             Translucent=yes\n\
             End=10\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        // Shadow=yes puts the shadow frames in the back half of the SHP, so the
        // drawn animation ends at half the header count.
        let shadowed = reg.anim_runtime_config("SHADOWED").unwrap();
        assert_eq!(anim_frame_source_alpha(shadowed, 5, 20), 0.5);
        assert_eq!(anim_frame_source_alpha(shadowed, 2, 20), 1.0);

        // An explicit End= overrides the header count either way.
        let capped = reg.anim_runtime_config("CAPPED").unwrap();
        assert_eq!(anim_frame_source_alpha(capped, 5, 100), 0.5);
    }

    #[test]
    fn anim_runtime_metadata_parses_tiberium_declaration_cluster() {
        let ini = IniFile::from_str(
            "[TIBERIUM]\n\
             TiberiumChainReaction=yes\n\
             IsTiberium=true\n\
             HideIfNoOre=1\n\
             IsAnimatedTiberium=Yes\n\
             TiberiumSpreadRadius=-7\n\
             TiberiumSpawnType=  tiB2_01  \n\
             [UNRELATED]\n\
             Rate=900\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let tiberium = reg.anim_runtime_config("TIBERIUM").unwrap();
        assert!(tiberium.tiberium_chain_reaction);
        assert!(tiberium.is_tiberium);
        assert!(tiberium.hide_if_no_ore);
        assert!(tiberium.is_animated_tiberium);
        assert_eq!(tiberium.tiberium_spread_radius, -7);
        assert_eq!(tiberium.tiberium_spawn_type.as_deref(), Some("TIB2_01"));

        let unrelated = reg.anim_runtime_config("UNRELATED").unwrap();
        assert!(!unrelated.tiberium_chain_reaction);
        assert!(!unrelated.is_tiberium);
        assert!(!unrelated.hide_if_no_ore);
        assert!(!unrelated.is_animated_tiberium);
        assert_eq!(unrelated.tiberium_spread_radius, 0);
        assert_eq!(unrelated.tiberium_spawn_type, None);
    }

    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_artmd_tiberium_declarations_match_active_yr() {
        let ra2_dir = std::path::PathBuf::from(
            std::env::var_os("RA2_DIR")
                .expect("set RA2_DIR to the installed retail RA2/YR directory"),
        );
        let assets = AssetManager::new(&ra2_dir).expect("load retail RA2/YR archive stack");
        let artmd_bytes = assets.get_ref("ARTMD.INI").expect("load retail ARTMD.INI");
        let artmd = IniFile::from_bytes(artmd_bytes).expect("parse retail ARTMD.INI");
        let registry = ArtRegistry::from_ini(&artmd);

        assert!(
            registry
                .anim_runtime_config("TWNK1")
                .expect("stock TWNK1 declaration")
                .hide_if_no_ore
        );

        let crystal1 = registry
            .anim_runtime_config("CRYSTAL1")
            .expect("stock CRYSTAL1 declaration");
        assert!(crystal1.is_tiberium);
        assert_eq!(crystal1.tiberium_spread_radius, 0);
        assert_eq!(crystal1.tiberium_spawn_type.as_deref(), Some("TIB2_01"));

        assert!(
            registry
                .anim_runtime_config("TWLT070T")
                .expect("stock TWLT070T declaration")
                .tiberium_chain_reaction
        );
        assert!(
            registry
                .anim_runtime_config("BIGBLUE")
                .expect("stock BIGBLUE declaration")
                .is_animated_tiberium
        );
    }

    /// The retail values behind the four `ReadDouble` keys and the
    /// `AltPalette=`/`UseNormalLight=` counts this module documents.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_artmd_anim_damage_cluster_matches_active_yr() {
        let ra2_dir = std::path::PathBuf::from(
            std::env::var_os("RA2_DIR")
                .expect("set RA2_DIR to the installed retail RA2/YR directory"),
        );
        let assets = AssetManager::new(&ra2_dir).expect("load retail RA2/YR archive stack");
        let artmd_bytes = assets.get_ref("ARTMD.INI").expect("load retail ARTMD.INI");
        let artmd = IniFile::from_bytes(artmd_bytes).expect("parse retail ARTMD.INI");
        let registry = ArtRegistry::from_ini(&artmd);

        let debris = registry
            .anim_runtime_config("DBRIS1LG")
            .expect("stock DBRIS1LG declaration");
        assert_eq!(f64::from_bits(debris.damage.bits()), 20.0);
        assert_eq!(debris.warhead.as_deref(), Some("HE"));
        assert_eq!(debris.damage_radius, 80);
        assert_eq!(f64::from_bits(debris.elasticity.bits()), 0.0);
        assert_eq!(f64::from_bits(debris.min_z_vel.bits()), 25.0);
        assert_eq!(f64::from_bits(debris.max_xy_vel.bits()), 25.0);
        assert!(debris.bouncer);

        // The one live per-frame-damage producer in stock skirmish: the fire a
        // destroyed building leaves behind.
        let fire3 = registry
            .anim_runtime_config("FIRE3")
            .expect("stock FIRE3 declaration");
        let fire3_damage = f64::from_bits(fire3.damage.bits());
        assert!(
            fire3_damage > 0.0 && fire3_damage < 0.01,
            "[FIRE3] Damage= is `.003`, read back as {fire3_damage}"
        );

        // A section with neither key keeps the constructor's non-zero doubles.
        let twlt036 = registry
            .anim_runtime_config("TWLT036")
            .expect("stock TWLT036 declaration");
        assert_eq!(f64::from_bits(twlt036.damage.bits()), 0.0);
        assert_eq!(twlt036.elasticity.bits(), 0x3fe9_9999_a000_0000);
        assert_eq!(f64::from_bits(twlt036.min_z_vel.bits()), 3.5);
        assert_eq!(f64::from_bits(twlt036.max_xy_vel.bits()), 15.0);

        // The counts this module's `alt_palette` / `use_normal_light` docs
        // claim, derived rather than asserted from memory. gamemd's INI key
        // lookup is case-sensitive, so only the exact spellings count.
        let alt_palette = registry
            .anim_runtime_configs
            .values()
            .filter(|config| config.alt_palette)
            .count();
        let use_normal_light = registry
            .anim_runtime_configs
            .values()
            .filter(|config| config.use_normal_light)
            .count();
        assert_eq!(alt_palette, 31, "stock artmd.ini AltPalette= sections");
        assert_eq!(
            use_normal_light, 43,
            "stock artmd.ini UseNormalLight= sections"
        );
    }

    /// `AnimTypeClass::ReadINI @ 0x00427D00` reads `Damage=`, `Elasticity=`,
    /// `MinZVel=` and `MaxXYVel=` through `CCINIClass::ReadDouble`, so each of
    /// them keeps a fraction. `Damage` in particular is a double at `+0x2A8`,
    /// never an integer: the stock population is `.003`-scale.
    #[test]
    fn anim_double_keys_keep_their_fraction() {
        let ini = IniFile::from_str(
            "[DEBRIS]\n\
             Damage=20\n\
             Warhead=he\n\
             DamageRadius=80\n\
             Elasticity=0.0\n\
             MinZVel=25.0\n\
             MaxXYVel=25.0\n\
             [BURNER]\n\
             Damage=.003\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        let debris = reg.anim_runtime_config("DEBRIS").unwrap();
        assert_eq!(f64::from_bits(debris.damage.bits()), 20.0);
        assert_eq!(debris.warhead.as_deref(), Some("HE"));
        assert_eq!(debris.damage_radius, 80);
        assert_eq!(f64::from_bits(debris.elasticity.bits()), 0.0);
        assert_eq!(f64::from_bits(debris.min_z_vel.bits()), 25.0);
        assert_eq!(f64::from_bits(debris.max_xy_vel.bits()), 25.0);

        // The fraction survives; an i32 read would have produced 0.
        let burner = reg.anim_runtime_config("BURNER").unwrap();
        let damage = f64::from_bits(burner.damage.bits());
        assert!(
            damage > 0.0 && damage < 0.01,
            "{damage} is not `.003`-scale"
        );
    }

    /// Constructor defaults from `AnimTypeClass::AnimTypeClass @ 0x00427530`.
    /// Three of them are NOT the zero/false a naive port would pick:
    /// `Elasticity` 0.8, `MinZVel` 3.5, `MaxXYVel` 15.0, and both
    /// `ShouldUseCellDrawer` and `ShouldFogRemove` start true.
    #[test]
    fn anim_constructor_defaults_are_not_all_zero() {
        let ini = IniFile::from_str("[BARE]\nFixtureOnly=1\n");
        let reg = ArtRegistry::from_ini(&ini);
        let bare = reg.anim_runtime_config("BARE").unwrap();

        assert_eq!(f64::from_bits(bare.damage.bits()), 0.0);
        // The literal dword pair the constructor stores is the SINGLE-precision
        // 0.8f widened, not the nearest double to 0.8.
        assert_eq!(bare.elasticity.bits(), 0x3fe9_9999_a000_0000);
        assert_eq!(f64::from_bits(bare.elasticity.bits()), f64::from(0.8_f32));
        assert_ne!(f64::from_bits(bare.elasticity.bits()), 0.8_f64);
        assert_eq!(f64::from_bits(bare.min_z_vel.bits()), 3.5);
        assert_eq!(f64::from_bits(bare.max_xy_vel.bits()), 15.0);
        assert!(bare.should_use_cell_drawer);
        assert!(bare.should_fog_remove);

        assert_eq!(bare.warhead, None);
        assert_eq!(bare.damage_radius, 0);
        assert_eq!(bare.spawns_particle, None);
        assert_eq!(bare.num_particles, 0);
        assert!(!bare.is_meteor);
        assert!(!bare.is_veins);
        assert!(!bare.is_flaming_guy);
        assert!(!bare.scorch);
        assert!(!bare.crater);
        assert!(!bare.force_big_craters);
        assert!(!bare.sticky);
        assert!(!bare.psi_warning);
        assert!(!bare.double_thick);
        assert!(!bare.flamer);
    }

    /// The two true-by-default flags must be readable back to false, or the
    /// default would be indistinguishable from an authored `no`.
    #[test]
    fn anim_true_by_default_flags_accept_an_authored_no() {
        let ini = IniFile::from_str(
            "[HIDDEN]\n\
             ShouldUseCellDrawer=no\n\
             ShouldFogRemove=no\n",
        );
        let reg = ArtRegistry::from_ini(&ini);
        let hidden = reg.anim_runtime_config("HIDDEN").unwrap();
        assert!(!hidden.should_use_cell_drawer);
        assert!(!hidden.should_fog_remove);
    }

    #[test]
    fn anim_middle_effect_flags_and_particle_spawn_parse() {
        let ini = IniFile::from_str(
            "[VIRUSD]\n\
             SpawnsParticle=  gasCloud  \n\
             NumParticles=8\n\
             Scorch=yes\n\
             Crater=yes\n\
             ForceBigCraters=yes\n\
             Sticky=yes\n\
             PsiWarning=yes\n\
             DoubleThick=yes\n\
             Flamer=yes\n\
             IsMeteor=yes\n\
             IsVeins=yes\n\
             IsFlamingGuy=yes\n",
        );
        let reg = ArtRegistry::from_ini(&ini);
        let config = reg.anim_runtime_config("VIRUSD").unwrap();
        assert_eq!(config.spawns_particle.as_deref(), Some("GASCLOUD"));
        assert_eq!(config.num_particles, 8);
        assert!(config.scorch);
        assert!(config.crater);
        assert!(config.force_big_craters);
        assert!(config.sticky);
        assert!(config.psi_warning);
        assert!(config.double_thick);
        assert!(config.flamer);
        assert!(config.is_meteor);
        assert!(config.is_veins);
        assert!(config.is_flaming_guy);
    }

    #[test]
    fn anim_bounds_preserve_omission_explicit_zero_and_negative_one() {
        let ini = IniFile::from_str(
            "[OMITTED]\n\
             FixtureOnly=1\n\
             [ZERO]\nEnd=0\nLoopEnd=0\n\
             [LAST]\nEnd=-1\nLoopEnd=-1\n",
        );
        let reg = ArtRegistry::from_ini(&ini);
        let omitted = reg.anim_runtime_config("OMITTED").unwrap();
        assert_eq!(omitted.explicit_end, None);
        assert_eq!(omitted.explicit_loop_end, None);
        let zero = reg.anim_runtime_config("ZERO").unwrap();
        assert_eq!(zero.explicit_end, Some(0));
        assert_eq!(zero.explicit_loop_end, Some(0));
        let last = reg.anim_runtime_config("LAST").unwrap();
        assert_eq!(last.explicit_end, Some(-1));
        assert_eq!(last.explicit_loop_end, Some(-1));
    }

    #[test]
    fn damage_fire_pixel_offsets_bind_verified_world_deltas() {
        assert_eq!(damage_fire_world_delta(-24, -1), Some((-110, 93)));
        assert_eq!(damage_fire_world_delta(64, 36), Some((580, 34)));
        assert_eq!(damage_fire_world_delta(i32::MIN, 0), None);
    }

    #[test]
    fn loaded_bounds_allow_native_sentinels_but_reject_out_of_range_values() {
        assert!(validate_loaded_bound("FIRE", "End", -1, 30).is_ok());
        assert!(validate_loaded_bound("FIRE", "End", 0, 30).is_ok());
        assert!(validate_loaded_bound("FIRE", "End", 30, 30).is_ok());
        assert!(validate_loaded_bound("FIRE", "End", -2, 30).is_err());
        assert!(validate_loaded_bound("FIRE", "End", 31, 30).is_err());
    }

    #[test]
    fn loaded_bounds_apply_shadow_before_explicit_native_overrides() {
        assert_eq!(
            resolve_loaded_bounds("FIRE", 64, true, None, None).unwrap(),
            (32, 32),
        );
        assert_eq!(
            resolve_loaded_bounds("FIRE", 64, true, Some(7), None).unwrap(),
            (7, 32),
        );
        assert_eq!(
            resolve_loaded_bounds("FIRE", 64, true, Some(0), Some(-1)).unwrap(),
            (0, -1),
        );
    }
}

#[cfg(test)]
#[path = "art_data_tests.rs"]
mod tests;
