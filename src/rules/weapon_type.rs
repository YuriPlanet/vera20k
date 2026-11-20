//! Weapon type definitions parsed from rules.ini.
//!
//! Each weapon in RA2 has its own `[WeaponName]` section in rules.ini,
//! defining damage, range, rate of fire, and references to a projectile
//! type and warhead type. Units reference weapons via their `Primary=`
//! and `Secondary=` keys.
//!
//! ## rules.ini format
//! ```ini
//! [105mm]
//! Damage=65
//! ROF=50
//! Range=5.75
//! Speed=40
//! Projectile=InvisibleLow
//! Warhead=AP
//! ```
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// A weapon definition parsed from a rules.ini section.
///
/// Weapons are the bridge between units and damage. A unit fires a weapon,
/// which spawns a projectile carrying a warhead. The warhead determines
/// actual damage based on the target's armor type.
///
/// Fields read by the original WeaponTypeClass::ReadINI at 0x772080.
#[derive(Debug, Clone)]
pub struct WeaponType {
    // ── Core fields ──────────────────────────────────────────────────
    /// Section name in rules.ini (e.g., "105mm", "Vulcan", "RedEye2").
    pub id: String,
    /// Base damage per hit (`+0xA4`).
    pub damage: i32,
    /// Maximum range in LEPTONS — the value gamemd actually stores.
    ///
    /// `WeaponTypeClass::ReadINI` 0x00772080 fills `+0xB4` through
    /// `CCINIClass::ReadRange` 0x00474620 at 0x00772336, and that reader
    /// multiplies the INI cell count by `256.0` (`FMUL double ptr
    /// [0x007E1710]` at 0x0047464C) before `Math__ftol` chops it, so a
    /// fractional `Range=` keeps its fraction. Every native consumer —
    /// `TechnoClass::InRange` 0x006F7220 included — reads leptons. The ordered
    /// Rules processor reads it on each reached pass; `Range=-1` keeps it.
    ///
    /// The `-2` "infinite" sentinel survives the conversion as `-512`, which
    /// is the constant 0x006F724E tests.
    pub range_leptons: i32,
    /// Rate of fire: frames between consecutive shots (lower = faster).
    pub rof: i32,
    /// Native stored travel speed in leptons/frame, after ReadSpeed and the
    /// per-pass ballistic postpass. Authored Speed=40 reads as 102. Zero alone
    /// does not designate hitscan; projectile type and launch determine motion.
    pub speed: i32,
    /// Projectile type ID (`+0xA0`, references a [ProjectileName] section).
    pub projectile: Option<String>,
    /// Warhead type ID (`+0xAC`, references a [WarheadName] section).
    pub warhead: Option<String>,
    /// Derived: [`Self::warhead`] and [`Self::projectile`] as the RuleSet
    /// holding this weapon resolved them (`RuleSet::resolve_handles`).
    parts: crate::rules::ruleset::WeaponParts,
    /// `Report=` firing sounds read by `WeaponTypeClass::ReadINI @ 0x00772080`
    /// through `ReadSoundList @ 0x00525430`, resolved against the fixed registry.
    pub report: Vec<String>,
    /// `DownReport=` read through `ReadSoundList @ 0x00525430` by
    /// `WeaponTypeClass::ReadINI @ 0x00772080`; the constructor starts with an
    /// empty list (`WeaponTypeClass::Constructor @ 0x00771C70`).
    pub down_report: Vec<String>,
    /// Number of rapid shots per attack cycle (default 1).
    /// After the full burst, the ROF cooldown begins.
    pub burst: i32,
    /// Ares WeaponTypeExt constructor100549B4 and reader10055898 (+D0).
    /// Valueable<int> retains failed/missing reads; default one round, which
    /// a stock-way read keeps.
    ares_ammo_cost: i32,
    /// Ares WeaponTypeExt rad beam, bolt and wave visuals and
    /// `IsDetachedRailgun` ([`crate::rules::ares_weapon`]).
    pub ares_visuals: crate::rules::ares_weapon::AresWeaponVisuals,
    /// Ares WeaponTypeExt custom Ivan bombs ([`crate::rules::ares_weapon`]).
    pub ares_ivan_bomb: crate::rules::ares_weapon::AresIvanBomb,
    /// Ares `ApplyDamage=` (WeaponTypeExt +0xCC, Nullable<bool>, reader
    /// 10055520): whether FireAt's shot carries `Damage=`, by default unless
    /// `IsSonic=` or `UseFireParticles=` (TechnoClass_Fire_AllowDamage
    /// 1004A370, hook 0x6FE31C).
    pub ares_apply_damage: Option<bool>,
    /// Ares WeaponTypeExt abduction ([`crate::rules::ares_weapon`]).
    pub ares_abductor: crate::rules::ares_weapon::AresAbductor,
    /// Ares `Cursor.Attack`/`Cursor.AttackOutOfRange`
    /// ([`crate::rules::ares_mouse_cursor`]), bound by the ordered Rules
    /// reader; presentation only.
    pub ares_cursors: crate::rules::ares_mouse_cursor::WeaponCursors,
    /// Ares `ProjectileRange=` (WeaponTypeExt +0xC8, Valueable<Leptons>,
    /// 100000): the leptons a FireAt bullet of a `Ranged=` type may travel
    /// (TechnoClass_Fire_CreateBullet 1001ADE0, BulletClass_Update_Ranged
    /// 1001AC90).
    pub ares_projectile_range: i32,
    /// `RevealOnFire=` (`+0x137`, default yes): a launched shot reveals the
    /// firer to a human victim (FireAt `0x006FF66C`). Retail turns it off on
    /// the sniper, Virus, disguise and Mirage weapons.
    pub reveal_on_fire: bool,
    /// `TerrainFire=` (`+0x138`, default no): the weapon may fire unforced at
    /// a Terrain object, an `IsARock=` overlay or a disguised object wearing
    /// no house (`TechnoClass` vt+0x398, `0x0070EF00`,
    /// `Simulation::fires_at_terrain`). Retail authors it only on the unused
    /// `TankMakeupKit`.
    pub terrain_fire: bool,
    /// `DisguiseFireOnly=` (`+0x13B`, constructor default false at
    /// `WeaponTypeClass::Constructor @ 0x00771C70`); `ReadBool` is called by
    /// `WeaponTypeClass::ReadINI @ 0x00772080` at `0x007721F9`.
    pub disguise_fire_only: bool,
    /// Infantry WhatAction51EA80: Attack becomes action64 (C4 cursor).
    pub sabotage_cursor: bool,
    /// Infantry WhatAction51EAD2: eligible buildings show action71 (air strike).
    pub mig_attack_cursor: bool,

    // ── Int fields ───────────────────────────────────────────────────
    /// Ambient damage dealt to units standing near the weapon's owner (+0x98).
    pub ambient_damage: i32,
    /// `DisguiseFakeBlinkTime=` (`+0x13C`): the frames a `DisguiseWhenStill=`
    /// Unit's disguise blinks after it fires this weapon
    /// (`UnitClass::Fire_At @ 0x0074145E`).
    pub disguise_fake_blink_time: i32,
    /// Minimum firing range in LEPTONS — `+0xB8` through the same
    /// `CCINIClass::ReadRange` 0x00474620 call at 0x00772350.
    pub minimum_range_leptons: i32,
    /// Signed byte duration of the laser effect, sign-extended for its timer
    /// consumer (+0x14E; constructor10, ReadInt7727DE, MOVSX6FD21D).
    pub laser_duration: i32,
    /// Radiation level emitted on impact (+0x158).
    pub rad_level: i32,
    /// Phobos `RadType=`: the `[RadiationTypes]` entry this weapon's
    /// radiation field uses (`WeaponTypeExt::RadType`, default the
    /// `Radiation` type). Only read in Phobos builds; `None` binds the
    /// default type.
    pub rad_type: Option<String>,

    // ── String/reference fields ──────────────────────────────────────
    // The Type references are the stored IDs the ordered Rules processor's
    // factories answered (`native_processing`); `RuleSet` publishes them.
    /// Animation for the native `AssaultAnim=` pointer at +0x114.
    pub assault_anim: Option<String>,
    /// Animation played by occupants when firing from a building.
    pub occupant_anim: Option<String>,
    /// Animation played by units firing from an open-topped transport.
    pub open_topped_anim: Option<String>,
    /// Particle system spawned when this weapon fires.
    pub attached_particle_system: Option<String>,
    /// Comma-separated list of impact animations, indexed by damage magnitude.
    pub anim: Vec<String>,

    // ── Color fields (3 bytes each, "R,G,B" format) ──────────────────
    /// Inner color of laser beam (+0x120).
    pub laser_inner_color: [u8; 3],
    /// Outer color of laser beam (+0x123).
    pub laser_outer_color: [u8; 3],
    /// Outer spread/glow color of laser beam (+0x126).
    pub laser_outer_spread: [u8; 3],

    // ── Bool fields ──────────────────────────────────────────────────
    /// Weapon uses fire particle effects (+0x129).
    pub use_fire_particles: bool,
    /// Weapon uses spark particle effects (+0x12a).
    pub use_spark_particles: bool,
    /// Fires in all directions regardless of facing (+0x12b).
    pub omni_fire: bool,
    /// `DistributedWeaponFire=` (`+0x12C`, constructor default false at
    /// `WeaponTypeClass::Constructor @ 0x00771C70`); `ReadBool` is called by
    /// `WeaponTypeClass::ReadINI @ 0x00772080` at `0x0077271D`.
    pub distributed_weapon_fire: bool,
    /// Weapon is a railgun (special projectile visual) (+0x12d).
    pub is_railgun: bool,
    /// Projectile follows a lobbed arc trajectory (+0x12e).
    pub lobber: bool,
    /// Weapon flash is extra bright (+0x12f).
    pub bright: bool,
    /// Weapon deals sonic/disruptor damage (+0x130).
    pub is_sonic: bool,
    /// Weapon spawns aircraft (like aircraft carriers) (+0x131).
    pub spawner: bool,
    /// `LimboLaunch=` (+0x132, ReadINI `0x00772114`): TechnoClass::Fire
    /// `0x006FF751` puts the FIRER in limbo after launching the bullet, which
    /// carries it (attack dog, Terror Drone and Giant Squid jumps).
    pub limbo_launch: bool,
    /// Unit must decloak before firing this weapon (+0x133).
    pub decloak_to_fire: bool,
    /// Uses cell-center rangefinding instead of edge-to-edge (+0x134).
    pub cell_rangefinding: bool,
    /// Weapon fires only once then is consumed (+0x135).
    pub fire_once: bool,
    /// Weapon is never automatically selected for use (+0x136).
    pub never_use: bool,
    /// Mind control effect has no unit limit (+0x140).
    pub infinite_mind_control: bool,
    /// Unit can fire this weapon while moving (+0x141). The constructor sets
    /// it (`0x00771DE6`) and `ReadINI` keeps that as its default
    /// (`0x0077221E..0x00772232`); stock only DiskDrain says no.
    pub fire_while_moving: bool,
    /// Weapon drains target's health to heal the firer (+0x142).
    pub drain_weapon: bool,
    /// `FireInTransport=` (+0x143): an open-topped passenger may fire it.
    /// The constructor sets it (`0x00771DF3`) and `ReadINI` keeps that as its
    /// default (ReadBool at `0x00772252`); stock opts only melee and special
    /// weapons out. GetFireError refuses it from an open-topped transport
    /// (`0x006FC57D`).
    pub fire_in_transport: bool,
    /// Firing this weapon kills the attacker (+0x144).
    pub suicide: bool,
    /// Grants speed boost on hit (+0x145).
    pub turbo_boost: bool,
    /// Suppresses target (Westwood's misspelling preserved) (+0x146).
    pub supress: bool,
    /// Weapon spawns a camera/reveal at impact point (+0x147).
    pub camera: bool,
    /// Weapon has limited charges (+0x148).
    pub charges: bool,
    /// Weapon fires a laser beam visual (+0x149).
    pub is_laser: bool,
    /// Weapon fires a disk-shaped laser (Vortex) (+0x14a).
    pub disk_laser: bool,
    /// Phobos `DiskLaser.Radius=` (`Ext/WeaponType/Body.cpp:86`,
    /// `Valueable<double>` constructed at `DiskLaserClass::Radius` = 240 at
    /// `Ext/WeaponType/Body.h:28,123`): the ring radius a disk laser fired by
    /// this weapon draws. `DiskLaser_Circle` (hook `0x4A757B`,
    /// `Ext/WeaponType/Hooks.DiskLaserRadius.cpp:27-42`) rebuilds the
    /// 16-point table when the value differs from the last applied radius.
    /// Phobos `ProjectileRange.ApplyModifiers=` (`Ext/WeaponType/Body.cpp:88`,
    /// `Nullable<bool>` unset at `Body.h:30,125`): whether a `Ranged=`
    /// projectile's initial range folds `GetRangeWithModifiers`; unset takes
    /// `[CombatDamage] ProjectileRange.ApplyModifiers` (default true).
    pub phobos_projectile_range_apply_modifiers: Option<bool>,
    pub phobos_disk_laser_radius: f64,
    /// Weapon draws a visible line to target (+0x14b).
    pub is_line: bool,
    /// Weapon fires a thicker laser beam (+0x14c).
    pub is_big_laser: bool,
    /// Laser color matches house/player color (+0x14d).
    pub is_house_color: bool,
    /// Weapon is affected by ion storms (+0x14f).
    pub ion_sensitive: bool,
    /// Weapon fires at area/ground instead of specific target (+0x150).
    pub area_fire: bool,
    /// Weapon fires an electric bolt visual (Tesla) (+0x151).
    pub is_electric_bolt: bool,
    /// Draw electric bolt using laser rendering (+0x152).
    pub draw_bolt_as_laser: bool,
    /// Laser uses alternate (darker) color scheme (+0x153).
    pub is_alternate_color: bool,
    /// Weapon fires a radiation beam visual (+0x154).
    pub is_rad_beam: bool,
    /// Weapon triggers a radiation eruption effect (+0x155).
    pub is_rad_eruption: bool,
    /// Weapon fires a magnetron beam (+0x15c).
    pub is_mag_beam: bool,

    // ── Phobos AttachEffect gating ───────────────────────────────────
    // `WeaponTypeExt::HasRequiredAttachedEffects` gates a shot on the AE the
    // target (or, with `CheckOnFirer`, the firer) carries. The required lists
    // must all match; the disallowed lists refuse the shot on any match.
    // Counts pair with the type/group lists by position, the last value
    // covering the rest.
    pub attach_effect_required_types: Vec<String>,
    pub attach_effect_disallowed_types: Vec<String>,
    pub attach_effect_required_groups: Vec<String>,
    pub attach_effect_disallowed_groups: Vec<String>,
    pub attach_effect_required_min_counts: Vec<i32>,
    pub attach_effect_required_max_counts: Vec<i32>,
    pub attach_effect_disallowed_min_counts: Vec<i32>,
    pub attach_effect_disallowed_max_counts: Vec<i32>,
    pub attach_effect_check_on_firer: bool,
    pub attach_effect_ignore_from_same_source: bool,
    /// `AttachEffect.Required.Any` / `Disallowed.Any`: the native
    /// `requireAll = !Any` switch. Each list defaults to its constructor value
    /// (required `false`, disallowed `true`).
    pub attach_effect_required_any: bool,
    pub attach_effect_disallowed_any: bool,
    /// `AttachEffect.Required.Houses` / `Disallowed.Houses`: the house the
    /// matched instance's invoker must belong to.
    pub attach_effect_required_houses: AffectedHouse,
    pub attach_effect_disallowed_houses: AffectedHouse,
    /// `AttachEffect.SameSourceOnly`: only instances from the shot's exact
    /// invoker and source match.
    pub attach_effect_same_source_only: bool,
    /// Phobos `ROF.RandomDelay` (`WeaponType/Body.cpp:134`,
    /// `Nullable<PartialVector2D<int>>`): this weapon's full-rearm jitter
    /// range; `None` inherits `[CombatDamage]`.
    pub rof_random_delay: Option<crate::rules::ini_value::PartialIntPair>,
    /// Phobos `WeaponTypeExt::Burst_Delays` (`Ext/WeaponType/Body.cpp:120`,
    /// `ValueableVector<int>` declared at `Body.h:52`, default empty at
    /// `Body.h:147`): one rearm delay per burst shot, consumed by
    /// [`Self::phobos_burst_delay`] through the `0x6FD05E` GetROF hook.
    pub phobos_burst_delays: Vec<i32>,
    /// Phobos `WeaponTypeExt::Burst_FireWithinSequence`
    /// (`Ext/WeaponType/Body.cpp:121`, declared `Body.h:53`, default false
    /// `Body.h:148`): an infantry firing sequence fires the burst's later shots
    /// at cumulative-delay frames (`world_fire`'s admission) instead of one per
    /// frame. The unit arm of the same hook is gated on `UnitTypeExt::FireUp`,
    /// which VERA does not read yet.
    pub phobos_burst_fire_within_sequence: bool,
    /// Phobos `WeaponTypeExt::Burst_NoDelay` (`Ext/WeaponType/Body.cpp:122`,
    /// `Valueable<bool>`, default false): an infantry or vehicle firer empties
    /// the whole burst in one frame instead of one shot per frame. Only the
    /// `InfantryClass::Fire_AG` and `UnitClass::Fire_AG` firing paths read it
    /// (hooks `0x5209EE` / `0x736F67`), and only when `Burst` is above 1.
    pub phobos_burst_no_delay: bool,
    /// Phobos `WeaponTypeExt::DelayedFire_Duration` (`Ext/WeaponType/Body.cpp:189`,
    /// `Nullable<PartialVector2D<int>>`, game frames). A key authored with no
    /// parsable field stays unset, which is what `Nullable::Read` leaves. The
    /// `isset` state also drives the `Burst.NoDelay` compatibility gate
    /// ([`Self::delayed_fire_allows_burst_repeat`]); the value itself is consumed
    /// by the `0x6FDDC0` gate (`src/Ext/Techno/Hooks.Firing.cpp:638-693`) as the
    /// delay timer's duration.
    pub(crate) phobos_delayed_fire_duration: Option<crate::rules::ini_value::PartialIntPair>,
    /// Phobos `WeaponTypeExt::DelayedFire_OnlyOnInitialBurst` (`Body.cpp:197`,
    /// `Valueable<bool>` declared at `Body.h:105`, default false at `Body.h:201`):
    /// the second half of that gate.
    pub(crate) phobos_delayed_fire_only_on_initial_burst: bool,
    /// The other nine `DelayedFire.*` keys, read by
    /// `WeaponTypeExt::LoadFromINIFile` (`src/Ext/WeaponType/Body.cpp:190-199`),
    /// declared at `Body.h:98-107` and defaulted at `Body.h:194-203`; serialized
    /// at `Body.cpp:299-309`. All are consumed by the `0x6FDDC0` gate
    /// (`src/Ext/Techno/Hooks.Firing.cpp:638-693`) and its animation helpers
    /// (`src/Ext/Techno/Body.cpp:658-697`).
    ///
    /// `DelayedFire_SkipInTransport` (`Body.cpp:190`, `Body.h:98`, default
    /// false): `TechnoClass_FireAt_BeforeTruelyFire @ 0x6FDDC0` suppresses the
    /// delay for a firer whose `TechnoClass+0x11C` Transporter is set.
    pub(crate) phobos_delayed_fire_skip_in_transport: bool,
    /// `DelayedFire_Animation` (`Body.cpp:191`, `Body.h:99`, `Valueable<AnimTypeClass*>`,
    /// default null): the wind-up anim. A blank value sets null; a non-type
    /// token such as `none` fails the native parse and keeps the prior value.
    /// VERA reads the merged section once from the constructor's null, so both
    /// spellings end at null here.
    pub(crate) phobos_delayed_fire_animation: Option<String>,
    /// `DelayedFire_OpenToppedAnimation` (`Body.cpp:192`, `Body.h:100`,
    /// `Nullable<AnimTypeClass*>`, default unset): an open-topped override.
    /// `none`/`<none>`/`<default>` clear the nullable (leave it unset), which
    /// the `0x6FDDC0` gate answers by falling back to `Animation`.
    pub(crate) phobos_delayed_fire_open_topped_animation: Option<String>,
    /// `DelayedFire_AnimIsAttached` (`Body.cpp:193`, `Body.h:101`, default
    /// true): `TechnoExt::CreateDelayedFireAnim` (`Body.cpp:671-697`) only
    /// calls `SetOwnerObject @ 0x00424B50` and registers
    /// `CurrentDelayedFireAnim` when this is set.
    pub(crate) phobos_delayed_fire_anim_is_attached: bool,
    /// `DelayedFire_CenterAnimOnFirer` (`Body.cpp:194`, `Body.h:102`, default
    /// false): `Body.cpp:677-680` short-circuits the FLH path with
    /// `GetCenterCoords()`.
    pub(crate) phobos_delayed_fire_center_anim_on_firer: bool,
    /// `DelayedFire_RemoveAnimOnNoDelay` (`Body.cpp:195`, `Body.h:103`, default
    /// false): `TechnoExt::ResetDelayedFireTimer @ 0x6FDDC0`
    /// (`Body.cpp:658-669`) force-destroys the tracked anim on reset; only
    /// stored on the AnimExt of an attached anim (`Body.cpp:691-695`), so it
    /// is inert when `AnimIsAttached` is false.
    pub(crate) phobos_delayed_fire_remove_anim_on_no_delay: bool,
    /// `DelayedFire_PauseFiringSequence` (`Body.cpp:196`, `Body.h:104`, default
    /// false): `0x6FDDC0` (`Hooks.Firing.cpp:658-662`) yields to
    /// `TechnoExt::HandleDelayedFireWithPauseSequence` (`Body.cpp:699-750`),
    /// whose `UnitClass_AI @ 0x736480` hook freezes the firing-sequence frame.
    pub(crate) phobos_delayed_fire_pause_firing_sequence: bool,
    /// `DelayedFire_AnimOffset` (`Body.cpp:198`, `Body.h:106`,
    /// `Nullable<CoordStruct>`, default unset): `Forward,Lateral,Height`
    /// **replaces** the weapon FLH at `Hooks.Firing.cpp:680-681`.
    pub(crate) phobos_delayed_fire_anim_offset: Option<[i32; 3]>,
    /// `DelayedFire_AnimOnTurret` (`Body.cpp:199`, `Body.h:107`, default true):
    /// passed as `isOnTurret` to `TechnoExt::GetFLHAbsoluteCoords`
    /// (`Body.Internal.cpp:80-89`).
    pub(crate) phobos_delayed_fire_anim_on_turret: bool,
    /// Phobos `WeaponTypeExt::CanTargetHouses` (`AffectedHouse::All` default),
    /// read for `Splits.UseWeaponTargeting`.
    pub(crate) phobos_can_target_houses: crate::rules::phobos_shields::AffectedHouse,
    /// Phobos `CanTarget`, `CanTarget.MinHealth`/`MaxHealth` and
    /// `CanTargetVeterancy`, read for `Splits.UseWeaponTargeting`.
    pub(crate) phobos_weapon_targeting: crate::rules::phobos_airburst::PhobosWeaponTargeting,
    /// Phobos `WeaponTypeExt::Laser_IsSingleColor` (`IsSingleColor=`): a
    /// simulated child laser draws one color instead of the inner/outer pair.
    pub phobos_laser_is_single_color: bool,
    /// Phobos `WeaponTypeExt::LaserThickness` (`LaserThickness=`,
    /// constructor default 3): a simulated child laser's line count.
    pub phobos_laser_thickness: i32,
    /// Phobos `WeaponTypeExt::LaserZAdjust` (`Ext/WeaponType/Body.cpp:130`,
    /// `Nullable<int>`, unset inherits `[AudioVisual] LaserZAdjust`, default
    /// 0): added to the LaserZap zAdjust (`0x6FD3FD`,
    /// `Misc/Hooks.LaserDraw.cpp:51-58`).
    pub phobos_laser_z_adjust: Option<i32>,
    /// Phobos `WeaponTypeExt::EBoltZAdjust` (`Ext/WeaponType/Body.cpp:131`,
    /// `Nullable<int>`, unset inherits `[AudioVisual] EBoltZAdjust`, default
    /// 0): added to the electric-bolt zAdjust (`Ext/EBolt/Hooks.cpp:215`).
    pub phobos_ebolt_z_adjust: Option<i32>,
    /// Phobos `EBoltZAdjust.ClampInitialDepthForBuilding=`
    /// (`Ext/WeaponType/Body.cpp:132`, `Nullable<bool>` unset at
    /// `Body.h:62,157`): the weapon's override of the
    /// `[AudioVisual] EBoltZAdjust.ClampInitialDepthForBuilding` clamp for a
    /// building's bolt.
    pub phobos_ebolt_clamp_initial_depth_for_building: Option<bool>,
    /// Phobos `WeaponTypeExt::Bolt_Duration` (`Ext/WeaponType/Body.cpp:101`,
    /// `Valueable<int>`, constructor `Body.h:131` stores 17): the EBolt
    /// lifetime exponent; `EBoltExt::CreateEBolt` stores
    /// `1 << (clamp(value, 1, 31) - 1)` (`Ext/EBolt/Body.cpp:30`).
    pub phobos_bolt_duration: i32,
    /// Phobos `WeaponTypeExt::Bolt_Arcs` (`Ext/WeaponType/Body.cpp:100`,
    /// `Valueable<int>`, constructor `Body.h` stores 8):
    /// `EBoltExt::CreateEBolt` copies it to the bolt (`Ext/EBolt/Body.cpp:29`)
    /// and `EBolt_DrawArcs` stops the subdivision at that plot index.
    pub phobos_bolt_arcs: i32,
    /// Phobos `WeaponTypeExt::Bolt_FollowFLH` (`Ext/WeaponType/Body.cpp:102`,
    /// `Nullable<bool>`): whether the bolt follows its firer's FLH; unset
    /// follows only a Unit (`Ext/EBolt/Hooks.cpp:113`).
    pub phobos_bolt_follow_flh: Option<bool>,
    /// Phobos `WeaponTypeExt::Beam_IsHouseColor` (`Beam.IsHouseColor=`): a
    /// simulated child radiation beam takes the firing House's laser color.
    pub phobos_beam_is_house_color: bool,
    /// Phobos `LaserPositionUpdate` (`Valueable<PositionFollow>`, default
    /// none): which ends of this weapon's lasers follow their live objects.
    pub laser_position_update: crate::rules::phobos_laser_tracking::PositionFollow,
    /// Phobos `LaserPositionUpdate.StopOnFirerConvert` (`Nullable<bool>`):
    /// stop following the firer once its TechnoType changes; `None` inherits
    /// `[AudioVisual]`.
    pub laser_position_update_stop_on_firer_convert: Option<bool>,
    /// Phobos `WeaponTypeExt` ExtraWarheads (`Ext/WeaponType/Body.cpp:139-166`):
    /// warheads detonated after this weapon's own at the same impact.
    pub phobos_extra_warheads: crate::rules::phobos_extra_warheads::PhobosExtraWarheads,
    /// Phobos `KeepRange=` (`Valueable<Leptons>`, `Ext/WeaponType/Body.cpp:179`):
    /// the distance a rearming Infantry/Unit keeps; positive is a minimum
    /// range, negative a maximum one.
    pub phobos_keep_range: i32,
    /// `KeepRange.AllowAI=` (`Nullable<bool>`): AI-house gate, else `[General]`.
    pub phobos_keep_range_allow_ai: Option<bool>,
    /// `KeepRange.AllowPlayer=` (`Nullable<bool>`): human-house gate.
    pub phobos_keep_range_allow_player: Option<bool>,
    /// `KeepRange.EarlyStopFrame=` (`Nullable<int>`): the rearm remainder
    /// below which KeepRange stops applying.
    pub phobos_keep_range_early_stop: Option<i32>,
    /// Phobos `WeaponTypeExt` strafing keys (`Ext/WeaponType/Body.cpp:108-112`,
    /// declarations `Body.h:40-44`): the round's shot count, burst simulation,
    /// per-shot ammo, Target-to-cell switch and end delay. Read by the
    /// `AircraftClass::Mission_Attack` hooks and `AircraftExt::FireWeapon`;
    /// unset booleans fall back to `[General] Strafing.*`.
    pub(crate) phobos_strafing: crate::rules::phobos_strafing::PhobosStrafingRules,
    /// Phobos `AmbientDamage.Warhead=` (`Nullable<WarheadTypeClass*>`,
    /// `Ext/WeaponType/Body.cpp:167`): the warhead the railgun and blast-wave
    /// ambient damage uses; unset keeps the weapon's own warhead.
    pub phobos_ambient_damage_warhead: Option<String>,
    /// Phobos `AmbientDamage.IgnoreTarget=` (`Nullable<bool>`): leave the
    /// primary target out of the ambient-damage list; unset uses the fixed
    /// `[General] AmbientDamage.IgnoreTarget=`.
    pub phobos_ambient_damage_ignore_target: Option<bool>,
    /// Phobos `CanTarget.IronCurtained=` (`Nullable<bool>`,
    /// `Ext/WeaponType/Body.cpp:118`): this weapon's override for shooting an
    /// iron-curtained target; `None` falls back to the human/AI
    /// `[CombatDamage]` global (`Ext/Techno/Hooks.Firing.cpp:367`).
    pub phobos_can_target_iron_curtained: Option<bool>,
    /// Phobos `AutoTarget.IronCurtained=` (`Nullable<bool>`, `:119`): this
    /// weapon's override for retaliating at or auto-acquiring an
    /// iron-curtained target (`Hooks.Firing.cpp:1246`,
    /// `Hooks.Targeting.cpp:63`).
    pub phobos_auto_target_iron_curtained: Option<bool>,
    /// Phobos `FeedbackWeapon`, `VisualScatter`,
    /// `AttackNoThreatBuildings`, `TurretRecoil.Suppress` and
    /// `ChargeTurret.Delays` (`Ext/WeaponType/Body.cpp:124`, `:133`, `:206`,
    /// `:138`, `:135`); see [`crate::rules::phobos_weapon_extras`].
    pub(crate) phobos_extras: crate::rules::phobos_weapon_extras::PhobosWeaponExtras,
    /// Phobos `IvanBomb.Visibility=` (`Nullable<AffectedHouse>`,
    /// `Ext/WeaponType/Body.cpp:103`): the per-weapon override of the
    /// `[AudioVisual] IvanIconVisibility` icon gate
    /// (`Ext/WeaponType/Hooks.Bomb.cpp:38-77`, hook `0x438D44`).
    pub phobos_ivan_bomb_visibility: Option<crate::rules::phobos_shields::AffectedHouse>,
    /// Phobos `CylinderRangefinding=` (`Nullable<bool>`,
    /// `Ext/WeaponType/Body.cpp:207`): this weapon's direct range is measured
    /// in the plan; an unset value falls back to `[General]`
    /// (`Ext/Techno/Hooks.Firing.cpp:1265-1274`).
    pub phobos_cylinder_rangefinding: Option<bool>,
    /// Phobos `WeaponTypeExt::KickOutPassengers` (`Nullable<bool>`, unset;
    /// `Ext/WeaponType/Body.cpp:183`, `Body.h:91,186`): with a passenger
    /// aboard, a firing aircraft still drops one (`AircraftClass::Fire_At`
    /// `0x415EEE`) only when this key, else
    /// `[General] AircraftWeapon.KickOutPassengers=`, is yes.
    pub phobos_kick_out_passengers: Option<bool>,
}

impl WeaponType {
    /// Phobos `FeedbackWeapon=` (`Ext/WeaponType/Body.cpp:124`): the weapon
    /// also detonated at the firer after a shot.
    pub fn phobos_feedback_weapon(&self) -> Option<&str> {
        self.phobos_extras.feedback_weapon.as_deref()
    }

    /// Phobos `VisualScatter=` (`:133`), default `no`.
    pub fn phobos_visual_scatter(&self) -> bool {
        self.phobos_extras.visual_scatter
    }

    /// Phobos `TurretRecoil.Suppress=` (`Ext/WeaponType/Body.cpp:138`).
    pub fn phobos_turret_recoil_suppress(&self) -> bool {
        self.phobos_extras.turret_recoil_suppress
    }

    /// Phobos `Anim.Update=` (`Ext/WeaponType/Body.cpp:208`) over
    /// `[AudioVisual] FiringAnim.Update=` (`Ext/Rules/Body.cpp:548`): a
    /// muzzle anim from this weapon's `Anim=` list keeps following its
    /// firer every frame (`Ext/Anim/Hooks.cpp:714-740`).
    pub fn phobos_anim_update(&self, rules: &crate::rules::ruleset::RuleSet) -> bool {
        self.phobos_extras
            .anim_update
            .unwrap_or(rules.phobos_globals.firing_anim_update)
    }

    /// Phobos `FireOnce.ResetSequence=` (`Ext/WeaponType/Body.cpp:137`).
    pub fn phobos_fire_once_reset_sequence(&self) -> bool {
        self.phobos_extras.fire_once_reset_sequence
    }

    /// Phobos `OmniFire.TurnToTarget=` resolved against `[General]`
    /// (`Ext/Unit/Hooks.Jumpjet.cpp:79`, hook `0x736E6E`).
    pub fn phobos_omni_fire_turn_to_target(&self, rules: &crate::rules::ruleset::RuleSet) -> bool {
        self.phobos_extras
            .omni_fire_turn_to_target
            .unwrap_or(rules.phobos_globals.omni_fire_turn_to_target)
    }

    /// Phobos `AreaFire.Target=` (`Ext/WeaponType/Body.cpp:123`).
    pub fn phobos_area_fire_target(&self) -> crate::rules::phobos_weapon_extras::AreaFireTarget {
        self.phobos_extras.area_fire_target
    }

    /// Phobos `AttackNoThreatBuildings=` (`:206`) resolved against the
    /// human/AI `[General]` pair.
    pub fn phobos_attack_no_threat_buildings(
        &self,
        rules: &crate::rules::ruleset::RuleSet,
        human: bool,
    ) -> bool {
        self.phobos_extras.attack_no_threat_buildings(rules, human)
    }

    /// The weapon's warhead and projectile as its RuleSet resolved them;
    /// reads go through `RuleSet::weapon_warhead`/`weapon_projectile`, which
    /// check the resolving set.
    #[inline]
    pub(crate) fn parts(&self) -> crate::rules::ruleset::WeaponParts {
        self.parts
    }

    /// Store the warhead and projectile resolved against the current set.
    pub(crate) fn project_parts(&mut self, parts: crate::rules::ruleset::WeaponParts) {
        self.parts = parts;
    }

    /// `FireAt`'s zeroing test (`0x006FE306..0x006FE32A`): an `IsSonic=` or
    /// `UseFireParticles=` weapon's shot carries no damage, its wave or
    /// particles hurt instead. Ares TechnoClass_Fire_AllowDamage (1004A370,
    /// hook 0x6FE31C) lets `ApplyDamage=` decide, defaulting to that test;
    /// a stock-way read leaves it unset.
    pub(crate) fn fire_damage_zeroed(&self) -> bool {
        let stock = self.is_sonic || self.use_fire_particles;
        !self.ares_apply_damage.unwrap_or(!stock)
    }

    /// `WeaponTypeExt::HasRequiredAttachedEffects` reads no list: every shot
    /// passes without asking the target.
    pub fn has_attach_effect_gates(&self) -> bool {
        !self.attach_effect_required_types.is_empty()
            || !self.attach_effect_disallowed_types.is_empty()
            || !self.attach_effect_required_groups.is_empty()
            || !self.attach_effect_disallowed_groups.is_empty()
    }

    /// Phobos `CanTargetHouses=`, default `AffectedHouse::All`.
    pub(crate) const fn can_target_houses(&self) -> crate::rules::phobos_shields::AffectedHouse {
        self.phobos_can_target_houses
    }

    /// Phobos `WeaponTypeExt`'s five `Strafing.*` keys.
    pub(crate) const fn phobos_strafing(
        &self,
    ) -> &crate::rules::phobos_strafing::PhobosStrafingRules {
        &self.phobos_strafing
    }

    /// Phobos `CanTarget`, health range and `CanTargetVeterancy`, defaults
    /// `All`/`0.0`/`1.0`/`All`.
    pub(crate) const fn weapon_targeting(
        &self,
    ) -> crate::rules::phobos_airburst::PhobosWeaponTargeting {
        self.phobos_weapon_targeting
    }

    /// Phobos `WeaponTypeExt::GetBurstDelay`
    /// (`Ext/WeaponType/Body.cpp:62-74`): burst index 0 answers 0 before the
    /// list is consulted; a list longer than the index answers entry
    /// `index - 1`; a shorter non-empty list answers its last entry; an empty
    /// list answers -1. The length compare casts the index to `unsigned`, so a
    /// negative index only matches the last-entry arm. The `0x6FD05E` hook
    /// takes the answer only when it is not negative
    /// (`Ext/Techno/Hooks.Firing.cpp:1158`), which is what `Some` mirrors.
    pub(crate) fn phobos_burst_delay(&self, burst_index: i32) -> Option<i32> {
        let delays = &self.phobos_burst_delays;
        let delay = if burst_index == 0 {
            0
        } else if delays.len() > burst_index as u32 as usize {
            delays[burst_index as usize - 1]
        } else if let Some(last) = delays.last() {
            *last
        } else {
            -1
        };
        (delay >= 0).then_some(delay)
    }

    /// The second clause both `Burst.NoDelay` hooks test alongside the key:
    /// `!DelayedFire_Duration.isset() || DelayedFire_OnlyOnInitialBurst`
    /// (`src/Ext/Unit/Hooks.Firing.cpp:125`,
    /// `src/Ext/Infantry/Hooks.Firing.cpp:189`).
    pub(crate) const fn delayed_fire_allows_burst_repeat(&self) -> bool {
        self.phobos_delayed_fire_duration.is_none()
            || self.phobos_delayed_fire_only_on_initial_burst
    }

    pub(crate) fn ammo_cost(&self) -> i32 {
        self.ares_ammo_cost
    }

    /// `Report.Count` (`WeaponTypeClass+0xCC`).
    pub fn report_count(&self) -> i32 {
        self.report.len() as i32
    }

    /// `Report.Items[index]`.
    pub fn report_item(&self, index: usize) -> Option<&str> {
        self.report.get(index).map(String::as_str)
    }

    /// Parse a WeaponType from a rules.ini section.
    ///
    /// Damage, Speed, Range, Projectile and Warhead keep the constructor's
    /// zero and null (`0x00771C70`): the ordered Rules processor owns those
    /// reads (Damage `0x007722E3`, Speed `0x007722FD`, Range `0x00772336`,
    /// Warhead `0x0077295F`, Projectile `0x00772998`) and the speed postpass
    /// (`0x007729F0`), and `RuleSet` builds each weapon with its retained
    /// values.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        // Phobos `WeaponTypeExt::RadType` read; absent (and every build
        // without the capability) binds the default radiation type.
        let rad_type = section.read_name("RadType", 0x80).map(str::to_owned);
        // `WeaponTypeExt::LoadFromINIFile`'s AttachEffect gates. A missing key
        // keeps the constructor's empty list / false.
        let (
            attach_effect_required_types,
            attach_effect_disallowed_types,
            attach_effect_required_groups,
            attach_effect_disallowed_groups,
        ) = (
            name_list(section, "AttachEffect.RequiredTypes"),
            name_list(section, "AttachEffect.DisallowedTypes"),
            name_list(section, "AttachEffect.RequiredGroups"),
            name_list(section, "AttachEffect.DisallowedGroups"),
        );
        let (
            attach_effect_required_min_counts,
            attach_effect_required_max_counts,
            attach_effect_disallowed_min_counts,
            attach_effect_disallowed_max_counts,
        ) = (
            section
                .read_int_list("AttachEffect.RequiredMinCounts")
                .unwrap_or_default(),
            section
                .read_int_list("AttachEffect.RequiredMaxCounts")
                .unwrap_or_default(),
            section
                .read_int_list("AttachEffect.DisallowedMinCounts")
                .unwrap_or_default(),
            section
                .read_int_list("AttachEffect.DisallowedMaxCounts")
                .unwrap_or_default(),
        );
        let attach_effect_check_on_firer = section.read_bool("AttachEffect.CheckOnFirer", false);
        let attach_effect_ignore_from_same_source =
            section.read_bool("AttachEffect.IgnoreFromSameSource", false);
        let (
            attach_effect_required_any,
            attach_effect_disallowed_any,
            attach_effect_required_houses,
            attach_effect_disallowed_houses,
            attach_effect_same_source_only,
        ) = (
            section.read_bool("AttachEffect.Required.Any", false),
            section.read_bool("AttachEffect.Disallowed.Any", true),
            section.read_string_with(
                "AttachEffect.Required.Houses",
                0x100,
                AffectedHouse::ALL,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            ),
            section.read_string_with(
                "AttachEffect.Disallowed.Houses",
                0x100,
                AffectedHouse::ALL,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            ),
            section.read_bool("AttachEffect.SameSourceOnly", false),
        );
        Self {
            id: id.to_string(),
            damage: 0,
            // `WeaponTypeClass::Constructor` 0x00771C70 zeroes +0xB4 at
            // 0x00771CB6.
            range_leptons: 0,
            rof: section.read_int("ROF", 0),
            speed: 0,
            projectile: None,
            warhead: None,
            parts: Default::default(),
            report: sound_list(section, "Report"),
            down_report: Vec::new(),
            burst: section.read_int("Burst", 1),
            // `ammo_cost` and `fire_damage_zeroed` run in every build, so
            // these two Ares reads happen only the Ares way.
            ares_ammo_cost: if crate::rules::ares_switch::ares_way() {
                section.read_ares_int("Ammo", 1)
            } else {
                1
            },
            ares_visuals: crate::rules::ares_weapon::AresWeaponVisuals::read(
                section,
                crate::rules::ares_switch::ares_read(|| section.read_bool("IsMagBeam", false)),
            ),
            ares_ivan_bomb: crate::rules::ares_weapon::AresIvanBomb::read(section),
            ares_apply_damage: if crate::rules::ares_switch::ares_way() {
                section.read_ares_nullable_bool("ApplyDamage", None)
            } else {
                None
            },
            ares_abductor: crate::rules::ares_weapon::AresAbductor::read(section),
            ares_cursors: Default::default(),
            ares_projectile_range: section.read_ares_leptons("ProjectileRange", 100000),
            // Constructor 1 (`0x00771DBB`); ReadINI passes it as the default
            // (`0x00772182..0x00772196`).
            reveal_on_fire: section.read_bool("RevealOnFire", true),
            // Constructor771DC2/771DC8/771DCE zero +138/+139/+13A;
            // ReadINI7721A3..7721E4 passes the retained bytes to the native
            // boolean reader5295F0.
            terrain_fire: section.read_bool("TerrainFire", false),
            disguise_fire_only: section.read_bool("DisguiseFireOnly", false),
            sabotage_cursor: section.read_bool("SabotageCursor", false),
            mig_attack_cursor: section.read_bool("MigAttackCursor", false),

            // Int fields
            ambient_damage: section.read_int("AmbientDamage", 0),
            // Constructor 0 (`0x00771DDA`); ReadINI passes it as the default
            // (`0x0077226C..0x00772280`).
            disguise_fake_blink_time: section.read_int("DisguiseFakeBlinkTime", 0),
            // +0xB8 is zeroed alongside +0xB4 at 0x00771CBF.
            minimum_range_leptons: section.read_range("MinimumRange", 0),
            // ReadINI stores only AL; SpawnLaser6FD21D sign-extends the
            // retained byte. ReadInt uses its default only for absent keys,
            // so narrowing after the projected pass history is equivalent.
            laser_duration: i32::from(section.read_int("LaserDuration", 10) as i8),
            rad_level: section.read_int("RadLevel", 0),
            rad_type,
            phobos_can_target_houses: crate::rules::phobos_shields::read_affected_house(
                section,
                "CanTargetHouses",
                crate::rules::phobos_shields::AffectedHouse::ALL,
            ),
            phobos_weapon_targeting: crate::rules::phobos_airburst::read_weapon_targeting(section),
            // Phobos `Valueable<T>` reads through the shared Ares parser: a
            // failed conversion keeps the constructor's value.
            phobos_laser_is_single_color: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_bool("IsSingleColor", false)
            }),
            phobos_laser_thickness: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_int("LaserThickness", 3)
            }),
            phobos_laser_z_adjust: section.read_phobos_nullable_int("LaserZAdjust", None),
            phobos_ebolt_z_adjust: section.read_phobos_nullable_int("EBoltZAdjust", None),
            phobos_ebolt_clamp_initial_depth_for_building: section
                .read_phobos_nullable_bool("EBoltZAdjust.ClampInitialDepthForBuilding", None),
            phobos_bolt_duration: section.read_int("Bolt.Duration", 17),
            phobos_bolt_arcs: section.read_int("Bolt.Arcs", 8),
            phobos_bolt_follow_flh: section.read_phobos_nullable_bool("Bolt.FollowFLH", None),
            phobos_beam_is_house_color: crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_bool("Beam.IsHouseColor", false)
            }),

            // The constructor's nulls and empty list (`0x00771C70`); the
            // ordered Process owner reads the Animation references.
            assault_anim: None,
            occupant_anim: None,
            open_topped_anim: None,
            attached_particle_system: None,
            anim: Vec::new(),

            // Color fields: ReadColorRGB over the constructor's zero
            // (`0x00771D30`-`0x00771D60`), at `0x00772770`, `0x00772796` and
            // `0x007727BC`.
            laser_inner_color: section.read_color_rgb("LaserInnerColor", [0; 3], None),
            laser_outer_color: section.read_color_rgb("LaserOuterColor", [0; 3], None),
            laser_outer_spread: section.read_color_rgb("LaserOuterSpread", [0; 3], None),

            // Bool fields
            use_fire_particles: section.read_bool("UseFireParticles", false),
            use_spark_particles: section.read_bool("UseSparkParticles", false),
            omni_fire: section.read_bool("OmniFire", false),
            distributed_weapon_fire: section.read_bool("DistributedWeaponFire", false),
            is_railgun: section.read_bool("IsRailgun", false),
            lobber: section.read_bool("Lobber", false),
            bright: section.read_bool("Bright", false),
            is_sonic: section.read_bool("IsSonic", false),
            spawner: section.read_bool("Spawner", false),
            limbo_launch: section.read_bool("LimboLaunch", false),
            // WeaponTypeClass constructor / ReadINI consumed by
            // TechnoClass::GetFireError @ 0x006FC0B0: omitted
            // DecloakToFire= is YES. Stock CruiseLauncher relies on the
            // constructor default; BoomerTorpedo explicitly opts out.
            decloak_to_fire: section.read_bool("DecloakToFire", true),
            cell_rangefinding: section.read_bool("CellRangefinding", false),
            fire_once: section.read_bool("FireOnce", false),
            never_use: section.read_bool("NeverUse", false),
            infinite_mind_control: section.read_bool("InfiniteMindControl", false),
            fire_while_moving: section.read_bool("FireWhileMoving", true),
            drain_weapon: section.read_bool("DrainWeapon", false),
            fire_in_transport: section.read_bool("FireInTransport", true),
            suicide: section.read_bool("Suicide", false),
            turbo_boost: section.read_bool("TurboBoost", false),
            supress: section.read_bool("Supress", false),
            camera: section.read_bool("Camera", false),
            charges: section.read_bool("Charges", false),
            is_laser: section.read_bool("IsLaser", false),
            disk_laser: section.read_bool("DiskLaser", false),
            phobos_disk_laser_radius: section.read_double("DiskLaser.Radius", 240.0),
            phobos_projectile_range_apply_modifiers: section
                .read_phobos_nullable_bool("ProjectileRange.ApplyModifiers", None),
            is_line: section.read_bool("IsLine", false),
            is_big_laser: section.read_bool("IsBigLaser", false),
            is_house_color: section.read_bool("IsHouseColor", false),
            ion_sensitive: section.read_bool("IonSensitive", false),
            area_fire: section.read_bool("AreaFire", false),
            is_electric_bolt: section.read_bool("IsElectricBolt", false),
            draw_bolt_as_laser: section.read_bool("DrawBoltAsLaser", false),
            is_alternate_color: section.read_bool("IsAlternateColor", false),
            is_rad_beam: section.read_bool("IsRadBeam", false),
            is_rad_eruption: section.read_bool("IsRadEruption", false),
            is_mag_beam: section.read_bool("IsMagBeam", false),

            attach_effect_required_types,
            attach_effect_disallowed_types,
            attach_effect_required_groups,
            attach_effect_disallowed_groups,
            attach_effect_required_min_counts,
            attach_effect_required_max_counts,
            attach_effect_disallowed_min_counts,
            attach_effect_disallowed_max_counts,
            attach_effect_check_on_firer,
            attach_effect_ignore_from_same_source,
            attach_effect_required_any,
            attach_effect_disallowed_any,
            attach_effect_required_houses,
            attach_effect_disallowed_houses,
            attach_effect_same_source_only,
            rof_random_delay: Some(crate::rules::ares_switch::not_ares_read(|| {
                section.read_ares_partial_int_pair(
                    "ROF.RandomDelay",
                    crate::rules::ini_value::PartialIntPair {
                        low: 0,
                        high: 0,
                        values: 0,
                    },
                )
            }))
            .filter(|range| range.values > 0),
            laser_position_update: section.read_string_with(
                "LaserPositionUpdate",
                2048,
                crate::rules::phobos_laser_tracking::PositionFollow::None,
                |current, raw| {
                    crate::rules::phobos_laser_tracking::PositionFollow::parse(raw)
                        .unwrap_or(current)
                },
            ),
            laser_position_update_stop_on_firer_convert: section
                .read_phobos_nullable_bool("LaserPositionUpdate.StopOnFirerConvert", None),
            phobos_extra_warheads:
                crate::rules::phobos_extra_warheads::PhobosExtraWarheads::from_section(section),
            phobos_keep_range: section.read_phobos_leptons("KeepRange", 0),
            phobos_keep_range_allow_ai: section
                .read_phobos_nullable_bool("KeepRange.AllowAI", None),
            phobos_keep_range_allow_player: section
                .read_phobos_nullable_bool("KeepRange.AllowPlayer", None),
            phobos_keep_range_early_stop: section
                .read_phobos_nullable_int("KeepRange.EarlyStopFrame", None),
            phobos_strafing: crate::rules::phobos_strafing::PhobosStrafingRules::from_section(
                section,
            ),
            phobos_ambient_damage_warhead: section.read_phobos_nullable_with(
                "AmbientDamage.Warhead",
                None,
                |raw| Some(raw.to_owned()),
            ),
            phobos_ambient_damage_ignore_target: section
                .read_phobos_nullable_bool("AmbientDamage.IgnoreTarget", None),
            phobos_can_target_iron_curtained: section
                .read_phobos_nullable_bool("CanTarget.IronCurtained", None),
            phobos_auto_target_iron_curtained: section
                .read_phobos_nullable_bool("AutoTarget.IronCurtained", None),
            phobos_extras: crate::rules::phobos_weapon_extras::PhobosWeaponExtras::from_section(
                section,
            ),
            phobos_ivan_bomb_visibility: section
                .read_name("IvanBomb.Visibility", 0x20)
                .and_then(crate::rules::phobos_shields::AffectedHouse::parse),
            phobos_cylinder_rangefinding: section
                .read_phobos_nullable_bool("CylinderRangefinding", None),
            phobos_kick_out_passengers: section
                .read_phobos_nullable_bool("KickOutPassengers", None),
            phobos_burst_delays: section
                .read_ares_int_list("Burst.Delays")
                .unwrap_or_default(),
            phobos_burst_fire_within_sequence: section
                .read_ares_bool("Burst.FireWithinSequence", false),
            phobos_burst_no_delay: section.read_ares_bool("Burst.NoDelay", false),
            phobos_delayed_fire_duration: {
                let unset = crate::rules::ini_value::PartialIntPair {
                    low: 0,
                    high: 0,
                    values: 0,
                };
                let read = section.read_ares_partial_int_pair("DelayedFire.Duration", unset);
                (read.values > 0).then_some(read)
            },
            phobos_delayed_fire_only_on_initial_burst: section
                .read_ares_bool("DelayedFire.OnlyOnInitialBurst", false),
            // The remaining `DelayedFire.*` reads (`Body.cpp:190-199`).
            // `Valueable<bool>` keeps the constructor default on an absent or
            // rejected read; `AnimIsAttached`/`AnimOnTurret` default true.
            phobos_delayed_fire_skip_in_transport: section
                .read_ares_bool("DelayedFire.SkipInTransport", false),
            // `Valueable<AnimTypeClass*>`: a present type name resolves, `none`
            // and `<none>` answer the factory's null (no `isset` flag to clear).
            phobos_delayed_fire_animation: section
                .read_type_name("DelayedFire.Animation", 0x100)
                .map(str::to_owned),
            // `Nullable<AnimTypeClass*>::Read<false, false>`: `<default>`,
            // `none` and `<none>` clear the nullable to "unset"; a resolved
            // name sets it. VERA models "unset" as `None` (fall back to
            // `Animation`) and a set name as `Some`.
            phobos_delayed_fire_open_topped_animation: section.read_phobos_nullable_with(
                "DelayedFire.OpenToppedAnimation",
                None,
                |raw| Some(raw.to_owned()),
            ),
            phobos_delayed_fire_anim_is_attached: section
                .read_ares_bool("DelayedFire.AnimIsAttached", true),
            phobos_delayed_fire_center_anim_on_firer: section
                .read_ares_bool("DelayedFire.CenterAnimOnFirer", false),
            phobos_delayed_fire_remove_anim_on_no_delay: section
                .read_ares_bool("DelayedFire.RemoveAnimOnNoDelay", false),
            phobos_delayed_fire_pause_firing_sequence: section
                .read_ares_bool("DelayedFire.PauseFiringSequence", false),
            // `Nullable<CoordStruct>`: a complete `Forward,Lateral,Height`
            // scan, the same reader as `weapon_offsets::nullable_flh`.
            phobos_delayed_fire_anim_offset: section
                .read_int_fields_complete::<3>("DelayedFire.AnimOffset", 2048),
            phobos_delayed_fire_anim_on_turret: section
                .read_ares_bool("DelayedFire.AnimOnTurret", true),
        }
    }
}

/// A Phobos name list: `ReadString(2048)`, `strtok(",")`, tokens kept as the
/// factory would resolve them. Absent keys leave the constructor's empty list.
fn name_list(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_list(key, 0x800)
        .map(|tokens| tokens.into_iter().map(str::to_string).collect())
        .unwrap_or_default()
}

/// `Report=` through ReadSoundList (`0x00772394`); production binding resolves
/// both report lists through the fixed SOUNDMD registry.
fn sound_list(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_sound_list(key)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    #[test]
    fn ares_projectile_range_reads_cells_as_leptons_and_apply_damage_decides_zeroing() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let read = |text: &str| {
            let file = IniFile::from_str(text);
            WeaponType::from_ini_section("W", file.section("W").unwrap())
        };
        let plain = read("[W]\nDamage=5\n");
        assert_eq!(plain.ares_projectile_range, 100000);
        assert!(!plain.fire_damage_zeroed());
        let sonic = read("[W]\nIsSonic=yes\nProjectileRange=1.5\nApplyDamage=yes\n");
        assert_eq!(sonic.ares_projectile_range, 384);
        assert!(!sonic.fire_damage_zeroed(), "ApplyDamage=yes keeps it");
        assert!(read("[W]\nApplyDamage=no\n").fire_damage_zeroed());
        assert_eq!(
            read("[W]\nProjectileRange=x\n").ares_projectile_range,
            100000
        );
    }

    #[test]
    fn weapon_ammo_cost_uses_ares_retained_integer_reader_only_when_enabled() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        for ares_way in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
            let mut layers = RulesLayerStack::new(IniFile::from_str(
                "[InfantryTypes]\n0=PROBE\n[PROBE]\nPrimary=PAIR\nSecondary=FREE\n[PAIR]\nAmmo=2\n[FREE]\nAmmo=0\n",
            ));
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str("[PAIR]\nAmmo=invalid\n"),
            );
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                rules.weapon("PAIR").unwrap().ammo_cost(),
                if ares_way { 2 } else { 1 }
            );
            assert_eq!(
                rules.weapon("FREE").unwrap().ammo_cost(),
                if ares_way { 0 } else { 1 }
            );
        }
    }

    /// Phobos `Burst.NoDelay`: the constructor's false default, then the key
    /// through the production reader, including a later pass that says no.
    #[test]
    fn burst_no_delay_reads_with_a_false_default() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let base = "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Slow\nSecondary=Fast\n\
                    [Slow]\nDamage=1\nBurst=3\n\
                    [Fast]\nDamage=1\nBurst=3\nBurst.NoDelay=yes\n";
        let rules = RuleSet::from_ini(&IniFile::from_str(base)).unwrap();
        assert!(
            !rules.weapon("Slow").unwrap().phobos_burst_no_delay,
            "the absent key keeps the constructor default"
        );
        assert!(
            rules.weapon("Fast").unwrap().phobos_burst_no_delay,
            "yes is read"
        );

        // A later rules pass can turn it on for a weapon the base left unset.
        let mut layers = RulesLayerStack::new(IniFile::from_str(base));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Slow]\nBurst.NoDelay=yes\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert!(rules.weapon("Slow").unwrap().phobos_burst_no_delay);

        // `no` is a value, not a cleared field: the pass that authored it reads
        // false.
        let mut layers = RulesLayerStack::new(IniFile::from_str(base));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Fast]\nBurst.NoDelay=no\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert!(!rules.weapon("Fast").unwrap().phobos_burst_no_delay);
    }

    /// Phobos `Burst.Delays=` (`Ext/WeaponType/Body.cpp:120`): a
    /// `ValueableVector<int>` read through the production layers. An absent key
    /// keeps the constructor's empty list, a present value replaces it, a token
    /// that does not convert is skipped, and a value with no usable token
    /// replaces with empty.
    #[test]
    fn burst_delays_reads_a_sparse_int_list() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let base = "[VehicleTypes]\n0=A\n1=B\n2=C\n\
                    [A]\nPrimary=Absent\nSecondary=Set\n\
                    [B]\nPrimary=Skip\nSecondary=Tokenless\n\
                    [C]\nPrimary=Empty\n\
                    [Absent]\nDamage=1\n\
                    [Set]\nDamage=1\nBurst.Delays=4,0\n\
                    [Skip]\nDamage=1\nBurst.Delays=4,x,0\n\
                    [Tokenless]\nDamage=1\nBurst.Delays=,,,\n\
                    [Empty]\nDamage=1\nBurst.Delays=\n";
        let rules = RuleSet::from_ini(&IniFile::from_str(base)).unwrap();
        assert_eq!(
            rules.weapon("Absent").unwrap().phobos_burst_delays,
            Vec::<i32>::new(),
            "the absent key keeps the constructor's empty list"
        );
        assert_eq!(rules.weapon("Set").unwrap().phobos_burst_delays, vec![4, 0]);
        assert_eq!(
            rules.weapon("Skip").unwrap().phobos_burst_delays,
            vec![4, 0],
            "a token that does not convert is skipped"
        );
        assert_eq!(
            rules.weapon("Tokenless").unwrap().phobos_burst_delays,
            Vec::<i32>::new(),
            "a present value with no usable token replaces with empty"
        );
        assert_eq!(
            rules.weapon("Empty").unwrap().phobos_burst_delays,
            Vec::<i32>::new(),
            "an empty value read keeps the constructor's empty list"
        );

        // A later pass that authors the key replaces the base list.
        let mut layers = RulesLayerStack::new(IniFile::from_str(base));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Set]\nBurst.Delays=7\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.weapon("Set").unwrap().phobos_burst_delays, vec![7]);
    }

    /// Retail `RevealOnFire=` through the production reader: the constructor's
    /// yes stands unless a section says no (the sniper and disguise weapons).
    #[test]
    fn retail_reveal_on_fire_defaults_to_yes() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        assert!(rules.weapon("M60").unwrap().reveal_on_fire);
        assert!(rules.weapon("105mm").unwrap().reveal_on_fire);
        assert!(!rules.weapon("AWP").unwrap().reveal_on_fire);
    }

    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn laser_corpus() -> &'static serde_json::Value {
        crate::test_fixture::json("tools/rules_oracle/weapon_laser.json")
    }

    fn laser_state(weapon: &WeaponType) -> serde_json::Value {
        serde_json::json!({
            "is_laser": weapon.is_laser,
            "is_house_color": weapon.is_house_color,
            "is_big_laser": weapon.is_big_laser,
            "duration": weapon.laser_duration,
            "inner": weapon.laser_inner_color,
            "outer": weapon.laser_outer_color,
            "spread": weapon.laser_outer_spread,
        })
    }

    fn cached_laser_sections(sections: &serde_json::Value) -> IniFile {
        let mut ini = IniFile::empty();
        for (name, keys) in sections.as_object().unwrap() {
            let mut section = IniSection::new(name.clone());
            for (key, value) in keys.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            ini.replace_first_section(section);
        }
        ini
    }

    #[test]
    fn laser_constructor_and_signed_duration_match_original_reader() {
        let native = laser_corpus();
        let controls = &native["controls"];
        let defaults = WeaponType::from_ini_section("LaserProbe", IniSection::empty());
        assert_eq!(laser_state(&defaults), controls["constructor"]);
        let rows = controls["duration_controls"].as_array().unwrap();
        assert_eq!(rows.len(), 24);
        for row in rows {
            let ini = cached_laser_sections(&row["sections"]);
            let weapon =
                WeaponType::from_ini_section("LaserProbe", ini.section("LaserProbe").unwrap());
            assert_eq!(laser_state(&weapon), row["state"], "{}", row["raw"]);
        }
    }

    #[test]
    fn laser_fields_retain_native_defaults_through_production_rules_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let native = laser_corpus();
        let rows = native["controls"]["retained_history"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=Tower\n[Tower]\nPrimary=LaserProbe\n",
        ));
        for (index, row) in rows.iter().enumerate() {
            // Preserve the native cache's explicit empty values; the physical
            // INI loader omits them, so this is a reader-component control.
            layers.push(
                RulesLayerKind::Scenario,
                cached_laser_sections(&row["sections"]),
            );
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                laser_state(rules.weapon("LaserProbe").unwrap()),
                row["state"],
                "pass {index}"
            );
        }
    }

    #[test]
    fn retail_prism_laser_fields_match_original_layered_readers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let native = laser_corpus();
        let rows = native["physical_prism"].as_array().unwrap();
        let mut layers = RulesLayerStack::new(ini);
        for row in rows {
            if row["absent"] == true {
                continue;
            }
            if row["file"] != "RULESMD.INI" {
                layers.push(
                    RulesLayerKind::Scenario,
                    cached_laser_sections(&row["sections"]),
                );
            }
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                laser_state(rules.weapon("PrismShot").unwrap()),
                row["state"],
                "{}",
                row["file"]
            );
        }
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .general
                .prism_support
                .duration,
            15
        );
    }

    #[test]
    fn retail_prism_laser_and_detail_use_complete_production_source_chain() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let native = laser_corpus();
        assert_eq!(
            laser_state(retail.rules.weapon("PrismShot").unwrap()),
            native["physical_prism"].as_array().unwrap().last().unwrap()["state"]
        );
        let detail = retail.rules.general.detail;
        assert_eq!(
            serde_json::json!({
                "min_frame_rate_normal": detail.min_frame_rate_normal,
                "min_frame_rate_movie": detail.min_frame_rate_movie,
                "buffer_zone_width": detail.buffer_zone_width,
            }),
            native["physical_detail"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["state"]
        );
        assert_eq!(retail.rules.general.prism_support.duration, 15);
    }

    #[test]
    fn retained_laser_fields_participate_in_configuration_identity() {
        use crate::rules::native_processing::RulesLayerStack;
        use crate::rules::ruleset::RuleSet;

        let common = RulesLayerStack::new(IniFile::from_str(
            "[LaserProbe]\nIsLaser=junk\nIsHouseColor=junk\nIsBigLaser=junk\n",
        ));
        let mut identities = Vec::new();
        for initial in ["yes", "no"] {
            let first = RulesLayerStack::new(IniFile::from_str(&format!(
                "[BuildingTypes]\n0=Tower\n[Tower]\nPrimary=LaserProbe\n\
                 [LaserProbe]\nIsLaser={initial}\nIsHouseColor={initial}\nIsBigLaser={initial}\n",
            )))
            .process()
            .unwrap();
            let (_, trace) = first.into_ini_and_native_type_construction_trace();
            let processed = common
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            assert_eq!(
                rules.weapon("LaserProbe").unwrap().is_laser,
                initial == "yes"
            );
            identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
        }
        assert_eq!(
            identities[0].0, identities[1].0,
            "same current rules source"
        );
        assert_ne!(
            identities[0].1, identities[1].1,
            "different retained laser admission"
        );
    }

    /// Phobos `WeaponTypeExt` AttachEffect gates: the constructor's empty
    /// lists and false switches, then every key through the production reader.
    #[test]
    fn attach_effect_gate_keys_read_with_their_defaults() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Plain\nSecondary=Gated\n\
             [Plain]\nDamage=1\n\
             [Gated]\nDamage=1\nAttachEffect.RequiredTypes=AE1,AE2\n\
             AttachEffect.DisallowedTypes=AE3\n\
             AttachEffect.RequiredGroups=tag\nAttachEffect.DisallowedGroups=bad\n\
             AttachEffect.RequiredMinCounts=2,3\nAttachEffect.RequiredMaxCounts=4\n\
             AttachEffect.DisallowedMinCounts=1\nAttachEffect.DisallowedMaxCounts=5,6\n\
             AttachEffect.CheckOnFirer=yes\nAttachEffect.IgnoreFromSameSource=yes\n\
             AttachEffect.Required.Any=yes\nAttachEffect.Required.Houses=enemies\n\
             AttachEffect.Disallowed.Any=no\nAttachEffect.Disallowed.Houses=allies\n\
             AttachEffect.SameSourceOnly=yes\n\
             [AttachEffectTypes]\n0=AE1\n1=AE2\n2=AE3\n\
             [AE1]\n[AE2]\n[AE3]\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Gated]\nAttachEffect.RequiredGroups=other\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let plain = rules.weapon("Plain").unwrap();
        assert!(!plain.has_attach_effect_gates());
        assert!(plain.attach_effect_required_types.is_empty());
        assert!(plain.attach_effect_disallowed_types.is_empty());
        assert!(plain.attach_effect_required_groups.is_empty());
        assert!(plain.attach_effect_disallowed_groups.is_empty());
        assert!(plain.attach_effect_required_min_counts.is_empty());
        assert!(plain.attach_effect_required_max_counts.is_empty());
        assert!(plain.attach_effect_disallowed_min_counts.is_empty());
        assert!(plain.attach_effect_disallowed_max_counts.is_empty());
        assert!(!plain.attach_effect_check_on_firer);
        assert!(!plain.attach_effect_ignore_from_same_source);
        // `AEAttachInfoTypeClass`'s `requireAll = !Any` defaults.
        assert!(!plain.attach_effect_required_any);
        assert!(plain.attach_effect_disallowed_any);
        assert_eq!(
            plain.attach_effect_required_houses,
            crate::rules::phobos_shields::AffectedHouse::ALL
        );
        assert_eq!(
            plain.attach_effect_disallowed_houses,
            crate::rules::phobos_shields::AffectedHouse::ALL
        );
        assert!(!plain.attach_effect_same_source_only);

        let gated = rules.weapon("Gated").unwrap();
        assert!(gated.has_attach_effect_gates());
        assert_eq!(gated.attach_effect_required_types, vec!["AE1", "AE2"]);
        assert_eq!(gated.attach_effect_disallowed_types, vec!["AE3"]);
        assert_eq!(
            gated.attach_effect_required_groups,
            vec!["other"],
            "the later pass replaces the list"
        );
        assert_eq!(gated.attach_effect_disallowed_groups, vec!["bad"]);
        assert_eq!(gated.attach_effect_required_min_counts, vec![2, 3]);
        assert_eq!(gated.attach_effect_required_max_counts, vec![4]);
        assert_eq!(gated.attach_effect_disallowed_min_counts, vec![1]);
        assert_eq!(gated.attach_effect_disallowed_max_counts, vec![5, 6]);
        assert!(gated.attach_effect_check_on_firer);
        assert!(gated.attach_effect_ignore_from_same_source);
        assert!(gated.attach_effect_required_any);
        assert!(!gated.attach_effect_disallowed_any);
        assert!(
            gated
                .attach_effect_required_houses
                .contains(crate::rules::phobos_shields::AffectedHouse::ENEMIES)
        );
        assert!(
            gated
                .attach_effect_disallowed_houses
                .contains(crate::rules::phobos_shields::AffectedHouse::ALLIES)
        );
        assert!(gated.attach_effect_same_source_only);
    }

    #[test]
    fn test_parse_weapon() {
        // Damage, Speed, Range, Projectile and Warhead come from the ordered
        // Rules owner. A nonzero ROT keeps the ballistic postpass off
        // ReadSpeed's value.
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nPrimary=105mm\n\
             [105mm]\nDamage=65\nROF=50\nRange=5.75\nSpeed=40\n\
             Projectile=InvisibleLow\nWarhead=AP\n\
             [InvisibleLow]\nROT=1\n",
        ))
        .unwrap();
        let weapon = rules.weapon("105mm").unwrap();

        assert_eq!(weapon.id, "105mm");
        assert_eq!(weapon.damage, 65);
        assert_eq!(weapon.range_leptons, 1472, "5.75 cells * 256");
        assert_eq!(weapon.rof, 50);
        assert_eq!(weapon.speed, 102);
        assert_eq!(weapon.projectile, Some("InvisibleLow".to_string()));
        assert_eq!(weapon.warhead, Some("AP".to_string()));
    }

    #[test]
    fn test_weapon_defaults() {
        let ini: IniFile = IniFile::from_str("[Empty]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("Empty").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("Empty", section);

        assert_eq!(weapon.damage, 0);
        assert_eq!(weapon.range_leptons, 0);
        assert_eq!(weapon.rof, 0);
        assert_eq!(weapon.projectile, None);
        assert_eq!(weapon.warhead, None);
        // New fields should all default correctly
        assert_eq!(weapon.ambient_damage, 0);
        assert_eq!(weapon.laser_duration, 10);
        assert_eq!(weapon.rad_level, 0);
        assert!(!weapon.is_sonic);
        assert!(!weapon.is_laser);
        assert!(!weapon.is_electric_bolt);
        assert!(!weapon.distributed_weapon_fire);
        assert!(!weapon.disguise_fire_only);
        assert!(weapon.anim.is_empty());
        assert_eq!(weapon.laser_inner_color, [0, 0, 0]);
        assert!(weapon.decloak_to_fire, "native omitted-key default is yes");
    }

    #[test]
    fn retail_battle_retains_native_weapon_reference_fields() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let akm = retail.rules.weapon("AKM").unwrap();
        assert_eq!(akm.assault_anim.as_deref(), Some("UCBLOOD"));
        assert!(akm.down_report.is_empty(), "constructor list is empty");

        let electric_bolt = retail.rules.weapon("CRElectricBolt").unwrap();
        assert_eq!(electric_bolt.assault_anim.as_deref(), Some("UCELEC"));
        assert!(
            electric_bolt.down_report.is_empty(),
            "stock has no DownReport"
        );
    }

    #[test]
    fn assault_anim_keeps_current_on_empty_and_uses_the_native_factory() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=KEEP_MISSING\n1=KEEP_EMPTY\n2=CLEAR\n3=CREATE\n\
             [KEEP_MISSING]\nPrimary=KeepMissing\n[KEEP_EMPTY]\nPrimary=KeepEmpty\n\
             [CLEAR]\nPrimary=Clear\n[CREATE]\nPrimary=Create\n\
             [KeepMissing]\nDamage=1\nAssaultAnim=UCFLASH\n\
             [KeepEmpty]\nDamage=1\nAssaultAnim=UCFLASH\n\
             [Clear]\nDamage=1\nAssaultAnim=UCFLASH\n\
             [Create]\nDamage=1\nAssaultAnim=UCFLASH\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[KEEP_MISSING]\nSecondary=KeepMissing\n\
                 [KeepMissing]\nDamage=1\n\
                 [KeepEmpty]\nAssaultAnim=\n\
                 [Clear]\nAssaultAnim=none\n\
                 [Create]\nAssaultAnim=LateAnim\n",
            ),
        );
        let processed = layers.process().unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();

        assert_eq!(
            rules.weapon("KeepMissing").unwrap().assault_anim.as_deref(),
            Some("UCFLASH"),
            "a later missing key retains the current animation"
        );
        assert_eq!(
            rules.weapon("KeepEmpty").unwrap().assault_anim.as_deref(),
            Some("UCFLASH"),
            "an empty ReadString result retains the current animation"
        );
        assert_eq!(rules.weapon("Clear").unwrap().assault_anim, None);
        assert_eq!(
            rules.weapon("Create").unwrap().assault_anim.as_deref(),
            Some("LateAnim"),
            "the native factory allocates an unknown nonempty name"
        );
    }

    #[test]
    fn weapon_report_lists_use_the_sound_registry_filter_and_native_reset_rules() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        use crate::rules::sound_ini::SoundRegistry;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Filtered\nSecondary=Cleared\n\
             [Filtered]\nDamage=1\nReport=A,Unknown,<none>,B,A\n\
             DownReport=A,Unknown,<none>,B,A\n\
             [Cleared]\nDamage=1\nReport=A,B\nDownReport=A,B\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Filtered]\nReport=\nDownReport=\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Cleared]\nReport=Unknown,<none>\nDownReport=,,,\n"),
        );

        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        let sounds = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=A\n1=B\n[A]\nSounds=a.wav\n[B]\nSounds=b.wav\n",
        ));
        rules.bind_type_sound_references(processed.ini(), &sounds);

        let filtered = rules.weapon("Filtered").unwrap();
        assert_eq!(
            filtered
                .report
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["A", "B", "A"],
            "registered sounds retain native order and duplicate entries"
        );
        assert_eq!(
            filtered
                .down_report
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["A", "B", "A"]
        );
        assert_eq!(filtered.report_count(), 3);

        let cleared = rules.weapon("Cleared").unwrap();
        assert_eq!(
            cleared.report_count(),
            0,
            "unknown-only replaces with empty"
        );
        assert!(
            cleared.down_report.is_empty(),
            "comma-only replaces with empty"
        );
    }

    #[test]
    fn weapon_fire_flags_keep_read_bool_defaults_across_rules_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Probe\nSecondary=Defaults\n\
             [Probe]\nDistributedWeaponFire=yes\nDisguiseFireOnly=yes\n\
             [Defaults]\nDamage=1\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Probe]\nDistributedWeaponFire=on\nDisguiseFireOnly=no\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Probe]\nDisguiseFireOnly=on\n"),
        );

        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let probe = rules.weapon("Probe").unwrap();
        assert!(
            probe.distributed_weapon_fire,
            "invalid 'on' keeps the current true value"
        );
        assert!(
            !probe.disguise_fire_only,
            "valid no followed by invalid 'on' keeps the current false value"
        );
        let defaults = rules.weapon("Defaults").unwrap();
        assert!(!defaults.distributed_weapon_fire);
        assert!(!defaults.disguise_fire_only);
    }

    #[test]
    fn weapon_fire_flags_retail_battle_defaults_and_map_overrides() {
        let Some(stock) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        for id in ["105mm", "MirageGun", "MirageGunE"] {
            let weapon = stock.rules.weapon(id).expect("stock WeaponType");
            assert!(!weapon.distributed_weapon_fire, "{id}");
            assert!(!weapon.disguise_fire_only, "{id}");
        }

        let patch = IniFile::from_str("[105mm]\nDistributedWeaponFire=yes\nDisguiseFireOnly=yes\n");
        let Some(patched) =
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
        else {
            return;
        };
        let weapon = patched.rules.weapon("105mm").unwrap();
        assert!(weapon.distributed_weapon_fire);
        assert!(weapon.disguise_fire_only);
    }

    /// `CCINIClass::ReadRange` 0x00474620 multiplies by `256.0` and THEN chops,
    /// so a fractional `Range=` keeps its fraction. 60 of the 256 stock
    /// `Range=` weapons carry one and 21 sit at exactly `1.5`, the stock
    /// `[BlimpBomb]` among them. Its `-1` sentinel (`0x0047463E`) returns the
    /// default, the field's current value: the 0x00771CB6 constructor zero, or
    /// what an earlier reached pass read.
    #[test]
    fn fractional_range_keeps_its_fraction_in_leptons() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=A\n1=B\n2=C\n3=D\n4=E\n5=F\n\
             [A]\nPrimary=BlimpBomb\n[B]\nPrimary=Nuke\n[C]\nPrimary=Quarter\n\
             [D]\nPrimary=NoKey\n[E]\nPrimary=Unset\n[F]\nPrimary=Kept\n\
             [BlimpBomb]\nDamage=250\nROF=50\nRange=1.5\nCellRangefinding=yes\n\
             [Nuke]\nRange=-2\n\
             [Quarter]\nRange=0.25\n\
             [NoKey]\nDamage=1\n\
             [Unset]\nRange=-1\n\
             [Kept]\nRange=3\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Kept]\nRange=-1\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let range = |id: &str| rules.weapon(id).unwrap().range_leptons;

        assert_eq!(range("BlimpBomb"), 384, "1.5 cells is 384 leptons, not 256");
        assert!(rules.weapon("BlimpBomb").unwrap().cell_rangefinding);
        assert_eq!(
            range("Nuke"),
            -512,
            "the -2 'infinite' sentinel scales to the -512 that 0x006F724E tests"
        );
        assert_eq!(range("Quarter"), 64);
        assert_eq!(range("NoKey"), 0, "absent key keeps the constructor zero");
        assert_eq!(rules.weapon("NoKey").unwrap().minimum_range_leptons, 0);
        assert_eq!(range("Unset"), 0, "-1 keeps the constructor zero");
        assert_eq!(range("Kept"), 3 * 256, "-1 keeps the earlier pass's range");
    }

    /// Phobos `CanTarget.IronCurtained`/`AutoTarget.IronCurtained` on a
    /// weapon (`Nullable<bool>`) and their `[CombatDamage]` fallbacks
    /// (`Ext/WeaponType/Body.cpp:118-119`, `Ext/Rules/Body.cpp:437-439`).
    #[test]
    fn iron_curtained_targeting_keys_read_with_their_fallbacks() {
        use crate::rules::ruleset::RuleSet;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n\
             [UNIT]\nStrength=100\nPrimary=Plain\nSecondary=Locked\n\
             [Plain]\nDamage=1\n\
             [Locked]\nDamage=1\nCanTarget.IronCurtained=no\nAutoTarget.IronCurtained=yes\n\
             [CombatDamage]\nCanTarget.IronCurtained=no\nAutoTarget.IronCurtained=no\n",
        ))
        .expect("curtain targeting rules");
        assert!(!rules.phobos_globals.can_target_iron_curtained);
        assert!(
            !rules.phobos_globals.can_target_ai_iron_curtained,
            "the absent AI global keeps the stock refusal"
        );
        assert!(!rules.phobos_globals.auto_target_iron_curtained);
        let plain = rules.weapon("Plain").unwrap();
        assert_eq!(plain.phobos_can_target_iron_curtained, None);
        assert_eq!(plain.phobos_auto_target_iron_curtained, None);
        let locked = rules.weapon("Locked").unwrap();
        assert_eq!(locked.phobos_can_target_iron_curtained, Some(false));
        assert_eq!(locked.phobos_auto_target_iron_curtained, Some(true));

        let defaults = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nStrength=100\nPrimary=Gun\n[Gun]\nDamage=1\n",
        ))
        .expect("default curtain rules");
        assert!(defaults.phobos_globals.can_target_iron_curtained);
        assert!(!defaults.phobos_globals.can_target_ai_iron_curtained);
        assert!(defaults.phobos_globals.auto_target_iron_curtained);
    }

    /// Phobos `AmbientDamage.Warhead` / `.IgnoreTarget` on a weapon and the
    /// `[General] AmbientDamage.IgnoreTarget` fallback
    /// (`Ext/WeaponType/Body.cpp:167`, `Ext/Rules/Body.cpp:268`).
    #[test]
    fn ambient_damage_keys_read_with_their_fallbacks() {
        use crate::rules::ruleset::RuleSet;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n\
             [UNIT]\nStrength=100\nPrimary=Gun\n\
             [Gun]\nDamage=1\nAmbientDamage=10\n\
             AmbientDamage.Warhead=AltWH\nAmbientDamage.IgnoreTarget=no\n\
             [Warheads]\n0=WH\n1=AltWH\n[WH]\n[AltWH]\n\
             [General]\nAmbientDamage.IgnoreTarget=yes\n",
        ))
        .expect("ambient damage rules");
        let gun = rules.weapon("Gun").unwrap();
        assert_eq!(gun.phobos_ambient_damage_warhead.as_deref(), Some("AltWH"));
        assert_eq!(gun.phobos_ambient_damage_ignore_target, Some(false));
        assert!(rules.phobos_globals.ambient_damage_ignore_target);

        let defaults = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nStrength=100\nPrimary=Gun\n[Gun]\nDamage=1\n",
        ))
        .expect("default ambient rules");
        let gun = defaults.weapon("Gun").unwrap();
        assert_eq!(gun.phobos_ambient_damage_warhead, None);
        assert_eq!(gun.phobos_ambient_damage_ignore_target, None);
        assert!(!defaults.phobos_globals.ambient_damage_ignore_target);
    }
}
