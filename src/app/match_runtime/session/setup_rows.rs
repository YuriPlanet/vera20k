//! The network room's row setup: a peer's private choices (WOL `R` arm),
//! host moves, row vacancies and kinds, per-row choices and AI options,
//! country resets, game options, and the host's published setup and roles.

use crate::skirmish_launch::{
    NETWORK_PLAYER_SLOT_COUNT, SkirmishAiSlot, SkirmishLaunchOptions, SkirmishLaunchSession,
    SkirmishLocalSlot,
};

use super::{
    MatchSession, NetworkAiOptions, NetworkPhase, NetworkRosterEntry, NetworkRowKind,
    NetworkSession, NetworkVacancy, PeerChoicePolicy, PeerChoicesChange, SessionMode,
    apply_peer_choices, display_ai_rows, native_network_row_defaults, network_color_in_use,
    row_capacity, validate_network_option_rows,
};

impl MatchSession {
    /// The WOL host's private-options `R` arm (`0x007A8260`, `0x7A83CC..
    /// 0x7A8BD2`) for the participant at `index`. A node without a row is
    /// ignored. An observer asking to play moves to the first Open row
    /// (`0x005E9240`), a player asking to observe moves to the observer row;
    /// with no such row (`0x005E9510` / `0x005E9560`) its own row is vacated
    /// instead (`0x005EC9F0` mode 0) and nothing else is checked. Otherwise
    /// each field is checked against the session: the country by the mode
    /// (`+0x1C`, here `countries` for Cooperative, else any; the current one,
    /// else the mode's `+0x28` Random), a colour or start already held
    /// becomes Random (`0x0069B600`, `0x005E9720`), a team outside 0..3
    /// takes the mode default (`+0x34`/`+0x2C`). In a VERA room with Teams
    /// set, a pick outside its teams is no team instead, and the room seats
    /// the player on its smallest team. No sound, text or reply.
    pub(crate) fn accept_network_peer_choices(
        &mut self,
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
        countries: Option<&[crate::skirmish_launch::LaunchCountry]>,
    ) -> anyhow::Result<PeerChoicesChange> {
        anyhow::ensure!(self.host, "only the host checks a peer's choices");
        let human = self
            .network_setup()
            .and_then(|launch| launch.humans.get(index))
            .ok_or_else(|| anyhow::anyhow!("unknown participant"))?;
        anyhow::ensure!(
            index != self.network_setup().expect("checked").local_player_index,
            "the host's own row is not a peer's"
        );
        let Some(row) = human.raw_slot_index else {
            return Ok(PeerChoicesChange::default());
        };
        let teams = self.network_teams();
        let mut change = self.accept_peer_choices(
            index,
            row,
            choices,
            observer,
            PeerChoicePolicy::Wol { countries, teams },
        )?;
        change.options |= self.keep_players_on_teams()?;
        Ok(change)
    }

    /// VERA's host watching, or playing again from the spectators, as a
    /// guest asking would ([`Self::accept_lan_peer_choices`]), with `choices`
    /// its own.
    pub(crate) fn move_network_host(
        &mut self,
        choices: SkirmishLocalSlot,
        observer: bool,
    ) -> anyhow::Result<PeerChoicesChange> {
        anyhow::ensure!(
            self.host && self.network_several_observers(),
            "only VERA's host moves itself"
        );
        let launch = self
            .network_setup()
            .ok_or_else(|| anyhow::anyhow!("no network setup"))?;
        let index = launch.local_player_index;
        let row = launch.humans[index]
            .raw_slot_index
            .ok_or_else(|| anyhow::anyhow!("the host has no row"))?;
        let mut change =
            self.accept_peer_choices(index, row, choices, observer, PeerChoicePolicy::Lan)?;
        change.options |= self.keep_players_on_teams()?;
        Ok(change)
    }

    /// LAN Type18 receiver5E0D1C..5E0F20: writes country and team directly,
    /// with the same color/start collision checks as WOL. It does not perform
    /// WOL's observer row moves or mode country/team validation; VERA's rooms
    /// move a guest asking to observe (country -3) or to play as WOL does.
    pub(crate) fn accept_lan_peer_choices(
        &mut self,
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
    ) -> anyhow::Result<PeerChoicesChange> {
        anyhow::ensure!(self.host, "only the host checks LAN peer choices");
        let several = self.network_several_observers();
        let launch = self.editable_network_setup()?;
        anyhow::ensure!(
            index != launch.local_player_index,
            "the host's own row is not a peer's"
        );
        let human = launch
            .humans
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("unknown participant"))?;
        let Some(row) = human.raw_slot_index else {
            return Ok(PeerChoicesChange::default());
        };
        let observer = if several {
            observer
        } else {
            human.is_observer()
        };
        let mut change =
            self.accept_peer_choices(index, row, choices, observer, PeerChoicePolicy::Lan)?;
        change.options |= self.keep_players_on_teams()?;
        Ok(change)
    }

    fn accept_peer_choices(
        &mut self,
        index: usize,
        row: u8,
        choices: SkirmishLocalSlot,
        observer: bool,
        policy: PeerChoicePolicy<'_>,
    ) -> anyhow::Result<PeerChoicesChange> {
        let launch = self.network_setup().expect("checked");
        let was_observer = launch.humans[index].is_observer();
        let own_move = index == launch.local_player_index;
        let observer_row = self.network_observer_row();
        let mut change = PeerChoicesChange::default();
        if was_observer && !observer {
            change.roster = true;
            change.options = true;
            if self.network_no_open_player_row() {
                // VERA's rooms keep the spectator watching, in the join
                // queue for the next free seat.
                if self.network_several_observers() {
                    self.queue_network_spectator(index)?;
                } else {
                    self.set_network_row_kind(row, NetworkRowKind::OpenObserver)?;
                }
                return Ok(change);
            }
            // 0x7A8838..0x7A88A4: country Random and a player role, the
            // observer row opened for observers, then 0x5E9240's row. The
            // country check still compares against the -3 read at 0x7A865F,
            // so the options always go out (0x7A8905..0x7A8915). In VERA's
            // rooms the observer's own row is the one it leaves.
            let left = if self.network_several_observers() {
                row
            } else {
                observer_row.unwrap_or(row)
            };
            self.change_network_row_kind(left, NetworkRowKind::OpenObserver, own_move)?;
            let free = self
                .network_free_row(false)
                .ok_or_else(|| anyhow::anyhow!("no Open row after 0x5E9510"))?;
            let human = &mut self.editable_network_setup()?.humans[index];
            human.set_observer(false);
            human.slot.country_random = true;
            human.raw_slot_index = Some(free);
            self.keep_spectator_row()?;
            self.leave_network_queue(index)?;
        } else if was_observer {
            // A spectator asking to watch leaves the join queue.
            change.options |= self.leave_network_queue(index)?;
        } else if observer {
            change.roster = true;
            change.options = true;
            let target = self.network_free_row(true);
            let several = self.network_several_observers();
            // VERA's rooms keep the player seated without an open
            // spectator row.
            if several && target.is_none() {
                return Ok(change);
            }
            self.change_network_row_kind(row, NetworkRowKind::Open, own_move)?;
            if let Some(target) = target {
                // 0x7A89F4..0x7A8A6F: the node goes to [0x8316A4]; in VERA's
                // rooms, to the open observer row 0x5E9240 found.
                let human = &mut self.editable_network_setup()?.humans[index];
                human.set_observer(true);
                human.raw_slot_index = Some(if several {
                    target
                } else {
                    observer_row.unwrap_or(target)
                });
            }
            self.keep_spectator_row()?;
            return Ok(change);
        }
        let applied = self.apply_peer_choice_fields(index, choices, observer, policy)?;
        change.options |= applied.options;
        Ok(change)
    }

    fn apply_peer_choice_fields(
        &mut self,
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
        policy: PeerChoicePolicy<'_>,
    ) -> anyhow::Result<PeerChoicesChange> {
        let listed_colors = self.lobby_color_count;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_ai_options,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        Ok(apply_peer_choices(
            launch,
            inactive_ai_options,
            index,
            choices,
            observer,
            policy,
            listed_colors,
        ))
    }

    pub(crate) fn network_vacancy(&self, row: u8) -> Option<NetworkVacancy> {
        let NetworkPhase::Setup {
            launch,
            vacancies,
            display_ai,
            ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if usize::from(row) >= NETWORK_PLAYER_SLOT_COUNT
            || launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))
            || display_ai[usize::from(row)].is_some()
        {
            return None;
        }
        Some(vacancies[usize::from(row)])
    }

    pub(crate) fn network_display_ai(
        &self,
        row: u8,
    ) -> Option<crate::skirmish_launch::AiDifficulty> {
        let NetworkPhase::Setup { display_ai, .. } = &self.network.as_ref()?.phase else {
            return None;
        };
        display_ai.get(usize::from(row)).copied().flatten()
    }

    ///69B600 shared by peer-choice changes and5E96A0 new-node admission.
    pub(crate) fn network_color_in_use(&self, color: u8) -> bool {
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_ai_options,
                    ..
                },
            ..
        }) = self.network.as_ref()
        else {
            return false;
        };
        network_color_in_use(launch, inactive_ai_options, color, self.lobby_color_count)
    }

    pub(crate) fn network_row_choices(&self, row: u8) -> Option<SkirmishLocalSlot> {
        let NetworkPhase::Setup {
            launch,
            inactive_choices,
            ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if let Some(human) = launch
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row))
        {
            return Some(human.slot.clone());
        }
        if self.network_display_ai(row).is_some()
            && let Some(ai) = launch.opponents.iter().find(|ai| ai.raw_slot_index == row)
        {
            return Some(SkirmishLocalSlot {
                country: ai.country,
                country_random: ai.country_random,
                color_index: ai.color_index,
                color_random: ai.color_random,
                start_position: ai.start_position,
                team: ai.team,
            });
        }
        inactive_choices.get(usize::from(row)).cloned()
    }

    /// Build_Options_String5DBD5C reads the AI arrays even where a human
    /// occupies the same display row. Human start/team never leak into them.
    pub(crate) fn network_ai_options(&self, row: u8) -> Option<NetworkAiOptions> {
        let NetworkPhase::Setup {
            launch,
            inactive_ai_options,
            ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if let Some(ai) = launch.opponents.iter().find(|ai| ai.raw_slot_index == row) {
            return Some(NetworkAiOptions::of(ai));
        }
        inactive_ai_options.get(usize::from(row)).copied()
    }

    ///5E5BAB..5E5C3C retains every received AI word. Active rows have already
    /// entered HostSetup's opponents; only the inactive words need retention.
    pub(crate) fn accept_network_inactive_ai_options<const N: usize>(
        &mut self,
        incoming: [NetworkAiOptions; N],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.host, "host cannot receive its own AI arrays");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_ai_options,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(N <= NETWORK_PLAYER_SLOT_COUNT, "too many AI option rows");
        for (row, value) in incoming.into_iter().enumerate() {
            if !launch
                .opponents
                .iter()
                .any(|ai| usize::from(ai.raw_slot_index) == row)
            {
                inactive_ai_options[row] = value;
            }
        }
        Ok(())
    }

    /// LAN/WOL roster receiver5E8EA0 replaces the eight display pointers,
    /// retaining source-list order, node acknowledgements and endpoints.
    /// Unknown names enter as Waiting; a later type3 admission binds its row.
    pub(crate) fn accept_network_roster<const N: usize>(
        &mut self,
        rows: [NetworkRosterEntry; N],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.host, "a host cannot receive its own roster");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    vacancies,
                    display_ai,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(N <= NETWORK_PLAYER_SLOT_COUNT, "too many roster rows");
        // Validated on a copy, committed on success: a refused change leaves the
        // session as it was.
        let mut candidate = launch.clone();
        for human in &mut candidate.humans {
            human.raw_slot_index = None;
        }
        let mut next_vacancies = [NetworkVacancy::Open; NETWORK_PLAYER_SLOT_COUNT];
        let mut next_ai = [None; NETWORK_PLAYER_SLOT_COUNT];
        for (row, binding) in rows.into_iter().enumerate() {
            match binding {
                NetworkRosterEntry::Human(source) => {
                    let human = candidate
                        .humans
                        .get_mut(source)
                        .ok_or_else(|| anyhow::anyhow!("roster names an unadmitted source"))?;
                    anyhow::ensure!(
                        human.raw_slot_index.is_none(),
                        "duplicate roster participant"
                    );
                    human.raw_slot_index = Some(row as u8);
                }
                NetworkRosterEntry::Vacancy(vacancy) => next_vacancies[row] = vacancy,
                NetworkRosterEntry::Ai(difficulty) => next_ai[row] = Some(difficulty),
            }
        }
        validate_network_option_rows(&candidate)?;
        *launch = candidate;
        *vacancies = next_vacancies;
        *display_ai = next_ai;
        Ok(())
    }

    /// Sole5EC9F0 row-kind transition: any AI selection resets its choices,
    /// leavingAI/human/observer resets vacant choices, and every transition
    /// reapplies the mode's team default via5EE510. Returned peers need the
    /// caller's already-authorized service kick; no transport lives here.
    pub(crate) fn set_network_row_kind(
        &mut self,
        row: u8,
        kind: NetworkRowKind,
    ) -> anyhow::Result<Option<crate::skirmish_launch::SkirmishHumanSlot>> {
        self.change_network_row_kind(row, kind, false)
    }

    /// [`Self::set_network_row_kind`]; `own_move` lets VERA's host change
    /// its own row as it moves itself between playing and watching
    /// ([`Self::move_network_host`]).
    fn change_network_row_kind(
        &mut self,
        row: u8,
        kind: NetworkRowKind,
        own_move: bool,
    ) -> anyhow::Result<Option<crate::skirmish_launch::SkirmishHumanSlot>> {
        anyhow::ensure!(self.host, "only the host may change participant row kinds");
        //5EC9F0 returns before any reset when the selected item pointer is unchanged.
        let current_kind = self
            .network_display_ai(row)
            .map(NetworkRowKind::Ai)
            .or_else(|| {
                self.network_vacancy(row).and_then(|vacancy| match vacancy {
                    NetworkVacancy::Open => Some(NetworkRowKind::Open),
                    NetworkVacancy::Closed => Some(NetworkRowKind::Closed),
                    NetworkVacancy::OpenObserver => Some(NetworkRowKind::OpenObserver),
                    NetworkVacancy::Waiting => None,
                })
            });
        if current_kind == Some(kind) {
            return Ok(None);
        }
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    vacancies,
                    inactive_choices,
                    inactive_ai_options,
                    display_ai,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            usize::from(row) < NETWORK_PLAYER_SLOT_COUNT
                && (own_move || launch.local_player().raw_slot_index != Some(row)),
            "row change cannot replace the local participant"
        );
        let removed = if let Some(index) = launch
            .humans
            .iter()
            .position(|human| human.raw_slot_index == Some(row))
        {
            let human = launch.humans[index].clone();
            launch.humans[index].raw_slot_index = None;
            Some(human)
        } else {
            None
        };
        let had_ai = launch.opponents.iter().any(|ai| ai.raw_slot_index == row);
        launch.opponents.retain(|ai| ai.raw_slot_index != row);
        let index = usize::from(row);
        let was_observer = removed.as_ref().is_some_and(|human| human.is_observer())
            || vacancies[index] == NetworkVacancy::OpenObserver;
        display_ai[index] = match kind {
            NetworkRowKind::Ai(difficulty) => Some(difficulty),
            _ => None,
        };
        if let NetworkRowKind::Ai(difficulty) = kind {
            let defaults = native_network_row_defaults(&launch.mode, true);
            launch.opponents.push(SkirmishAiSlot {
                raw_slot_index: row,
                country: defaults.country,
                country_random: defaults.country_random,
                color_index: defaults.color_index,
                color_random: defaults.color_random,
                start_position: defaults.start_position,
                team: defaults.team,
                difficulty,
            });
            launch.opponents.sort_by_key(|ai| ai.raw_slot_index);
        } else {
            //5ECFD7 clears three fields when leaving an observer;5ED0CF
            //and5ED17D clear five when entering one or removing an AI.
            //Open <-> Closed alone retains these separate array words.
            if had_ai || (!was_observer && kind == NetworkRowKind::OpenObserver) {
                inactive_ai_options[index] = NetworkAiOptions::CLEARED;
            } else if was_observer && kind != NetworkRowKind::OpenObserver {
                inactive_ai_options[index].difficulty = -1;
                inactive_ai_options[index].country = -1;
                inactive_ai_options[index].color = -1;
            }
            if had_ai || removed.is_some() || vacancies[index] == NetworkVacancy::OpenObserver {
                inactive_choices[index] = native_network_row_defaults(&launch.mode, false);
            }
            vacancies[index] = match kind {
                NetworkRowKind::Open => NetworkVacancy::Open,
                NetworkRowKind::Closed => NetworkVacancy::Closed,
                NetworkRowKind::OpenObserver => NetworkVacancy::OpenObserver,
                NetworkRowKind::Ai(_) => unreachable!(),
            };
            if kind == NetworkRowKind::OpenObserver {
                inactive_choices[index] = native_network_row_defaults(&launch.mode, false);
                inactive_choices[index].country_random = false;
                inactive_choices[index].color_index = 8;
            }
            inactive_choices[index].team = native_network_row_defaults(&launch.mode, false).team;
            //5ED1B0 ->5EE510 runs after the clear above, including for Open
            //and Closed rows: the raw team array takes mode+2C last.
            inactive_ai_options[index].team = inactive_choices[index].team.shell_value();
        }
        launch.ai_player_count = launch.opponents.len() as i32;
        Ok(removed)
    }

    /// Cooperative's country refresh (`0x005C2500`), on the host and a
    /// guest alike: every row below the setup's capacity (`[0xA8BB0C]`)
    /// takes `country` (None is Random) in its combo and its node
    /// (`NodeNameTag__SetCountry`), whoever owns the row. Without `ai_rows`
    /// the AI rows keep theirs.
    pub(crate) fn reset_network_countries(
        &mut self,
        country: Option<crate::skirmish_launch::LaunchCountry>,
        ai_rows: bool,
    ) -> anyhow::Result<()> {
        let rows = self
            .network_setup_capacity()
            .unwrap_or(0)
            .clamp(0, NETWORK_PLAYER_SLOT_COUNT as i32) as u8;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        let set = |current: crate::skirmish_launch::LaunchCountry| match country {
            Some(country) => (country, false),
            None => (current, true),
        };
        for human in &mut launch.humans {
            if human.raw_slot_index.is_some_and(|row| row < rows) {
                (human.slot.country, human.slot.country_random) = set(human.slot.country);
            }
        }
        for ai in launch.opponents.iter_mut().filter(|_| ai_rows) {
            if ai.raw_slot_index < rows {
                (ai.country, ai.country_random) = set(ai.country);
            }
        }
        for choices in inactive_choices.iter_mut().take(usize::from(rows)) {
            (choices.country, choices.country_random) = set(choices.country);
        }
        Ok(())
    }

    /// VERA's room bans on the host: each player, AI and inactive row whose
    /// chosen country is `banned` goes back to Random.
    pub(crate) fn random_for_network_countries(
        &mut self,
        banned: impl Fn(crate::skirmish_launch::LaunchCountry) -> bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host enforces the room's bans");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        for human in launch
            .humans
            .iter_mut()
            .filter(|human| !human.is_observer())
        {
            human.slot.country_random |= banned(human.slot.country);
        }
        for ai in &mut launch.opponents {
            ai.country_random |= banned(ai.country);
        }
        for choices in inactive_choices.iter_mut() {
            choices.country_random |= banned(choices.country);
        }
        Ok(())
    }

    pub(crate) fn set_network_row_choices(
        &mut self,
        row: u8,
        choices: SkirmishLocalSlot,
    ) -> anyhow::Result<()> {
        let host = self.host;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        if let Some(index) = launch
            .humans
            .iter()
            .position(|human| human.raw_slot_index == Some(row))
        {
            anyhow::ensure!(
                index == launch.local_player_index,
                "a client may edit only its own human row"
            );
            launch.humans[index].slot = choices;
        } else {
            anyhow::ensure!(
                host && usize::from(row) < NETWORK_PLAYER_SLOT_COUNT,
                "only the host may edit AI/vacant choices"
            );
            if let Some(ai) = launch
                .opponents
                .iter_mut()
                .find(|ai| ai.raw_slot_index == row)
            {
                ai.country = choices.country;
                ai.country_random = choices.country_random;
                ai.color_index = choices.color_index;
                ai.color_random = choices.color_random;
                ai.start_position = choices.start_position;
                ai.team = choices.team;
            } else {
                inactive_choices[usize::from(row)] = choices;
            }
        }
        Ok(())
    }

    pub(crate) fn set_network_vacancy(
        &mut self,
        row: u8,
        vacancy: NetworkVacancy,
    ) -> anyhow::Result<()> {
        let kind = match vacancy {
            NetworkVacancy::Open => NetworkRowKind::Open,
            NetworkVacancy::Closed => NetworkRowKind::Closed,
            NetworkVacancy::OpenObserver => NetworkRowKind::OpenObserver,
            NetworkVacancy::Waiting => anyhow::bail!("Waiting is admitted by a received roster"),
        };
        anyhow::ensure!(
            self.network_setup().is_none_or(|launch| !launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))),
            "vacancy cannot replace a human participant"
        );
        self.set_network_row_kind(row, kind).map(|_| ())
    }

    pub(crate) fn set_network_options(
        &mut self,
        options: SkirmishLaunchOptions,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host may change game options");
        let mut options = options;
        // A Teams count this build does not offer leaves Teams Off.
        options.teams = options
            .teams
            .filter(|teams| crate::skirmish_launch::ROOM_TEAMS.contains(teams));
        self.editable_network_setup()?.options = options;
        // Native host publishes changes; a guest later invalidates its own
        // readiness and sends A0. Do not optimistically clear every peer here.
        Ok(())
    }
}

impl MatchSession {
    /// The admitted host's options packet (ParsePacket5E3D10) replaces setup
    /// values, never roster identity or its independent ready/map acknowledgements.
    /// The service adapter applies the packet's local-ready invalidation separately.
    /// A WOL Quick Match host applies the matchbot's options the same way
    /// (`matchbot`, `0x007A6DE2..0x007A6DF3`). In WOL the options also bind
    /// the rows, as the roster `'L'` (`0x5E8EA0`) does, so they replace the
    /// AI display rows too; a LAN guest takes those from its roster packet.
    pub(crate) fn accept_network_host_setup<const N: usize>(
        &mut self,
        mut incoming: SkirmishLaunchSession,
        incoming_vacancies: [NetworkVacancy; N],
        capacity: i32,
        observer_row: Option<u8>,
        matchbot: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host || matchbot,
            "a host cannot receive guest-authored host setup"
        );
        anyhow::ensure!(capacity > 0, "host map capacity must be positive");
        anyhow::ensure!(N <= NETWORK_PLAYER_SLOT_COUNT, "too many vacancy rows");
        anyhow::ensure!(
            observer_row.is_none_or(|row| usize::from(row) < NETWORK_PLAYER_SLOT_COUNT),
            "observer reservation is outside setup rows"
        );
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    vacancies,
                    display_ai,
                    capacity: retained_capacity,
                    observer_row: retained_observer_row,
                    ..
                },
            return_context,
            room,
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        let admitted_capacity = row_capacity(
            return_context.session_mode(),
            *room,
            &incoming.mode,
            capacity,
        );
        anyhow::ensure!(
            launch.local_player_index == incoming.local_player_index
                && launch.humans.len() == incoming.humans.len()
                && launch
                    .humans
                    .iter()
                    .zip(&incoming.humans)
                    .all(|(old, new)| old.name == new.name),
            "host options cannot replace admitted participant identity or source order"
        );
        validate_network_option_rows(&incoming)?;
        // SpeedNormalize (`0x005FB2E0`) indexes its table's eight columns
        // with the stored speed; the setup offers 0..=6 and VERA's 8.
        anyhow::ensure!(
            u8::try_from(incoming.options.game_speed)
                .is_ok_and(crate::sim::game_options::is_selectable_game_speed),
            "host game speed {} is not a selectable speed",
            incoming.options.game_speed
        );
        incoming.opponents.sort_by_key(|ai| ai.raw_slot_index);
        incoming.use_live_house_rows();
        // A Teams count this build does not offer leaves Teams Off.
        incoming.options.teams = incoming
            .options
            .teams
            .filter(|teams| crate::skirmish_launch::ROOM_TEAMS.contains(teams));
        // The join queue names spectators, each once.
        let queue = std::mem::take(&mut incoming.options.queue);
        for name in queue {
            let waits = incoming.humans.iter().any(|human| {
                human.name == name && human.is_observer() && human.raw_slot_index.is_some()
            });
            if waits && !incoming.options.queue.contains(&name) {
                incoming.options.queue.push(name);
            }
        }
        *launch = incoming;
        *vacancies = [NetworkVacancy::Open; NETWORK_PLAYER_SLOT_COUNT];
        vacancies[..N].copy_from_slice(&incoming_vacancies);
        if return_context.session_mode() == SessionMode::Wol {
            // The host sends each AI row it shows as Closed; Cooperative's
            // start AI (`0x5C30C0`) fills AI arrays only, behind Open rows.
            *display_ai = display_ai_rows(launch);
            for (ai, vacancy) in display_ai.iter_mut().zip(vacancies.iter()) {
                if *vacancy != NetworkVacancy::Closed {
                    *ai = None;
                }
            }
        }
        *retained_capacity = admitted_capacity;
        *retained_observer_row = observer_row;
        Ok(())
    }

    /// Mode callbacks write node+6B independently of the team selection at+63.
    /// FFA uses source indices; Siege uses defender/attacker groups1/2.
    pub(crate) fn accept_network_human_role(
        &mut self,
        index: usize,
        role: crate::skirmish_launch::LaunchHumanRole,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            role != crate::skirmish_launch::LaunchHumanRole::Player { group: -1 },
            "native role -1 must be represented as Observer"
        );
        let launch = self.editable_network_setup()?;
        launch
            .humans
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("unknown participant"))?
            .role = role;
        Ok(())
    }
}
