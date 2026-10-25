//! Bounded opt-in facts from the existing global send/ACK/row application owners.
//! This log has no retry, sequence, progress, membership or timer authority.

use super::GlobalDestination;

#[derive(Debug, Clone, PartialEq)]
pub enum LoadingObservation {
    Admitted {
        sequence: u32,
        destination: GlobalDestination,
        percent: i32,
        reliable: bool,
    },
    Retired {
        sequence: u32,
        destination: GlobalDestination,
    },
    Applied {
        source: usize,
        sequence: u32,
        percent: i32,
        displayed: f64,
        applied: bool,
    },
}

#[derive(Debug, Default, Clone)]
pub struct LoadingObservationLog {
    records: Vec<LoadingObservation>,
    overflowed: bool,
}

impl LoadingObservationLog {
    pub fn records(&self) -> &[LoadingObservation] {
        &self.records
    }
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    fn push(&mut self, observation: LoadingObservation) {
        // At most30 source rows *101 normal percentages plus local admissions
        // and terminal ACKs. Repeated loss/retry callbacks retain their FIRST
        // sequence rather than growing with wall time. Malformed traffic cannot
        // allocate an unbounded trace; capture fails explicitly on overflow.
        if self.records.len() == 8192 {
            self.overflowed = true;
            return;
        }
        self.records.push(observation);
    }

    pub(super) fn admitted(
        &mut self,
        bytes: &[u8],
        destination: GlobalDestination,
        sequence: u32,
        reliable: bool,
    ) {
        use crate::net::lan_messages::ingame::{LoadingProgressMessage, SCOPED_LOADING_MAGIC};
        // Same native/scoped loading grammar; this is classification only.
        // The app still owns endpoint/scope/source admission before row mutation.
        let progress = if bytes.starts_with(SCOPED_LOADING_MAGIC) {
            let scope = bytes
                .get(4..4 + crate::net::lan_messages::SessionScope::LEN)
                .and_then(crate::net::lan_messages::SessionScope::decode);
            scope.and_then(|scope| LoadingProgressMessage::decode_scoped(bytes, scope).ok())
        } else {
            LoadingProgressMessage::decode(bytes).ok()
        };
        let Some(progress) = progress else {
            return;
        };
        let percent = progress.percent();
        if self.records.iter().any(|r| matches!(r, LoadingObservation::Admitted { destination:d, percent:p, .. } if *d==destination && *p==percent)) { return; }
        self.push(LoadingObservation::Admitted {
            sequence,
            destination,
            percent,
            reliable,
        });
    }

    pub(super) fn retired(&mut self, destination: GlobalDestination, sequence: u32) {
        if !self.records.iter().any(|r| matches!(r, LoadingObservation::Admitted { sequence:s, destination:d, percent:100, reliable:true } if *s==sequence && *d==destination))
            || self.records.iter().any(|r| matches!(r, LoadingObservation::Retired { sequence:s, destination:d } if *s==sequence && *d==destination)) { return; }
        self.push(LoadingObservation::Retired {
            sequence,
            destination,
        });
    }

    pub(super) fn application(
        &mut self,
        source: usize,
        sequence: u32,
        percent: i32,
        displayed: f64,
        applied: bool,
    ) {
        if self.records.iter().any(|r| matches!(r, LoadingObservation::Applied { source:s, percent:p, displayed:d, applied:a, .. } if *s==source && *p==percent && *d==displayed && *a==applied)) { return; }
        self.push(LoadingObservation::Applied {
            source,
            sequence,
            percent,
            displayed,
            applied,
        });
    }
}
