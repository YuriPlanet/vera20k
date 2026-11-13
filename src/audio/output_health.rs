//! Notices a lost sound device (headphones unplugged, a Bluetooth headset or
//! HDMI display gone) so its player can open the default device again.
//!
//! VERA-only: gamemd's DirectSound buffers are not ported, rodio is the output.
//! cpal reports a lost device only through the stream's error callback, on its
//! own thread; the players read the flag from the main thread's audio service.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rodio::cpal::StreamError;

/// Wait between tries to open a device, so a machine with no output left does
/// not enumerate devices every frame.
const RETRY_MS: u64 = 2_000;

/// One opened output's health. Each reopened sink gets a new one, so a late
/// error from the stream being dropped cannot mark the new stream lost.
#[derive(Debug, Default)]
pub(crate) struct OutputHealth {
    lost: Arc<AtomicBool>,
    next_try_ms: u64,
}

impl OutputHealth {
    /// The error callback for this output's stream. Only a device that is
    /// gone or a stream that must be rebuilt counts: cpal also reports
    /// underruns and backend errors (ALSA poll and short writes) and plays on.
    pub(crate) fn callback(
        &self,
        output: &'static str,
    ) -> impl FnMut(StreamError) + Send + Clone + 'static {
        let lost = Arc::clone(&self.lost);
        move |error| {
            if !matches!(
                error,
                StreamError::DeviceNotAvailable | StreamError::StreamInvalidated
            ) {
                return;
            }
            if !lost.swap(true, Ordering::Relaxed) {
                log::error!("{output} output lost: {error}");
            }
        }
    }

    /// True when the output is lost and another try to open one is due.
    pub(crate) fn reopen_due(&mut self, now_ms: u64) -> bool {
        if !self.lost.load(Ordering::Relaxed) || now_ms < self.next_try_ms {
            return false;
        }
        self.next_try_ms = now_ms.saturating_add(RETRY_MS);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_device_asks_for_a_reopen_once_per_retry_period() {
        let mut health = OutputHealth::default();
        let mut report = health.callback("Test");
        report(StreamError::BufferUnderrun);
        assert!(!health.reopen_due(10), "an underrun is not a lost device");
        report(StreamError::DeviceNotAvailable);
        assert!(health.reopen_due(10));
        assert!(!health.reopen_due(10 + RETRY_MS - 1));
        assert!(health.reopen_due(10 + RETRY_MS));
    }
}
