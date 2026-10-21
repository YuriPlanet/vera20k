//! Command admission and scheduled batch execution.
//!
//! Preserves envelope insertion order, due filtering, house order and staged
//! group destinations. The world frame chooses ingress and tail call positions;
//! `world_commands` implements each individual command payload. Queue state
//! remains on Simulation for persistence and replay.

use super::Simulation;
use crate::sim::world::FrameEffects;

use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::movement::group_destination;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS;

impl Simulation {
    /// Queue one already-prepared command for future execution.
    ///
    /// This is an append-only ingress: it preserves the command owner, execution
    /// tick, and insertion position exactly. It does not sort, retime, normalize,
    /// intern, deduplicate, or apply the negotiated input delay.
    pub fn queue_command(&mut self, command: CommandEnvelope) {
        self.queue_commands([command]);
    }

    /// Queue an already-ordered batch for future execution.
    ///
    /// Commands are appended in iterator order with their owners, execution
    /// ticks, and payloads unchanged. Native byte round-trips and network timing
    /// remain the responsibility of the producer that prepares each envelope.
    pub fn queue_commands(&mut self, commands: impl IntoIterator<Item = CommandEnvelope>) {
        self.pending_commands.extend(commands);
    }

    /// Defensive queue snapshot for sealed input diagnostics and tests.
    /// Insertion order, stamps and payloads are copied without admitting,
    /// draining or exposing writable queue state. Ordinary rendering does not
    /// call this diagnostic boundary.
    pub fn pending_command_snapshot(&self) -> Vec<CommandEnvelope> {
        self.pending_commands.clone()
    }

    /// Whether provisional local selection still has authoritative queued
    /// work. Derive this from the existing queue; selected-bit equality cannot
    /// distinguish an unconsumed snapshot from later lifecycle deselection.
    pub fn has_pending_selection_commands(&self) -> bool {
        self.pending_commands
            .iter()
            .any(|command| matches!(command.payload, Command::Select { .. }))
    }

    /// Read-only input projection; accepted commands remain the sole authority.
    pub fn pending_paid_super_weapon_sequence(
        &self,
        owner: InternedId,
        source: InternedId,
    ) -> Option<u64> {
        self.pending_commands
            .iter()
            .filter_map(|command| match &command.payload {
                Command::BeginPaidSuperWeapon { selection, .. }
                    if command.owner == owner && selection.source == source =>
                {
                    Some(selection.sequence)
                }
                _ => None,
            })
            .max()
    }

    /// Provisional input projection of the existing queue, not a second
    /// lifecycle. In particular, cancel-before-source-exec and save-before-exec
    /// need no unsaved client nonce/latch. Admission is still execution-owned.
    pub fn projected_paid_super_weapon_selection(
        &self,
        owner: InternedId,
    ) -> Option<(crate::sim::superweapon::PaidSuperWeaponSelection, bool)> {
        let mut selected = self
            .paid_super_weapon_selection(owner)
            .map(|ticket| (ticket, false));
        for command in self
            .pending_commands
            .iter()
            .filter(|command| command.owner == owner)
        {
            match command.payload {
                Command::BeginPaidSuperWeapon { selection, .. } => {
                    selected = Some((selection, true))
                }
                Command::CancelPaidSuperWeapon { selection }
                | Command::CompletePaidSuperWeapon { selection, .. }
                    if selected.is_some_and(|(ticket, _)| ticket == selection) =>
                {
                    selected = None
                }
                _ => {}
            }
        }
        selected
    }

    /// Speed the next ordinary frame will observe after all due offline
    /// Options transitions execute in canonical house/insertion order.
    ///
    /// This read-only projection lets fresh-map and snapshot replacement sync
    /// client UI without consuming or applying a pending authoritative command.
    pub fn projected_in_game_options_speed(&self) -> Option<u8> {
        let mut projected = u8::try_from(self.session.game_options().game_speed)
            .ok()
            .filter(|speed| *speed <= crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED);
        let execute_tick = self.session.tick().saturating_add(1);
        for owner in self.due_command_house_order(&self.pending_commands, execute_tick) {
            for command in self
                .pending_commands
                .iter()
                .filter(|command| command.execute_tick <= execute_tick && command.owner == owner)
            {
                if !self.houses.contains_key(&owner) {
                    continue;
                }
                let (Command::SetGameSpeed { speed } | Command::LiveGameSpeed { speed }) =
                    &command.payload
                else {
                    continue;
                };
                if *speed <= crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED {
                    projected = Some(*speed);
                }
            }
        }
        projected
    }

    /// Consume the terminal edge raised by one executed EXIT command.
    pub fn take_executed_exit_owner(&mut self) -> Option<InternedId> {
        self.executed_exit_owner.take()
    }

    /// Drain the remote EXIT senders the executor recorded.
    pub fn take_departed_owners(&mut self) -> Vec<InternedId> {
        std::mem::take(&mut self.departed_owners)
    }

    /// Drain commands that are due for the next tick from `pending_commands`.
    /// Returns owned commands; remaining commands stay queued.
    pub fn take_due_commands(&mut self) -> Vec<CommandEnvelope> {
        let execute_tick = self.session.tick().saturating_add(1);
        let mut due = Vec::new();
        let mut kept = Vec::new();
        for cmd in std::mem::take(&mut self.pending_commands) {
            if cmd.execute_tick <= execute_tick {
                due.push(cmd);
            } else {
                kept.push(cmd);
            }
        }
        self.pending_commands = kept;
        due
    }

    /// Admit the current frame's Strip publication after the batch already
    /// accepted by the app. Native MainTick55D8AB -> Strip6A8B30 precedes
    /// Logic55DC9E/Factory55B680; LocalEvent6474EF preserves OutList insertion.
    /// Thus an already queued Building443860 SetRally precedes mobile PLACE.
    ///
    /// Playback may already contain that PLACE. Consume one exact fresh copy
    /// for each recorded envelope, without deduplicating player commands or
    /// draining unrelated/future work left in the scheduler.
    pub(super) fn admit_frame_commands(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
    ) -> Vec<CommandEnvelope> {
        let previous_pending = self.pending_commands.len();
        crate::sim::production::publish_production_changes(self, rules);
        let published = self.pending_commands.split_off(previous_pending);
        let mut admitted = commands.to_vec();
        self.admit_generated_frame_commands(&mut admitted, published, true);
        admitted
    }

    /// Local events produced during the frame join the existing batch in
    /// OutList insertion order before the canonical House-ordered dispatcher.
    /// Recorded diagnostic batches already contain these events; consume one
    /// exact regenerated copy per recorded entry only on that route. Ordinary
    /// player cancellation and a strip cancellation may be identical and both
    /// must execute. Future events stay with the one persisted queue owner.
    pub(super) fn admit_generated_frame_commands(
        &mut self,
        admitted: &mut Vec<CommandEnvelope>,
        mut generated: Vec<CommandEnvelope>,
        commands_include_generated: bool,
    ) {
        let execute_tick = self.session.tick().saturating_add(1);
        if commands_include_generated {
            for command in admitted.iter() {
                if command.execute_tick <= execute_tick
                    && let Some(index) = generated.iter().position(|fresh| fresh == command)
                {
                    generated.remove(index);
                }
            }
        }
        for command in generated {
            if command.execute_tick <= execute_tick {
                admitted.push(command);
            } else {
                self.pending_commands.push(command);
            }
        }
    }

    /// Admit and drain a diagnostic-replay batch, consuming exact matching
    /// due work regenerated by the simulation itself.
    ///
    /// The diagnostic log already contains the batch drained by the live app.
    /// Playback can independently recreate later work such as C4 scatter, so
    /// mixing that pending work with the recorded batch would admit it twice.
    /// Consume one exact due regenerated copy for each recorded envelope, then
    /// temporarily isolate the remaining queue:
    /// the recorded batch still uses [`Self::queue_commands`] and the canonical
    /// due filter, malformed future-stamped replay entries are discarded, and
    /// unrelated/future simulation work remains untouched. Keeping a consumed
    /// PLACE would leave replay/save state different from the ordinary app.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn take_due_replay_commands(
        &mut self,
        commands: impl IntoIterator<Item = CommandEnvelope>,
    ) -> Vec<CommandEnvelope> {
        #[cfg(feature = "debug-tools")]
        self.set_debug_session_context(crate::sim::cheats::DebugSessionContext::Replay);
        self.replaying_recorded_commands = true;
        let commands: Vec<_> = commands.into_iter().collect();
        let execute_tick = self.session.tick().saturating_add(1);
        for recorded in &commands {
            if recorded.execute_tick <= execute_tick
                && let Some(index) = self.pending_commands.iter().position(|cmd| cmd == recorded)
            {
                self.pending_commands.remove(index);
            }
        }
        let simulation_pending = std::mem::take(&mut self.pending_commands);
        self.queue_commands(commands);
        let due = self.take_due_commands();
        self.pending_commands = simulation_pending;
        due
    }

    /// VERA-only: one command and its spawn-refresh report: a Deploy whose MCV left
    /// the store or carries the `TerminalLifetime` dying mark counts as a spawn.
    fn apply_one_due_command(
        &mut self,
        cmd: &CommandEnvelope,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> (bool, bool) {
        // Scatter's found-cell arm immediately runs the locomotor. Its
        // authoritative bridge flag can change before the ordinary movement
        // phase, so that phase must rebuild its derived navigation view too.
        let scatter_bridge_before = match cmd.payload {
            Command::Scatter { entity_id } => self
                .substrate
                .entities
                .get(entity_id)
                .map(|entity| (entity_id, entity.on_bridge())),
            _ => None,
        };
        let cmd_owner_str = self.interner.resolve(cmd.owner).to_string();
        let applied = self.apply_command_with_overlays(
            &cmd_owner_str,
            &cmd.payload,
            rules,
            overlay_registry,
            frame_effects,
        );
        let placed_building_owner = self.successful_non_wall_placement_owner(cmd, applied, rules);
        let synchronous_deploy = applied
            && matches!(cmd.payload, Command::DeployMcv { entity_id }
            if self.substrate.entities.get(entity_id).is_none_or(|e| e.death.is_dying()));
        let spawned_entity = synchronous_deploy
            || placed_building_owner.is_some()
            || applied && matches!(cmd.payload, Command::PlaceProducedMobile { .. })
            || applied
                && matches!(
                    cmd.payload,
                    Command::LaunchSuperWeapon { .. }
                        | Command::CompletePaidSuperWeapon { .. }
                        | Command::BeginPaidSuperWeapon { .. }
                )
            || scatter_bridge_before.is_some_and(|(id, before)| {
                self.substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.on_bridge() != before)
            });
        (applied, spawned_entity)
    }

    pub(super) fn successful_non_wall_placement_owner(
        &self,
        command: &CommandEnvelope,
        applied: bool,
        rules: &RuleSet,
    ) -> Option<InternedId> {
        let Command::PlaceReadyBuilding { type_id, .. } = command.payload else {
            return None;
        };
        (applied
            && self
                .interner
                .object_type(type_id, rules)
                .is_some_and(|object| !object.wall))
        .then_some(command.owner)
    }

    fn command_uses_megamission(command: &Command) -> bool {
        if let Command::PlanningRecord { record } = command {
            return record.opcode() == crate::sim::command::MEGAMISSION_OPCODE;
        }
        command.megamission_actor().is_some()
    }

    /// The run key of `0x0064CDA0`'s caller: action, target and destination
    /// tokens (`0x0064CB0A..0x0064CB60`). `0x0064CDA0` returns at once unless
    /// the destination token is a Cell (`0x0064CDCA..0x0064CDDC`), so a Move
    /// onto an object, and an attack-move with a target or an object
    /// destination, have no key: each ends a run and is never spread. The
    /// click never sends an attack-move with both tokens.
    fn formation_key(command: &Command) -> Option<((u8, i32, u8), u16, u16)> {
        match command {
            Command::Move {
                target_rx,
                target_ry,
                ..
            } => Some(((2, 0, 0), *target_rx, *target_ry)),
            Command::AttackMove {
                target: None,
                destination: Some(crate::sim::combat::TargetKind::Cell(rx, ry)),
                ..
            } => Some(((29, 0, 0), *rx, *ry)),
            // 64CB0A..64CBDB groups all staged opcode4 records by mission,
            // target and destination tokens, including planning flags1/2.
            Command::PlanningRecord { record } if record.opcode() == 4 => {
                let p = record.payload();
                if p[16] != 11 {
                    return None;
                }
                let destination = i32::from_le_bytes(p[12..16].try_into().ok()?);
                let crate::sim::command::MegaMissionTarget::Cell { x, y } =
                    crate::sim::command::MegaMissionTarget::from_token(destination, 11)?
                else {
                    return None;
                };
                Some((
                    (p[5], i32::from_le_bytes(p[7..11].try_into().ok()?), p[11]),
                    x as u16,
                    y as u16,
                ))
            }
            _ => None,
        }
    }

    fn formation_entity_id(command: &Command) -> Option<u64> {
        match command {
            Command::Move { entity_id, .. } | Command::AttackMove { entity_id, .. } => {
                Some(*entity_id)
            }
            Command::PlanningRecord { record } if record.opcode() == 4 => {
                let p = record.payload();
                if p[4] != 0x34 {
                    return None;
                }
                u64::try_from(i32::from_le_bytes(p[0..4].try_into().ok()?)).ok()
            }
            _ => None,
        }
    }

    /// What `0x0064CDA0` reads once for each member it probes, before the
    /// candidates: the target's height (`0x0064D2AA..0x0064D2DF`), and the
    /// target's zone for the member type's MovementZone (`+0x5B4`, GetZoneID
    /// `0x0056D230` with bridge resolution on, `0x0064D407..0x0064D42C`).
    fn group_member_target(
        &self,
        rules: &RuleSet,
        terrain: &ResolvedTerrainGrid,
        zones: &ZoneGrid,
        clicked_target: (i16, i16),
        entity_id: u64,
    ) -> GroupMemberTarget {
        let entity = self
            .substrate
            .entities
            .get(entity_id)
            .expect("a group member stays live while its run is staged");
        let movement_zone = self
            .interner
            .object_type(entity.type_ref(), rules)
            .map_or_else(Default::default, |object| object.movement_zone);
        let cells = NativeCellQuery::canonical(terrain);
        GroupMemberTarget {
            entity_id,
            movement_zone,
            height: spread_height(terrain, cells.lookup(clicked_target)),
            zone: zones.get_zone_id_native(
                terrain,
                (clicked_target.0 as u16, clicked_target.1 as u16),
                movement_zone,
                true,
            ),
        }
    }

    /// One candidate's gates for one member, as `0x0064CDA0` reads them:
    /// - the candidate's cell (`0x0064D4D6`) and `IsCellInPlayfield(candidate,
    ///   1)` (`0x0064D4E9`);
    /// - its zone (`0x0056D230` with the cell's own bridge flag, `0x0064D51B`),
    ///   compared with the target's as the native DWORD (`0x0064D537`);
    /// - the member's own `Can_Enter_Cell` (`vt+0x1AC` at `0x0064D52F`) with
    ///   no direction, the target's height and no source cell;
    /// - the height band ([`within_spread_band`], `0x0064D53D..0x0064D592`).
    ///
    /// A `None` zone is an inactive-deck walk that native never finishes; it
    /// compares equal only to another `None`. A member whose `Can_Enter_Cell`
    /// VERA cannot read (a retired receiver, a missing type or overlay registry)
    /// refuses the candidate with 7 and a warning; native has no such outcome.
    fn group_destination_candidate_facts(
        &self,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        terrain: &ResolvedTerrainGrid,
        zones: &ZoneGrid,
        target: &GroupMemberTarget,
        candidate: (i16, i16),
    ) -> group_destination::CandidateFacts {
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup(candidate);
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(candidate.0), i32::from(candidate.1)),
            self.playfield_bounds,
            Some(terrain),
        ) {
            return group_destination::CandidateFacts::outside_playfield();
        }
        let id = target.entity_id;
        let bridge = terrain.native_cell_flags(cell) & BRIDGE_FLAG_STRUCTURAL != 0;
        let zone = zones.get_zone_id_native(
            terrain,
            (candidate.0 as u16, candidate.1 as u16),
            target.movement_zone,
            bridge,
        );
        let args = InfantryEntryArgs {
            direction: -1,
            height: target.height,
            previous_cell: None,
        };
        let can_enter_code = self
            .mover_can_enter(
                id,
                candidate,
                args,
                crate::sim::movement::infantry_entry::EntryQueryMode::CheckLocomotor,
                rules,
                registry,
            )
            .unwrap_or_else(|error| {
                log::warn!("group destination entry for {id}: {error}");
                7
            });
        group_destination::CandidateFacts {
            in_playfield: true,
            same_zone: zone == target.zone,
            height_band_ok: within_spread_band(target.height, spread_height(terrain, cell)),
            can_enter_code,
        }
    }

    /// Adjust consecutive same-target movement runs after their house's
    /// non-megamission scan, immediately before staged command execution.
    /// The group spread `0x0064CDA0` per run (`0x0064CB0A..0x0064CB60`): each Foot
    /// member's coordinates (Location) and source cell.
    pub(super) fn adjust_staged_megamission_destinations(
        &self,
        commands: &mut [CommandEnvelope],
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let (Some(terrain), Some(zones)) =
            (self.resolved_terrain.as_ref(), self.zone_grid.as_ref())
        else {
            return;
        };
        let mut run_start = 0;
        while run_start < commands.len() {
            let Some(key) = Self::formation_key(&commands[run_start].payload) else {
                run_start += 1;
                continue;
            };
            let mut run_end = run_start + 1;
            while run_end < commands.len()
                && Self::formation_key(&commands[run_end].payload) == Some(key)
            {
                run_end += 1;
            }
            if run_end - run_start > 1 {
                let mut members = Vec::new();
                for (command_index, command) in commands[run_start..run_end].iter().enumerate() {
                    let Some(entity_id) = Self::formation_entity_id(&command.payload) else {
                        continue;
                    };
                    let Some(entity) = self.substrate.entities.get(entity_id) else {
                        continue;
                    };
                    if !matches!(
                        entity.category(),
                        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                    ) || entity.low_bridge_tube_state.is_some()
                    {
                        continue;
                    }
                    let location =
                        crate::sim::movement::ground_pose::object_location(entity, Some(terrain));
                    members.push(group_destination::GroupDestinationMember {
                        command_index,
                        entity_id,
                        coord: [location.x, location.y, location.z],
                        source_cell: (entity.position.rx() as i16, entity.position.ry() as i16),
                    });
                }
                let clicked_target = (key.1 as i16, key.2 as i16);
                let mut member_target: Option<GroupMemberTarget> = None;
                let assignments = group_destination::distribute_group_destinations(
                    clicked_target,
                    &members,
                    |member, candidate| {
                        if member_target
                            .as_ref()
                            .is_none_or(|target| target.entity_id != member.entity_id)
                        {
                            member_target = Some(self.group_member_target(
                                rules,
                                terrain,
                                zones,
                                clicked_target,
                                member.entity_id,
                            ));
                        }
                        let target = member_target.as_ref().expect("read for this member");
                        self.group_destination_candidate_facts(
                            rules, registry, terrain, zones, target, candidate,
                        )
                    },
                );
                for assignment in assignments {
                    let Some(command) = commands.get_mut(run_start + assignment.command_index)
                    else {
                        continue;
                    };
                    match &mut command.payload {
                        Command::Move {
                            target_rx,
                            target_ry,
                            ..
                        }
                        | Command::AttackMove {
                            destination:
                                Some(crate::sim::combat::TargetKind::Cell(target_rx, target_ry)),
                            ..
                        } => {
                            *target_rx = assignment.destination.0 as u16;
                            *target_ry = assignment.destination.1 as u16;
                        }
                        Command::PlanningRecord { record } => {
                            let destination = crate::sim::command::MegaMissionTarget::Cell {
                                x: assignment.destination.0,
                                y: assignment.destination.1,
                            }
                            .token()
                            .expect("Cell target token");
                            record.payload_mut()[12..16]
                                .copy_from_slice(&destination.0.to_le_bytes());
                        }
                        _ => {}
                    }
                }
            }
            run_start = run_end;
        }
    }

    /// Canonical receiver order for every due command in one master frame.
    /// Registered HouseClass order wins; standalone-fixture owners are appended
    /// in their first issue order, matching the established tail dispatcher.
    fn due_command_house_order(
        &self,
        commands: &[CommandEnvelope],
        execute_tick: u64,
    ) -> Vec<InternedId> {
        let mut house_order = self.houses.order().to_vec();
        for command in commands
            .iter()
            .filter(|command| command.execute_tick <= execute_tick)
        {
            if !house_order.contains(&command.owner) {
                house_order.push(command.owner);
            }
        }
        house_order
    }

    fn command_uses_frame_ingress(command: &Command) -> bool {
        matches!(
            command,
            Command::SetGameSpeed { .. } | Command::Select { .. }
        )
    }

    /// Apply session/local-input transitions before triggers and live Logic.
    /// Offline Options stores GameSpeed before the next MainTick. Native
    /// MainTick55D8AB/55D8B4 -> Command Execute5367F0 -> P732280/Select5F4520
    /// also commits local selection before Logic55DC9E, unlike EventClass's
    /// late tail. Transport its snapshot through the existing receiver here,
    /// so a same-frame Destroy/Deselect cannot be undone by delayed replay.
    /// Source/order and executed cleanup controls:
    /// tools/input_oracle/selection_navigation.{py,json,meta.json,md}.
    pub(super) fn apply_due_frame_ingress_commands(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
        execute_tick: u64,
        frame_effects: FrameEffects<'_>,
    ) -> usize {
        let mut executed_commands = 0usize;
        for owner in self.due_command_house_order(commands, execute_tick) {
            for command in commands.iter().filter(|command| {
                command.execute_tick <= execute_tick
                    && command.owner == owner
                    && Self::command_uses_frame_ingress(&command.payload)
            }) {
                match &command.payload {
                    Command::SetGameSpeed { speed } => {
                        if self.houses.contains_key(&owner) {
                            let _ = self.session.apply_in_game_speed(*speed);
                        }
                    }
                    Command::Select { .. } => {
                        let _ = self.apply_one_due_command(command, rules, None, frame_effects);
                    }
                    _ => unreachable!("frame-ingress predicate admitted a tail command"),
                }
                // Preserve the established dispatcher convention: every due
                // envelope is consumed/counts even when validation rejects it.
                executed_commands += 1;
            }
        }
        executed_commands
    }

    /// Apply all due tail commands in HouseClass registration
    /// order. Each house preserves insertion order within the normal and
    /// staged-megamission streams. Returns
    /// `(executed_commands, spawned_entities)`. A sale or undeploy order only
    /// starts the Selling mission: the building leaves the map at its last
    /// visit (`Simulation::visit_building_down`).
    pub(crate) fn apply_due_commands(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
        execute_tick: u64,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> (usize, bool) {
        let mut executed_commands = 0usize;
        let mut spawned_entities = false;

        for owner in self.due_command_house_order(commands, execute_tick) {
            for command in commands.iter().filter(|command| {
                command.execute_tick <= execute_tick
                    && command.owner == owner
                    && !Self::command_uses_frame_ingress(&command.payload)
                    && !Self::command_uses_megamission(&command.payload)
            }) {
                let (_, spawned) =
                    self.apply_one_due_command(command, rules, overlay_registry, frame_effects);
                spawned_entities |= spawned;
                executed_commands += 1;
            }

            let mut staged = Vec::new();
            for command in commands.iter().filter(|command| {
                command.execute_tick <= execute_tick
                    && command.owner == owner
                    && Self::command_uses_megamission(&command.payload)
            }) {
                // Execute_DoList marks excess records processed without
                // executing them. The drained input batch already owns their
                // consumption; only accepted members reach group adjustment.
                crate::sim::command::stage_megamission(&mut staged, command);
            }
            self.adjust_staged_megamission_destinations(&mut staged, rules, overlay_registry);
            for command in &staged {
                let (_, spawned) =
                    self.apply_one_due_command(command, rules, overlay_registry, frame_effects);
                spawned_entities |= spawned;
                executed_commands += 1;
            }
        }

        (executed_commands, spawned_entities)
    }
}

/// The target facts `0x0064CDA0` reads once per probed member.
struct GroupMemberTarget {
    entity_id: u64,
    movement_zone: MovementZone,
    height: i32,
    zone: Option<u32>,
}

/// The spread's height of a Cell: its signed level (`+0x11B`), plus
/// `BRIDGE_DECK_HEIGHT_LEVELS` on the bridge flag (`+0x140 & 0x100`). The
/// target's is EBX (`0x0064D2AA..0x0064D2DF`), which Can_Enter_Cell receives;
/// a candidate's is measured against it (`0x0064D55F..0x0064D592`).
/// Native executions: tools/spatial_oracle/group_spread_gates.{py,json,meta.json}.
fn spread_height(terrain: &ResolvedTerrainGrid, cell: NativeCellIdentity) -> i32 {
    let level = i32::from(terrain.native_cell_ground_fields(cell).0 as i8);
    if terrain.native_cell_flags(cell) & BRIDGE_FLAG_STRUCTURAL != 0 {
        level + BRIDGE_DECK_HEIGHT_LEVELS
    } else {
        level
    }
}

/// A candidate more than two levels from the target's height ends the
/// member's probes (`JG` at `0x0064D575` and `0x0064D592`).
fn within_spread_band(target_height: i32, candidate_height: i32) -> bool {
    (candidate_height - target_height).abs() <= 2
}

#[cfg(test)]
#[path = "group_spread_gate_tests.rs"]
mod group_spread_gate_tests;
