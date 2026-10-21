//! The Foot navigation list (`Foot+0x5AC`, count `+0x5BC`, loop flag
//! `+0x6B1`): posts a Foot visits one at a time. Its state belongs to
//! [`NavigationState`](super::navcom::NavigationState); these are the list's
//! queue and send operations.
//!
//! - Producers: ExitObject's Infantry post (`0x00444DAC`,
//!   `production_queue::infantry_exit_tail`) and the Unit setter's
//!   WeaponsFactory arm (`0x00742E28`, `Simulation::unit_factory_exit_arm`):
//!   an order to a vehicle still leaving its war factory.
//! - Consumers: Handle_Navigation_List from Infantry (`0x0051CC21`) and Unit
//!   (`0x00738A2F`) Enter_Idle_Mode, Mission_Area_Guard's head for a
//!   computer Foot ([`Simulation::area_guard_navigation_head`]), and Unit
//!   Per_Cell_Process's building Scatter tail, which reads the count
//!   (`0x0073ABAE`, `Simulation::unit_per_cell_building_scatter`).
//! - Clears: the MEGAMISSION event (`0x004C7453`, `Simulation::queue_megamission`),
//!   the IDLE event (`0x004C758B`, `0x004C759C`), the Unit setter's
//!   WeaponsFactory arm (`0x00742DFF`) and Unit Receive_Radio's message 7
//!   (`0x00737546`, an arm with no represented sender). PointerExpired
//!   removes every entry naming the expiring object
//!   (`0x004D9B43..0x004D9B95`).
//!
//! RESIDUALS: two setters insert their destination at the list's front, and
//! neither is ported. Both are dormant on retail data.
//! - Unit Assign_Destination (`0x00741E8E..0x00741F97`), for a moving Unit
//!   whose type has `Subterranean=` (`+0xD2C`) and whose active locomotor is
//!   Drive (CLSID `0x007E9A30`). Retail `rulesmd.ini` sets no
//!   `Subterranean=`.
//! - Infantry Assign_Destination (`0x0051AD17..0x0051AE23`), for a walking
//!   `JumpJet=` infantryman (see `assign_infantry_destination`). Retail sets
//!   `HoverAttack=` on each such type.
//!
//! Trigger: a map or mode layer that sets the key. Effect: the order replaces
//! the current post instead of preceding it.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// `FootClass::Queue_Navigation_List @ 0x004DA0E0`: the list takes
    /// `target` ([`NavigationState::queue_nav_list`]). Then a Foot with no
    /// NavCom whose mission (Get_Mission, vt+0x184) is Guard enters idle
    /// mode (vt+0x484(0, 1), `0x004DA1A6`), unless radio contact 0 is a
    /// `WeaponsFactory=` building (`0x0065AD30`, BuildingType `+0x16BD`).
    /// The native null-target return (`0x004DA0EB`) has no caller here.
    ///
    /// [`NavigationState::queue_nav_list`]: super::navcom::NavigationState::queue_nav_list
    pub(crate) fn queue_navigation_list(
        &mut self,
        id: u64,
        target: NavTargetRef,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let names_self = TargetKind::from(target) == TargetKind::Entity(id);
        entity.navigation.queue_nav_list(target, names_self);
        if entity.navigation.nav_com().is_some()
            || entity.mission.effective().known() != Some(MissionType::Guard)
        {
            return;
        }
        let factory_contact = entity
            .radio_contacts
            .slot(0)
            .is_some_and(|contact| self.is_weapons_factory(contact, rules));
        if !factory_contact {
            crate::sim::world::enter_idle_mode(self, id, rules, registry, effects);
        }
    }

    /// `FootClass::Handle_Navigation_List @ 0x004DA030`: a Foot without a
    /// NavCom whose list has a head is sent there
    /// ([`Self::send_to_navigation_head`]).
    pub(crate) fn handle_navigation_list(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.navigation.nav_com().is_some() {
            return;
        }
        let Some(&head) = entity.navigation.nav_list().first() else {
            return;
        };
        self.send_to_navigation_head(id, head, rules, registry, effects);
    }

    /// `FootClass::Mission_Area_Guard`'s list head (`0x004D6B23..0x004D6B8D`),
    /// after the tank bunker and building-entry heads: a Foot whose owner is
    /// not controlled by a human (`0x0050B730`), with no NavCom, whose list
    /// head is its ArchiveTarget (`+0x218`), is sent there
    /// ([`Self::send_to_navigation_head`]). The mission body follows either
    /// way. A missing House sends nothing.
    pub(crate) fn area_guard_navigation_head(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let computer = self
            .houses
            .get(&entity.owner())
            .is_some_and(|house| !house.is_controlled_by_human(self.session.game_mode_nonzero()));
        if !computer || entity.navigation.nav_com().is_some() {
            return;
        }
        let Some(&head) = entity.navigation.nav_list().first() else {
            return;
        };
        if entity.archive_target() != Some(TargetKind::from(head)) {
            return;
        }
        self.send_to_navigation_head(id, head, rules, registry, effects);
    }

    /// The send both consumers make: vt+0x480(head, 1) (`0x004DA062`,
    /// `0x004D6B65`), then the list rotates
    /// ([`NavigationState::rotate_nav_list`]).
    ///
    /// [`NavigationState::rotate_nav_list`]: super::navcom::NavigationState::rotate_nav_list
    fn send_to_navigation_head(
        &mut self,
        id: u64,
        head: NavTargetRef,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if let Err(cause) =
            self.assign_destination_represented(id, Some(head), rules, registry, effects)
        {
            log::debug!("Foot {id} navigation list head: {cause}");
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.rotate_nav_list(head);
        }
    }
}

#[cfg(test)]
#[path = "nav_list_tests.rs"]
mod tests;
