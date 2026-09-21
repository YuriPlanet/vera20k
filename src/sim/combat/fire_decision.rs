//! Per-tick fire-decision outcomes for one attacker.
//!
//! Behavioral subset of gamemd's GetFireError codes (see
//! ra2-rust-game-docs/UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md
//! §4.2). Code 5 (Generic) collapses ~30 binary sub-reasons since they all
//! map to "no fire this tick"; threading sub-reason complexity buys zero
//! observable difference.
//!
//! RESIDUAL (GSI-08.03) — this vocabulary has no producer. There is no
//! `GetFireError` function; the fire gates are scattered early returns across
//! `combat/mod.rs`, `combat_fire_gate.rs` and the attacker snapshot loop, none
//! of which yields a code, so nothing downstream can consume one. That is why
//! the whole enum is ``.
//! - Trigger: any consumer that needs to know *why* a shot did not happen —
//!   gattling spin-up (which native drives from codes {0, 2, 3, 4}), the attack
//!   cursor, and the EVA "cannot deploy/fire" feedback.
//! - Player effect: gattling weapons do not spin up on the near-miss codes, so
//!   a Gattling Cannon reaches full rate differently from retail; the other
//!   consumers fall back to coarser conditions.
//! - Frequency: every gattling engagement; the cursor and EVA arms are
//!   whenever the player targets something illegal.
//! - Downstream risk: two gates the same native step applies are also absent
//!   and recorded at their call site in `combat/mod.rs` —
//!   `TechnoClass::IsOnBridge_ForFiring @ 0x00703B10` and the vtable `+0x380`
//!   test, both yielding error 6. Introducing codes means routing every early
//!   return through one function, which is a refactor of the whole fire path
//!   rather than an addition to it.
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on standard library.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[allow(dead_code)] // Staged GetFireError vocabulary for the gattling/fire-gate handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireDecision {
    Fire,
    Cooldown,
    Facing,
    Range,
    NoAmmo,
    CloakedTarget,
    ForceFire,
    Generic,
}

#[allow(dead_code)] // The staged decision consumers are not runtime-wired yet.
impl FireDecision {
    /// Whether this decision drives gattling-weapon spin-up (gamemd codes
    /// {0, 2, 3, 4} per research doc §4.8). Code 4 is unmapped in our enum;
    /// we approximate with Generic since it covers "rotation/cooldown-related
    /// no-fire" cases.
    #[cfg(test)]
    pub fn drives_gattling_spinup(self) -> bool {
        matches!(
            self,
            Self::Fire | Self::Facing | Self::Cooldown | Self::Generic
        )
    }

    /// Whether this decision means "fire happens this tick".
    #[cfg(test)]
    pub fn is_fire(self) -> bool {
        matches!(self, Self::Fire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_gattling_spinup_truth_table() {
        assert!(FireDecision::Fire.drives_gattling_spinup());
        assert!(FireDecision::Facing.drives_gattling_spinup());
        assert!(FireDecision::Cooldown.drives_gattling_spinup());
        assert!(FireDecision::Generic.drives_gattling_spinup());

        assert!(!FireDecision::Range.drives_gattling_spinup());
        assert!(!FireDecision::NoAmmo.drives_gattling_spinup());
        assert!(!FireDecision::CloakedTarget.drives_gattling_spinup());
        assert!(!FireDecision::ForceFire.drives_gattling_spinup());
    }

    #[test]
    fn is_fire_only_for_fire_variant() {
        assert!(FireDecision::Fire.is_fire());
        assert!(!FireDecision::Facing.is_fire());
        assert!(!FireDecision::ForceFire.is_fire());
        assert!(!FireDecision::Cooldown.is_fire());
    }
}
