//! Object property writes (appendix 7.1) that `set` and the actions share,
//! each through its owner: `Mission` (and `SetMission`), `Ammo` (and
//! `AddAmmo`) and `Online`. `Health`, `Owner` and `Rank` stay beside `set`
//! in the parent module.
//!
//! Residuals:
//! - A mission is ordered as the object's own house's player would order
//!   it, so it passes that receiver's checks (a dead, limboed or captured
//!   object refuses); `IgnoreOrders` is not kept yet.
//! - An `Ammo` write starts the reload clock as ReceiveDamage's readiness
//!   write does (StartReloading6FB080), restarting a running one.

use vera20k_wasm_ext::{Invalid, Status};

use super::Host;
use crate::map::entities::EntityCategory;
use crate::rules::mission_data::MissionType;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;

impl Host<'_, '_> {
    /// `Mission` writes and `SetMission`: the MEGAMISSION event a player's
    /// order sends (`EventClass::Execute`, `0x004C71F6..0x004C74BA`), run by
    /// the command receiver for the object's own house. `token` goes in the
    /// target for Attack and Area Guard and in the destination otherwise, as
    /// the player's Attack, Guard, Move, Capture and Sabotage orders put
    /// theirs. `None` (255) or an unknown mission is invalid.
    pub(super) fn order_mission(
        &mut self,
        id: u64,
        mission: i64,
        token: Option<TargetKind>,
    ) -> Result<Status, Invalid> {
        let Some(mission) = u8::try_from(mission).ok().and_then(MissionType::from_id) else {
            return Err(Invalid(format!("{mission} is not a Mission to order")));
        };
        let Some(entity) = self.sim.entities().get(id) else {
            return Ok(Status::Gone);
        };
        let owner = self.sim.interner.resolve(entity.owner()).to_owned();
        let (target, destination) = match mission {
            MissionType::Attack | MissionType::AreaGuard => (token, None),
            _ => (None, token),
        };
        let command = Command::MegaMission {
            entity_id: id,
            mission,
            target,
            destination,
        };
        let applied = self.sim.apply_command_with_overlays(
            crate::sim::command::CommandSender::Player(None),
            &owner,
            &command,
            self.rules,
            self.overlay_registry,
            self.frame_effects,
        );
        Ok(if applied.is_ok() {
            Status::Ok
        } else {
            Status::Refused
        })
    }

    /// `Ammo` and `AddAmmo`: the count through the ammo owner, clamped to the
    /// type's `Ammo=`, then StartReloading (`0x006FB080`). An object without
    /// ammo or a positive `Ammo=` answers `Refused`.
    pub(super) fn write_ammo(&mut self, id: u64, count: i64) -> Status {
        let sim = &mut *self.sim;
        let Some(entity) = sim.substrate.entities.get(id) else {
            return Status::Gone;
        };
        let Some(object) = sim.entity_object_type(entity, self.rules) else {
            return Status::Refused;
        };
        let now = sim.session.binary_frame() as i32;
        let Some(entity) = sim.substrate.entities.get_mut(id) else {
            return Status::Gone;
        };
        let written = entity
            .ammo_mut()
            .is_some_and(|ammo| ammo.set_clamped(count, object.ammo));
        if !written {
            return Status::Refused;
        }
        entity.start_reloading_ammo(object, now);
        Status::Ok
    }

    /// `Online`: a building goes offline or online as stock actions 61 and
    /// 62 turn it (GoOffline452360, GoOnline452260). Other objects answer
    /// `Unsupported`; a building that cannot turn answers `Refused`.
    pub(super) fn write_online(&mut self, id: u64, value: i64) -> Status {
        let Some(entity) = self.sim.entities().get(id) else {
            return Status::Gone;
        };
        if entity.category() != EntityCategory::Structure {
            return Status::Unsupported;
        }
        let online = value != 0;
        if entity.building_online() == online {
            return Status::Ok;
        }
        let turned = if online {
            self.sim
                .go_building_online(id, self.rules, self.frame_effects)
        } else {
            self.sim
                .go_building_offline(id, self.rules, self.frame_effects)
        };
        if turned { Status::Ok } else { Status::Refused }
    }
}
