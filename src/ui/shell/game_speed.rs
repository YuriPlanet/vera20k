//! Display order for game speed, independent of the persisted native values.

use crate::sim::game_options::GAME_SPEED_90_TPS;

pub const MAX_POSITION: i32 = 7;

/// The CSF keys of a speed's name by slider position, 0 the slowest: the
/// table gamemd's Game Controls (BBB, 4E2278..4E232B) names its GameSpeed and
/// ScrollRate positions from.
const SPEED_LABEL_KEYS: [&str; 7] = [
    "TXT_SLOWEST",
    "TXT_SLOWER",
    "TXT_SLOW",
    "TXT_MEDIUM",
    "TXT_FAST",
    "TXT_FASTER",
    "TXT_FASTEST",
];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SpeedContext {
    #[default]
    Offline,
    /// Multiplayer's existing speed 0 is a synchronized 60-tick preset.
    Network,
}

impl SpeedContext {
    pub const fn visual_position(self, stored: i32) -> i32 {
        match (self, stored) {
            (Self::Offline, 0) | (Self::Network, 8) => 7,
            (Self::Offline, 8) | (Self::Network, 0) => 6,
            (_, 1..=6) => 6 - stored,
            _ => -1,
        }
    }

    pub const fn stored_speed(self, position: i32) -> i32 {
        match (self, position) {
            (Self::Offline, 7) | (Self::Network, 6) => 0,
            (Self::Offline, 6) | (Self::Network, 7) => GAME_SPEED_90_TPS as i32,
            _ => 6 - position,
        }
    }

    pub fn label_key(self, stored: i32) -> &'static str {
        match stored {
            0 if self == Self::Offline => "VERA:Unlimited",
            0 => "VERA:GameSpeed60",
            speed if speed == i32::from(GAME_SPEED_90_TPS) => "VERA:GameSpeed90",
            _ => SPEED_LABEL_KEYS[(6 - stored).clamp(0, 6) as usize],
        }
    }

    /// Compact text for the setup slider's 49-pixel value plaque.
    pub fn value_text(self, stored: i32) -> String {
        match stored {
            0 if self == Self::Offline => crate::i18n::tr("VERA:Unlimited"),
            0 => "60/s".to_owned(),
            speed if speed == i32::from(GAME_SPEED_90_TPS) => "90/s".to_owned(),
            _ => self.visual_position(stored).to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_keep_their_values_and_extra_notch_precedes_unlimited() {
        for stored in 0..=6 {
            let position = SpeedContext::Offline.visual_position(stored);
            assert_eq!(SpeedContext::Offline.stored_speed(position), stored);
        }
        assert_eq!(SpeedContext::Offline.stored_speed(5), 1);
        assert_eq!(SpeedContext::Offline.stored_speed(6), 8);
        assert_eq!(SpeedContext::Offline.stored_speed(7), 0);
        assert_eq!(SpeedContext::Offline.label_key(0), "VERA:Unlimited");
        assert_eq!(SpeedContext::Offline.label_key(6), "TXT_SLOWEST");
        assert_eq!(SpeedContext::Offline.label_key(3), "TXT_MEDIUM");
        assert_eq!(SpeedContext::Network.label_key(1), "TXT_FASTER");
        assert_eq!(SpeedContext::Network.stored_speed(6), 0);
        assert_eq!(SpeedContext::Network.stored_speed(7), 8);
        assert_eq!(SpeedContext::Network.label_key(0), "VERA:GameSpeed60");
    }
}
