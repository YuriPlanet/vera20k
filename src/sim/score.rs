//! Authoritative terminal score snapshot construction.
//!
//! The app owns localization, row colours, elapsed wall time, and display
//! sorting. The simulation owns the raw house statistics and the Scenario RNG
//! draws used by the existing Rust victory-bonus projection.
//!
//! The exact native victory-bonus formula and score-dialog traversal remain
//! UNCHECKED. This module preserves the prior Rust formula/admission rules, uses
//! the sim's canonical house registration order, and prevents presentation from
//! directly advancing the gameplay Scenario stream.

use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// One sim-owned row before localization, colour selection, or display sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TerminalScoreRowSnapshot {
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
pub(crate) struct TerminalScoreSnapshot {
    pub rows: Vec<TerminalScoreRowSnapshot>,
}

impl Simulation {
    /// Read the already-finalized raw terminal score rows without mutation.
    pub(crate) fn terminal_score_snapshot(&self) -> Option<&TerminalScoreSnapshot> {
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

        // The sim's canonical HouseClass registration order owns deterministic
        // house traversal; the keyed house store is only a lookup table. Append
        // unregistered fixture houses deterministically so standalone tests and
        // malformed inputs retain a bounded compatibility path. Whether the
        // native score dialog uses this exact traversal remains UNCHECKED.
        let mut contenders = Vec::new();
        for owner in self
            .session
            .house_order
            .iter()
            .copied()
            .chain(self.houses.keys().copied())
        {
            if self
                .houses
                .get(&owner)
                .is_some_and(|house| !house.multiplay_passive)
                && !contenders.contains(&owner)
            {
                contenders.push(owner);
            }
        }
        let mut rows = Vec::with_capacity(contenders.len());
        for owner in contenders {
            let Some((country, survived, stats, harvested_credits)) =
                self.houses.get(&owner).map(|house| {
                    (
                        house.country,
                        !house.is_defeated,
                        house.stats,
                        house.economy.harvested_credits,
                    )
                })
            else {
                continue;
            };
            let raw_score = stats.score(harvested_credits);
            let score = if survived && raw_score > 0 {
                let half = raw_score / 2;
                let bonus = self
                    .scenario_rng
                    .next_range_u32_inclusive(half.max(0) as u32, raw_score.max(0) as u32);
                raw_score.saturating_add(half).saturating_add(bonus as i32)
            } else {
                raw_score
            };
            rows.push(TerminalScoreRowSnapshot {
                owner,
                country,
                survived,
                kills: stats.kills(),
                losses: stats.losses(),
                built: stats.built,
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
        harvested: i32,
        kill_score: i32,
        units_killed: u32,
        buildings_killed: u32,
        units_lost: u32,
        buildings_lost: u32,
        built: u32,
    }

    fn insert_house(sim: &mut Simulation, fixture: HouseFixture<'_>) -> InternedId {
        let owner = sim.interner.intern(fixture.name);
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.is_defeated = fixture.defeated;
        house.multiplay_passive = fixture.passive;
        house.economy.harvested_credits = fixture.harvested;
        house.stats.score_points = fixture.kill_score;
        house.stats.units_killed = fixture.units_killed;
        house.stats.buildings_killed = fixture.buildings_killed;
        house.stats.units_lost = fixture.units_lost;
        house.stats.buildings_lost = fixture.buildings_lost;
        house.stats.built = fixture.built;
        sim.houses.insert(owner, house);
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
                harvested: 80,
                kill_score: 20,
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
                harvested: 200,
                kill_score: 0,
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
                harvested: 0,
                kill_score: 0,
                units_killed: 0,
                buildings_killed: 0,
                units_lost: 0,
                buildings_lost: 0,
                built: 0,
            },
        );
        insert_house(
            &mut sim,
            HouseFixture {
                name: "Passive",
                defeated: false,
                passive: true,
                harvested: 300,
                kill_score: 0,
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
                harvested: 40,
                kill_score: 10,
                units_killed: 1,
                buildings_killed: 0,
                units_lost: 2,
                buildings_lost: 0,
                built: 3,
            },
        );

        sim.session.house_order = vec![omega, alpha, defeated, zero];
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
    fn terminal_score_snapshot_is_latched_without_repeating_rng_draws() {
        let mut sim = Simulation::with_seed(0x51C0_5678);
        let owner = insert_house(
            &mut sim,
            HouseFixture {
                name: "Alpha",
                defeated: false,
                passive: false,
                harvested: 100,
                kill_score: 0,
                units_killed: 0,
                buildings_killed: 0,
                units_lost: 0,
                buildings_lost: 0,
                built: 0,
            },
        );
        sim.session.house_order.push(owner);

        assert!(sim.finalize_terminal_score_snapshot());
        let first = sim
            .terminal_score_snapshot()
            .expect("first terminal score snapshot")
            .clone();
        let cursor_after_first = sim.clone_scenario_rng().state();
        let hash_after_first = sim.state_hash();
        assert!(!sim.finalize_terminal_score_snapshot());
        let second = sim
            .terminal_score_snapshot()
            .expect("latched terminal score snapshot")
            .clone();

        assert_eq!(second, first);
        assert_eq!(sim.clone_scenario_rng().state(), cursor_after_first);
        assert_eq!(sim.state_hash(), hash_after_first);
    }

    fn natural_terminal_sim(seed: u64) -> Simulation {
        let mut sim = Simulation::with_seed(seed);
        let human = sim.interner.intern("Human");
        let mut human_house = HouseState::new(human, 0, None, true, 0, 10);
        human_house.is_defeated = true;
        human_house.economy.harvested_credits = 200;
        human_house.outcome_state = Some(HouseOutcomeState {
            kind: HouseOutcomeKind::Defeat,
            savour_until_tick: 0,
            exit_ready: true,
        });
        sim.houses.insert(human, human_house);

        let opponent = sim.interner.intern("Opponent");
        let mut opponent_house = HouseState::new(opponent, 1, None, false, 0, 10);
        opponent_house.economy.harvested_credits = 100;
        sim.houses.insert(opponent, opponent_house);
        sim.session.house_order = vec![human, opponent];
        sim
    }

    #[test]
    fn natural_terminal_frame_finalizes_score_before_returned_hash_once() {
        let mut sim = natural_terminal_sim(0x51C0_9ABC);

        let first = sim.advance_tick(
            &[],
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );

        assert!(!first.frame_committed);
        assert!(first.terminal_score_finalized);
        assert_eq!(first.state_hash, sim.state_hash());
        let first_snapshot = sim
            .terminal_score_snapshot()
            .expect("natural terminal frame latches score")
            .clone();
        let cursor_after_first = sim.clone_scenario_rng().state();

        let second = sim.advance_tick(
            &[],
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
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
    fn quit_and_connection_alone_do_not_score_but_ready_outcome_wins_exit_race() {
        let height_map = std::collections::BTreeMap::new();

        let mut quit_only = Simulation::with_seed(0x51C0_B001);
        quit_only.quit_requested = true;
        let quit_cursor = quit_only.clone_scenario_rng().state();
        let quit_tick = quit_only.advance_tick(&[], None, &height_map, None, None, 67);
        assert!(!quit_tick.frame_committed);
        assert!(!quit_tick.terminal_score_finalized);
        assert!(quit_only.terminal_score_snapshot().is_none());
        assert_eq!(quit_only.clone_scenario_rng().state(), quit_cursor);

        let mut connection_only = Simulation::with_seed(0x51C0_B002);
        connection_only.connection_lost = true;
        let connection_cursor = connection_only.clone_scenario_rng().state();
        let connection_tick = connection_only.advance_tick(&[], None, &height_map, None, None, 67);
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
        let raced_tick = raced.advance_tick(&[exit], None, &height_map, None, None, 67);
        assert!(!raced_tick.frame_committed);
        assert_eq!(raced_tick.executed_commands, 1);
        assert!(raced_tick.terminal_score_finalized);
        assert!(raced.terminal_score_snapshot().is_some());
    }

    #[test]
    fn terminal_score_snapshot_roundtrips_in_current_version() {
        let mut sim = natural_terminal_sim(0x51C0_DEF0);
        let tick = sim.advance_tick(
            &[],
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
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
        let tick = recorded.advance_tick(
            &[],
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
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
        let hashes = ReplayRunner::run_fixture(
            &mut playback,
            &replay,
            None,
            &std::collections::BTreeMap::new(),
            None,
            67,
        );

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
