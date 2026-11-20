//! `Phobos FlyingStrings` value carried from simulation owners to the client
//! display: a consumed signed credit label. Ares' bounty payout and Phobos'
//! `DisplaySoylent` both emit it; display rejection cannot undo the wallet.
//!
//! ## Dependency rules
//! - Part of sim/ — presentation consumes it, never the reverse.

use crate::sim::projectile::ProjectileCoord;

/// A consumed signed credit label, before the client's shroud/visual-character
/// checks. Display rejection cannot undo the wallet or requeue the amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BountyText {
    /// The anchor object. `None` for a label with no source object, such as
    /// Phobos `FlyingStrings::AddMoneyString`'s `pSource = nullptr` warhead
    /// `TransactMoney` label: it has coordinates only and skips the source's
    /// visual-character test.
    pub entity_id: Option<u64>,
    pub amount: i32,
    /// Unraised virtual+48 point: IsShrouded586360 reads its height too.
    pub coord: ProjectileCoord,
    pub z_offset: i32,
    /// Phobos `FlyingStrings::AddMoneyString`'s `displayToHouses` filter;
    /// `None` keeps the bounty path's unconditional delivery.
    pub display_to: Option<crate::rules::phobos_shields::AffectedHouse>,
    /// Phobos `FlyingStrings::AddMoneyString`'s `pOwner`: the House the
    /// `displayToHouses` filter compares with the viewer. `None` uses the
    /// anchor object's owner; the drain target label passes the drainer's
    /// House while the anchor stays the drained building.
    pub display_owner: Option<crate::sim::intern::InternedId>,
    /// Screen-pixel offset added after the centred anchor (Phobos
    /// `DisplaySoylentOffset`); zero for the bounty path.
    pub pixel_offset: [i32; 2],
}
