//! Phobos `DroppodTypeClass` (`src/New/Type/Affiliated/DroppodTypeClass.cpp:7-21`):
//! the twelve per-TechnoType drop-pod keys that shape the descent of a Foot
//! riding a drop pod.
//!
//! The reader runs from `TechnoTypeExt::LoadFromINI` for every non-building
//! TechnoType (`Ext/TechnoType/Body.cpp:1430-1445`) in the section's own
//! order: Angle, AtmosphereEntry, GroundAnim, AirImage, Height, Puff, Speed,
//! Trailer, Trailer.SpawnDelay, Trailer.Attached, Weapon (a
//! `Nullable<WeaponTypeClass*>` read with `Read<true>`, so its weapon is
//! constructed on the rules pass) and Weapon.HitLandOnly.
//!
//! Consumers: `DroppodLocomotionClass_ILoco_Process` (hook `0x4B5B70`) and
//! `_ILoco_MoveTo` (hook `0x4B607D`), both in
//! [`crate::sim::superweapon::drop_pod`]. Every unset key falls back to the
//! stock `RulesClass` block the same hook reads (`DropPodHeight`,
//! `DropPodSpeed`, `DropPodAngle`, `DropPod`, `DropPodPuff`,
//! `AtmosphereEntry`, `DropPodWeapon`) or to Ares' `[General]
//! DropPodTrailer=` then `SMOKEY`. A blank, `none`, `<none>` or `<default>`
//! value resets a `Nullable` back to unset, so it falls back to the global as
//! well; only a value that names a type overrides it.
//!
//! `DropPod.AirImage` is the infantry's pod SHP; drawing it is presentation
//! and stays out of the simulation (recorded as a residual in the doc page).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::is_native_none_type_name;
use crate::util::native_x87::NativeF64Bits;

/// The `DroppodTypeClass` fields, each `None` when the key is absent or clears
/// the `Nullable` (the value then comes from the stock globals).
#[derive(Debug, Clone, PartialEq)]
pub struct PhobosDropPodRules {
    /// `DropPod.Angle=`: `Nullable<double>`, the descent angle in radians;
    /// default `[General] DropPodAngle`.
    pub angle: Option<NativeF64Bits>,
    /// `DropPod.AtmosphereEntry=`: `Nullable<AnimTypeClass*>`, the entry anim;
    /// default `[AudioVisual] AtmosphereEntry`.
    pub atmosphere_entry: Option<String>,
    /// `DropPod.GroundAnim=`: `ValueableVector<AnimTypeClass*>`; item 0 is the
    /// landed pod's anim and item 1 an out-of-map pod's (a single item serves
    /// both). An empty or absent list keeps `[General] DropPod=`.
    pub ground_anims: Option<[String; 2]>,
    /// `DropPod.AirImage=`: `Nullable<SHPStruct*>`, the pod's SHP; default
    /// `POD.SHP`. Presentation only.
    pub air_image: Option<String>,
    /// `DropPod.Height=`: `Nullable<int>`, the entry height; default
    /// `[General] DropPodHeight`.
    pub height: Option<i32>,
    /// `DropPod.Puff=`: `Nullable<AnimTypeClass*>`; default `[AudioVisual]
    /// DropPodPuff`.
    pub puff: Option<String>,
    /// `DropPod.Speed=`: `Nullable<int>`; default `[General] DropPodSpeed`.
    pub speed: Option<i32>,
    /// `DropPod.Trailer=`: `Nullable<AnimTypeClass*>`; default Ares'
    /// `[General] DropPodTrailer=`, itself defaulting to `SMOKEY`.
    pub trailer: Option<String>,
    /// `DropPod.Trailer.SpawnDelay=`: `Valueable<int>`, constructor 6.
    pub trailer_spawn_delay: i32,
    /// `DropPod.Trailer.Attached=`: `Valueable<bool>`, constructor false.
    pub trailer_attached: bool,
    /// `DropPod.Weapon=`: `Nullable<WeaponTypeClass*>` `Read<true>`; default
    /// `[General] DropPodWeapon`.
    pub weapon: Option<String>,
    /// `DropPod.Weapon.HitLandOnly=`: `Valueable<bool>`, constructor false.
    /// False strafes the destination in the air, true detonates the weapon at
    /// the landing point instead.
    pub weapon_hit_land_only: bool,
}

impl Default for PhobosDropPodRules {
    fn default() -> Self {
        Self {
            angle: None,
            atmosphere_entry: None,
            ground_anims: None,
            air_image: None,
            height: None,
            puff: None,
            speed: None,
            trailer: None,
            trailer_spawn_delay: 6,
            trailer_attached: false,
            weapon: None,
            weapon_hit_land_only: false,
        }
    }
}

impl PhobosDropPodRules {
    /// `DroppodTypeClass::LoadFromINI` in the native key order.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            angle: section
                .read_phobos_nullable_double("DropPod.Angle", None)
                .map(|angle| NativeF64Bits::from_bits(angle.to_bits())),
            atmosphere_entry: nullable_name(section, "DropPod.AtmosphereEntry"),
            ground_anims: ground_anims(section),
            air_image: nullable_name(section, "DropPod.AirImage"),
            height: section.read_phobos_nullable_int("DropPod.Height", None),
            puff: nullable_name(section, "DropPod.Puff"),
            speed: section.read_phobos_nullable_int("DropPod.Speed", None),
            trailer: nullable_name(section, "DropPod.Trailer"),
            trailer_spawn_delay: section.read_ares_int("DropPod.Trailer.SpawnDelay", 6),
            trailer_attached: section.read_bool("DropPod.Trailer.Attached", false),
            weapon: nullable_name(section, "DropPod.Weapon"),
            weapon_hit_land_only: section.read_bool("DropPod.Weapon.HitLandOnly", false),
        }
    }
}

/// `Nullable<T*>::Read<false>`: a present `<default>`, blank, `none` or
/// `<none>` resets the field to unset (`Reset`, `HasValue = false`), and any
/// other token is the `Find` name. An unknown name leaves the `Nullable`
/// unset natively; the consumer skips names with no registered type.
fn nullable_name(section: &IniSection, key: &str) -> Option<String> {
    let raw = section.read_name(key, 2048)?;
    if raw.eq_ignore_ascii_case("<default>") || is_native_none_type_name(raw) {
        return None;
    }
    Some(raw.to_owned())
}

/// `ValueableVector<AnimTypeClass*>`: a reached non-empty list replaces the
/// field (empty keeps the global); item 0 is the landing anim and item 1 the
/// out-of-map anim, a one-item list filling both (`anims[anims.size() > 1]`).
fn ground_anims(section: &IniSection) -> Option<[String; 2]> {
    let anims: Vec<String> = section
        .read_list("DropPod.GroundAnim", 2048)?
        .into_iter()
        .map(str::trim)
        .filter(|token| !token.is_empty() && !is_native_none_type_name(token))
        .map(str::to_owned)
        .collect();
    let first = anims.first()?.clone();
    let second = anims.get(1).cloned().unwrap_or_else(|| first.clone());
    Some([first, second])
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_droppod_tests.rs"]
mod tests;
