//! Command admission: which House and grant source may command which object, and
//! how a command or native record resolves to the subject it acts on.

use super::CommandSubject;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{
    ArchiveRecord, AresTrenchRedirectRecord, Command, CommandRecord, MegaMissionRecord,
    MegaMissionTarget, SpecialPlaceRecord,
};
use crate::sim::control_authority::{CommandCapability, ControlScope};
use crate::sim::intern::{InternKey, InternedId};
use crate::sim::world::Simulation;

impl Simulation {
    /// Direct owner comparison for component fixtures.
    #[cfg(test)]
    pub(crate) fn entity_owned_by_id(&self, command_owner: &str, stable_id: u64) -> bool {
        self.command_entity_allowed(
            command_owner.into(),
            None,
            stable_id,
            CommandCapability::PrimaryHouse,
        )
    }

    /// Resolve the source of an admitted native record. Owner-only semantic
    /// envelopes and local-view identity cannot select a grant row.
    pub(in crate::sim::world) fn resolve_command_grant_source(
        &self,
        command_owner: InternKey<'_>,
        source: i8,
    ) -> Option<(i8, InternedId)> {
        let primary = self.admitted_grant_source_owner(source)?;
        self.interner
            .key_name(command_owner)
            .eq_ignore_ascii_case(self.interner.resolve(primary))
            .then_some((source, primary))
    }

    pub(super) fn admitted_grant_source_owner(&self, source: i8) -> Option<InternedId> {
        if !self.session.execution_topology().is_lockstep()
            || !self.session.has_custom_control_grants()
            || self
                .session
                .network_command_timing()
                .is_some_and(|timing| !timing.admits_source(source))
        {
            return None;
        }
        let primary = self.command_sender_owner(source)?;
        (self.control_scope_for_source(source, primary) == ControlScope::OwnFull).then_some(primary)
    }

    pub(in crate::sim::world) fn local_grant_source(&self) -> Option<(i8, InternedId)> {
        if self.session.watcher() {
            return None;
        }
        // A shared House's native index or a pending record's sender is not
        // proof of this process's admitted seat.
        let source = self.local_admitted_command_source()?;
        Some((source, self.admitted_grant_source_owner(source)?))
    }

    pub(in crate::sim::world) fn planning_command_actor_allowed(
        &self,
        source: (i8, InternedId),
        actor: u64,
    ) -> bool {
        self.command_entity_allowed(
            source.1.into(),
            Some(source),
            actor,
            CommandCapability::Tactical,
        )
    }
    pub(super) fn command_house_allowed(
        &self,
        command_owner: InternKey<'_>,
        granted_source: Option<(i8, InternedId)>,
        subject_house: InternedId,
        capability: CommandCapability,
    ) -> bool {
        let scope = match granted_source {
            Some((source, _)) => self.control_scope_for_source(source, subject_house),
            None if self
                .interner
                .key_name(command_owner)
                .eq_ignore_ascii_case(self.interner.resolve(subject_house)) =>
            {
                ControlScope::OwnFull
            }
            None => ControlScope::None,
        };
        scope.permits(capability)
    }

    pub(super) fn command_entity_allowed(
        &self,
        command_owner: InternKey<'_>,
        granted_source: Option<(i8, InternedId)>,
        stable_id: u64,
        capability: CommandCapability,
    ) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                self.command_house_allowed(
                    command_owner,
                    granted_source,
                    entity.owner(),
                    capability,
                )
            })
    }

    /// Project lockstep control and the receiver's slave gate for a
    /// prospective command before encoding or showing its feedback. The
    /// receiver rechecks the same subjects against the live state and its
    /// independently admitted source.
    pub fn can_issue_local_command(&self, command: &Command, rules: &RuleSet) -> bool {
        self.can_issue_local_command_with(&self.control_projection(), command, rules)
    }

    pub fn can_issue_local_command_with(
        &self,
        control: &super::super::ControlProjection<'_>,
        command: &Command,
        rules: &RuleSet,
    ) -> bool {
        if !self.session.has_custom_control_grants()
            || matches!(
                command,
                Command::Select { .. }
                    | Command::MoveFlash { .. }
                    | Command::AssignControlGroup { .. }
            )
        {
            return self.command_subject_can_receive_order(command, rules, None);
        }
        let Some((source, primary)) = control.source() else {
            return false;
        };
        let decoded;
        let command = if let Command::SynchronizedRecord { record }
        | Command::PlanningRecord { record } = command
        {
            if record.house_id() != source
                || super::super::planning::planning_dispatch_record(record)
            {
                return false;
            }
            let Some(envelope) = self.decode_native_command_record(record, self.session.tick())
            else {
                return false;
            };
            decoded = envelope.payload;
            &decoded
        } else {
            command
        };
        !self.network_command_actor_foreign(
            primary.into(),
            Some((source, primary)),
            command,
            rules,
            Some(control),
            false,
        ) && self.command_subject_can_receive_order(command, rules, Some(source))
    }

    pub(super) fn command_delegates_yard_redeploy(
        &self,
        granted_source: Option<(i8, InternedId)>,
        id: u64,
        rules: &RuleSet,
    ) -> bool {
        granted_source.is_some_and(|(source, _)| {
            self.substrate.entities.get(id).is_some_and(|yard| {
                !self.owner_is_human(yard.owner())
                    && self
                        .object_type(yard.type_ref(), rules)
                        .is_some_and(|object| object.construction_yard)
                    && self.control_scope_for_source(source, yard.owner())
                        == ControlScope::FullControl
            })
        })
    }

    /// No player order may act on an enslaved object. An enslaved unit's
    /// `TechnoClass::IsControllable @ 0x00700C40` (vt+0xA0) is false — the
    /// producer side already drops a slave through `techno_controllable_for`
    /// (`SlaveOwner +0x2DC`). A command that reaches the receiver without that
    /// producer (a replayed native record, a synchronized record, a captured
    /// object) must not slip past it: the action is refused here regardless of
    /// House, mode or source. A freed slave (`SlaveOwner` cleared) is an
    /// ordinary unit again and stays orderable.
    ///
    /// `resolve_command_subject` is the one mapping of a command to the
    /// objects it acts on, so this gate rides every path that reaches the
    /// receiver, including decoded native MEGAMISSION records.
    pub(super) fn command_subject_can_receive_order(
        &self,
        cmd: &Command,
        rules: &RuleSet,
        sender: Option<i8>,
    ) -> bool {
        let enslaved = |actor| {
            self.substrate
                .entities
                .get(actor)
                .is_some_and(|entity| entity.seldom.slave.owner().is_some())
        };
        match self.resolve_command_subject(cmd, rules) {
            CommandSubject::Actor(actor, _) => !enslaved(actor),
            // EjectBunker acts on the occupant (`issue_order(unit_id, ..)`),
            // not on the bunker that names it in the command.
            CommandSubject::Bunker(bunker) => !self
                .substrate
                .entities
                .get(bunker)
                .and_then(|building| building.bunker.occupant())
                .is_some_and(enslaved),
            CommandSubject::Actors(actors, _) => !actors.iter().copied().any(enslaved),
            // The Ares transfer owns the actual moving occupants and their permissions.
            CommandSubject::Trench(_, event) => {
                self.ares_trench_redirect_admits(sender, &event, rules)
            }
            _ => true,
        }
    }

    /// VERA-only network admission: an existing actor's current House must
    /// authorize the source for this command's capability.
    ///
    /// `EventClass::Execute @ 0x004C6CB0` trusts the sender for most object
    /// orders (MEGAMISSION, IDLE, Deploy, Repair, Primary, On/Off-line,
    /// Archive, DETONATE @ 0x004C7823, wall sale @ 0x004FCE80, PlanMgr
    /// connect @ 0x00639800 and append @ 0x00638120), so a modified client
    /// could retask, deploy, sell, power down or blow up an opponent's
    /// objects. Ordinary input orders `IsHumanPlayer @ 0x0050B6F0` actors,
    /// the sending House in every network mode; DETONATE is offered only to
    /// the bomb's House, whose house byte it carries. Offline modes keep the
    /// native receivers unchanged (campaign PlayerControl houses).
    ///
    /// Intended retail differences, all refused alike on every client:
    /// * retail input quirks that order a selected enemy object — the Guard
    ///   key's miner arm (`0x00730D60`) and Stop on an enemy building with a
    ///   rally line — do nothing online;
    /// * an order whose actor is captured or mind-controlled between issue
    ///   and execution is dropped if outside the source's control, including
    ///   its planning detach, where retail runs it on the new owner's object.
    ///   The flag-2 planning
    ///   acknowledge stays ahead of this gate, so that actor's latch clears.
    ///   The group spread (`0x0064CDA0`) leaves that actor out as well, so
    ///   the sender's other units spread as if it had not been ordered, where
    ///   retail spreads it with them;
    /// * a DETONATE whose carrier by execution holds another House's bomb, or
    ///   no bomb and belongs to another House, is dropped, where retail clears
    ///   the carrier's planning tokens and sets off whatever bomb it holds.
    ///
    /// A Wasm module's `house_order` (RFC 0001 6g) is its house's own
    /// sender: under custom control grants its house-wide commands
    /// (production, placement, superweapons) need no granted source.
    pub(super) fn network_command_actor_foreign(
        &self,
        command_owner: InternKey<'_>,
        granted_source: Option<(i8, InternedId)>,
        cmd: &Command,
        rules: &RuleSet,
        control: Option<&super::super::ControlProjection<'_>>,
        house_order: bool,
    ) -> bool {
        if !self.session.execution_topology().is_lockstep() {
            return false;
        }
        let house_refused = |house, capability| {
            !control
                .and_then(|view| view.permits(house, capability))
                .unwrap_or_else(|| {
                    self.command_house_allowed(command_owner, granted_source, house, capability)
                })
        };
        let actor_refused = |actor, capability| {
            self.substrate
                .entities
                .get(actor)
                .is_some_and(|entity| house_refused(entity.owner(), capability))
        };
        match self.resolve_command_subject(cmd, rules) {
            CommandSubject::Refused => true,
            CommandSubject::Local => false,
            CommandSubject::Primary(capability) => {
                self.session.has_custom_control_grants()
                    && !house_order
                    && granted_source.is_none_or(|(_, house)| house_refused(house, capability))
            }
            CommandSubject::House(house, capability) => house_refused(house, capability),
            CommandSubject::TargetHouse(target) => {
                granted_source.is_none()
                    || target
                        .is_none_or(|house| house_refused(house, CommandCapability::FullControl))
            }
            CommandSubject::Actor(actor, capability) => actor_refused(actor, capability),
            CommandSubject::Actors(actors, capability) => {
                actors.iter().any(|&actor| actor_refused(actor, capability))
            }
            CommandSubject::Bunker(bunker) => {
                actor_refused(bunker, CommandCapability::Tactical)
                    || granted_source.is_some()
                        && self
                            .substrate
                            .entities
                            .get(bunker)
                            .and_then(|building| building.bunker.occupant())
                            .is_some_and(|occupant| {
                                actor_refused(occupant, CommandCapability::Tactical)
                            })
            }
            CommandSubject::Trench(building, _) => {
                actor_refused(building, CommandCapability::Tactical)
            }
        }
    }

    fn resolve_command_subject<'a>(&self, cmd: &'a Command, rules: &RuleSet) -> CommandSubject<'a> {
        use CommandCapability::{FullControl, PrimaryHouse, Tactical};

        match cmd {
            Command::ExitMatch
            | Command::Destruct
            | Command::Ally { .. }
            | Command::RemovePlayer { .. }
            | Command::LiveGameSpeed { .. }
            | Command::SetGameSpeed { .. }
            | Command::OpenOptions
            | Command::SetLatencyFudge { .. }
            | Command::Cheer
            | Command::ModuleMessage { .. } => CommandSubject::Primary(PrimaryHouse),
            Command::SellBuilding { entity_id }
            | Command::ToggleRepair { entity_id }
            | Command::SetBuildingOnLine { entity_id, .. }
            | Command::TogglePrimaryFactory { entity_id }
            | Command::UndeployBuilding { entity_id } => {
                CommandSubject::Actor(*entity_id, FullControl)
            }
            Command::SellWallAtCell { x, y } => self
                .wall_sale_cell(*x, *y)
                .and_then(|(_, _, cell)| cell.wall_owner)
                .map_or(CommandSubject::Local, |owner| {
                    CommandSubject::House(owner, FullControl)
                }),
            Command::QueueProduction { .. }
            | Command::SuspendProduction { .. }
            | Command::CycleProducerFocus { .. }
            | Command::PlaceReadyBuilding { .. }
            | Command::CancelProductionByType { .. }
            | Command::PlaceProducedMobile { .. }
            | Command::LaunchSuperWeapon { .. }
            | Command::BeginPaidSuperWeapon { .. }
            | Command::CompletePaidSuperWeapon { .. }
            | Command::CancelPaidSuperWeapon { .. } => CommandSubject::Primary(FullControl),
            Command::Move { entity_id, .. }
            | Command::MoveToObject { entity_id, .. }
            | Command::Stop { entity_id }
            | Command::AttackMove { entity_id, .. }
            | Command::Guard { entity_id, .. }
            | Command::Scatter { entity_id }
            | Command::DeployMcv { entity_id }
            | Command::ToggleInfantryDeploy { entity_id }
            | Command::ArchiveObject { entity_id, .. }
            | Command::MegaMission { entity_id, .. } => CommandSubject::Actor(*entity_id, Tactical),
            Command::Attack { attacker_id, .. }
            | Command::ForceAttackCell { attacker_id, .. }
            | Command::PlantC4 { attacker_id, .. } => CommandSubject::Actor(*attacker_id, Tactical),
            Command::CaptureBuilding { engineer_id, .. } => {
                CommandSubject::Actor(*engineer_id, Tactical)
            }
            Command::UnloadPassengers { transport_id } => {
                CommandSubject::Actor(*transport_id, Tactical)
            }
            Command::EjectBunker { bunker_id } => CommandSubject::Bunker(*bunker_id),
            Command::SetRally { producer_ids, .. } => {
                CommandSubject::Actors(producer_ids, Tactical)
            }
            Command::PlanningRecord { record } => {
                super::super::planning::planning_record_actor(record)
                    .map_or(CommandSubject::Primary(Tactical), |actor| {
                        CommandSubject::Actor(actor, Tactical)
                    })
            }
            Command::SynchronizedRecord { record } => {
                self.resolve_native_command_subject(record, rules)
            }
            Command::Select { .. }
            | Command::MoveFlash { .. }
            | Command::AssignControlGroup { .. } => CommandSubject::Local,
        }
    }

    fn resolve_native_command_subject(
        &self,
        record: &CommandRecord,
        rules: &RuleSet,
    ) -> CommandSubject<'static> {
        use CommandCapability::{FullControl, PrimaryHouse, Tactical};

        match record.opcode() {
            // A bomb belongs to its planting House regardless of its carrier.
            // An empty carrier's plan can only be cleared by the carrier owner.
            crate::sim::command::DETONATE_OPCODE => {
                crate::sim::command::DetonateRecord::decode(record)
                    .and_then(|event| u64::try_from(event.entity_id).ok())
                    .and_then(|id| self.substrate.entities.get(id))
                    .map_or(CommandSubject::Local, |entity| match &entity.bomb {
                        Some(bomb) => CommandSubject::House(bomb.house(), Tactical),
                        None => CommandSubject::House(entity.owner(), PrimaryHouse),
                    })
            }
            crate::sim::command::ARCHIVE_OPCODE => ArchiveRecord::decode(record)
                .and_then(|event| u64::try_from(event.entity_id).ok())
                .map_or(CommandSubject::Local, |actor| {
                    CommandSubject::Actor(actor, Tactical)
                }),
            // The source trench and heads that will transfer are ordered.
            // The destination Cell is still governed by native traverse admission.
            crate::sim::command::ARES_TRENCH_REDIRECT_OPCODE if rules.ares() => {
                AresTrenchRedirectRecord::decode(record)
                    .and_then(|event| {
                        let MegaMissionTarget::Object { id } = event.source else {
                            return None;
                        };
                        Some(CommandSubject::Trench(u64::try_from(id).ok()?, event))
                    })
                    .unwrap_or(CommandSubject::Local)
            }
            crate::sim::command::MEGAMISSION_OPCODE => MegaMissionRecord::decode(record)
                .and_then(|event| u64::try_from(event.source_id).ok())
                .map_or(CommandSubject::Local, |actor| {
                    CommandSubject::Actor(actor, Tactical)
                }),
            crate::sim::command::TARGET_HOUSE_OPCODE => CommandSubject::TargetHouse(
                crate::sim::command::TargetHouseRecord::decode(record).and_then(|event| {
                    self.houses
                        .order()
                        .get(usize::from(event.target_house))
                        .copied()
                }),
            ),
            crate::sim::command::PRODUCE_OPCODE
            | crate::sim::command::SUSPEND_OPCODE
            | crate::sim::command::ABANDON_OPCODE
            | crate::sim::command::ABANDON_ALL_OPCODE
            | crate::sim::command::PLACE_OPCODE
            | crate::sim::command::VERA_PAID_SUPER_OPCODE
            | crate::sim::command::VERA_CYCLE_PRODUCER_OPCODE
            | SpecialPlaceRecord::OPCODE => CommandSubject::Primary(FullControl),
            // Only decoded receivers above may execute synchronized records.
            _ => CommandSubject::Refused,
        }
    }

    /// The lockstep scope test shared by dispatch and formation. Let native
    /// receiver checks handle missing actors and their existing side effects.
    pub(in crate::sim::world) fn command_actor_outside_scope(
        &self,
        command_owner: InternKey<'_>,
        granted_source: Option<(i8, InternedId)>,
        id: u64,
        capability: CommandCapability,
    ) -> bool {
        self.substrate.entities.get(id).is_some_and(|entity| {
            !self.command_house_allowed(command_owner, granted_source, entity.owner(), capability)
        })
    }
}
