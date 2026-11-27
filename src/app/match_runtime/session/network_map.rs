//! The network room's map: map metadata and verification, local, external
//! and random-map sources, prepared filenames, `SelectScenario` and
//! `ChooseMap` selection, and the mode's AI preparation.

use crate::skirmish_launch::{NETWORK_PLAYER_SLOT_COUNT, SkirmishAiSlot, SkirmishLaunchSession};

use super::{
    MatchSession, NetworkMapMetadata, NetworkPhase, NetworkRowKind, NetworkSession, NetworkVacancy,
    host_extra_row, native_network_row_defaults, row_capacity, validate_network_rows,
};

impl MatchSession {
    pub(crate) fn network_map_metadata(&self) -> Option<&NetworkMapMetadata> {
        self.network.as_ref()?.map_metadata.as_ref()
    }

    ///5E4F49 mapsame flag, independent of general option/readiness changes.
    /// Call before host setup admission, avoiding a second filename cache.
    pub(crate) fn network_map_requires_verification(
        &self,
        incoming: &SkirmishLaunchSession,
        metadata: &NetworkMapMetadata,
    ) -> anyhow::Result<bool> {
        let network = self
            .network
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let old = self
            .network_setup()
            .ok_or_else(|| anyhow::anyhow!("no network setup"))?;
        let Some(previous) = &network.map_metadata else {
            return Ok(true);
        };
        Ok(previous.display_name.is_empty()
            || previous.player_capacity != metadata.player_capacity
            || old.mode.id != incoming.mode.id
            || !old
                .selected_map_file
                .as_deref()
                .unwrap_or("")
                .eq_ignore_ascii_case(incoming.selected_map_file.as_deref().unwrap_or(""))
            || previous.official != metadata.official
            || previous.byte_length != metadata.byte_length
            || previous.digest != metadata.digest
            || previous.source_sha256 != metadata.source_sha256
            || previous.effective_digest != metadata.effective_digest)
    }

    /// Local qualified map title takes precedence; absent local bytes use the
    /// nonempty advertised title. An unchanged packet leaves the cached title.
    pub(crate) fn accept_network_map_metadata(
        &mut self,
        mut metadata: NetworkMapMetadata,
        local_title: Option<&str>,
        refresh_description: bool,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::Setup {
            launch, capacity, ..
        } = &network.phase
        else {
            anyhow::bail!("network setup is not editable");
        };
        let expected_capacity = row_capacity(
            network.return_context.session_mode(),
            network.room,
            &launch.mode,
            metadata.player_capacity,
        );
        anyhow::ensure!(
            *capacity == expected_capacity,
            "map metadata and derived setup capacity disagree"
        );
        let previous_title = network
            .map_metadata
            .as_ref()
            .map_or("", |old| old.display_name.as_str());
        if refresh_description {
            if let Some(title) = local_title {
                metadata.display_name = title.to_owned();
            } else if metadata.display_name.is_empty() {
                metadata.display_name = previous_title.to_owned();
            }
        } else {
            metadata.display_name = previous_title.to_owned();
        }
        if network.map_metadata.as_ref().is_none_or(|previous| {
            previous.source_sha256 != metadata.source_sha256
                || previous.byte_length != metadata.byte_length
                || previous.effective_digest != metadata.effective_digest
        }) {
            network.local_map_source = None;
            network.local_map_effective_digest = None;
            network.local_random_map_preview = None;
            if let NetworkPhase::Setup { admission, .. } = &mut network.phase {
                for entry in admission {
                    entry.reported_map_status = None;
                }
            }
        }
        network.map_metadata = Some(metadata);
        Ok(())
    }

    pub(crate) fn network_local_map_source(&self) -> Option<&crate::map::source::LoadedMapSource> {
        self.network.as_ref()?.local_map_source.as_ref()
    }

    /// Bind content only to its currently accepted map identity. A late download
    /// cannot replace a newly selected map or acknowledge it as present.
    /// An external launch's map is the launcher's file beside spawn.ini:
    /// each participant loads its own copy, while the shared logical name
    /// (the setup's `selected_map_file`) keeps every world identical.
    pub(crate) fn accept_external_network_map_source(
        &mut self,
        source: crate::map::source::LoadedMapSource,
        effective_digest: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network_setup().is_some(),
            "map binding requires an admitted setup"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        network.local_map_source = Some(source);
        network.local_map_effective_digest = Some(effective_digest);
        Ok(())
    }

    pub(crate) fn accept_verified_network_map_source(
        &mut self,
        source: crate::map::source::LoadedMapSource,
        effective_digest: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network_setup().is_some(),
            "map binding requires editable setup"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let expected = network
            .map_metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no map metadata"))?;
        let (length, digest) = source
            .physical_identity()
            .ok_or_else(|| anyhow::anyhow!("map source has no physical identity"))?;
        anyhow::ensure!(
            length == usize::try_from(expected.byte_length)?
                && crate::map::source::valid_source_sha256(digest)
                && digest == expected.source_sha256
                && effective_digest == expected.effective_digest,
            "local source differs from the selected map"
        );
        network.local_map_source = Some(source);
        network.local_map_effective_digest = Some(effective_digest);
        Ok(())
    }

    pub(crate) fn network_local_map_effective_digest(&self) -> Option<&str> {
        self.network.as_ref()?.local_map_effective_digest.as_deref()
    }

    pub(crate) fn network_random_map_preview(
        &self,
    ) -> Option<&crate::map::source::LoadedMapSource> {
        self.network.as_ref()?.local_random_map_preview.as_ref()
    }
    pub(crate) fn clear_network_random_map_preview(&mut self) {
        if let Some(network) = self.network.as_mut() {
            network.local_random_map_preview = None;
        }
    }

    pub(crate) fn accept_network_random_map_preview(
        &mut self,
        seed_digest: &str,
        source: crate::map::source::LoadedMapSource,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        anyhow::ensure!(
            network
                .map_metadata
                .as_ref()
                .is_some_and(|meta| meta.source_sha256 == seed_digest),
            "preview belongs to another seed"
        );
        network.local_random_map_preview = Some(source);
        Ok(())
    }

    /// DD8A0 copies the prepared filename to Scenario+125C atDD97F:
    /// local6951F0 resolves catalog spelling; successful693B70 supplies the
    /// host FILE_INFO name directly, without catalog insertion.
    pub(crate) fn accept_prepared_network_map_filename(
        &mut self,
        file: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host,
            "host cannot accept its own guest map-preparation result"
        );
        anyhow::ensure!(
            !file.is_empty() && file.trim() == file && !file.eq_ignore_ascii_case("auto"),
            "prepared network map requires an exact filename"
        );
        self.editable_network_setup()?.selected_map_file = Some(file);
        Ok(())
    }
}

impl MatchSession {
    /// `MPGameOptions__SelectScenario` (`0x005E7BF0`): the map file and the
    /// row count `[0xA8BB0C]` (the map's players, capped by the mode and
    /// the WOL room) only. Unlike ChooseMap it re-cuts no rows and keeps
    /// every start and team.
    pub(crate) fn select_network_scenario(
        &mut self,
        file: String,
        capacity: i32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !file.is_empty() && capacity > 0,
            "map requires a valid participant capacity"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let session_mode = network.return_context.session_mode();
        let room = network.room;
        let NetworkPhase::Setup {
            launch,
            capacity: rows,
            ..
        } = &mut network.phase
        else {
            anyhow::bail!("network setup is not editable");
        };
        *rows = row_capacity(session_mode, room, &launch.mode, capacity);
        launch.selected_map_file = Some(file);
        Ok(())
    }

    /// Accepted BC ChooseMap5DE520: atomic map/mode selection followed by
    /// capacity5EC3A0, starts5EE6A0 and all-row team defaults5EE510(-1).
    /// Unbound human nodes are returned for service Kick intents. Their source
    /// membership survives until the separate type10 departure response.
    /// A VERA room keeps every observer row and closes the other rows past
    /// its players.
    pub(crate) fn select_network_map(
        &mut self,
        mode: crate::skirmish_launch::SkirmishLaunchMode,
        file: String,
        capacity: i32,
    ) -> anyhow::Result<Vec<crate::skirmish_launch::SkirmishHumanSlot>> {
        anyhow::ensure!(self.host, "only host may choose the map");
        anyhow::ensure!(
            !file.is_empty() && capacity > 0,
            "map requires a valid participant capacity"
        );
        // Validated on a copy, committed on success: a refused change leaves the
        // session as it was.
        let mut candidate = self.clone();
        let several = self.network_several_observers();
        let (old_capacity, observer_row, host_extra, new_capacity) = {
            let Some(NetworkSession {
                phase:
                    NetworkPhase::Setup {
                        launch,
                        capacity: previous,
                        observer_row,
                        admission,
                        ..
                    },
                return_context,
                room,
                ..
            }) = candidate.network.as_mut()
            else {
                anyhow::bail!("network setup is not editable");
            };
            // VERA: a guest sends K again for another map or mode (the
            // 5E4F49 mapsame test), which is news to the host's room.
            let old_file = launch.selected_map_file.as_deref().unwrap_or("");
            if launch.mode.id != mode.id || !old_file.eq_ignore_ascii_case(&file) {
                for entry in admission.iter_mut() {
                    entry.reported_map_status = None;
                }
            }
            let session_mode = return_context.session_mode();
            let room = *room;
            // Cooperative's +0x20 (`0x005C1A10`) clears the observer
            // reservation whenever the mode becomes current: LAN and WOL setup
            // (`0x005ED6FE`, `0x00791293`) call it without comparing with the
            // previous mode, so a restored setup's first map pick clears it too.
            if !mode.class.policy().observers_allowed {
                *observer_row = None;
            }
            let new_capacity = row_capacity(session_mode, room, &mode, capacity);
            let host_extra = i32::from(
                *observer_row == launch.local_player().raw_slot_index
                    && host_extra_row(session_mode, room, &mode, new_capacity),
            );
            let old = *previous;
            *previous = new_capacity;
            launch.mode = mode;
            launch.selected_map_file = Some(file);
            (old, *observer_row, host_extra, new_capacity)
        };
        let mut removed = Vec::new();
        let local_row = candidate
            .network_setup()
            .unwrap()
            .local_player()
            .raw_slot_index;
        // A VERA room's players end at its player row limit, which the host's
        // own observer row extends at every capacity. Its people past them
        // watch there and its AI leave, unless it shares bases and keeps
        // them.
        candidate.keep_to_map_seats()?;
        let (old_rows, new_rows) = if several {
            (
                i32::from(self.network_player_row_limit()),
                i32::from(candidate.network_player_row_limit()),
            )
        } else {
            (
                old_capacity.saturating_add(host_extra),
                new_capacity.saturating_add(host_extra),
            )
        };
        if new_rows < old_rows {
            for row in new_rows..NETWORK_PLAYER_SLOT_COUNT as i32 {
                let row = row as u8;
                // VERA's rooms keep every observer row, and one sharing
                // bases its players too.
                let setup = candidate.network_setup().unwrap();
                let held = setup
                    .humans
                    .iter()
                    .any(|human| human.raw_slot_index == Some(row))
                    || setup.opponents.iter().any(|ai| ai.raw_slot_index == row);
                let retained = (several || Some(row) == observer_row)
                    && candidate.network_row_for_observer(row)
                    || candidate.network_shares_bases() && held;
                if Some(row) != local_row && !retained {
                    // Native's hidden eight-row suffix stays Open. Extended
                    // rows have no native slot and must remain closed until a
                    // later map explicitly makes them usable again, as must
                    // a VERA room's spectator rows.
                    let kind = if several || row >= 8 || new_capacity > 8 {
                        NetworkRowKind::Closed
                    } else {
                        NetworkRowKind::Open
                    };
                    removed.extend(candidate.set_network_row_kind(row, kind)?);
                }
            }
        } else if new_rows > old_rows {
            for row in old_rows..new_rows.min(NETWORK_PLAYER_SLOT_COUNT as i32) {
                let row = row as u8;
                // A VERA room's reserved row is no different from the others.
                if Some(row) == local_row || (!several && Some(row) == observer_row) {
                    continue;
                }
                let setup = candidate.network_setup().unwrap();
                let seated = |row: u8| {
                    setup
                        .humans
                        .iter()
                        .position(|human| human.raw_slot_index == Some(row))
                };
                if let Some(index) = seated(row) {
                    // Its spectators move on past the new players.
                    let free = (new_rows as u8..NETWORK_PLAYER_SLOT_COUNT as u8)
                        .find(|row| seated(*row).is_none())
                        .filter(|_| several && setup.humans[index].is_observer());
                    let Some(free) = free else {
                        continue;
                    };
                    candidate.editable_network_setup()?.humans[index].raw_slot_index = Some(free);
                }
                let setup = candidate.network_setup().unwrap();
                let kind = setup
                    .opponents
                    .iter()
                    .find(|ai| ai.raw_slot_index == row)
                    .map_or(NetworkRowKind::Open, |ai| NetworkRowKind::Ai(ai.difficulty));
                candidate.set_network_row_kind(row, kind)?;
            }
        }
        // A VERA room seats the new map's players: slots its host removed
        // open again.
        if several {
            for row in 0..new_rows.clamp(0, NETWORK_PLAYER_SLOT_COUNT as i32) as u8 {
                if candidate.network_vacancy(row) == Some(NetworkVacancy::Closed) {
                    candidate.set_network_row_kind(row, NetworkRowKind::Open)?;
                }
            }
        }
        //5EC030 clears former nonlocal observer slots when activation cleared
        //the reserved row. A subsequent ordinary mode reserves row7 below.
        if observer_row.is_none() {
            let observer_rows: Vec<_> = candidate
                .network_setup()
                .unwrap()
                .humans
                .iter()
                .filter(|human| human.raw_slot_index != local_row && human.is_observer())
                .filter_map(|human| human.raw_slot_index)
                .chain((0..NETWORK_PLAYER_SLOT_COUNT as u8).filter(|row| {
                    candidate.network_vacancy(*row) == Some(NetworkVacancy::OpenObserver)
                }))
                .collect();
            for row in observer_rows {
                // A VERA room's rows past its players stay Closed.
                let kind = if several && i32::from(row) >= new_rows {
                    NetworkRowKind::Closed
                } else {
                    NetworkRowKind::Open
                };
                removed.extend(candidate.set_network_row_kind(row, kind)?);
            }
        }
        let ordinary = candidate
            .network_setup()
            .unwrap()
            .mode
            .class
            .policy()
            .observers_allowed;
        let default_observer = if new_capacity > 8 {
            NETWORK_PLAYER_SLOT_COUNT as u8 - 1
        } else {
            7
        };
        let reserved = observer_row
            .filter(|row| {
                Some(*row) == local_row || new_capacity <= 8 || *row >= new_capacity as u8
            })
            .or(ordinary.then_some(default_observer));
        if let Some(row) =
            reserved.filter(|row| Some(*row) != local_row && i32::from(*row) > new_capacity)
        {
            if !candidate.network_row_for_observer(row) {
                removed.extend(candidate.set_network_row_kind(row, NetworkRowKind::Closed)?);
            }
        }
        // A VERA room with Teams set keeps its players' teams.
        let keep_teams = candidate.network_teams().is_some();
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    inactive_ai_options,
                    observer_row,
                    ..
                },
            ..
        }) = candidate.network.as_mut()
        else {
            unreachable!()
        };
        *observer_row = reserved;
        let human_team = native_network_row_defaults(&launch.mode, false).team;
        let ai_team = native_network_row_defaults(&launch.mode, true).team;
        for human in &mut launch.humans {
            human.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            if !keep_teams {
                human.slot.team = human_team;
            }
        }
        for ai in &mut launch.opponents {
            ai.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            if !keep_teams {
                ai.team = ai_team;
            }
        }
        for choices in inactive_choices {
            choices.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            choices.team = human_team;
        }
        //5EE510 writes A8B2FC even for rows whose display pointer is human
        //or vacant.5E9E70's start reset affects controls, not A8B2DC.
        for raw in inactive_ai_options {
            raw.team = human_team.shell_value();
        }
        candidate.keep_spectator_row()?;
        candidate.keep_players_on_teams()?;
        *self = candidate;
        Ok(removed)
    }

    /// Mode+68 Cooperative5C30C0 commits its count before stage validation;
    /// a missing stage leaves the physical AI rows unchanged. This is distinct
    /// from a user's ordinary AI-row edit, which derives count from rows.
    pub(crate) fn apply_network_ai_preparation(
        &mut self,
        declared_count: Option<i32>,
        opponents: Option<Vec<SkirmishAiSlot>>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only host mode preparation may write AI state");
        let launch = self.editable_network_setup()?;
        if let Some(count) = declared_count {
            launch.ai_player_count = count;
        }
        if let Some(opponents) = opponents {
            // Validated on a copy, committed on success: a refused change leaves the
            // session as it was.
            let mut candidate = launch.clone();
            candidate.opponents = opponents;
            validate_network_rows(&candidate)?;
            candidate.opponents.sort_by_key(|ai| ai.raw_slot_index);
            *launch = candidate;
        }
        Ok(())
    }
}
