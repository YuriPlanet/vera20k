//! Base Techno GetPipFillLevel708C30, including Unit740E50's direct thunk.
//! Building44D700 has a distinct House-storage Tiberium override; this base
//! query does not reproduce that override. No RNG, timers or state writes.

use super::GameEntity;
use crate::rules::object_type::{ObjectType, PipScale};
use crate::rules::ruleset::RuleSet;
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

/// Ordered numeric kernel shared by708D15..708D2A and the independent
/// Tiberium draw quantities70A230..70A29C. Native PC53/chop division,
/// multiplication and addition precede ftol7C5F00; the caller consumes only
/// its low signed dword, including masked invalid/overflow conversions.
/// Each caller retains its own minimum or drawing bounds.
pub fn rounded_pip_quantity(amount: i32, denominator: i32, maximum: i32) -> i32 {
    X87::ftol_i32_low_masked(X87::add(
        X87::mul(
            X87::div(X87::load_i32(amount), X87::load_i32(denominator)),
            X87::load_i32(maximum),
        ),
        X87::load_f64(NativeF64Bits::from_bits(0x3fe0_0000_0000_0000)),
    ))
}

impl GameEntity {
    /// The whole bales in this object's Storage (Techno `+0x33C`): the
    /// Miner's hold, or, without one, the SlaveLink's. Native keeps one
    /// Storage per Techno, which both the Harvest missions fill (Unit and
    /// Infantry `Mission_Harvest`) and the pips read.
    pub fn storage_bales(&self) -> &[crate::sim::miner::CargoBale] {
        self.miner
            .as_ref()
            .map_or_else(|| self.slave.cargo(), |miner| miner.cargo())
    }

    /// Base GetPipFillLevel708C30, virtual+2B8. This reads the existing
    /// ammo, Cargo count, capture-manager and miner authorities; it creates
    /// no derived status state. The result is signed and is not capped at
    /// Type.GetPipMax716290.
    ///
    /// `controlled_count` is this controller's `GetControlledCount`
    /// (0x004722D0, the PipScale=MindControl arm's only native caller at
    /// 0x00708C88): [`crate::sim::world::Simulation::capture_effective_count`],
    /// which Phobos makes the sum of `MindControlSize=` when
    /// `MindControl.IgnoreSize=` is false. `None` reads the node count.
    pub fn techno_pip_fill_level(
        &self,
        object: &ObjectType,
        rules: &RuleSet,
        controlled_count: Option<i32>,
    ) -> i32 {
        let maximum = object.pip_max(rules);
        let (amount, denominator) = match object.pip_scale {
            PipScale::Ammo => (self.ammo().map_or(0, |ammo| ammo.current()), maximum),
            PipScale::Passengers => (
                self.passenger_role
                    .cargo()
                    .map_or(0, |cargo| cargo.count() as i32),
                maximum,
            ),
            PipScale::MindControl => {
                let Some(manager) = self.capture_manager.as_ref() else {
                    return 0;
                };
                (
                    controlled_count.unwrap_or_else(|| manager.count()),
                    maximum,
                )
            }
            PipScale::Tiberium => {
                // Storage6C9650 repeatedly truncates its four float slots to
                // an i32 accumulator and returns that integer as ST0. The
                // existing Miner and (when absent) SlaveLink store whole
                // bales. Slave harvest pushes a level and deposit takes the
                // hold; losing its master does not empty it. Raw fractional
                // or negative slots and Weeder cargo production remain
                // unavailable state. Custom simultaneous Miner/SlaveLink
                // storage is not represented as one native hold.
                (self.storage_bales().len() as i32, object.storage)
            }
            PipScale::None | PipScale::Power => return 0,
        };
        let quantity = rounded_pip_quantity(amount, denominator, maximum);
        //708D2D..708D4C tests the unrounded amount only when EAX is zero.
        // Positive/zero-denominator controls also take this minimum, after
        // ftol's invalid signed64 result has supplied a zero low dword.
        if quantity == 0 && amount > 0 {
            1
        } else {
            quantity
        }
    }
}

#[cfg(test)]
#[path = "pip_fill_tests.rs"]
mod tests;
