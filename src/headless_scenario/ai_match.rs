//! Playing a match between computer houses, headless, for evaluation.
//!
//! The trainer and the evaluation tool both need the same thing: a match
//! between computer houses, played to its end on a retail map, that reports
//! who won. This module owns that run, so both tools read the same result
//! and the trainer's numbers mean what the evaluator's numbers mean.
//!
//! The match runs on the lockstep route, because the decision of who won
//! lives there: `Simulation::network_match_winners` answers only for a
//! lockstep session. Every house is a computer house, so the match ends when
//! one side is left standing.
//!
//! The rules are the retail ones. A run may replace VERA's own computer-player
//! policy with one the caller holds, which is how the trainer tries a set of
//! numbers; a run that does not keeps the retail policy, and the retail
//! computer player plays.

use std::path::Path;

use crate::headless_scenario::{self, HeadlessScenario};
use crate::rules::vera_ai::VeraAiPolicy;
use crate::sim::command::CommandEnvelope;
use crate::sim::intern::InternedId;
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, SkirmishAiSlot, SkirmishLaunchMode, SkirmishLaunchOptions,
    SkirmishLaunchSession,
};

/// The countries the computer houses take, in order, so that two houses in a
/// match are never the same country.
const COUNTRIES: [LaunchCountry; 10] = [
    LaunchCountry::Russia,
    LaunchCountry::France,
    LaunchCountry::Yuri,
    LaunchCountry::Iraq,
    LaunchCountry::Korea,
    LaunchCountry::Cuba,
    LaunchCountry::GreatBritain,
    LaunchCountry::America,
    LaunchCountry::Germany,
    LaunchCountry::Libya,
];

/// The difficulties the computer houses take, in order.
const DIFFICULTIES: [AiDifficulty; 3] =
    [AiDifficulty::Hard, AiDifficulty::Normal, AiDifficulty::Easy];

/// What a match to play holds.
#[derive(Debug, Clone)]
pub struct AiMatchRequest {
    /// The retail map file, as the asset resolver names it (`"Death.mmx"`).
    pub map: String,
    /// The seed the match runs from.
    pub seed: u32,
    /// How many computer houses play.
    pub computers: usize,
    /// How many frames the match may run before it is called unfinished.
    pub ticks: u64,
    /// VERA's own computer-player policy for this match.
    pub policy: VeraAiPolicy,
    /// How far the computer houses' roster is rotated, so that a caller may
    /// play the same match from the other side.
    pub rotation: usize,
}

impl Default for AiMatchRequest {
    fn default() -> Self {
        Self {
            map: "Death.mmx".to_string(),
            seed: 1,
            computers: 2,
            ticks: 9_000,
            policy: VeraAiPolicy::default(),
            rotation: 0,
        }
    }
}

/// What a played match ended with.
#[derive(Debug, Clone, Default)]
pub struct AiMatchOutcome {
    /// How many frames the match ran.
    pub ticks: u64,
    /// Whether the match reached a decision before the frame limit.
    pub decided: bool,
    /// The houses that were still standing, by name.
    pub survivors: Vec<String>,
    /// The houses that were defeated, by name.
    pub defeated: Vec<String>,
    /// The state hash of the world the match ended in.
    pub state_hash: u64,
    /// The objects the world held at the end.
    pub live_objects: usize,
    /// How many living objects each house held at the end, by name.
    pub house_objects: Vec<(String, usize)>,
    /// The computer houses, in house order. The first is the one VERA's own
    /// player takes when the policy limits it to one house.
    pub houses: Vec<String>,
    /// Whether the two computer houses are allied: a match whose houses are
    /// allies has no fight to win.
    pub houses_allied: bool,
    /// How each house's living objects are spread over the categories and
    /// the missions, by name: the mobile objects, and how many of them are
    /// on an attack.
    pub house_detail: Vec<(String, HouseDetail)>,
    /// What each house was worth at the end, by name: the strength of the
    /// objects it held. A house's worth falls as it loses them, so an
    /// unfinished match still says who is ahead.
    pub house_value: Vec<(String, i64)>,
}

impl AiMatchOutcome {
    /// Whether the house named `name` was still standing at the end.
    pub fn survived(&self, name: &str) -> bool {
        self.survivors.iter().any(|survivor| survivor == name)
    }

    /// How many living objects the house named `name` held at the end.
    pub fn objects_of(&self, name: &str) -> usize {
        self.house_objects
            .iter()
            .find(|(house, _)| house == name)
            .map_or(0, |(_, objects)| *objects)
    }

    /// What the match is worth to the house named `vera`: it won when it is
    /// the last one standing, it lost when it was defeated, and an unfinished
    /// match is worth the difference of what the two houses hold.
    ///
    /// A win is worth `1.0` and a loss `-1.0`, so no unfinished match is
    /// worth as much as the win it did not reach. Between them the value
    /// difference says how far ahead the house is, so a match that neither
    /// side won still tells the trainer which way it went.
    pub fn reward(&self, vera: &str, opponent: &str) -> f64 {
        if self.decided {
            if self.survived(vera) {
                return 1.0;
            }
            return -1.0;
        }
        if self.defeated.iter().any(|house| house == vera) {
            return -1.0;
        }
        let mine = self.value_of(vera) as f64;
        let theirs = self.value_of(opponent) as f64;
        0.5 * (mine - theirs) / (mine + theirs).max(1.0)
    }

    /// What the house named `name` was worth at the end of the match.
    pub fn value_of(&self, name: &str) -> i64 {
        self.house_value
            .iter()
            .find(|(house, _)| house == name)
            .map_or(0, |(_, value)| *value)
    }
}

/// What a house held at the end of a match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HouseDetail {
    pub infantry: usize,
    pub vehicles: usize,
    pub aircraft: usize,
    pub structures: usize,
    /// The mobile objects that were on an attack or a hunt.
    pub attacking: usize,
    /// The mobile objects that were walking to a place while engaging.
    pub marching: usize,
    /// The mobile objects that were walking to a place.
    pub moving: usize,
    /// The mobile objects that were holding a place.
    pub guarding: usize,
}

/// Whether one computer house is left standing: the match is over.
fn one_side_left(simulation: &crate::sim::world::Simulation) -> bool {
    let standing = simulation
        .houses()
        .order()
        .iter()
        .filter(|owner| {
            simulation.houses().get(owner).is_some_and(|house| {
                !house.is_defeated() && !house.is_human() && !house.multiplay_passive()
            })
        })
        .count();
    standing <= 1
}

/// The launch a request plays: computer houses only, no human's.
///
/// A match with a human in it ends only when the human's defeat ends it; a
/// match with none ends when one side is left, which is the match the
/// trainer needs.
fn session(request: &AiMatchRequest) -> SkirmishLaunchSession {
    let opponents = (0..request.computers)
        .map(|slot| {
            let index = (slot + request.rotation) % COUNTRIES.len();
            SkirmishAiSlot {
                raw_slot_index: slot as u8,
                country: COUNTRIES[index],
                country_random: false,
                color_index: slot as u8,
                color_random: false,
                start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
                team: crate::skirmish_launch::LaunchTeam::None,
                difficulty: DIFFICULTIES[index % DIFFICULTIES.len()],
            }
        })
        .collect();
    SkirmishLaunchSession::from_rows(
        SkirmishLaunchMode::stock(crate::skirmish_launch::BATTLE_MODE_ID)
            .expect("the stock Battle row"),
        Some(request.map.clone()),
        Vec::new(),
        0,
        opponents,
        SkirmishLaunchOptions::default(),
    )
}

/// Play the match the request holds, and report how it ended.
pub fn play_ai_match(
    retail_dir: &Path,
    request: &AiMatchRequest,
) -> Result<AiMatchOutcome, String> {
    let mut scenario = load(retail_dir, request)?;
    let outcome = run(&mut scenario, request)?;
    Ok(outcome)
}

/// Load the match's world: the retail map, the computer houses, and the
/// policy the request holds.
fn load(retail_dir: &Path, request: &AiMatchRequest) -> Result<HeadlessScenario, String> {
    let mut scenario =
        headless_scenario::load_session(retail_dir, &request.map, request.seed, session(request))?;
    scenario
        .runtime
        .resources_mut()
        .rules
        .set_vera_ai(request.policy);
    let simulation = &mut scenario.runtime.simulation;
    simulation.batch_path_searches();
    simulation.batch_target_scans();
    Ok(scenario)
}

/// Run the loaded match until it is decided or the frame limit is reached.
fn run(
    scenario: &mut HeadlessScenario,
    request: &AiMatchRequest,
) -> Result<AiMatchOutcome, String> {
    let mut ticks = 0u64;
    let mut decided = false;
    while ticks < request.ticks {
        let commands: Vec<CommandEnvelope> = Vec::new();
        scenario
            .runtime
            .advance_frame_for_tooling(&commands, headless_scenario::SIM_TICK_MS)
            .map_err(|error| format!("tick {ticks}: {error}"))?;
        ticks += 1;
        if one_side_left(&scenario.runtime.simulation) {
            decided = true;
            break;
        }
    }
    let simulation = &scenario.runtime.simulation;
    let rules = &scenario.runtime.resources.rules;
    let mut survivors = Vec::new();
    let mut defeated = Vec::new();
    let mut house_objects = Vec::new();
    let mut house_detail: Vec<(String, HouseDetail)> = Vec::new();
    let mut house_value: Vec<(String, i64)> = Vec::new();
    let mut houses = Vec::new();
    let mut computer_houses: Vec<InternedId> = Vec::new();
    for owner in simulation.houses().order() {
        let Some(house) = simulation.houses().get(owner) else {
            continue;
        };
        let name = simulation.resolve(*owner).to_string();
        let mut objects = 0;
        let mut value = 0i64;
        let mut detail = HouseDetail::default();
        for entity in simulation.entities().values() {
            if entity.owner() != *owner || !entity.is_live_on_map() {
                continue;
            }
            objects += 1;
            let strength = rules
                .object(simulation.resolve(entity.type_ref()))
                .map(|object| i64::from(object.strength))
                .unwrap_or(0);
            value += strength.max(1);
            match entity.category() {
                crate::map::entities::EntityCategory::Infantry => detail.infantry += 1,
                crate::map::entities::EntityCategory::Unit => detail.vehicles += 1,
                crate::map::entities::EntityCategory::Aircraft => detail.aircraft += 1,
                crate::map::entities::EntityCategory::Structure => detail.structures += 1,
            }
            if entity.category() != crate::map::entities::EntityCategory::Structure
                && simulation.mega_mission(entity.stable_id()).is_attack_move()
            {
                detail.marching += 1;
            }
            match entity.mission.current().known() {
                Some(crate::rules::mission_data::MissionType::Move) => detail.moving += 1,
                Some(crate::rules::mission_data::MissionType::Guard)
                | Some(crate::rules::mission_data::MissionType::AreaGuard) => detail.guarding += 1,
                _ => {}
            }
            if entity.category() != crate::map::entities::EntityCategory::Structure
                && entity.mission.current().known().is_some_and(|mission| {
                    matches!(
                        mission,
                        crate::rules::mission_data::MissionType::Attack
                            | crate::rules::mission_data::MissionType::AttackMove
                            | crate::rules::mission_data::MissionType::Hunt
                    )
                })
            {
                detail.attacking += 1;
            }
        }
        house_objects.push((name.clone(), objects));
        house_value.push((name.clone(), value));
        house_detail.push((name.clone(), detail));
        if !house.is_human() && !house.multiplay_passive() {
            houses.push(name.clone());
            computer_houses.push(*owner);
        }
        if house.is_defeated() {
            defeated.push(name);
        } else {
            survivors.push(name);
        }
    }
    let houses_allied = computer_houses.len() == 2
        && simulation.house_alliances.is_allied(
            &simulation.interner,
            computer_houses[0],
            computer_houses[1],
        );
    Ok(AiMatchOutcome {
        ticks,
        decided,
        survivors,
        defeated,
        state_hash: simulation.state_hash(),
        live_objects: simulation.entities().len(),
        house_objects,
        houses,
        house_detail,
        house_value,
        houses_allied,
    })
}

/// The name of the house VERA's own player takes in a match: the first
/// computer house in house order.
pub fn vera_house_name(outcome: &AiMatchOutcome) -> Option<&str> {
    outcome.houses.first().map(String::as_str)
}
