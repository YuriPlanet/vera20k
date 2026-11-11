//! Base Techno GetPipFillLevel708C30, including Unit740E50's direct thunk.
//! Building44D700 overrides it for PipScale=Tiberium only: a Weeder reads the
//! House's weed storage (`0x0044D71F..0x0044D754`; no retail type sets
//! `Weeder=`) and any other building answers 0. DrawPipScalePips draws no
//! Tiberium pips for a Building (`0x00709B4E..0x00709B68`), so this base query
//! does not reproduce that override. No RNG or timers; the one state
//! write is [`GameEntity::add_storage_bales`], AddAmount on the hold the
//! Tiberium pips read.

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

    /// StorageClass::AddAmount (`0x006C9690`) on the Storage that
    /// [`Self::storage_bales`] reads. The Full reinforcement fill
    /// (CreateMembers `0x0065DE7B`) is its whole-hold caller.
    pub(crate) fn add_storage_bales(&mut self, bale: crate::sim::miner::CargoBale, amount: u16) {
        match self.miner.as_mut() {
            Some(miner) => miner.add_bales(bale, amount),
            None => self.slave.add_bales(bale, amount),
        }
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
                (controlled_count.unwrap_or_else(|| manager.count()), maximum)
            }
            PipScale::Tiberium => {
                // Storage6C9650 repeatedly truncates its four float slots to
                // an i32 accumulator and returns that integer as ST0. The
                // existing Miner and (when absent) SlaveLink store whole
                // bales. Slave harvest pushes a level and deposit takes the
                // hold; losing its master does not empty it. Raw fractional
                // or negative slots and Weeder cargo production remain
                // unavailable state. Custom simultaneous Miner/SlaveLink
                // storage is not represented as one native hold: an infantry
                // type with Harvester= or Enslaves= and Storage>0 gets a Miner
                // (construction has no category gate), so a Full team fills
                // that hold while Infantry Mission_Harvest (0x00522E70) here
                // reads and fills the SlaveLink one. Its pips show full and it
                // still digs up to Storage more levels, where native sees the
                // load at 1.0 and goes to Guard. Retail has no such infantry.
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
