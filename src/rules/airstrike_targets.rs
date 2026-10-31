//! Phobos `[Warhead] AirstrikeTargets=`: which target kinds an `Airstrike=`
//! warhead may designate.
//!
//! Source: Phobos `src/Utilities/Enum.h` (`AffectedTarget`),
//! `src/Utilities/EnumFunctions.cpp::IsTechnoEligible` and the reader in
//! `src/Utilities/TemplateDef.h`; read into `WarheadTypeExt::AirstrikeTargets`
//! (`src/Ext/WarheadType/Body.cpp:400`) with the default
//! `AffectedTarget::Building` (`src/Ext/WarheadType/Body.h:495`).
//!
//! The list is comma-separated, case-insensitive and trimmed. `land`, `water`
//! and `empty` are accepted by the reader but name no techno kind, so they
//! only exist to keep a shared mod value parsing.

/// One target kind an airstrike warhead may designate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirstrikeKind {
    Infantry,
    /// Vehicles and any other `Unit` (what gamemd calls a non-infantry,
    /// non-building techno).
    Unit,
    Aircraft,
    Building,
}

/// The `AirstrikeTargets=` bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirstrikeTargets(u8);

impl Default for AirstrikeTargets {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl AirstrikeTargets {
    pub const NONE: Self = Self(0);
    pub const INFANTRY: Self = Self(1 << 0);
    pub const UNIT: Self = Self(1 << 1);
    pub const BUILDING: Self = Self(1 << 2);
    pub const AIRCRAFT: Self = Self(1 << 3);
    pub const ALL: Self = Self(0b1111);

    /// `AirstrikeTargets=` with the documented default `buildings`.
    pub const DEFAULT: Self = Self::BUILDING;

    /// Read the name list. `None` when a token names no target, which leaves
    /// the retained default in place like the reader's failed parse.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for part in raw.split(',') {
            let token = part.trim();
            if token.is_empty() {
                return None;
            }
            let flag = if token.eq_ignore_ascii_case("infantry") {
                Self::INFANTRY
            } else if token.eq_ignore_ascii_case("units") {
                Self::UNIT
            } else if token.eq_ignore_ascii_case("buildings") {
                Self::BUILDING
            } else if token.eq_ignore_ascii_case("aircraft") {
                Self::AIRCRAFT
            } else if token.eq_ignore_ascii_case("all") {
                Self::ALL
            } else if token.eq_ignore_ascii_case("none")
                || token.eq_ignore_ascii_case("land")
                || token.eq_ignore_ascii_case("water")
                || token.eq_ignore_ascii_case("empty")
            {
                Self::NONE
            } else {
                return None;
            };
            parsed = Self(parsed.0 | flag.0);
        }
        Some(parsed)
    }

    /// Whether a target of `kind` may be designated.
    ///
    /// `Aircraft` reads the `units` bit: Phobos' eligibility test calls
    /// `IsTechnoEligible` without `considerAircraftSeparately`, so aircraft
    /// fall under units there too. `none` designates nothing, not everything.
    pub fn allows(self, kind: AirstrikeKind) -> bool {
        let bit = match kind {
            AirstrikeKind::Infantry => Self::INFANTRY,
            AirstrikeKind::Unit | AirstrikeKind::Aircraft => Self::UNIT,
            AirstrikeKind::Building => Self::BUILDING,
        };
        self.0 & bit.0 != 0
    }
}

#[cfg(test)]
#[path = "airstrike_targets_tests.rs"]
mod tests;
