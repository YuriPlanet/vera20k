//! Warhead type definitions parsed from rules.ini.
//!
//! Warheads define HOW damage is applied: effectiveness against each armor
//! type (Verses=), splash radius (CellSpread=), and damage falloff
//! (PercentAtMax=). Weapons reference warheads via `Warhead=`.
//!
//! ## rules.ini format
//! ```ini
//! [AP]
//! CellSpread=0
//! PercentAtMax=1
//! Verses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%
//! ```
//!
//! ## Verses armor order (11 types)
//! none, flak, plate, light, medium, heavy, wood, steel, concrete, special_1, special_2
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::{PERCENT_SCALE, crt_atoi, parse_leading_f64};
use crate::util::native_x87::X87Chop53;

/// A warhead definition parsed from a rules.ini section.
///
/// Warheads are the final link in the damage chain: weapon -> projectile -> warhead.
/// The Verses field determines damage effectiveness against each armor type.
#[derive(Debug, Clone)]
pub struct WarheadType {
    /// Section name in rules.ini (e.g., "AP", "HE", "SA").
    pub id: String,
    /// Damage effectiveness per armor type, full f64 precision (gamemd stores
    /// Verses as a `double[11]` at warhead+0xA0 and keeps full precision through
    /// the damage kernel — the single float exception). Index order: 0=none,
    /// 1=flak, 2=plate, 3=light, 4=medium, 5=heavy, 6=wood, 7=steel,
    /// 8=concrete, 9=special_1, 10=special_2. Defaults to `[1.0; 11]` (100%)
    /// when `Verses=` is absent.
    pub verses_f64: [f64; 11],
    /// Splash radius in cells (0 = direct hit only). Native `CellSpread=` is a
    /// **float** at `WarheadTypeClass+0x124` (ReadDouble `0x005283d0` called at
    /// `0x0075d3e6`, `FSTP float ptr` at `0x0075d3eb`, key string
    /// `0x00847ea0`), widened exactly to f64. Its splash scan reads are in
    /// `sim::combat::cell_spread`.
    pub cell_spread_f64: f64,
    /// Damage fraction at maximum spread distance. Native `PercentAtMax=` is
    /// a **float** at `WarheadTypeClass+0x12C` (ReadDouble at `0x0075d424`,
    /// `FSTP float ptr` at `0x0075d429`, key string `0x00847e84`), widened to
    /// f64 for the receiver damage kernel.
    pub percent_at_max_f64: f64,
    /// Building fatal hits may enter the PostMortem delayed-death branch.
    /// Native WarheadTypeClass `+0x130`; default false.
    pub causes_delay_kill: bool,
    /// Native signed PostMortem base duration (`+0x134`); default 5.
    pub delay_kill_frames: i32,
    /// Native binary32 `DelayKillAtMax` widened exactly to f64; default 1.0f.
    /// `WarheadTypeClass+0x138` (ReadDouble at `0x0075d477`, `FSTP float ptr`
    /// at `0x0075d47c`, key string `0x00847e54`).
    pub delay_kill_at_max_f64: f64,
    /// Whether this warhead can damage walls/bridges (Wall=yes).
    /// NO-DIFF (GSI-08.33) — pass 1's three "unparsed effect flags" have no
    /// gameplay consumer in gamemd either, and its stock-authorship claim was
    /// inverted. `WarheadTypeClass::ReadINI @ 0x0075D3A0` does store `Sparky=`
    /// (`+0x14A`), `Bullets=` (`+0x17A`), `Deform=` (`+0x98`) and
    /// `DeformThreshhold=` (`+0x100`), but exhaustive displacement sweeps find
    /// `Sparky`, `Deform` and `DeformThreshhold` reaching only
    /// `WarheadTypeClass::Compute_CRC @ 0x0075DEC0`, and `Bullets` reaching
    /// nothing at all. `Apply_area_damage @ 0x00489280`, which owns every
    /// warhead effect on ore, walls, bridges, barrels and the rocker, never
    /// touches any of them. `Deform` is Tiberian Sun cell-height deformation
    /// with its consumer deleted. And all 28 stock `Sparky=` entries are `no`,
    /// including `[Fire]` and `[Fire2]` — the opposite of "most fire weapons are
    /// Sparky". Parsing them here would add fields nothing can read.
    ///
    /// The field itself is `WarheadTypeClass+0x144` (ReadINI `0x0075d508`,
    /// key string `0x0081ac58`).
    pub wall: bool,
    /// Whether this warhead can damage terrain objects with Wood armor gate.
    /// TerrainClass::Take_Damage requires this before applying damage.
    /// `WarheadTypeClass+0x147` (ReadINI `0x0075d556`, key string
    /// `0x00847e00`).
    pub wood: bool,
    /// `Fire=` -> `WarheadTypeClass+0x14C` (ReadINI `0x0075d5f2`, key string
    /// `0x00836f04`) over the constructor's 0 (`0x0075cf83`). Techno
    /// What_Action on a Cell keeps a Fire warhead off a destroyable cliff
    /// (`0x00700908..0x00700924`).
    pub fire: bool,
    /// Whether this warhead reaches a target installed in a bunker. The active
    /// receiver has category-specific linked-building/occupant semantics.
    /// `WarheadTypeClass+0x146` (ReadINI `0x0075d53c`, key string
    /// `0x00847e08`).
    pub penetrates_bunker: bool,
    /// Whether this warhead can affect allied targets. Native default is true.
    /// `WarheadTypeClass+0x179` (ReadINI `0x0075d9f2`, key string
    /// `0x00847cc8`).
    pub affects_allies: bool,
    /// Psychic-damage immunity selector (distinct from Psychedelic).
    /// `WarheadTypeClass+0x178` (ReadINI `0x0075d9d2`, key string
    /// `0x00847cd8`).
    pub psychic_damage: bool,
    /// Explosion animation names indexed by damage magnitude (AnimList= in rules.ini).
    /// The original engine selects by `damage / 25`, clamped to list length.
    /// Example: ["XGRYSML1","EXPLOSML","EXPLOMED","EXPLOLRG","TWLT070"].
    pub anim_list: Vec<String>,
    /// Infantry death animation variant (InfDeath= in rules.ini, 0–10).
    /// Maps to Die1–Die5 infantry sequences. Default 1 (standard rifle death).
    pub inf_death: u8,

    // --- Bool fields (verified offsets from WarheadTypeClass::ReadINI) ---
    /// Conventional warhead — no special effects. `WarheadTypeClass+0x14D`,
    /// written by `WarheadTypeClass::ReadINI` @ `0x0075d4ee` from the key
    /// string at `0x00847e34`. (`+0x14B` is `Sonic=`, not this.)
    pub conventional: bool,
    /// Area-damage rocker: `489B49..489E87` visits 49 cells (center ±3 per
    /// axis) after receivers. Shared `Techno70B280` owns the loaded-VXL
    /// candidate gate (`Rocker=` in `[Warhead]`). Default `no`.
    /// `WarheadTypeClass+0x14E`, written by `WarheadTypeClass::ReadINI`
    /// @ `0x0075d5be` from the key string at `0x00847de8`.
    pub rocker: bool,
    /// Direct-hit rocker: fires an impulse on the bullet's target if that
    /// target is a vehicle (`DirectRocker=` in `[Warhead]`). Default `no`.
    /// `WarheadTypeClass+0x14F`, written by `WarheadTypeClass::ReadINI`
    /// @ `0x0075d5d8` from the key string at `0x00847dd8`.
    ///
    /// This is the eighth arm of the detonation chain — tested at
    /// `BulletClass::DetonateAtCoord @ 0x0046978e`, the only arm there whose
    /// predicate also inspects the target. Dead in stock YR: `rulesmd.ini`
    /// has no live `DirectRocker=` line (its one textual occurrence sits
    /// inside a `;` comment at line 27314), so the arm never fires in stock
    /// play. Kept correct anyway.
    pub direct_rocker: bool,
    /// Spawns tiberium/ore on impact. `WarheadTypeClass+0x148`, written by
    /// `WarheadTypeClass::ReadINI` @ `0x0075d570` from the key string at
    /// `0x00817278`.
    pub tiberium: bool,
    /// Bright flash on detonation. `WarheadTypeClass+0x150`, written by
    /// `WarheadTypeClass::ReadINI` @ `0x0075d60c` from the key string at
    /// `0x00847dd0`.
    pub bright: bool,
    /// `CLDisableRed=` / `CLDisableGreen=` / `CLDisableBlue=`
    /// (`WarheadTypeClass+0x151..+0x153`, `ReadBool` at `0x0075D621`,
    /// `0x0075D63B`, `0x0075D655`; the constructor zeroes all three at
    /// `0x0075CFA1..0x0075CFAD`): a Bright detonation's light drops that
    /// channel (flags 2 / 4 / 8).
    pub cl_disable_red: bool,
    pub cl_disable_green: bool,
    pub cl_disable_blue: bool,
    /// Positive values override the damage-derived transient combat-light size.
    /// Parsed through native `ReadDouble`, so `40%` is the chopped double just
    /// below 0.4. `WarheadTypeClass+0x13C` is a float (ReadDouble at
    /// `0x0075d496`, `FSTP float ptr` at `0x0075d49b`, key string
    /// `0x00847e44`) that native narrows again, to `0x3ECCCCCC` for `40%`;
    /// VERA keeps the double. Retail `[IonWH]`'s light size is 25 either way;
    /// a modded size whose product with 63 lands on an integer could differ by
    /// one (presentation only).
    pub combat_light_size_f64: f64,
    /// `ProneDamage=` (`WarheadTypeClass+0xF8`, ReadDouble at `0x0075D999`,
    /// `FSTP qword` at `0x0075D9A4`; the constructor stores 1.0 at
    /// `0x0075CEE4..0x0075CEEB`): the double InfantryClass::ReceiveDamage
    /// scales a prone infantryman's raw damage by before the shared
    /// Foot/Techno/Object receiver. `50%` is 0.5; `70%` chops to 0.7's own
    /// double and `80%` to the one below 0.8, each one ulp under the
    /// nearest-rounded product.
    pub prone_damage_f64: f64,
    /// Instantly destroys any wall. `WarheadTypeClass+0x145`, written by
    /// `WarheadTypeClass::ReadINI` @ `0x0075d522` from the key string at
    /// `0x00847e1c`. (`+0x151` is `CLDisableRed=`, not this.)
    pub wall_absolute_destroyer: bool,
    /// Chrono legionnaire erase effect. `WarheadTypeClass+0x15A` (ReadINI
    /// `0x0075D871`, key string `0x00817168`), tested by the detonation chain
    /// at `BulletClass::DetonateAtCoord @ 0x00469423`.
    pub temporal: bool,
    /// Changes target's locomotor (magnetron). `WarheadTypeClass+0x15B`
    /// (ReadINI `0x0075D87C`), read by
    /// `TechnoClass::What_Weapon_Should_I_Use @ 0x006F352E` and by the
    /// detonation's locomotor arm (`BulletClass::DetonateAtCoord @
    /// 0x004694CB`, `sim::locomotor_lift`).
    pub is_locomotor: bool,
    /// `Locomotor=`: the class an `IsLocomotor=` hit installs on its victim.
    /// `WarheadTypeClass+0x15C`, read at `0x0075D88F` (key string
    /// `0x0084444C`) by `INIClass::ReadCLSID @ 0x00527920` (`char[128]`,
    /// trimmed), whose default is the field's constructor value: the Teleport
    /// GUID copied from `0x007E9A90` at `0x0075CFE8`. An absent or unparseable
    /// value keeps it ([`resolve_installed_kind`]). Stock `LocomotorBeam`
    /// names the Jumpjet.
    ///
    /// [`resolve_installed_kind`]: crate::rules::locomotor_type::resolve_installed_kind
    pub locomotor: crate::rules::locomotor_type::LocomotorKind,
    /// Terror drone / attack dog / squid attach. `WarheadTypeClass+0x159`
    /// (ReadINI `0x0075D84E`, key string `0x0081717C`), tested by the
    /// detonation chain at `BulletClass::DetonateAtCoord @ 0x004693d3`.
    pub parasite: bool,
    /// Psychedelic (berserk) effect. `WarheadTypeClass+0x16D` (ReadINI
    /// `0x0075D8FB`, key string `0x00847D30`). Not a detonation-chain arm.
    pub psychedelic: bool,
    /// Crazy ivan bomb attach. `WarheadTypeClass+0x157` (ReadINI
    /// `0x0075D823`, key string `0x0081BD60`), tested by the detonation chain
    /// at `BulletClass::DetonateAtCoord @ 0x00469343`.
    pub ivan_bomb: bool,
    /// Yuri mind control. `WarheadTypeClass+0x155` (ReadINI `0x0075D7E0`, key
    /// string `0x0081BBC8`), tested first in the detonation chain at
    /// `BulletClass::DetonateAtCoord @ 0x00469211`.
    pub mind_control: bool,
    /// Poison damage. `WarheadTypeClass+0x156` (ReadINI `0x0075D800`, key
    /// string `0x00847D58`).
    pub poison: bool,
    /// Calls in airstrike. `WarheadTypeClass+0x16C` (ReadINI `0x0075D8F0`),
    /// read by `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3481`.
    pub airstrike: bool,
    /// Radiation contamination. `WarheadTypeClass+0x177`, written by
    /// `WarheadTypeClass::ReadINI` @ `0x0075d9c7` from the key string at
    /// `0x00839e80`. (`+0x179` is `AffectsAllies=`, not this.)
    pub radiation: bool,
    /// Squid finishing move — kills a weakened victim outright instead of
    /// dealing the parasite's per-cycle damage. `WarheadTypeClass+0x174`
    /// (ReadINI `0x0075D949`, key string `0x00847D10`); consumed by the
    /// ParasiteClass squid grapple `0x006297F0` (run from ParasiteClass AI
    /// `0x00629FD0`), not by the detonation chain.
    pub culling: bool,
    /// Frames a parasite hit paralyzes its victim. `WarheadTypeClass+0x170`
    /// (ReadInteger at `0x0075D92A`, key string `0x00847D18`); ParasiteClass
    /// AI restarts the victim's Foot+6A0 timer with it on every bite/grapple.
    pub paralyzes: i32,
    /// `Sonic=` (`WarheadTypeClass+0x14B`, ReadINI `0x0075D5A4`, key string
    /// `0x00847DF0`). FootClass ReceiveDamage `0x004D7330` ejects a parasite
    /// from a victim hit by a Sonic warhead.
    pub sonic: bool,
    /// Spy disguise warhead. `WarheadTypeClass+0x175` (ReadINI `0x0075D969`,
    /// key string `0x00847D00`), tested by the detonation chain at
    /// `BulletClass::DetonateAtCoord @ 0x00469a03`.
    pub makes_disguise: bool,
    /// Tesla Trooper charging a Tesla Coil. `WarheadTypeClass+0x158` (ReadINI
    /// `0x0075D82E`, key string `0x00847D48`), read by
    /// `TechnoClass::What_Weapon_Should_I_Use @ 0x006F361F` and tested by the
    /// detonation chain at `BulletClass::DetonateAtCoord @ 0x0046937a`.
    pub electric_assault: bool,
    /// Engineer defuse kit. `WarheadTypeClass+0x16E` (ReadINI `0x0075D91B`,
    /// key string `0x00847D24`), tested by the detonation chain at
    /// `BulletClass::DetonateAtCoord @ 0x004699ca`.
    pub bomb_disarm: bool,
    /// Spawns the descending half of a nuke at the target cell.
    /// `WarheadTypeClass+0x176` (ReadINI `0x0075D983`, key string
    /// `0x00847CF4`), tested last in the detonation chain at
    /// `BulletClass::DetonateAtCoord @ 0x00469a2c`.
    pub nuke_maker: bool,
    /// `EMEffect=`, a bool at `WarheadTypeClass+0x154`: `ReadBool` at
    /// `0x0075D7C1` with the field as default, which the constructor zeroes
    /// (`0x0075CFB3`). The bullet's impact resolution skips its last snaps
    /// for it (`0x00468E9F`). The one stock `EMEffect=yes`, `[EMPuls]`, is
    /// mounted by no stock weapon.
    pub em_effect: bool,

    // --- List fields ---
    /// `[VoxelAnims]` type names thrown on detonation.
    ///
    /// gamemd-derived: `WarheadTypeClass::ReadINI` "DebrisTypes" @ `0x0075DAF5`
    /// -> warhead `+0x18C`, resolved through the `VoxelAnimTypeClass` name
    /// lookup at `0x0074B960`, which answers null for `none` and `<none>`.
    /// These are voxel-anim types, NOT AnimTypes. Only the ordered Rules
    /// processor reads the key; `RuleSet` publishes its list.
    ///
    /// Stock Smashing (Brute Smash/SmashE) and TRexWH author MinDebris1 /
    /// MaxDebris3, with no typed entries: Bullet469D06..469EBA uses the
    /// fallback General.MetallicDebris SHP path on each admitted hit. This
    /// differs from TechnoType debris, which belongs to the death receiver.
    ///
    /// RESIDUAL: the authored nonempty typed-warhead arm469D40..469E18
    /// still lacks VoxelAnim emission. Its budget request precedes that arm;
    /// a nonempty list must not instead produce fallback metallic SHPs.
    /// Stock Brute's empty typed list does not exercise this residual. General
    /// Airburst child emission469EBA..46A28A is a separate later mechanism.
    pub debris_types: Vec<String>,
    /// Per-type debris count cap.
    ///
    /// gamemd-derived: `WarheadTypeClass::ReadINI` "DebrisMaximums" @
    /// `0x0075DCFC` -> warhead `+0x1A8`.
    pub debris_maximums: Vec<i32>,
    /// Lower bound on the debris count.
    ///
    /// gamemd-derived: `WarheadTypeClass::ReadINI` "MinDebris" @ `0x0075DAAB`
    /// -> warhead `+0x1C8`, clamped up to 0 at `0x0075DAC6`.
    pub min_debris: i32,
    /// Upper bound on the debris count.
    ///
    /// gamemd-derived: `WarheadTypeClass::ReadINI` "MaxDebris" @ `0x0075DA95`
    /// -> warhead `+0x1C4`. Native RAISES this to `min_debris` when it is
    /// lower (`0x0075DAE0`); it never lowers `min_debris` to meet it.
    pub max_debris: i32,
}

impl WarheadType {
    /// Parse a WarheadType from a rules.ini section.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        // The absent-key default is the reader's own all-`100%` string
        // (`0x0075DDE6`), which scans to `[1.0; 11]`.
        let verses_f64: [f64; 11] = section
            .read_list("Verses", 0x80)
            .map_or([1.0; 11], |tokens| verses_from_tokens(&tokens));

        // Float fields (`FSTP dword` at `0x0075D3E6`, `0x0075D424`, `0x0075D477`).
        let cell_spread_f64 = f64::from(section.read_float("CellSpread", 0.0));
        let percent_at_max_native = section.read_float("PercentAtMax", 1.0);
        let percent_at_max_f64 = f64::from(percent_at_max_native);
        let delay_kill_at_max_f64 = f64::from(section.read_float("DelayKillAtMax", 1.0));

        // The IntVector reader (`0x0075DD22` -> `0x00475D70`).
        let debris_maximums: Vec<i32> = section.read_int_list("DebrisMaximums").unwrap_or_default();

        // `WarheadTypeClass::ReadINI @ 0x0075DA95..0x0075DAE6`: MaxDebris is
        // read first (`0x0075DA95 PUSH 0x84439C` -> `0x0075DAB1` +0x1C4),
        // MinDebris second (`0x0075DAAB PUSH 0x844390` -> `0x0075DABE` +0x1C8),
        // MinDebris is clamped up to zero (`0x0075DAC4 JGE`), and only then is
        // MaxDebris raised to meet MinDebris (`0x0075DAE0`). The order is
        // load-bearing — a negative MinDebris must not drag MaxDebris below
        // zero.
        //
        // Both keys are read case-exactly, the same as their TechnoType
        // siblings: `CCINIClass::ReadInt @ 0x005276D0` CRCs the raw key bytes
        // and folds no case, and the pushed literals are `MaxDebris`
        // (`0x0084439C`) and `MinDebris` (`0x00844390`). No stock `[Warheads]`
        // section mis-spells either — only `Smashing` and `TRexWH` author them
        // at all — so this changes nothing in retail and is right in principle.
        let max_debris = section.read_int("MaxDebris", 0);
        let min_debris = section.read_int("MinDebris", 0).max(0);
        let max_debris = max_debris.max(min_debris);

        Self {
            id: id.to_string(),
            verses_f64,
            cell_spread_f64,
            percent_at_max_f64,
            causes_delay_kill: section.read_bool("CausesDelayKill", false),
            delay_kill_frames: section.read_int("DelayKillFrames", 5),
            delay_kill_at_max_f64,
            wall: section.read_bool("Wall", false),
            wood: section.read_bool("Wood", false),
            fire: section.read_bool("Fire", false),
            penetrates_bunker: section.read_bool("PenetratesBunker", false),
            affects_allies: section.read_bool("AffectsAllies", true),
            psychic_damage: section.read_bool("PsychicDamage", false),
            // Conventional, AnimList and EMEffect keep the constructor's false
            // and empty values (`0x0075CF89`, `0x0075CFB3`): the ordered Rules
            // processor owns those reads and RuleSet publishes its result.
            anim_list: Vec::new(),
            inf_death: section.read_int("InfDeath", 1).clamp(0, 10) as u8,

            // Bool fields — all default false
            conventional: false,
            rocker: section.read_bool("Rocker", false),
            direct_rocker: section.read_bool("DirectRocker", false),
            tiberium: section.read_bool("Tiberium", false),
            bright: section.read_bool("Bright", false),
            cl_disable_red: section.read_bool("CLDisableRed", false),
            cl_disable_green: section.read_bool("CLDisableGreen", false),
            cl_disable_blue: section.read_bool("CLDisableBlue", false),
            combat_light_size_f64: section.read_double("CombatLightSize", 0.0),
            prone_damage_f64: section.read_double("ProneDamage", 1.0),
            wall_absolute_destroyer: section.read_bool("WallAbsoluteDestroyer", false),
            temporal: section.read_bool("Temporal", false),
            is_locomotor: section.read_bool("IsLocomotor", false),
            locomotor: crate::rules::locomotor_type::resolve_installed_kind(
                section.read_name("Locomotor", 0x80),
            ),
            parasite: section.read_bool("Parasite", false),
            psychedelic: section.read_bool("Psychedelic", false),
            ivan_bomb: section.read_bool("IvanBomb", false),
            mind_control: section.read_bool("MindControl", false),
            poison: section.read_bool("Poison", false),
            airstrike: section.read_bool("Airstrike", false),
            radiation: section.read_bool("Radiation", false),
            culling: section.read_bool("Culling", false),
            paralyzes: section.read_int("Paralyzes", 0),
            sonic: section.read_bool("Sonic", false),
            makes_disguise: section.read_bool("MakesDisguise", false),
            electric_assault: section.read_bool("ElectricAssault", false),
            bomb_disarm: section.read_bool("BombDisarm", false),
            nuke_maker: section.read_bool("NukeMaker", false),
            em_effect: false,

            // List fields
            debris_types: Vec::new(),
            debris_maximums,
            min_debris,
            max_debris,
        }
    }
}

/// Original WarheadType ReadINI75DDCC..75DE5A, over the `strtok(",")`
/// tokens of the `char[128]` ReadString. Each token containing `%` takes
/// signed32 atoi, FILD and PC53/chop multiplication by the original binary64
/// 0.01 constant; other tokens use CRT atof's binary64 numeric prefix.
/// Original-reader and damage goldens: rules_oracle/bridge_landing_inputs.
///
/// Rust retains safe trailing 1.0 entries for malformed short lists. Native
/// unconditionally performs eleven strchr calls and faults on a null strtok
/// result; this recovery is deliberately not described as native equivalence.
fn verses_from_tokens(tokens: &[&str]) -> [f64; 11] {
    // Nearest-rounded host multiplication by the original 0.01 changes retail
    // HE's integer damage by one for several armor classes.
    let scale = X87Chop53::load_f64(PERCENT_SCALE).expect("finite original constant");
    let mut out = [1.0_f64; 11];
    for (i, token) in tokens.iter().take(11).enumerate() {
        out[i] = if token.contains('%') {
            let scaled = X87Chop53::mul(X87Chop53::load_i32(crt_atoi(token)), scale);
            f64::from_bits(
                X87Chop53::store_f64(scaled)
                    .expect("signed32 percentage is finite")
                    .bits(),
            )
        } else {
            parse_leading_f64(token)
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn native_bridge_landing_verses_reader_bits() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/bridge_landing_inputs.json",
        ))
        .unwrap();
        let mut completed = 0;
        let mut native_faults = 0;
        for row in native["verses_cases"].as_array().unwrap() {
            if row["status"] == "null_strtok_token_fault" {
                native_faults += 1;
                continue; // Native crashes here; Rust's safe recovery is not a parity claim.
            }
            let raw = row["raw"].as_str();
            let mut text = "[HE]\nFixtureOnly=1\n".to_string();
            if let Some(raw) = raw {
                text.push_str(&format!("Verses={raw}\n"));
            }
            let ini = IniFile::from_str(&text);
            let actual = WarheadType::from_ini_section("HE", ini.section("HE").unwrap());
            for (index, value) in actual.verses_f64.iter().enumerate() {
                assert_eq!(
                    format!("{:016x}", value.to_bits()),
                    row["verses_bits"][index],
                    "raw={raw:?}, armor{index}"
                );
            }
            completed += 1;
        }
        assert_eq!(completed, 13);
        assert_eq!(native_faults, 4);
    }

    #[test]
    fn test_parse_warhead() {
        let ini: IniFile = IniFile::from_str(
            "[AP]\nCellSpread=0.5\nPercentAtMax=0.25\nWall=yes\n\
             Verses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
        );
        let section: &IniSection = ini.section("AP").unwrap();
        let wh: WarheadType = WarheadType::from_ini_section("AP", section);

        assert_eq!(wh.id, "AP");
        assert_eq!(wh.cell_spread_f64, 0.5);
        assert_eq!(wh.percent_at_max_f64, f64::from(0.25f32));
        assert!(wh.wall);
        assert!((wh.verses_f64[0] - 1.00).abs() < 1e-9); // none: 100%
        assert!((wh.verses_f64[2] - 0.90).abs() < 1e-9); // plate: 90%
        assert!((wh.verses_f64[6] - 0.60).abs() < 1e-9); // wood: 60%
        assert!((wh.verses_f64[10] - 0.0).abs() < 1e-9); // special_2: 0%
    }

    #[test]
    fn verses_f64_defaults_to_all_full_when_absent() {
        let ini: IniFile = IniFile::from_str("[Empty]\nFixtureOnly=1\n");
        let wh = WarheadType::from_ini_section("Empty", ini.section("Empty").unwrap());
        // gamemd default is all-100% (1.0), NOT empty.
        assert_eq!(wh.verses_f64, [1.0; 11]);
    }

    #[test]
    fn fractional_verses_preserved() {
        // % branch: integer atoi BEFORE x0.01 => "50.5%" -> 50*0.01 = 0.5 (NOT
        // 0.505). Bare branch: "0.505" -> 0.505 full precision.
        let pct = verses_from_tokens(&["50.5%", "1.5%", "0.5%"]);
        let bare = verses_from_tokens(&["0.505", "0.015", "0.005"]);
        assert!((pct[0] - 0.5).abs() < 1e-9); // 50.5% -> atoi(50)*0.01
        assert!((pct[1] - 0.01).abs() < 1e-9); // 1.5%  -> atoi(1)*0.01
        assert!((pct[2] - 0.0).abs() < 1e-9); // 0.5%   -> atoi(0)*0.01
        assert!((bare[0] - 0.505).abs() < 1e-9);
        assert!((bare[2] - 0.005).abs() < 1e-9);
        // Rust-only safe recovery: native faults on missing strtok tokens.
        assert!((pct[3] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_warhead_defaults() {
        let ini: IniFile = IniFile::from_str("[Empty]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("Empty").unwrap();
        let wh: WarheadType = WarheadType::from_ini_section("Empty", section);

        assert_eq!(wh.verses_f64, [1.0; 11]);
        assert_eq!(wh.cell_spread_f64, 0.0);
        assert_eq!(wh.percent_at_max_f64, 1.0);
        assert!(!wh.causes_delay_kill);
        assert_eq!(wh.delay_kill_frames, 5);
        assert_eq!(wh.delay_kill_at_max_f64, 1.0);
        assert_eq!(wh.prone_damage_f64, 1.0);
        assert!(!wh.wall);
    }

    #[test]
    fn postmortem_fields_preserve_signed_and_f32_inputs() {
        let ini = IniFile::from_str(
            "[OilExplosionWH]\nCellSpread=4\nCausesDelayKill=yes\n\
             DelayKillFrames=-7\nDelayKillAtMax=7.1\n",
        );
        let wh =
            WarheadType::from_ini_section("OilExplosionWH", ini.section("OilExplosionWH").unwrap());
        assert!(wh.causes_delay_kill);
        assert_eq!(wh.delay_kill_frames, -7);
        assert_eq!(
            wh.delay_kill_at_max_f64.to_bits(),
            f64::from(7.1_f32).to_bits()
        );
    }

    #[test]
    fn test_warhead_mind_control() {
        let ini: IniFile = IniFile::from_str(
            "[YOUREWEAK]\nMindControl=yes\nCellSpread=0\nPercentAtMax=1\n\
             Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\n",
        );
        let section: &IniSection = ini.section("YOUREWEAK").unwrap();
        let wh: WarheadType = WarheadType::from_ini_section("YOUREWEAK", section);

        assert!(wh.mind_control);
        assert!(!wh.temporal);
        assert!(!wh.ivan_bomb);
    }

    #[test]
    fn test_prone_damage_parses_as_multiplier() {
        let ini: IniFile = IniFile::from_str(
            "[AP]\nProneDamage=50%\n[Gas]\nProneDamage=300%\n[Raw]\nProneDamage=1.25\n",
        );

        let ap = WarheadType::from_ini_section("AP", ini.section("AP").unwrap());
        let gas = WarheadType::from_ini_section("Gas", ini.section("Gas").unwrap());
        let raw = WarheadType::from_ini_section("Raw", ini.section("Raw").unwrap());

        assert_eq!(ap.prone_damage_f64, 0.5);
        assert_eq!(gas.prone_damage_f64, 3.0);
        assert_eq!(raw.prone_damage_f64, 1.25);
    }

    #[test]
    fn parse_rocker_default_false() {
        let ini: IniFile = IniFile::from_str("[TestWH]\nFixtureOnly=1\n");
        let wh = WarheadType::from_ini_section("TestWH", ini.section("TestWH").unwrap());
        assert!(!wh.rocker);
        assert!(!wh.direct_rocker);
    }

    #[test]
    fn parse_rocker_yes() {
        let ini: IniFile = IniFile::from_str("[TestWH]\nRocker=yes\nDirectRocker=yes\n");
        let wh = WarheadType::from_ini_section("TestWH", ini.section("TestWH").unwrap());
        assert!(wh.rocker);
        assert!(wh.direct_rocker);
    }

    #[test]
    fn parse_retail_v3wh_has_rocker() {
        let ini = IniFile::from_str("[V3WH]\nRocker=yes\n");
        let section = ini.section("V3WH").expect("V3WH missing from rulesmd.ini");
        let wh = WarheadType::from_ini_section("V3WH", section);
        assert!(wh.rocker, "V3WH should have Rocker=yes");
        // V3WH does not set DirectRocker — default no.
        assert!(!wh.direct_rocker);
    }

    #[test]
    fn gsi_05_14_warhead_reads_debristypes_not_debris() {
        // `WarheadTypeClass::ReadINI` never looks at "Debris" — that string's
        // only reader is `TiberiumClass::ReadINI_Fields @ 0x00721B4F`. The
        // warhead key is "DebrisTypes" @ 0x0075DAF5.
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Warheads]\n0=W\n[W]\nDebris=CRYSTAL1\nDebrisTypes=TIRE\n",
        ))
        .unwrap();
        assert_eq!(rules.warhead("W").unwrap().debris_types, ["TIRE"]);
    }

    #[test]
    fn gsi_05_14_mindebris_clamps_to_zero_then_raises_maxdebris() {
        // 0x0075DAC6 clamps MinDebris up to 0; 0x0075DAE0 then raises MaxDebris
        // to MinDebris. The second clamp is the one an obvious implementation
        // gets backwards by lowering MinDebris instead.
        let ini = IniFile::from_str(
            "[Neg]
MinDebris=-4
MaxDebris=2
[Inverted]
MinDebris=7
MaxDebris=2
",
        );
        let neg = WarheadType::from_ini_section("Neg", ini.section("Neg").unwrap());
        assert_eq!((neg.min_debris, neg.max_debris), (0, 2));
        let inv = WarheadType::from_ini_section("Inverted", ini.section("Inverted").unwrap());
        assert_eq!((inv.min_debris, inv.max_debris), (7, 7));
    }
}
