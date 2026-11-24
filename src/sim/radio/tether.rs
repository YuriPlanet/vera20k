//! The tether latch, `TechnoClass+0x418`: a docked pair's "entered" mark.

use serde::{Deserialize, Serialize};

/// `TechnoClass+0x418`. Native keeps a bool; VERA keeps the partner that sent
/// TETHER, so a departing partner can be forgotten.
///
/// Native writers:
/// - the constructor clears it (`0x006F2FA9`);
/// - TETHER (radio `0x18`) sets it (`0x006F4B72`) and UNTETHER (`0x19`)
///   clears it (`0x006F4BA6`), in `TechnoClass::Receive_Radio @ 0x006F4AB0`
///   ([`super::receive`]);
/// - `EventClass::Execute @ 0x004C6CB0`'s MegaMission clears it after the
///   BREAK to a `Refinery=` contact (`0x004C7342`, `mission::retask`);
/// - `BuildingClass::ChangeOwner @ 0x00448260` sets it on both ends of each
///   docked object it takes along (`0x004492B7`, `0x004492C3`), when that
///   object held it before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tether {
    partner: Option<u64>,
}

impl Tether {
    /// The partner whose TETHER set the latch, while it is set.
    pub const fn partner(self) -> Option<u64> {
        self.partner
    }

    /// TETHER's set (`0x006F4B72`), and `BuildingClass::ChangeOwner`'s on
    /// both ends of an object it takes along (`0x004492B7`, `0x004492C3`).
    pub(crate) fn attach(&mut self, partner: u64) {
        self.partner = Some(partner);
    }

    /// UNTETHER's clear (`0x006F4BA6`) and the MegaMission event's
    /// (`0x004C7342`).
    pub(crate) fn release(&mut self) {
        self.partner = None;
    }

    /// Clear a latch set by `expired`, as its BREAK would have. VERA-only:
    /// the store's expiry (`EntityStore::clear_radio_contacts_for`) calls this
    /// for a partner leaving without the BREAK cascade.
    pub(crate) fn forget(&mut self, expired: u64) {
        if self.partner == Some(expired) {
            self.partner = None;
        }
    }

    /// Install a fixture's latch.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_for_test(&mut self, partner: Option<u64>) {
        self.partner = partner;
    }
}
