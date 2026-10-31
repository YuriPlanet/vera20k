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

use crate::rules::ares_armor::AresWarheadVerses;
use crate::rules::armor::{ArmorId, ArmorInteraction};
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
    /// The resolved `Particle=` pointer at WarheadTypeClass+0x140. Constructor
    /// 0x0075CF4D starts null; ReadINI @0x0075D4BE retains it for an empty
    /// ReadString and stores ParticleSystemTypeClass::FindOrAllocate's answer
    /// @0x00644890 for a nonempty name (`none`/`<none>` resolve null).
    pub particle_system: Option<String>,
    /// Damage effectiveness per armor type, full f64 precision (gamemd stores
    /// Verses as a `double[11]` at warhead+0xA0 and keeps full precision through
    /// the damage kernel — the single float exception). Index order: 0=none,
    /// 1=flak, 2=plate, 3=light, 4=medium, 5=heavy, 6=wood, 7=steel,
    /// 8=concrete, 9=special_1, 10=special_2. Defaults to `[1.0; 11]` (100%)
    /// on each admitted rules layer without `Verses=`. The process registry
    /// retains the result when that layer has no section for this warhead.
    pub verses_f64: [f64; 11],
    /// Immutable view of the separately retained Ares WH extension.
    ares_verses: Option<AresWarheadVerses>,
    ares: Option<crate::rules::ares_warhead::AresWarheadRules>,
    /// Splash radius in cells (0 = direct hit only). Native `CellSpread=` is a
    /// **float** at `WarheadTypeClass+0x124` (ReadDouble `0x005283d0` called at
    /// `0x0075d3e6`, `FSTP float ptr` at `0x0075d3eb`, key string
    /// `0x00847ea0`), widened exactly to f64. Its splash scan reads are in
    /// `sim::combat::cell_spread`.
    pub cell_spread_f64: f64,
    /// Native binary32 `CellInset=` at `WarheadTypeClass+0x128`, read by
    /// `WarheadTypeClass::ReadINI @ 0x0075D3A0` after `CellSpread` and defaulted
    /// to the constructor's zero. Kept as f32 because the native field is a
    /// float and `TriggersCellInset @ 0x006F7660` subtracts it from CellSpread.
    pub cell_inset_f32: f32,
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
    /// `Sparky=`, `Bullets=`, `Deform=` and `DeformThreshhold=` are retained
    /// from their WarheadTypeClass ReadINI fields; their gameplay effects are
    /// outside these state-retention issues. `Sparky` has one consumer:
    /// `BuildingClass::ReceiveDamage @ 0x00442230` tests it for each
    /// foundation cell of its NowYellow/NowRed loop (`0x00442785`) and, by
    /// Scenario `RandomRanged` draws (`0x004427C0`, `0x00442804`), may build
    /// one of Rules' `OnFire` anims (`+0xB78`) there. That arm is dormant:
    /// the constructor stores `false` (`0x0075CF77`), and all 28 stock
    /// `Sparky=` entries are `no`, including `[Fire]` and `[Fire2]`. The
    /// `Apply_area_damage @ 0x00489280` helper does not read the retained
    /// metadata above.
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
    /// Phobos `AffectsOwner`; `None` falls back to [`Self::affects_allies`].
    pub affects_owner: Option<bool>,
    /// Phobos `AffectsEnemies`, default true.
    pub affects_enemies: bool,
    /// Phobos `AffectsNeutral`, default true.
    pub affects_neutral: bool,
    /// Psychic-damage immunity selector (distinct from Psychedelic).
    /// `WarheadTypeClass+0x178` (ReadINI `0x0075d9d2`, key string
    /// `0x00847cd8`).
    pub psychic_damage: bool,
    /// Explosion animation names indexed by damage magnitude (AnimList= in rules.ini).
    /// The original engine selects by `damage / 25`, clamped to list length.
    /// Example: ["XGRYSML1","EXPLOSML","EXPLOMED","EXPLOLRG","TWLT070"].
    pub anim_list: Vec<String>,
    /// `InfDeath=`: the arm of `InfantryClass::ReceiveDamage`'s death ladder
    /// (`0x00517FA0`, `world::infantry_death`) this warhead's kill takes. A
    /// signed dword at `WarheadTypeClass+0x120`: the constructor stores
    /// [`Self::DEFAULT_INF_DEATH`] (`0x0075CF14`), and ReadINI reads
    /// `INIClass::ReadInt @ 0x005276D0` over the current value with no clamp
    /// (`0x0075D75D..0x0075D771`, key string `0x00847D88`). The ladder's table
    /// (`0x00518D58`) takes 1..=10; any other value, the default 0 included,
    /// UnInits the infantryman inside the killing call with neither sequence
    /// nor anim (`0x005185C8` -> `0x00518B9A`). Retail `[CometWH]` (the
    /// Prism Tank's beam and fragments) and `[GrandCannonWH]` keep the 0.
    pub inf_death: i32,
    /// Retained native metadata: `Deform=` is a double at WarheadTypeClass+
    /// 0x98, read by ReadDouble @0x005283D0 from ReadINI @0x0075D78D over the
    /// constructor's 0.0 (`0x0075CED8`).
    pub deform_f64: f64,
    /// Exact native key spelling is `DeformThreshhold=`. ReadInt
    /// @0x005276D0 at ReadINI @0x0075D7A7 stores the signed value at +0x100;
    /// constructor default is 0 (`0x0075CEF8`) and there is no post-read clamp.
    pub deform_threshold: i32,
    /// `Bullets=` at +0x17A, ReadBool @0x005295F0 called from ReadINI
    /// @0x0075DA01; constructor default false (`0x0075D04A`).
    pub bullets: bool,
    /// `Veinhole=` at +0x17B, ReadBool @0x005295F0 called from ReadINI
    /// @0x0075DA1B; constructor default false (`0x0075D050`).
    pub veinhole: bool,
    /// `ShakeXlo/ShakeXhi/ShakeYlo/ShakeYhi`, signed `ReadInt` fields at
    /// WarheadTypeClass+0x17C/+0x180/+0x184/+0x188. ReadINI @0x0075D3A0
    /// reads them in Xlo, Xhi, Ylo, Yhi order, each with the current field as
    /// default; the constructor @0x0075CED0 initializes all four to zero.
    /// BulletClass::DetonateAtCoord @0x004690B0 uses each nonzero pair to
    /// conditionally draw and write its axis before the special/damage chain.
    pub shake_x_lo: i32,
    pub shake_x_hi: i32,
    pub shake_y_lo: i32,
    pub shake_y_hi: i32,

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
    /// Phobos `AirstrikeTargets=`: which target kinds an `Airstrike=` warhead
    /// may designate. Default `buildings`.
    #[cfg(feature = "phobos")]
    pub airstrike_targets: crate::rules::airstrike_targets::AirstrikeTargets,
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
    /// `combat::world_receiver`'s `emit_warhead_debris` owns both arms after
    /// their one budget: a nonempty list throws its VoxelAnims through the
    /// typed selector Techno deaths share (`throw_typed_debris`,
    /// `0x00469D40..0x00469E18`), an empty one the MetallicDebris SHPs.
    /// General Airburst child emission (`0x00469EBA..0x0046A28A`) is a
    /// separate later mechanism.
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
    /// Phobos `WarheadTypeExt::AllowDamageOnSelf` (`Nullable<bool>`): this
    /// warhead may deal damage and damage-adjacent effects to the object that
    /// fired it even when that object's type keeps `DamageSelf=no`.
    /// `Explosion_Damage_DamageSelf @ 0x004896EC` skips the area-list source
    /// exclusion, `BuildingClass_ReceiveDamage_DamageSelf @ 0x0044224F` skips
    /// the building wrapper's self return, and
    /// `MapClass_DamageArea_AirDamageSelfFix @ 0x00489416` applies the same
    /// rule to the airborne phase. `None` falls back to
    /// [`crate::rules::ruleset::RuleSet::allow_damage_on_self_for`]'s
    /// `[General] AllowDamageOnSelf` value.
    #[cfg(feature = "phobos")]
    allow_damage_on_self: Option<bool>,
    /// Phobos `WarheadTypeExt::DecloakDamagedTargets` (`Nullable<bool>`): a
    /// surviving object this warhead damages calls native
    /// `StartUncloaking(0)` (`TechnoClass::ReceiveDamage @ 0x0070281D`) only
    /// when this is true. `None` falls back to
    /// [`crate::rules::ruleset::RuleSet::decloak_damaged_targets_for`]'s
    /// `[General] DecloakDamagedTargets` value.
    #[cfg(feature = "phobos")]
    decloak_damaged_targets: Option<bool>,
    /// Phobos warhead shield overrides (`Shield.Penetrate`,
    /// `Shield.AffectTypes`, `Shield.Penetrate.Types`, absorb/pass and
    /// received-damage bounds). Phobos `WarheadTypeExt` reads them after the
    /// stock keys; a build without the capability carries only the defaults.
    #[cfg(feature = "phobos")]
    pub shield: crate::rules::phobos_shields::ShieldWarheadRules,
    /// Phobos `WarheadTypeExt`'s `Crit.*` block, read in `LoadFromINI` order.
    #[cfg(feature = "phobos")]
    pub crit: crate::rules::phobos_crit::CritRules,
    /// Phobos `WarheadTypeExt::Jumpjet*`: overrides for an `IsLocomotor=`
    /// warhead whose installed locomotor is Jumpjet.
    #[cfg(feature = "phobos")]
    pub jumpjet_override: crate::rules::jumpjet_params::JumpjetOverride,
    /// Phobos `WarheadTypeExt::MindControl_Anim` (`MindControl.Anim=`): the
    /// ring animation this warhead gives a captured object; `None` uses
    /// `[CombatDamage] ControlledAnimationType`.
    #[cfg(feature = "phobos")]
    pub mind_control_anim: Option<String>,
    /// Phobos AttachEffect attach/remove lists and the reflected-damage
    /// suppressor this warhead carries.
    #[cfg(feature = "phobos")]
    pub attach_effect: WarheadAttachEffectRules,
    /// Phobos `WarheadTypeExt::AffectsGround`/`AffectsAir` (`Body.cpp:413..414`,
    /// `Valueable<bool>` default true): `CanAffectTarget`'s air/ground split.
    #[cfg(feature = "phobos")]
    pub affects_ground: bool,
    #[cfg(feature = "phobos")]
    pub affects_air: bool,
    /// Phobos `EffectsRequireVerses` (`Body.cpp:501`, default true): the
    /// per-target pass skips a target whose armor this warhead's Verses zero.
    #[cfg(feature = "phobos")]
    pub effects_require_verses: bool,
    /// Phobos `ApplyPerTargetEffectsOnDetonate` (`Body.cpp:442`,
    /// `Nullable<bool>`); `None` inherits `[CombatDamage]`.
    #[cfg(feature = "phobos")]
    pub apply_per_target_effects_on_detonate: Option<bool>,
    /// Phobos animation-list enhancement settings (`AnimList.*`,
    /// `SplashList.*`, `CreateAnimsOnZeroDamage`, `Conventional.IgnoreUnits`).
    #[cfg(feature = "phobos")]
    pub phobos_anim: crate::rules::phobos_warhead_anim::PhobosWarheadAnimRules,
}

/// Phobos `AEAttachInfoTypeClass` as a warhead carries it: the AE lists a
/// detonation applies to every target it reaches, the attach parameters its
/// `Attach` call passes (`hasDelay = false`, so the delay lists never apply),
/// and the reflect suppressor.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarheadAttachEffectRules {
    /// `AttachEffect.AttachTypes`.
    pub attach_types: Vec<String>,
    /// `AttachEffect.DurationOverrides`, paired with `attach_types`.
    pub duration_overrides: Vec<i32>,
    /// `AttachEffect.CumulativeSourceMaxCount`; negative means no cap.
    pub cumulative_source_max_count: i32,
    pub cumulative_refresh_all: bool,
    pub cumulative_refresh_all_on_attach: bool,
    pub cumulative_refresh_same_source_only: bool,
    /// `AttachEffect.ReplaceLongerDuration`; `None` inherits `[General]`.
    pub replace_longer_duration: Option<bool>,
    /// `AttachEffect.RemoveTypes`.
    pub remove_types: Vec<String>,
    /// `AttachEffect.RemoveGroups`.
    pub remove_groups: Vec<String>,
    /// `AttachEffect.CumulativeRemoveMinCounts`, paired by list position.
    pub cumulative_remove_min_counts: Vec<i32>,
    /// `AttachEffect.CumulativeRemoveMaxCounts`, paired by list position.
    pub cumulative_remove_max_counts: Vec<i32>,
    /// `SuppressReflectDamage`: the warhead turns reflection off. With
    /// `.Types`/`.Groups` listed it suppresses only those effects; alone it
    /// suppresses every effect's reflection.
    pub suppress_reflect_damage: bool,
    pub suppress_reflect_damage_types: Vec<String>,
    pub suppress_reflect_damage_groups: Vec<String>,
}

#[cfg(feature = "phobos")]
impl Default for WarheadAttachEffectRules {
    /// `AEAttachInfoTypeClass`'s constructor defaults.
    fn default() -> Self {
        Self {
            attach_types: Vec::new(),
            duration_overrides: Vec::new(),
            cumulative_source_max_count: -1,
            cumulative_refresh_all: false,
            cumulative_refresh_all_on_attach: false,
            cumulative_refresh_same_source_only: true,
            replace_longer_duration: None,
            remove_types: Vec::new(),
            remove_groups: Vec::new(),
            cumulative_remove_min_counts: Vec::new(),
            cumulative_remove_max_counts: Vec::new(),
            suppress_reflect_damage: false,
            suppress_reflect_damage_types: Vec::new(),
            suppress_reflect_damage_groups: Vec::new(),
        }
    }
}

#[cfg(feature = "phobos")]
impl WarheadAttachEffectRules {
    fn from_section(section: &IniSection) -> Self {
        let list = |key: &str| {
            section
                .read_list(key, 0x800)
                .map(|tokens| tokens.into_iter().map(str::to_string).collect())
                .unwrap_or_default()
        };
        Self {
            attach_types: list("AttachEffect.AttachTypes"),
            duration_overrides: section
                .read_int_list("AttachEffect.DurationOverrides")
                .unwrap_or_default(),
            cumulative_source_max_count: section
                .read_int("AttachEffect.CumulativeSourceMaxCount", -1),
            cumulative_refresh_all: section.read_bool("AttachEffect.CumulativeRefreshAll", false),
            cumulative_refresh_all_on_attach: section
                .read_bool("AttachEffect.CumulativeRefreshAll.OnAttach", false),
            cumulative_refresh_same_source_only: section
                .read_bool("AttachEffect.CumulativeRefreshSameSourceOnly", true),
            replace_longer_duration: section
                .read_phobos_nullable_bool("AttachEffect.ReplaceLongerDuration", None),
            remove_types: list("AttachEffect.RemoveTypes"),
            remove_groups: list("AttachEffect.RemoveGroups"),
            cumulative_remove_min_counts: section
                .read_int_list("AttachEffect.CumulativeRemoveMinCounts")
                .unwrap_or_default(),
            cumulative_remove_max_counts: section
                .read_int_list("AttachEffect.CumulativeRemoveMaxCounts")
                .unwrap_or_default(),
            suppress_reflect_damage: section.read_bool("SuppressReflectDamage", false),
            suppress_reflect_damage_types: list("SuppressReflectDamage.Types"),
            suppress_reflect_damage_groups: list("SuppressReflectDamage.Groups"),
        }
    }

    /// The detonation applies nothing to its targets.
    pub fn is_empty(&self) -> bool {
        self.attach_types.is_empty()
            && self.remove_types.is_empty()
            && self.remove_groups.is_empty()
    }
}

impl WarheadType {
    pub(crate) fn ares_rules(&self) -> Option<&crate::rules::ares_warhead::AresWarheadRules> {
        self.ares.as_ref()
    }
    pub(crate) fn bind_ares_rules(
        &mut self,
        rules: Option<&crate::rules::ares_warhead::AresWarheadRules>,
    ) {
        self.ares = rules.cloned();
    }

    /// `InfDeath=`'s constructor value (`0x0075CF14`, the zeroed EBX of
    /// `0x0075CED0`): what a warhead whose sections name no `InfDeath=` keeps.
    pub(crate) const DEFAULT_INF_DEATH: i32 = 0;

    pub fn interaction(&self, armor: ArmorId) -> ArmorInteraction {
        #[cfg(feature = "ares")]
        {
            self.ares_verses
                .as_ref()
                .expect("Ares build requires its retained warhead extension")
                .interaction(armor)
        }
        #[cfg(not(feature = "ares"))]
        {
            ArmorInteraction::retail(crate::util::native_x87::NativeF64Bits::from_bits(
                self.verses_f64[armor.index()].to_bits(),
            ))
        }
    }

    /// WH+149 controls an Infantry passive-target class shortcut. The
    /// installed Ares75DDCC hook skips its native derivation and clears it;
    /// effective extension entries must never reintroduce that shortcut.
    pub fn uses_medium_wood_target_mask(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            false
        }
        #[cfg(not(feature = "ares"))]
        {
            self.verses_f64[4] == 0.0 && self.verses_f64[6] == 0.0
        }
    }

    pub(crate) fn install_ares_verses(&mut self, state: Option<&AresWarheadVerses>) {
        self.ares_verses = state.cloned();
    }

    pub(crate) fn ares_verses_hash_state(&self) -> Option<&AresWarheadVerses> {
        self.ares_verses.as_ref()
    }

    /// Phobos `AllowDamageOnSelf`: `Some` is the authored per-warhead value;
    /// `None` falls back to the `[General]` global.
    #[cfg(feature = "phobos")]
    pub fn allow_damage_on_self(&self) -> Option<bool> {
        self.allow_damage_on_self
    }

    /// Phobos `DecloakDamagedTargets`: `Some` is the authored per-warhead
    /// value; `None` falls back to the `[General]` global.
    #[cfg(feature = "phobos")]
    pub fn decloak_damaged_targets(&self) -> Option<bool> {
        self.decloak_damaged_targets
    }

    /// Parse a WarheadType from a rules.ini section.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        let verses_f64 = read_verses(section, [1.0; 11]);

        // Float fields (`FSTP dword` at `0x0075D3E6`, `0x0075D424`, `0x0075D477`).
        let cell_spread_f64 = f64::from(section.read_float("CellSpread", 0.0));
        let cell_inset_f32 = section.read_float("CellInset", 0.0);
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
            particle_system: None,
            verses_f64,
            ares_verses: cfg!(feature = "ares").then(AresWarheadVerses::default),
            ares: None,
            cell_spread_f64,
            cell_inset_f32,
            percent_at_max_f64,
            causes_delay_kill: section.read_bool("CausesDelayKill", false),
            delay_kill_frames: section.read_int("DelayKillFrames", 5),
            delay_kill_at_max_f64,
            wall: section.read_bool("Wall", false),
            wood: section.read_bool("Wood", false),
            fire: section.read_bool("Fire", false),
            penetrates_bunker: section.read_bool("PenetratesBunker", false),
            affects_allies: section.read_bool("AffectsAllies", true),
            affects_owner: section
                .is_present("AffectsOwner")
                .then(|| section.read_bool("AffectsOwner", false)),
            affects_enemies: section.read_bool("AffectsEnemies", true),
            affects_neutral: section.read_bool("AffectsNeutral", true),
            psychic_damage: section.read_bool("PsychicDamage", false),
            // Conventional, AnimList and EMEffect keep the constructor's false
            // and empty values (`0x0075CF89`, `0x0075CFB3`): the ordered Rules
            // processor owns those reads and RuleSet publishes its result.
            anim_list: Vec::new(),
            inf_death: section.read_int("InfDeath", Self::DEFAULT_INF_DEATH),
            deform_f64: section.read_double("Deform", 0.0),
            deform_threshold: section.read_int("DeformThreshhold", 0),
            bullets: section.read_bool("Bullets", false),
            veinhole: section.read_bool("Veinhole", false),
            // ReadInt current defaults fold each projected Rules pass; there
            // is no post-read clamp in WarheadTypeClass::ReadINI @0x0075D3A0.
            shake_x_lo: section.read_int("ShakeXlo", 0),
            shake_x_hi: section.read_int("ShakeXhi", 0),
            shake_y_lo: section.read_int("ShakeYlo", 0),
            shake_y_hi: section.read_int("ShakeYhi", 0),

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
            #[cfg(feature = "phobos")]
            airstrike_targets: {
                let raw = section.read_string("AirstrikeTargets", "", 0x100);
                crate::rules::airstrike_targets::AirstrikeTargets::parse(&raw)
                    .unwrap_or(crate::rules::airstrike_targets::AirstrikeTargets::DEFAULT)
            },
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
            #[cfg(feature = "phobos")]
            allow_damage_on_self: section.read_phobos_nullable_bool("AllowDamageOnSelf", None),
            #[cfg(feature = "phobos")]
            decloak_damaged_targets: section
                .read_phobos_nullable_bool("DecloakDamagedTargets", None),
            #[cfg(feature = "phobos")]
            shield: crate::rules::phobos_shields::ShieldWarheadRules::from_section(section),
            #[cfg(feature = "phobos")]
            crit: crate::rules::phobos_crit::CritRules::from_section(section),
            #[cfg(feature = "phobos")]
            attach_effect: WarheadAttachEffectRules::from_section(section),
            #[cfg(feature = "phobos")]
            affects_ground: section.read_bool("AffectsGround", true),
            #[cfg(feature = "phobos")]
            affects_air: section.read_bool("AffectsAir", true),
            #[cfg(feature = "phobos")]
            effects_require_verses: section.read_bool("EffectsRequireVerses", true),
            #[cfg(feature = "phobos")]
            apply_per_target_effects_on_detonate: section
                .read_phobos_nullable_bool("ApplyPerTargetEffectsOnDetonate", None),
            #[cfg(feature = "phobos")]
            phobos_anim: crate::rules::phobos_warhead_anim::PhobosWarheadAnimRules::from_section(
                section,
            ),
            #[cfg(feature = "phobos")]
            jumpjet_override: crate::rules::jumpjet_params::JumpjetOverride::from_section(section),
            #[cfg(feature = "phobos")]
            // The name resolves at the capture site; an unknown later-layer
            // name replaces an earlier valid one, where Phobos keeps it.
            mind_control_anim: section.read_phobos_nullable_with("MindControl.Anim", None, |raw| {
                Some(raw.to_owned())
            }),
        }
    }
}

/// Original WarheadType ReadINI75DDCC..75DE5A, called once for each admitted
/// rules-layer section. The missing-key default is a literal all-100% string,
/// not the current table. A present empty cached string instead makes
/// ReadString528A10 return zero and skips the parse, retaining `current`.
/// Physical INI loading omits empty values, so those use the missing-key default.
/// Native controls: tools/rules_oracle/bridge_landing_inputs.{py,json,meta.json}.
pub(crate) fn read_verses(section: &IniSection, current: [f64; 11]) -> [f64; 11] {
    if !section.is_present("Verses") {
        return [1.0; 11];
    }
    section
        .read_list("Verses", 0x80)
        .map_or(current, |tokens| verses_from_tokens(&tokens))
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
    let mut out = [1.0_f64; 11];
    for (i, token) in tokens.iter().take(11).enumerate() {
        out[i] = verses_value(token);
    }
    out
}

/// Numeric token read shared by native Verses75DDCC and Ares100523E0.
/// A percentage uses signed32 atoi and PC53/chop multiplication, not a
/// nearest-rounded host product; nonpercent input uses the CRT prefix reader.
pub(crate) fn verses_value(token: &str) -> f64 {
    if token.contains('%') {
        let scale = X87Chop53::load_f64(PERCENT_SCALE).expect("finite original constant");
        let scaled = X87Chop53::mul(X87Chop53::load_i32(crt_atoi(token)), scale);
        f64::from_bits(
            X87Chop53::store_f64(scaled)
                .expect("signed32 percentage is finite")
                .bits(),
        )
    } else {
        parse_leading_f64(token)
    }
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

    /// Phobos `AEAttachInfoTypeClass` and `SuppressReflectDamage` on a
    /// warhead, through the production Rules reader, plus the `ReflectDamage`
    /// warhead the AttachEffect catalog alone names.
    #[cfg(feature = "phobos")]
    #[test]
    fn attach_effect_lists_and_reflect_suppressor_read_through_rules() {
        use crate::rules::ruleset::RuleSet;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=W\n\
             [Warheads]\n0=PLAIN\n1=HIT\n\
             [W]\nDamage=1\nWarhead=HIT\n\
             [HIT]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             AttachEffect.AttachTypes=AE1,AE2\nAttachEffect.RemoveTypes=AE3\n\
             AttachEffect.RemoveGroups=tag\n\
             AttachEffect.DurationOverrides=40,60\n\
             AttachEffect.CumulativeSourceMaxCount=2\n\
             AttachEffect.CumulativeRefreshAll=yes\n\
             AttachEffect.CumulativeRefreshAll.OnAttach=yes\n\
             AttachEffect.CumulativeRefreshSameSourceOnly=no\n\
             AttachEffect.ReplaceLongerDuration=yes\n\
             AttachEffect.CumulativeRemoveMinCounts=2\n\
             AttachEffect.CumulativeRemoveMaxCounts=3,4\n\
             SuppressReflectDamage=yes\nSuppressReflectDamage.Types=AE1\n\
             SuppressReflectDamage.Groups=tag\n\
             [AttachEffectTypes]\n0=AE1\n1=AE2\n2=AE3\n\
             [AE1]\nReflectDamage.Warhead=SPIKE\n[AE2]\n[AE3]\n\
             [SPIKE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("rules");
        let warhead = rules.warhead("HIT").expect("warhead");
        assert_eq!(warhead.attach_effect.attach_types, vec!["AE1", "AE2"]);
        assert_eq!(warhead.attach_effect.duration_overrides, vec![40, 60]);
        assert_eq!(warhead.attach_effect.cumulative_source_max_count, 2);
        assert!(warhead.attach_effect.cumulative_refresh_all);
        assert!(warhead.attach_effect.cumulative_refresh_all_on_attach);
        assert!(!warhead.attach_effect.cumulative_refresh_same_source_only);
        assert_eq!(warhead.attach_effect.replace_longer_duration, Some(true));
        assert_eq!(warhead.attach_effect.remove_types, vec!["AE3"]);
        assert_eq!(warhead.attach_effect.remove_groups, vec!["tag"]);
        assert_eq!(warhead.attach_effect.cumulative_remove_min_counts, vec![2]);
        assert_eq!(
            warhead.attach_effect.cumulative_remove_max_counts,
            vec![3, 4]
        );
        assert!(warhead.attach_effect.suppress_reflect_damage);
        assert_eq!(
            warhead.attach_effect.suppress_reflect_damage_types,
            vec!["AE1"]
        );
        assert_eq!(
            warhead.attach_effect.suppress_reflect_damage_groups,
            vec!["tag"]
        );
        assert!(!warhead.attach_effect.is_empty());

        let plain = rules.warhead("PLAIN").expect("plain warhead");
        assert!(plain.attach_effect.is_empty());
        // `AEAttachInfoTypeClass`'s constructor defaults.
        assert_eq!(plain.attach_effect.cumulative_source_max_count, -1);
        assert!(!plain.attach_effect.cumulative_refresh_all);
        assert!(!plain.attach_effect.cumulative_refresh_all_on_attach);
        assert!(plain.attach_effect.cumulative_refresh_same_source_only);
        assert_eq!(plain.attach_effect.replace_longer_duration, None);
        assert!(plain.attach_effect.duration_overrides.is_empty());
        assert!(!plain.attach_effect.suppress_reflect_damage);
        assert!(plain.attach_effect.suppress_reflect_damage_types.is_empty());
        assert!(
            rules.warhead("SPIKE").is_some(),
            "the AttachEffect catalog keeps the warhead it alone names"
        );
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

    /// `InfDeath=` (`+0x120`): the constructor's 0 (`0x0075CF14`) when no
    /// layer names it, else ReadInt's value with no clamp
    /// (`0x0075D75D..0x0075D771`).
    #[test]
    fn inf_death_reads_unclamped_over_the_constructor_zero() {
        let ini = IniFile::from_str(
            "[Unset]\nFixtureOnly=1\n[Brute]\nInfDeath=10\n[High]\nInfDeath=11\n\
             [Negative]\nInfDeath=-2\n",
        );
        let read = |id| WarheadType::from_ini_section(id, ini.section(id).unwrap()).inf_death;
        assert_eq!(read("Unset"), 0);
        assert_eq!(read("Brute"), 10);
        assert_eq!(read("High"), 11);
        assert_eq!(read("Negative"), -2);
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

    /// The `[General] AllowDamageOnSelf` fallback and per-warhead override
    /// that Phobos `WarheadTypeExt::AllowDamageOnSelf.Get(...)` applies.
    #[cfg(feature = "phobos")]
    #[test]
    fn allow_damage_on_self_falls_back_to_general() {
        let read = |text: &str, warhead: &str| {
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(text)).unwrap();
            rules.allow_damage_on_self_for(rules.warhead(warhead).unwrap())
        };
        let with_global = "[General]\nAllowDamageOnSelf=yes\n\
             [Warheads]\n0=Unset\n1=Off\n2=On\n\
             [Unset]\nFixtureOnly=1\n[Off]\nAllowDamageOnSelf=no\n[On]\nAllowDamageOnSelf=yes\n";
        assert!(
            read(with_global, "Unset"),
            "global yes reaches an unset warhead"
        );
        assert!(
            !read(with_global, "Off"),
            "per-warhead no overrides global yes"
        );
        assert!(read(with_global, "On"), "per-warhead yes overrides global");

        let no_global =
            "[Warheads]\n0=Unset\n1=On\n[Unset]\nFixtureOnly=1\n[On]\nAllowDamageOnSelf=yes\n";
        assert!(!read(no_global, "Unset"), "absent global defaults to no");
        assert!(read(no_global, "On"));
    }

    /// The `[General] DecloakDamagedTargets` fallback (default yes) and the
    /// per-warhead override that Phobos
    /// `WarheadTypeExt::DecloakDamagedTargets.Get(...)` applies.
    #[cfg(feature = "phobos")]
    #[test]
    fn decloak_damaged_targets_falls_back_to_general() {
        let read = |text: &str, warhead: &str| {
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(text)).unwrap();
            rules.decloak_damaged_targets_for(rules.warhead(warhead).unwrap())
        };
        let no_global =
            "[Warheads]\n0=Unset\n1=Off\n[Unset]\nFixtureOnly=1\n[Off]\nDecloakDamagedTargets=no\n";
        assert!(read(no_global, "Unset"), "absent global defaults to yes");
        assert!(!read(no_global, "Off"), "per-warhead no overrides");

        let with_global = "[General]\nDecloakDamagedTargets=no\n\
             [Warheads]\n0=Unset\n1=On\n\
             [Unset]\nFixtureOnly=1\n[On]\nDecloakDamagedTargets=yes\n";
        assert!(
            !read(with_global, "Unset"),
            "global no reaches an unset warhead"
        );
        assert!(
            read(with_global, "On"),
            "per-warhead yes overrides global no"
        );
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn mind_control_anim_reads_nullable_name() {
        let ini = IniFile::from_str(
            "[WH]\nMindControl.Anim=ALTANIM\n\
             [RESET]\nKeep=1\nMindControl.Anim=<default>\n\
             [EMPTY]\nKeep=1\n",
        );
        let warhead = WarheadType::from_ini_section("WH", ini.section("WH").unwrap());
        assert_eq!(warhead.mind_control_anim.as_deref(), Some("ALTANIM"));
        assert_eq!(
            WarheadType::from_ini_section("RESET", ini.section("RESET").unwrap()).mind_control_anim,
            None
        );
        assert_eq!(
            WarheadType::from_ini_section("EMPTY", ini.section("EMPTY").unwrap()).mind_control_anim,
            None
        );
    }
}
