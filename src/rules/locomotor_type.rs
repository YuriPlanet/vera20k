//! Locomotor, SpeedType, and MovementZone enums parsed from rules.ini.
//!
//! RA2/YR movement is a 4-layer system:
//! 1. **LocomotorKind** — runtime state machine class (Drive, Walk, Fly, etc.)
//! 2. **SpeedType** — which terrain cells are actually traversable
//! 3. **MovementZone** — pathfinder routing assumptions and special logic
//! 4. **Per-unit flags** — JumpJet, Teleporter, HoverAttack, etc. (on ObjectType)
//!
//! RA2 identifies locomotors by COM CLSIDs (e.g., `{4A582741-9839-11d1-B709-00A024DDAFD1}`
//! for Drive). We parse these into the `LocomotorKind` enum.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

// ---------------------------------------------------------------------------
// LocomotorKind
// ---------------------------------------------------------------------------

/// Which locomotor class controls a unit's movement behavior: the eight
/// classes an uncommented retail YR `Locomotor=` key selects.
///
/// Each variant is a distinct movement controller / state machine in the
/// original engine. Do NOT collapse these into one generic "ground mover" —
/// they have meaningfully different behavior (see locomotor report).
///
/// The executable also registers Tiberian Sun's Mech, Tunnel and DropPod
/// classes. They are dormant in YR: no retail `Locomotor=` names them and
/// DropPod's only installer (`0x004DB8A0`) has no references
/// (`docs/plans/2026-07-29-locomotion-substrate-design.md`), so they have no
/// variant here.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum LocomotorKind {
    /// Standard ground vehicle movement. Baseline for all ground movers.
    Drive,
    /// Hovering vehicle (Robot Tank, Hover MLRS).
    Hover,
    /// Infantry ground movement. Distinct arrival threshold from vehicles.
    Walk,
    /// True aircraft (Harrier). Dedicated altitude state machine.
    Fly,
    /// Chrono movement (instant relocation). Often a temporary override.
    Teleport,
    /// Naval vessel. Drive-like but carries naval identity for AI recognition.
    Ship,
    /// Jumpjet hover-flight (Rocketeer, Kirov). Altitude-holding state machine, NOT Fly.
    Jumpjet,
    /// Spawned missile (V3, Dreadnought). Scripted missile controller.
    Rocket,
}

impl LocomotorKind {
    #[cfg(test)]
    pub const ALL: [Self; 8] = [
        Self::Drive,
        Self::Hover,
        Self::Walk,
        Self::Fly,
        Self::Teleport,
        Self::Ship,
        Self::Jumpjet,
        Self::Rocket,
    ];

    /// Row of this kind in `Self::ALL` and [`INSTALLED_CLSID_KIND_TABLE`].
    pub const fn table_index(self) -> usize {
        self as usize
    }

    /// The canonical retail CLSID text for this kind.
    pub const fn clsid(self) -> &'static str {
        INSTALLED_CLSID_KIND_TABLE[self.table_index()].0
    }
}

/// The eight CLSIDs selected by uncommented `Locomotor=` keys in retail YR, in
/// the braces-and-upper-case spelling the retail INI uses, in
/// `LocomotorKind::ALL` order.
///
/// The dormant Tiberian Sun CLSIDs (Mech, Tunnel, DropPod) are deliberately
/// absent: an INI naming one falls back to the constructor seed like any other
/// unrecognized value.
pub const INSTALLED_CLSID_KIND_TABLE: [(&str, LocomotorKind); 8] = [
    (
        "{4A582741-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Drive,
    ),
    (
        "{4A582742-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Hover,
    ),
    (
        "{4A582744-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Walk,
    ),
    ("{4A582746-9839-11D1-B709-00A024DDAFD1}", LocomotorKind::Fly),
    (
        "{4A582747-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Teleport,
    ),
    (
        "{2BEA74E1-7CCA-11D3-BE14-00104B62A16C}",
        LocomotorKind::Ship,
    ),
    (
        "{92612C46-F71F-11D1-AC9F-006008055BB5}",
        LocomotorKind::Jumpjet,
    ),
    (
        "{B7B49766-E576-11D3-9BD9-00104B972FE8}",
        LocomotorKind::Rocket,
    ),
];

/// Parse a retail GUID spelling into one of the eight installable kinds.
///
/// Braces are optional and ASCII case is ignored. Parse failure stays
/// explicit; [`resolve_installed_kind`] owns the native silent/default
/// fallback.
///
/// With the `phobos` feature, values that do not start with `{` may instead
/// name a semantic alias (Phobos `Utilities/TemplateDef.h`, `read<CLSID>`).
pub fn kind_from_clsid(text: &str) -> Option<LocomotorKind> {
    let normalized = text
        .trim()
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
        .unwrap_or_else(|| text.trim());

    if let Some(kind) = INSTALLED_CLSID_KIND_TABLE
        .iter()
        .find(|(clsid, _)| clsid[1..clsid.len() - 1].eq_ignore_ascii_case(normalized))
        .map(|(_, kind)| *kind)
    {
        return Some(kind);
    }

    #[cfg(feature = "phobos")]
    {
        kind_from_alias(text)
    }
    #[cfg(not(feature = "phobos"))]
    {
        None
    }
}

/// Phobos semantic locomotor aliases: a value without a leading `{` matched
/// case-insensitively to a class name. The dormant Tiberian Sun classes
/// (`Tunnel`, `Mech`, `DropPod`) have no VERA kind, so their aliases resolve
/// like their CLSIDs — to `None` — rather than selecting a different movement
/// system. `Chrono` is not an alias either; it falls back to the Teleport
/// constructor seed.
#[cfg(feature = "phobos")]
fn kind_from_alias(text: &str) -> Option<LocomotorKind> {
    let text = text.trim();
    if text.starts_with('{') {
        return None;
    }
    Some(match text.to_ascii_lowercase().as_str() {
        "drive" => LocomotorKind::Drive,
        "jumpjet" => LocomotorKind::Jumpjet,
        "hover" => LocomotorKind::Hover,
        "rocket" => LocomotorKind::Rocket,
        "walk" => LocomotorKind::Walk,
        "fly" => LocomotorKind::Fly,
        "teleport" => LocomotorKind::Teleport,
        "ship" => LocomotorKind::Ship,
        _ => return None,
    })
}

/// The kind a type installs when its `Locomotor=` value is absent or does not
/// parse: the native type constructor's seed.
///
/// There is one field and one default, not a "no key" rule and a "bad CLSID"
/// rule. The type constructor seeds the type's locomotor-CLSID field with the
/// **Teleport** GUID before any INI is read (a plain 16-byte copy, not a
/// category lookup); the INI read then passes the field's current value as the
/// CLSID reader's default argument. So an absent key and an unparseable value
/// both keep the seed, with no dependence on the unit's category. (VERA once
/// fell back per category — infantry Walk, vehicles Drive, aircraft Fly — which
/// has no native counterpart.) In stock YR only `DeathDummy`, an internal type,
/// of the 157 units on the `InfantryTypes`/`VehicleTypes`/`AircraftTypes`
/// rosters omits `Locomotor=`.
pub const DEFAULT_INSTALLED_KIND: LocomotorKind = LocomotorKind::Teleport;

/// Resolve the locomotor kind a type installs at spawn from its raw
/// `Locomotor=` text (`None` when the key is absent).
pub fn resolve_installed_kind(value: Option<&str>) -> LocomotorKind {
    value
        .and_then(kind_from_clsid)
        .unwrap_or(DEFAULT_INSTALLED_KIND)
}

// ---------------------------------------------------------------------------
// SpeedType
// ---------------------------------------------------------------------------

/// Determines which terrain cells are actually traversable for a unit.
///
/// Read from the exact `SpeedType` key in the scenario's layered rules.
/// Controls terrain legality in the pathfinder: a cell is only enterable
/// if the SpeedType allows it.
///
/// Variant order matches the binary enum table at 0x81DA58 in gamemd.exe.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
pub enum SpeedType {
    /// Infantry default. Can traverse most land terrain.
    Foot = 0,
    /// Most vehicles. Cannot cross water, limited on rough terrain.
    #[default]
    Track,
    /// Wheeled vehicles. Slower on rough terrain than Track.
    Wheel,
    /// Jumpjet hover movement type.
    Hover,
    /// Aircraft. Ignores terrain entirely.
    Winged,
    /// Hover units. Can cross water and land.
    Float,
    /// Amphibious units. Can traverse both land and water.
    Amphibious,
    /// Hover that can go on beaches (specific to certain hover units).
    FloatBeach,
    /// Original48DFF0 returns -1 for a nonempty unknown name. Keep this
    /// distinct from Track; native invalid-index gameplay is not established.
    /// Last in declaration order to preserve existing serialized variant tags.
    Invalid = -1,
}

impl SpeedType {
    /// UnitType747620's reached Rules pass. Its constructor passes -1 at
    /// 7470D7..7470DE (TechnoType stores it at7110E0). The parent first reads
    /// SpeedType at7121D1..7121E5 and Crusher at714CC8..714CE3; Unit then
    /// replaces only -1 with Crusher ? Track : Wheel at7476D3..7476EB and
    /// reads SpeedType again at7476F1..74770B. Both use ReadSpeedType476FC0.
    /// `crusher` is the retained value after this pass's parent reader.
    pub(crate) fn read_vehicle_pass(
        self,
        section: &crate::rules::ini_parser::IniSection,
        crusher: bool,
    ) -> Self {
        let current = section.read_speed_type("SpeedType", self);
        let default = if current == Self::Invalid {
            if crusher { Self::Track } else { Self::Wheel }
        } else {
            current
        };
        section.read_speed_type("SpeedType", default)
    }

    /// All SpeedTypes with a land row (excludes Winged, which ignores terrain).
    /// Order matches the binary enum table.
    pub const ALL_WITH_LAND_ROWS: &[SpeedType] = &[
        SpeedType::Foot,
        SpeedType::Track,
        SpeedType::Wheel,
        SpeedType::Hover,
        SpeedType::Float,
        SpeedType::Amphibious,
        SpeedType::FloatBeach,
    ];

    /// Original48DFF0 whole-name parser, after ReadString has trimmed input.
    /// The field reader owns absent/empty defaults and its 128-byte buffer.
    pub fn from_ini(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "foot" => Self::Foot,
            "track" => Self::Track,
            "wheel" => Self::Wheel,
            "float" => Self::Float,
            "amphibious" => Self::Amphibious,
            "winged" => Self::Winged,
            "floatbeach" => Self::FloatBeach,
            "hover" => Self::Hover,
            _ => Self::Invalid,
        }
    }

    /// Human-readable name for debug display.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Invalid => "Invalid",
            Self::Foot => "Foot",
            Self::Track => "Track",
            Self::Wheel => "Wheel",
            Self::Float => "Float",
            Self::Amphibious => "Amphibious",
            Self::Winged => "Winged",
            Self::FloatBeach => "FloatBeach",
            Self::Hover => "Hover",
        }
    }

    /// Next SpeedType in `ALL_WITH_LAND_ROWS`, wrapping around.
    pub fn cycle_next(&self) -> SpeedType {
        let list = Self::ALL_WITH_LAND_ROWS;
        let idx = list.iter().position(|s| s == self).unwrap_or(0);
        list[(idx + 1) % list.len()]
    }

    /// Previous SpeedType in `ALL_WITH_LAND_ROWS`, wrapping around.
    pub fn cycle_prev(&self) -> SpeedType {
        let list = Self::ALL_WITH_LAND_ROWS;
        let idx = list.iter().position(|s| s == self).unwrap_or(0);
        list[(idx + list.len() - 1) % list.len()]
    }
}

// ---------------------------------------------------------------------------
// MovementZone
// ---------------------------------------------------------------------------

/// Determines path search behavior and special routing logic.
///
/// Parsed from rules.ini `MovementZone=` key. Controls what kind of route
/// the pathfinder plans — distinct from SpeedType which controls terrain legality.
///
/// The numeric value IS the passability-matrix row index used by the original
/// pathfinding code. Recent RE shows these rows are keyed by derived
/// `MovementClass8`, not directly by our terrain `LandType` buckets.
///
/// Example: `MovementZone=Subterannean` enables dig-in/dig-out cell search
/// logic that plain Drive does not have. The misspelling is the retail parser
/// spelling.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(i8)]
#[derive(Default)]
pub enum MovementZone {
    /// Invalid parser result. Retail stores `-1` for unknown strings rather than
    /// silently falling back to Normal; downstream runtime behavior still needs
    /// a dedicated trace before it should be used for parity claims.
    Invalid = -1,
    /// Row 0: only movement class 0 is passable.
    #[default]
    Normal = 0,
    /// Row 1: classes 0 and 1 are passable.
    Crusher = 1,
    /// Row 2: classes 0, 1, and 2 are passable.
    Destroyer = 2,
    /// Row 3: classes 0, 1, 2, 3, 4, and 5 are passable.
    AmphibiousDestroyer = 3,
    /// Row 4: classes 0, 1, 3, and 4 are passable.
    AmphibiousCrusher = 4,
    /// Row 5: classes 0, 3, and 4 are passable.
    Amphibious = 5,
    /// Row 6: classes 0, 1, 2, and 6 are passable.
    Subterranean = 6,
    /// Row 7: classes 0 and 5 are passable.
    Infantry = 7,
    /// Row 8: classes 0, 1, 2, and 5 are passable.
    InfantryDestroyer = 8,
    /// Row 9: classes 0 through 6 are passable.
    Fly = 9,
    /// Row 10: only class 4 is passable.
    Water = 10,
    /// Row 11: classes 3 and 4 are passable.
    WaterBeach = 11,
    /// Row 12: classes 0, 1, and 2 are passable.
    CrusherAll = 12,
}

impl MovementZone {
    /// Parse from a rules.ini MovementZone= value string (case-insensitive).
    pub fn from_ini(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "normal" => Self::Normal,
            "crusher" => Self::Crusher,
            "destroyer" => Self::Destroyer,
            "amphibiousdestroyer" => Self::AmphibiousDestroyer,
            "amphibiouscrusher" => Self::AmphibiousCrusher,
            "amphibious" => Self::Amphibious,
            "subterannean" => Self::Subterranean,
            "infantry" => Self::Infantry,
            "infantrydestroyer" => Self::InfantryDestroyer,
            "fly" => Self::Fly,
            "water" => Self::Water,
            "waterbeach" => Self::WaterBeach,
            "crusherall" => Self::CrusherAll,
            _ => {
                log::warn!(
                    "Unknown MovementZone '{}', preserving binary invalid row -1",
                    value
                );
                Self::Invalid
            }
        }
    }

    /// Passability matrix row index. Invalid parser rows have no safe matrix row
    /// in the current Rust model; callers should treat `None` as non-parity data.
    pub fn matrix_row(self) -> Option<usize> {
        if self == Self::Invalid {
            None
        } else {
            Some(self as usize)
        }
    }

    /// All MovementZone variants that need computed zone grids.
    /// gamemd rebuilds every binary movement-zone row, including Fly.
    pub fn all_ground() -> &'static [MovementZone] {
        &[
            MovementZone::Normal,
            MovementZone::Crusher,
            MovementZone::Destroyer,
            MovementZone::AmphibiousDestroyer,
            MovementZone::AmphibiousCrusher,
            MovementZone::Amphibious,
            MovementZone::Subterranean,
            MovementZone::Infantry,
            MovementZone::InfantryDestroyer,
            MovementZone::Fly,
            MovementZone::Water,
            MovementZone::WaterBeach,
            MovementZone::CrusherAll,
        ]
    }

    /// Whether this MovementZone can traverse bridges (ground-capable).
    pub fn can_use_bridges(&self) -> bool {
        !matches!(
            self,
            MovementZone::Water
                | MovementZone::WaterBeach
                | MovementZone::Fly
                | MovementZone::Invalid
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two locomotor CLSIDs whose movement systems were removed as Tiberian
    /// Sun legacy. Kept here, not in the production table, precisely because
    /// nothing in the engine may resolve them any more.
    const DORMANT_CLSID_TUNNEL: &str = "4A582743-9839-11D1-B709-00A024DDAFD1";
    const DORMANT_CLSID_DROPPOD: &str = "4A582745-9839-11D1-B709-00A024DDAFD1";

    /// No stock unit selects the Tunnel or DropPod locomotor, which is what
    /// makes removing those two movement systems safe.
    ///
    /// The golden is retail INI bytes — not a hand-written list and not a
    /// Rust-vs-Rust comparison — so this is a genuine parity check on the
    /// dormancy claim. If a future INI reintroduces either CLSID this goes red,
    /// which is the correct signal: the engine would then silently fall back to
    /// the default locomotor for those units.
    ///
    /// Scope of the claim: active YR `rulesmd.ini` only. Campaign, mission
    /// and map INIs are UNCHECKED.
    #[test]
    fn dormant_clsids_absent_from_retail_inis() {
        for name in ["rulesmd.ini"] {
            let Some(text) = crate::rules::retail_ini_fixture::retail_ini_text(name) else {
                return;
            };
            let upper = text.to_ascii_uppercase();
            for (label, clsid) in [
                ("Tunnel", DORMANT_CLSID_TUNNEL),
                ("DropPod", DORMANT_CLSID_DROPPOD),
            ] {
                let hits = upper.matches(clsid).count();
                assert_eq!(
                    hits, 0,
                    "{name} references the dormant {label} locomotor CLSID {clsid} {hits} time(s); \
                     its movement system was removed, so those units would fall back to the \
                     default locomotor"
                );
            }
        }
    }

    #[test]
    fn installed_table_resolves_every_row_with_and_without_braces() {
        for &(clsid, kind) in &INSTALLED_CLSID_KIND_TABLE {
            assert_eq!(kind_from_clsid(clsid), Some(kind), "CLSID: {clsid}");
            assert_eq!(
                kind_from_clsid(&clsid[1..clsid.len() - 1]),
                Some(kind),
                "braceless CLSID: {clsid}"
            );
            assert_eq!(resolve_installed_kind(Some(clsid)), kind);
        }
    }

    #[test]
    fn retail_locomotor_keys_select_only_installed_kinds() {
        // PARITY: the golden is the retail `ini/rulesmd.ini` byte content. Strip
        // `;` comments before counting: two Drive rows name the dormant Mech
        // GUID in trailing comments.
        let Some(rulesmd) = crate::rules::retail_ini_fixture::retail_ini_text("rulesmd.ini") else {
            return;
        };
        let mut histogram = std::collections::BTreeMap::new();
        let mut locomotor_key_total = 0usize;
        for raw_line in rulesmd.lines() {
            let line = raw_line.split_once(';').map_or(raw_line, |(body, _)| body);
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if !key.trim().eq_ignore_ascii_case("Locomotor") {
                continue;
            }
            locomotor_key_total += 1;
            if let Some(kind) = kind_from_clsid(value.trim()) {
                *histogram.entry(kind).or_insert(0usize) += 1;
            }
        }
        let expected = std::collections::BTreeMap::from([
            (LocomotorKind::Walk, 60),
            (LocomotorKind::Drive, 52),
            (LocomotorKind::Ship, 13),
            (LocomotorKind::Jumpjet, 9),
            (LocomotorKind::Fly, 8),
            (LocomotorKind::Teleport, 6),
            (LocomotorKind::Hover, 4),
            (LocomotorKind::Rocket, 3),
        ]);
        assert_eq!(histogram, expected);
        assert_eq!(locomotor_key_total, 155);
    }

    /// The six stock sections that run a Teleport locomotor. Four name the
    /// CLSID with a trailing comment, and two of those spell `11d1` in lower
    /// case.
    #[test]
    fn six_stock_teleport_sections_resolve_to_teleport() {
        for section_value in [
            "{4A582747-9839-11d1-B709-00A024DDAFD1}", // CLEG
            "{4A582747-9839-11d1-B709-00A024DDAFD1}", // CCOMAND
            "{4A582747-9839-11d1-B709-00A024DDAFD1}", // CIVAN
            "{4A582747-9839-11d1-B709-00A024DDAFD1}", // CMIN
            "{4A582747-9839-11d1-B709-00A024DDAFD1}", // CMON
            "{4A582747-9839-11D1-B709-00A024DDAFD1}", // SMON
        ] {
            assert_eq!(
                resolve_installed_kind(Some(section_value)),
                LocomotorKind::Teleport
            );
        }
    }

    #[test]
    fn absent_and_unparseable_values_take_the_constructor_seed() {
        assert_eq!(resolve_installed_kind(None), DEFAULT_INSTALLED_KIND);
        for bad in ["", "not-a-guid", "{00000000-0000-0000-0000-000000000000}"] {
            assert_eq!(kind_from_clsid(bad), None);
            assert_eq!(resolve_installed_kind(Some(bad)), DEFAULT_INSTALLED_KIND);
        }
    }

    #[test]
    fn dormant_mech_clsid_does_not_resolve() {
        // The executable registers the Mech class but YR never selects it; the
        // install path treats its CLSID like any other unrecognized value.
        let mech = "{55D141B8-DB94-11D1-AC98-006008055BB5}";
        assert_eq!(kind_from_clsid(mech), None);
        assert_eq!(resolve_installed_kind(Some(mech)), LocomotorKind::Teleport);
    }

    #[test]
    fn gsi_04_04_movement_zone_parser_accepts_only_retail_labels() {
        assert_eq!(MovementZone::from_ini("Normal"), MovementZone::Normal);
        assert_eq!(MovementZone::from_ini("crusher"), MovementZone::Crusher);
        assert_eq!(MovementZone::from_ini("DESTROYER"), MovementZone::Destroyer);
        assert_eq!(
            MovementZone::from_ini("AmphibiousCrusher"),
            MovementZone::AmphibiousCrusher
        );
        assert_eq!(
            MovementZone::from_ini("AmphibiousDestroyer"),
            MovementZone::AmphibiousDestroyer
        );
        assert_eq!(MovementZone::from_ini("Infantry"), MovementZone::Infantry);
        assert_eq!(
            MovementZone::from_ini("InfantryDestroyer"),
            MovementZone::InfantryDestroyer
        );
        assert_eq!(MovementZone::from_ini("Fly"), MovementZone::Fly);
        assert_eq!(
            MovementZone::from_ini("Subterannean"),
            MovementZone::Subterranean
        );
        assert_eq!(
            MovementZone::from_ini("Amphibious"),
            MovementZone::Amphibious
        );
        assert_eq!(MovementZone::from_ini("Water"), MovementZone::Water);
        assert_eq!(
            MovementZone::from_ini("WaterBeach"),
            MovementZone::WaterBeach
        );
        assert_eq!(
            MovementZone::from_ini("CrusherAll"),
            MovementZone::CrusherAll
        );
        assert_eq!(
            MovementZone::from_ini("Subterranean"),
            MovementZone::Invalid
        );
        assert_eq!(
            MovementZone::from_ini("Subterrannean"),
            MovementZone::Invalid
        );
    }

    #[test]
    fn test_movement_zone_unknown_preserves_invalid_row() {
        assert_eq!(MovementZone::from_ini("invalid"), MovementZone::Invalid);
        assert_eq!(MovementZone::Invalid.matrix_row(), None);
    }

    /// Phobos `read<CLSID>` semantic aliases: braceless, ASCII
    /// case-insensitive, and equivalent to the CLSID they abbreviate.
    #[cfg(feature = "phobos")]
    #[test]
    fn phobos_locomotor_aliases_select_their_kinds() {
        for (alias, kind) in [
            ("Drive", LocomotorKind::Drive),
            ("jumpJET", LocomotorKind::Jumpjet),
            ("Hover", LocomotorKind::Hover),
            ("Rocket", LocomotorKind::Rocket),
            ("walk", LocomotorKind::Walk),
            ("Fly", LocomotorKind::Fly),
            ("Teleport", LocomotorKind::Teleport),
            ("Ship", LocomotorKind::Ship),
        ] {
            assert_eq!(kind_from_clsid(alias), Some(kind), "alias {alias}");
            assert_eq!(resolve_installed_kind(Some(alias)), kind, "alias {alias}");
        }
    }

    /// `Chrono` is not an alias and the dormant classes have no VERA kind;
    /// both keep the Teleport constructor seed, and a braced alias is not an
    /// alias.
    #[cfg(feature = "phobos")]
    #[test]
    fn phobos_dormant_and_unknown_aliases_keep_the_teleport_seed() {
        for value in ["Chrono", "Tunnel", "Mech", "DropPod", "droppod", "{Drive}"] {
            assert_eq!(kind_from_clsid(value), None, "{value}");
            assert_eq!(resolve_installed_kind(Some(value)), DEFAULT_INSTALLED_KIND);
        }
    }

    #[cfg(not(feature = "phobos"))]
    #[test]
    fn locomotor_aliases_require_the_phobos_feature() {
        assert_eq!(kind_from_clsid("Drive"), None);
        assert_eq!(
            resolve_installed_kind(Some("Drive")),
            DEFAULT_INSTALLED_KIND
        );
    }
}
