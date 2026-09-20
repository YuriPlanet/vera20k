//! Object+70 is retained targeting state, independent of actual health+6C.
//!
//! Native identity: TechnoAI6F9F6E..6F9F9F; scanner7099B5 and FireAt6FE622
//! reserve estimated damage separately. See tools/spatial_oracle/estimated_health
//! and the scanner prerequisite packet. No floating-point arithmetic lives here.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct EstimatedHealth(i32);

impl EstimatedHealth {
    pub(crate) const fn from_raw(value: i32) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    /// Full-strength repair/reload and constructor/map/conversion initializers
    /// assign this field explicitly; ordinary actual damage does not.
    pub(crate) fn reset(&mut self, actual_health: i32) {
        self.0 = actual_health;
    }

    /// Radio6F4D4D..5C and Building4508A8..CD add the repair amount to both
    /// retained values. The caller owns any full-strength clamp/reset branch.
    pub(crate) fn add_repair(&mut self, amount: i32) {
        self.0 = self.0.wrapping_add(amount);
    }

    /// Original TechnoAI6F9F6E: signed clamp, then frame-mask recovery. This is
    /// an actor visit, not a once-per-global-tick sweep or a modulo-four pulse.
    pub(crate) fn recover(&mut self, actual_health: i32, binary_frame: u32) {
        self.0 = self.0.min(actual_health);
        if binary_frame & 4 != 0 && self.0 < actual_health {
            if self.0.wrapping_add(30) < 0 {
                self.0 = -30;
            }
            self.0 = self.0.wrapping_add(1);
        }
    }
}

#[cfg(test)]
#[path = "estimated_health_tests.rs"]
mod tests;
