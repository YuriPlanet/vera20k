//! Command admission and scheduled batch execution.
//!
//! Preserves envelope insertion order, due filtering, house order and staged
//! group destinations. The world frame chooses ingress and tail call positions;
//! `world_commands` implements each individual command payload. Queue state
//! remains on Simulation for persistence and replay.

use super::Simulation;
use super::queued_command::QueuedCommand;
use crate::sim::command_queue::{
    CommandDispatchClock, CommandDispatchHouse, CommandDispatchMode, DoListEvent, DoListHost,
    DoListRetimeWindow,
};
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
    /// Foot's cell click constructs MoveFlash before it issues the unit order
    /// (4D7EB5..4D8065), outside EventClass. In LAN/WOL the native flash joins
    /// a local unsynced list (4D7F28..4D8062). Keep that local action off the
    /// packet FIFO while using the existing ingress and animation owners.
    /// `spawn_move_flash` still declines network animation construction until
    /// that local visual owner exists. Offline flashes retain their established
    /// tail timing, including animation identities and any constructor RNG.
    fn command_uses_frame_ingress(command: &Command, is_network: bool) -> bool {
        command.uses_frame_ingress() || is_network && matches!(command, Command::MoveFlash { .. })
    }

    /// Queue_Options647040 checks the current House's result/death flags and
    /// appends OPTIONS. Full admission returns false without opening a menu.
    pub fn queue_options_request(&mut self) -> bool {
        let Some(owner) = self.session.current_house() else {
            return false;
        };
        let Some(house) = self.houses.get(&owner) else {
            return false;
        };
        if house.has_won() || house.has_lost() || house.is_to_die() {
            return false;
        }
        let Some(record) = self.encode_options_record(owner) else {
            return false;
        };
        self.issue_local_command(CommandEnvelope::synchronized(record, self.session.tick()))
    }

    /// Submit a freshly produced local command. Native events share the
    /// process-wide OutList cap; local selection/Options use their ingress.
    /// The result describes queue admission, not whether input was handled.
    pub fn issue_local_command(&mut self, command: CommandEnvelope) -> bool {
        // A recorded frame is the authoritative accepted batch. Run local
        // producers for their other effects, but never recreate a record
        // that live OutList saturation may have dropped.
        #[cfg(any(test, feature = "test-support"))]
        if self.replaying_recorded_commands {
            return false;
        }
        if Self::command_uses_frame_ingress(&command.payload, self.session.game_mode().is_network())
        {
            return self.admit_queued_command(QueuedCommand::from_envelope(command), false);
        }
        if let Command::SetRally {
            rx,
            ry,
            producer_ids,
        } = &command.payload
        {
            let mut accepted = true;
            for &producer in producer_ids {
                accepted &= self.local_commands.try_push(QueuedCommand::from_envelope(
                    CommandEnvelope::new(
                        command.owner,
                        command.execute_tick,
                        Command::SetRally {
                            rx: *rx,
                            ry: *ry,
                            producer_ids: vec![producer],
                        },
                    ),
                ));
            }
            return accepted;
        }
        self.local_commands
            .try_push(QueuedCommand::from_envelope(command))
    }

    pub fn issue_local_commands(&mut self, commands: impl IntoIterator<Item = CommandEnvelope>) {
        for command in commands {
            let _ = self.issue_local_command(command);
        }
    }

    fn queued_command_entries(&self) -> impl Iterator<Item = &QueuedCommand> + '_ {
        self.pending_commands
            .iter()
            .chain(self.local_commands.iter())
            .filter(|entry| !entry.is_processed())
    }

    fn queued_commands(&self) -> impl Iterator<Item = CommandEnvelope> + '_ {
        self.queued_command_entries()
            .map(|entry| entry.diagnostic_envelope(self))
    }

    /// Add_Event_To_DoList clears the processed bit only on fresh admission.
    /// The dispatcher reserves its moved FIFO's slots until retirement.
    pub(super) fn admit_queued_command(
        &mut self,
        mut command: QueuedCommand,
        record: bool,
    ) -> bool {
        command.clear_processed();
        let receipt = record.then(|| command.replay_envelope());
        let accepted = self
            .pending_commands
            .admit_with_reserved_native(command, self.dispatch_reserved_slots);
        if accepted && let Some(receipt) = receipt {
            self.frame_command_admissions.push(receipt);
        }
        accepted
    }

    /// Admit received bytes without decoding their actor, House or frame.
    /// Execute_DoList64C380 owns those checks at its post-Logic boundary.
    pub fn admit_native_command_record(
        &mut self,
        record: crate::sim::command::CommandRecord,
    ) -> bool {
        self.admit_queued_command(
            QueuedCommand::Native {
                record,
                diagnostic_issue_tick: self.session.tick(),
            },
            true,
        )
    }

    /// Offline transfer retains each native frame and all FIFO positions.
    /// Physical network send selection and retiming still belong to transport.
    pub(crate) fn transfer_local_commands(&mut self) {
        let count = self.local_commands.len();
        let entries: Vec<_> = self.local_commands.take_prefix(count).collect();
        for command in entries {
            #[cfg(any(test, feature = "test-support"))]
            if self.replaying_recorded_commands {
                // The recorded accepted stream owns due input. A snapshot
                // may also retain future input not present in this batch.
                let future = command.record().map_or_else(
                    || {
                        command.semantic().is_some_and(|entry| {
                            entry.execute_tick > self.session.tick().saturating_add(1)
                        })
                    },
                    |record| record.frame_stamp() > self.session.binary_frame() as i32,
                );
                if !future {
                    continue;
                }
            }
            self.admit_queued_command(command, true);
        }
    }

    /// Queue one already-admitted synchronized/deferred command, or a local
    /// frame-ingress action, for future execution. Fresh local events must use
    /// [`Self::issue_local_command`] so they retain their OutList occupancy.
    ///
    /// This is an append-only ingress: it preserves the command owner, execution
    /// tick, and insertion position exactly. It does not sort, retime, normalize,
    /// intern, deduplicate, or apply the negotiated input delay.
    pub fn queue_command(&mut self, command: CommandEnvelope) {
        #[cfg(any(test, feature = "test-support"))]
        if self.replaying_recorded_commands {
            return;
        }
        let record = !Self::command_uses_frame_ingress(
            &command.payload,
            self.session.game_mode().is_network(),
        );
        self.admit_queued_command(QueuedCommand::from_envelope(command), record);
    }

    /// Already-ordered arrivals share one admitted FIFO, including commands
    /// whose native codec has not yet been ported.
    pub fn queue_commands(&mut self, commands: impl IntoIterator<Item = CommandEnvelope>) {
        for command in commands {
            self.queue_command(command);
        }
    }

    /// Defensive queue snapshot for sealed input diagnostics and tests.
    /// Insertion order, stamps and payloads are copied without admitting,
    /// draining or exposing writable queue state. Ordinary rendering does not
    /// call this diagnostic boundary.
    pub fn pending_command_snapshot(&self) -> Vec<CommandEnvelope> {
        self.queued_commands().collect()
    }

    /// Whether provisional local selection still has authoritative queued
    /// work. Derive this from the existing queue; selected-bit equality cannot
    /// distinguish an unconsumed snapshot from later lifecycle deselection.
    pub fn has_pending_selection_commands(&self) -> bool {
        self.queued_commands()
            .any(|command| matches!(command.payload, Command::Select { .. }))
    }

    /// Read-only input projection; accepted commands remain the sole authority.
    pub fn pending_paid_super_weapon_sequence(
        &self,
        owner: InternedId,
        source: InternedId,
    ) -> Option<u64> {
        self.queued_commands()
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
            .queued_commands()
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

    /// Speed after the next ordinary frame's local Options ingress and then
    /// its admitted live-speed events, in canonical house/insertion order.
    ///
    /// This read-only projection lets fresh-map and snapshot replacement sync
    /// client UI without consuming or applying a pending authoritative command.
    pub fn projected_in_game_options_speed(&self) -> Option<u8> {
        let mut projected = u8::try_from(self.session.game_options().game_speed)
            .ok()
            .filter(|speed| *speed <= crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED);
        let execute_tick = self.session.tick().saturating_add(1);
        let queued: Vec<_> = self
            .queued_command_entries()
            .filter_map(|entry| entry.projected_envelope(self))
            .collect();
        let house_order = self.due_command_house_order(queued.iter(), execute_tick);
        // Match execution phases: all local Options ingress first, then all
        // eligible EventClass tails. Interleaving them by House can predict
        // a different speed from the one the frame actually commits.
        for at_tail in [false, true] {
            for &owner in &house_order {
                let Some(house) = self.houses.get(&owner) else {
                    continue;
                };
                if at_tail && !house.event_dispatch_eligible() {
                    continue;
                }
                for command in queued.iter().filter(|command| {
                    command.execute_tick <= execute_tick && command.owner == owner
                }) {
                    let speed = match (at_tail, &command.payload) {
                        (false, Command::SetGameSpeed { speed })
                        | (true, Command::LiveGameSpeed { speed }) => speed,
                        _ => continue,
                    };
                    if *speed <= crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED {
                        projected = Some(*speed);
                    }
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

    /// Only local frame-ingress actions leave the FIFO before Logic. Native
    /// and semantic tail events stay in place until the canonical DoList scan.
    pub fn take_due_commands(&mut self) -> Vec<CommandEnvelope> {
        let execute_tick = self.session.tick().saturating_add(1);
        let is_network = self.session.game_mode().is_network();
        self.pending_commands
            .take_where(|entry| {
                entry.semantic().is_some_and(|command| {
                    Self::command_uses_frame_ingress(&command.payload, is_network)
                        && command.execute_tick <= execute_tick
                })
            })
            .into_iter()
            .filter_map(|entry| match entry {
                QueuedCommand::Semantic { envelope, .. } => Some(envelope),
                _ => None,
            })
            .collect()
    }

    /// Component fixtures can drain a transferred batch instead of running
    /// unrelated world AI. Production never uses this early extraction.
    #[cfg(any(test, feature = "test-support"))]
    pub fn take_frame_commands_for_test(&mut self) -> Vec<CommandEnvelope> {
        self.transfer_local_commands();
        let native_frame = self.session.binary_frame() as i32;
        let tick = self.session.tick().saturating_add(1);
        let entries = self.pending_commands.take_where(|entry| {
            entry.record().map_or_else(
                || {
                    entry
                        .semantic()
                        .is_some_and(|command| command.execute_tick <= tick)
                },
                |record| record.frame_stamp() <= native_frame,
            )
        });
        entries
            .into_iter()
            .map(|entry| entry.diagnostic_envelope(self))
            .collect()
    }

    /// Native replay/input imports join the same FIFO at the frame boundary.
    /// Only local frame-ingress actions have already executed before Logic.
    pub(super) fn admit_frame_commands(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
    ) -> Vec<CommandEnvelope> {
        crate::sim::production::publish_production_changes(self, rules);
        for command in commands {
            if Self::command_uses_frame_ingress(
                &command.payload,
                self.session.game_mode().is_network(),
            ) {
                self.frame_command_admissions.push(command.clone());
            } else {
                self.admit_queued_command(QueuedCommand::from_envelope(command.clone()), true);
            }
        }
        Vec::new()
    }

    /// House-generated events use the same local/deferred owners. Playback
    /// supplies every accepted admission, so regenerated writes are suppressed.
    pub(super) fn issue_generated_frame_commands(&mut self, generated: Vec<CommandEnvelope>) {
        let execute_tick = self.session.tick().saturating_add(1);
        for command in generated {
            if command.execute_tick <= execute_tick {
                let _ = self.issue_local_command(command);
            } else {
                self.queue_command(command);
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn take_due_replay_commands(
        &mut self,
        commands: impl IntoIterator<Item = CommandEnvelope>,
    ) -> Vec<CommandEnvelope> {
        #[cfg(feature = "debug-tools")]
        self.set_debug_session_context(crate::sim::cheats::DebugSessionContext::Replay);
        self.replaying_recorded_commands = true;
        self.frame_command_admissions.clear();
        let commands: Vec<_> = commands.into_iter().collect();
        for recorded in &commands {
            let expected = QueuedCommand::from_envelope(recorded.clone());
            let same = |entry: &QueuedCommand| match (entry.record(), expected.record()) {
                (Some(a), Some(b)) => a == b,
                (None, None) => entry.semantic() == expected.semantic(),
                _ => false,
            };
            if self.pending_commands.remove_first(same).is_none() {
                self.local_commands.remove_first(same);
            }
        }
        commands
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
        // PLACE resolves the completed Factory head before dispatch and may
        // release it during settlement. Capture only this derived refresh
        // fact now; the production owner remains the placement authority.
        let placed_building_owner = self.successful_non_wall_placement_owner(cmd, true, rules);
        let cmd_owner_str = self.interner.resolve(cmd.owner).to_string();
        let applied = self.apply_command_with_overlays(
            &cmd_owner_str,
            &cmd.payload,
            rules,
            overlay_registry,
            frame_effects,
        );
        let placed_building_owner = placed_building_owner.filter(|_| applied);
        let synchronous_deploy = applied
            && matches!(cmd.payload, Command::DeployMcv { entity_id }
            if self.substrate.entities.get(entity_id).is_none_or(|e| e.death.is_dying()));
        let spawned_entity = synchronous_deploy
            || placed_building_owner.is_some()
            || applied && matches!(cmd.payload, Command::PlaceProducedMobile { .. })
            || applied
                && matches!(&cmd.payload, Command::SynchronizedRecord { record }
                if crate::sim::command::PlaceRecord::decode(record).is_some())
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
        if let Command::SynchronizedRecord { record } = &command.payload {
            let event = crate::sim::command::PlaceRecord::decode(record)?;
            let owner = *self
                .houses
                .order()
                .get(usize::try_from(event.house_id).ok()?)?;
            let category = crate::sim::production::ProductionSelection {
                rtti: event.rtti,
                type_index: event.type_index,
                naval: event.naval != 0,
            }
            .category(rules)?;
            let held = self
                .production
                .factories()
                .view(owner, category)?
                .complete_object()?;
            return (applied
                && self.object_type(held.type_id, rules).is_some_and(|object| {
                    object.category == crate::rules::object_type::ObjectCategory::Building
                        && !object.wall
                }))
            .then_some(owner);
        }
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

    /// The run key of `0x0064CDA0`'s caller: action, target and destination
    /// tokens (`0x0064CB0A..0x0064CB60`). `0x0064CDA0` returns at once unless
    /// the destination token is a Cell (`0x0064CDCA..0x0064CDDC`), so a Move
    /// onto an object, and an attack-move with a target or an object
    /// destination, have no key: each ends a run and is never spread. The
    /// click never sends an attack-move with both tokens.
    fn formation_key(command: &impl FormationCommand) -> Option<((u8, i32, u8), u16, u16)> {
        if let Some(record) = command.formation_record() {
            return Self::native_formation_key(record);
        }
        match command.formation_semantic()? {
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
            Command::PlanningRecord { record } => Self::native_formation_key(record),
            _ => None,
        }
    }

    fn native_formation_key(
        record: &crate::sim::command::CommandRecord,
    ) -> Option<((u8, i32, u8), u16, u16)> {
        if record.opcode() != 4 || record.payload()[16] != 11 {
            return None;
        }
        let p = record.payload();
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

    fn formation_entity_id(command: &impl FormationCommand) -> Option<u64> {
        let raw = command
            .formation_record()
            .or_else(|| match command.formation_semantic()? {
                Command::PlanningRecord { record } => Some(record),
                _ => None,
            });
        if let Some(record) = raw {
            let p = record.payload();
            if record.opcode() != 4 || p[4] != 0x34 {
                return None;
            }
            return u64::try_from(i32::from_le_bytes(p[0..4].try_into().ok()?)).ok();
        }
        match command.formation_semantic()? {
            Command::Move { entity_id, .. } | Command::AttackMove { entity_id, .. } => {
                Some(*entity_id)
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
        commands: &mut [impl FormationCommand],
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
            let Some(key) = Self::formation_key(&commands[run_start]) else {
                run_start += 1;
                continue;
            };
            let mut run_end = run_start + 1;
            while run_end < commands.len() && Self::formation_key(&commands[run_end]) == Some(key) {
                run_end += 1;
            }
            if run_end - run_start > 1 {
                let mut members = Vec::new();
                for (command_index, command) in commands[run_start..run_end].iter().enumerate() {
                    let Some(entity_id) = Self::formation_entity_id(command) else {
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
                    command.set_formation_destination(assignment.destination);
                }
            }
            run_start = run_end;
        }
    }

    /// Canonical receiver order for every due command in one master frame.
    /// Registered HouseClass order wins; standalone-fixture owners are appended
    /// in their first issue order, matching the established tail dispatcher.
    fn due_command_house_order<'a>(
        &self,
        commands: impl IntoIterator<Item = &'a CommandEnvelope>,
        execute_tick: u64,
    ) -> Vec<InternedId> {
        let mut house_order = self.houses.order().to_vec();
        for command in commands.into_iter().filter(|command| {
            Self::command_uses_frame_ingress(
                &command.payload,
                self.session.game_mode().is_network(),
            ) && command.execute_tick <= execute_tick
        }) {
            if !house_order.contains(&command.owner) {
                house_order.push(command.owner);
            }
        }
        house_order
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
        let is_network = self.session.game_mode().is_network();
        for owner in self.due_command_house_order(commands.iter(), execute_tick) {
            for command in commands.iter().filter(|command| {
                command.execute_tick <= execute_tick
                    && command.owner == owner
                    && Self::command_uses_frame_ingress(&command.payload, is_network)
            }) {
                match &command.payload {
                    Command::SetGameSpeed { speed } => {
                        if self.houses.contains_key(&owner) {
                            let _ = self.session.apply_in_game_speed(*speed);
                        }
                    }
                    Command::Select { .. } | Command::MoveFlash { .. } => {
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

    /// Apply admitted tail commands in HouseClass registration
    /// order. Each eligible house preserves insertion order within the normal and
    /// staged-megamission streams. Returns
    /// `(executed_commands, spawned_entities)`. A sale or undeploy order only
    /// starts the Selling mission: the building leaves the map at its last
    /// visit (`Simulation::visit_building_down`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn apply_due_commands(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
        execute_tick: u64,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> (usize, bool) {
        self.dispatch_owned_commands(
            commands,
            rules,
            execute_tick,
            overlay_registry,
            frame_effects,
        )
        .expect("component command dispatch must succeed")
    }

    pub(super) fn dispatch_owned_commands(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
        execute_tick: u64,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(usize, bool), (super::NetworkCommandError, usize, bool)> {
        let is_network = self.session.game_mode().is_network();
        for command in commands
            .iter()
            .filter(|command| !Self::command_uses_frame_ingress(&command.payload, is_network))
        {
            self.admit_queued_command(QueuedCommand::from_envelope(command.clone()), true);
        }
        let mut list = std::mem::take(&mut self.pending_commands);
        self.dispatch_reserved_slots = list.native_len();
        let mut houses: Vec<_> = self
            .houses
            .order()
            .iter()
            .enumerate()
            .filter_map(|(index, &owner)| {
                let house_id = i8::try_from(index).ok()?;
                Some(
                    CommandDispatchHouse::new(owner, house_id, true)
                        .with_current_player(self.session.current_house() == Some(owner)),
                )
            })
            .collect();
        if self.houses.is_empty() {
            for entry in list.iter() {
                if let Some(command) = entry.semantic()
                    && !houses.iter().any(|house| house.owner == command.owner)
                {
                    houses.push(CommandDispatchHouse::new(command.owner, -1, true));
                }
            }
        }
        let clock = CommandDispatchClock {
            native_frame: self.session.binary_frame() as i32,
            execute_tick,
        };
        let mode = if self.session.game_mode().command_queue_route()
            == super::CommandQueueRoute::Multiplayer
        {
            CommandDispatchMode::Network
        } else {
            CommandDispatchMode::Offline
        };
        let mut host = WorldCommandHost {
            sim: self,
            rules,
            overlay_registry,
            frame_effects,
            execute_tick,
            spawned: false,
            executed: 0,
        };
        let result = list.dispatch(clock, &houses, mode, &mut host);
        let spawned = host.spawned;
        let executed = host.executed;
        self.dispatch_reserved_slots = 0;
        let mut newly_admitted = std::mem::take(&mut self.pending_commands);
        list.append_retained(&mut newly_admitted);
        self.pending_commands = list;
        result
            .map(|summary| (summary.executed, spawned))
            .map_err(|cause| (cause, executed, spawned))
    }
}

/// One view of the shared formation algorithm for stored native records and
/// semantic component fixtures. It never resolves an actor to obtain a key.
pub(super) trait FormationCommand {
    fn formation_record(&self) -> Option<&crate::sim::command::CommandRecord>;
    fn formation_semantic(&self) -> Option<&Command>;
    fn set_formation_destination(&mut self, destination: (i16, i16));
}

fn write_formation_destination(
    record: &mut crate::sim::command::CommandRecord,
    destination: (i16, i16),
) {
    let token = crate::sim::command::MegaMissionTarget::Cell {
        x: destination.0,
        y: destination.1,
    }
    .token()
    .expect("Cell target token");
    record.payload_mut()[12..16].copy_from_slice(&token.0.to_le_bytes());
}

impl FormationCommand for Command {
    fn formation_record(&self) -> Option<&crate::sim::command::CommandRecord> {
        match self {
            Command::PlanningRecord { record } => Some(record),
            _ => None,
        }
    }
    fn formation_semantic(&self) -> Option<&Command> {
        Some(self)
    }
    fn set_formation_destination(&mut self, destination: (i16, i16)) {
        match self {
            Command::Move {
                target_rx,
                target_ry,
                ..
            }
            | Command::AttackMove {
                destination: Some(crate::sim::combat::TargetKind::Cell(target_rx, target_ry)),
                ..
            } => {
                *target_rx = destination.0 as u16;
                *target_ry = destination.1 as u16;
            }
            Command::PlanningRecord { record } => write_formation_destination(record, destination),
            _ => {}
        }
    }
}

impl FormationCommand for CommandEnvelope {
    fn formation_record(&self) -> Option<&crate::sim::command::CommandRecord> {
        self.payload.formation_record()
    }
    fn formation_semantic(&self) -> Option<&Command> {
        Some(&self.payload)
    }
    fn set_formation_destination(&mut self, destination: (i16, i16)) {
        self.payload.set_formation_destination(destination);
    }
}

impl FormationCommand for QueuedCommand {
    fn formation_record(&self) -> Option<&crate::sim::command::CommandRecord> {
        self.record()
            .or_else(|| self.semantic()?.formation_record())
    }
    fn formation_semantic(&self) -> Option<&Command> {
        self.semantic().map(|entry| &entry.payload)
    }
    fn set_formation_destination(&mut self, destination: (i16, i16)) {
        match self {
            QueuedCommand::Native { record, .. } => {
                write_formation_destination(record, destination)
            }
            QueuedCommand::Semantic { envelope, .. } => {
                envelope.set_formation_destination(destination)
            }
        }
    }
}

struct WorldCommandHost<'a, 'r, 'e> {
    sim: &'a mut Simulation,
    rules: &'r RuleSet,
    overlay_registry: Option<&'r crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'e>,
    execute_tick: u64,
    spawned: bool,
    executed: usize,
}

impl DoListHost<QueuedCommand> for WorldCommandHost<'_, '_, '_> {
    type Error = super::NetworkCommandError;

    fn house_is_eligible(&self, house: CommandDispatchHouse) -> bool {
        self.sim.houses.is_empty()
            || self
                .sim
                .houses
                .get(&house.owner)
                .is_some_and(|state| state.event_dispatch_eligible())
    }

    fn late_entry(&mut self, _house: CommandDispatchHouse, _entry: &QueuedCommand) {}

    fn frame_info(
        &mut self,
        _house: CommandDispatchHouse,
        entry: &QueuedCommand,
    ) -> Result<bool, Self::Error> {
        let record = entry.record().ok_or_else(|| {
            super::NetworkCommandError::Unavailable("FRAMEINFO requires native bytes".to_owned())
        })?;
        self.frame_effects
            .compare_frame_info(record, self.sim.session.binary_frame() as i32)
    }

    fn adjust_megamissions(&mut self, _house: CommandDispatchHouse, entries: &mut [QueuedCommand]) {
        self.sim
            .adjust_staged_megamission_destinations(entries, self.rules, self.overlay_registry);
    }

    fn execute(
        &mut self,
        _house: CommandDispatchHouse,
        entry: &QueuedCommand,
        _late: bool,
        _window: &mut DoListRetimeWindow,
    ) {
        self.executed += 1;
        let command = match entry {
            QueuedCommand::Native { record, .. } => self
                .sim
                .decode_native_command_record(record, self.execute_tick),
            QueuedCommand::Semantic { envelope, .. } => Some(envelope.clone()),
        };
        if let Some(command) = command {
            let (_, spawned) = self.sim.apply_one_due_command(
                &command,
                self.rules,
                self.overlay_registry,
                self.frame_effects,
            );
            self.spawned |= spawned;
        }
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

#[cfg(test)]
#[path = "command_admission_tests.rs"]
mod command_admission_tests;

#[cfg(test)]
#[path = "move_flash_ingress_tests.rs"]
mod move_flash_ingress_tests;
