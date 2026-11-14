//! A Unit's art `DeathFrames=` death.
//!
//! `UnitClass::ReceiveDamage @ 0x00737C90` (`0x00737DA7..0x00737DDD`): a
//! lethal hit on a UnitType whose `DeathFrames=` (`+0xE20`) is above zero
//! starts the Unit's death counter (`+0x6D8`, -1 to 0; only the hit that
//! starts it announces the loss, `0x00737DC9`), stores Health 1 and IsAlive,
//! and jumps past the ship sink, the bomb splash and `Death_Explosion` to
//! Mark(UP) (`0x00737F74`). The receiver then crashes a `Crashable=` Unit
//! (`Crash(0)` at Health 1 also runs Crash's live prefix: its Tag event and a
//! second RecordKill, with no attacker) and UnInits any other at once.
//!
//! A counting Unit's AI (`UnitClass::AI @ 0x007360C0`,
//! `0x0073635B..0x007363A3`) steps the count after its warp gates. Once the
//! count before the step has reached `MaxDeathCounter=` (`+0xE38`), it calls
//! `Death_Explosion` (`0x00738680`), Mark(UP) and UnInit. Meanwhile its SHP
//! body draws the death frames (`sim::animation::resolve_shp_vehicle_body_frame`)
//! and its vt+0x37C answers yes (`GameEntity::is_under_emp`).
//!
//! No retail ARTMD type sets `DeathFrames=`.
//!
//! RESIDUAL: gamemd steps the count before its tube test (`0x007363A4`);
//! VERA's object turn leaves a tube Unit before this step. A tube Unit is
//! below ground, so its Crash is refused and the receiver UnInits it: none
//! counts.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::shp_vehicle_sequence::UnitShpReadState;

use super::{FrameEffects, Simulation, UninitContext};

impl Simulation {
    fn unit_death_art(&self, id: u64, rules: &RuleSet) -> Option<UnitShpReadState> {
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Unit {
            return None;
        }
        let object = self.object_type(entity.type_ref(), rules)?;
        rules.unit_shp_read_state(&object.id).copied()
    }

    /// `0x00737DA7..0x00737DDD`: `None` when the Unit's death is not deferred.
    /// Otherwise the count starts (`Some(true)`, which announces the loss) or
    /// keeps running (`Some(false)`), and the Unit takes Health 1 and IsAlive
    /// back.
    pub(crate) fn defer_unit_death(&mut self, id: u64, rules: &RuleSet) -> Option<bool> {
        if self.unit_death_art(id, rules)?.death_frames() <= 0 {
            return None;
        }
        let entity = self.substrate.entities.get_mut(id)?;
        let started = entity.unit_death_counter.start();
        entity.health.set(1);
        entity.lifecycle.restore_alive();
        Some(started)
    }

    /// `0x0073635B..0x007363A3`. Returns true when the count ended the Unit,
    /// which returns from its AI.
    pub(crate) fn unit_death_count(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if !self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.unit_death_counter.running())
        {
            return false;
        }
        let max = self
            .unit_death_art(id, rules)
            .unwrap_or_default()
            .max_death_counter();
        let ended = self
            .substrate
            .entities
            .get_mut(id)
            .is_some_and(|entity| entity.unit_death_counter.advance(max));
        if !ended {
            return false;
        }
        self.unit_death_explosion(rules, id, &mut Vec::new(), registry, effects);
        self.foot_mark_remove(id, rules, registry, effects);
        self.uninit_with_context(
            id,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        true
    }
}
