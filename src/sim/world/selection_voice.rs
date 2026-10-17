//! Techno selection and committed-order voice requests share the existing
//! process Main RNG with terrain variants and the other sound producers.
//!
//! Native: TechnoSelect6FBFA0, VoiceSelect708EB0, VoiceMove708FC0, VoiceAttack7090A0,
//! QueueMegaMission6FFBE0's default VoiceSpecialAttack6FFD42, and shared
//! QueueVoice708D90. Executed comparisons: input_oracle/selection_navigation
//! and spatial_oracle/{allied_object_action,attack_order_voice} under tools/.
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

#[cfg(test)]
#[path = "order_voice_tests.rs"]
mod order_tests;

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

    /// QueueMegaMission6FFCBD..6FFDA5 chooses the committed order's voice
    /// only while its enable latch is set. QueueVoice708D90 applies the
    /// human-House gate after the reached voice function chooses a sound.
    /// Capture708DC0 falls to Enter709020, which falls to Move708FC0;
    /// Harvest709060 also falls to Move when its scalar slot is absent.
    ///
    /// VoiceAttack7090A0 consumes the committed attack target before event
    /// encoding, independently of the actor's old TarCom or UI hover.
    /// VoiceMove708FC0 consumes the full fixed-catalog list retained by the
    /// existing type-processing owner, including Enter/Capture/Harvest fallback.
    pub fn order_voice_request<'a>(
        &mut self,
        rules: &'a RuleSet,
        entity_id: u64,
        voice_field: &str,
        target: Option<crate::sim::combat::TargetKind>,
        voices_enabled: bool,
    ) -> Option<&'a str> {
        if !voices_enabled {
            return None;
        }
        if voice_field == "VoiceAttack" {
            return self.attack_voice_request(rules, entity_id, target, voices_enabled);
        }
        let entity = self.entities().get(entity_id)?;
        let human_player = self.house_is_human_player(entity.owner());
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        // 6FFD42's default arm, including AreaGuard and Sabotage. Caller
        // loops own suppression of later actors; this owner spends only Main.
        if voice_field == "VoiceSpecialAttack" {
            return self.voice_request_from_list(
                &object.voice_special_attack,
                voices_enabled,
                human_player,
            );
        }
        let scalar = match voice_field {
            "VoiceMove" => None,
            "VoiceHarvest" => object.voice_harvest(),
            "VoiceEnter" => object.voice_enter(),
            "VoiceCapture" => object.voice_capture().or(object.voice_enter()),
            _ => return None,
        };
        if let Some(sound) = scalar {
            return Self::queue_voice_request(Some(sound), voices_enabled, human_player);
        }
        // VoiceMove708FD6..70900C: every nonempty list, including a
        // singleton, spends one raw Main65C780 draw before
        // QueueVoice's enable/House rejection. The caller enable gate above
        // instead prevents reaching VoiceMove at all (6FFCBD..6FFCCB).
        self.voice_request_from_list(object.voice_move(), voices_enabled, human_player)
    }

    /// VoiceAttack7090A0 through the existing SelectWeapon/GetWeapon,
    /// raw Veterancy750010, type/General sound readers and Main/QueueVoice
    /// owners. Original executions: spatial_oracle/attack_order_voice.json.
    /// A reached list still draws before QueueVoice rejects disabled/nonhuman
    /// requests; QueueMegaMission's earlier enable gate is owned above.
    fn attack_voice_request<'a>(
        &mut self,
        rules: &'a RuleSet,
        entity_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
        voices_enabled: bool,
    ) -> Option<&'a str> {
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::sim::combat::{combat_weapon, veterancy};

        let entity = self.entities().get(entity_id)?;
        let human_player = self.house_is_human_player(entity.owner());
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        // Input-time selection keeps the shared fallback Cell unchanged.
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(NativeCellQuery::isolated);
        let index = combat_weapon::select_weapon_index_against_in_query(
            rules,
            entity,
            object,
            &combat_weapon::attacker_facts(entity, object),
            target.as_ref(),
            self,
            &self.interner,
            cells.as_ref(),
            Some(&self.house_alliances),
        )?;
        // GetWeapon(-1) returns a NULL slot, which7090C0 dereferences before
        // any voice or draw. A valid slot containing a NULL weapon is distinct
        // and continues to the ordinary override/list logic.
        assert_ne!(index, -1, "VoiceAttack7090C0 dereferences GetWeapon(-1)");
        let weapon = combat_weapon::get_weapon(
            entity,
            object,
            index,
            self.entities(),
            rules,
            &self.interner,
        );
        let scalar = if weapon.is_some_and(|weapon| weapon.damage < 0)
            && object.id.eq_ignore_ascii_case("FV")
        {
            //7090D0..7090EB: native literal FV, MatchesTypeID5F3E50/410A40.
            // This jumps past all four overrides even when Rules+230 is -1.
            rules.general.voice_ifv_repair()
        } else {
            object.voice_attack_override(index, veterancy::raw_is_elite(entity.veterancy.raw()))
        };
        if let Some(sound) = scalar {
            return Self::queue_voice_request(Some(sound), voices_enabled, human_player);
        }
        //70918D..7091C4: unsigned raw draw/modulo, including a singleton.
        self.voice_request_from_list(object.voice_attack(), voices_enabled, human_player)
    }

    /// QueueVoice708D90's sole admission predicate. The caller owns any list
    /// draw and House50B6F0 query; the audio owner alone retains the pending
    /// and playing slots. Scalar requests consume no random draw.
    fn queue_voice_request(
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

    /// Reached VoiceSelect708EB0, VoiceMove708FC0, VoiceAttack7090A0 and default6FFD42
    /// consume one raw Random65C780 draw for every nonempty list, including
    /// a singleton, then unsigned modulo.
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
