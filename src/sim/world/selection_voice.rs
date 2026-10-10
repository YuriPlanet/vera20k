//! The successful normal Techno Select's voice tail, sharing the existing
//! process Main RNG with terrain variants and the other sound producers.
//!
//! Native: TechnoSelect6FBFA0, VoiceSelect708EB0, QueueMegaMission6FFBE0's
//! default VoiceSpecialAttack6FFD42, and shared QueueVoice708D90;
//! executed comparisons: tools/input_oracle/selection_navigation.json.
//! Main886B88 is excluded from lockstep hashes and snapshot bytes. Fresh
//! loading advances it before gameplay; in-scenario restore retains its live
//! continuation through `retain_in_scenario_process_state_from`.
//!
//! RESIDUAL: enslaved and offline Robot receivers choose type+430/+44C in
//! VoiceSelect708EB0. Their lifecycle and separate voice lists remain outside
//! this normal type+414 path. The process audio service borrows this same Main
//! continuation through Simulation's RNG capability; these selection controls
//! alone do not establish its device/queue behavior.

use super::Simulation;
use crate::rules::ruleset::RuleSet;

#[cfg(test)]
#[path = "selection_voice_tests.rs"]
mod tests;

impl Simulation {
    /// TechnoSelect6FBFA0 admits VoiceSelect only for House50B6F0 or
    /// HouseType+1A6 (MultiplayPassive), and only while the caller's selection
    /// voice latch is enabled (6FBFF4..6FC018). Rejection consumes no draw.
    /// The returned registered sound name is a presentation request; this
    /// operation changes only the existing process Main cursor.
    pub(crate) fn selection_voice_request<'a>(
        &mut self,
        rules: &'a RuleSet,
        entity_id: u64,
        voices_enabled: bool,
    ) -> Option<&'a str> {
        let entity = self.entities().get(entity_id)?;
        let human_player = self.house_is_human_player(entity.owner());
        let passive = self
            .houses
            .get(&entity.owner())
            .is_some_and(|house| house.multiplay_passive);
        if !voices_enabled || (!human_player && !passive) {
            return None;
        }
        let object = rules.object(self.interner.resolve(entity.type_ref()))?;
        self.voice_request_from_list(&object.voice_select, voices_enabled, human_player)
    }

    /// QueueMegaMission6FFCBD..6FFDA5's default voice arm, used by AreaGuard
    /// and Sabotage. The enable latch gates the list before any Main draw;
    /// QueueVoice's human-house gate follows the draw. Caller loops decide
    /// whether to suppress later actors (click dispatch does, Guard key does
    /// not). This request never changes the deterministic Scenario stream.
    pub(crate) fn default_order_voice_request<'a>(
        &mut self,
        rules: &'a RuleSet,
        entity_id: u64,
        voices_enabled: bool,
    ) -> Option<&'a str> {
        if !voices_enabled {
            return None;
        }
        let entity = self.entities().get(entity_id)?;
        let human_player = self.house_is_human_player(entity.owner());
        let object = rules.object(self.interner.resolve(entity.type_ref()))?;
        self.voice_request_from_list(&object.voice_special_attack, voices_enabled, human_player)
    }

    /// Reached VoiceSelect708EB0 and default command6FFD42 consume one raw
    /// Random65C780 draw for every nonempty list, including a singleton,
    /// then unsigned modulo.
    /// QueueVoice708D90 applies enable/House50B6F0/-1 rejection after the draw.
    /// Native ReadSoundList binding skips unresolved names, so its selected-ID
    /// -1 rejection is represented only by deliberately unbound test inputs.
    fn voice_request_from_list<'a>(
        &mut self,
        voices: &'a [String],
        voices_enabled: bool,
        human_player: bool,
    ) -> Option<&'a str> {
        if voices.is_empty() {
            return None;
        }
        let index = (self.main_rng.next_u32() % voices.len() as u32) as usize;
        // Empty is the existing sound-name representation of native ID -1.
        if !voices_enabled || voices[index].is_empty() || !human_player {
            return None;
        }
        Some(voices[index].as_str())
    }
}
