//! Synchronized timing values owned by ScenarioSession. Local clocks and
//! transport measurements enter only as ordinary EventClass records.

use std::collections::BTreeMap;
use std::num::NonZeroU8;

use serde::{Deserialize, Serialize};

/// Native FPS A8B558, FrameSendRate A8B554, MaxMaxAhead A8B568 and
/// NodeNameTag+73 process measurements. MaxAhead remains ScenarioSession's
/// existing field; it is not copied into this record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkCommandTiming {
    host_house: i8,
    #[serde(deserialize_with = "read_frame_send_rate")]
    frame_send_rate: NonZeroU8,
    frames_per_second: u16,
    max_max_ahead: u16,
    processing_millis: BTreeMap<i8, Option<u16>>,
}

fn read_frame_send_rate<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU8, D::Error> {
    let value = u8::deserialize(deserializer)?;
    // The shared save-layout tracer supplies zero placeholders. Keep the wire
    // field a u8 while retaining the nonzero invariant for every real snapshot.
    if crate::util::save_codec::layout::describing() {
        return Ok(NonZeroU8::new(1).unwrap());
    }
    NonZeroU8::new(value).ok_or_else(|| serde::de::Error::custom("frame send rate must be nonzero"))
}

impl NetworkCommandTiming {
    pub(crate) fn new(
        host_house: i8,
        frame_send_rate: NonZeroU8,
        max_ahead: u16,
        houses: impl IntoIterator<Item = i8>,
    ) -> Self {
        let processing_millis: BTreeMap<_, _> =
            houses.into_iter().map(|house| (house, None)).collect();
        // The authenticated WOL publisher can already be absent after the
        // loading wait's DestroyConnection684596. Session697E70 then falls
        // back to a remaining source; installation must keep those live rows.
        assert!(host_house >= 0 && !processing_millis.is_empty());
        assert!(processing_millis.keys().all(|house| *house >= 0));
        Self {
            host_house,
            frame_send_rate,
            // Main__PrepareSession52DABD; the first TIMING event replaces it.
            frames_per_second: 30,
            max_max_ahead: max_ahead,
            processing_millis,
        }
    }

    /// Startup's authenticated publisher preference. Live authority is derived
    /// from the existing admitted rows and House state by Simulation.
    pub const fn host_house(&self) -> i8 {
        self.host_house
    }
    pub const fn frame_send_rate(&self) -> NonZeroU8 {
        self.frame_send_rate
    }
    pub const fn frames_per_second(&self) -> u16 {
        self.frames_per_second
    }

    /// ProcessNetwork647BF4 waits until every active node has reported. The
    /// caller supplies the existing live roster, excluding disconnected peers.
    pub fn maximum_processing_millis(&self, houses: impl IntoIterator<Item = i8>) -> Option<u16> {
        houses.into_iter().try_fold(0u16, |maximum, house| {
            Some(maximum.max(self.processing_millis.get(&house).copied().flatten()?))
        })
    }

    pub(crate) fn admits_source(&self, house: i8) -> bool {
        self.processing_millis.contains_key(&house)
    }

    /// NodeNameTag removal5DAA8A..5DAB38 retires its timing sample too.
    /// EXIT/REMOVEPLAYER call this only at their synchronized command tail.
    pub(crate) fn retire_source(&mut self, house: i8) {
        self.processing_millis.remove(&house);
    }

    pub(crate) fn record_processing_millis(&mut self, house: i8, milliseconds: u16) -> bool {
        // EventClass4C80B1 looks up a NodeNameTag by its bound House, so an AI
        // or an unadmitted House cannot create a new process-timing row.
        if let Some(value) = self.processing_millis.get_mut(&house) {
            *value = Some(milliseconds);
            true
        } else {
            false
        }
    }

    pub(crate) fn apply(&mut self, timing: crate::sim::command::TimingRecord, adjusted_ahead: u16) {
        self.frames_per_second = timing.frames_per_second;
        self.frame_send_rate = timing.frame_send_rate;
        self.max_max_ahead = self.max_max_ahead.max(adjusted_ahead);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_snapshot_layout_keeps_the_nonzero_rate_invariant() {
        let timing = NetworkCommandTiming::new(0, NonZeroU8::new(2).unwrap(), 15, [0, 1]);
        assert!(crate::util::save_codec::layout::describe::<NetworkCommandTiming>().is_ok());
        let mut value = serde_json::to_value(&timing).unwrap();
        assert_eq!(
            serde_json::from_value::<NetworkCommandTiming>(value.clone()).unwrap(),
            timing
        );
        value["frame_send_rate"] = serde_json::json!(0);
        assert!(serde_json::from_value::<NetworkCommandTiming>(value).is_err());
    }
}
