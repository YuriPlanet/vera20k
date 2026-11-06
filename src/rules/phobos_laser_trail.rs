//! Phobos `LaserTrailTypeClass`: the `[LaserTrailTypes]` catalog read from
//! the ART layer. Native identity: `LaserTrailTypeClass::LoadFromINI`
//! (`src/New/Type/LaserTrailTypeClass.cpp:9..50`), registered from
//! `RulesExt::LoadFromINI` over `CCINIClass::INI_Art`
//! (`src/Ext/Rules/Body.cpp:62`). A trail type names how one trail segment is
//! drawn; the trail itself is owned by the AttachEffect instance that names it.

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::IniSection;
use crate::util::lookup_hash::LookupMap;

/// `LaserTrailDrawType`: which drawing system one segment uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LaserTrailDrawType {
    #[default]
    Laser,
    EBolt,
    RadBeam,
}

/// One `[LaserTrailTypes]` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LaserTrailType {
    pub draw_type: LaserTrailDrawType,
    pub is_house_color: bool,
    /// `Color` (`R,G,B`, 0..=255), default red.
    pub color: [u8; 3],
    /// `Thickness`, default 4.
    pub thickness: i32,
    pub is_alternate_color: bool,
    /// `Bolt.Color1..3`; `None` keeps the EBolt default for that line.
    pub bolt_color: [Option<[u8; 3]>; 3],
    /// `Bolt.Disable1..3`.
    pub bolt_disable: [bool; 3],
    /// `Bolt.Arcs`, default 8.
    pub bolt_arcs: i32,
    /// `Bolt.ZAdjust`, default 0.
    pub bolt_z_adjust: i32,
    /// `Beam.Color`; `None` keeps `[Radiation] Color=`.
    pub beam_color: Option<[u8; 3]>,
    /// `Beam.Amplitude`, default 40.0.
    pub beam_amplitude: f64,
    /// `FadeDuration`; `None` picks the draw type's default at use.
    pub fade_duration: Option<i32>,
    /// `SegmentLength`, default 128.
    pub segment_length: i32,
    pub ignore_vertical: bool,
    pub is_intense: bool,
    pub cloak_visible: bool,
    pub cloak_visible_detected_only: bool,
    pub droppod_only: bool,
    pub is_hideable: bool,
}

impl Default for LaserTrailType {
    /// `LaserTrailTypeClass`'s constructor defaults.
    fn default() -> Self {
        Self {
            draw_type: LaserTrailDrawType::Laser,
            is_house_color: false,
            color: [255, 0, 0],
            thickness: 4,
            is_alternate_color: false,
            bolt_color: [None; 3],
            bolt_disable: [false; 3],
            bolt_arcs: 8,
            bolt_z_adjust: 0,
            beam_color: None,
            beam_amplitude: 40.0,
            fade_duration: None,
            segment_length: 128,
            ignore_vertical: false,
            is_intense: false,
            cloak_visible: false,
            cloak_visible_detected_only: false,
            droppod_only: false,
            is_hideable: true,
        }
    }
}

impl LaserTrailType {
    fn read(section: &IniSection) -> Self {
        let mut value = Self::default();
        if let Some(raw) = section.read_name("DrawType", 2048) {
            value.draw_type = if raw.eq_ignore_ascii_case("ebolt") {
                LaserTrailDrawType::EBolt
            } else if raw.eq_ignore_ascii_case("radbeam") {
                LaserTrailDrawType::RadBeam
            } else {
                LaserTrailDrawType::Laser
            };
        }
        value.is_house_color = section.read_bool("IsHouseColor", value.is_house_color);
        value.color = section.read_phobos_color("Color", value.color);
        value.thickness = section.read_int("Thickness", value.thickness);
        value.is_alternate_color = section.read_bool("IsAlternateColor", value.is_alternate_color);
        for index in 0..3 {
            let color_key = match index {
                0 => "Bolt.Color1",
                1 => "Bolt.Color2",
                _ => "Bolt.Color3",
            };
            let disable_key = match index {
                0 => "Bolt.Disable1",
                1 => "Bolt.Disable2",
                _ => "Bolt.Disable3",
            };
            value.bolt_color[index] =
                section.read_phobos_nullable_color(color_key, value.bolt_color[index]);
            value.bolt_disable[index] = section.read_bool(disable_key, value.bolt_disable[index]);
        }
        value.bolt_arcs = section.read_int("Bolt.Arcs", value.bolt_arcs);
        value.bolt_z_adjust = section.read_int("Bolt.ZAdjust", value.bolt_z_adjust);
        value.beam_color = section.read_phobos_nullable_color("Beam.Color", value.beam_color);
        value.beam_amplitude = section.read_phobos_double("Beam.Amplitude", value.beam_amplitude);
        value.fade_duration = section.read_phobos_nullable_int("FadeDuration", value.fade_duration);
        value.segment_length = section.read_int("SegmentLength", value.segment_length);
        value.ignore_vertical = section.read_bool("IgnoreVertical", value.ignore_vertical);
        value.is_intense = section.read_bool("IsIntense", value.is_intense);
        value.cloak_visible = section.read_bool("CloakVisible", value.cloak_visible);
        value.cloak_visible_detected_only = section.read_bool(
            "CloakVisible.DetectedOnly",
            value.cloak_visible_detected_only,
        );
        value.droppod_only = section.read_bool("DropPodOnly", value.droppod_only);
        value.is_hideable = section.read_bool("IsHideable", value.is_hideable);
        value
    }

    /// The `FadeDuration` default each drawing system uses when the key is unset.
    pub fn fade_duration(&self) -> i32 {
        self.fade_duration.unwrap_or(match self.draw_type {
            LaserTrailDrawType::Laser => 64,
            LaserTrailDrawType::EBolt => 17,
            LaserTrailDrawType::RadBeam => 15,
        })
    }
}

/// The catalog, case-insensitive by `Enumerable` identity.
#[derive(Debug, Clone, Default)]
pub struct LaserTrailTypeRegistry {
    types: LookupMap<String, LaserTrailType>,
}

impl LaserTrailTypeRegistry {
    /// `[LaserTrailTypes]` list order, each entry reading its own ART section.
    /// A listed name without a section keeps the constructor defaults, as
    /// `Enumerable::LoadFromINIList` allocates the entry before its read.
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut types = LookupMap::default();
        for name in crate::rules::ruleset::parse_registry(ini, "LaserTrailTypes") {
            let value = ini
                .section(&name)
                .map(LaserTrailType::read)
                .unwrap_or_default();
            types.insert(name.to_ascii_uppercase(), value);
        }
        Self { types }
    }

    pub fn get(&self, name: &str) -> Option<&LaserTrailType> {
        self.types.get(&name.to_ascii_uppercase())
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ART-layer list and one entry's keys, against the constructor
    /// defaults for a section that leaves them unset.
    #[test]
    fn laser_trail_types_read_keys_and_keep_constructor_defaults() {
        let ini = IniFile::from_str(
            "[LaserTrailTypes]\n0=TRAIL\n1=PLAIN\n2=MISSING\n\
             [TRAIL]\nDrawType=ebolt\nColor=1,2,3\nThickness=2\nIsHouseColor=yes\n\
             FadeDuration=7\nSegmentLength=64\nIgnoreVertical=yes\nIsIntense=yes\n\
             CloakVisible=yes\nBolt.Arcs=5\nBeam.Amplitude=12.5\nBolt.Color2=4,5,6\n\
             [PLAIN]\nIgnoreVertical=no\n",
        );
        let registry = LaserTrailTypeRegistry::from_ini(&ini);
        let trail = registry.get("trail").expect("case-insensitive entry");
        assert_eq!(trail.draw_type, LaserTrailDrawType::EBolt);
        assert_eq!(trail.color, [1, 2, 3]);
        assert_eq!(trail.thickness, 2);
        assert!(trail.is_house_color);
        assert_eq!(trail.fade_duration(), 7);
        assert_eq!(trail.segment_length, 64);
        assert!(trail.ignore_vertical);
        assert!(trail.is_intense);
        assert!(trail.cloak_visible);
        assert_eq!(trail.bolt_arcs, 5);
        assert_eq!(trail.beam_amplitude, 12.5);
        assert_eq!(trail.bolt_color[1], Some([4, 5, 6]));

        let plain = registry.get("plain").expect("defaulted entry");
        assert_eq!(plain.draw_type, LaserTrailDrawType::Laser);
        assert_eq!(plain.color, [255, 0, 0]);
        assert_eq!(plain.thickness, 4);
        assert_eq!(plain.segment_length, 128);
        assert_eq!(plain.fade_duration(), 64, "laser's FadeDuration default");
        assert!(plain.is_hideable);

        let missing = registry.get("missing").expect("allocated entry");
        assert_eq!(
            missing,
            &LaserTrailType::default(),
            "a listed name without a section keeps the constructor defaults"
        );
    }
}
