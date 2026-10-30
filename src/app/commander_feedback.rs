//! Skirmish commander talking-heads: the clip a computer opponent plays in the
//! radar window when a personality moment happens.
//!
//! This is the product-extension half of
//! [`crate::rules::personality`]: that module decides *which* commander a
//! computer house is, this one picks the clip that commander has for a moment
//! and hands it to retail's own transmission path.
//! Nothing here has gamemd evidence — retail has no personalities and no such
//! videos — so no line may cite a native address.
//!
//! Scope, deliberately:
//!
//! * Computer houses only. A house the local player controls never speaks, and
//!   a house without an assigned personality has nothing to say.
//! * Skirmish only (`ScenarioGameMode::Skirmish`), never a replay.
//! * Presentation only. The queue, the clip choice and the subtitle are local:
//!   they consume no simulation RNG, are not synchronised and never touch a
//!   save. Two clients legitimately show different clips.
//! * `[VERA] CommanderFeedback=no` turns the whole layer off. Turning it off
//!   leaves the personality — and therefore the AI's trigger weights — intact.
//!
//! The events come from the world's own observable state rather than from new
//! simulation events, so nothing in the simulation knows this exists:
//!
//! | event | moment | spoken by |
//! |---|---|---|
//! | `StartGame` | opening of the match | an ally greets |
//! | `FirstSeen` | any enemy object — building, defence, vehicle, soldier or aircraft — standing on a cell the local player sees | that enemy taunts |
//! | `InTrouble` | `HouseTracking::buildings` losing `[CommanderFeedback] TroubleBuildingsLost=` | an ally calls for help |
//! | `DefeatedLocalPlayer` | the local player's house defeated | an enemy gloats |
//! | `DefeatedByLocalPlayer` | an enemy house destroyed while the local player stands | that enemy, playing its losing clip |
//! | `Lost` | an ally house destroyed | that ally, playing its losing clip |
//!
//! Every event is read through the speaker's own relation to the local player
//! (`[Houses]`' ally graph, the same one the tactical view colours by), and
//! [`speaks_to`] is the one place that decides which relation may say what: a
//! friend greets, calls for help and admits its own defeat; an opponent taunts,
//! gloats and admits defeat. The two moments where a house is destroyed are not
//! a side's privilege — the house that lost speaks, whoever the player is to
//! it. A moment with no line for this relation is silent rather than using the
//! other side's line: an enemy asking the player for help reads as a bug, and a
//! friend's greeting in an enemy's mouth gives the commander away.
//!
//! This layer does not play anything: it picks the clip and hands the file
//! name to the retail radar transmission FIFO
//! ([`crate::app::radar_movie::enqueue_file`]). The IncomingTransmission
//! prelude, the housing that opens over the tactical map, the Bink decode, the
//! movie audio hold, the `subtitlemd.txt` cues on the message lane and the
//! closing are all retail's own code. There is no second screen: whatever the
//! radar window does with a campaign transmission, it does with these clips
//! too, including when the radar itself is offline — `movie_step` opens the
//! housing from `Offline` just as it does from `Online`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::rules::personality::{FeedbackAudience, FeedbackEntry, FeedbackEvent, PersonalityDef};
use crate::sim::intern::InternedId;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::ui::game_screen::GameScreen;

/// Simulation ticks before the opening greeting. The world is a second old by
/// then, so the opponent greets a match that has actually started.
const START_GREETING_DELAY_TICKS: u64 = 20;
/// `InTrouble` scan cadence: 5 s at the native 15 Hz.
const TROUBLE_SCAN_TICKS: u64 = 75;
/// `FirstSeen` sweep cadence: 1 s at the native 15 Hz. First contact must feel
/// immediate, unlike the trouble watch which measures a slow trend.
const FIRST_SEEN_SCAN_TICKS: u64 = 15;
/// Buildings a house must lose from its own high-water mark to be in trouble,
/// and the silence that follows, come from `[CommanderFeedback]`
/// (`TroubleBuildingsLost=`, `TroubleQuiet=`). The defaults live in
/// [`crate::rules::personality::CommanderFeedbackRules`]: one building, three
/// minutes.
/// Clips a single burst of events may queue. A seven-opponent match greets with
/// seven clips; more than this in one frame is a duplicate, not a queue.
const QUEUE_LIMIT: usize = 8;

/// One house's trouble high-water mark.
#[derive(Debug, Clone, Copy, Default)]
struct TroubleWatch {
    /// Most buildings seen since the last clip.
    peak: i32,
    last_scan_tick: u64,
    /// Wall time before which this house may be in trouble again.
    quiet_until_ms: u64,
}

/// What [`CommanderFeedback::enqueue`] resolved: the entry's own
/// `EntryN.Timeout=`, which the `InTrouble` watcher reads as its silence.
struct Queued {
    timeout_seconds: Option<u32>,
}

/// A clip waiting for its turn.
struct ClipRequest {
    house: InternedId,
    entry: FeedbackEntry,
    /// Kept for the diagnostic line only; the entry was already chosen with it.
    relation: Relation,
}

/// How the speaker stands towards the local player. This is what RA3's
/// `EntryN.To=` (`FeedbackTo=`) tokens name, and it is the whole difference
/// between a commander greeting you and a commander taunting you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    /// Same alliance: the only thing this commander says is its greeting.
    Ally,
    /// Not an ally: everything it says is aimed at the player.
    Enemy,
}

impl Relation {
    /// The `FeedbackTo=` token this relation listens to.
    fn audience(self) -> FeedbackAudience {
        match self {
            Self::Ally => FeedbackAudience::ALLIES,
            Self::Enemy => FeedbackAudience::ENEMIES,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ally => "ally",
            Self::Enemy => "enemy",
        }
    }
}

/// Who each moment is spoken to. This is the whole friend/foe split, in one
/// place: a friend greets at the opening, calls for help when its base is being
/// overrun, and says so when it is wiped out; an opponent taunts on first
/// contact, gloats when the player loses and also admits its own defeat.
///
/// It is a gate, not a preference. A moment whose relation does not match is
/// not played at all: the entry's own `To=` token is a second filter on top of
/// this one, so a mis-filed entry can only ever cost a line, never put the
/// wrong words in a commander's mouth.
fn speaks_to(event: FeedbackEvent, relation: Relation) -> bool {
    match event {
        FeedbackEvent::StartGame | FeedbackEvent::InTrouble => relation == Relation::Ally,
        FeedbackEvent::FirstSeen | FeedbackEvent::DefeatedLocalPlayer => {
            relation == Relation::Enemy
        }
        // Losing is nobody's privilege. An ally that is overrun and an opponent
        // the player destroys both play a losing clip, from their own pool:
        // `Lost` for the ally, `DefeatedByLocalPlayer` for the enemy.
        FeedbackEvent::DefeatedByLocalPlayer | FeedbackEvent::Lost => true,
    }
}

/// Every event the world showed this frame that a commander may comment on.
struct Observed {
    house: InternedId,
    personality: u16,
    event: FeedbackEvent,
    seed: u64,
    relation: Relation,
}

/// A house's building count at a scan boundary, applied after the read-only
/// observation pass so the watchers can be updated.
struct Scan {
    house: InternedId,
    personality: u16,
    buildings: i32,
    tick: u64,
    seed: u64,
    /// The scanning house's own relation, so the trouble watch asks
    /// [`speaks_to`] instead of assuming an audience.
    relation: Relation,
}

/// Match-scoped owner of the commander clips. `Default` is "nothing playing and
/// nothing remembered", which is also what a retired scenario returns to.
#[derive(Default)]
pub(crate) struct CommanderFeedback {
    /// Houses whose `StartGame` has been spent.
    greeted: BTreeSet<InternedId>,
    /// Houses the local player has met in the field (`FirstSeen`).
    seen: BTreeSet<InternedId>,
    /// Houses whose defeat has been announced (either defeat event).
    announced: BTreeSet<InternedId>,
    /// Whether the local player's own defeat has been announced.
    local_defeat_announced: bool,
    /// The last clip each house played, so two moments that share a pool do
    /// not play the same file twice in a row.
    last_clip: BTreeMap<InternedId, String>,
    trouble: BTreeMap<InternedId, TroubleWatch>,
    queue: VecDeque<ClipRequest>,
    /// One-shot diagnostic: the first frame this layer actually ran.
    logged_start: bool,
}

impl CommanderFeedback {
    /// Drop everything. Used when the feature is switched off, the screen
    /// leaves the battle, or the scenario is torn down. A clip already handed
    /// to the radar FIFO ends with that FIFO's own teardown
    /// (`crate::app::radar_movie::retire`), which every retire path runs.
    pub(crate) fn retire(&mut self) {
        *self = Self::default();
    }
}

/// The feature's own admit test. Only a live skirmish draws clips, and only
/// while the switches say so: the player's own option, and the rules' — a mod
/// or a map that ships its own scripted AI writes
/// `[CommanderFeedback] Enabled=no` (no clips) or `Personalities=no` (no
/// commander at all) and gets no talking heads.
fn enabled(state: &AppState) -> bool {
    state.persistence.options_profile.commander_feedback
        && state
            .rules()
            .map(|rules| {
                let rules = rules.commander_feedback();
                rules.enabled && rules.personalities
            })
            .unwrap_or(true)
        && state.frontend.screen == GameScreen::InGame
        && !state.match_state.match_diagnostics.is_replay_playback()
}

/// Scenario teardown. Drops this layer's own queue; the clip it may already
/// have handed to the radar FIFO is cleared by
/// [`crate::app::radar_movie::retire`], which every teardown path calls first.
pub(crate) fn retire(state: &mut AppState) {
    state
        .match_state
        .match_presentation
        .commander_feedback
        .retire();
}

/// Drive the clip queue. Called once per frame outside the simulation gate, so
/// a paused game finishes the sentence it started.
pub(crate) fn service(state: &mut AppState, wall_ms: u64) {
    // Taken out for the pass: the loader needs `process_assets` and
    // `renderer` mutably while the queue is mutated, and both live in `state`.
    let mut feedback = std::mem::take(&mut state.match_state.match_presentation.commander_feedback);
    feedback.service_inner(state, wall_ms);
    state.match_state.match_presentation.commander_feedback = feedback;
}

impl CommanderFeedback {
    fn service_inner(&mut self, state: &mut AppState, wall_ms: u64) {
        if !enabled(state) {
            self.retire();
            return;
        }
        let Some((observed, scans)) = self.observe(state) else {
            // Not a skirmish, no world, or no personalities at all.
            self.retire();
            return;
        };
        for scan in scans {
            self.apply_scan(state, scan, wall_ms);
        }
        for observed in observed {
            self.record_and_enqueue(state, observed);
        }
        self.advance(state);
    }

    /// Read-only sweep. `None` means the layer does not apply at all this
    /// frame; an empty pair means it applies but nothing happened.
    fn observe(&mut self, state: &AppState) -> Option<(Vec<Observed>, Vec<Scan>)> {
        let runtime = state.match_state.sim_runtime.as_ref()?;
        let sim = &runtime.simulation;
        // `game_mode_nonzero()` is true for LAN, WOL *and* skirmish, so it
        // cannot stand in for "this is a skirmish"; only mode 5 is one.
        if sim.session().game_mode() != ScenarioGameMode::Skirmish {
            return None;
        }
        let local = sim.session().current_house()?;
        let tick = sim.session().tick();
        let rules = state.rules()?;
        let catalog = rules.personalities();
        if catalog.is_empty() {
            return None;
        }
        let local_defeated = sim
            .houses()
            .get(&local)
            .is_some_and(|house| house.is_defeated());
        // The alliance map the tactical presentation itself reads: the same
        // answer the player sees on screen decides who is a friendly commander.
        let alliances = sim.house_alliances();
        let local_name = sim.resolve(local);
        // First contact is the local player's own sight test, so it runs on its
        // own cadence and stops as soon as every opponent has been met. Allies
        // never enter this set: they are not met in the field, they are on the
        // same side, and scanning for them would keep the sweep running for
        // the whole match.
        let awaiting_contact = sim.houses().order().iter().copied().any(|id| {
            id != local
                && !self.seen.contains(&id)
                && !crate::map::houses::is_allied_with(alliances, local_name, sim.resolve(id))
        });
        let spotted = if awaiting_contact && tick % FIRST_SEEN_SCAN_TICKS == 0 {
            spotted_houses(sim, local)
        } else {
            BTreeSet::new()
        };

        let mut observed = Vec::new();
        let mut scans = Vec::new();
        let mut speakers = 0usize;
        let mut roster = Vec::new();
        for (index, house_id) in sim.houses().order().iter().copied().enumerate() {
            let Some(house) = sim.houses().get(&house_id) else {
                continue;
            };
            // The local player's own commander never talks to them, and a
            // house with no personality has no clips.
            if house_id == local {
                continue;
            }
            let Some(personality) = house.personality() else {
                continue;
            };
            if catalog.get(personality).is_none() {
                continue;
            }
            speakers += 1;
            let relation =
                if crate::map::houses::is_allied_with(alliances, local_name, sim.resolve(house_id))
                {
                    Relation::Ally
                } else {
                    Relation::Enemy
                };
            roster.push(format!("{} ({})", sim.resolve(house_id), relation.label()));
            let seed = feedback_seed(index, 0, tick);
            // Opening words are for a friend only. An opponent stays silent
            // until the player meets its army: retail writes every `StartGame`
            // line `To=ALLIES`, and greeting the player with the line a friend
            // would use both gives the commander away before the match has
            // begun and spends the contact taunt the field meeting earns.
            if speaks_to(FeedbackEvent::StartGame, relation)
                && !house.is_defeated()
                && tick >= START_GREETING_DELAY_TICKS
                && !self.greeted.contains(&house_id)
            {
                observed.push(Observed {
                    house: house_id,
                    personality,
                    event: FeedbackEvent::StartGame,
                    seed,
                    relation,
                });
            }
            if house.is_defeated() && !self.announced.contains(&house_id) {
                // Which losing line this is depends on who lost, not on who
                // won: an ally that is overrun says so with `Lost`, an opponent
                // the player destroys with `DefeatedByLocalPlayer`. Both are the
                // defeated house's own clip, and `speaks_to` lets either
                // relation speak them.
                let event = match relation {
                    Relation::Ally => FeedbackEvent::Lost,
                    Relation::Enemy => FeedbackEvent::DefeatedByLocalPlayer,
                };
                if speaks_to(event, relation) {
                    observed.push(Observed {
                        house: house_id,
                        personality,
                        event,
                        seed,
                        relation,
                    });
                }
            }
            // First contact: the player can see something of this house. A
            // building — a defence especially — usually comes first.
            if speaks_to(FeedbackEvent::FirstSeen, relation)
                && !house.is_defeated()
                && !self.seen.contains(&house_id)
                && spotted.contains(&house_id)
            {
                observed.push(Observed {
                    house: house_id,
                    personality,
                    event: FeedbackEvent::FirstSeen,
                    seed: feedback_seed(index, 3, tick),
                    relation,
                });
            }
            // Scanned for every speaker, gated in `apply_scan`: an ally's base
            // being shelled is a call for help, an opponent's is not the
            // player's business.
            if !house.is_defeated() {
                let last = self
                    .trouble
                    .get(&house_id)
                    .map_or(0, |watch| watch.last_scan_tick);
                if tick > 0 && tick.saturating_sub(last) >= TROUBLE_SCAN_TICKS {
                    scans.push(Scan {
                        house: house_id,
                        personality,
                        buildings: house.tracking().buildings(),
                        tick,
                        seed: feedback_seed(index, 1, tick),
                        relation,
                    });
                }
            }
        }

        // The local player's defeat is announced by an opponent that is still
        // standing — that is the one who beat them.
        if local_defeated && !self.local_defeat_announced {
            for (index, house_id) in sim.houses().order().iter().copied().enumerate() {
                let Some(house) = sim.houses().get(&house_id) else {
                    continue;
                };
                if house_id == local || house.is_defeated() {
                    continue;
                }
                let Some(personality) = house.personality() else {
                    continue;
                };
                if catalog.get(personality).is_none() {
                    continue;
                }
                if !crate::map::houses::is_allied_with(alliances, local_name, sim.resolve(house_id))
                {
                    observed.push(Observed {
                        house: house_id,
                        personality,
                        event: FeedbackEvent::DefeatedLocalPlayer,
                        seed: feedback_seed(index, 2, tick),
                        relation: Relation::Enemy,
                    });
                    break;
                }
            }
        }

        if !self.logged_start {
            self.logged_start = true;
            // The alliance graph is the only thing that separates a friendly
            // commander from a hostile one, so it is worth one line: an empty
            // ally set here means the launch did not carry the player's team,
            // and every commander would then be treated as an opponent.
            let local_allies = local_name.trim().to_ascii_uppercase();
            log::info!(
                "Commander feedback active: {} personalities, {speakers} computer houses with a commander; \
                 local={local_name} allies={:?}; speakers: {}",
                catalog.len(),
                alliances.get(&local_allies),
                roster.join(", ")
            );
        }
        Some((observed, scans))
    }

    fn record_and_enqueue(&mut self, state: &AppState, observed: Observed) {
        let _ = self.enqueue(
            state,
            observed.house,
            observed.personality,
            observed.event,
            observed.seed,
            observed.relation,
        );
        match observed.event {
            FeedbackEvent::StartGame => {
                self.greeted.insert(observed.house);
            }
            FeedbackEvent::FirstSeen => {
                self.seen.insert(observed.house);
            }
            FeedbackEvent::DefeatedByLocalPlayer | FeedbackEvent::Lost => {
                self.announced.insert(observed.house);
            }
            FeedbackEvent::DefeatedLocalPlayer => {
                self.local_defeat_announced = true;
            }
            FeedbackEvent::InTrouble => {}
        }
    }

    fn apply_scan(&mut self, state: &AppState, scan: Scan, wall_ms: u64) {
        // `[CommanderFeedback]` owns the sensitivity: one lost building is
        // enough by default, because a commander whose base is being shelled
        // should say so on the first building, not on the third.
        let rules = state
            .rules()
            .map(|rules| rules.commander_feedback())
            .unwrap_or_default();
        // Read-only half: update the watch, decide whether this is an emergency.
        let quiet_until_ms = {
            let watch = self.trouble.entry(scan.house).or_insert(TroubleWatch {
                peak: scan.buildings,
                ..TroubleWatch::default()
            });
            watch.last_scan_tick = scan.tick;
            watch.peak = watch.peak.max(scan.buildings);
            // A call for help is spoken to friends. An opponent losing
            // buildings is not something this layer has a line for, and the
            // enemy defeat lines it does have belong to the moment the house
            // actually falls.
            if !speaks_to(FeedbackEvent::InTrouble, scan.relation) {
                return;
            }
            // Recovering and losing again is a new emergency; the high-water
            // mark follows the house up so a rebuilt base can fall once more.
            let lost = watch.peak - scan.buildings;
            if lost < rules.trouble_buildings_lost {
                if lost > 0 {
                    log::debug!(
                        "Commander feedback: {:?} lost {lost} of {} buildings, \
                         {} are needed for InTrouble",
                        scan.house,
                        watch.peak,
                        rules.trouble_buildings_lost
                    );
                }
                return;
            }
            watch.quiet_until_ms
        };
        if wall_ms < quiet_until_ms {
            return;
        }
        let timeout = self
            .enqueue(
                state,
                scan.house,
                scan.personality,
                FeedbackEvent::InTrouble,
                scan.seed,
                scan.relation,
            )
            .and_then(|queued| queued.timeout_seconds)
            .unwrap_or(0);
        let quiet_ms = u64::from(timeout)
            .checked_mul(1000)
            .filter(|quiet| *quiet > 0)
            .unwrap_or_else(|| u64::from(rules.trouble_quiet_seconds).saturating_mul(1000));
        if let Some(watch) = self.trouble.get_mut(&scan.house) {
            watch.peak = scan.buildings;
            watch.quiet_until_ms = wall_ms + quiet_ms;
        }
    }

    /// Pick one entry for `event` and queue its clip.
    fn enqueue(
        &mut self,
        state: &AppState,
        house: InternedId,
        personality: u16,
        event: FeedbackEvent,
        seed: u64,
        relation: Relation,
    ) -> Option<Queued> {
        if self.queue.len() >= QUEUE_LIMIT {
            return None;
        }
        let catalog = state.rules()?.personalities();
        let definition = catalog.get(personality)?;
        // Only an installed clip can be played. Choosing among the files that
        // actually exist keeps one missing conversion from taking the whole
        // event down with it.
        let avoid = self.last_clip.get(&house).map(String::as_str);
        let entry = choose_entry(
            definition,
            event,
            seed,
            relation,
            |video| clip_installed(state, video),
            avoid,
        )?
        .clone();
        if let Some(video) = entry.video.as_deref() {
            self.last_clip.insert(house, video.to_owned());
        }
        let timeout_seconds = entry.timeout_seconds;
        self.queue.push_back(ClipRequest {
            house,
            entry,
            relation,
        });
        Some(Queued { timeout_seconds })
    }

    /// Hand the head of the queue to the retail radar FIFO when that FIFO is
    /// free. Nothing here decodes, paints or speaks: from the admission on,
    /// the clip is an ordinary transmission.
    ///
    /// One clip at a time. A burst of events (seven opponents greeting) plays
    /// back to back instead of stacking, and a stale clip cannot wait minutes
    /// behind a queue nobody is watching.
    fn advance(&mut self, state: &mut AppState) {
        // Without a radar housing there is no aperture at all; waiting keeps
        // clips from piling up in a FIFO that cannot draw them.
        if state.match_state.match_presentation.radar_anim.is_none()
            || crate::app::radar_movie::has_pending(state)
        {
            return;
        }
        while let Some(request) = self.queue.pop_front() {
            let Some(video) = request.entry.video.as_deref() else {
                continue;
            };
            if !clip_installed(state, video) {
                log::info!("Commander feedback clip {video}.BIK is not installed");
                continue;
            }
            log::info!(
                "Commander feedback: {video}.BIK for {:?} ({}, {:?})",
                request.house,
                request.relation.label(),
                request.entry.event
            );
            // Retail's transmissions pause the scenario only when the movie
            // action asks for it; a commander quip must not stop the match.
            if crate::app::radar_movie::enqueue_file(state, video, false) {
                return;
            }
        }
    }
}

/// The entries for `event` that `audience` may hear and whose clip is
/// installed.
fn entry_pool<'a>(
    definition: &'a PersonalityDef,
    event: FeedbackEvent,
    audience: FeedbackAudience,
    installed: &impl Fn(&str) -> bool,
) -> Vec<&'a FeedbackEntry> {
    definition
        .feedback_for(event)
        .filter(|entry| entry.audience.contains(audience))
        .filter(|entry| entry.video.as_deref().is_some_and(installed))
        .collect()
}

/// One entry for `event`, chosen among the entries whose `To=` names
/// `relation` and whose clip `installed` accepts. Presentation only, so the
/// seed may come from wall time; it never reaches the simulation.
///
/// The audience is a hard filter, with one deliberate exception: an entry with
/// no `To=` at all (`ALL`) belongs to both sides and is offered to either. A
/// commander never borrows the other side's line — silence at a moment it has
/// no line for is far better than a friend's greeting in an enemy's mouth, or
/// an enemy asking the player for help.
///
/// `avoid` is the last clip this house played: two moments that share a pool
/// must not play the same file twice in a row.
fn choose_entry<'a>(
    definition: &'a PersonalityDef,
    event: FeedbackEvent,
    seed: u64,
    relation: Relation,
    installed: impl Fn(&str) -> bool,
    avoid: Option<&str>,
) -> Option<&'a FeedbackEntry> {
    let mut candidates = entry_pool(definition, event, relation.audience(), &installed);
    if candidates.is_empty() {
        return None;
    }
    if let Some(avoid) = avoid {
        if candidates.len() > 1 {
            candidates.retain(|entry| entry.video.as_deref() != Some(avoid));
        }
    }
    let pick = (seed % candidates.len() as u64) as usize;
    Some(candidates[pick])
}

/// One object as the contact sweep sees it.
#[derive(Debug, Clone, Copy)]
struct Sighting {
    owner: InternedId,
    category: EntityCategory,
    rx: u16,
    ry: u16,
}

/// The houses the local player currently has something of in sight: any
/// object of theirs standing on a cell the local player sees and no hostile
/// gap generator hides.
///
/// Buildings count, and they are usually the *first* thing met: a scout that
/// walks into the edge of an enemy base sees the pillbox that opens up on it
/// long before a tank wanders into view. Waiting for a vehicle made the taunt
/// arrive after the shooting had started, so every class is a meeting —
/// structure, defence, vehicle, soldier, aircraft.
fn spotted_houses(sim: &crate::sim::world::Simulation, local: InternedId) -> BTreeSet<InternedId> {
    let fog = sim.fog();
    contacts(
        sim.entities().values().map(|entity| Sighting {
            owner: entity.owner(),
            category: entity.category(),
            rx: entity.position.rx(),
            ry: entity.position.ry(),
        }),
        local,
        |rx, ry| fog.is_cell_visible(local, rx, ry) && !fog.is_cell_gap_covered(local, rx, ry),
    )
}

/// The owners with at least one [`Sighting`] on a cell `visible` accepts. The
/// local player's own objects are not a meeting, and one sighting per house is
/// enough — this answers "have I met them", not "how much of them can I see".
fn contacts(
    sightings: impl Iterator<Item = Sighting>,
    local: InternedId,
    visible: impl Fn(u16, u16) -> bool,
) -> BTreeSet<InternedId> {
    let mut spotted = BTreeSet::new();
    for sighting in sightings {
        if sighting.owner == local || spotted.contains(&sighting.owner) {
            continue;
        }
        if visible(sighting.rx, sighting.ry) {
            log::debug!(
                "Commander feedback: {:?} of {:?} in sight at ({}, {})",
                sighting.category,
                sighting.owner,
                sighting.rx,
                sighting.ry
            );
            spotted.insert(sighting.owner);
        }
    }
    spotted
}

/// Whether `<video>.BIK` is installed. Catalog presence only: the payload is
/// read when the clip actually starts.
fn clip_installed(state: &AppState, video: &str) -> bool {
    let file_name = format!("{video}.BIK");
    state
        .process_assets
        .manager()
        .is_some_and(|assets| assets.contains(&file_name))
}

/// Stable per-`(house, kind, tick)` selector for the entry draw. Not the
/// simulation's RNG: a different clip on another client is expected.
fn feedback_seed(house_index: usize, kind: u64, tick: u64) -> u64 {
    tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((house_index as u64) << 40)
        .wrapping_add(kind.wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(event: FeedbackEvent, video: Option<&str>) -> FeedbackEntry {
        entry_to(event, video, FeedbackAudience::ALL)
    }

    fn entry_to(
        event: FeedbackEvent,
        video: Option<&str>,
        audience: FeedbackAudience,
    ) -> FeedbackEntry {
        FeedbackEntry {
            event,
            video: video.map(str::to_owned),
            audience,
            timeout_seconds: None,
        }
    }

    /// The clip layer only ever chooses entries that name a video: a text-only
    /// entry is not a talking head.
    #[test]
    fn only_entries_with_a_clip_can_be_chosen() {
        let definition = PersonalityDef {
            feedback: vec![
                entry(FeedbackEvent::StartGame, None),
                entry(FeedbackEvent::StartGame, Some("AWAGR1")),
                entry(FeedbackEvent::StartGame, None),
                entry(FeedbackEvent::StartGame, Some("AWAGR2")),
            ],
            ..PersonalityDef::default()
        };
        for seed in 0..16 {
            let chosen = choose_entry(
                &definition,
                FeedbackEvent::StartGame,
                seed,
                Relation::Enemy,
                |_| true,
                None,
            )
            .expect("two entries carry a clip");
            assert_eq!(chosen.event, FeedbackEvent::StartGame);
            assert!(chosen.video.is_some(), "{:?}", chosen.video);
        }
        assert!(
            choose_entry(
                &definition,
                FeedbackEvent::Lost,
                0,
                Relation::Enemy,
                |_| { true },
                None
            )
            .is_none(),
            "an event with no entries has nothing to play"
        );
    }

    /// Two entries for the same event must be reachable, and the same
    /// `(house, kind, tick)` must always pick the same one — otherwise a
    /// replayed frame would flip which clip the commander "chose".
    #[test]
    fn the_entry_draw_is_stable_and_reaches_every_clip() {
        let definition = PersonalityDef {
            feedback: vec![
                entry(FeedbackEvent::InTrouble, Some("AWAND1")),
                entry(FeedbackEvent::InTrouble, Some("AWAND2")),
            ],
            ..PersonalityDef::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for tick in 0..32_u64 {
            let seed = feedback_seed(3, 1, tick);
            assert_eq!(seed, feedback_seed(3, 1, tick), "stable for one frame");
            let chosen = choose_entry(
                &definition,
                FeedbackEvent::InTrouble,
                seed,
                Relation::Enemy,
                |_| true,
                None,
            )
            .unwrap();
            seen.insert(chosen.video.clone().unwrap());
        }
        assert_eq!(seen.len(), 2, "both clips are reachable");
    }

    /// `To=` is what separates a greeting from a taunt: the same commander
    /// says the ally line to an ally and the enemy line to an opponent.
    #[test]
    fn the_audience_decides_which_line_a_commander_says() {
        let definition = PersonalityDef {
            feedback: vec![
                entry_to(
                    FeedbackEvent::StartGame,
                    Some("GREETING"),
                    FeedbackAudience::ALLIES,
                ),
                entry_to(
                    FeedbackEvent::StartGame,
                    Some("TAUNT"),
                    FeedbackAudience::ENEMIES,
                ),
            ],
            ..PersonalityDef::default()
        };
        for seed in 0..16 {
            let ally = choose_entry(
                &definition,
                FeedbackEvent::StartGame,
                seed,
                Relation::Ally,
                |_| true,
                None,
            )
            .unwrap();
            let enemy = choose_entry(
                &definition,
                FeedbackEvent::StartGame,
                seed,
                Relation::Enemy,
                |_| true,
                None,
            )
            .unwrap();
            assert_eq!(ally.video.as_deref(), Some("GREETING"));
            assert_eq!(enemy.video.as_deref(), Some("TAUNT"));
        }
    }

    /// Retail writes every `StartGame` entry `To=ALLIES`. An enemy commander
    /// has no opening line of its own and must not borrow the one the data
    /// filed for an ally: hearing it would both hand the player its character
    /// before the match has begun and spend the taunt the field meeting earns.
    /// Silence beats the wrong register.
    #[test]
    fn an_enemy_never_borrows_the_ally_opening() {
        let definition = PersonalityDef {
            feedback: vec![
                entry_to(
                    FeedbackEvent::StartGame,
                    Some("SOLGR1"),
                    FeedbackAudience::ALLIES,
                ),
                entry_to(
                    FeedbackEvent::FirstSeen,
                    Some("SOLTG1"),
                    FeedbackAudience::ENEMIES,
                ),
            ],
            ..PersonalityDef::default()
        };
        for seed in 0..8 {
            assert!(
                choose_entry(
                    &definition,
                    FeedbackEvent::StartGame,
                    seed,
                    Relation::Enemy,
                    |_| true,
                    None,
                )
                .is_none(),
                "an enemy has no opening line in retail data"
            );
            let ally = choose_entry(
                &definition,
                FeedbackEvent::StartGame,
                seed,
                Relation::Ally,
                |_| true,
                None,
            )
            .unwrap();
            assert_eq!(ally.video.as_deref(), Some("SOLGR1"));
            // Meeting them in the field is still their moment.
            let met = choose_entry(
                &definition,
                FeedbackEvent::FirstSeen,
                seed,
                Relation::Enemy,
                |_| true,
                None,
            )
            .unwrap();
            assert_eq!(met.video.as_deref(), Some("SOLTG1"));
        }
    }

    /// Two moments that share a pool must not play the same file twice: the
    /// opening taunt and the contact taunt both come from `FirstSeen`.
    #[test]
    fn the_same_clip_is_not_played_twice_in_a_row() {
        let definition = PersonalityDef {
            feedback: vec![
                entry_to(
                    FeedbackEvent::FirstSeen,
                    Some("SOLTG1"),
                    FeedbackAudience::ENEMIES,
                ),
                entry_to(
                    FeedbackEvent::FirstSeen,
                    Some("SOLTG2"),
                    FeedbackAudience::ENEMIES,
                ),
            ],
            ..PersonalityDef::default()
        };
        for seed in 0..8 {
            let second = choose_entry(
                &definition,
                FeedbackEvent::FirstSeen,
                seed,
                Relation::Enemy,
                |_| true,
                Some("SOLTG1"),
            )
            .unwrap();
            assert_eq!(second.video.as_deref(), Some("SOLTG2"));
        }
        // A pool of one has nothing else to offer, so it repeats rather than
        // going silent.
        let single = PersonalityDef {
            feedback: vec![entry_to(
                FeedbackEvent::FirstSeen,
                Some("SOLTG1"),
                FeedbackAudience::ENEMIES,
            )],
            ..PersonalityDef::default()
        };
        assert_eq!(
            choose_entry(
                &single,
                FeedbackEvent::FirstSeen,
                0,
                Relation::Enemy,
                |_| true,
                Some("SOLTG1")
            )
            .unwrap()
            .video
            .as_deref(),
            Some("SOLTG1")
        );
    }

    /// Silence beats the wrong register in both directions: an ally does not
    /// borrow an enemy line any more than an enemy borrows a greeting. A
    /// mis-filed entry costs the line, it does not put words in a mouth.
    #[test]
    fn neither_side_borrows_the_other_line() {
        let enemy_only = PersonalityDef {
            feedback: vec![entry_to(
                FeedbackEvent::StartGame,
                Some("SOLTG1"),
                FeedbackAudience::ENEMIES,
            )],
            ..PersonalityDef::default()
        };
        assert!(
            choose_entry(
                &enemy_only,
                FeedbackEvent::StartGame,
                7,
                Relation::Ally,
                |_| true,
                None,
            )
            .is_none()
        );
        let ally_only = PersonalityDef {
            feedback: vec![entry_to(
                FeedbackEvent::StartGame,
                Some("SOLGR1"),
                FeedbackAudience::ALLIES,
            )],
            ..PersonalityDef::default()
        };
        assert!(
            choose_entry(
                &ally_only,
                FeedbackEvent::StartGame,
                7,
                Relation::Enemy,
                |_| true,
                None,
            )
            .is_none()
        );
        // `To=` written as `ALL`, or left out, belongs to both sides.
        let either = PersonalityDef {
            feedback: vec![entry(FeedbackEvent::StartGame, Some("ANYGR1"))],
            ..PersonalityDef::default()
        };
        for relation in [Relation::Ally, Relation::Enemy] {
            assert_eq!(
                choose_entry(
                    &either,
                    FeedbackEvent::StartGame,
                    7,
                    relation,
                    |_| true,
                    None
                )
                .unwrap()
                .video
                .as_deref(),
                Some("ANYGR1")
            );
        }
    }

    /// Meeting a commander is seeing *anything* of theirs. A building or a
    /// defence tower counts exactly like a tank: it is usually what a scout
    /// meets first, and it must not be filtered out again.
    #[test]
    fn every_class_of_object_is_a_first_contact() {
        let local = InternedId::from_index(0);
        let enemy = InternedId::from_index(1);
        for category in [
            EntityCategory::Structure,
            EntityCategory::Unit,
            EntityCategory::Infantry,
            EntityCategory::Aircraft,
        ] {
            let spotted = contacts(
                std::iter::once(Sighting {
                    owner: enemy,
                    category,
                    rx: 4,
                    ry: 9,
                }),
                local,
                |rx, ry| (rx, ry) == (4, 9),
            );
            assert_eq!(
                spotted.into_iter().collect::<Vec<_>>(),
                vec![enemy],
                "{category:?} in sight is a meeting"
            );
        }
    }

    /// In sight means in sight: shroud or a hostile gap generator keeps the
    /// house unmet, and the local player's own objects are never a meeting.
    #[test]
    fn contact_needs_sight_and_a_foreign_owner() {
        let local = InternedId::from_index(0);
        let enemy = InternedId::from_index(1);
        let sighting = |owner, rx| Sighting {
            owner,
            category: EntityCategory::Structure,
            rx,
            ry: 0,
        };
        let spotted = contacts(
            [
                sighting(enemy, 5), // hidden cell
                sighting(local, 1), // own base
            ]
            .into_iter(),
            local,
            |rx, _| rx == 1,
        );
        assert!(spotted.is_empty());
        // One visible object is enough, however many others are hidden.
        let spotted = contacts(
            [sighting(enemy, 5), sighting(enemy, 1)].into_iter(),
            local,
            |rx, _| rx == 1,
        );
        assert_eq!(spotted.into_iter().collect::<Vec<_>>(), vec![enemy]);
    }

    /// The friend/foe split of the six moments, in one table. It is a gate: a
    /// moment the speaker has no business in is never played, whatever the data
    /// says, so an opponent cannot beg the player for help and a friend cannot
    /// taunt them on contact. The two losing moments are open to both sides —
    /// whoever was wiped out is the one who says it.
    #[test]
    fn the_moment_decides_who_may_speak() {
        let allies = [FeedbackEvent::StartGame, FeedbackEvent::InTrouble];
        let enemies = [FeedbackEvent::FirstSeen, FeedbackEvent::DefeatedLocalPlayer];
        let either = [FeedbackEvent::DefeatedByLocalPlayer, FeedbackEvent::Lost];
        for event in allies {
            assert!(
                speaks_to(event, Relation::Ally),
                "{event:?} is for a friend"
            );
            assert!(
                !speaks_to(event, Relation::Enemy),
                "{event:?} is not an enemy's"
            );
        }
        for event in enemies {
            assert!(
                speaks_to(event, Relation::Enemy),
                "{event:?} is for an opponent"
            );
            assert!(
                !speaks_to(event, Relation::Ally),
                "{event:?} is not a friend's"
            );
        }
        for event in either {
            assert!(
                speaks_to(event, Relation::Ally) && speaks_to(event, Relation::Enemy),
                "{event:?} is the defeated house's own line"
            );
        }
        // Every event in the enum is covered, so a new one cannot be forgotten.
        assert_eq!(allies.len() + enemies.len() + either.len(), 6);
    }

    /// An entry whose clip is missing is never chosen, whichever side asks.
    #[test]
    fn a_missing_clip_is_skipped_for_every_audience() {
        let definition = PersonalityDef {
            feedback: vec![entry_to(
                FeedbackEvent::FirstSeen,
                Some("AWATG1"),
                FeedbackAudience::ENEMIES,
            )],
            ..PersonalityDef::default()
        };
        assert!(
            choose_entry(
                &definition,
                FeedbackEvent::FirstSeen,
                0,
                Relation::Enemy,
                |_| false,
                None
            )
            .is_none()
        );
    }
}
