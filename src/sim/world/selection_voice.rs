//! The successful normal Techno Select's voice tail, sharing the existing
//! process Main RNG with terrain variants and the other sound producers.
//!
//! Native: TechnoSelect6FBFA0, VoiceSelect708EB0, QueueMegaMission6FFBE0's
//! default VoiceSpecialAttack6FFD42, and shared QueueVoice708D90;
//! executed comparisons: tools/input_oracle/selection_navigation.json.
//! Main886B88 is excluded from lockstep hashes and snapshot bytes. Fresh
//! loading advances it before gameplay; in-scenario restore retains its live
//! continuation through `retain_process_state_for_load`.
//!
//! VoiceSelect708EB5 checks the existing SlaveOwner+2DC before choosing
//! type+430, then the deactivated latch +1C8 (708EFD, `sim::powered_unit`)
//! before type+44C, instead of the normal+414 list. All three use this one
//! owner; a click can select an offline Robot Tank (Select5F4520 does not
//! test +1C8), so its line is reachable. The process audio service
//! borrows this same Main continuation through Simulation's RNG capability.
//! These selection controls alone do not establish its device/queue behavior.

use super::Simulation;
use crate::rules::ruleset::RuleSet;

#[cfg(test)]
#[path = "selection_voice_tests.rs"]
mod tests;

/// Techno `+0x4F8`: the next ordinary VoiceDeploy attempt is silenced. The
/// constructor clears it (`0x006F30FA`). A SlaveManager relocation sets it
/// ([`Self::suppress`], `0x006B01C4`) before the refinery's undeploy voice,
/// and the common `TechnoClass::VoiceDeploy @ 0x00708E00` clears it after
/// its attempt ([`Self::clear`], `0x00708E6A`/`0x00708EA0`), even when the
/// type has no voice. The Building resource-Archive voice (`0x00459C20`)
/// bypasses both the test and the clear. This control history is distinct
/// from the audio pending line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DeployVoiceSuppression(bool);

impl DeployVoiceSuppression {
    /// Whether the next VoiceDeploy is silenced.
    pub(crate) const fn get(self) -> bool {
        self.0
    }

    /// The SlaveManager relocation's store (`0x006B01C4`).
    pub fn suppress(&mut self) {
        self.0 = true;
    }

    /// VoiceDeploy's clear after its attempt (`0x00708E6A`, `0x00708EA0`).
    pub(crate) fn clear(&mut self) {
        self.0 = false;
    }
}

impl Simulation {
    /// TechnoSelect6FBFA0 admits VoiceSelect only for House50B6F0 or
    /// HouseType+1A6 (MultiplayPassive), and only while the caller's selection
    /// voice latch is enabled (6FBFF4..6FC018). Rejection consumes no draw.
    /// The returned registered sound name is a presentation request; this
    /// operation changes only the existing process Main cursor.
    pub fn selection_voice_request<'a>(
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
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        let voices = if entity.slave.owner().is_some() {
            &object.voice_select_enslaved
        } else if entity.deactivated.is_offline() {
            &object.voice_select_deactivated
        } else {
            &object.voice_select
        };
        self.voice_request_from_list(voices, voices_enabled, human_player)
    }

    /// QueueMegaMission6FFCBD..6FFDA5's default voice arm, used by AreaGuard
    /// and Sabotage. The enable latch gates the list before any Main draw;
    /// QueueVoice's human-house gate follows the draw. Caller loops decide
    /// whether to suppress later actors (click dispatch does, Guard key does
    /// not). This request never changes the deterministic Scenario stream.
    pub fn default_order_voice_request<'a>(
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
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        self.voice_request_from_list(&object.voice_special_attack, voices_enabled, human_player)
    }

    /// QueueVoice708D90's sole admission predicate. The caller owns any list
    /// draw and House50B6F0 query; the audio owner alone retains the pending
    /// and playing slots. Scalar requests consume no random draw.
    pub fn queue_voice_request(
        sound: Option<&str>,
        voices_enabled: bool,
        human_player: bool,
    ) -> Option<&str> {
        sound.filter(|sound| voices_enabled && human_player && !sound.is_empty())
    }

    /// Building459C20's resource-Archive override, then common
    /// VoiceDeploy708E00. The resource branch returns before the suppression
    /// test/clear. Common infantry522510 deployment chooses +594 instead of
    /// +590; a missing slot stays silent, with no fallback or Main draw.
    /// Source: tools/input_oracle/slave_miner_pack_voice.{json,meta.json};
    /// original1cdd1180.
    pub fn deploy_voice_request<'a>(
        &mut self,
        rules: &'a RuleSet,
        entity_id: u64,
        voices_enabled: bool,
    ) -> Option<&'a str> {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::TargetKind;

        let entity = self.entities().get(entity_id)?;
        let human_player = self.house_is_human_player(entity.owner());
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        if entity.category() == EntityCategory::Structure
            && object.resource_gatherer
            && object.resource_destination
            && let Some(TargetKind::Cell(x, y)) = entity.archive_target()
            && crate::sim::miner::ore_scan::cell_is_tiberium_land(self, None, (x, y))
            && let Some(sound) = object.voice_harvest()
        {
            return Self::queue_voice_request(Some(sound), voices_enabled, human_player);
        }
        let request = if entity.deploy_voice_suppressed() {
            None
        } else {
            let sound = if entity.category() == EntityCategory::Infantry && entity.is_deployed() {
                object.voice_undeploy()
            } else {
                object.voice_deploy()
            };
            Self::queue_voice_request(sound, voices_enabled, human_player)
        };
        self.entities_mut()
            .get_mut(entity_id)?
            .suppress_deploy_voice
            .clear();
        request
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
        Self::queue_voice_request(Some(voices[index].as_str()), voices_enabled, human_player)
    }
}
