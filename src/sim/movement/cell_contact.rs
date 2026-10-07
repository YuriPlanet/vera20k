//! Cell483480's ordered ground-list Object+FC contact callbacks.
//!
//! Techno703850 dispatches StartUncloaking(false); Terrain5F4310 returns.
//! Callers include Walk75BBAE and Drive/Ship code1. The list's next link is
//! read after each receiver, with no alive, ownership or category admission.
//! Native executable controls: tools/spatial_oracle/walk_prehead_response.

use crate::map::cell_index::NativeCellIdentity;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{SimSoundEvent, Simulation};

impl Simulation {
    pub(super) fn uncloak_contacts_at_cell(
        &mut self,
        cell: (i16, i16),
        rules: &RuleSet,
    ) -> Result<(), String> {
        let native = self
            .resolved_terrain
            .as_ref()
            .ok_or("cell contact requires map cells")?
            .native_cell_identity(cell);
        self.uncloak_cell_contacts(native, rules)
    }

    pub(super) fn uncloak_cell_contacts(
        &mut self,
        cell: NativeCellIdentity,
        rules: &RuleSet,
    ) -> Result<(), String> {
        let at = self
            .resolved_terrain
            .as_ref()
            .ok_or("cell contact requires map cells")?
            .native_cell_coord(cell);
        #[cfg(test)]
        super::fresh_oracle_seam::observe_live(
            super::fresh_oracle_seam::FreshCallRecord::UncloakContacts { cell: at },
        );
        let mut next = self
            .cell_objects((at.0 as u16, at.1 as u16), MovementLayer::Ground)
            .next();
        while let Some(member) = next {
            if let CellObjectMember::Entity(id) = member {
                let actor = self
                    .substrate
                    .entities
                    .get(id)
                    .ok_or("cell contact has retired receiver")?;
                if actor.cloak.is_some() {
                    let speed = rules
                        .object(self.interner.resolve(actor.type_ref()))
                        .ok_or("cell contact lacks TechnoType")?
                        .cloaking_speed;
                    let actor = self
                        .substrate
                        .entities
                        .get_mut(id)
                        .ok_or("cell contact has retired receiver")?;
                    let result = actor
                        .cloak
                        .as_mut()
                        .expect("same contact receiver")
                        .start_uncloaking_from_mover_contact(
                            self.session.binary_frame as i32,
                            speed,
                            rules.general.cloaking_stages,
                        );
                    if result.play_sound
                        && let Some(sound) = rules.general.cloak_sound.as_deref()
                    {
                        self.sound_events.push(SimSoundEvent::cloak_sound(
                            sound.to_owned(),
                            &actor.position,
                        ));
                    }
                }
            }
            //48348D reads +30 only after the synchronous+FC returned.
            next = self.next_cell_object(member);
        }
        Ok(())
    }
}
