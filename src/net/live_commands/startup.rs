//! VERA startup certificate over one immutable install-time admission.
//! Native Receive64A0C0 marks first FRAMESYNC64A3C9 and checks only the seed
//! 64A5DD..5E8; Wait_For_Players648710 tallies against a changing node count649148..1F7.
//! UDP/relay adaptation: certify the same frozen active set and canonical
//! initial state before allowing that native barrier to release. Source:
//! original gamemd.exe instructions. This is not a native consensus protocol.

use crate::net::reliable::Delivery;
use std::num::NonZeroU32;

const MAGIC: &[u8; 4] = b"VSA1";
const HELLO: u8 = 1;
const READY: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartupError {
    #[error("invalid original or active multiplayer admission")]
    InvalidAdmission,
    #[error("initial active roster is not a majority of the original admission")]
    NoMajority,
    #[error("startup control does not match this immutable admission")]
    Mismatch,
    #[error("startup lost a required participant before agreement")]
    Aborted,
}

/// Immutable protocol snapshot, not another simulation/membership owner.
/// Pairs bind shared raw setup row to constructed House. LAN source vectors
/// are local-first, so their ordinal or socket address must never be hashed.
/// Active rows are fixed at install after the native loading wait.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StartupAgreement {
    houses: Vec<(u8, i8)>,
    active: u32,
    seed: u32,
    max_ahead: i32,
    fps: u16,
    send_rate: u8,
    world_hash: u64,
}

impl StartupAgreement {
    pub fn new(
        houses: impl IntoIterator<Item = (u8, i8)>,
        active_rows: impl IntoIterator<Item = u8>,
        seed: u32,
        max_ahead: i32,
        fps: u16,
        frame_send_rate: NonZeroU32,
        world_hash: u64,
    ) -> Result<Self, StartupError> {
        let mut houses: Vec<_> = houses.into_iter().collect();
        houses.sort_unstable_by_key(|&(row, _)| row);
        let mut seen_houses = 0u32;
        let mut original = 0u32;
        if houses.is_empty()
            || houses.len() > crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT
            || max_ahead < 0
            || fps == 0
            || frame_send_rate.get() > u32::from(u8::MAX)
        {
            return Err(StartupError::InvalidAdmission);
        }
        for &(row, house) in &houses {
            if usize::from(row) >= crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT
                || original & (1u32 << row) != 0
                || !(0..32).contains(&house)
                || seen_houses & (1u32 << house) != 0
            {
                return Err(StartupError::InvalidAdmission);
            }
            original |= 1u32 << row;
            seen_houses |= 1u32 << house;
        }
        let mut active = 0u32;
        for row in active_rows {
            if row >= 32 || original & (1u32 << row) == 0 || active & (1u32 << row) != 0 {
                return Err(StartupError::InvalidAdmission);
            }
            active |= 1u32 << row;
        }
        // A full original one-human LAN game (with AI) remains native. Strict
        // original majorities intersect, so two different survivor sets cannot
        // both certify: an ordinary VERA identity emits only its one immutable
        // proposal in this fresh scope. This is not Byzantine consensus. A
        // narrowed singleton/minority may never elect itself into a match.
        let count = active.count_ones() as usize;
        if count * 2 <= houses.len() || (houses.len() > 1 && count < 2) {
            return Err(StartupError::NoMajority);
        }
        Ok(Self {
            houses,
            active,
            seed,
            max_ahead,
            fps,
            send_rate: frame_send_rate.get() as u8,
            world_hash,
        })
    }

    pub(super) fn is_original_singleton(&self) -> bool {
        self.houses.len() == 1
    }

    /// The relay clock requires the entire originally certified membership.
    /// A loading-survivor majority cannot start a different timeline.
    pub(super) fn sequencing_identity(&self) -> Result<([u8; 32], u16, usize), StartupError> {
        if self
            .houses
            .iter()
            .any(|(row, _)| self.active & (1 << row) == 0)
        {
            return Err(StartupError::InvalidAdmission);
        }
        Ok((
            crate::util::sha256::sha256_bytes(&self.identity()),
            self.fps,
            self.houses.len(),
        ))
    }

    fn row_for_house(&self, house: i8) -> Option<u8> {
        self.houses
            .iter()
            .find(|&&(_, candidate)| candidate == house)
            .map(|&(row, _)| row)
            .filter(|&row| self.active & (1u32 << row) != 0)
    }

    fn identity(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24 + 2 * self.houses.len());
        bytes.push(self.houses.len() as u8);
        for &(row, house) in &self.houses {
            bytes.extend([row, house as u8]);
        }
        bytes.extend(self.active.to_le_bytes());
        bytes.extend(self.seed.to_le_bytes());
        bytes.extend(self.max_ahead.to_le_bytes());
        bytes.extend(self.fps.to_le_bytes());
        bytes.push(self.send_rate);
        bytes.extend(self.world_hash.to_le_bytes());
        bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pending,
    Released,
    Failed,
}

/// Sole handshake state, retained by the existing LiveCommandSession. The
/// reliability owner retains every queued control until its own ACK retires it;
/// these flags mean queue admission, never delivery or a second retry queue.
#[derive(Debug)]
pub(super) struct StartupHandshake {
    agreement: StartupAgreement,
    local_row: u8,
    hello_sent: bool,
    ready_sent: bool,
    hello: u32,
    ready: u32,
    phase: Phase,
}

impl StartupHandshake {
    pub(super) fn new(
        agreement: StartupAgreement,
        local_house: i8,
        peers: impl IntoIterator<Item = i8>,
        seed: u32,
        rate: NonZeroU32,
    ) -> Result<Self, StartupError> {
        let local_row = agreement
            .row_for_house(local_house)
            .ok_or(StartupError::InvalidAdmission)?;
        let mut connected = 1u32 << local_row;
        for house in peers {
            let source = agreement
                .row_for_house(house)
                .ok_or(StartupError::InvalidAdmission)?;
            if connected & (1u32 << source) != 0 {
                return Err(StartupError::InvalidAdmission);
            }
            connected |= 1u32 << source;
        }
        if connected != agreement.active
            || agreement.seed != seed
            || u32::from(agreement.send_rate) != rate.get()
        {
            return Err(StartupError::InvalidAdmission);
        }
        Ok(Self {
            agreement,
            local_row,
            hello_sent: false,
            ready_sent: false,
            hello: 1u32 << local_row,
            ready: 1u32 << local_row,
            phase: Phase::Pending,
        })
    }

    pub(super) fn agreement(&self) -> &StartupAgreement {
        &self.agreement
    }

    pub(super) fn pending(&self) -> bool {
        self.phase == Phase::Pending
    }
    pub(super) fn failed(&self) -> bool {
        self.phase == Phase::Failed
    }
    pub(super) fn released(&self) -> bool {
        self.phase == Phase::Released
    }
    pub(super) fn abort(&mut self) {
        if self.pending() {
            self.phase = Phase::Failed;
        }
    }

    pub(super) fn flush(&mut self, mut send: impl FnMut(&[u8]) -> bool) {
        if !self.pending() {
            return;
        }
        if !self.hello_sent {
            let bytes = self.control(HELLO);
            if !send(&bytes) {
                return;
            }
            self.hello_sent = true;
        }
        if self.hello == self.agreement.active && !self.ready_sent {
            let bytes = self.control(READY);
            if send(&bytes) {
                self.ready_sent = true;
            }
        }
    }

    pub(super) fn control(&self, kind: u8) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(30 + 2 * self.agreement.houses.len());
        bytes.extend(MAGIC);
        bytes.push(kind);
        bytes.push(self.local_row);
        bytes.extend(self.agreement.identity());
        bytes
    }

    pub(super) fn receive(
        &mut self,
        house: i8,
        payload: &[u8],
        delivery: Delivery,
    ) -> Result<bool, StartupError> {
        if !payload.starts_with(MAGIC) {
            return Ok(false);
        }
        if self.failed() {
            return Err(StartupError::Aborted);
        }
        let source = self
            .agreement
            .row_for_house(house)
            .ok_or(StartupError::Mismatch)?;
        if delivery != Delivery::Reliable
            || payload.get(5) != Some(&source)
            || payload.get(6..) != Some(self.agreement.identity().as_slice())
        {
            self.phase = Phase::Failed;
            return Err(StartupError::Mismatch);
        }
        match payload.get(4).copied() {
            Some(HELLO) => self.hello |= 1u32 << source,
            Some(READY) if self.hello & (1u32 << source) != 0 => self.ready |= 1u32 << source,
            _ => {
                self.phase = Phase::Failed;
                return Err(StartupError::Mismatch);
            }
        }
        Ok(true)
    }

    pub(super) fn complete(&self) -> bool {
        self.phase == Phase::Released
            || (self.pending()
                && self.hello_sent
                && self.ready_sent
                && self.hello == self.agreement.active
                && self.ready == self.agreement.active)
    }

    pub(super) fn release(&mut self) {
        assert!(
            self.complete(),
            "only a complete frozen admission may release startup"
        );
        self.phase = Phase::Released;
    }
}
