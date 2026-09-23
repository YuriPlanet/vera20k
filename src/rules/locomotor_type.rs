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

/// Which locomotor class controls a unit's movement behavior.
///
/// Each variant is a distinct movement controller / state machine in the
/// original engine. Do NOT collapse these into one generic "ground mover" —
/// they have meaningfully different behavior (see locomotor report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LocomotorKind {
    /// Standard ground vehicle movement. Baseline for all ground movers.
    Drive,
    /// Hovering vehicle (Robot Tank, Hover MLRS). ~35% slower than Drive.
    Hover,
    /// Tiberian Sun subterranean burrowing. **Inert — no movement system, and no
    /// CLSID resolves to it**, so nothing can construct one. The variant is
    /// retained only because `world_hash` hashes this enum by discriminant
    /// (`(loco.kind as u8)`), so deleting it would renumber every later variant
    /// and shift the replay baseline for zero runtime benefit. Fold it into the
    /// substrate's locomotor class when that migration re-baselines anyway.
    /// Not to be confused with low-bridge `TubeClass` movement, which is live YR.
    Tunnel,
    /// Infantry ground movement. Distinct arrival threshold from vehicles.
    Walk,
    /// Tiberian Sun drop-pod entry. **Inert** for the same reason as
    /// [`LocomotorKind::Tunnel`] — no movement system, no CLSID resolves to it,
    /// and the variant is kept only to preserve discriminant numbering.
    DropPod,
    /// True aircraft (Harrier, Kirov). Dedicated altitude state machine.
    Fly,
    /// Chrono movement (instant relocation). Often a temporary override.
    Teleport,
    /// Walker vehicle (e.g., Mammoth Mk. II). Drive-like with wobble/gait quirks.
    Mech,
    /// Naval vessel. Drive-like but carries naval identity for AI recognition.
    Ship,
    /// Jumpjet hover-flight (Rocketeer). Altitude-holding state machine, NOT Fly.
    Jumpjet,
    /// Spawned missile (V3, Dreadnought). Scripted missile controller.
    Rocket,
    /// Falling under a parachute (paradropped infantry). Runtime-only — no
    /// CLSID maps to it, so it is not installable and cannot reach a locomotor
    /// slot. Nothing sets it today: the separate "override" mechanism that once
    /// did was folded into the single piggyback slot, and paradrop descent
    /// carries its own state rather than displacing the locomotor.
    Parachute,
}

/// The eight CLSIDs selected by uncommented `Locomotor=` keys in retail YR, in
/// the braces-and-upper-case spelling the retail INI uses. Kind-valued mirror
/// of the substrate's class table; `install_tables_agree_with_rules_kind_table`
/// in `sim::movement::locomotion::install` locks the two together.
///
/// The dormant Tiberian Sun CLSIDs (Mech, Tunnel, DropPod) are deliberately
/// absent: the executable registers those classes, but no live movement system
/// exists, so an INI naming one falls back to the constructor seed like any
/// other unrecognized value. (An earlier parser here resolved the Mech CLSID
/// to `LocomotorKind::Mech`; that was VERA-invented and never production-reachable.)
pub const INSTALLED_CLSID_KIND_TABLE: [(&str, LocomotorKind); 8] = [
    (
        "{4A582741-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Drive,
    ),
    (
        "{4A582742-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Hover,
    ),
    ("{4A582744-9839-11D1-B709-00A024DDAFD1}", LocomotorKind::Walk),
    ("{4A582746-9839-11D1-B709-00A024DDAFD1}", LocomotorKind::Fly),
    (
        "{4A582747-9839-11D1-B709-00A024DDAFD1}",
        LocomotorKind::Teleport,
    ),
    ("{2BEA74E1-7CCA-11D3-BE14-00104B62A16C}", LocomotorKind::Ship),
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
pub fn kind_from_clsid(text: &str) -> Option<LocomotorKind> {
    let normalized = text
        .trim()
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
        .unwrap_or_else(|| text.trim());

    INSTALLED_CLSID_KIND_TABLE
        .iter()
        .find(|(clsid, _)| clsid[1..clsid.len() - 1].eq_ignore_ascii_case(normalized))
        .map(|(_, kind)| *kind)
}

/// The kind a type installs when its `Locomotor=` value is absent or does not
/// parse: the native type constructor's seed.
///
/// gamemd seeds the type's locomotor-CLSID field with the **Teleport** GUID
/// before any INI is read, then passes the field's current value as the CLSID
/// reader's default argument — so an absent key and an unparseable value take
/// the same path with no category input. Full derivation and the retail
/// reachability analysis live in `sim::movement::locomotion::install`.
pub const DEFAULT_INSTALLED_KIND: LocomotorKind = LocomotorKind::Teleport;

/// Resolve the locomotor kind a type installs at spawn from its raw
/// `Locomotor=` text (`None` when the key is absent).
pub fn resolve_installed_kind(value: Option<&str>) -> LocomotorKind {
    value.and_then(kind_from_clsid).unwrap_or(DEFAULT_INSTALLED_KIND)
}

// ---------------------------------------------------------------------------
// SpeedType
// ---------------------------------------------------------------------------

/// Determines which terrain cells are actually traversable for a unit.
///
/// Parsed from rules.ini `SpeedType=` key. Controls terrain legality in the
/// pathfinder — a cell is only enterable if the SpeedType allows it.
///
/// Variant order matches the binary enum table at 0x81DA58 in gamemd.exe.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SpeedType {
    /// Infantry default. Can traverse most land terrain.
    Foot,
    /// Most vehicles. Cannot cross water, limited on rough terrain.
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
}

impl Default for SpeedType {
    fn default() -> Self {
        Self::Track
    }
}

impl SpeedType {
    /// All SpeedTypes that have terrain cost grids (excludes Winged which ignores terrain).
    /// Order matches the binary enum table.
    pub const ALL_WITH_COSTS: &[SpeedType] = &[
        SpeedType::Foot,
        SpeedType::Track,
        SpeedType::Wheel,
        SpeedType::Hover,
        SpeedType::Float,
        SpeedType::Amphibious,
        SpeedType::FloatBeach,
    ];

    /// Parse from a rules.ini SpeedType= value string (case-insensitive).
    pub fn from_ini(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "foot" => Self::Foot,
            "track" => Self::Track,
            "wheel" => Self::Wheel,
            "float" => Self::Float,
            "amphibious" => Self::Amphibious,
            "winged" => Self::Winged,
            "floatbeach" => Self::FloatBeach,
            "hover" => Self::Hover,
            _ => {
                log::warn!("Unknown SpeedType '{}', defaulting to Track", value);
                Self::Track
            }
        }
    }

    /// Human-readable name for debug display.
    pub fn name(&self) -> &'static str {
        match self {
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

    /// Next SpeedType in `ALL_WITH_COSTS`, wrapping around.
    pub fn cycle_next(&self) -> SpeedType {
        let list = Self::ALL_WITH_COSTS;
        let idx = list.iter().position(|s| s == self).unwrap_or(0);
        list[(idx + 1) % list.len()]
    }

    /// Previous SpeedType in `ALL_WITH_COSTS`, wrapping around.
    pub fn cycle_prev(&self) -> SpeedType {
        let list = Self::ALL_WITH_COSTS;
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
pub enum MovementZone {
    /// Invalid parser result. Retail stores `-1` for unknown strings rather than
    /// silently falling back to Normal; downstream runtime behavior still needs
    /// a dedicated trace before it should be used for parity claims.
    Invalid = -1,
    /// Row 0: only movement class 0 is passable.
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

impl Default for MovementZone {
    fn default() -> Self {
        Self::Normal
    }
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

    /// Water movers bypass the land PathGrid and use the passability matrix
    /// directly. Single source of truth for pathfinding, movement stepping,
    /// target redirect, and wake effects.
    pub fn is_water_mover(&self) -> bool {
        matches!(self, Self::Water | Self::WaterBeach)
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

    /// Which SpeedType governs terrain cost for this movement zone.
    /// Controls how fast a unit moves on passable cells (not which cells are passable).
    pub fn speed_type(&self) -> SpeedType {
        match self {
            MovementZone::Normal
            | MovementZone::Crusher
            | MovementZone::Destroyer
            | MovementZone::CrusherAll
            | MovementZone::Subterranean => SpeedType::Track,
            MovementZone::AmphibiousCrusher
            | MovementZone::AmphibiousDestroyer
            | MovementZone::Amphibious => SpeedType::Amphibious,
            MovementZone::Infantry | MovementZone::InfantryDestroyer => SpeedType::Foot,
            MovementZone::Water => SpeedType::Float,
            MovementZone::WaterBeach => SpeedType::FloatBeach,
            MovementZone::Fly => SpeedType::Winged,
            MovementZone::Invalid => SpeedType::Track,
        }
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
    /// Scope of the claim: `rulesmd.ini` and `rules.ini` only. Campaign, mission
    /// and map INIs are UNCHECKED.
    #[test]
    fn dormant_clsids_absent_from_retail_inis() {
        for name in ["rulesmd.ini", "rules.ini"] {
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
    fn lowercase_retail_spelling_resolves() {
        // Four stock sections spell `11d1` in lower case.
        assert_eq!(
            kind_from_clsid("{4A582747-9839-11d1-B709-00A024DDAFD1}"),
            Some(LocomotorKind::Teleport)
        );
    }

    #[test]
    fn absent_and_unparseable_values_take_the_constructor_seed() {
        assert_eq!(resolve_installed_kind(None), DEFAULT_INSTALLED_KIND);
        for bad in [
            "",
            "not-a-guid",
            "{00000000-0000-0000-0000-000000000000}",
        ] {
            assert_eq!(kind_from_clsid(bad), None);
            assert_eq!(resolve_installed_kind(Some(bad)), DEFAULT_INSTALLED_KIND);
        }
    }

    #[test]
    fn dormant_mech_clsid_does_not_resolve() {
        // The executable registers the Mech class but no live movement system
        // exists; the production install path treats its CLSID like any other
        // unrecognized value. The deleted `from_clsid` parser mapped it to
        // `LocomotorKind::Mech` — VERA-invented, never production-reachable.
        let mech = "{55D141B8-DB94-11D1-AC98-006008055BB5}";
        assert_eq!(kind_from_clsid(mech), None);
        assert_eq!(resolve_installed_kind(Some(mech)), LocomotorKind::Teleport);
    }

    #[test]
    fn test_speed_type_from_ini() {
        assert_eq!(SpeedType::from_ini("Foot"), SpeedType::Foot);
        assert_eq!(SpeedType::from_ini("Track"), SpeedType::Track);
        assert_eq!(SpeedType::from_ini("wheel"), SpeedType::Wheel);
        assert_eq!(SpeedType::from_ini("FLOAT"), SpeedType::Float);
        assert_eq!(SpeedType::from_ini("Amphibious"), SpeedType::Amphibious);
        assert_eq!(SpeedType::from_ini("Winged"), SpeedType::Winged);
        assert_eq!(SpeedType::from_ini("FloatBeach"), SpeedType::FloatBeach);
        assert_eq!(SpeedType::from_ini("Hover"), SpeedType::Hover);
    }

    #[test]
    fn test_speed_type_unknown_defaults_to_track() {
        assert_eq!(SpeedType::from_ini("bogus"), SpeedType::Track);
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
}
