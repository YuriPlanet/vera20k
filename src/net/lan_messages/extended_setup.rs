//! Bounded VERA setup fragmentation over the existing reliable global channel.
//! Native 455-byte message codecs remain available. VERA full setup messages
//! use the same channel, endpoint authority and retry owner; only their payload
//! is split here. Reassembly is discarded with the owning lobby visit.

use super::{LAN_MESSAGE_LEN, SessionScope};

pub const MAGIC: &[u8; 4] = b"VS31";
pub const MAX_BYTES: usize = 32 * 1024;
const HEADER: usize = 37;
const CHUNK: usize = LAN_MESSAGE_LEN - HEADER;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupKind {
    Options,
    Roster,
}

impl SetupKind {
    fn index(self) -> usize {
        match self {
            Self::Options => 0,
            Self::Roster => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid extended LAN setup fragment")]
pub struct FragmentError;

pub fn encode(
    kind: SetupKind,
    scope: SessionScope,
    bytes: &[u8],
) -> Result<Vec<Vec<u8>>, FragmentError> {
    if scope.epoch == [0; 16] || scope.revision == 0 || bytes.is_empty() || bytes.len() > MAX_BYTES
    {
        return Err(FragmentError);
    }
    Ok(bytes
        .chunks(CHUNK)
        .enumerate()
        .map(|(index, chunk)| {
            let mut output = Vec::with_capacity(HEADER + chunk.len());
            output.extend_from_slice(MAGIC);
            output.push(kind.index() as u8);
            output.extend_from_slice(&scope.encode());
            output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            output.extend_from_slice(&(index as u32).to_le_bytes());
            output.extend_from_slice(chunk);
            output
        })
        .collect())
}

/// Read the admission scope before allocating any reassembly storage.
pub fn fragment_scope(bytes: &[u8]) -> Result<SessionScope, FragmentError> {
    if bytes.len() <= HEADER || bytes.len() > LAN_MESSAGE_LEN || &bytes[..4] != MAGIC {
        return Err(FragmentError);
    }
    let scope = SessionScope::decode(&bytes[5..29]).ok_or(FragmentError)?;
    if scope.epoch == [0; 16] || scope.revision == 0 {
        return Err(FragmentError);
    }
    Ok(scope)
}

struct Pending {
    generation: u64,
    started: u32,
    length: usize,
    parts: Vec<Option<Vec<u8>>>,
}

#[derive(Default)]
pub struct Reassembly {
    pending: [Option<Pending>; 2],
    completed: [u64; 2],
}

impl Reassembly {
    /// Call only after checking the admitted host endpoint. At most one 32 KiB
    /// message per kind is retained, for 30 seconds of wall-clock buckets.
    pub fn receive(
        &mut self,
        bytes: &[u8],
        now: u32,
    ) -> Result<Option<(SetupKind, Vec<u8>)>, FragmentError> {
        let scope = fragment_scope(bytes)?;
        let kind = match bytes[4] {
            0 => SetupKind::Options,
            1 => SetupKind::Roster,
            _ => return Err(FragmentError),
        };
        let generation = scope.revision;
        let length =
            u32::from_le_bytes(bytes[29..33].try_into().map_err(|_| FragmentError)?) as usize;
        let index =
            u32::from_le_bytes(bytes[33..37].try_into().map_err(|_| FragmentError)?) as usize;
        if generation == 0 || length == 0 || length > MAX_BYTES {
            return Err(FragmentError);
        }
        let count = length.div_ceil(CHUNK);
        if index >= count || bytes.len() - HEADER != (length - index * CHUNK).min(CHUNK) {
            return Err(FragmentError);
        }
        let slot = kind.index();
        if generation <= self.completed[slot] {
            return Ok(None);
        }
        if self.pending[slot]
            .as_ref()
            .is_some_and(|pending| now.wrapping_sub(pending.started) > 1875)
        {
            self.pending[slot] = None;
        }
        if self.pending[slot]
            .as_ref()
            .is_some_and(|pending| pending.generation > generation)
        {
            return Ok(None);
        }
        if self.pending[slot]
            .as_ref()
            .is_none_or(|pending| pending.generation != generation)
        {
            self.pending[slot] = Some(Pending {
                generation,
                started: now,
                length,
                parts: vec![None; count],
            });
        }
        let pending = self.pending[slot].as_mut().expect("fragment initialized");
        if pending.length != length {
            return Err(FragmentError);
        }
        if let Some(previous) = &pending.parts[index] {
            if previous != &bytes[HEADER..] {
                return Err(FragmentError);
            }
        } else {
            pending.parts[index] = Some(bytes[HEADER..].to_vec());
        }
        if pending.parts.iter().any(Option::is_none) {
            return Ok(None);
        }
        let pending = self.pending[slot].take().expect("complete fragment set");
        let result = pending.parts.into_iter().flatten().flatten().collect();
        self.completed[slot] = generation;
        Ok(Some((kind, result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn larger_setup_survives_reordering_duplication_and_superseded_fragments() {
        let payload = vec![73; 1600];
        let parts = encode(
            SetupKind::Options,
            SessionScope {
                epoch: [2; 16],
                revision: 2,
            },
            &payload,
        )
        .unwrap();
        let mut receiver = Reassembly::default();
        assert!(receiver.receive(&parts[2], 10).unwrap().is_none());
        assert!(receiver.receive(&parts[2], 11).unwrap().is_none());
        assert!(
            receiver
                .receive(
                    &encode(
                        SetupKind::Options,
                        SessionScope {
                            epoch: [2; 16],
                            revision: 1
                        },
                        b"old"
                    )
                    .unwrap()[0],
                    11
                )
                .unwrap()
                .is_none()
        );
        for index in [0, 3] {
            assert!(receiver.receive(&parts[index], 12).unwrap().is_none());
        }
        assert_eq!(
            receiver.receive(&parts[1], 13).unwrap(),
            Some((SetupKind::Options, payload))
        );
        assert!(receiver.receive(&parts[1], 14).unwrap().is_none());
        let mut oversized = parts[0].clone();
        oversized[29..33].copy_from_slice(&(MAX_BYTES as u32 + 1).to_le_bytes());
        assert!(receiver.receive(&oversized, 15).is_err());
    }
}
