//! Ares 3.0 building tunnels (`Enum/TunnelTypes.cpp`,
//! `Ext/Building/Hooks.Tunnels.cpp`).
//!
//! A BuildingType with `Tunnel=` names a `[TunnelTypes]` entry
//! (`rules::ares_building_type`). Every House holds one network per entry
//! (HouseExt Tunnels, `+0x64`, created by `HouseExt::GetTunnel @
//! 10021560`): the Feet inside, in entry order. A Foot that walks into any
//! tunnel building enters the network and is limboed; it leaves from any
//! other building of the same type, last in first out.
//!
//! - Entry: CAN_LOAD's tunnel arm (`100174E0`, hook `0x43C326`,
//!   `sim::radio::receive::can_load`), DOCK_NOW's ROGER (`100176B0`, hook
//!   `0x43C716`), the Infantry and Unit arrival arms (`10017760`, hook
//!   `0x51A2AD`; `10017870`, hook `0x73A23F`; `sim::movement::per_cell`),
//!   the Unit cell entry and destination arms (`100177F0`, hook `0x73F606`;
//!   `10017830`, hook `0x741CE5`) and the Infantry enter cursor
//!   (`100176E0`, hook `0x51ED8E`, `world_orders`).
//! - Exit: the building's Unload mission (`100174A0`, hook `0x44D8A7`,
//!   `transport_unload::ares_tunnel_unload_one`) after the self click
//!   (`100173A0`, hook `0x44731C`).
//! - The last entrance: its destruction (`10017350`, hook `0x442DF2`) or
//!   erasure (`100177B0`, hook `0x71A995`) kills the network; its sale
//!   (`100173E0`, hook `0x44A37F`) puts every member out.
//!
//! The networks are saved and hashed. A Foot that leaves play while inside
//! is dropped from its network (`TunnelData::InvalidatePointer @
//! 100091E0`).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::{
    ConcealOutcome, FrameEffects, PlacementEvidence, RevealRequest, SimSoundEvent, Simulation,
    UninitContext,
};

/// One House's network of one tunnel type (TunnelData, `0x10` bytes): its
/// members in entry order (`Vector`). MaxCap (`+0xC`) is the type's
/// `Passengers`, read from the rules when asked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct TunnelNetwork {
    house: InternedId,
    tunnel: usize,
    members: Vec<u64>,
}

/// Every House's tunnel networks (HouseExt `+0x64`), in creation order.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresTunnels {
    networks: Vec<TunnelNetwork>,
}

impl AresTunnels {
    /// The members of `house`'s network of type `tunnel`, in entry order.
    pub(crate) fn members(&self, house: InternedId, tunnel: usize) -> &[u64] {
        self.networks
            .iter()
            .find(|network| network.house == house && network.tunnel == tunnel)
            .map_or(&[], |network| &network.members)
    }

    fn network_mut(&mut self, house: InternedId, tunnel: usize) -> &mut Vec<u64> {
        let index = match self
            .networks
            .iter()
            .position(|network| network.house == house && network.tunnel == tunnel)
        {
            Some(index) => index,
            None => {
                self.networks.push(TunnelNetwork {
                    house,
                    tunnel,
                    members: Vec::new(),
                });
                self.networks.len() - 1
            }
        };
        &mut self.networks[index].members
    }

    /// `Vector.push_back`.
    fn push(&mut self, house: InternedId, tunnel: usize, foot: u64) {
        self.network_mut(house, tunnel).push(foot);
    }

    /// `Vector.back()` then `pop_back()`.
    pub(crate) fn pop(&mut self, house: InternedId, tunnel: usize) -> Option<u64> {
        self.network_mut(house, tunnel).pop()
    }

    /// `TunnelData::InvalidatePointer @ 100091E0` over every network
    /// (HouseExt::InvalidatePointer, inlined in `100052B0`).
    pub(crate) fn forget(&mut self, foot: u64) {
        for network in &mut self.networks {
            if let Some(index) = network.members.iter().position(|&member| member == foot) {
                network.members.remove(index);
            }
        }
    }

    /// Folded only while a network holds a Foot, so other worlds hash as
    /// before.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        let mut held = self
            .networks
            .iter()
            .filter(|network| !network.members.is_empty())
            .peekable();
        if held.peek().is_some() {
            std::hash::Hash::hash(b"ares-tunnels-v1", hasher);
            for network in held {
                std::hash::Hash::hash(network, hasher);
            }
        }
    }
}

impl Simulation {
    /// `BuildingTypeExt::GetTunnel @ 1000D740`'s type test: the
    /// `[TunnelTypes]` index of `building`'s `Tunnel=`, when it names one.
    pub(crate) fn ares_tunnel_type(&self, building: u64, rules: &RuleSet) -> Option<usize> {
        let entity = self.substrate.entities.get(building)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let tunnel = self
            .object_type(entity.type_ref(), rules)?
            .building_type_ext()
            .tunnel?;
        rules
            .ares_building_globals()
            .tunnel_max_cap(tunnel)
            .map(|_| tunnel)
    }

    /// CAN_LOAD's tunnel capacity test (`10017617`): MaxCap, read unsigned,
    /// holds one more member of `house`'s network.
    pub(crate) fn ares_tunnel_has_room(
        &self,
        house: InternedId,
        tunnel: usize,
        rules: &RuleSet,
    ) -> bool {
        let max_cap = rules
            .ares_building_globals()
            .tunnel_max_cap(tunnel)
            .unwrap_or(0);
        (max_cap as u32) >= self.ares_tunnels.members(house, tunnel).len() as u32 + 1
    }

    /// The self click's unload rung (`100173A0`, hook `0x44731C`): a tunnel
    /// building whose owner's network of its type holds a Foot offers the
    /// unload action.
    pub fn ares_tunnel_offers_unload(&self, building: u64, rules: &RuleSet) -> bool {
        let Some(tunnel) = self.ares_tunnel_type(building, rules) else {
            return false;
        };
        self.substrate
            .entities
            .get(building)
            .is_some_and(|entity| !self.ares_tunnels.members(entity.owner(), tunnel).is_empty())
    }

    /// The tunnel arm of TechnoExt::GetPassengerPips (`10044C90`): for a
    /// tunnel `building`, its type's MaxCap and the members of its owner's
    /// network of that type, in entry order.
    pub fn ares_tunnel_pip_contents(
        &self,
        building: u64,
        rules: &RuleSet,
    ) -> Option<(i32, &[u64])> {
        let tunnel = self.ares_tunnel_type(building, rules)?;
        let owner = self.substrate.entities.get(building)?.owner();
        let max_cap = rules.ares_building_globals().tunnel_max_cap(tunnel)?;
        Some((max_cap, self.ares_tunnels.members(owner, tunnel)))
    }

    /// `TunnelData::CanEnterTunnel @ 100097D0`: `foot` asks `building`
    /// CAN_LOAD (RadioCommand QueryCanEnter) and, on ROGER, enters `house`'s
    /// network of type `tunnel`. Answers whether it went in.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ares_can_enter_tunnel(
        &mut self,
        foot: u64,
        building: u64,
        house: InternedId,
        tunnel: usize,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if crate::sim::radio::transmit(
            self,
            foot,
            building,
            crate::sim::radio::RadioMessage::CanEnter,
            crate::sim::radio::RadioPayload::default(),
            rules,
            effects,
        ) != crate::sim::radio::RadioResponse::Roger
        {
            return false;
        }
        self.ares_enter_tunnel(foot, building, house, tunnel, rules, registry, effects)
    }

    /// `TunnelData::EnterTunnel @ 10009000`: the Foot drops its focus
    /// (ArchiveTarget), its mission tick count (`+0xC4`) and gattling spin
    /// and its captor, the tunnel type's `EnterTransportSound=` plays at the
    /// building's coordinates, and the Foot is limboed (which also BREAKs
    /// its contacts and conceals it) and appended to the network; a Unit
    /// also aborts its motion.
    ///
    /// RESIDUAL: native clears OnBridge before Limbo; VERA clears it after,
    /// as [`crate::sim::passenger::board_passenger`] does (see there).
    #[allow(clippy::too_many_arguments)]
    fn ares_enter_tunnel(
        &mut self,
        foot: u64,
        building: u64,
        house: InternedId,
        tunnel: usize,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(category) = self.substrate.entities.get_mut(foot).map(|entity| {
            entity.set_archive_target(None);
            entity.mission.clear_ai_counter();
            entity.gattling.set_value(0);
            entity.gattling.set_stage(0);
            entity.category()
        }) else {
            return false;
        };
        if let Some(controller) = self
            .substrate
            .entities
            .get(foot)
            .and_then(|entity| entity.mind_control.controller())
        {
            self.free_unit(controller, foot, rules, registry, effects);
        }
        let sound = self.substrate.entities.get(building).and_then(|entity| {
            let sound = self
                .object_type(entity.type_ref(), rules)?
                .enter_transport_sound
                .clone()?;
            let coords = crate::sim::movement::ground_pose::object_get_coords(
                entity,
                self.resolved_terrain.as_ref(),
            );
            Some((sound, coords))
        });
        if let Some((sound, coords)) = sound {
            self.sound_events.emit(SimSoundEvent::voc_at_world_coord(
                sound,
                crate::sim::anim_class::AnimWorldCoord {
                    x: coords.x,
                    y: coords.y,
                    z: coords.z,
                },
            ));
        }
        let context = UninitContext::new(rules, registry).with_effects(effects);
        if self.techno_limbo_with_context(foot, context) != ConcealOutcome::Concealed {
            return false;
        }
        if let Some(entity) = self.substrate.entities.get_mut(foot) {
            entity.on_bridge.store(false);
            if category == EntityCategory::Unit {
                // AbortMotion (vt+0x4C0).
                entity.navigation.stop_moving();
            }
        }
        self.ares_tunnels.push(house, tunnel, foot);
        true
    }

    /// Whether `building` is the last entrance of its owner's network of
    /// type `tunnel`: no other building of that House (House+0x68) with the
    /// same type index is alive, out of limbo and neither being built nor
    /// sold (`IsLastEntrance @ 10009220`, inlined in `100093C0`).
    fn ares_is_last_tunnel_entrance(&self, building: u64, tunnel: usize, rules: &RuleSet) -> bool {
        let Some(owner) = self
            .substrate
            .entities
            .get(building)
            .map(|entity| entity.owner())
        else {
            return true;
        };
        let Some(buildings) = self
            .houses
            .get(&owner)
            .map(|state| state.base_projection().buildings())
        else {
            return true;
        };
        !buildings.iter().any(|&other| {
            other != building
                && self.substrate.entities.get(other).is_some_and(|entity| {
                    entity.is_alive()
                        && !entity.in_limbo()
                        && !matches!(
                            entity.mission.current().known(),
                            Some(MissionType::Construction | MissionType::Selling)
                        )
                })
                && self.ares_tunnel_type(other, rules) == Some(tunnel)
        })
    }

    /// `TunnelData::KillFootsIfLastTunnel @ 100093C0` for `building`'s
    /// owner's network of its type, from its destruction (SpawnSurvivors,
    /// `10017350`, hook `0x442DF2`, `killer` its C4 planter) and its
    /// erasure (`100177B0`, hook `0x71A995`, `killer` the erasing Techno):
    /// when it was the last entrance, every member, last first, leaves its
    /// Team (`LiberateMember(-1, false)`), records its kill by `killer` and
    /// is UnInit.
    pub(crate) fn ares_kill_tunnel_feet_if_last(
        &mut self,
        building: u64,
        killer: Option<u64>,
        context: UninitContext<'_>,
    ) {
        let rules = context.rules();
        let Some(tunnel) = self.ares_tunnel_type(building, rules) else {
            return;
        };
        let Some(owner) = self
            .substrate
            .entities
            .get(building)
            .map(|entity| entity.owner())
        else {
            return;
        };
        if self.ares_tunnels.members(owner, tunnel).is_empty()
            || !self.ares_is_last_tunnel_entrance(building, tunnel, rules)
        {
            return;
        }
        while let Some(foot) = self.ares_tunnels.pop(owner, tunnel) {
            self.leave_team(foot, false, rules, context.effects());
            self.record_kill_and_uninit(foot, killer, context);
        }
    }

    /// The sale's tunnel arm (`100173E0`, hook `0x44A37F`, before the
    /// absorbed passengers): when `building` is the last entrance, every
    /// member is put out by `TunnelData::PlaceOne @ 10009810`, last first:
    /// unlimboed on the building's Location cell under the IKnowWhatImDoing
    /// latch, facing the building's rounded DirType
    /// (`((facing >> 7) + 1) >> 1`), then scattered from no coordinate
    /// (`Scatter(Empty, true, false)`). PlaceOne ignores the foundation cell
    /// its caller passes and always succeeds, so `KillOne` never runs here.
    pub(crate) fn ares_sell_tunnel(
        &mut self,
        building: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(tunnel) = self.ares_tunnel_type(building, rules) else {
            return;
        };
        let now = self.session.binary_frame();
        let Some((owner, cell, z, facing, on_bridge)) =
            self.substrate.entities.get(building).map(|entity| {
                (
                    entity.owner(),
                    (entity.position.rx(), entity.position.ry()),
                    entity.position.z(),
                    crate::util::direction_tables::round_facing16_to_8(
                        entity.body_facing_current(now),
                    ),
                    entity.on_bridge(),
                )
            })
        else {
            return;
        };
        if !self.ares_is_last_tunnel_entrance(building, tunnel, rules) {
            return;
        }
        while let Some(&foot) = self.ares_tunnels.members(owner, tunnel).last() {
            if let Some(entity) = self.substrate.entities.get_mut(foot) {
                entity.on_bridge.store(on_bridge);
            }
            let position = crate::sim::passenger::exit_cell_position(self, foot, cell.0, cell.1, z);
            let _ = self.try_reveal_entity_with_context(
                foot,
                RevealRequest {
                    position,
                    placement: PlacementEvidence::MarkSucceeded,
                    logic_eligible: true,
                },
                UninitContext::new(rules, registry)
                    .with_effects(effects)
                    .with_unlimbo_facing(Some(facing)),
            );
            let _ = self.scatter(
                foot,
                None,
                crate::sim::movement::ScatterFlags::new(true, false),
                rules,
                registry,
                effects,
            );
            self.ares_tunnels.pop(owner, tunnel);
        }
    }
}

#[cfg(test)]
#[path = "ares_tunnel_tests.rs"]
mod tests;
