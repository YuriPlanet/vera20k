//! ProcessNetwork6475F0's command packets and peer progress. Simulation owns
//! both event queues; this owner keeps only codec, in-flight packet and peer
//! transport bookkeeping. Reliable delivery is supplied by the connection owner.

use crate::sim::command_queue::FrameInfo;
use crate::sim::world::{NetworkCommandQueues, NetworkQueueError};

use super::event_packet::{
    EncodeBlock, EventPacketCodec, EventPacketHeader, NATIVE_EVENT_PACKET_CAPACITY,
    PacketDecodeError, PacketEncodeError, decode_frame_info, decode_frame_sync,
    encode_live_frame_sync, live_command_frontier,
};
use super::lockstep::NetworkSendPolicy;

#[cfg(test)]
#[path = "command_exchange_tests.rs"]
mod tests;

/// Receive64A0C0's 24-byte peer row, represented only by the fields this
/// command path owns. Connection ordering comes from the admitted session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPeerProgress {
    house_id: i8,
    frame: i32,
    advertised_commands: u64,
    received_commands: u64,
    initial_response: bool,
    ///Peer+C: last FRAMESYNC response relative to the current wait entry.
    response_elapsed_buckets: u32,
    ///Peer+14: raw timeGetTime at Wait entry (648881..64889E).
    bar_reference_millis: u32,
    lag_frames: i32,
}

impl CommandPeerProgress {
    fn new(house_id: i8) -> Self {
        Self {
            house_id,
            frame: -1,
            advertised_commands: 0,
            received_commands: 0,
            initial_response: false,
            response_elapsed_buckets: 0,
            bar_reference_millis: 0,
            lag_frames: 0,
        }
    }

    pub const fn house_id(&self) -> i8 {
        self.house_id
    }
    pub const fn frame(&self) -> i32 {
        self.frame
    }
    pub const fn advertised_commands(&self) -> u64 {
        self.advertised_commands
    }
    pub const fn received_commands(&self) -> u64 {
        self.received_commands
    }

    pub const fn lag_frames(&self) -> i32 {
        self.lag_frames
    }

    ///64A861..64A927: keep the unsigned bucket lag separate from the
    ///wrapping millisecond age used by the blue-bar branch.
    pub fn bar_timing(&self, wait_elapsed_buckets: u32, now_millis: u32) -> (u32, u32) {
        (
            wait_elapsed_buckets.wrapping_sub(self.response_elapsed_buckets),
            now_millis
                .wrapping_sub(self.bar_reference_millis)
                .wrapping_mul(60)
                / 1000,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameWait {
    ready: bool,
    /// First connection whose decoded count trails its advertised frontier.
    pub missing_commands: Option<i8>,
    /// First minimum-frame connection, only when it actually blocks this frame.
    pub missing_frame: Option<i8>,
}

impl FrameWait {
    pub const fn ready(self) -> bool {
        self.ready
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandExchangeError {
    #[error("invalid or duplicate connection House {0}")]
    InvalidPeer(i8),
    #[error("command packet came from unregistered connection House {0}")]
    UnknownPeer(i8),
    #[error("a command packet requires a complete FRAMEINFO or FRAMESYNC header")]
    InvalidHeader,
    #[error("connection House {source_house} claimed event House {claimed_house}")]
    SourceMismatch { source_house: i8, claimed_house: i8 },
    #[error("peer {house_id} initial checksum {received:08x} differs from {expected:08x}")]
    InitialChecksum {
        house_id: i8,
        received: u32,
        expected: u32,
    },
    #[error(transparent)]
    Queue(#[from] NetworkQueueError),
    #[error(transparent)]
    Encode(#[from] PacketEncodeError),
    #[error(transparent)]
    Decode(#[from] PacketDecodeError),
    #[error("local packet encoder stopped at {0:?}")]
    UnsupportedLocalEvent(EncodeBlock),
    #[error("a retained network send cannot change frames")]
    UnfinishedSend,
    #[error("the multiplayer command counter overflowed")]
    CounterOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendPoll {
    /// Only the interface service runs off the protocol-2 send boundary.
    OffCadence,
    /// Keep the same post-Logic frame while the reliable owner makes room.
    Backpressure,
    Complete {
        commands: usize,
    },
}

/// The native packet cache and progress rows have one process-session owner.
/// There is deliberately no independently dispatched command collection here.
#[derive(Debug, Default)]
pub struct CommandExchange {
    codec: EventPacketCodec,
    peers: Vec<CommandPeerProgress>,
    sent_commands: u64,
    sending: Option<SendingFrame>,
}

#[derive(Debug, Clone, Copy)]
struct SendingFrame {
    frame: u32,
    remaining: usize,
    total: usize,
    frontier: u64,
    header: FrameInfo,
}

/// Read-only projection of the retained transaction; no encoded bytes or
/// independently maintained send/backpressure state escape the exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedSendProgress {
    pub frame: u32,
    pub remaining: usize,
    pub total: usize,
    pub frontier: u64,
}

impl CommandExchange {
    pub fn new(peers: impl IntoIterator<Item = i8>) -> Result<Self, CommandExchangeError> {
        let mut exchange = Self::default();
        for house in peers {
            if house < 0 || exchange.peers.iter().any(|peer| peer.house_id == house) {
                return Err(CommandExchangeError::InvalidPeer(house));
            }
            exchange.peers.push(CommandPeerProgress::new(house));
        }
        Ok(exchange)
    }

    pub fn peers(&self) -> &[CommandPeerProgress] {
        &self.peers
    }
    pub const fn sent_commands(&self) -> u64 {
        self.sent_commands
    }

    pub fn sending(&self) -> bool {
        self.sending.is_some()
    }

    pub fn retained_send(&self) -> Option<RetainedSendProgress> {
        self.sending.map(|sending| RetainedSendProgress {
            frame: sending.frame,
            remaining: sending.remaining,
            total: sending.total,
            frontier: sending.frontier,
        })
    }

    /// UpdatePeerFrameLag652187..6521B2: only after a source advances its
    /// authenticated frame. The measured Logic rate belongs to MainThrottle,
    /// and RTT to the existing private connection response history.
    pub fn record_peer_lag(&mut self, source: i8, frame: u32, logic_rate: u32, response: u32) {
        if let Some(peer) = self.peers.iter_mut().find(|peer| peer.house_id == source) {
            let transit_frames = logic_rate.wrapping_mul(response >> 1) / 60;
            peer.lag_frames = (frame as i32)
                .wrapping_sub(transit_frames as i32)
                .wrapping_sub(peer.frame);
        }
    }

    ///647825..6478CB resets progress and all packet caches. Reconnection after
    /// a terminal decode failure requires this reset, not a retry of its prefix.
    pub fn reset(&mut self) {
        self.codec.reset();
        for peer in &mut self.peers {
            *peer = CommandPeerProgress::new(peer.house_id);
        }
        self.sent_commands = 0;
        self.sending = None;
    }

    pub fn remove_peer(&mut self, house_id: i8) -> bool {
        let Some(index) = self.peers.iter().position(|peer| peer.house_id == house_id) else {
            return false;
        };
        self.peers.remove(index);
        true
    }

    pub fn initial_ready(&self) -> bool {
        self.peers.iter().all(|peer| peer.initial_response)
    }

    ///647B1E clears just each peer's frame after the initial response barrier.
    /// Advertised/decoded counts received during that wait remain intact.
    pub fn finish_initial_barrier(&mut self) {
        for peer in &mut self.peers {
            peer.frame = 0;
        }
    }

    ///648881..6488AE writes one raw millisecond sample to every peer+14.
    ///Recovery restarts countdowns without entering this function again.
    pub fn begin_wait(&mut self, now_millis: u32) {
        for peer in &mut self.peers {
            peer.bar_reference_millis = now_millis;
        }
    }

    ///64A000: the probe uses the scenario's initial sync value, whereas ordinary
    /// FRAMEINFO carries the current post-Logic checksum. Both use MaxAhead's
    /// low byte for delay and the same independently rounded execute frame.
    pub fn frame_probe(
        &self,
        house_id: i8,
        frame: u32,
        max_ahead: u32,
        initial_checksum: u32,
        policy: NetworkSendPolicy,
    ) -> Result<Vec<u8>, CommandExchangeError> {
        let frontier = self
            .sending
            .map_or(self.sent_commands, |sending| sending.frontier);
        Ok(encode_live_frame_sync(
            FrameInfo {
                house_id,
                event_frame: policy.execute_frame(frame, max_ahead) as i32,
                checksum: initial_checksum,
                timing_word: frontier as u16,
                delay: policy.delay(frame, max_ahead),
            },
            frontier,
        )?)
    }

    /// SendOutList649CA0's codec and queue transfer, extended for large VERA
    /// batches. A rejected broadcast changes neither codec cache, local FIFO,
    /// mirror queue nor committed counter. The caller resumes this same frame
    /// after servicing receives and ACKs through the reliable transport owner.
    pub fn send_frame(
        &mut self,
        queues: &mut NetworkCommandQueues<'_>,
        house_id: i8,
        frame: u32,
        max_ahead: u32,
        checksum: u32,
        policy: NetworkSendPolicy,
        mut broadcast_and_service: impl FnMut(&[u8], bool) -> bool,
    ) -> Result<SendPoll, CommandExchangeError> {
        if !policy.should_send(frame) {
            return Ok(SendPoll::OffCadence);
        }
        if let Some(sending) = &self.sending {
            if sending.frame != frame {
                return Err(CommandExchangeError::UnfinishedSend);
            }
        } else {
            let total = queues.local_len();
            let frontier = self
                .sent_commands
                .checked_add(total as u64)
                .ok_or(CommandExchangeError::CounterOverflow)?;
            self.sending = Some(SendingFrame {
                frame,
                total,
                remaining: total,
                frontier,
                header: FrameInfo {
                    house_id,
                    event_frame: policy.execute_frame(frame, max_ahead) as i32,
                    checksum,
                    timing_word: frontier as u16,
                    delay: policy.delay(frame, max_ahead),
                },
            });
        }
        loop {
            let sending = self.sending.expect("retained frame");
            if sending.remaining > queues.mirror_allowance() {
                return Ok(SendPoll::Backpressure);
            }
            let records = queues.local_records()?;
            let records = &records[..records.len().min(sending.remaining)];
            let Some(encoded) = self.codec.encode_packet_if_accepted(
                records,
                EventPacketHeader::Vera {
                    frame_info: sending.header,
                    command_frontier: sending.frontier,
                },
                frame as i32,
                NATIVE_EVENT_PACKET_CAPACITY,
                |packet| broadcast_and_service(&packet.bytes, packet.consumed_records() != 0),
            )?
            else {
                return Ok(SendPoll::Backpressure);
            };
            let count = encoded.consumed_records();
            queues.commit_local_mirrors(&encoded.mirrors)?;
            self.sent_commands = self
                .sent_commands
                .checked_add(count as u64)
                .ok_or(CommandExchangeError::CounterOverflow)?;
            self.sending.as_mut().unwrap().remaining -= count;
            if let Some(reason) = encoded
                .blocked
                .filter(|reason| *reason != EncodeBlock::Capacity)
            {
                return Err(CommandExchangeError::UnsupportedLocalEvent(reason));
            }
            if self.sending.as_ref().unwrap().remaining == 0 {
                self.sending = None;
                return Ok(SendPoll::Complete {
                    commands: sending.total,
                });
            }
            if count == 0 {
                return Ok(SendPoll::Backpressure);
            }
        }
    }

    /// Receive64A0C0 updates peer progress before decoding; queue overflow
    /// preserves admitted prefix but must not advance the decoded count.
    /// `source_house` comes from the reliable connection's identity, not from
    /// the packet's claimed House. Native decoder still inherits that header.
    pub fn receive(
        &mut self,
        source_house: i8,
        packet: &[u8],
        initial_checksum: u32,
        queues: &mut NetworkCommandQueues<'_>,
        wait_elapsed_buckets: u32,
    ) -> Result<bool, CommandExchangeError> {
        let peer = self
            .peers
            .iter_mut()
            .find(|peer| peer.house_id == source_house)
            .ok_or(CommandExchangeError::UnknownPeer(source_house))?;
        let info = decode_frame_info(packet)
            .or_else(|| decode_frame_sync(packet))
            .ok_or(CommandExchangeError::InvalidHeader)?;
        // VERA transport security: original64A0C0 trusts this header. Our
        // authenticated endpoint supplies its House, so a peer cannot submit
        // orders or checksum progress under another participant's identity.
        if info.house_id != source_house {
            return Err(CommandExchangeError::SourceMismatch {
                source_house,
                claimed_house: info.house_id,
            });
        }
        let frontier = live_command_frontier(packet).ok_or(CommandExchangeError::InvalidHeader)?;
        peer.advertised_commands = peer.advertised_commands.max(frontier);
        let reported_frame = info.event_frame.wrapping_sub(i32::from(info.delay));
        let advanced = reported_frame > peer.frame;
        if advanced {
            peer.initial_response |= peer.frame == -1;
            peer.frame = reported_frame;
        }
        if decode_frame_sync(packet).is_some() {
            if info.checksum != initial_checksum {
                return Err(CommandExchangeError::InitialChecksum {
                    house_id: source_house,
                    received: info.checksum,
                    expected: initial_checksum,
                });
            }
            //64A606..64A60D, after the seed comparison. Ordinary FRAMEINFO
            //does not refresh this field. Router MEGAFRAMEINFO is separate.
            peer.response_elapsed_buckets = wait_elapsed_buckets;
            return Ok(advanced);
        }
        let decoded = self
            .codec
            .decode_packet(packet, |record| queues.admit_received(record))?;
        peer.received_commands = peer
            .received_commands
            .checked_add(decoded.command_count(packet) as u64)
            .ok_or(CommandExchangeError::CounterOverflow)?;
        Ok(advanced)
    }

    /// Wait_For_Players648710's two independent gates at649595..64961F.
    /// Both frame comparisons are signed; minimum starts at frame+1000 and
    /// ties retain the first peer. VERA retains the full unsigned u64 frontier
    /// instead of the native packet's wrapping low16 projection.
    pub fn frame_wait(&self, frame: i32, max_ahead: i32) -> FrameWait {
        let mut minimum = frame.wrapping_add(1000);
        let mut slowest = None;
        for peer in &self.peers {
            if peer.frame < minimum {
                minimum = peer.frame;
                slowest = Some(peer.house_id);
            }
        }
        let missing_commands = self
            .peers
            .iter()
            .find(|peer| peer.received_commands < peer.advertised_commands)
            .map(|peer| peer.house_id);
        let has_frame_credit = frame < minimum.wrapping_add(max_ahead);
        FrameWait {
            ready: missing_commands.is_none() && has_frame_credit,
            missing_commands,
            missing_frame: if has_frame_credit { None } else { slowest },
        }
    }
}
