//! sim-bench's match scenario: the middle of a multiplayer match.
//!
//! The map loads as a Battle against [`COMPUTERS`] computer houses (the most
//! the skirmish lobby allows; `--computers` takes fewer), and every other
//! start waypoint gets a human player's house, as in a network game. The
//! loader's own local house sits the match out (see [`MatchScenario::setup`]).
//! Every playing house stands at its start with a finished base for its
//! country, whose refineries' harvesters work the nearest ore, and a mixed army
//! of its country's infantry, vehicles and aircraft on the way to its nearest
//! enemy.
//!
//! A computer house's own AI plays it: production, base planning, teams and
//! their scripts. The script plays every human house the way a person would:
//!
//! - The army attacks in squads of [`SQUAD`], each squad one of the
//!   [`FRONTS`] nearest houses, and every [`REORDER_EVERY`] ticks each squad
//!   is ordered on toward the same house until that house is defeated. One
//!   object in [`HOME_GUARD`] stays home.
//! - Every [`FOCUS_EVERY`] ticks one squad is ordered onto a building of the
//!   house it attacks, defenses first. Every [`REORDER_EVERY`] ticks the
//!   aircraft strike a building of the nearest house.
//! - The factories queue an infantryman, a vehicle and, with an airfield, an
//!   aircraft at a steady rate, paid from the house's credits and harvest.
//!   Every [`REINFORCE_EVERY`] ticks the newly built objects follow the army.
//!
//! The houses are all at war (free-for-all), so objects acquire and fight
//! each other, and a house whose base falls is defeated and blown up as in a
//! real match. Every choice depends only on the simulation state and the
//! tick, in integer math, so a run is as deterministic as the simulation.

use std::collections::BTreeSet;

use vera20k_core::headless_scenario::{HeadlessScenario, battle_session};
use vera20k_core::map::entities::EntityCategory;
use vera20k_core::rules::foundation::foundation_dimensions;
use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::ai_buildable::{CountryBuildMasks, candidate_allowed};
use vera20k_core::sim::combat::TargetKind;
use vera20k_core::sim::command::{Command, CommandEnvelope};
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::production::ProductionCategory;
use vera20k_core::sim::runtime::SimRuntime;
use vera20k_core::skirmish_launch::{AiDifficulty, LaunchCountry, SkirmishLaunchSession};

/// Objects in a squad.
const SQUAD: usize = 30;
/// One army object in this many stays home.
const HOME_GUARD: usize = 10;
/// The first squads leave over this many ticks.
const LAUNCH_SPREAD: u64 = 60;
const REORDER_EVERY: u64 = 225;
const FOCUS_EVERY: u64 = 60;
const REINFORCE_EVERY: u64 = 150;
const INFANTRY_EVERY: u64 = 30;
const VEHICLE_EVERY: u64 = 45;
const AIRCRAFT_EVERY: u64 = 225;
/// A factory's queue holds at most this many waiting objects, as a player keeps it.
const QUEUE_DEPTH: usize = 4;
/// Twice a multiplayer game's default 10,000: a mid-game bank.
const CREDITS: i32 = 20_000;
const TECH_LEVEL: i32 = 10;
/// The army stands one part in this of the way from its start to its first
/// front's, so that neighbours' armies do not touch.
const STAGING_DIVISOR: i32 = 5;
/// How far from its anchor a building may stand, in cells.
const BASE_REACH: i32 = 24;
/// How far from the staging cell the army may stand, in cells.
const ARMY_REACH: i32 = 48;
/// How far from its start a house looks for its ore, in cells.
const ORE_REACH: i32 = 40;
/// Cells kept clear around a computer house's start, where its MCV deploys.
const DEPLOY_PAD: i32 = 3;
/// Houses a house attacks: its nearest living ones.
const FRONTS: usize = 3;
/// Computer houses by default: the seven the skirmish lobby allows.
pub(crate) const COMPUTERS: usize = 7;
/// The default run, two minutes of game time, in which the armies reach the
/// nearest bases.
pub(crate) const TICKS: u64 = 1800;
/// The launch and the first contacts, before the battle settles.
pub(crate) const WARMUP: u64 = 150;

/// The computer houses in launch order: their countries and difficulties.
const COMPUTER_ROSTER: [(LaunchCountry, AiDifficulty); COMPUTERS] = [
    (LaunchCountry::Russia, AiDifficulty::Hard),
    (LaunchCountry::France, AiDifficulty::Normal),
    (LaunchCountry::Yuri, AiDifficulty::Easy),
    (LaunchCountry::Iraq, AiDifficulty::Hard),
    (LaunchCountry::Korea, AiDifficulty::Normal),
    (LaunchCountry::Cuba, AiDifficulty::Easy),
    (LaunchCountry::GreatBritain, AiDifficulty::Hard),
];

/// The human houses' countries, in a fixed rotation over the ten.
const HUMAN_COUNTRIES: [LaunchCountry; 10] = [
    LaunchCountry::America,
    LaunchCountry::Russia,
    LaunchCountry::Yuri,
    LaunchCountry::Korea,
    LaunchCountry::Iraq,
    LaunchCountry::France,
    LaunchCountry::Cuba,
    LaunchCountry::Germany,
    LaunchCountry::Libya,
    LaunchCountry::GreatBritain,
];

/// One side's base, army and build lists, by retail type name. The lists
/// also hold the side's country-only types; a house keeps what its country
/// may own (see [`Arsenal`]).
struct Side {
    /// Built outward from the start in this order, the construction yard first.
    base: &'static [&'static str],
    /// Refineries stand by the ore and defenses toward the front.
    refinery: &'static str,
    defenses: &'static [&'static str],
    barracks: &'static str,
    war_factory: &'static str,
    /// The airfields, whose four docks each hold one of the first ownable `planes`.
    airfields: &'static [&'static str],
    planes: &'static [&'static str],
    /// A second harvester for each refinery, beside its free one.
    harvester: Option<&'static str>,
    /// The army's mix, by weight.
    army: &'static [(&'static str, u32)],
    /// One each: the heroes a side may build only once.
    heroes: &'static [&'static str],
    infantry: &'static [&'static str],
    vehicles: &'static [&'static str],
}

static ALLIED: Side = Side {
    base: &[
        "GACNST", "GAPOWR", "GAPOWR", "GAPILE", "GAREFN", "GAWEAP", "GAPOWR", "GAREFN", "GAAIRC",
        "AMRADR", "GADEPT", "GATECH", "GAPOWR", "GAPILL", "GAPILL", "NASAM", "NASAM", "ATESLA",
        "ATESLA", "GTGCAN",
    ],
    refinery: "GAREFN",
    defenses: &["GAPILL", "NASAM", "ATESLA", "GTGCAN"],
    barracks: "GAPILE",
    war_factory: "GAWEAP",
    airfields: &["GAAIRC", "AMRADR"],
    planes: &["ORCA", "BEAG"],
    harvester: Some("CMIN"),
    army: &[
        ("E1", 30),
        ("GGI", 8),
        ("ADOG", 2),
        ("ENGINEER", 1),
        ("JUMPJET", 5),
        ("CLEG", 2),
        ("SNIPE", 3),
        ("MTNK", 24),
        ("FV", 12),
        ("SREF", 6),
        ("MGTK", 4),
        ("TNKD", 6),
    ],
    heroes: &["TANY"],
    infantry: &["E1", "E1", "GGI", "E1", "JUMPJET", "ADOG", "SNIPE"],
    vehicles: &["MTNK", "FV", "MTNK", "SREF", "MGTK", "TNKD"],
};

static SOVIET: Side = Side {
    base: &[
        "NACNST", "NAPOWR", "NAPOWR", "NAHAND", "NAREFN", "NAWEAP", "NAPOWR", "NAREFN", "NARADR",
        "NADEPT", "NATECH", "NAPOWR", "NALASR", "NALASR", "NAFLAK", "NAFLAK", "TESLA", "TESLA",
    ],
    refinery: "NAREFN",
    defenses: &["NALASR", "NAFLAK", "TESLA"],
    barracks: "NAHAND",
    war_factory: "NAWEAP",
    airfields: &[],
    planes: &[],
    harvester: Some("HARV"),
    army: &[
        ("E2", 30),
        ("FLAKT", 8),
        ("DOG", 2),
        ("SENGINEER", 1),
        ("SHK", 5),
        ("IVAN", 2),
        ("DESO", 3),
        ("TERROR", 3),
        ("HTNK", 24),
        ("HTK", 8),
        ("V3", 4),
        ("DRON", 5),
        ("APOC", 5),
        ("TTNK", 6),
        ("DTRUCK", 2),
        ("SCHP", 3),
        ("ZEP", 1),
    ],
    heroes: &["BORIS"],
    infantry: &["E2", "E2", "FLAKT", "E2", "SHK", "DOG", "DESO", "TERROR"],
    vehicles: &[
        "HTNK", "HTK", "HTNK", "V3", "DRON", "APOC", "TTNK", "DTRUCK",
    ],
};

static YURI: Side = Side {
    base: &[
        "YACNST", "YAPOWR", "YAPOWR", "YABRCK", "YAREFN", "YAWEAP", "YAPOWR", "YAREFN", "NAPSIS",
        "YAGRND", "YATECH", "YAPOWR", "YAGGUN", "YAGGUN", "YAGGUN", "YAPSYT",
    ],
    refinery: "YAREFN",
    defenses: &["YAGGUN", "YAPSYT"],
    barracks: "YABRCK",
    war_factory: "YAWEAP",
    airfields: &[],
    planes: &[],
    harvester: None,
    army: &[
        ("INIT", 30),
        ("BRUTE", 10),
        ("YENGINEER", 1),
        ("VIRUS", 3),
        ("YURI", 4),
        ("LTNK", 26),
        ("YTNK", 10),
        ("CAOS", 4),
        ("TELE", 4),
        ("MIND", 2),
        ("DISK", 6),
    ],
    heroes: &["YURIPR"],
    infantry: &["INIT", "INIT", "BRUTE", "INIT", "VIRUS", "YURI"],
    vehicles: &["LTNK", "YTNK", "LTNK", "CAOS", "TELE", "DISK"],
};

/// A Yuri refinery's slaves gather ore; the script never orders them.
const SLAVE: &str = "SLAV";

/// What one house builds and fields: its side's lists, less what its country
/// may not own, asked as `HouseClass::FirstBuildableFromArray` asks it.
struct Arsenal {
    side: &'static Side,
    base: Vec<&'static str>,
    airfields: Vec<&'static str>,
    plane: Option<&'static str>,
    harvester: Option<&'static str>,
    army: Vec<(&'static str, u32)>,
    heroes: Vec<&'static str>,
    infantry: Vec<&'static str>,
    vehicles: Vec<&'static str>,
}

impl Arsenal {
    fn new(rules: &RuleSet, side_index: i32, country: &str, super_weapons: bool) -> Self {
        let side = *[&ALLIED, &SOVIET, &YURI]
            .get(usize::try_from(side_index).expect("benchmark requires a nonnegative stock side"))
            .expect("benchmark requires a stock side");
        let masks = CountryBuildMasks::for_country(rules, country);
        let ownable = |type_id: &str| {
            rules.object(type_id).is_some_and(|object| {
                candidate_allowed(object, masks, side_index, super_weapons, rules)
            })
        };
        let keep = |types: &[&'static str]| -> Vec<&'static str> {
            types
                .iter()
                .copied()
                .filter(|&type_id| ownable(type_id))
                .collect()
        };
        Self {
            side,
            base: keep(side.base),
            airfields: keep(side.airfields),
            plane: side.planes.iter().copied().find(|&plane| ownable(plane)),
            harvester: side.harvester.filter(|&harvester| ownable(harvester)),
            army: side
                .army
                .iter()
                .copied()
                .filter(|&(type_id, _)| ownable(type_id))
                .collect(),
            heroes: keep(side.heroes),
            infantry: keep(side.infantry),
            vehicles: keep(side.vehicles),
        }
    }
}

struct Squad {
    /// Each object and its place in the formation, relative to the staging cell.
    members: Vec<(u64, (i32, i32))>,
    /// The house it attacks: an index into the script's houses.
    target: usize,
    departs: u64,
}

struct House {
    id: InternedId,
    name: String,
    /// The house's own AI plays it; the script only gives it its start.
    computer: bool,
    arsenal: Arsenal,
    home: (u16, u16),
    staging: (u16, u16),
    /// The other playing houses, nearest first.
    fronts: Vec<usize>,
    /// Buildings in base order, with their type names.
    buildings: Vec<(u64, &'static str)>,
    squads: Vec<Squad>,
    /// The aircraft, which strike buildings instead of marching.
    aircraft: Vec<u64>,
    /// Every object of this house the script has seen.
    known: BTreeSet<u64>,
    infantry: Vec<InternedId>,
    vehicles: Vec<InternedId>,
    plane: Option<InternedId>,
    queued: [usize; 2],
    next_focus: usize,
}

/// The houses the script plays and what it has done.
pub(crate) struct MatchScenario {
    houses: Vec<House>,
    pub(crate) spawned: usize,
    reinforcements: usize,
}

/// A 512x512 cell bitmap, gamemd's cell array.
struct CellSet(Vec<bool>);

impl CellSet {
    fn new() -> Self {
        Self(vec![false; 512 * 512])
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        (0..512).contains(&x) && (0..512).contains(&y) && self.0[y as usize * 512 + x as usize]
    }
    fn insert(&mut self, x: i32, y: i32) {
        if (0..512).contains(&x) && (0..512).contains(&y) {
            self.0[y as usize * 512 + x as usize] = true;
        }
    }
}

/// The map's cells, its ore and the cells the bases claim.
struct Ground {
    cells: CellSet,
    ore: CellSet,
    reserved: CellSet,
}

impl Ground {
    /// A map cell with no ore that no base claims.
    fn open(&self, x: i32, y: i32) -> bool {
        self.cells.contains(x, y) && !self.ore.contains(x, y) && !self.reserved.contains(x, y)
    }

    /// Claim the square of cells within `reach` of `centre` on both axes.
    fn reserve(&mut self, centre: (u16, u16), reach: i32) {
        for y in i32::from(centre.1) - reach..=i32::from(centre.1) + reach {
            for x in i32::from(centre.0) - reach..=i32::from(centre.0) + reach {
                self.reserved.insert(x, y);
            }
        }
    }
}

/// Offsets within `reach` of the origin, nearest first; ties by row, then column.
fn spiral(reach: i32) -> Vec<(i32, i32)> {
    let mut offsets: Vec<(i32, i32)> = (-reach..=reach)
        .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| dx * dx + dy * dy <= reach * reach)
        .collect();
    offsets.sort_by_key(|&(dx, dy)| (dx * dx + dy * dy, dy, dx));
    offsets
}

/// `count` types drawn from `mix` by smooth weighted round robin, so every
/// stretch of the list holds the mix.
fn army_types(mix: &[(&'static str, u32)], count: usize) -> Vec<&'static str> {
    let total: i64 = mix.iter().map(|&(_, weight)| i64::from(weight)).sum();
    let mut credit = vec![0i64; mix.len()];
    (0..if mix.is_empty() { 0 } else { count })
        .map(|_| {
            for (credit, &(_, weight)) in credit.iter_mut().zip(mix) {
                *credit += i64::from(weight);
            }
            let pick = (0..mix.len())
                .max_by_key(|&i| (credit[i], std::cmp::Reverse(i)))
                .unwrap();
            credit[pick] -= total;
            mix[pick].0
        })
        .collect()
}

fn offset_cell(cell: (u16, u16), offset: (i32, i32)) -> (u16, u16) {
    (
        (i32::from(cell.0) + offset.0).clamp(0, 511) as u16,
        (i32::from(cell.1) + offset.1).clamp(0, 511) as u16,
    )
}

fn squared_distance(a: (u16, u16), b: (u16, u16)) -> i64 {
    let (dx, dy) = (
        i64::from(a.0) - i64::from(b.0),
        i64::from(a.1) - i64::from(b.1),
    );
    dx * dx + dy * dy
}

/// Place `type_id` as a finished building with its foundation and a one-cell
/// lane around it on open ground, as near `anchor` as the map allows.
fn place_building(
    runtime: &mut SimRuntime,
    ground: &mut Ground,
    offsets: &[(i32, i32)],
    type_id: &str,
    owner: &str,
    anchor: (u16, u16),
) -> Option<u64> {
    let (width, height) = runtime
        .resources
        .rules
        .object(type_id)
        .map(|object| foundation_dimensions(&object.foundation))?;
    let (width, height) = (i32::from(width), i32::from(height));
    for &(dx, dy) in offsets {
        let x = i32::from(anchor.0) + dx - width / 2;
        let y = i32::from(anchor.1) + dy - height / 2;
        let lane_open =
            (y - 1..=y + height).all(|cy| (x - 1..=x + width).all(|cx| ground.open(cx, cy)));
        if !lane_open {
            continue;
        }
        let SimRuntime {
            simulation,
            resources,
        } = &mut *runtime;
        if let Some(id) =
            simulation.spawn_object(type_id, owner, x as u16, y as u16, 0, &resources.rules)
        {
            for cy in y - 1..=y + height {
                for cx in x - 1..=x + width {
                    ground.reserved.insert(cx, cy);
                }
            }
            return Some(id);
        }
    }
    None
}

/// Spawn `type_id` on the first open cell of `offsets` around `centre`.
fn place_mobile(
    runtime: &mut SimRuntime,
    ground: &Ground,
    offsets: &[(i32, i32)],
    next: &mut usize,
    type_id: &str,
    owner: &str,
    centre: (u16, u16),
) -> Option<(u64, (i32, i32))> {
    while let Some(&(dx, dy)) = offsets.get(*next) {
        *next += 1;
        let (x, y) = (i32::from(centre.0) + dx, i32::from(centre.1) + dy);
        if !ground.open(x, y) {
            continue;
        }
        let SimRuntime {
            simulation,
            resources,
        } = &mut *runtime;
        if let Some(id) =
            simulation.spawn_object(type_id, owner, x as u16, y as u16, 0, &resources.rules)
        {
            return Some((id, (dx, dy)));
        }
    }
    None
}

/// The launch for a match on `map` against `computers` computer houses: the
/// match's credits and tech level, and no starting units beside the MCVs.
pub(crate) fn session(map: &str, computers: usize) -> SkirmishLaunchSession {
    let mut session = battle_session(map, &COMPUTER_ROSTER[..computers]);
    session.options.starting_credits = CREDITS;
    session.options.tech_level = TECH_LEVEL;
    session.options.unit_count = 0;
    session
}

impl MatchScenario {
    /// Find the launch's computer houses at their starts, add human houses at
    /// the start waypoints the launch left free, in order, until `houses`
    /// play (by default, every start), and give every playing house its base
    /// and its share of `units` army objects.
    ///
    /// The launch's local house, the current one, keeps its MCV at its start.
    /// Its losing would end the match, so it sits the match out as a passive
    /// house.
    pub(crate) fn setup(
        scenario: &mut HeadlessScenario,
        houses: Option<usize>,
        units: usize,
    ) -> Result<Self, String> {
        let starts: Vec<(u16, u16)> = (0..)
            .map_while(|index| scenario.map.waypoints.get(&index))
            .map(|waypoint| (waypoint.rx, waypoint.ry))
            .collect();
        let sim = &mut scenario.runtime.simulation;
        let mut taken = Vec::new();
        if let Some(local) = sim.session().current_house()
            && sim.make_house_multiplay_passive(local)
        {
            taken.extend(
                sim.houses()
                    .get(&local)
                    .and_then(|house| house.base_center()),
            );
        }
        let computers: Vec<(InternedId, (u16, u16))> = sim
            .houses()
            .order()
            .iter()
            .filter_map(|&id| {
                let house = sim.houses().get(&id)?;
                (!house.is_human() && !house.multiplay_passive())
                    .then_some((id, house.base_center()?))
            })
            .collect();
        taken.extend(computers.iter().map(|&(_, home)| home));
        let free: Vec<(u16, u16)> = starts
            .iter()
            .copied()
            .filter(|start| !taken.contains(start))
            .collect();
        let playing = houses.unwrap_or(computers.len() + free.len());
        if playing < computers.len().max(2) || playing - computers.len() > free.len() {
            return Err(format!(
                "{playing} houses on {} start waypoints: the launch took {} for itself and {} \
                 computer houses, and a match needs two houses",
                starts.len(),
                taken.len(),
                computers.len()
            ));
        }
        let humans = playing - computers.len();

        let mut players: Vec<(InternedId, (u16, u16), bool)> = computers
            .iter()
            .map(|&(id, home)| (id, home, true))
            .collect();
        for (index, &home) in free.iter().take(humans).enumerate() {
            let country = HUMAN_COUNTRIES[index % HUMAN_COUNTRIES.len()];
            let id = scenario.add_human_house(&format!("Bench{index:02}"), country);
            players.push((id, home, false));
        }

        let runtime = &mut scenario.runtime;
        let mut ground = Ground {
            cells: CellSet::new(),
            ore: CellSet::new(),
            reserved: CellSet::new(),
        };
        let sim = &runtime.simulation;
        for terrain in sim.resolved_terrain().iter() {
            for cell in terrain.iter() {
                let (x, y) = (i32::from(cell.rx), i32::from(cell.ry));
                ground.cells.insert(x, y);
                let ore = sim
                    .overlay_grid()
                    .and_then(|grid| grid.cell(cell.rx, cell.ry).overlay_id)
                    .and_then(|id| runtime.resources.overlay_registry.flags(id))
                    .is_some_and(|flags| flags.tiberium);
                if ore {
                    ground.ore.insert(x, y);
                }
            }
        }
        let base_offsets = spiral(BASE_REACH);
        let army_offsets = spiral(ARMY_REACH);
        let ore_offsets = spiral(ORE_REACH);

        let homes: Vec<(u16, u16)> = players.iter().map(|&(_, home, _)| home).collect();
        let super_weapons = runtime.simulation.session().game_options().super_weapons;
        let mut houses = Vec::with_capacity(players.len());
        for (index, &(id, home, computer)) in players.iter().enumerate() {
            let sim = &mut runtime.simulation;
            let state = &sim.houses()[&id];
            let side_index = state.side_index();
            let country = state
                .country()
                .map(|country| sim.interner().resolve(country).to_string())
                .unwrap_or_default();
            let arsenal = Arsenal::new(
                &runtime.resources.rules,
                side_index,
                &country,
                super_weapons,
            );
            let mut fronts: Vec<usize> = (0..homes.len()).filter(|&other| other != index).collect();
            fronts.sort_by_key(|&other| (squared_distance(homes[other], home), other));
            let toward = homes[fronts[0]];
            let raw = (
                i32::from(home.0) + (i32::from(toward.0) - i32::from(home.0)) / STAGING_DIVISOR,
                i32::from(home.1) + (i32::from(toward.1) - i32::from(home.1)) / STAGING_DIVISOR,
            );
            let staging = army_offsets
                .iter()
                .map(|&(dx, dy)| (raw.0 + dx, raw.1 + dy))
                .find(|&(x, y)| ground.open(x, y))
                .map_or(home, |(x, y)| (x as u16, y as u16));
            let mut intern = |types: &[&'static str]| -> Vec<InternedId> {
                types.iter().map(|name| sim.intern(name)).collect()
            };
            let infantry = intern(&arsenal.infantry);
            let vehicles = intern(&arsenal.vehicles);
            let plane = intern(arsenal.plane.as_slice()).first().copied();
            houses.push(House {
                id,
                name: sim.interner().resolve(id).to_string(),
                computer,
                arsenal,
                home,
                staging,
                fronts,
                buildings: Vec::new(),
                squads: Vec::new(),
                aircraft: Vec::new(),
                known: BTreeSet::new(),
                infantry,
                vehicles,
                plane,
                queued: [0; 2],
                next_focus: 0,
            });
        }

        let mut spawned = 0;
        // A computer house deploys its own MCV at its start.
        for house in houses.iter().filter(|house| house.computer) {
            ground.reserve(house.home, DEPLOY_PAD);
        }
        for house in houses.iter_mut() {
            let owner = house.name.clone();
            let side = house.arsenal.side;
            let toward_ore = ore_offsets
                .iter()
                .map(|&offset| offset_cell(house.home, offset))
                .find(|&(x, y)| ground.ore.contains(i32::from(x), i32::from(y)))
                .unwrap_or(house.home);
            let front = offset_cell(
                house.home,
                (
                    (i32::from(house.staging.0) - i32::from(house.home.0)) * 3 / 5,
                    (i32::from(house.staging.1) - i32::from(house.home.1)) * 3 / 5,
                ),
            );
            // The construction yard heads the list; a computer house's comes
            // from its MCV.
            let base = &house.arsenal.base[usize::from(house.computer)..];
            for &type_id in base {
                let anchor = if type_id == side.refinery {
                    toward_ore
                } else if side.defenses.contains(&type_id) {
                    front
                } else {
                    house.home
                };
                match place_building(runtime, &mut ground, &base_offsets, type_id, &owner, anchor) {
                    Some(id) => {
                        house.buildings.push((id, type_id));
                        house.known.insert(id);
                    }
                    None => eprintln!("house {owner}: no room for {type_id}"),
                }
            }
            spawned += house.buildings.len();
            // Four aircraft to an airfield, and a second harvester per refinery.
            let extras: Vec<(&'static str, (u16, u16), usize)> = house
                .buildings
                .iter()
                .filter_map(|&(_, type_id)| {
                    if house.arsenal.airfields.contains(&type_id) {
                        house.arsenal.plane.map(|plane| (plane, house.home, 4))
                    } else if type_id == side.refinery {
                        house
                            .arsenal
                            .harvester
                            .map(|harvester| (harvester, toward_ore, 1))
                    } else {
                        None
                    }
                })
                .collect();
            for (type_id, centre, count) in extras {
                let mut next = 0;
                for _ in 0..count {
                    let Some((id, _)) = place_mobile(
                        runtime,
                        &ground,
                        &army_offsets,
                        &mut next,
                        type_id,
                        &owner,
                        centre,
                    ) else {
                        continue;
                    };
                    house.known.insert(id);
                    let aircraft = runtime
                        .simulation
                        .entities()
                        .get(id)
                        .is_some_and(|entity| entity.category() == EntityCategory::Aircraft);
                    if aircraft {
                        house.aircraft.push(id);
                    }
                    spawned += 1;
                }
            }
        }

        // The armies, a house at a time, each as near its staging cell as
        // the other armies and the bases leave room for.
        let quota = units / houses.len();
        let count = houses.len();
        for (index, house) in houses.iter_mut().enumerate() {
            let owner = house.name.clone();
            let share = quota + usize::from(index < units % count);
            let heroes = house.arsenal.heroes.len().min(share);
            let mut types = army_types(&house.arsenal.army, share - heroes);
            types.extend(house.arsenal.heroes.iter().copied().take(heroes));
            let mut next = 0;
            let mut army = Vec::with_capacity(types.len());
            for type_id in types {
                match place_mobile(
                    runtime,
                    &ground,
                    &army_offsets,
                    &mut next,
                    type_id,
                    &owner,
                    house.staging,
                ) {
                    Some(member) => army.push(member),
                    None => {
                        eprintln!("house {owner}: no room for {type_id}");
                        break;
                    }
                }
            }
            spawned += army.len();
            house.known.extend(army.iter().map(|&(id, _)| id));
            if house.computer {
                continue;
            }
            let entities = runtime.simulation.entities();
            let (aircraft, marching): (Vec<_>, Vec<_>) = army.into_iter().partition(|&(id, _)| {
                entities
                    .get(id)
                    .is_some_and(|entity| entity.category() == EntityCategory::Aircraft)
            });
            house.aircraft.extend(aircraft.iter().map(|&(id, _)| id));
            let marching: Vec<_> = marching
                .into_iter()
                .enumerate()
                .filter(|(position, _)| position % HOME_GUARD != HOME_GUARD - 1)
                .map(|(_, member)| member)
                .collect();
            let fronts = house.fronts.len().min(FRONTS);
            house.squads = marching
                .chunks(SQUAD)
                .enumerate()
                .map(|(squad, members)| Squad {
                    members: members.to_vec(),
                    target: house.fronts[squad % fronts],
                    departs: 1 + (index as u64 * 7 + squad as u64 * 3) % LAUNCH_SPREAD,
                })
                .collect();
        }
        runtime
            .simulation
            .resolve_type_handles(&runtime.resources.rules);
        Ok(Self {
            houses,
            spawned,
            reinforcements: 0,
        })
    }

    pub(crate) fn houses(&self) -> usize {
        self.houses.len()
    }

    pub(crate) fn computers(&self) -> usize {
        self.houses.iter().filter(|house| house.computer).count()
    }

    fn defeated(runtime: &SimRuntime, id: InternedId) -> bool {
        runtime
            .simulation
            .houses()
            .get(&id)
            .is_none_or(|house| house.is_defeated())
    }

    /// Alive and on the map, so that it can take an order.
    fn alive(runtime: &SimRuntime, id: u64) -> bool {
        runtime
            .simulation
            .entities()
            .get(id)
            .is_some_and(|entity| entity.is_alive() && !entity.in_limbo())
    }

    /// The `nth` living house on `house`'s fronts, counting round them.
    fn front(&self, runtime: &SimRuntime, house: usize, nth: usize) -> Option<usize> {
        let living: Vec<usize> = self.houses[house]
            .fronts
            .iter()
            .copied()
            .filter(|&other| !Self::defeated(runtime, self.houses[other].id))
            .take(FRONTS)
            .collect();
        (!living.is_empty()).then(|| living[nth % living.len()])
    }

    /// The first standing building the script placed for `house`, defenses first.
    fn target_building(&self, runtime: &SimRuntime, house: usize) -> Option<u64> {
        self.houses[house]
            .buildings
            .iter()
            .rev()
            .map(|&(id, _)| id)
            .find(|&id| Self::alive(runtime, id))
    }

    fn attack_move(
        orders: &mut Vec<CommandEnvelope>,
        runtime: &SimRuntime,
        owner: InternedId,
        at: u64,
        members: &[(u64, (i32, i32))],
        destination: (u16, u16),
    ) {
        for &(id, offset) in members {
            if !Self::alive(runtime, id) {
                continue;
            }
            let (x, y) = offset_cell(destination, offset);
            orders.push(CommandEnvelope::new(
                owner,
                at,
                Command::AttackMove {
                    entity_id: id,
                    target: None,
                    destination: Some(TargetKind::Cell(x, y)),
                },
            ));
        }
    }

    fn attack(
        orders: &mut Vec<CommandEnvelope>,
        runtime: &SimRuntime,
        owner: InternedId,
        at: u64,
        attackers: impl IntoIterator<Item = u64>,
        target: u64,
    ) {
        for attacker in attackers {
            if Self::alive(runtime, attacker) {
                orders.push(CommandEnvelope::new(
                    owner,
                    at,
                    Command::Attack {
                        attacker_id: attacker,
                        target_id: target,
                    },
                ));
            }
        }
    }

    /// The commands every human house gives before frame `tick` (0-based) runs.
    pub(crate) fn orders(&mut self, runtime: &SimRuntime, tick: u64) -> Vec<CommandEnvelope> {
        let at = runtime.simulation.session().tick() + 1;
        let mut orders = Vec::new();
        for index in 0..self.houses.len() {
            let (id, stagger) = (self.houses[index].id, index as u64);
            if self.houses[index].computer || Self::defeated(runtime, id) {
                continue;
            }
            if tick == 0 {
                let house = &self.houses[index];
                let side = house.arsenal.side;
                let producers: Vec<u64> = house
                    .buildings
                    .iter()
                    .filter(|&&(_, type_id)| {
                        type_id == side.barracks
                            || type_id == side.war_factory
                            || house.arsenal.airfields.contains(&type_id)
                    })
                    .map(|&(id, _)| id)
                    .collect();
                orders.push(CommandEnvelope::new(
                    id,
                    at,
                    Command::SetRally {
                        rx: house.staging.0,
                        ry: house.staging.1,
                        producer_ids: producers,
                    },
                ));
            }

            // Squads leave, then are ordered on now and then toward the house
            // they attack; when it falls, toward one of the nearest left.
            for squad in 0..self.houses[index].squads.len() {
                let departs = self.houses[index].squads[squad].departs;
                if tick < departs || !(tick - departs).is_multiple_of(REORDER_EVERY) {
                    continue;
                }
                let mut target = self.houses[index].squads[squad].target;
                if Self::defeated(runtime, self.houses[target].id) {
                    let Some(next) = self.front(runtime, index, squad) else {
                        continue;
                    };
                    target = next;
                    self.houses[index].squads[squad].target = target;
                }
                Self::attack_move(
                    &mut orders,
                    runtime,
                    id,
                    at,
                    &self.houses[index].squads[squad].members,
                    self.houses[target].home,
                );
            }

            // Focus fire: one squad onto a building of the house it attacks.
            if tick >= LAUNCH_SPREAD && (tick + stagger * 13).is_multiple_of(FOCUS_EVERY) {
                let squads = self.houses[index].squads.len();
                for step in 0..squads {
                    let squad = (self.houses[index].next_focus + step) % squads;
                    let Squad {
                        members, target, ..
                    } = &self.houses[index].squads[squad];
                    if !members.iter().any(|&(id, _)| Self::alive(runtime, id)) {
                        continue;
                    }
                    if let Some(building) = self.target_building(runtime, *target) {
                        let attackers = members.iter().map(|&(id, _)| id);
                        Self::attack(&mut orders, runtime, id, at, attackers, building);
                    }
                    self.houses[index].next_focus = squad + 1;
                    break;
                }
            }

            // The aircraft strike a building of the nearest house.
            if tick >= LAUNCH_SPREAD
                && (tick + stagger * 17).is_multiple_of(REORDER_EVERY)
                && let Some(building) = self
                    .front(runtime, index, 0)
                    .and_then(|target| self.target_building(runtime, target))
            {
                let entities = runtime.simulation.entities();
                self.houses[index]
                    .aircraft
                    .retain(|&plane| entities.get(plane).is_some_and(|plane| plane.is_alive()));
                let planes = self.houses[index].aircraft.iter().copied();
                Self::attack(&mut orders, runtime, id, at, planes, building);
            }

            // Production, while the factory stands.
            let house = &self.houses[index];
            let standing = |type_id: &str| {
                house
                    .buildings
                    .iter()
                    .any(|&(id, built)| built == type_id && Self::alive(runtime, id))
            };
            let room = |category| {
                runtime
                    .simulation
                    .production()
                    .factories()
                    .view(id, category)
                    .is_none_or(|factory| factory.queue.len() < QUEUE_DEPTH)
            };
            let mut queue = Vec::new();
            if !house.infantry.is_empty()
                && (tick + stagger * 3).is_multiple_of(INFANTRY_EVERY)
                && standing(house.arsenal.side.barracks)
                && room(ProductionCategory::Infantry)
            {
                queue.push((0, house.infantry[house.queued[0] % house.infantry.len()]));
            }
            if !house.vehicles.is_empty()
                && (tick + stagger * 5).is_multiple_of(VEHICLE_EVERY)
                && standing(house.arsenal.side.war_factory)
                && room(ProductionCategory::Vehicle)
            {
                queue.push((1, house.vehicles[house.queued[1] % house.vehicles.len()]));
            }
            if let Some(plane) = house.plane
                && (tick + stagger * 7).is_multiple_of(AIRCRAFT_EVERY)
                && house
                    .arsenal
                    .airfields
                    .iter()
                    .any(|&airfield| standing(airfield))
                && room(ProductionCategory::Aircraft)
            {
                queue.push((2, plane));
            }
            for (list, type_id) in queue {
                orders.push(CommandEnvelope::new(
                    id,
                    at,
                    Command::QueueProduction { type_id },
                ));
                if list < 2 {
                    self.houses[index].queued[list] += 1;
                }
            }

            // Reinforcements: what the factories built (or a mind controller
            // took) since the last look and stands on the map, which a player
            // could select, follows the army as a new squad or joins the
            // aircraft.
            if tick > 0 && (tick + stagger * 11).is_multiple_of(REINFORCE_EVERY) {
                let sim = &runtime.simulation;
                let slave = sim.interner().get(SLAVE);
                let house = &self.houses[index];
                let selectable = |type_ref: InternedId| {
                    sim.interner()
                        .object_type(type_ref, &runtime.resources.rules)
                        .is_some_and(|object| object.selectable)
                };
                let fresh: Vec<(u64, bool)> = sim
                    .entities()
                    .values()
                    .filter(|entity| {
                        entity.owner() == id
                            && entity.is_alive()
                            && !entity.in_limbo()
                            && entity.category() != EntityCategory::Structure
                            && !entity.is_harvester()
                            && Some(entity.type_ref()) != slave
                            && !house.known.contains(&entity.stable_id())
                            && selectable(entity.type_ref())
                    })
                    .map(|entity| {
                        (
                            entity.stable_id(),
                            entity.category() == EntityCategory::Aircraft,
                        )
                    })
                    .collect();
                if !fresh.is_empty() {
                    self.reinforcements += fresh.len();
                    let house = &mut self.houses[index];
                    house.known.extend(fresh.iter().map(|&(id, _)| id));
                    house.aircraft.extend(
                        fresh
                            .iter()
                            .filter(|&&(_, aircraft)| aircraft)
                            .map(|&(id, _)| id),
                    );
                    let members: Vec<(u64, (i32, i32))> = fresh
                        .iter()
                        .filter(|&&(_, aircraft)| !aircraft)
                        .enumerate()
                        .map(|(position, &(id, _))| {
                            let side = 5;
                            let position = position as i32;
                            (id, (position % side - side / 2, position / side - side / 2))
                        })
                        .collect();
                    if !members.is_empty()
                        && let Some(target) = self.front(runtime, index, 0)
                    {
                        Self::attack_move(
                            &mut orders,
                            runtime,
                            id,
                            at,
                            &members,
                            self.houses[target].home,
                        );
                        self.houses[index].squads.push(Squad {
                            members,
                            target,
                            departs: tick,
                        });
                    }
                }
            }
        }
        orders
    }

    /// One line on how the match stands: what the playing houses have on the
    /// map and have done.
    pub(crate) fn progress(&self, runtime: &SimRuntime) -> String {
        let sim = &runtime.simulation;
        let ids: BTreeSet<InternedId> = self.houses.iter().map(|house| house.id).collect();
        let mut counts = [0usize; 4];
        for entity in sim.entities().values() {
            if entity.is_alive() && !entity.in_limbo() && ids.contains(&entity.owner()) {
                counts[match entity.category() {
                    EntityCategory::Infantry => 0,
                    EntityCategory::Unit => 1,
                    EntityCategory::Aircraft => 2,
                    EntityCategory::Structure => 3,
                }] += 1;
            }
        }
        let (mut credits, mut spent, mut defeated) = (0i64, 0i64, 0usize);
        // Factories building, holding a finished object that has not left yet,
        // or empty; and the objects waiting in their queues.
        let (mut factories, mut queued, mut held, mut low_power) = ([0usize; 3], 0usize, 0, 0);
        for house in &self.houses {
            if let Some(state) = sim.houses().get(&house.id) {
                credits += i64::from(state.economy().credits());
                spent += i64::from(state.economy().spent_credits());
                defeated += usize::from(state.is_defeated());
            }
            let (output, drain) = vera20k_core::sim::production::power_balance_for_owner(
                sim,
                &runtime.resources.rules,
                &house.name,
            );
            low_power += usize::from(output < drain);
            for category in [
                ProductionCategory::Infantry,
                ProductionCategory::Vehicle,
                ProductionCategory::Aircraft,
            ] {
                if let Some(factory) = sim.production().factories().view(house.id, category) {
                    queued += factory.queue.len();
                    held += usize::from(factory.on_hold || factory.suspended);
                    factories[if factory.complete_object().is_some() {
                        1
                    } else if factory.object.is_some() {
                        0
                    } else {
                        2
                    }] += 1;
                }
            }
        }
        format!(
            "infantry={} vehicles={} aircraft={} buildings={} reinforcements={} credits={credits} spent={spent} defeated={defeated} factories_building={} factories_done={} factories_idle={} factories_held={held} queued={queued} low_power={low_power}",
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            self.reinforcements,
            factories[0],
            factories[1],
            factories[2]
        )
    }
}
