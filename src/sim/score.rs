//! Authoritative terminal score snapshot construction.
//!
//! The app owns localization, row colours, elapsed wall time, and display
//! sorting. The simulation owns the raw house statistics and the Scenario RNG
//! draws used by the existing Rust victory-bonus projection.
//!
//! The score follows `ScoreDialog__FillEntries` (`0x005C98A0`) as read from its
//! instructions (not executed): only a positive `house+0x54E8` enters the entry
//! (`0x005C99EB`), and a surviving house then gets `v + v/2 + Random(v/2, v)`,
//! one Scenario RNG draw at `0x005C9A21` unless `v` is 0 (`0x0065C7E0` draws
//! nothing when min equals max). The traversal walks `Houses[]` and skips a
//! null House, a `MultiplayPassive` type and the observer House
//! (`[0x00AC1198]`, `0x005C98CD..0x005C98EB`); the sim's House
//! registration order is that array's order. Presentation never advances the
//! gameplay Scenario stream directly.

use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// One sim-owned row before localization, colour selection, or display sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalScoreRowSnapshot {
    pub owner: InternedId,
    pub country: Option<InternedId>,
    pub survived: bool,
    pub kills: u32,
    pub losses: u32,
    pub built: u32,
    pub raw_score: i32,
    pub score: i32,
}

/// The one-shot raw score result for a finished match.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalScoreSnapshot {
    pub rows: Vec<TerminalScoreRowSnapshot>,
}

impl Simulation {
    /// Read the already-finalized raw terminal score rows without mutation.
    pub fn terminal_score_snapshot(&self) -> Option<&TerminalScoreSnapshot> {
        self.terminal_score_snapshot.as_ref()
    }

    /// Latch the snapshot at the natural win/loss terminal edge.
    ///
    /// Only the master-frame orchestrator may call this mutating half of the
    /// boundary. The app receives the immutable snapshot through
    /// [`Simulation::terminal_score_snapshot`]. The first call consumes
    /// Scenario RNG once for each surviving positive-score contender; later
    /// calls are no-ops so the cursor cannot advance twice.
    pub(super) fn finalize_terminal_score_snapshot(&mut self) -> bool {
        if self.terminal_score_snapshot.is_some() {
            return false;
        }

        // Houses[] order, without passive types or the observer House.
        let observer = self.session.observer_house();
        let contenders: Vec<InternedId> = self
            .houses
            .in_order()
            .filter(|house| !house.multiplay_passive() && Some(house.name()) != observer)
            .map(|house| house.name())
            .collect();
        let mut rows = Vec::with_capacity(contenders.len());
        for owner in contenders {
            let Some((country, survived, stats, raw_score)) =
                self.houses.get(&owner).map(|house| {
                    (
                        house.country(),
                        !house.is_defeated(),
                        house.stats(),
                        house.economy().score(),
                    )
                })
            else {
                continue;
            };
            // 0x005C99EB..0x005C99F5 adds only a positive score to the zeroed
            // entry; a survivor then gets v + v/2 + Random(v/2, v), which
            // draws nothing when v is 0 (0x0065C7E0 with min == max). The
            // sums wrap (0x005C9A26..0x005C9A28).
            let base = raw_score.max(0);
            let score = if survived && base > 0 {
                let half = base / 2;
                let bonus = self
                    .scenario_rng
                    .next_range_u32_inclusive(half as u32, base as u32);
                base.wrapping_add(half).wrapping_add(bonus as i32)
            } else {
                base
            };
            rows.push(TerminalScoreRowSnapshot {
                owner,
                country,
                survived,
                kills: stats.kills(),
                losses: stats.losses(),
                built: stats.built(),
                raw_score,
                score,
            });
        }

        let snapshot = TerminalScoreSnapshot { rows };
        self.terminal_score_snapshot = Some(snapshot);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::house_state::{HouseOutcomeKind, HouseOutcomeState, HouseState};

    struct HouseFixture<'a> {
        name: &'a str,
        defeated: bool,
        passive: bool,
        score: i32,
        units_killed: u32,
        buildings_killed: u32,
        units_lost: u32,
        buildings_lost: u32,
        built: u32,
    }

    fn insert_house(sim: &mut Simulation, fixture: HouseFixture<'_>) -> InternedId {
        let owner = sim.interner.intern(fixture.name);
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.set_is_defeated_for_test(fixture.defeated);
        house.set_multiplay_passive_for_test(fixture.passive);
        house.economy_mut().set_score_for_test(fixture.score);
        house.set_stats_for_test(
            crate::sim::house_state::MatchStatistics::from_totals_for_test(
                fixture.units_killed,
                fixture.buildings_killed,
                fixture.units_lost,
                fixture.buildings_lost,
                fixture.built,
            ),
        );
        sim.houses.register(house);
        owner
    }

    #[test]
    fn terminal_score_draws_in_house_order_and_skips_ineligible_rows() {
        let mut sim = Simulation::with_seed(0x51C0_1234);
        let alpha = insert_house(
            &mut sim,
            HouseFixture {
                name: "Alpha",
                defeated: false,
                passive: false,
                score: 100,
                units_killed: 2,
                buildings_killed: 3,
                units_lost: 4,
                buildings_lost: 5,
                built: 6,
            },
        );
        let defeated = insert_house(
            &mut sim,
            HouseFixture {
                name: "Defeated",
                defeated: true,
                passive: false,
                score: 200,
                units_killed: 0,
                buildings_killed: 0,
                units_lost: 0,
                buildings_lost: 0,
                built: 0,
            },
        );
        let zero = insert_house(
            &mut sim,
            HouseFixture {
                name: "Zero",
                defeated: false,
                passive: false,
                score: 0,
                units_killed: 0,
                buildings_killed: 0,
                units_lost: 0,
                buildings_lost: 0,
                built: 0,
            },
        );
        let passive = insert_house(
            &mut sim,
            HouseFixture {
                name: "Passive",
                defeated: false,
                passive: true,
                score: 300,
                units_killed: 0,
                buildings_killed: 0,
                units_lost: 0,
                buildings_lost: 0,
                built: 0,
            },
        );
        let omega = insert_house(
            &mut sim,
            HouseFixture {
                name: "Omega",
                defeated: false,
                passive: false,
                score: 50,
                units_killed: 1,
                buildings_killed: 0,
                units_lost: 2,
                buildings_lost: 0,
                built: 3,
            },
        );

        sim.houses
            .reorder_for_test(&[omega, passive, alpha, defeated, zero]);
        let mut expected_rng = sim.clone_scenario_rng();
        let omega_score = 50 + 25 + expected_rng.next_range_u32_inclusive(25, 50) as i32;
        let alpha_score = 100 + 50 + expected_rng.next_range_u32_inclusive(50, 100) as i32;
        let before_hash = sim.state_hash();

        assert!(sim.finalize_terminal_score_snapshot());
        let snapshot = sim
            .terminal_score_snapshot()
            .expect("terminal score snapshot")
            .clone();

        assert_eq!(
            snapshot
                .rows
                .iter()
                .map(|row| row.owner)
                .collect::<Vec<_>>(),
            vec![omega, alpha, defeated, zero]
        );
        assert_eq!(snapshot.rows[0].raw_score, 50);
        assert_eq!(snapshot.rows[0].score, omega_score);
        assert_eq!(snapshot.rows[0].kills, 1);
        assert_eq!(snapshot.rows[0].losses, 2);
        assert_eq!(snapshot.rows[0].built, 3);
        assert!(snapshot.rows[0].survived);
        assert_eq!(snapshot.rows[1].raw_score, 100);
        assert_eq!(snapshot.rows[1].score, alpha_score);
        assert_eq!(snapshot.rows[1].kills, 5);
        assert_eq!(snapshot.rows[1].losses, 9);
        assert_eq!(snapshot.rows[1].built, 6);
        assert!(snapshot.rows[1].survived);
        assert_eq!(snapshot.rows[2].score, 200);
        assert!(!snapshot.rows[2].survived);
        assert_eq!(snapshot.rows[3].score, 0);
        assert_eq!(sim.clone_scenario_rng().state(), expected_rng.state());
        assert_ne!(
            sim.state_hash(),
            before_hash,
            "score RNG draws are hash-visible"
        );
    }

    #[test]
    fn a_negative_score_shows_zero_and_draws_nothing() {
        // 0x005C99EB..0x005C99F5 adds only a positive score to the entry, so
        // a survivor's bonus range is Random(0, 0), which draws nothing.
        let mut sim = Simulation::with_seed(0x51C0_0FF5);
        let mut owners = Vec::new();
        for (name, defeated) in [("Survivor", false), ("Loser", true)] {
            owners.push(insert_house(
                &mut sim,
                HouseFixture {
                    name,
                    defeated,
                    passive: false,
                    score: -40,
                    units_killed: 0,
                    buildings_killed: 0,
                    units_lost: 0,
                    buildings_lost: 0,
                    built: 0,
                },
            ));
        }
        sim.houses.reorder_for_test(&owners);
        let cursor = sim.clone_scenario_rng().state();

        assert!(sim.finalize_terminal_score_snapshot());
        let snapshot = sim.terminal_score_snapshot().expect("snapshot");
        for row in &snapshot.rows {
            assert_eq!(row.raw_score, -40);
            assert_eq!(row.score, 0);
        }
        assert_eq!(sim.clone_scenario_rng().state(), cursor);
    }

    /// 0x005C98E5: the observer House takes no row and draws nothing.
    #[test]
    fn the_observer_house_takes_no_score_row() {
        let mut sim = Simulation::with_seed(0x51C0_0B5E);
        let mut owners = Vec::new();
        for name in ["Player", "Observer"] {
            owners.push(insert_house(
                &mut sim,
                HouseFixture {
                    name,
                    defeated: false,
                    passive: false,
                    score: 50,
                    units_killed: 0,
                    buildings_killed: 0,
                    units_lost: 0,
                    buildings_lost: 0,
                    built: 0,
                },
            ));
        }
        sim.houses.reorder_for_test(&owners);
        sim.session.install_multiplayer_house_identity(
            crate::skirmish_modes::MpModeClass::Battle,
            Some(owners[1]),
        );
        let mut one_draw = sim.clone_scenario_rng();
        one_draw.next_range_u32_inclusive(25, 50);

        assert!(sim.finalize_terminal_score_snapshot());
        let snapshot = sim.terminal_score_snapshot().expect("snapshot");
        assert_eq!(
            snapshot
                .rows
                .iter()
                .map(|row| row.owner)
                .collect::<Vec<_>>(),
            [owners[0]]
        );
        assert_eq!(sim.clone_scenario_rng().state(), one_draw.state());
    }

    fn natural_terminal_sim(seed: u64) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let human = sim.interner.intern("Human");
        let mut human_house = HouseState::new(human, 0, None, true, 0, 10);
        human_house.set_is_defeated_for_test(true);
        human_house.economy_mut().set_score_for_test(200);
        human_house.supply_outcome_state_for_test(
            false,
            true,
            Some(HouseOutcomeState {
                kind: HouseOutcomeKind::Defeat,
                exit_ready: true,
            }),
            crate::sim::timer::CdTimer::default(),
        );
        sim.houses.register(human_house);
        sim.session.set_current_house_for_test(Some(human));

        let opponent = sim.interner.intern("Opponent");
        let mut opponent_house = HouseState::new(opponent, 1, None, false, 0, 10);
        opponent_house.economy_mut().set_score_for_test(100);
        sim.houses.register(opponent_house);
        sim
    }

    #[test]
    fn natural_terminal_frame_finalizes_score_before_returned_hash_once() {
        let mut sim = natural_terminal_sim(0x51C0_9ABC);
        // At normal play's cadence the ending frame hashes off the interval.
        // It does not commit, so its tick stays the last committed one.
        sim.hash_state_every(crate::sim::replay::PLAY_HASH_INTERVAL);
        sim.session.set_tick_for_test(7);

        let first = sim.advance_tick(&[], None, None, None, 67);

        assert_eq!(first.tick, 7);
        assert!(!first.frame_committed);
        assert!(first.terminal_score_finalized);
        assert_eq!(first.state_hash, Some(sim.state_hash()));
        let first_snapshot = sim
            .terminal_score_snapshot()
            .expect("natural terminal frame latches score")
            .clone();
        let cursor_after_first = sim.clone_scenario_rng().state();

        let second = sim.advance_tick(&[], None, None, None, 67);
        assert!(!second.frame_committed);
        assert!(!second.terminal_score_finalized);
        assert_eq!(second.state_hash, first.state_hash);
        assert_eq!(sim.clone_scenario_rng().state(), cursor_after_first);
        assert_eq!(
            sim.terminal_score_snapshot(),
            Some(&first_snapshot),
            "later terminal pumps expose the same immutable snapshot"
        );
    }

    #[test]
    fn campaign_terminal_does_not_finalize_skirmish_bonus_or_draw_rng() {
        let mut sim = natural_terminal_sim(0x51C0_CAFE);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        let cursor = sim.clone_scenario_rng().state();

        let tick = sim.advance_tick(&[], None, None, None, 67);

        assert!(!tick.terminal_score_finalized);
        assert!(sim.terminal_score_snapshot().is_none());
        assert_eq!(sim.clone_scenario_rng().state(), cursor);
    }

    #[test]
    fn another_humans_ready_result_does_not_finalize_the_current_players_score() {
        let mut sim = natural_terminal_sim(0x51C0_9ABD);
        sim.session
            .set_current_house_for_test(sim.interner.get("Opponent"));
        let cursor = sim.clone_scenario_rng().state();
        let tick = sim.advance_tick(&[], None, None, None, 67);
        assert!(tick.frame_committed);
        assert!(!tick.terminal_score_finalized);
        assert!(sim.terminal_score_snapshot().is_none());
        assert_eq!(sim.clone_scenario_rng().state(), cursor);
    }

    #[test]
    fn quit_and_connection_alone_do_not_score_but_ready_outcome_wins_exit_race() {
        let mut quit_only = Simulation::with_seed(0x51C0_B001);
        quit_only.set_quit_requested_for_test(true);
        let quit_cursor = quit_only.clone_scenario_rng().state();
        let quit_tick = quit_only.advance_tick(&[], None, None, None, 67);
        assert!(!quit_tick.frame_committed);
        assert!(!quit_tick.terminal_score_finalized);
        assert!(quit_only.terminal_score_snapshot().is_none());
        assert_eq!(quit_only.clone_scenario_rng().state(), quit_cursor);

        let mut connection_only = Simulation::with_seed(0x51C0_B002);
        connection_only.set_connection_lost_for_test(true);
        let connection_cursor = connection_only.clone_scenario_rng().state();
        let connection_tick = connection_only.advance_tick(&[], None, None, None, 67);
        assert!(!connection_tick.frame_committed);
        assert!(!connection_tick.terminal_score_finalized);
        assert!(connection_only.terminal_score_snapshot().is_none());
        assert_eq!(
            connection_only.clone_scenario_rng().state(),
            connection_cursor
        );

        let mut raced = natural_terminal_sim(0x51C0_B003);
        let human = raced.interner.get("Human").expect("human owner");
        let exit = crate::sim::command::CommandEnvelope::new(
            human,
            1,
            crate::sim::command::Command::ExitMatch,
        );
        let raced_tick = raced.advance_tick(&[exit], None, None, None, 67);
        assert!(!raced_tick.frame_committed);
        assert_eq!(raced_tick.executed_commands, 1);
        assert!(raced_tick.terminal_score_finalized);
        assert!(raced.terminal_score_snapshot().is_some());
    }

    #[test]
    fn terminal_score_snapshot_roundtrips_in_current_version() {
        let mut sim = natural_terminal_sim(0x51C0_DEF0);
        let tick = sim.advance_tick(&[], None, None, None, 67);
        assert!(tick.terminal_score_finalized);
        let expected = sim
            .terminal_score_snapshot()
            .expect("terminal score before save")
            .clone();

        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 2, "score.map", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("current score snapshot")
            .sim;
        assert_eq!(restored.terminal_score_snapshot(), Some(&expected));
    }

    #[test]
    fn replay_reproduces_empty_command_terminal_score_edge() {
        use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};

        let seed = 0x51C0_AA55;
        let mut recorded = natural_terminal_sim(seed);
        let tick = recorded.advance_tick(&[], None, None, None, 67);
        assert!(tick.terminal_score_finalized);
        let mut replay = ReplayLog::new(ReplayHeader {
            pixel_conversion_bounds: Default::default(),
            version: 1,
            tick_hz: 15,
            seed,
            map_name: "terminal-score.map".to_string(),
            rules_hash: 0,
        });
        replay.record_tick(tick.tick, Vec::new(), tick.state_hash);

        let mut playback = natural_terminal_sim(seed);
        let hashes = ReplayRunner::run_fixture(&mut playback, &replay, None, None, 67);

        assert_eq!(hashes, vec![tick.state_hash]);
        assert_eq!(
            playback.terminal_score_snapshot(),
            recorded.terminal_score_snapshot()
        );
    }

    fn hash_with_score_row(mutate: impl FnOnce(&mut TerminalScoreRowSnapshot, InternedId)) -> u64 {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Alpha");
        let alternate = sim.interner.intern("Beta");
        let country = sim.interner.intern("America");
        let mut row = TerminalScoreRowSnapshot {
            owner,
            country: Some(country),
            survived: true,
            kills: 1,
            losses: 2,
            built: 3,
            raw_score: 100,
            score: 225,
        };
        mutate(&mut row, alternate);
        sim.terminal_score_snapshot = Some(TerminalScoreSnapshot { rows: vec![row] });
        sim.state_hash()
    }

    #[test]
    fn every_terminal_score_row_field_is_hash_authoritative() {
        let absent = Simulation::new().state_hash();
        let mut present_empty = Simulation::new();
        present_empty.terminal_score_snapshot = Some(TerminalScoreSnapshot::default());
        assert_ne!(absent, present_empty.state_hash());

        let base = hash_with_score_row(|_, _| {});
        assert_ne!(base, hash_with_score_row(|row, other| row.owner = other));
        assert_ne!(base, hash_with_score_row(|row, _| row.country = None));
        assert_ne!(base, hash_with_score_row(|row, _| row.survived = false));
        assert_ne!(base, hash_with_score_row(|row, _| row.kills += 1));
        assert_ne!(base, hash_with_score_row(|row, _| row.losses += 1));
        assert_ne!(base, hash_with_score_row(|row, _| row.built += 1));
        assert_ne!(base, hash_with_score_row(|row, _| row.raw_score += 1));
        assert_ne!(base, hash_with_score_row(|row, _| row.score += 1));
    }
}
