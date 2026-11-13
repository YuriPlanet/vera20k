//! Per-match audio state (F11 `MatchAudioState`): the app-side sound event
//! queue.
//!
//! Everything here is scoped to one match and must reset when a new match
//! installs. The EVA state predicates (funds nag, low power, unit lost,
//! under-attack cadence) are sim-owned (`sim::house_eva`, the combat death
//! and damage sites); this owner only carries their events to the player.
//! Process/device-wide audio (players, registries, volumes) stays outside
//! this owner, in `AppAudioRuntime`. `match_runtime::sound_dispatch` interprets
//! ordered simulation events into this queue for the local listener.

use crate::audio::events::SoundEventQueue;
use crate::sim::intern::InternedId;
use crate::sim::world::SimSoundEvent;
use crate::util::fixed_math::SimFixed;

#[derive(Default)]
pub(crate) struct MatchAudioState {
    /// Sim/app sound events queued for playback this frame.
    pub(crate) sound_events: SoundEventQueue,
    /// Sounds the player's own automatic orders cause and this game skips.
    pub(crate) quiet: QuietSounds,
}

impl MatchAudioState {
    /// Reset every per-match latch and drop queued events. Called when a new
    /// match installs and when the player leaves a match for the shell.
    pub(crate) fn reset_for_new_match(&mut self) {
        *self = Self::default();
    }
}

/// One simulation sound fact this game expects and skips.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum QuietSound {
    /// [`SimSoundEvent::Repairing`] of this House and building type.
    Repairing {
        owner: InternedId,
        type_id: InternedId,
    },
    /// [`SimSoundEvent::VocAt`] of this sound at this spot.
    VocAt {
        sound_id: String,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
    },
    /// [`SimSoundEvent::HouseEva`] of this line for this House.
    HouseEva {
        owner: InternedId,
        event: &'static str,
    },
}

impl QuietSound {
    fn matches(&self, event: &SimSoundEvent) -> bool {
        match (self, event) {
            (
                Self::Repairing { owner, type_id },
                SimSoundEvent::Repairing {
                    owner: event_owner,
                    type_id: event_type,
                },
            ) => owner == event_owner && type_id == event_type,
            (
                Self::VocAt {
                    sound_id,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                },
                SimSoundEvent::VocAt {
                    sound_id: event_sound,
                    rx: event_rx,
                    ry: event_ry,
                    sub_x: event_sub_x,
                    sub_y: event_sub_y,
                    ..
                },
            ) => {
                sound_id == event_sound
                    && (rx, ry, sub_x, sub_y) == (event_rx, event_ry, event_sub_x, event_sub_y)
            }
            (
                Self::HouseEva { owner, event },
                SimSoundEvent::HouseEva {
                    owner: event_owner,
                    event: event_line,
                },
            ) => owner == event_owner && event == event_line,
            _ => false,
        }
    }
}

/// The sounds the player's own automatic orders will cause and this game
/// skips: VERA Auto Repair's starts (`app::input::auto_repair`), which play
/// like a wrench click otherwise, and primary rotation's turns
/// (`app::input::primary_rotation`), which would each say "Primary building
/// selected". Each entry keeps one matching fact out of
/// `match_runtime::sound_dispatch` and lapses after its last frame if none
/// came. Presentation only: the simulation, replays and other players' games
/// still have every fact.
#[derive(Debug, Default)]
pub(crate) struct QuietSounds {
    /// Each expected fact and the last frame it may come in.
    expected: Vec<(QuietSound, u64)>,
}

impl QuietSounds {
    /// Skip the next `sound` that comes by frame `until`; `now` is the
    /// current frame, before which lapsed entries are dropped.
    pub(crate) fn expect(&mut self, sound: QuietSound, now: u64, until: u64) {
        self.expected.retain(|&(_, last)| last >= now);
        self.expected.push((sound, until));
    }

    /// Whether this game plays `event` at frame `now`: not when an entry
    /// expects it, and that entry is then spent.
    pub(crate) fn admit(&mut self, event: &SimSoundEvent, now: u64) -> bool {
        if self.expected.is_empty() {
            return true;
        }
        let Some(at) = self
            .expected
            .iter()
            .position(|(sound, last)| *last >= now && sound.matches(event))
        else {
            return true;
        };
        self.expected.remove(at);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{MatchAudioState, QuietSound, QuietSounds};
    use crate::sim::intern::InternedId;
    use crate::sim::world::SimSoundEvent;

    /// F11: the new-match reset drops the queued sound events (previously
    /// surviving teardown undrained).
    #[test]
    fn new_match_reset_clears_queued_events() {
        let mut audio = MatchAudioState::default();
        audio
            .sound_events
            .push(crate::audio::events::GameSoundEvent::UiSound {
                sound_id: "leftover".to_string(),
            });

        audio.reset_for_new_match();

        assert!(
            audio.sound_events.drain().is_empty(),
            "queued events dropped"
        );
    }

    /// An entry skips one matching fact; a second one, another building
    /// type and a fact after the entry's last frame are all heard.
    #[test]
    fn a_quiet_entry_skips_one_matching_sound_until_its_last_frame() {
        let (owner, power, barracks) = (
            InternedId::from_index(0),
            InternedId::from_index(1),
            InternedId::from_index(2),
        );
        let repairing = |type_id| SimSoundEvent::Repairing { owner, type_id };
        let mut quiet = QuietSounds::default();
        quiet.expect(
            QuietSound::Repairing {
                owner,
                type_id: power,
            },
            10,
            100,
        );
        assert!(quiet.admit(&repairing(barracks), 11));
        assert!(!quiet.admit(&repairing(power), 11));
        assert!(quiet.admit(&repairing(power), 11));

        quiet.expect(
            QuietSound::Repairing {
                owner,
                type_id: power,
            },
            20,
            100,
        );
        assert!(quiet.admit(&repairing(power), 101));
    }
}
