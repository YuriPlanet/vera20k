//! Test fixtures for refinery dock contacts.
//!
//! A refinery admits up to `NumberOfDocks` miners into its `Contacts[]` list
//! (capacity-1 for a stock refinery). The slot count `RadioClass+0xE8` starts
//! at 1 in the `RadioClass` ctor (0x0065A764) and is then sized by
//! `BuildingClass::Constructor` 0x0043BCBD..0x0043BCD0: `MOV EAX,[Type+0x1780]`
//! (`NumberOfDocks=`), `CMP EAX,1 ; JGE`, else `MOV EAX,1`, `PUSH EAX`,
//! `CALL Set_Contact_Count` — i.e. capacity = `max(NumberOfDocks, 1)`, which is
//! installed by the shared Rust construction owner before any HELLO (stock
//! refineries are `NumberOfDocks=1`). Admission belongs to `Contacts`, and
//! HELLO belongs to `radio::transmit`.
//! gamemd stores **no** wait-queue: a denied miner re-probes on demand and
//! whichever re-probing miner wins a freed slot docks next (V3).
//!
//! The refinery handshake has one record, the radio state on the two objects:
//! the refinery's `GameEntity::radio_contacts` and the miner's tether
//! (`GameEntity::tether`, `TechnoClass+0x418`). The miner FSM and retasking
//! use [`radio::transmit`], and these fixtures drive the same bus. (Other
//! mechanisms write `radio_contacts` for their own links, e.g. a factory
//! exit; none of them touches a refinery.)
//!
//! ## Dependency rules
//! - Part of sim/ -- no dependencies outside sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::world::Simulation;

/// Whether the refinery's Contacts[] list holds the miner.
pub(crate) fn has_contact(sim: &Simulation, refinery_sid: u64, miner_sid: u64) -> bool {
    sim.substrate
        .entities
        .get(refinery_sid)
        .is_some_and(|refinery| refinery.seldom.radio_contacts.contains(miner_sid))
}

/// Test fixtures drive the same bus the FSM does; there is no test-only store.
pub(crate) mod test_support {
    use super::*;

    /// Whether the 0x18 ENTER_DOCK handshake linked this miner to the refinery.
    pub(crate) fn has_entered(sim: &Simulation, refinery_sid: u64, miner_sid: u64) -> bool {
        sim.substrate
            .entities
            .get(miner_sid)
            .is_some_and(|miner| miner.seldom.tether.partner() == Some(refinery_sid))
    }

    /// Whether any miner holds a contact slot of the refinery.
    pub(crate) fn dock_test_is_occupied(sim: &Simulation, refinery_sid: u64) -> bool {
        sim.substrate
            .entities
            .get(refinery_sid)
            .is_some_and(|refinery| !refinery.seldom.radio_contacts.is_empty())
    }
}
