//! Game data parsed from the retail YR INI files, and the reference for how gamemd reads them.
//!
//! ## Retail INI data
//!
//! What gamemd reads and how, and where VERA reproduces it. Addresses are retail
//! gamemd.exe instructions (Ghidra labels are leads); nothing here was executed.
//!
//! ### Files and layers
//! - gamemd never opens the RA2 base INIs: no `RULES.INI`, `ART.INI`, `AI.INI`,
//!   `SOUND.INI`, `EVA.INI` or `THEME.INI` string exists; those files in `ini/` are unused.
//!   `Init_Game` 0x0052BA60 loads `SOUNDMD.INI` (0x0052C6DF, required), `EVAMD.INI`
//!   (0x0052C7AF), `THEMEMD.INI` (0x0052C8B9), then `Load_Game_Rules` (0x0052C95C).
//! - `Load_Game_Rules` 0x0052CD70: root `RULESMD.INI` (a loose `RULEMD*.INI` adds a
//!   picker) gets only color schemes, `[ColorAdd]`, `[AudioVisual]` and
//!   `[MultiplayerDialogSettings]`, plus `[Movies]` from ART; `ARTMD.INI` -> g_ArtINI
//!   0x00887180 (required); `LANGRULE.INI` with a valid `[Digest]` gets a full
//!   `RulesClass::Process` (retail ships none); `AIMD.INI` -> g_AIINI.
//! - `ScenarioClass::Full_Init` 0x00686B20, in order:
//!   1. campaign: a scenario-named `.INI` Process, if present, before the reset
//!      (0x00686D35);
//!   2. multiplayer: `[Countries]`, `[General]`, HouseType bodies from RULESMD;
//!   3. reset 0x006686C0 (random maps call it too, 0x0059A0F0): destroy type
//!      registries; Process RULESMD (0x00668A27), LANGRULE (0x00668B05); re-read
//!      `[ColorAdd]` from RULESMD; Process the mode INI named in `MPModesMD.ini` if a
//!      mode is set (0x00668BAA). A LANGRULE digest failure skips the last two;
//!   4. map INI Process (0x0068774F): overrides any rules section, registries included;
//!   5. AI registries from AIMD, then from the map (0x0068797A..0x006879E3);
//!   6. game mode 5 with flag 0x00A8ED91: `TMCJ4F.INI` Process (0x00687B76).
//! - ART, SOUNDMD, EVAMD, THEMEMD are never layered; only ARTMD enters g_ArtINI
//!   (0x0052D053, reload 0x00679EE0). A later `Image=` still redirects a type's art.
//!
//! ### Parser: `INIClass::LoadFromStraw` 0x00525A60
//! - Bytes, not text. LF ends a line, CR is dropped anywhere, 511 bytes are kept and the
//!   rest discarded (`Straw::ReadLine` 0x0065D5C0); NUL ends the line.
//! - Header: trimmed line starting `[` with a `]`; name = raw bytes to the first `]`.
//! - Entry: `;` cuts the line (`#` is not a comment); split at the first `=`; trim both
//!   sides. An empty key or value is dropped: `Key=` is absent and cannot clear an
//!   earlier layer. Lines before the first header and entry-less sections are dropped.
//! - Lookup hashes raw bytes (CRCEngine 0x004A1DE0): section and key names are
//!   case-sensitive. gamemd never reads retail `Maxdebris` (17 sections),
//!   `JumpJetAccel`/`JumpJetTurnRate` (8 each), `Vshift`, `Fshift`, `volume`.
//! - Duplicates in one file are all stored; which one a lookup finds depends on qsort
//!   order (0x0052B6A0, 0x0052B720). VERA takes the first (residual, stock-inert).
//!
//! ### Readers: `CCINIClass` (absent section, key or value -> the default argument)
//! - ReadInt 0x005276D0: `$`-prefix or `h`-suffix hex (failure: default), else `atoi`.
//! - ReadBool 0x005295F0: first char `1`/`T`/`Y` true, `0`/`F`/`N` false, else default.
//! - ReadDouble 0x005283D0: `%f` into f32, widened; any `%` -> x0.01; junk -> stale bits.
//! - ReadString 0x00528A10: byte cut at the call site's capacity, then trim.
//! - ReadRange 0x00474620: absent or -1.0 -> default; else x256.0 and chop (disassembly
//!   only: the decompile hides the FMUL).
//! - Enum by name: `_stricmp` table; no match is per reader (Armor 0x00772A50 -> 0,
//!   MovementZone 0x00474E40 -> -1, SpeedType 0x0048DFF0 -> -1).
//! - Unknown names: type references (`Warhead=`, `Anim=`) allocate a default type
//!   (0x0075E3B0; `none`/`<none>` -> null); lookup-only reads (`CrushSound=`,
//!   `Prerequisite=`) keep the field or drop the list element.
//! - Lists: `strtok(",")`, tokens untrimmed; a present list replaces the old one.
//! - Most type and `[General]` reads pass the current field as default, so a later layer
//!   changes only the keys it names (0x00772080, 0x00670EDD). The land type table reader
//!   0x00674000 passes literals instead. Confirm each reader.
//! - Post-read passes overwrite values: weapon pass 0x007729F0 rewrites `Speed` from
//!   `Range` when the projectile's +0x2DC is 0. Port them with the keys.
//!
//! ### Types
//! - `RulesClass::Process` 0x00668BF0 allocates from `[Countries]`, `[Sides]` (key is
//!   the ID), then Overlay, SuperWeapon, Warhead, Smudge, Terrain, Building, Vehicle,
//!   Aircraft, Infantry, Animation, VoxelAnim, Particle, ParticleSystem lists, walking
//!   entries by index and re-reading each value by key name (keys must be unique).
//!   Weapons and projectiles have no registry; a body naming them allocates them.
//! - Type identity is ASCII case-insensitive (0x007480D0), cut to 24 bytes (0x00410800);
//!   the body is looked up exact-case by the first spelling (`0=htnk` never reads
//!   `[HTNK]`). No body section leaves constructor defaults (0x00410A60).
//! - `ReadTypeData` 0x00679A10 reads bodies family by family; a type allocated after its
//!   family's loop keeps constructor defaults until a later pass.
//! - `ObjectTypeClass::ReadINI` 0x005F92D0 reads `Image=` from the rules pass, then art
//!   keys from ARTMD `[<Image>]`. `[Animations]` bodies come only from ARTMD.
//!
//! ### In VERA
//! - Parser `ini_parser.rs`, readers `ini_value.rs` (not `str::parse`); layers
//!   `native_processing.rs` (`RulesLayerStack`), `process_owner.rs`, loaders
//!   `app/loading/init_helpers.rs`, `app/loading/init.rs`. Typed readers fold passes via
//!   `projected_values`, which cannot model per-pass clamps or literal defaults. Fields:
//!   each type's `from_ini_section`; `art_data.rs`, `team_ai_ini.rs`, `sound_ini.rs`.
//! - VERA-only: `sound.ini` under SOUNDMD, case-insensitive sound keys, fatal missing
//!   mode INI (native: empty INI, 0x005D67B0); no scenario-named `.INI`, `TMCJ4F.INI` or
//!   `[Digest]` handling.
//!
//! ### Retail INIs in tests
//! - `cargo run --bin extract-ini` fills the gitignored `ini/`, RA2 base files included;
//!   mode INIs and maps are not extracted (use `asset`/`asset-browser`).
//! - `retail_ini_fixture.rs`: a missing file prints SKIPPED and passes unless
//!   `VERA20K_REQUIRE_RETAIL_INI=1`; `retail_rules_and_art()` has no LANGRULE, mode or
//!   map layer. Test-only `load_rules_with_merged_ini` runs the production stack.
//!
//! ## Dependency rules
//! - rules/ depends on: assets/ (reads INI files extracted from .mix archives)
//! - rules/ is depended on by: sim/, map/, render/, sidebar/, app/, audio/, ui/, net/
//! - rules/ does NOT depend on (outside tests): sim/, render/, ui/, sidebar/, audio/, net/

pub mod animation_sequence;
pub mod art_data;
pub mod bridge_warheads;
pub mod color_add;
pub mod color_scheme;
pub mod combat_damage;
pub mod crate_rules;
pub mod effect_asset_catalog;
pub mod error;
pub mod flh;
pub mod foundation;
pub mod gattling_type;
pub mod house_colors;
pub mod infantry_sequence;
pub mod ini_enum;
pub mod ini_parser;
pub mod ini_value;
pub mod jumpjet_params;
pub mod locomotor_type;
pub mod mind_control_rules;
pub mod missile_spawn;
pub mod mission_data;
pub mod native_processing;
pub mod object_type;
pub mod overlay_types;
pub mod particle_system_type;
pub mod particle_type;
pub mod powerups;
pub(crate) mod process_owner;
pub mod projectile_type;
pub mod radar_event_config;
pub mod ruleset;
pub mod shp_vehicle_sequence;
pub mod smudge_type;
pub mod sound_ini;
pub mod superweapon_type;
pub mod team_ai_ini;
pub mod terrain_asset_catalog;
pub mod terrain_object_type;
pub mod terrain_rules;
pub mod tiberium_type;
pub mod voxel_anim_type;
pub mod warhead_type;
pub mod weapon_type;

#[cfg(test)]
mod path_delay_rules_tests;
#[cfg(test)]
pub(crate) mod retail_ini_fixture;
