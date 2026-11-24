//! Vanguard: a Rust bot that plays like a fast, tidy human. It deploys,
//! runs an economy-first opening, walls its base with the side's defenses
//! toward the enemy, keeps its factories busy with the side's best tanks (with
//! anti-air mixed in), defends its base with everything at home, and attacks
//! in growing waves the enemy nearest to it, its Construction Yard first.
//!
//! Unit and defense names are a policy table (the stock YR arsenal); the
//! bot falls back to the rules' own facts for anything it does not know.

use serde::{Deserialize, Serialize};

use super::view::{BotView, cell, dist2, is_structure};
use super::{Bot, BotRng};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::production::{BuildOption, ProductionCategory};

/// Frames between two looks.
const THINK_EVERY: u32 = 12;
/// The opening Construction Yard capture (`opening_capture`).
const CAPTURE: bool = false;
/// Engineer raids on enemy yards (off: see `raid`).
const RAIDS: bool = false;
/// Enemies this close (cells) to a base building are an attack on the base.
const BASE_ALARM: i32 = 16;
/// Units that make a wave after the opening rush.
const WAVE: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    ConYard,
    Power,
    Refinery,
    Barracks,
    WarFactory,
    Radar,
    Lab,
    Defense,
    /// An offensive superweapon (nuke, storm, dominator).
    Super,
    /// A Service Depot (`UnitRepair=yes`).
    Depot,
    /// Yuri's Grinder (`Grinding=yes`): captured units are sold in it.
    Grinder,
    Other,
}
const ROLES: usize = 12;

/// Fighting vehicles in order of preference; anti-air ones mixed in.
const TANKS: &[&str] = &["APOC", "TNKD", "TTNK", "HTNK", "MTNK", "LTNK"];
const ANTI_AIR: &[&str] = &["HTK", "FV", "YTNK"];
const AA_DEFENSES: &[&str] = &["NAFLAK", "NASAM", "YAGGUN"];
/// Yuri's Psychic Tower first: it takes the attackers over, and the
/// Grinder sells them.
const DEFENSES: &[&str] = &["TESLA", "GTGCAN", "ATESLA", "YAPSYT", "YAGGUN"];
/// Infantry that hold the base deployed (GI, Guardian GI: `Deployer=yes`,
/// `DeployFire=yes`; the Guardian GI's deployed missiles hit tanks and
/// aircraft at 8 cells).
const DEPLOYERS: &[&str] = &["GGI", "E1"];
/// Long-range units that take a base apart from outside its defenses' reach
/// (Prism Tank 10 cells, Siege Chopper deployed 12, Magnetron 10-12); the
/// rest of the army screens them.
const SIEGE: &[&str] = &["SREF", "SCHP", "TELE"];
/// Floating Discs: drain a power plant (a drained plant zeroes the whole
/// House's power, `0x00508D79`), then shoot the unpowered anti-air.
const DISC: &str = "DISK";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vanguard {
    rng: BotRng,
    offset: u32,
    /// Units sent to attack.
    army: Vec<u64>,
    last_attack: u32,
    last_deploy: u32,
    deploy_tries: u32,
    /// v42d: the only yard packed up under attack: until this frame its
    /// MCV drives away from `flee_from` instead of deploying.
    #[serde(default)]
    flee_until: u32,
    #[serde(default)]
    flee_from: (u16, u16),
    waves: u32,
    /// Engineer raids on enemy yards sent so far.
    #[serde(default)]
    raids: u32,
    #[serde(default)]
    last_raid: u32,
    /// Ore cells seen at the last scan (a sample, nearest first from the yard).
    ore: Vec<(u16, u16)>,
    ore_scan: u32,
    rally_set: Vec<u64>,
    vehicles_built: u32,
    /// Unused: buildings are repaired by `bot::repair_buildings`; kept for
    /// the save layout.
    repaired: Vec<u64>,
    /// The opening yard capture: 0 preparing, 1 gathering, 3 going in, 2 over.
    #[serde(default)]
    capture: u8,
    /// Engineers sent into the enemy yard (ordered once: a repeated order
    /// restarts the walk).
    #[serde(default)]
    sent_in: Vec<u64>,
    /// Enemy aircraft alive at this look (recomputed every look).
    #[serde(skip)]
    air_threat: usize,
    /// The most armed enemy aircraft seen at once (Kirovs come back: the
    /// anti-air stays sized for them).
    #[serde(default)]
    air_peak: usize,
    /// Own anti-air vehicles at this look.
    #[serde(skip)]
    own_aa: usize,
    /// The army's worth (credits) when it set out.
    #[serde(default)]
    launch_value: i32,
    /// Reinforcements on their way to the army in the field (REINFORCE).
    #[serde(default)]
    joining: Vec<u64>,
    /// Engineers on their way to neutral tech buildings (engineer, building).
    #[serde(default)]
    tech_targets: Vec<(u64, u64)>,
    /// Looks in a row a finished building found no site.
    #[serde(default)]
    place_fails: [u32; 2],
    /// Building types given up on (no site), with the frame until when.
    #[serde(default)]
    no_site: Vec<(InternedId, u32)>,
    /// The unit looking for the enemy base, and since when.
    #[serde(default)]
    scout: Option<(u64, u32)>,
    /// Focus fire: each unit's current target (unit, target), kept until
    /// the target dies or leaves reach.
    #[serde(default)]
    focus: Vec<(u64, u64)>,
    /// Units sent home to be repaired (unit, since frame).
    #[serde(default)]
    repairing: Vec<(u64, u32)>,
    /// Siege orders given (unit, target), so an order is not repeated.
    #[serde(default)]
    siege_orders: Vec<(u64, u64)>,
    /// The army's front (knob CREEP): the cell it holds, and since when.
    #[serde(default)]
    front: Option<((u16, u16), u32)>,
    /// Disc orders given (disc, target), so an order is not repeated.
    #[serde(default)]
    disc_orders: Vec<(u64, u64)>,
    /// Whether the discs are out on a raid.
    #[serde(default)]
    disc_raid: bool,
    /// Kirovs out on a raid, and what each was told to bomb (ZEPS).
    #[serde(default)]
    zep_raid: bool,
    #[serde(default)]
    zep_orders: Vec<(u64, u64)>,
    /// Yuri's Slave Miners on their way to ore (miner, goal, since).
    #[serde(default)]
    miner_goals: Vec<(u64, (u16, u16), u32)>,
    /// Deploy toggles given (unit, frame): one at a time, the animation
    /// takes a while.
    #[serde(default)]
    toggled: Vec<(u64, u32)>,
    /// Whether an enemy superweapon (nuke, storm, dominator) was seen.
    #[serde(default)]
    enemy_super: bool,
    /// Tesla Troopers sent to charge a coil (trooper, coil), three a coil.
    #[serde(default)]
    charging: Vec<(u64, u64)>,
    /// When the last offensive superweapon was fired.
    #[serde(default)]
    strike: u32,
    /// Captured units sent into the Grinder (unit, frame).
    #[serde(default)]
    grinding: Vec<(u64, u32)>,
    /// The crate run (`bot::CrateHunt`).
    #[serde(default)]
    crates: crate::sim::bot::CrateHunt,
    /// Dogs and a Chrono Legionnaire that scout for crates (`bot::CrateScouts`).
    #[serde(default)]
    scouts: crate::sim::bot::CrateScouts,
    /// Slave Miners driven to ore and deployed (`bot::SlaveMiners`), any
    /// side's (a crate's too).
    #[serde(default)]
    slave_miner_chore: crate::sim::bot::SlaveMiners,
    /// A Mastermind's spare captives being ground or shot
    /// (`bot::mastermind_overload`).
    #[serde(default)]
    overload: Vec<(u64, u32)>,
    /// Oil derricks and other tech buildings (`bot::TechBuildings`).
    #[serde(default)]
    tech: crate::sim::bot::TechBuildings,
    /// Neutral buildings garrisoned near home (`bot::Garrisons`).
    #[serde(default)]
    garrisons: crate::sim::bot::Garrisons,
    /// The roaming Rocketeers (`bot::RocketeerSquad`).
    #[serde(default)]
    rocketeers: crate::sim::bot::RocketeerSquad,
    /// Chrono Miners' health at the last think (`bot::tend_miners`).
    #[serde(default)]
    miner_health: Vec<(u64, i32)>,
    /// The 3x3 Prism Tank squad (`bot::PrismSquad`).
    #[serde(default)]
    prisms: crate::sim::bot::PrismSquad,
    /// Spies walking into enemy buildings (`bot::Spies`).
    #[serde(default)]
    spies: crate::sim::bot::Spies,
    /// The strike group out in the field (GROUP): 9 Apocalypse tanks
    /// (Soviet) or Prism Tanks (Allies) that go for power, then the units
    /// that hit hardest.
    #[serde(default)]
    group: Vec<u64>,
    /// The spare MCV this bot built and parks (PARK); any other MCV (a
    /// crate's, `FreeMCV=`) deploys as a further yard.
    #[serde(default)]
    parked: Option<u64>,
    /// An MCV is queued: the next new one is the bot's own spare.
    #[serde(default)]
    mcv_queued: bool,
    /// Own MCVs at the last look.
    #[serde(default)]
    mcvs_seen: Vec<u64>,
    /// What the last look decided, for spectators (not saved).
    #[serde(skip)]
    notes: Notes,
}

/// The last look's decisions, as [`Bot::status`] tells them.
#[derive(Debug, Clone, Default)]
struct Notes {
    credits: i32,
    power: (i32, i32),
    army_target: Option<(String, (u16, u16))>,
    searching: Option<(u16, u16)>,
    scouting: Option<(u16, u16)>,
    threatened: bool,
    home_value: i32,
    want: i32,
    known_enemy_buildings: usize,
    army_center: Option<(u16, u16)>,
    scout_at: Option<(u16, u16)>,
    /// Home units' middle and where they gather.
    gathering: Option<((u16, u16), (u16, u16))>,
    /// GIs deployed / held at home.
    deployed: (usize, usize),
    /// What the long-range units shoot.
    siege: Option<(String, (u16, u16))>,
    /// Mop-up: the enemy is down to a few buildings (`attack`).
    mop_up: bool,
    /// What the discs do.
    discs: Option<String>,
    repairing: usize,
    /// The crate units gather at.
    crate_run: Option<(u16, u16)>,
    /// The strike group's middle and what it shoots.
    group: Option<((u16, u16), (u16, u16))>,
}

impl Vanguard {
    pub fn new(seed: u32) -> Self {
        let mut rng = BotRng::new(seed);
        let offset = rng.below(THINK_EVERY);
        Self {
            rng,
            offset,
            army: Vec::new(),
            last_attack: 0,
            last_deploy: 0,
            deploy_tries: 0,
            flee_until: 0,
            flee_from: (0, 0),
            waves: 0,
            raids: 0,
            last_raid: 0,
            ore: Vec::new(),
            ore_scan: 0,
            rally_set: Vec::new(),
            vehicles_built: 0,
            repaired: Vec::new(),
            capture: 0,
            sent_in: Vec::new(),
            air_threat: 0,
            air_peak: 0,
            focus: Vec::new(),
            repairing: Vec::new(),
            siege_orders: Vec::new(),
            front: None,
            disc_orders: Vec::new(),
            disc_raid: false,
            zep_raid: false,
            zep_orders: Vec::new(),
            miner_goals: Vec::new(),
            toggled: Vec::new(),
            strike: 0,
            charging: Vec::new(),
            enemy_super: false,
            grinding: Vec::new(),
            own_aa: 0,
            launch_value: 0,
            joining: Vec::new(),
            tech_targets: Vec::new(),
            place_fails: [0; 2],
            no_site: Vec::new(),
            scout: None,
            crates: Default::default(),
            scouts: Default::default(),
            slave_miner_chore: Default::default(),
            overload: Vec::new(),
            tech: Default::default(),
            garrisons: Default::default(),
            rocketeers: Default::default(),
            miner_health: Vec::new(),
            prisms: Default::default(),
            spies: Default::default(),
            group: Vec::new(),
            parked: None,
            mcv_queued: false,
            mcvs_seen: Vec::new(),
            notes: Notes::default(),
        }
    }
}

fn role(view: &BotView<'_>, ty: &ObjectType) -> Role {
    if ty.wall {
        return Role::Other;
    }
    if ty.construction_yard {
        return Role::ConYard;
    }
    if ty
        .super_weapon
        .as_deref()
        .and_then(|sw| view.rules.super_weapon(sw))
        .is_some_and(|sw| offensive(sw.kind))
    {
        return Role::Super;
    }
    // Yuri's refinery is a deployed Slave Miner.
    let miner_base = ty.undeploys_into.as_deref().is_some_and(|unit| {
        view.rules
            .object(unit)
            .is_some_and(|u| u.harvester || u.resource_gatherer)
    });
    if ty.refinery || miner_base {
        return Role::Refinery;
    }
    match ty.factory_type() {
        Some(FactoryType::InfantryType) => return Role::Barracks,
        Some(FactoryType::UnitType) if !ty.naval => return Role::WarFactory,
        _ => {}
    }
    if ty.is_base_defense || ty.build_cat == Some(crate::rules::object_type::BuildCategory::Combat)
    {
        return Role::Defense;
    }
    if ty.radar {
        return Role::Radar;
    }
    if ty.unit_repair && !ty.naval {
        return Role::Depot;
    }
    if ty.grinding {
        return Role::Grinder;
    }
    if ty.power > 0 {
        return Role::Power;
    }
    if matches!(ty.id.as_str(), "GATECH" | "NATECH" | "YATECH") {
        return Role::Lab;
    }
    Role::Other
}

fn offensive(kind: SuperWeaponKind) -> bool {
    matches!(
        kind,
        SuperWeaponKind::MultiMissile
            | SuperWeaponKind::LightningStorm
            | SuperWeaponKind::PsychicDominator
    )
}

/// A building's footprint, top-left cell and size.
#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    gap: i32,
}

impl Rect {
    fn conflicts(&self, other: &Rect) -> bool {
        let gap = self.gap.max(other.gap);
        self.x < other.x + other.w + gap
            && other.x < self.x + self.w + gap
            && self.y < other.y + other.h + gap
            && other.y < self.y + self.h + gap
    }
}

/// Free cells kept around a building: lanes by factories and refineries,
/// and `spread` around the rest (two, the most `Adjacent=2` allows, once
/// the enemy has a superweapon: it then catches fewer buildings at once; a
/// wide base costs against armies, so not before).
fn gap_for(role: Role, spread: i32) -> i32 {
    match role {
        Role::WarFactory | Role::Refinery | Role::Barracks => 2,
        Role::Defense => 0,
        _ => spread,
    }
}

struct Base {
    yard: Option<(u16, u16)>,
    counts: [usize; ROLES],
    rects: Vec<Rect>,
    buildings: Vec<(u64, (u16, u16))>,
    factories: Vec<u64>,
    harvesters: usize,
    aa_defenses: usize,
    /// Refinery cells (top-left).
    refineries: Vec<(u16, u16)>,
    /// Service Depots (id, cell).
    depots: Vec<(u64, (u16, u16))>,
    /// Grinders (id, cell).
    grinders: Vec<(u64, (u16, u16))>,
    /// Free cells around ordinary buildings (`gap_for`).
    spread: i32,
    /// The middle of the base's buildings.
    center: (u16, u16),
}

impl Base {
    fn count(&self, role: Role) -> usize {
        self.counts[role as usize]
    }
}

#[derive(Clone)]
struct Unit {
    id: u64,
    at: (u16, u16),
    aircraft: bool,
    infantry: bool,
    dog: bool,
    aa: bool,
    /// A GI or Guardian GI that holds the base deployed.
    deployer: bool,
    deployed: bool,
    /// Has a destination.
    moving: bool,
    siege: bool,
    disc: bool,
    /// Health in percent of strength.
    health: i32,
    /// Taken over by a Mastermind or Psychic Tower.
    captured: bool,
    /// A Tesla Trooper: stays by a Tesla Coil and charges it.
    trooper: bool,
    /// A Rocketeer: guards the base.
    rocketeer: bool,
    /// A Yuri Prime: goes with the waves for the enemy yard (PRIMES).
    prime: bool,
}

impl Unit {
    /// Base holders that never join a wave or chase intruders.
    fn stays_home(&self) -> bool {
        self.deployer || self.trooper || self.rocketeer
    }
}

impl Bot for Vanguard {
    fn errand_units(&self) -> Vec<u64> {
        let mut out: Vec<u64> = self.scouts.ids().to_vec();
        out.extend_from_slice(self.spies.ids());
        out.extend(self.crates.runner());
        out.extend(self.repairing.iter().map(|r| r.0));
        out.extend(self.grinding.iter().map(|g| g.0));
        out.extend(self.overload.iter().map(|o| o.0));
        out.extend(self.tech.ids());
        out.extend(self.garrisons.ids());
        out.extend_from_slice(self.rocketeers.ids());
        out.extend_from_slice(self.prisms.ids());
        out
    }

    fn markers(&self) -> Vec<crate::sim::bot::BotMarker> {
        use crate::sim::bot::{BotMarker, MarkerKind};
        let n = &self.notes;
        let mut out = Vec::new();
        if let Some(from) = n.army_center.filter(|_| !self.army.is_empty()) {
            if let Some((_, to)) = &n.army_target {
                out.push(BotMarker {
                    from,
                    to: *to,
                    kind: MarkerKind::Attack,
                });
            } else if let Some(to) = n.searching {
                out.push(BotMarker {
                    from,
                    to,
                    kind: MarkerKind::Search,
                });
            }
        }
        if let Some((from, to)) = n.gathering.filter(|_| self.army.is_empty()) {
            out.push(BotMarker {
                from,
                to,
                kind: MarkerKind::Gather,
            });
        }
        if let (Some(from), Some(to)) = (n.scout_at, n.scouting) {
            out.push(BotMarker {
                from,
                to,
                kind: MarkerKind::Search,
            });
        }
        out
    }

    fn status(&self) -> Vec<String> {
        let n = &self.notes;
        let mut lines = vec![format!(
            "Vanguard: ${} power {}/{}  waves {}  enemy buildings seen {}",
            n.credits, n.power.0, n.power.1, self.waves, n.known_enemy_buildings
        )];
        lines.push(if n.threatened {
            "Base under attack: everyone at home defends".into()
        } else if self.army.is_empty() {
            format!(
                "Gathering at home: army worth {} of {} needed",
                n.home_value, n.want
            )
        } else {
            match (&n.army_target, n.searching) {
                (Some((what, at)), _) => format!(
                    "Attacking {what} at {},{} with {} units (left {}% of launch worth)",
                    at.0,
                    at.1,
                    self.army.len(),
                    if self.launch_value > 0 {
                        n.home_value * 100 / self.launch_value
                    } else {
                        0
                    }
                ),
                (None, Some(at)) => format!(
                    "No enemy in sight: {} units search at {},{}",
                    self.army.len(),
                    at.0,
                    at.1
                ),
                (None, None) => format!("Army of {} units, no target", self.army.len()),
            }
        });
        if let Some(at) = n.scouting {
            lines.push(match n.scout_at {
                Some(now) => format!(
                    "Scout at {},{} heading for start position {},{}",
                    now.0, now.1, at.0, at.1
                ),
                None => format!("Scout heading for start position {},{}", at.0, at.1),
            });
        }
        if matches!(self.capture, 1 | 3) {
            lines.push("Engineers going for the enemy Construction Yard".into());
        }
        if n.deployed.1 > 0 {
            lines.push(format!(
                "GIs holding the base: {} of {} deployed",
                n.deployed.0, n.deployed.1
            ));
        }
        if let Some((what, at)) = &n.siege {
            lines.push(format!(
                "Long-range units shell {what} at {},{} from out of its reach",
                at.0, at.1
            ));
        }
        if let Some(discs) = &n.discs {
            lines.push(discs.clone());
        }
        if n.repairing > 0 {
            lines.push(format!("{} units at the Service Depot", n.repairing));
        }
        if let Some(at) = n.crate_run {
            lines.push(format!("Units gather at the crate at {},{}", at.0, at.1));
        }
        lines
    }

    fn think(&mut self, view: &BotView<'_>, orders: &mut Vec<Command>) {
        let frame = view.frame();
        if (frame + self.offset) % THINK_EVERY != 0 {
            return;
        }
        let mut base = Base {
            yard: None,
            counts: [0; ROLES],
            rects: Vec::new(),
            buildings: Vec::new(),
            factories: Vec::new(),
            harvesters: 0,
            aa_defenses: 0,
            refineries: Vec::new(),
            depots: Vec::new(),
            grinders: Vec::new(),
            spread: 1,
            center: (0, 0),
        };
        if !self.enemy_super {
            self.enemy_super = view.enemy_objects().any(|e| {
                is_structure(e)
                    && view
                        .object_type(e.type_ref())
                        .is_some_and(|t| role(view, t) == Role::Super)
            });
        }
        base.spread = match tune::SPREAD {
            -1 => 1 + i32::from(self.enemy_super),
            n => n,
        };
        let mut mcvs = Vec::new();
        let mut yards: Vec<(u64, i32, (u16, u16))> = Vec::new();
        let mut units = Vec::new();
        self.air_threat = view
            .enemy_objects()
            .filter(|e| flies(view, e) && armed(view, e))
            .count();
        self.air_peak = self.air_peak.max(self.air_threat);
        if tune::AA_PEAK {
            self.air_threat = self.air_peak;
        }
        self.own_aa = 0;
        for entity in view.own() {
            let Some(ty) = view.object_type(entity.type_ref()) else {
                continue;
            };
            if is_structure(entity) {
                let r = role(view, ty);
                base.counts[r as usize] += 1;
                let at = cell(entity);
                if AA_DEFENSES.contains(&view.name(entity.type_ref())) {
                    base.aa_defenses += 1;
                }
                if r == Role::ConYard {
                    base.yard.get_or_insert(at);
                    yards.push((
                        entity.stable_id(),
                        entity.health.current() * 100 / ty.strength.max(1),
                        at,
                    ));
                }
                if r == Role::Refinery {
                    base.refineries.push(at);
                }
                if r == Role::Depot {
                    base.depots.push((entity.stable_id(), at));
                }
                if r == Role::Grinder {
                    base.grinders.push((entity.stable_id(), at));
                }
                let (w, h) = ty.foundation.dimensions();
                base.rects.push(Rect {
                    x: i32::from(at.0),
                    y: i32::from(at.1),
                    w: i32::from(w),
                    h: i32::from(h),
                    gap: gap_for(r, base.spread),
                });
                base.buildings.push((entity.stable_id(), at));
                if matches!(r, Role::WarFactory | Role::Barracks) {
                    base.factories.push(entity.stable_id());
                }
                continue;
            }
            if ty
                .deploys_into
                .as_deref()
                .is_some_and(|into| view.rules.object(into).is_some_and(|b| b.construction_yard))
            {
                mcvs.push(entity);
                continue;
            }
            if entity.is_harvester() || ty.harvester || ty.resource_gatherer || ty.slaved {
                base.harvesters += 1;
                continue;
            }
            if ty.weapon_list.iter().all(Option::is_none) || ty.engineer {
                continue;
            }
            let name = view.name(entity.type_ref());
            if ANTI_AIR.contains(&name) {
                self.own_aa += 1;
            }
            let siege = SIEGE.contains(&name) && tune::SIEGE;
            let disc = name == DISC && tune::DISCS > 0;
            units.push(Unit {
                id: entity.stable_id(),
                at: cell(entity),
                // A Siege Chopper is ConsideredAircraft but fights with the
                // army; a disc has its own raids.
                aircraft: flies(view, entity) && !siege,
                infantry: entity.category() == EntityCategory::Infantry,
                dog: matches!(name, "DOG" | "ADOG"),
                aa: ANTI_AIR.contains(&name),
                deployer: DEPLOYERS.contains(&name) && tune::HOLD,
                deployed: entity.is_deployed(),
                moving: entity.navigation.nav_com().is_some(),
                siege,
                disc,
                health: entity.health.current() * 100 / ty.strength.max(1),
                captured: entity.seldom.mind_control.is_mind_controlled(),
                trooper: name == "SHK" && tune::TROOPERS > 0,
                // v46 (knob CLONES): Yuri Clones guard the staging point too.
                rocketeer: name == "JUMPJET" || (name == "YURI" && tune::CLONES > 0),
                prime: name == "YURIPR",
            });
        }

        // The MCV this bot handles itself: without a yard any (it deploys),
        // with one only the spare it built (PARK). Every other one (a
        // crate's) `bot::deploy_spare_mcvs` deploys as a further yard.
        for m in &mcvs {
            if self.mcv_queued && !self.mcvs_seen.contains(&m.stable_id()) {
                self.parked = Some(m.stable_id());
                self.mcv_queued = false;
            }
        }
        self.mcvs_seen = mcvs.iter().map(|m| m.stable_id()).collect();
        let parked = self.parked;
        self.parked = parked.filter(|p| self.mcvs_seen.contains(p));
        let mut mcv = mcvs.iter().copied().find(|m| Some(m.stable_id()) == parked);
        if base.yard.is_none() {
            mcv = mcv.or(mcvs.first().copied());
        }
        // A unit still inside a building (driving out of its factory) takes
        // no orders: one would cancel the exit and leave it there for good.
        // Aircraft hover over buildings: only a factory's (a new disc
        // leaving the war factory) holds them back (a Yuri disc fleet parked
        // over its defences never counted).
        units.retain(|u| {
            let (x, y) = (i32::from(u.at.0), i32::from(u.at.1));
            !base
                .rects
                .iter()
                .zip(&base.buildings)
                .filter(|(r, _)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
                .any(|(_, (id, _))| !(u.aircraft || u.disc) || base.factories.contains(id))
        });
        if !base.buildings.is_empty() {
            let n = base.buildings.len() as i32;
            let (sx, sy) = base.buildings.iter().fold((0i32, 0i32), |(x, y), (_, at)| {
                (x + i32::from(at.0), y + i32::from(at.1))
            });
            base.center = ((sx / n) as u16, (sy / n) as u16);
        }
        // Badly damaged units go home for repair and take no other orders.
        self.repair_units(view, &base, &mut units, orders);
        let disposing =
            crate::sim::bot::mastermind_overload(view, THINK_EVERY, &mut self.overload, orders);
        units.retain(|u| !disposing.contains(&u.id));
        self.army.retain(|id| !disposing.contains(id));
        self.grind_captured(view, &base, &mut units, orders);
        self.strike_group(view, &mut units, orders);
        // v42d (knob YARD_FLEE, off: 48 games m12/m16 -0.51/-0.75 vs
        // -0.50/-0.72 without, more yard losses): the only yard, badly hurt with armed
        // enemies on it and no spare MCV, packs up and drives off: an MCV
        // can redeploy, a destroyed yard ends the building.
        if tune::YARD_FLEE
            && mcv.is_none()
            && frame >= self.flee_until
            && let &[(id, pct, at)] = yards.as_slice()
            && pct < tune::FLEE_PCT
        {
            let threat = view
                .enemy_objects()
                .filter(|e| armed(view, e) && !is_structure(e))
                .map(cell)
                .filter(|&c| dist2(c, at) <= 9 * 9)
                .min_by_key(|&c| dist2(c, at));
            if let Some(from) = threat {
                orders.push(Command::UndeployBuilding { entity_id: id });
                self.flee_until = frame + tune::FLEE_FRAMES as u32;
                self.flee_from = from;
            }
        }
        // The MCV deploys where it stands, or a little further on.
        let yard = if let Some(yard) = base.yard {
            self.second_yard(view, &base, yard, mcv, orders);
            yard
        } else {
            // v44 (knob MCV_WAIT): no deploying under the aircraft that just
            // razed the yard (Kirovs bombed the redeployed spare within a
            // minute in 8 of 48 games); drive off from them instead.
            if let Some(mcv) = mcv
                && tune::MCV_WAIT > 0
                && let Some(flyer) = view
                    .enemy_objects()
                    .filter(|e| flies(view, e) && armed(view, e))
                    .map(cell)
                    .filter(|&c| dist2(c, cell(mcv)) <= tune::MCV_WAIT.pow(2))
                    .min_by_key(|&c| dist2(c, cell(mcv)))
            {
                self.flee_until = frame + 1;
                self.flee_from = flyer;
            }
            if let Some(mcv) = mcv
                && frame < self.flee_until
            {
                if frame.saturating_sub(self.last_deploy) > 60 {
                    self.last_deploy = frame;
                    let (x, y) = cell(mcv);
                    let away = |a: u16, b: u16| -> u16 {
                        let d = (i32::from(a) - i32::from(b)).signum() * 12;
                        (i32::from(a) + d).max(2) as u16
                    };
                    orders.push(Command::Move {
                        entity_id: mcv.stable_id(),
                        target_rx: away(x, self.flee_from.0),
                        target_ry: away(y, self.flee_from.1),
                    });
                }
            } else if let Some(mcv) = mcv
                && frame.saturating_sub(self.last_deploy) > 60
            {
                self.last_deploy = frame;
                self.deploy_tries += 1;
                if self.deploy_tries % 3 == 0 {
                    let (x, y) = cell(mcv);
                    let dx = self.rng.below(9) as i32 - 4;
                    let dy = self.rng.below(9) as i32 - 4;
                    orders.push(Command::Move {
                        entity_id: mcv.stable_id(),
                        target_rx: (i32::from(x) + dx).max(1) as u16,
                        target_ry: (i32::from(y) + dy).max(1) as u16,
                    });
                } else {
                    orders.push(Command::DeployMcv {
                        entity_id: mcv.stable_id(),
                    });
                }
            }
            // A lost yard: a war factory builds a new MCV.
            if mcv.is_none() && base.count(Role::WarFactory) > 0 && frame % 90 < THINK_EVERY {
                let options = view.build_options();
                let building = view.factory(ProductionCategory::Vehicle).is_some_and(|f| {
                    f.object.is_some_and(|o| {
                        view.object_type(o.type_id)
                            .is_some_and(|t| t.deploys_into.is_some())
                    })
                });
                let pick = options.iter().find(|o| {
                    o.enabled
                        && o.queue_category == ProductionCategory::Vehicle
                        && view.object_type(o.type_id).is_some_and(|t| {
                            t.deploys_into.as_deref().is_some_and(|into| {
                                view.rules.object(into).is_some_and(|b| b.construction_yard)
                            })
                        })
                });
                if !building && let Some(pick) = pick {
                    self.mcv_queued = true;
                    orders.push(Command::QueueProduction {
                        type_id: pick.type_id,
                    });
                }
            }
            // Without a yard the factories still build and the army still
            // fights, around the middle of what is left.
            if base.buildings.is_empty() {
                self.defend(view, Some(&base), &units, orders);
                return;
            }
            base.center
        };
        if self.ore.is_empty() || frame.saturating_sub(self.ore_scan) > 1500 {
            self.scan_ore(view, yard);
            self.ore_scan = frame;
        }
        let options = view.build_options();
        if log::log_enabled!(log::Level::Debug) && frame % 900 < THINK_EVERY {
            let list: Vec<String> = options
                .iter()
                .map(|o| {
                    let r = view.object_type(o.type_id).map(|t| role(view, t));
                    format!(
                        "{}:{:?}:{}:{:?}",
                        view.name(o.type_id),
                        o.queue_category,
                        o.enabled,
                        r
                    )
                })
                .collect();
            log::debug!(
                "bot f{frame} credits={} power={:?} options {}",
                view.credits(),
                view.power(),
                list.join(" ")
            );
        }
        if log::log_enabled!(log::Level::Debug) && frame % 90 < THINK_EVERY {
            let air: Vec<String> = view
                .enemy_objects()
                .filter(|e| flies(view, e))
                .map(|e| {
                    format!(
                        "{}@{:?}d{}hp{}",
                        view.name(e.type_ref()),
                        cell(e),
                        dist2(cell(e), yard).isqrt(),
                        e.health.current()
                    )
                })
                .collect();
            if !air.is_empty() {
                log::debug!("air f{frame} {air:?}");
            }
        }
        crate::sim::bot::deploy_spare_mcvs(
            view,
            mcv.map(|m| m.stable_id()),
            yard,
            THINK_EVERY,
            orders,
        );
        self.scouts.run(view, yard, self.crates.target(), orders);
        self.spies.run(view, orders);
        let scouts = self.scouts.ids();
        let spies = self.spies.ids();
        units.retain(|u| !scouts.contains(&u.id) && !spies.contains(&u.id));
        let walkers: Vec<(u64, (u16, u16))> = units
            .iter()
            .filter(|u| !u.aircraft && !u.disc && !u.captured)
            .map(|u| (u.id, u.at))
            .collect();
        let on_crates = self.crates.run(view, yard, &walkers, orders);
        units.retain(|u| !on_crates.contains(&u.id));
        let free: Vec<(u64, (u16, u16))> = units
            .iter()
            .filter(|u| !u.aircraft && !u.disc && !u.captured && !self.army.contains(&u.id))
            .map(|u| (u.id, u.at))
            .collect();
        let on_drones = crate::sim::bot::fight_drones(view, THINK_EVERY, &free, orders);
        units.retain(|u| !on_drones.contains(&u.id));
        let free: Vec<(u64, (u16, u16))> = units
            .iter()
            .filter(|u| !u.aircraft && !u.disc && !u.captured && !self.army.contains(&u.id))
            .map(|u| (u.id, u.at))
            .collect();
        let on_tech = self
            .tech
            .run(view, yard, THINK_EVERY, &free, &options, orders);
        units.retain(|u| !on_tech.contains(&u.id));
        let free: Vec<(u64, (u16, u16))> = free
            .into_iter()
            .filter(|(id, _)| !on_tech.contains(id))
            .collect();
        let garrisoning = self.garrisons.run(view, yard, THINK_EVERY, &free, orders);
        units.retain(|u| !garrisoning.contains(&u.id));
        let keep = tune::ROCKETEERS as usize;
        let squad = self.rocketeers.run(view, yard, keep, THINK_EVERY, orders);
        units.retain(|u| !squad.contains(&u.id));
        self.prisms.produce(view, &options, orders);
        let prisms = self.prisms.run(view, yard, THINK_EVERY, orders);
        units.retain(|u| !prisms.contains(&u.id));
        self.notes.crate_run = self.crates.target();
        self.build_base(view, &base, yard, &options, orders);
        self.slave_miner_chore.run(view, yard, THINK_EVERY, orders);
        self.build_defenses(view, &base, yard, &options, orders);
        self.opening_capture(view, &base, &options, &units, orders);

        self.produce(view, &base, &options, orders);
        self.set_rally(view, &base, yard, orders);
        crate::sim::bot::repair_buildings(view, orders);
        crate::sim::bot::tend_miners(view, THINK_EVERY, &mut self.miner_health, orders);
        crate::sim::bot::sell_for_miner(view, THINK_EVERY, &options, orders);
        self.fire_supers(view, yard, orders);
        // Engineer raids on foot (v6) lost every escort and captured
        // nothing; kept for a transport-borne retry.
        if RAIDS {
            self.raid(view, &base, yard, &options, &units, orders);
        }
        self.notes.credits = view.credits();
        self.notes.power = view.power();
        self.notes.known_enemy_buildings = view.enemy_objects().filter(|e| is_structure(e)).count();
        self.scout(view, yard, &units, orders);
        let threatened = self.defend(view, Some(&base), &units, orders);
        self.notes.threatened = threatened;
        self.hold_ground(view, yard, &units, threatened, orders);
        self.base_guards(view, yard, &units, orders);
        self.discs(view, yard, &units, orders);
        self.kirovs(view, yard, orders);
        if !threatened && !matches!(self.capture, 1 | 3) {
            self.attack(view, yard, &units, orders);
        }
        self.kite(view, &units, orders);
    }
}

impl Vanguard {
    /// A second Construction Yard once affordable, on the far side of the
    /// base from the enemy: one Kirov run on the only yard otherwise ends
    /// the building for good.
    fn second_yard(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        yard: (u16, u16),
        mcv: Option<&crate::sim::game_entity::GameEntity>,
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        let minutes = (frame / 45 / 60) as i32;
        if !tune::YARD2 || base.count(Role::ConYard) >= 2 {
            return;
        }
        if let Some(mcv) = mcv {
            if frame.saturating_sub(self.last_deploy) < 90 {
                return;
            }
            self.last_deploy = frame;
            let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
            let away = |a: u16, e: u16| -> u16 {
                let d = (i32::from(a) - i32::from(e)).signum() * tune::YARD2_DIST;
                (i32::from(a) + d).clamp(2, 400) as u16
            };
            let target = (away(yard.0, enemy.0), away(yard.1, enemy.1));
            let at = cell(mcv);
            let mz = view.object_type(mcv.type_ref()).map(|t| t.movement_zone);
            // The reachable cell nearest the target (placement previews
            // judge building adjacency, which an MCV's deploy does not need).
            let site = (0..16i32)
                .find_map(|ring| {
                    let mut best: Option<(i32, (u16, u16))> = None;
                    for dx in -ring..=ring {
                        for dy in -ring..=ring {
                            if dx.abs() != ring && dy.abs() != ring {
                                continue;
                            }
                            let x = i32::from(target.0) + dx;
                            let y = i32::from(target.1) + dy;
                            if x < 2 || y < 2 {
                                continue;
                            }
                            let c = (x as u16, y as u16);
                            if c != at && !view.can_deploy_at(mcv, c.0, c.1) {
                                continue;
                            }
                            if mz.is_some_and(|mz| !view.can_reach(mz, at, c)) {
                                continue;
                            }
                            let d = dist2(c, target);
                            if best.is_none_or(|(bd, _)| d < bd) {
                                best = Some((d, c));
                            }
                        }
                    }
                    best.map(|(_, c)| c)
                })
                .unwrap_or(at);
            self.deploy_tries += 1;
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "yard2 f{frame} mcv@{at:?} target={target:?} site={site:?} tries={}",
                    self.deploy_tries
                );
            }
            // v42 (knob PARK): the spare stays an MCV out of the way and
            // deploys only once the yard is gone. A second yard was a second
            // target for engineer drops and factory attacks (team scripts
            // `47`/`0:6` pick Construction Yards and factories, not MCVs).
            // Off since 2026-11-14 (playtest: no MCVs standing around).
            if tune::PARK {
                if dist2(at, site) > 16 {
                    orders.push(Command::Move {
                        entity_id: mcv.stable_id(),
                        target_rx: site.0,
                        target_ry: site.1,
                    });
                }
                return;
            }
            if dist2(at, site) <= 16 && mcv.navigation.nav_com().is_some() {
                // TryToDeploy (`0x00738D30`) deploys only an MCV without a
                // destination: stop first.
                orders.push(Command::Stop {
                    entity_id: mcv.stable_id(),
                });
            } else if dist2(at, site) <= 16 && self.deploy_tries % 3 != 0 {
                // Turning and unfolding take a while: no order meanwhile.
                self.last_deploy = frame + 210;
                orders.push(Command::DeployMcv {
                    entity_id: mcv.stable_id(),
                });
            } else {
                // On the way, or the deploy failed here: a step aside.
                let jitter = if dist2(at, site) <= 16 {
                    (2 + self.deploy_tries / 3).min(10)
                } else {
                    0
                };
                let dx = self.rng.below(2 * jitter + 1) as i32 - jitter as i32;
                let dy = self.rng.below(2 * jitter + 1) as i32 - jitter as i32;
                orders.push(Command::Move {
                    entity_id: mcv.stable_id(),
                    target_rx: (i32::from(site.0) + dx).max(1) as u16,
                    target_ry: (i32::from(site.1) + dy).max(1) as u16,
                });
            }
            return;
        }
        if minutes < tune::YARD2_MIN
            || view.credits() < tune::YARD2_CASH
            || base.count(Role::WarFactory) == 0
            || frame % 90 >= THINK_EVERY
        {
            return;
        }
        let building = view.factory(ProductionCategory::Vehicle).is_some_and(|f| {
            f.object.is_some_and(|o| {
                view.object_type(o.type_id)
                    .is_some_and(|t| t.deploys_into.is_some())
            }) || f.queue.iter().any(|q| {
                view.object_type(q.type_id)
                    .is_some_and(|t| t.deploys_into.is_some())
            })
        });
        if building {
            return;
        }
        let options = view.build_options();
        if let Some(pick) = options.iter().find(|o| {
            o.enabled
                && o.queue_category == ProductionCategory::Vehicle
                && view.object_type(o.type_id).is_some_and(|t| {
                    t.deploys_into.as_deref().is_some_and(|into| {
                        view.rules.object(into).is_some_and(|b| b.construction_yard)
                    })
                })
        }) {
            self.mcv_queued = true;
            orders.push(Command::QueueProduction {
                type_id: pick.type_id,
            });
            return;
        }
        // The MCV needs a Service Depot (`Prerequisite=GAWEAP,GADEPT`).
        let have_depot = view.own().any(|e| {
            is_structure(e)
                && view
                    .object_type(e.type_ref())
                    .is_some_and(|t| t.unit_repair)
        });
        let strip_idle = view
            .factory(ProductionCategory::Building)
            .is_none_or(|f| f.object.is_none() && f.queue.is_empty());
        if !have_depot
            && strip_idle
            && let Some(depot) = options.iter().find(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Building
                    && view
                        .object_type(o.type_id)
                        .is_some_and(|t| t.unit_repair && !t.naval)
            })
        {
            orders.push(Command::QueueProduction {
                type_id: depot.type_id,
            });
        }
    }

    fn scan_ore(&mut self, view: &BotView<'_>, yard: (u16, u16)) {
        let (w, h) = view.map_size();
        let mut found = Vec::new();
        for y in (0..h).step_by(2) {
            for x in (0..w).step_by(2) {
                if view.has_ore(x, y) {
                    found.push((x, y));
                }
            }
        }
        found.sort_by_key(|&c| dist2(c, yard));
        found.truncate(400);
        self.ore = found;
    }

    /// Start positions the House has not explored yet, nearest `from` first:
    /// where an unseen enemy base may stand.
    fn unexplored_starts(view: &BotView<'_>, from: (u16, u16)) -> Vec<(u16, u16)> {
        let mut starts: Vec<(u16, u16)> = view
            .start_locations()
            .into_iter()
            .filter(|&(x, y)| !view.explored(x, y))
            .collect();
        starts.sort_by_key(|&c| (dist2(c, from), c));
        starts
    }

    /// Where to look when no enemy building is in sight: the nearest
    /// unexplored start position, else the nearest unexplored patch the army
    /// can drive to (`reach`), else (fog of war: buildings put up where the
    /// bot looked before) the nearest such patch out of sight now.
    fn search_cell(
        view: &BotView<'_>,
        from: (u16, u16),
        reach: impl Fn((u16, u16)) -> bool,
    ) -> Option<(u16, u16)> {
        if let Some(&start) = Self::unexplored_starts(view, from)
            .iter()
            .find(|&&c| reach(c))
        {
            return Some(start);
        }
        // Playtest, 2026-11-22 (the enemy's last buildings untouched for four
        // hours): an unexplored patch on a cliff or the water held the army
        // at its foot for good, and an explored map ended the search.
        let (w, h) = view.map_size();
        for unexplored in [true, false] {
            let mut best: Option<(i32, (u16, u16))> = None;
            for y in (4..h.saturating_sub(4)).step_by(6) {
                for x in (4..w.saturating_sub(4)).step_by(6) {
                    let hidden = if unexplored {
                        !view.explored(x, y)
                    } else {
                        !view.in_sight(x, y)
                    };
                    if !hidden || !view.in_playfield(x, y) {
                        continue;
                    }
                    let d = dist2((x, y), from);
                    if best.is_none_or(|(bd, _)| d < bd) && reach((x, y)) {
                        best = Some((d, (x, y)));
                    }
                }
            }
            if best.is_some() {
                return best.map(|(_, c)| c);
            }
        }
        None
    }

    /// One fast unit (a dog first, else infantry) runs to the unexplored
    /// start positions until an enemy building is in sight.
    fn scout(
        &mut self,
        view: &BotView<'_>,
        yard: (u16, u16),
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        if view.enemy_objects().any(|e| is_structure(e)) {
            self.notes.scouting = None;
            self.notes.scout_at = None;
            self.scout = None;
            return;
        }
        self.notes.scouting = None;
        self.notes.scout_at = None;
        let Some(&goal) = Self::unexplored_starts(view, yard).first() else {
            return;
        };
        self.notes.scouting = Some(goal);
        let alive = self
            .scout
            .and_then(|(id, _)| units.iter().find(|u| u.id == id));
        let scout = match alive {
            Some(unit) => {
                self.notes.scout_at = Some(unit.at);
                unit
            }
            None => {
                let pick = units
                    .iter()
                    .filter(|u| {
                        !u.aircraft && !u.stays_home() && !u.disc && !self.army.contains(&u.id)
                    })
                    .min_by_key(|u| (!u.dog, !u.infantry, u.id));
                let Some(pick) = pick else { return };
                self.scout = Some((pick.id, 0));
                pick
            }
        };
        let (id, sent) = self.scout.expect("scout");
        if sent == 0 || frame.saturating_sub(sent) >= 300 {
            self.scout = Some((id, frame.max(1)));
            orders.push(Command::Move {
                entity_id: scout.id,
                target_rx: goal.0,
                target_ry: goal.1,
            });
        }
    }

    fn nearest_enemy_base(&self, view: &BotView<'_>, from: (u16, u16)) -> Option<(u16, u16)> {
        view.enemy_objects()
            .filter(|e| is_structure(e))
            .map(cell)
            .min_by_key(|&c| dist2(c, from))
            .or_else(|| Self::unexplored_starts(view, from).first().copied())
    }

    fn build_base(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        yard: (u16, u16),
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) {
        let factory = view.factory(ProductionCategory::Building);
        if log::log_enabled!(log::Level::Debug) && view.frame() % 900 < THINK_EVERY {
            log::debug!(
                "bq f{} obj={:?} queue={} done={:?}",
                view.frame(),
                factory
                    .as_ref()
                    .and_then(|f| f.object.map(|o| view.name(o.type_id))),
                factory.as_ref().map_or(0, |f| f.queue.len()),
                factory
                    .as_ref()
                    .and_then(|f| f.complete_object())
                    .map(|o| view.name(o.type_id))
            );
        }
        if let Some(done) = factory.as_ref().and_then(|f| f.complete_object()) {
            if view.object_type(done.type_id).is_some_and(|t| t.wall) {
                // A wall segment: in front of a defense (`bot::wall_site`).
                let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
                match crate::sim::bot::wall_site(view, done.type_id, enemy) {
                    Some((rx, ry)) => orders.push(Command::PlaceReadyBuilding {
                        type_id: done.type_id,
                        rx,
                        ry,
                    }),
                    None => {
                        // No site: cancelled, and no walls for 2 minutes.
                        self.no_site.push((done.type_id, view.frame() + 45 * 120));
                        orders.push(Command::CancelProductionByType {
                            type_id: done.type_id,
                            all: true,
                        });
                    }
                }
                return;
            }
            self.place(view, base, done.type_id, yard, orders);
            return;
        }
        let idle = factory.is_none_or(|f| f.object.is_none() && f.queue.is_empty());
        if idle && let Some(next) = self.next_building(view, base, options) {
            orders.push(Command::QueueProduction { type_id: next });
        }
    }

    fn next_building(
        &self,
        view: &BotView<'_>,
        base: &Base,
        options: &[BuildOption],
    ) -> Option<InternedId> {
        let (output, drain) = view.power();
        let credits = view.credits();
        let refineries = base.count(Role::Refinery);
        let factories = base.count(Role::WarFactory);
        let ore = !self.ore.is_empty();
        let minutes = view.frame() / 45 / 60;
        let want = if output - drain < 50 + 25 * factories as i32 {
            Role::Power
        } else if self.capture == 0 && base.count(Role::Barracks) == 0 {
            Role::Barracks
        } else if refineries == 0 {
            Role::Refinery
        } else if base.count(Role::Barracks) == 0 {
            Role::Barracks
        } else if refineries < 2 {
            Role::Refinery
        } else if factories == 0 {
            Role::WarFactory
        } else if refineries < 3 && ore && tune::EARLY_REF3 {
            Role::Refinery
        } else if refineries < tune::REF_FIRST as usize
            && ore
            && minutes >= 3
            && base.count(Role::Defense) >= 2
        {
            // v46d (knob REF_FIRST): more refineries before radar, Grinder
            // and lab (Yuri stayed at ~4 refineries by minute 12).
            Role::Refinery
        } else if base.count(Role::Defense) < 2 {
            // The first two towers come before more economy; the radar
            // first where the tower needs it (Prism).
            let tower = pick_named(view, options, ProductionCategory::Defense, DEFENSES).is_some();
            if tower || base.count(Role::Radar) > 0 {
                return None;
            }
            Role::Radar
        } else if refineries < 3 && ore {
            Role::Refinery
        } else if base.count(Role::Radar) == 0 && (minutes >= 5 || credits > 2000) {
            Role::Radar
        } else if base.count(Role::Grinder) == 0
            && minutes >= tune::GRINDER_MIN as u32
            && tune::GRIND
            && options.iter().any(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Building
                    && view
                        .object_type(o.type_id)
                        .is_some_and(|t| role(view, t) == Role::Grinder)
            })
        {
            // Yuri sells what the towers and Masterminds take over.
            Role::Grinder
        } else if base.count(Role::Depot) == 0
            && minutes >= tune::DEPOT_MIN as u32
            && tune::REPAIR_PCT > 0
            && options.iter().any(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Building
                    && view
                        .object_type(o.type_id)
                        .is_some_and(|t| role(view, t) == Role::Depot)
            })
        {
            // Damaged tanks come home to it (`repair_units`).
            Role::Depot
        } else if base.count(Role::Radar) > 0
            && base.count(Role::Lab) == 0
            && (credits > 2000 || minutes >= tune::LAB_MIN as u32)
        {
            // The lab opens the long-range units (Prism Tank, Siege
            // Chopper, Floating Disc, Mastermind).
            Role::Lab
        } else if factories < 2 && minutes >= tune::WF2_MIN as u32 {
            // Production, not banked money, limits the army: the credits
            // are spent as they come, so "more than 2000" never held (v38
            // fought minute 14 with one war factory and no tanks).
            Role::WarFactory
        } else if refineries < tune::REFS as usize && ore && minutes >= 5 {
            Role::Refinery
        } else if factories < 3 && minutes >= tune::WF3_MIN as u32 {
            Role::WarFactory
        } else {
            return None;
        };
        options
            .iter()
            .filter(|o| o.enabled && o.queue_category == ProductionCategory::Building)
            .filter(|o| self.sited(view, o.type_id))
            .filter(|o| {
                view.object_type(o.type_id)
                    .is_some_and(|ty| role(view, ty) == want && !ty.naval)
            })
            .min_by_key(|o| o.cost)
            .map(|o| o.type_id)
    }

    /// Places a finished building; one that finds no site for 10 looks is
    /// cancelled (refunded) and its type skipped for two minutes, so it does
    /// not hold the strip for good.
    fn place(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        type_id: InternedId,
        yard: (u16, u16),
        orders: &mut Vec<Command>,
    ) {
        let strip = usize::from(
            view.object_type(type_id)
                .is_some_and(|t| role(view, t) == Role::Defense || role(view, t) == Role::Super),
        );
        if let Some((rx, ry)) = self.find_site(view, base, type_id, yard) {
            self.place_fails[strip] = 0;
            orders.push(Command::PlaceReadyBuilding { type_id, rx, ry });
            return;
        }
        self.place_fails[strip] += 1;
        if self.place_fails[strip] >= 10 {
            self.place_fails[strip] = 0;
            let frame = view.frame();
            self.no_site.retain(|&(_, until)| until > frame);
            self.no_site.push((type_id, view.frame() + 45 * 120));
            orders.push(Command::CancelProductionByType {
                type_id,
                all: false,
            });
        }
    }

    fn sited(&self, view: &BotView<'_>, type_id: InternedId) -> bool {
        !self
            .no_site
            .iter()
            .any(|&(t, until)| t == type_id && view.frame() < until)
    }

    /// A refinery goes where it is allowed nearest the closest ore no
    /// refinery serves yet (else the closest ore): the base creeps toward the
    /// next field as the near one runs out.
    fn refinery_site(
        &self,
        view: &BotView<'_>,
        base: &Base,
        name: &str,
        w: u16,
        h: u16,
    ) -> Option<(u16, u16)> {
        let serve = tune::REF_SERVE.pow(2);
        let target = self
            .ore
            .iter()
            .copied()
            .find(|&o| base.refineries.iter().all(|&r| dist2(r, o) > serve))
            .or_else(|| self.ore.first().copied())?;
        let (w, h) = (i32::from(w), i32::from(h));
        let (mw, mh) = view.map_size();
        let reach = 26;
        let mut spots: Vec<(i32, (i32, i32))> = Vec::new();
        for y in i32::from(target.1) - reach..=i32::from(target.1) + reach {
            for x in i32::from(target.0) - reach..=i32::from(target.0) + reach {
                if x < 2 || y < 2 || x + w + 2 > i32::from(mw) || y + h + 2 > i32::from(mh) {
                    continue;
                }
                let rect = Rect {
                    x,
                    y,
                    w,
                    h,
                    gap: gap_for(Role::Refinery, base.spread),
                };
                if base.rects.iter().any(|other| rect.conflicts(other)) {
                    continue;
                }
                let c = ((x + w / 2) as u16, (y + h / 2) as u16);
                spots.push((dist2(c, target), (x, y)));
            }
        }
        spots.sort_unstable();
        let site = spots
            .iter()
            .take(1500)
            .find(|(_, (x, y))| view.can_place(name, *x as u16, *y as u16))
            .map(|&(_, (x, y))| (x as u16, y as u16));
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "refsite f{} target={target:?} spots={} site={site:?} ore0={:?} n_ore={}",
                view.frame(),
                spots.len(),
                self.ore.first(),
                self.ore.len()
            );
        }
        site
    }

    /// The nearest free spot to the building's anchor that leaves lanes
    /// around factories and refineries.
    fn find_site(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        type_id: InternedId,
        yard: (u16, u16),
    ) -> Option<(u16, u16)> {
        let name = view.name(type_id);
        let ty = view.object_type(type_id)?;
        let r = role(view, ty);
        let (w, h) = ty.foundation.dimensions();
        let toward = |from: (u16, u16), to: (u16, u16), reach: i32| -> (u16, u16) {
            let step = |a: u16, b: u16| -> u16 {
                let d = (i32::from(b) - i32::from(a)).clamp(-reach, reach);
                (i32::from(a) + d).max(1) as u16
            };
            (step(from.0, to.0), step(from.1, to.1))
        };
        if r == Role::Refinery
            && let Some(site) = self.refinery_site(view, base, name, w, h)
        {
            return Some(site);
        }
        let anchor = match r {
            // The anti-infantry one stands by the yard, against engineers.
            Role::Defense if matches!(name, "NALASR" | "GAPILL" | "YAPSYT") => yard,
            // Anti-air on the enemy's side of the yard: aircraft come
            // straight from there and go for the yard first.
            Role::Defense if AA_DEFENSES.contains(&name) && name != "YAGGUN" => {
                let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
                toward(yard, enemy, tune::AA_REACH)
            }
            Role::Defense => {
                let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
                let front = toward(yard, enemy, 9);
                // Prism Towers stand together: each one in reach of another
                // adds its beam to the shot.
                let same = (name == "ATESLA").then(|| {
                    view.own()
                        .filter(|e| is_structure(e) && view.name(e.type_ref()) == name)
                        .map(cell)
                        .min_by_key(|&c| (dist2(c, front), c))
                });
                match same.flatten() {
                    Some(tower) if tune::PRISM_CLUSTER => tower,
                    _ => front,
                }
            }
            // Knob BAIT (off: v42 rounds m12 -0.71 with 6 vs -0.51 without):
            // factories stand toward the enemy, the yard
            // behind them. AI attack teams take the nearest factory from
            // their gathering point (`0:6`, threat mask 0x1000 includes the
            // yard), so Kirovs and tank teams meet a factory under the
            // anti-air first.
            Role::Barracks | Role::WarFactory if tune::BAIT > 0 => {
                let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
                toward(yard, enemy, tune::BAIT)
            }
            _ => yard,
        };
        let gap = gap_for(r, base.spread);
        let (mw, mh) = view.map_size();
        let mut best: Option<((u16, u16), i32)> = None;
        // The anchor first, then the yard and the base's middle (an anchor
        // out by the ore can be beyond where building is allowed).
        for anchor in [anchor, yard, base.center] {
            for radius in 0i32..18 {
                if best.is_some() && radius > 3 {
                    break;
                }
                for dx in -radius..=radius {
                    for dy in -radius..=radius {
                        if dx.abs().max(dy.abs()) != radius {
                            continue;
                        }
                        let (x, y) = (i32::from(anchor.0) + dx, i32::from(anchor.1) + dy);
                        if x < 2
                            || y < 2
                            || x + i32::from(w) + 2 > i32::from(mw)
                            || y + i32::from(h) + 2 > i32::from(mh)
                        {
                            continue;
                        }
                        let rect = Rect {
                            x,
                            y,
                            w: i32::from(w),
                            h: i32::from(h),
                            gap,
                        };
                        if base.rects.iter().any(|other| rect.conflicts(other)) {
                            continue;
                        }
                        let score = dx * dx + dy * dy;
                        if best.is_some_and(|(_, s)| s <= score) {
                            continue;
                        }
                        if view.can_place(name, x as u16, y as u16) {
                            best = Some(((x as u16, y as u16), score));
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        if log::log_enabled!(log::Level::Debug) && r == Role::Refinery {
            log::debug!(
                "site {name} anchor={anchor:?} yard={yard:?} best={best:?} ore0={:?}",
                self.ore.first()
            );
        }
        best.map(|(c, _)| c)
    }

    fn build_defenses(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        yard: (u16, u16),
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) {
        if base.count(Role::Barracks) == 0 {
            return;
        }
        let factory = view.factory(ProductionCategory::Defense);
        if let Some(done) = factory.as_ref().and_then(|f| f.complete_object()) {
            if view.object_type(done.type_id).is_some_and(|t| t.wall) {
                // A wall segment: in front of a defense (`bot::wall_site`).
                let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
                match crate::sim::bot::wall_site(view, done.type_id, enemy) {
                    Some((rx, ry)) => orders.push(Command::PlaceReadyBuilding {
                        type_id: done.type_id,
                        rx,
                        ry,
                    }),
                    None => orders.push(Command::CancelProductionByType {
                        type_id: done.type_id,
                        all: true,
                    }),
                }
                return;
            }
            self.place(view, base, done.type_id, yard, orders);
            return;
        }
        let idle = factory.is_none_or(|f| f.object.is_none() && f.queue.is_empty());
        let have = base.count(Role::Defense);
        let minutes = (view.frame() / 45 / 60) as usize;
        let cap = if base.count(Role::WarFactory) == 0 || base.count(Role::Refinery) < 2 {
            0
        } else {
            (minutes + 1).clamp(2, tune::DEF_CAP.max(2) as usize)
        };
        let (output, drain) = view.power();
        let wall = (idle && tune::WALLS)
            .then(|| crate::sim::bot::wall_to_build(view, options))
            .flatten()
            .filter(|&wall| self.sited(view, wall));
        if !idle || view.credits() < 400 || output - drain < 40 {
            // Walls take no power: they go up while the power is short.
            if let Some(wall) = wall {
                orders.push(Command::QueueProduction { type_id: wall });
            }
            return;
        }
        // The superweapon (the Defense strip builds it) once there is a lab.
        // Playtest, 2026-11-14: every superweapon type the side has, rich a
        // second of each (a spare when one falls); the options list leaves
        // out what BuildLimit or "One of each" forbids.
        let copies = if view.credits() > 8000 && minutes >= 20 {
            2
        } else {
            1
        };
        let owned = |t: InternedId| view.own().filter(|e| e.type_ref() == t).count();
        if tune::SUPER
            && base.count(Role::Lab) > 0
            && view.credits() > 2500
            && let Some(sw) = options.iter().find(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Defense
                    && view
                        .object_type(o.type_id)
                        .is_some_and(|t| role(view, t) == Role::Super)
                    && owned(o.type_id) < copies
            })
        {
            orders.push(Command::QueueProduction {
                type_id: sw.type_id,
            });
            return;
        }
        // v45d (knob SUPPORT): then the Iron Curtain or Genetic Mutator.
        let support = |o: &&BuildOption| {
            view.object_type(o.type_id)
                .and_then(|t| t.super_weapon.as_deref())
                .and_then(|sw| view.rules.super_weapon(sw))
                .is_some_and(|sw| {
                    matches!(
                        sw.kind,
                        SuperWeaponKind::IronCurtain | SuperWeaponKind::GeneticConverter
                    )
                })
        };
        if tune::SUPPORT
            && base.count(Role::Lab) > 0
            && base.count(Role::Super) > 0
            && view.credits() > 2500
            && let Some(sw) = options
                .iter()
                .filter(|o| o.enabled && o.queue_category == ProductionCategory::Defense)
                .filter(|o| owned(o.type_id) < copies)
                .find(support)
        {
            orders.push(Command::QueueProduction {
                type_id: sw.type_id,
            });
            return;
        }
        // Anti-air on a clock: the Brutal AI's first Kirovs and planes come
        // from minute 6 or 7.
        let aa_from = tune::AA_FROM as usize;
        let aa_want = match minutes {
            m if m < aa_from => 0,
            m if m < aa_from + 2 => tune::AA_EARLY as usize,
            _ => tune::AA_MID as usize,
        } + self.air_threat.min(3) * tune::AA_PER as usize;
        let want_aa = base.aa_defenses < aa_want && have >= 3 && cap > 0;
        // Walls in front of the defenses (WALLS), first while money is to
        // spare, else once the defenses are complete (below).
        if !want_aa
            && view.credits() >= 2000
            && let Some(wall) = wall
        {
            orders.push(Command::QueueProduction { type_id: wall });
            return;
        }
        if !want_aa && have.saturating_sub(base.aa_defenses) >= cap {
            if let Some(wall) = wall {
                orders.push(Command::QueueProduction { type_id: wall });
            }
            return;
        }
        let names: &[&str] = if have == 2 && !want_aa {
            &["NALASR", "GAPILL", "YAPSYT"]
        } else if want_aa {
            AA_DEFENSES
        } else {
            DEFENSES
        };
        let pick = pick_named(view, options, ProductionCategory::Defense, names).or_else(|| {
            (have != 2).then(|| pick_named(view, options, ProductionCategory::Defense, DEFENSES))?
        });
        if let Some(pick) = pick
            && self.sited(view, pick)
        {
            orders.push(Command::QueueProduction { type_id: pick });
        }
    }

    fn produce(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) {
        if base.count(Role::Refinery) == 0 && view.credits() < 3000 {
            return;
        }
        let busy = |category| {
            view.factory(category)
                .map_or(0, |f| f.queue.len() + usize::from(f.object.is_some()))
        };
        // Harvesters first: two per refinery.
        if base.count(Role::WarFactory) > 0 && busy(ProductionCategory::Vehicle) < 2 {
            let refineries = base.count(Role::Refinery);
            let want_harvesters = (refineries * 2).min(tune::HARVS as usize);
            // A miner that deploys into a refinery (Yuri's) is a refinery.
            let harvester = options.iter().find(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Vehicle
                    && view
                        .object_type(o.type_id)
                        .is_some_and(|t| t.harvester && t.deploys_into.is_none())
            });
            // v46b (knob SMINS): Yuri's war factory adds Slave Miners (a
            // refinery each once deployed) while the yard's strip is busy.
            let slave_miner = options.iter().find(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Vehicle
                    && view.object_type(o.type_id).is_some_and(|t| {
                        (t.harvester || t.resource_gatherer) && t.deploys_into.is_some()
                    })
            });
            let miners_out = self.slave_miner_chore.heading();
            let pick = if let Some(sm) = slave_miner
                && refineries + miners_out < tune::SMINS as usize
                && miners_out == 0
                && self.vehicles_built % 2 == 0
            {
                self.vehicles_built += 1;
                Some(sm.type_id)
            } else if base.harvesters < want_harvesters && harvester.is_some() {
                harvester.map(|o| o.type_id)
            } else {
                self.vehicles_built += 1;
                let minutes = view.frame() / 45 / 60;
                let aa_wanted = if minutes >= tune::AA_FROM as u32 {
                    3
                } else {
                    0
                } + self.air_threat.min(3);
                let aa = self.own_aa < aa_wanted && self.vehicles_built % 3 != 0
                    || self.vehicles_built % 6 == 0;
                let own = |name: &str| {
                    view.own()
                        .filter(|e| view.name(e.type_ref()) == name)
                        .count()
                };
                let pick =
                    |names: &[&str]| pick_named(view, options, ProductionCategory::Vehicle, names);
                let n = self.vehicles_built;
                let group_unit = group_type(view);
                if !aa
                    && n % 3 != 0
                    && let Some(name) = group_unit
                    && own(name) < tune::GROUP as usize
                    && let Some(unit) = pick(&[name])
                {
                    Some(unit)
                } else if aa {
                    pick(ANTI_AIR)
                } else if n % 2 == 0
                    && (own(DISC) as i32) < tune::DISCS
                    && let Some(disc) = pick(&[DISC])
                {
                    Some(disc)
                } else if own("MIND") < tune::MINDS as usize
                    && n % 4 == 1
                    && let Some(mind) = pick(&["MIND"])
                {
                    Some(mind)
                } else if (own("ZEP") as i32) < tune::ZEPS
                    && n % 3 == 0
                    && let Some(zep) = pick(&["ZEP"])
                {
                    Some(zep)
                } else if tune::SIEGE
                    && (n % 4) < tune::SIEGE_OF_4 as u32
                    && let Some(siege) = pick(SIEGE)
                {
                    // Long-range units are the core; the rest escort them.
                    Some(siege)
                } else {
                    pick(TANKS)
                        .or_else(|| pick(SIEGE))
                        .or_else(|| best_unit(view, options, ProductionCategory::Vehicle))
                }
            };
            if let Some(unit) = pick {
                orders.push(Command::QueueProduction { type_id: unit });
            }
        }
        // Crate scouts first (`bot::CrateScouts`).
        if base.count(Role::Barracks) > 0
            && (self.scouts.produce(view, options, orders)
                || self.spies.produce(view, options, orders))
        {
            return;
        }
        // Two dogs for the garrison (they kill engineers at once).
        let dogs = view
            .own()
            .filter(|e| matches!(view.name(e.type_ref()), "DOG" | "ADOG"))
            .count()
            .saturating_sub(self.scouts.dogs(view));
        if dogs < 2
            && busy(ProductionCategory::Infantry) < 1
            && let Some(dog) = pick_named(
                view,
                options,
                ProductionCategory::Infantry,
                &["DOG", "ADOG"],
            )
        {
            orders.push(Command::QueueProduction { type_id: dog });
            return;
        }
        // GIs and Guardian GIs that hold the base deployed (`hold_ground`):
        // a few from the start, more as the game goes on.
        // (The starting GIs hold too; Guardian GIs are built.)
        let gis = view
            .own()
            .filter(|e| view.name(e.type_ref()) == "GGI")
            .count() as i32;
        let minutes = (view.frame() / 45 / 60) as i32;
        let gis_wanted = (tune::GIS + minutes * tune::GIS_PER_2MIN / 2).min(tune::GI_CAP);
        if tune::HOLD
            && gis < gis_wanted
            && base.count(Role::Refinery) > 0
            && busy(ProductionCategory::Infantry) < 1
        {
            if let Some(gi) = pick_named(view, options, ProductionCategory::Infantry, &["GGI"]) {
                orders.push(Command::QueueProduction { type_id: gi });
                return;
            }
        }
        // Occupiers for the garrisons (playtest, 2026-11-14: "garrison more and
        // more buildings"): the side's basic soldier, while they can take one.
        if self.garrisons.wanted() > 0
            && view.credits() > 600
            && busy(ProductionCategory::Infantry) < 1
            && let Some(gi) = pick_named(
                view,
                options,
                ProductionCategory::Infantry,
                &["E1", "E2", "INIT"],
            )
        {
            orders.push(Command::QueueProduction { type_id: gi });
            return;
        }
        // Tesla Troopers by the coils, Rocketeers over the base.
        let coils = view
            .own()
            .filter(|e| is_structure(e) && view.name(e.type_ref()) == "TESLA")
            .count() as i32;
        let own = |name: &str| {
            view.own()
                .filter(|e| view.name(e.type_ref()) == name)
                .count() as i32
        };
        if own("SHK") < (coils * tune::TROOPERS_PER_COIL).min(tune::TROOPERS)
            && busy(ProductionCategory::Infantry) < 1
            && let Some(shk) = pick_named(view, options, ProductionCategory::Infantry, &["SHK"])
        {
            orders.push(Command::QueueProduction { type_id: shk });
            return;
        }
        if own("YURIPR") < tune::PRIMES
            && busy(ProductionCategory::Infantry) < 1
            && let Some(prime) =
                pick_named(view, options, ProductionCategory::Infantry, &["YURIPR"])
        {
            orders.push(Command::QueueProduction { type_id: prime });
            return;
        }
        // v46 (knob CLONES): Yuri Clones by the staging point take over
        // what the stock AI sends; the Grinder sells it (`grind_captured`).
        if own("YURI") < tune::CLONES
            && minutes >= 3
            && busy(ProductionCategory::Infantry) < 1
            && let Some(clone) = pick_named(view, options, ProductionCategory::Infantry, &["YURI"])
        {
            orders.push(Command::QueueProduction { type_id: clone });
            return;
        }
        if self
            .rocketeers
            .produce(view, options, tune::ROCKETEERS as usize, orders)
        {
            return;
        }
        if own("JUMPJET") < tune::ROCKETEERS
            && minutes >= 4
            && busy(ProductionCategory::Infantry) < 1
            && let Some(jj) = pick_named(view, options, ProductionCategory::Infantry, &["JUMPJET"])
        {
            orders.push(Command::QueueProduction { type_id: jj });
            return;
        }
        // Arena knob DESOS: Iraq's Desolators with the army.
        if own("DESO") < tune::DESOS
            && minutes >= 4
            && busy(ProductionCategory::Infantry) < 1
            && let Some(deso) = pick_named(view, options, ProductionCategory::Infantry, &["DESO"])
        {
            orders.push(Command::QueueProduction { type_id: deso });
            return;
        }
        // A few soldiers before the war factory, more when rich.
        let infantry_wanted =
            view.credits() > 4000 && units_infantry(view) < tune::INF_CAP as usize;
        if infantry_wanted
            && busy(ProductionCategory::Infantry) < 1
            && let Some(unit) = best_unit(view, options, ProductionCategory::Infantry)
        {
            orders.push(Command::QueueProduction { type_id: unit });
        }
    }

    /// Factories send what they build to a point in front of the base.
    fn set_rally(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        yard: (u16, u16),
        orders: &mut Vec<Command>,
    ) {
        let fresh: Vec<u64> = base
            .factories
            .iter()
            .copied()
            .filter(|id| !self.rally_set.contains(id))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let point = self.staging(view, yard);
        self.rally_set.extend(fresh.iter().copied());
        orders.push(Command::SetRally {
            rx: point.0,
            ry: point.1,
            producer_ids: fresh,
        });
    }

    fn staging(&self, view: &BotView<'_>, yard: (u16, u16)) -> (u16, u16) {
        let enemy = self.nearest_enemy_base(view, yard).unwrap_or(yard);
        let step = |a: u16, b: u16| -> u16 {
            let d = (i32::from(b) - i32::from(a)).clamp(-6, 6);
            (i32::from(a) + d).max(1) as u16
        };
        (step(yard.0, enemy.0), step(yard.1, enemy.1))
    }

    /// The opening: three engineers, escorted by the whole starting army,
    /// capture the enemy Construction Yard before it has defenses or a war
    /// factory to build a new MCV from. Short game then ends that House once
    /// the rest of its few buildings fall.
    fn opening_capture(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        options: &[BuildOption],
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) {
        const ENGINEERS: usize = 3;
        if !CAPTURE {
            self.capture = 2;
        }
        if self.capture == 2 {
            return;
        }
        let frame = view.frame();
        let engineers: Vec<&crate::sim::game_entity::GameEntity> = view
            .own()
            .filter(|e| view.object_type(e.type_ref()).is_some_and(|t| t.engineer))
            .collect();
        let target = view
            .enemy_objects()
            .filter(|e| is_structure(e))
            .filter(|e| {
                view.object_type(e.type_ref())
                    .is_some_and(|t| t.construction_yard)
            })
            .min_by_key(|e| (dist2(cell(e), base.yard.unwrap_or_default()), e.stable_id()));
        let Some(target) = target else {
            if self.capture == 1 {
                self.capture = 2;
            }
            return;
        };
        let at = cell(target);
        if self.capture == 0 {
            let busy = view
                .factory(ProductionCategory::Infantry)
                .is_some_and(|f| f.object.is_some() || !f.queue.is_empty());
            if engineers.len() < ENGINEERS && !busy {
                if let Some(engineer) = options.iter().find(|o| {
                    o.enabled
                        && o.queue_category == ProductionCategory::Infantry
                        && view.object_type(o.type_id).is_some_and(|t| t.engineer)
                }) {
                    orders.push(Command::QueueProduction {
                        type_id: engineer.type_id,
                    });
                }
            }
            if engineers.len() >= ENGINEERS || (frame > 45 * 120 && !engineers.is_empty()) {
                self.capture = 1;
                for unit in units.iter().filter(|u| !u.aircraft && !u.dog) {
                    if !self.army.contains(&unit.id) {
                        self.army.push(unit.id);
                    }
                }
                self.waves = self.waves.max(1);
            } else if frame > 45 * 180 {
                self.capture = 2;
            }
            return;
        }
        if engineers.is_empty() || frame > 45 * 60 * 5 {
            self.capture = 2;
            return;
        }
        if frame % 48 >= THINK_EVERY {
            return;
        }
        // Gather everyone at a point short of the enemy base, then go in
        // together, the engineers just behind the front.
        let army: Vec<(u16, u16)> = self
            .army
            .iter()
            .filter_map(|id| view.entity(*id))
            .map(cell)
            .collect();
        let home = base.yard.unwrap_or(at);
        let gather = {
            let mix = |a: u16, b: u16| ((i32::from(a) * 2 + i32::from(b) * 3) / 5) as u16;
            (mix(home.0, at.0), mix(home.1, at.1))
        };
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "capture f{frame} state={} army={} engineers={} gather={gather:?} target={at:?} lead={} eng={:?}",
                self.capture,
                army.len(),
                engineers.len(),
                army.iter().map(|&c| dist2(c, at)).min().unwrap_or(-1),
                engineers.iter().map(|e| cell(e)).collect::<Vec<_>>()
            );
        }
        if self.capture == 1 {
            let mut everyone = army.clone();
            everyone.extend(engineers.iter().map(|e| cell(e)));
            let there = everyone
                .iter()
                .filter(|&&c| dist2(c, gather) <= 7 * 7)
                .count();
            if there * 5 >= everyone.len() * 4 || frame.saturating_sub(self.last_attack) > 45 * 50 {
                self.capture = 3;
                self.last_attack = 0;
            } else {
                if self.last_attack == 0 || self.last_attack > frame {
                    self.last_attack = frame;
                }
                for &id in &self.army {
                    orders.push(Command::Move {
                        entity_id: id,
                        target_rx: gather.0,
                        target_ry: gather.1,
                    });
                }
                for engineer in &engineers {
                    orders.push(Command::Move {
                        entity_id: engineer.stable_id(),
                        target_rx: gather.0,
                        target_ry: gather.1,
                    });
                }
                return;
            }
        }
        let lead = army.iter().map(|&c| dist2(c, at)).min().unwrap_or(i32::MAX);
        if frame.saturating_sub(self.last_attack) >= 270 {
            self.last_attack = frame;
            let mut goals = Vec::new();
            for &id in &self.army {
                let Some(e) = view.entity(id) else { continue };
                let goal = goal_for(view, &mut goals, e, target, gather);
                orders.push(Command::AttackMove {
                    entity_id: id,
                    target: None,
                    destination: Some(TargetKind::Cell(goal.0, goal.1)),
                });
            }
        }
        for engineer in &engineers {
            let d = dist2(cell(engineer), at);
            if self.sent_in.contains(&engineer.stable_id()) {
                continue;
            }
            if d <= 14 * 14 || lead <= 11 * 11 || army.is_empty() {
                self.sent_in.push(engineer.stable_id());
                orders.push(Command::CaptureBuilding {
                    engineer_id: engineer.stable_id(),
                    target_building_id: target.stable_id(),
                });
            } else {
                // A few cells behind the army's front.
                let front = army
                    .iter()
                    .copied()
                    .min_by_key(|&c| dist2(c, at))
                    .unwrap_or(at);
                let back = |a: u16, b: u16| -> u16 {
                    let d = (i32::from(b) - i32::from(a)).clamp(-3, 3);
                    (i32::from(a) + d).max(1) as u16
                };
                let me = cell(engineer);
                orders.push(Command::Move {
                    entity_id: engineer.stable_id(),
                    target_rx: back(front.0, me.0),
                    target_ry: back(front.1, me.1),
                });
            }
        }
    }

    /// Every charged superweapon fires: offensive ones and paradrops at the
    /// enemy Construction Yard nearest the base, else its densest buildings.
    fn fire_supers(&mut self, view: &BotView<'_>, yard: (u16, u16), orders: &mut Vec<Command>) {
        let Some(supers) = view.super_weapons() else {
            return;
        };
        for (type_id, sw) in supers {
            if !sw.is_active() || !sw.is_ready() || sw.is_suspended() {
                continue;
            }
            let Some(kind) = view.rules.super_weapon(view.name(*type_id)).map(|t| t.kind) else {
                continue;
            };
            let paradrop = matches!(
                kind,
                SuperWeaponKind::ParaDrop | SuperWeaponKind::AmerParaDrop
            );
            if tune::SUPPORT
                && let Some(at) = self.support_target(view, kind)
            {
                orders.push(Command::LaunchSuperWeapon {
                    sw_type_id: *type_id,
                    target_rx: at.0,
                    target_ry: at.1,
                });
                continue;
            }
            if !offensive(kind) && !paradrop {
                continue;
            }
            // Paratroopers garrison neutral buildings while there are empty
            // ones (playtest, 2026-11-14).
            if paradrop && let Some(at) = self.garrisons.paradrop_target(view, yard) {
                orders.push(Command::LaunchSuperWeapon {
                    sw_type_id: *type_id,
                    target_rx: at.0,
                    target_ry: at.1,
                });
                continue;
            }
            // v42 (knob SW_LAB): the Battle Lab first. The stock AI's
            // heaviest teams (aimd.ini "Soviet KIrov Hard", "Soviet Apoc
            // Hard", weight 500) need it (condition 1: self owns NATECH),
            // and it has 500 Strength, wood armor.
            let enemy_super = (!paradrop && tune::WAVE_SW)
                .then(|| {
                    view.enemy_objects()
                        .filter(|e| {
                            is_structure(e)
                                && view
                                    .object_type(e.type_ref())
                                    .is_some_and(|t| role(view, t) == Role::Super)
                        })
                        .map(|e| {
                            let (x, y) = cell(e);
                            (x + 1, y + 1)
                        })
                        .min()
                })
                .flatten();
            let lab = (!paradrop && tune::SW_LAB)
                .then(|| lab_target(view))
                .flatten();
            let lab = enemy_super.or(lab);
            let target = if let Some(lab) = lab {
                Some(lab)
            } else if paradrop || !tune::SW_POWER {
                self.super_target(view)
            } else {
                power_target(view).or_else(|| self.super_target(view))
            };
            let Some(at) = target else {
                return;
            };
            if !paradrop {
                // The enemy's power is down: the army goes in at once.
                self.strike = view.frame().max(1);
            }
            orders.push(Command::LaunchSuperWeapon {
                sw_type_id: *type_id,
                target_rx: at.0,
                target_ry: at.1,
            });
        }
    }

    /// The strike group (knob GROUP, after human anti-Brutal play): once
    /// GROUP (9) of the side's group unit stand at home, they set out
    /// together and take no other orders. Apocalypse tanks (with the Iron
    /// Curtain as they close in, `support_target`) shoot the nearest seen
    /// enemy power plant, and once none is seen the enemy unit that hits
    /// hardest near them, then the nearest building; Prism Tanks do the
    /// same from their full reach, stepping back from anything armed that
    /// comes closer. Below a third left, they come home.
    fn strike_group(
        &mut self,
        view: &BotView<'_>,
        units: &mut Vec<Unit>,
        orders: &mut Vec<Command>,
    ) {
        let want = tune::GROUP as usize;
        self.notes.group = None;
        let Some(name) = group_type(view).filter(|_| want > 0) else {
            self.group.clear();
            return;
        };
        let frame = view.frame();
        self.group.retain(|id| units.iter().any(|u| u.id == *id));
        if self.group.is_empty() {
            let ready: Vec<u64> = units
                .iter()
                .filter(|u| {
                    !self.army.contains(&u.id)
                        && view
                            .entity(u.id)
                            .is_some_and(|e| view.name(e.type_ref()) == name)
                })
                .map(|u| u.id)
                .collect();
            if ready.len() < want {
                return;
            }
            self.group = ready;
            self.focus.retain(|(id, _)| !self.group.contains(id));
            self.siege_orders.retain(|(id, _)| !self.group.contains(id));
        } else if self.group.len() * 3 < want {
            // Too few left: home, as ordinary home units again.
            self.group.clear();
            return;
        }
        let members: Vec<Unit> = units
            .iter()
            .filter(|u| self.group.contains(&u.id))
            .cloned()
            .collect();
        units.retain(|u| !self.group.contains(&u.id));
        let n = members.len() as i32;
        let (sx, sy) = members.iter().fold((0i32, 0i32), |(x, y), u| {
            (x + i32::from(u.at.0), y + i32::from(u.at.1))
        });
        let center = ((sx / n) as u16, (sy / n) as u16);
        let power = view
            .enemy_objects()
            .filter(|e| {
                is_structure(e)
                    && view
                        .object_type(e.type_ref())
                        .is_some_and(|t| t.power > 0 && role(view, t) == Role::Power)
            })
            .min_by_key(|e| (dist2(cell(e), center), e.stable_id()));
        // The hardest hitter near the group: weapon damage per frame.
        let hitter = || {
            view.enemy_objects()
                .filter(|e| !is_structure(e) && !flies(view, e) && armed(view, e))
                .filter(|e| dist2(cell(e), center) <= 14 * 14)
                .max_by_key(|e| {
                    let dps = view.object_type(e.type_ref()).map_or(0, |t| {
                        t.weapon_list
                            .iter()
                            .flatten()
                            .filter_map(|w| view.rules.weapon(w))
                            .map(|w| w.damage.max(0) * w.burst.max(1) * 100 / w.rof.max(1))
                            .max()
                            .unwrap_or(0)
                    });
                    (dps, std::cmp::Reverse(e.stable_id()))
                })
        };
        let building = || {
            view.enemy_objects()
                .filter(|e| is_structure(e))
                .min_by_key(|e| (dist2(cell(e), center), e.stable_id()))
        };
        let target = power.or_else(hitter).or_else(building);
        let Some(target) = target else {
            // Nothing seen: look where the enemy may be.
            if let Some(goal) = self.nearest_enemy_base(view, center)
                && frame % 90 < THINK_EVERY
            {
                for u in &members {
                    orders.push(Command::AttackMove {
                        entity_id: u.id,
                        target: None,
                        destination: Some(TargetKind::Cell(goal.0, goal.1)),
                    });
                }
            }
            return;
        };
        let at = cell(target);
        self.notes.group = Some((center, at));
        let prism = name == "SREF";
        let threats: Vec<(u16, u16)> = view
            .enemy_objects()
            .filter(|e| armed(view, e) && !flies(view, e))
            .map(cell)
            .collect();
        for u in &members {
            let reach = view
                .entity(u.id)
                .map_or(0, |e| reach_of(view, e.type_ref()));
            if prism
                && let Some(&near) = threats
                    .iter()
                    .filter(|&&c| dist2(c, u.at) < (reach - 1).max(1).pow(2))
                    .min_by_key(|&&c| dist2(c, u.at))
            {
                // Too close: back to full reach, away from it.
                let back = toward_dist(near, u.at, reach);
                orders.push(Command::Move {
                    entity_id: u.id,
                    target_rx: back.0,
                    target_ry: back.1,
                });
                continue;
            }
            // Again when idle or every 3 seconds: the target changes.
            if u.moving && frame % 45 >= THINK_EVERY {
                continue;
            }
            orders.push(Command::Attack {
                attacker_id: u.id,
                target_id: target.stable_id(),
            });
        }
    }

    /// v45d (knob SUPPORT): the Iron Curtain on the army once it is in a
    /// fight (armed enemies within 8 cells of its middle); the Genetic
    /// Mutator on the thickest enemy infantry seen (5 or more within 2 cells).
    fn support_target(&self, view: &BotView<'_>, kind: SuperWeaponKind) -> Option<(u16, u16)> {
        match kind {
            SuperWeaponKind::IronCurtain => {
                // The strike group first, as it closes in on its target.
                if let Some((center, target)) = self.notes.group
                    && dist2(center, target) <= 10 * 10
                {
                    return Some(center);
                }
                let center = self.notes.army_center.filter(|_| self.army.len() >= 4)?;
                let fighting = view
                    .enemy_objects()
                    .filter(|e| armed(view, e) && dist2(cell(e), center) <= 8 * 8)
                    .count();
                (fighting >= 3).then_some(center)
            }
            SuperWeaponKind::GeneticConverter => {
                let infantry: Vec<(u16, u16)> = view
                    .enemy_objects()
                    .filter(|e| e.category() == EntityCategory::Infantry)
                    .map(cell)
                    .collect();
                infantry
                    .iter()
                    .map(|&at| {
                        let n = infantry.iter().filter(|&&b| dist2(at, b) <= 2 * 2).count();
                        (n, std::cmp::Reverse(at))
                    })
                    .max()
                    .filter(|&(n, _)| n >= 5)
                    .map(|(_, std::cmp::Reverse(at))| at)
            }
            _ => None,
        }
    }

    fn super_target(&self, view: &BotView<'_>) -> Option<(u16, u16)> {
        let buildings: Vec<(&crate::sim::game_entity::GameEntity, (u16, u16))> = view
            .enemy_objects()
            .filter(|e| is_structure(e))
            .map(|e| (e, cell(e)))
            .collect();
        let yard = buildings
            .iter()
            .filter(|(e, _)| {
                view.object_type(e.type_ref())
                    .is_some_and(|t| t.construction_yard)
            })
            .map(|(_, at)| *at)
            .min();
        if let Some((x, y)) = yard {
            return Some((x + 1, y + 1));
        }
        buildings
            .iter()
            .map(|(_, at)| {
                let near = buildings
                    .iter()
                    .filter(|(_, b)| dist2(*b, *at) <= 25)
                    .count();
                (std::cmp::Reverse(near), *at)
            })
            .min()
            .map(|(_, at)| at)
    }

    /// Units a Mastermind or Psychic Tower took over walk into the Grinder
    /// and are sold (Yuri's money for discs, Masterminds, Magnetrons and
    /// towers); a Mastermind also has room again for the next ones.
    fn grind_captured(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        units: &mut Vec<Unit>,
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        self.grinding.retain(|&(id, since)| {
            view.entity(id).is_some() && frame.saturating_sub(since) < 45 * 90
        });
        if tune::GRIND && !base.grinders.is_empty() {
            for unit in units.iter().filter(|u| u.captured && !u.aircraft) {
                if self.grinding.iter().any(|&(id, _)| id == unit.id) {
                    continue;
                }
                let Some(&(grinder, _)) = base
                    .grinders
                    .iter()
                    .min_by_key(|(id, at)| (dist2(*at, unit.at), *id))
                else {
                    continue;
                };
                self.grinding.push((unit.id, frame));
                self.army.retain(|&id| id != unit.id);
                orders.push(Command::MoveToObject {
                    entity_id: unit.id,
                    target_id: grinder,
                });
            }
        }
        units.retain(|u| !self.grinding.iter().any(|&(id, _)| id == u.id));
    }

    /// Tesla Troopers stand by the Tesla Coils and charge them (a charged
    /// coil reaches further and fires without power); Rocketeers guard the
    /// front of the base.
    fn base_guards(
        &mut self,
        view: &BotView<'_>,
        yard: (u16, u16),
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        let mut coils: Vec<(u64, (u16, u16))> = view
            .own()
            .filter(|e| is_structure(e) && view.name(e.type_ref()) == "TESLA")
            .map(|e| (e.stable_id(), cell(e)))
            .collect();
        coils.sort_unstable();
        let mut troopers: Vec<&Unit> = units.iter().filter(|u| u.trooper).collect();
        troopers.sort_by_key(|u| u.id);
        self.charging.retain(|&(t, c)| {
            troopers.iter().any(|u| u.id == t) && coils.iter().any(|&(id, _)| id == c)
        });
        if !coils.is_empty() {
            for unit in &troopers {
                // TROOPERS_PER_COIL a coil (a coil charged by three is
                // supercharged): a trooper keeps its coil, a new one takes
                // the coil with the fewest (then the nearest).
                let coil = match self.charging.iter().find(|&&(t, _)| t == unit.id) {
                    Some(&(_, coil)) => coil,
                    None => {
                        let Some(&(coil, _)) = coils
                            .iter()
                            .filter(|&&(c, _)| {
                                self.charging.iter().filter(|&&(_, x)| x == c).count()
                                    < tune::TROOPERS_PER_COIL as usize
                            })
                            .min_by_key(|&&(c, at)| {
                                (
                                    self.charging.iter().filter(|&&(_, x)| x == c).count(),
                                    dist2(at, unit.at),
                                    c,
                                )
                            })
                        else {
                            continue;
                        };
                        coil
                    }
                };
                // Once, and again now and then (a fight pulls them away).
                let ordered = self.charging.contains(&(unit.id, coil));
                if ordered && (unit.moving || frame % 900 >= THINK_EVERY) {
                    continue;
                }
                if !ordered {
                    self.charging.retain(|&(t, _)| t != unit.id);
                    self.charging.push((unit.id, coil));
                }
                orders.push(Command::Attack {
                    attacker_id: unit.id,
                    target_id: coil,
                });
            }
        }
        if frame % 450 < THINK_EVERY {
            let staging = self.staging(view, yard);
            for unit in units.iter().filter(|u| u.rocketeer) {
                orders.push(Command::Guard {
                    entity_id: unit.id,
                    target: Some(TargetKind::Cell(staging.0, staging.1)),
                });
            }
        }
    }

    /// Kiting: a long-range unit of the army with an armed enemy closer than
    /// its own reach allows steps back, so it fires from its full range and
    /// takes no fire from shorter guns.
    fn kite(&mut self, view: &BotView<'_>, units: &[Unit], orders: &mut Vec<Command>) {
        if !tune::KITE {
            return;
        }
        let near = tune::KITE_NEAR;
        let enemies: Vec<(u16, u16)> = view
            .enemy_objects()
            .filter(|e| !is_structure(e) && !flies(view, e) && armed(view, e))
            .map(cell)
            .collect();
        if enemies.is_empty() {
            return;
        }
        let (mw, mh) = view.map_size();
        for unit in units
            .iter()
            .filter(|u| u.siege && !u.deployed && self.army.contains(&u.id))
        {
            let Some(&enemy) = enemies
                .iter()
                .filter(|&&e| dist2(e, unit.at) <= near * near)
                .min_by_key(|&&e| (dist2(e, unit.at), e))
            else {
                continue;
            };
            let away = (
                (2 * i32::from(unit.at.0) - i32::from(enemy.0)).clamp(1, i32::from(mw) - 2) as u16,
                (2 * i32::from(unit.at.1) - i32::from(enemy.1)).clamp(1, i32::from(mh) - 2) as u16,
            );
            let step = toward_dist(unit.at, away, 4);
            self.siege_orders.retain(|&(u, _)| u != unit.id);
            orders.push(Command::Move {
                entity_id: unit.id,
                target_rx: step.0,
                target_ry: step.1,
            });
        }
    }

    /// Deploys or undeploys `id` (a GI, a Siege Chopper), at most once in
    /// 150 frames: the animation takes a while, and a second toggle meanwhile
    /// would turn it back.
    fn toggle_deploy(&mut self, id: u64, infantry: bool, frame: u32, orders: &mut Vec<Command>) {
        self.toggled
            .retain(|&(_, at)| frame.saturating_sub(at) < 150);
        if self.toggled.iter().any(|&(u, _)| u == id) {
            return;
        }
        self.toggled.push((id, frame));
        orders.push(if infantry {
            Command::ToggleInfantryDeploy { entity_id: id }
        } else {
            Command::DeployMcv { entity_id: id }
        });
    }

    /// Badly damaged vehicles leave the fight for the nearest Service
    /// Depot and take no other order until mended (or two and a half
    /// minutes passed). Without a depot (Yuri has none) they only stay home
    /// (`attack` leaves them out of waves).
    fn repair_units(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        units: &mut Vec<Unit>,
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        let low = tune::REPAIR_PCT;
        let health = |id: u64| {
            view.entity(id).and_then(|e| {
                let strength = view.object_type(e.type_ref())?.strength.max(1);
                Some(e.health.current() * 100 / strength)
            })
        };
        self.repairing.retain(|&(id, since)| {
            health(id).is_some_and(|h| h < 95) && frame.saturating_sub(since) < 45 * 150
        });
        if low > 0 && !base.depots.is_empty() {
            for unit in units.iter() {
                if unit.infantry
                    || unit.aircraft
                    || unit.health >= low
                    || self.repairing.iter().any(|&(id, _)| id == unit.id)
                {
                    continue;
                }
                // A landed Siege Chopper takes off first.
                if unit.deployed {
                    self.toggle_deploy(unit.id, false, frame, orders);
                    continue;
                }
                let Some(&(depot, _)) = base
                    .depots
                    .iter()
                    .min_by_key(|(id, at)| (dist2(*at, unit.at), *id))
                else {
                    continue;
                };
                self.repairing.push((unit.id, frame));
                self.army.retain(|&id| id != unit.id);
                self.focus.retain(|&(id, _)| id != unit.id);
                self.siege_orders.retain(|&(id, _)| id != unit.id);
                self.disc_orders.retain(|&(id, _)| id != unit.id);
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "repair f{frame} unit {} at {:?} health {}%",
                        unit.id,
                        unit.at,
                        unit.health
                    );
                }
                orders.push(Command::MoveToObject {
                    entity_id: unit.id,
                    target_id: depot,
                });
            }
        } else if low > 0 {
            // No depot: the badly hurt leave the attack and come home.
            let home = base.yard.unwrap_or(base.center);
            for unit in units.iter() {
                if unit.infantry || unit.health >= low || !self.army.contains(&unit.id) {
                    continue;
                }
                self.army.retain(|&id| id != unit.id);
                self.siege_orders.retain(|&(id, _)| id != unit.id);
                if unit.deployed && unit.siege {
                    self.toggle_deploy(unit.id, false, frame, orders);
                    continue;
                }
                orders.push(Command::Move {
                    entity_id: unit.id,
                    target_rx: home.0 + 2,
                    target_ry: home.1 + 2,
                });
            }
        }
        self.notes.repairing = self.repairing.len();
        units.retain(|u| !self.repairing.iter().any(|&(id, _)| id == u.id));
    }

    /// GIs and Guardian GIs hold posts around the base, deployed whenever
    /// they are not moving: half in front of the base toward the enemy,
    /// half around the yard.
    fn hold_ground(
        &mut self,
        view: &BotView<'_>,
        yard: (u16, u16),
        units: &[Unit],
        threatened: bool,
        orders: &mut Vec<Command>,
    ) {
        const POSTS: [(i32, i32); 13] = [
            (0, 0),
            (2, 0),
            (0, 2),
            (-2, 0),
            (0, -2),
            (2, 2),
            (-2, 2),
            (2, -2),
            (-2, -2),
            (4, 0),
            (0, 4),
            (-4, 0),
            (0, -4),
        ];
        const YARD_POSTS: [(i32, i32); 8] = [
            (5, 2),
            (2, 5),
            (-2, 2),
            (2, -2),
            (5, 5),
            (-2, 5),
            (5, -2),
            (-2, -2),
        ];
        let frame = view.frame();
        let staging = self.staging(view, yard);
        let mut gis: Vec<&Unit> = units.iter().filter(|u| u.deployer).collect();
        gis.sort_by_key(|u| u.id);
        self.notes.deployed = (gis.iter().filter(|u| u.deployed).count(), gis.len());
        let (mw, mh) = view.map_size();
        let at = |anchor: (u16, u16), (dx, dy): (i32, i32)| -> (u16, u16) {
            (
                (i32::from(anchor.0) + dx).clamp(1, i32::from(mw) - 2) as u16,
                (i32::from(anchor.1) + dy).clamp(1, i32::from(mh) - 2) as u16,
            )
        };
        for (i, unit) in gis.iter().enumerate() {
            let k = i / 2;
            let post = if i % 2 == 0 || !tune::YARD_GIS {
                at(staging, POSTS[k % POSTS.len()])
            } else {
                at(yard, YARD_POSTS[k % YARD_POSTS.len()])
            };
            let d = dist2(unit.at, post);
            if unit.deployed {
                // Pulled away by a fight (an engineer chase): back once calm.
                if d > 8 * 8 && !threatened && frame % 450 < THINK_EVERY {
                    self.toggle_deploy(unit.id, true, frame, orders);
                }
                continue;
            }
            if unit.moving {
                continue;
            }
            if d > 3 * 3 && !threatened {
                if frame % 180 < THINK_EVERY {
                    orders.push(Command::Move {
                        entity_id: unit.id,
                        target_rx: post.0,
                        target_ry: post.1,
                    });
                }
            } else {
                self.toggle_deploy(unit.id, true, frame, orders);
            }
        }
    }

    /// The siege: the army's long-range units (`SIEGE`) shoot the enemy
    /// defense nearest them from their own reach, beyond most defenses'
    /// (Prism Tank 10 cells, a landed Siege Chopper 12, against a Tesla
    /// Coil's 7, a Prism Tower's 8), while the rest of the army holds a
    /// screen a few cells in front of them and fights whatever comes. Only
    /// while such a defense stands within 16 cells of them; returns whether
    /// the army besieges.
    fn siege(
        &mut self,
        view: &BotView<'_>,
        units: &[Unit],
        cells: &[(u64, (u16, u16), bool)],
        goal: (u16, u16),
        orders: &mut Vec<Command>,
    ) -> bool {
        let frame = view.frame();
        let siege: Vec<&Unit> = units
            .iter()
            .filter(|u| u.siege && cells.iter().any(|c| c.0 == u.id))
            .collect();
        if siege.is_empty() {
            self.siege_orders.clear();
            return false;
        }
        let mut xs: Vec<u16> = siege.iter().map(|u| u.at.0).collect();
        let mut ys: Vec<u16> = siege.iter().map(|u| u.at.1).collect();
        xs.sort_unstable();
        ys.sort_unstable();
        let center = (xs[xs.len() / 2], ys[ys.len() / 2]);
        let reach = tune::SIEGE_REACH;
        // The defense in the way: nearest the long-range units, ahead of
        // them (not behind, toward home).
        let ahead = dist2(center, goal).isqrt();
        let Some(defense) = view
            .enemy_objects()
            .filter(|e| is_structure(e) && is_defense(view, e))
            .filter(|e| dist2(cell(e), goal).isqrt() <= ahead + 4)
            .map(|e| (dist2(cell(e), center), e.stable_id(), e))
            .filter(|&(d, _, _)| d <= reach * reach)
            .min_by_key(|&(d, id, _)| (d, id))
            .map(|(_, _, e)| e)
        else {
            self.siege_orders.clear();
            return false;
        };
        let target = defense.stable_id();
        let at = cell(defense);
        self.notes.siege = Some((
            view.object_type(defense.type_ref())
                .and_then(|t| t.name.clone())
                .unwrap_or_else(|| view.name(defense.type_ref()).to_owned()),
            at,
        ));
        self.siege_orders
            .retain(|&(u, t)| t == target && siege.iter().any(|s| s.id == u));
        for unit in &siege {
            let d = dist2(unit.at, at).isqrt();
            let ordered = self.siege_orders.iter().any(|&(u, _)| u == unit.id);
            let chopper = view
                .entity(unit.id)
                .and_then(|e| view.object_type(e.type_ref()))
                .is_some_and(|t| t.is_simple_deployer);
            if chopper {
                // Lands 10 cells out and shells it (160mm, 12 cells).
                if unit.deployed {
                    if d > 12 {
                        self.toggle_deploy(unit.id, false, frame, orders);
                    } else if !ordered {
                        self.siege_orders.push((unit.id, target));
                        orders.push(Command::Attack {
                            attacker_id: unit.id,
                            target_id: target,
                        });
                    }
                    continue;
                }
                if d <= 11 && !unit.moving {
                    self.toggle_deploy(unit.id, false, frame, orders);
                } else if !unit.moving || d > 12 && frame % 90 < THINK_EVERY {
                    let stand = toward_dist(at, unit.at, 10);
                    orders.push(Command::Move {
                        entity_id: unit.id,
                        target_rx: stand.0,
                        target_ry: stand.1,
                    });
                }
                continue;
            }
            // A Prism Tank or Magnetron drives into its own range and fires.
            if !ordered || !unit.moving && frame % 450 < THINK_EVERY {
                self.siege_orders.retain(|&(u, _)| u != unit.id);
                self.siege_orders.push((unit.id, target));
                orders.push(Command::Attack {
                    attacker_id: unit.id,
                    target_id: target,
                });
            }
        }
        // The screen: a few cells in front of the long-range units.
        if frame % 90 < THINK_EVERY {
            // Never inside the defense's reach: units lost to a tower
            // are lost for nothing.
            let d = dist2(center, at).isqrt();
            let keep = reach_of(view, defense.type_ref()) + 2;
            let screen = toward_dist(center, at, tune::SCREEN.min(d - keep).max(0));
            for &(id, c, _) in cells {
                if siege.iter().any(|s| s.id == id) || dist2(c, screen) <= 2 * 2 {
                    continue;
                }
                orders.push(Command::AttackMove {
                    entity_id: id,
                    target: None,
                    destination: Some(TargetKind::Cell(screen.0, screen.1)),
                });
            }
        }
        true
    }

    /// Floating Discs (Yuri): gathered at home, they set out together. One
    /// drains the nearest seen power plant, which leaves the whole enemy
    /// House without power and its anti-air dark; the others shoot that
    /// anti-air (force-fired: a disc ordered onto a Drainable building would
    /// drain it rather than shoot, `0x006F3558`), then the other defenses,
    /// then the rest. A disc below half health goes home.
    fn discs(
        &mut self,
        view: &BotView<'_>,
        yard: (u16, u16),
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        let discs: Vec<&Unit> = units.iter().filter(|u| u.disc).collect();
        if log::log_enabled!(log::Level::Debug) && view.frame() % 900 < THINK_EVERY {
            let all: Vec<String> = view
                .own()
                .filter(|e| view.name(e.type_ref()) == DISC)
                .map(|e| {
                    format!(
                        "{}@{:?}hp{}{}",
                        e.stable_id(),
                        cell(e),
                        e.health.current(),
                        if discs.iter().any(|u| u.id == e.stable_id()) {
                            ""
                        } else {
                            "-out"
                        }
                    )
                })
                .collect();
            log::debug!("discs f{} {}", view.frame(), all.join(" "));
        }
        self.disc_orders
            .retain(|&(d, t)| discs.iter().any(|u| u.id == d) && view.entity(t).is_some());
        self.notes.discs = None;
        if discs.is_empty() {
            self.disc_raid = false;
            return;
        }
        let low = tune::DISC_LOW;
        let home = (yard.0 + 2, yard.1 + 2);
        let (fit, hurt): (Vec<&Unit>, Vec<&Unit>) = discs.iter().partition(|u| u.health >= low);
        for unit in &hurt {
            self.disc_orders.retain(|&(d, _)| d != unit.id);
            if dist2(unit.at, home) > 6 * 6 && !unit.moving {
                orders.push(Command::Move {
                    entity_id: unit.id,
                    target_rx: home.0,
                    target_ry: home.1,
                });
            }
        }
        if !self.disc_raid && fit.len() >= tune::DISC_GROUP as usize {
            self.disc_raid = true;
        }
        if fit.is_empty() {
            self.disc_raid = false;
        }
        if !self.disc_raid {
            self.notes.discs = Some(format!("{} discs wait at home", discs.len()));
            if frame % 270 < THINK_EVERY {
                for unit in fit.iter().filter(|u| dist2(u.at, home) > 6 * 6) {
                    orders.push(Command::Move {
                        entity_id: unit.id,
                        target_rx: home.0,
                        target_ry: home.1,
                    });
                }
            }
            return;
        }
        if frame % 24 >= THINK_EVERY {
            return;
        }
        let draining = fit.iter().find_map(|u| {
            let target = view.entity(u.id)?.drain.target()?;
            view.entity(target)
                .filter(|e| view.object_type(e.type_ref()).is_some_and(|t| t.power > 0))
                .map(|_| (u.id, target))
        });
        let mut xs: Vec<u16> = fit.iter().map(|u| u.at.0).collect();
        let mut ys: Vec<u16> = fit.iter().map(|u| u.at.1).collect();
        xs.sort_unstable();
        ys.sort_unstable();
        let center = (xs[xs.len() / 2], ys[ys.len() / 2]);
        let structures: Vec<&crate::sim::game_entity::GameEntity> =
            view.enemy_objects().filter(|e| is_structure(e)).collect();
        let nearest = |pred: &dyn Fn(&ObjectType) -> bool, from: (u16, u16)| {
            structures
                .iter()
                .filter(|e| view.object_type(e.type_ref()).is_some_and(pred))
                .min_by_key(|e| (dist2(cell(e), from), e.stable_id()))
                .copied()
        };
        let plant = nearest(&|t: &ObjectType| t.power > 0 && t.drainable, center);
        let anti_air = |t: &ObjectType| AA_DEFENSES.contains(&t.id.as_str());
        let mut drainer = draining.map(|(d, _)| d);
        if drainer.is_none()
            && let Some(plant) = plant
        {
            // The healthiest disc drains.
            let pick = fit
                .iter()
                .max_by_key(|u| (u.health, std::cmp::Reverse(u.id)))
                .map(|u| u.id);
            if let Some(pick) = pick {
                drainer = Some(pick);
                if !self.disc_orders.contains(&(pick, plant.stable_id())) {
                    self.disc_orders.retain(|&(d, _)| d != pick);
                    self.disc_orders.push((pick, plant.stable_id()));
                    orders.push(Command::Attack {
                        attacker_id: pick,
                        target_id: plant.stable_id(),
                    });
                }
            }
        }
        let focus = plant.map_or(center, cell);
        // The others: anti-air first, near the drained plant; then the
        // other defenses; then anything.
        let shoot = nearest(&anti_air, focus)
            .or_else(|| nearest(&|t: &ObjectType| t.is_base_defense, focus))
            .or_else(|| nearest(&|_| true, focus));
        self.notes.discs = Some(match (draining, shoot) {
            (Some(_), Some(t)) => format!(
                "{} discs: power drained, shooting {}",
                fit.len(),
                view.name(t.type_ref())
            ),
            (None, _) if plant.is_some() => format!("{} discs: draining a power plant", fit.len()),
            _ => format!("{} discs: looking for the enemy", fit.len()),
        });
        for unit in fit.iter().filter(|u| Some(u.id) != drainer) {
            match shoot {
                // Until the power is down only the drainer goes in; the
                // rest wait out of the anti-air's reach.
                Some(t) if draining.is_some() || plant.is_none() => {
                    if self.disc_orders.contains(&(unit.id, t.stable_id())) {
                        continue;
                    }
                    self.disc_orders.retain(|&(d, _)| d != unit.id);
                    self.disc_orders.push((unit.id, t.stable_id()));
                    let (x, y) = cell(t);
                    orders.push(Command::ForceAttackCell {
                        attacker_id: unit.id,
                        target_rx: x,
                        target_ry: y,
                    });
                }
                _ => {
                    let wait = if plant.is_some() {
                        toward_dist(focus, center, 14)
                    } else {
                        self.nearest_enemy_base(view, yard).unwrap_or(center)
                    };
                    if dist2(unit.at, wait) > 3 * 3 && !unit.moving {
                        orders.push(Command::Move {
                            entity_id: unit.id,
                            target_rx: wait.0,
                            target_ry: wait.1,
                        });
                    }
                }
            }
        }
        if drainer.is_none() && plant.is_none() && shoot.is_none() {
            // Nothing seen: the discs look where the enemy should be.
            let goal = self.nearest_enemy_base(view, yard).unwrap_or(center);
            for unit in &fit {
                if !unit.moving {
                    orders.push(Command::Move {
                        entity_id: unit.id,
                        target_rx: goal.0,
                        target_ry: goal.1,
                    });
                }
            }
        }
    }

    /// v45e (knob ZEPS, ZEP_GROUP): Kirovs gather at home and bomb the
    /// enemy Construction Yard together, then its factories: Kirovs were
    /// the stock AI's commonest yard killer against Vanguard.
    fn kirovs(&mut self, view: &BotView<'_>, yard: (u16, u16), orders: &mut Vec<Command>) {
        let frame = view.frame();
        if frame % 24 >= THINK_EVERY {
            return;
        }
        let zeps: Vec<(u64, (u16, u16), i32)> = view
            .own()
            .filter(|e| view.name(e.type_ref()) == "ZEP")
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                Some((
                    e.stable_id(),
                    cell(e),
                    e.health.current() * 100 / t.strength.max(1),
                ))
            })
            .collect();
        self.zep_orders
            .retain(|&(z, t)| zeps.iter().any(|u| u.0 == z) && view.entity(t).is_some());
        if zeps.is_empty() {
            self.zep_raid = false;
            return;
        }
        if !self.zep_raid && zeps.len() >= tune::ZEP_GROUP as usize {
            self.zep_raid = true;
        }
        let home = (yard.0 + 2, yard.1 + 2);
        if !self.zep_raid {
            if frame % 270 < THINK_EVERY {
                for &(id, _, _) in zeps.iter().filter(|z| dist2(z.1, home) > 6 * 6) {
                    orders.push(Command::Move {
                        entity_id: id,
                        target_rx: home.0,
                        target_ry: home.1,
                    });
                }
            }
            return;
        }
        let mut xs: Vec<u16> = zeps.iter().map(|z| z.1.0).collect();
        let mut ys: Vec<u16> = zeps.iter().map(|z| z.1.1).collect();
        xs.sort_unstable();
        ys.sort_unstable();
        let center = (xs[xs.len() / 2], ys[ys.len() / 2]);
        let Some(target) = target_from(view, center) else {
            let goal = self.nearest_enemy_base(view, yard).unwrap_or(center);
            for &(id, at, _) in &zeps {
                if dist2(at, goal) > 4 * 4 && frame % 270 < THINK_EVERY {
                    orders.push(Command::Move {
                        entity_id: id,
                        target_rx: goal.0,
                        target_ry: goal.1,
                    });
                }
            }
            return;
        };
        for &(id, _, _) in &zeps {
            if self.zep_orders.contains(&(id, target.stable_id())) {
                continue;
            }
            self.zep_orders.retain(|&(z, _)| z != id);
            self.zep_orders.push((id, target.stable_id()));
            orders.push(Command::Attack {
                attacker_id: id,
                target_id: target.stable_id(),
            });
        }
    }

    /// The garrison: dogs and the first soldiers area-guard the yard, where
    /// enemy engineers and their transports come for it.
    fn garrison(&self, units: &[Unit]) -> Vec<u64> {
        let scout = self.scout.map(|(id, _)| id);
        let mut garrison: Vec<u64> = units
            .iter()
            .filter(|u| u.dog && Some(u.id) != scout)
            .map(|u| u.id)
            .collect();
        garrison.extend(
            units
                .iter()
                .filter(|u| {
                    u.infantry
                        && !u.dog
                        && !u.prime
                        && !u.stays_home()
                        && !self.army.contains(&u.id)
                })
                .map(|u| u.id)
                .take(6),
        );
        // A few tanks stay home too: a raid of two or three while the
        // army is out otherwise finds the yard bare.
        garrison.extend(
            units
                .iter()
                .filter(|u| !u.infantry && !u.aircraft && !u.aa && !self.army.contains(&u.id))
                .map(|u| u.id)
                .take(tune::HOME_TANKS.max(0) as usize),
        );
        garrison
    }

    /// Everything at home meets enemies near the base: engineers and loaded
    /// transports first, by name, then the nearest intruder. Returns whether
    /// the base is under attack.
    fn defend(
        &mut self,
        view: &BotView<'_>,
        base: Option<&Base>,
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) -> bool {
        let anchors: Vec<(u16, u16)> = match base {
            Some(base) => base.buildings.iter().map(|&(_, at)| at).collect(),
            None => units.iter().map(|u| u.at).take(1).collect(),
        };
        if anchors.is_empty() {
            return false;
        }
        let frame = view.frame();
        let garrison = self.garrison(units);
        if let Some(yard) = base.and_then(|b| b.yard)
            && frame % 450 < THINK_EVERY
        {
            for &id in &garrison {
                orders.push(Command::Guard {
                    entity_id: id,
                    target: Some(TargetKind::Cell(yard.0 + 1, yard.1 + 1)),
                });
            }
        }
        let alarm = BASE_ALARM * BASE_ALARM;
        let yard = base.and_then(|b| b.yard);
        let eng_alarm = tune::ENG_ALARM.pow(2);
        // Anti-air meets armed aircraft coming for the base.
        let air_alarm = tune::AIR_ALARM.pow(2);
        let flyer = view
            .enemy_objects()
            .filter(|e| flies(view, e) && armed(view, e))
            .filter_map(|e| {
                let at = cell(e);
                let d = anchors.iter().map(|&a| dist2(a, at)).min()?;
                (d <= air_alarm).then_some((d, e.stable_id()))
            })
            .min();
        // Without aircraft about, anti-air waits by the yard.
        if flyer.is_none()
            && frame % 270 < THINK_EVERY
            && let Some(yard) = base.and_then(|b| b.yard)
        {
            for unit in units
                .iter()
                .filter(|u| u.aa && !self.army.contains(&u.id) && dist2(u.at, yard) > 6 * 6)
            {
                orders.push(Command::Move {
                    entity_id: unit.id,
                    target_rx: yard.0 + 2,
                    target_ry: yard.1 + 3,
                });
            }
        }
        if let Some((_, flyer)) = flyer
            && frame % 48 < THINK_EVERY
        {
            for unit in units.iter().filter(|u| u.aa && !self.army.contains(&u.id)) {
                orders.push(Command::Attack {
                    attacker_id: unit.id,
                    target_id: flyer,
                });
            }
        }
        let mut intruders: Vec<(i32, u64, (u16, u16), bool)> = view
            .enemy_objects()
            .filter(|e| !is_structure(e) && !flies(view, e))
            .filter_map(|e| {
                let at = cell(e);
                let d = anchors.iter().map(|&a| dist2(a, at)).min()?;
                let ty = view.object_type(e.type_ref());
                let urgent = ty.is_some_and(|t| t.engineer || t.passengers > 0);
                // Engineer drops are met on the way to the yard (v42): a
                // transport killed at the yard lets its engineers walk in.
                let far = urgent && yard.is_some_and(|y| dist2(y, at) <= eng_alarm);
                (d <= alarm || far).then_some((d, e.stable_id(), at, urgent))
            })
            .collect();
        // v45b (knob HARV_SKIP): an enemy miner on ore near the base is no
        // attack on it (v44 rounds sat "under attack" by miners for minutes,
        // and no wave goes out then): two tanks go for it, the rest carry on.
        if tune::HARV_SKIP {
            let miner = |id: u64| {
                view.entity(id)
                    .and_then(|e| view.object_type(e.type_ref()))
                    .is_some_and(|t| t.harvester || t.resource_gatherer || t.slaved)
            };
            let (miners, rest): (Vec<_>, Vec<_>) = intruders
                .iter()
                .copied()
                .partition(|&(_, id, _, _)| miner(id));
            intruders = rest;
            if intruders.is_empty()
                && let Some(&(_, target, at, _)) = miners.iter().min()
                && frame % 90 < THINK_EVERY
            {
                let mut hunters: Vec<&Unit> = units
                    .iter()
                    .filter(|u| {
                        !u.aircraft
                            && !u.infantry
                            && !u.aa
                            && !u.stays_home()
                            && !self.army.contains(&u.id)
                            && !self.joining.contains(&u.id)
                    })
                    .collect();
                hunters.sort_by_key(|u| (dist2(u.at, at), u.id));
                for u in hunters.into_iter().take(2) {
                    orders.push(Command::Attack {
                        attacker_id: u.id,
                        target_id: target,
                    });
                }
            }
        }
        if intruders.is_empty() {
            return false;
        }
        intruders.sort_by_key(|&(d, id, _, urgent)| (!urgent, d, id));
        // An attack on the base heavier than what is home calls the army
        // back (v33: two Apocalypse tanks razed the base while the army
        // stood at the enemy's walls).
        let cost = |id: u64| {
            view.entity(id)
                .and_then(|e| view.object_type(e.type_ref()))
                .map_or(0, |t| t.cost.max(0))
        };
        let threat: i32 = intruders.iter().map(|&(_, id, _, _)| cost(id)).sum();
        let home: i32 = units
            .iter()
            .filter(|u| !u.aircraft && !self.army.contains(&u.id))
            .map(|u| cost(u.id))
            .sum();
        if !self.army.is_empty() && threat > home && tune::RECALL {
            self.army.clear();
            self.joining.clear();
        }
        // Iron Curtain discipline (v42, knob IC_DODGE): shots at an
        // invulnerable unit are wasted, and its 750 frames end soon. Fight
        // the others; with only shielded ones about, step out of their way.
        if tune::IC_DODGE {
            let shielded = |id: u64| {
                view.entity(id).is_some_and(|e| {
                    crate::sim::superweapon::invulnerability::is_invulnerable(
                        e.invulnerability.as_ref(),
                        frame,
                    )
                })
            };
            let (ic, open): (Vec<_>, Vec<_>) = intruders
                .iter()
                .copied()
                .partition(|&(_, id, _, _)| shielded(id));
            if open.is_empty() {
                if frame % 48 < THINK_EVERY {
                    let reach = tune::IC_REACH.pow(2);
                    for unit in units
                        .iter()
                        .filter(|u| !u.aircraft && !u.stays_home() && !self.army.contains(&u.id))
                    {
                        let Some(&(_, _, from, _)) =
                            ic.iter().min_by_key(|&&(_, _, c, _)| dist2(c, unit.at))
                        else {
                            continue;
                        };
                        if dist2(from, unit.at) > reach {
                            continue;
                        }
                        let away = |a: u16, b: u16| -> u16 {
                            let d = (i32::from(a) - i32::from(b)).signum() * 10;
                            (i32::from(a) + d).max(1) as u16
                        };
                        orders.push(Command::Move {
                            entity_id: unit.id,
                            target_rx: away(unit.at.0, from.0),
                            target_ry: away(unit.at.1, from.1),
                        });
                    }
                }
                return true;
            }
            if !ic.is_empty() {
                intruders = open;
            }
        }
        let (_, target, at, urgent) = intruders[0];
        // Focus fire on the intruders in reach (not while an engineer or a
        // loaded transport needs everyone).
        let focused = if urgent || !tune::FOCUS {
            Vec::new()
        } else {
            let shooters: Vec<&Unit> = units
                .iter()
                .filter(|u| !u.aircraft && !u.aa && !self.army.contains(&u.id))
                .collect();
            let enemies: Vec<u64> = intruders.iter().map(|&(_, id, _, _)| id).collect();
            self.focus_fire(view, &shooters, &enemies, orders)
        };
        if !urgent && frame % 60 >= THINK_EVERY {
            return true;
        }
        // v43 (knob DEF_HOLD): the home army meets a raid at the edge of
        // the base, in reach of its defenses, instead of running out to it
        // one by one (95% of the losses were at home).
        let hold = tune::DEF_HOLD;
        let meet = if hold > 0 {
            anchors
                .iter()
                .copied()
                .min_by_key(|&a| (dist2(a, at), a))
                .map_or(at, |a| toward_dist(a, at, hold))
        } else {
            at
        };
        for unit in units
            .iter()
            .filter(|u| !u.aircraft && !self.army.contains(&u.id) && !focused.contains(&u.id))
        {
            if urgent && unit.disc {
                continue;
            }
            if urgent {
                orders.push(Command::Attack {
                    attacker_id: unit.id,
                    target_id: target,
                });
            } else if !garrison.contains(&unit.id) && !unit.stays_home() && !unit.disc {
                orders.push(Command::AttackMove {
                    entity_id: unit.id,
                    target: None,
                    destination: Some(TargetKind::Cell(meet.0, meet.1)),
                });
            }
        }
        true
    }

    /// Focus fire (knob FOCUS, off: v40 traded worse with it, kills/losses
    /// at minute 8 0.88 vs 1.12): every shooter with an enemy in reach (its longest weapon
    /// range plus two cells) shoots the one with the least health left, and
    /// keeps that target until it dies or leaves reach: concentrated fire
    /// kills sooner, so less fire comes back (each unit choosing its own
    /// target spreads the damage). Returns the shooters given a target.
    fn focus_fire(
        &mut self,
        view: &BotView<'_>,
        shooters: &[&Unit],
        enemies: &[u64],
        orders: &mut Vec<Command>,
    ) -> Vec<u64> {
        let targets: Vec<(u64, (u16, u16), i32)> = enemies
            .iter()
            .filter_map(|&id| view.entity(id))
            .filter(|e| !flies(view, e) && !is_structure(e))
            .map(|e| (e.stable_id(), cell(e), e.health.current()))
            .collect();
        let alive = |id: u64| targets.iter().any(|t| t.0 == id);
        self.focus.retain(|&(_, t)| alive(t));
        let mut given = Vec::new();
        for unit in shooters {
            let Some(reach) = view
                .entity(unit.id)
                .and_then(|e| view.object_type(e.type_ref()))
                .map(|t| {
                    t.weapon_list
                        .iter()
                        .flatten()
                        .filter_map(|w| view.rules.weapon(w))
                        .map(|w| w.range_leptons / 256)
                        .max()
                        .unwrap_or(0)
                        + 2
                })
            else {
                continue;
            };
            let in_reach = |at: (u16, u16)| dist2(unit.at, at) <= reach * reach;
            let current = self
                .focus
                .iter()
                .find(|&&(u, _)| u == unit.id)
                .map(|&(_, t)| t)
                .filter(|&t| targets.iter().any(|x| x.0 == t && in_reach(x.1)));
            let pick = current.or_else(|| {
                targets
                    .iter()
                    .filter(|t| in_reach(t.1))
                    .min_by_key(|t| (t.2, t.0))
                    .map(|t| t.0)
            });
            let Some(pick) = pick else {
                continue;
            };
            given.push(unit.id);
            if current != Some(pick) {
                self.focus.retain(|&(u, _)| u != unit.id);
                self.focus.push((unit.id, pick));
                orders.push(Command::Attack {
                    attacker_id: unit.id,
                    target_id: pick,
                });
            }
        }
        given
    }

    /// The Brutal AI's own trick, turned on it: engineers, escorted by
    /// everything at hand, capture the nearest enemy Construction Yard (then
    /// its factories); every captured building far from home is sold.
    fn raid(
        &mut self,
        view: &BotView<'_>,
        base: &Base,
        yard: (u16, u16),
        options: &[BuildOption],
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        // Sell what was captured (it stands far from home).
        for &(id, at) in &base.buildings {
            if dist2(at, yard) > 30 * 30 {
                orders.push(Command::SellBuilding { entity_id: id });
            }
        }
        let engineers: Vec<u64> = view
            .own()
            .filter(|e| view.object_type(e.type_ref()).is_some_and(|t| t.engineer))
            .map(|e| e.stable_id())
            .collect();
        let infantry_busy = view
            .factory(ProductionCategory::Infantry)
            .is_some_and(|f| f.object.is_some() || !f.queue.is_empty());
        if frame < 45 * 60 * 10
            && engineers.len() < 4
            && !infantry_busy
            && base.count(Role::Barracks) > 0
        {
            let engineer = options.iter().find(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Infantry
                    && view.object_type(o.type_id).is_some_and(|t| t.engineer)
            });
            if let Some(engineer) = engineer {
                orders.push(Command::QueueProduction {
                    type_id: engineer.type_id,
                });
            }
        }
        if engineers.len() < 3 || frame.saturating_sub(self.last_raid) < 90 {
            return;
        }
        // The target: the nearest enemy yard, else factory.
        let target = view
            .enemy_objects()
            .filter(|e| is_structure(e))
            .filter_map(|e| {
                let ty = view.object_type(e.type_ref())?;
                let rank = if ty.construction_yard {
                    0
                } else if ty.factory_type().is_some() {
                    1
                } else {
                    return None;
                };
                Some((rank, dist2(cell(e), yard), e.stable_id(), cell(e)))
            })
            .min();
        let Some((_, _, target, at)) = target else {
            return;
        };
        self.last_raid = frame;
        if self.raids == 0 {
            self.raids = 1;
        }
        for &id in &engineers {
            orders.push(Command::CaptureBuilding {
                engineer_id: id,
                target_building_id: target,
            });
        }
        let garrison = self.garrison(units);
        for unit in units
            .iter()
            .filter(|u| !u.aircraft && !garrison.contains(&u.id))
        {
            if !self.army.contains(&unit.id) {
                self.army.push(unit.id);
            }
            orders.push(Command::AttackMove {
                entity_id: unit.id,
                target: None,
                destination: Some(TargetKind::Cell(at.0, at.1)),
            });
        }
    }

    /// The army: everything that fights gathers in front of the base and
    /// sets out together once it is worth enough; it moves as one body
    /// (the front waits for the rest), takes the nearest enemy yard and
    /// factories first, and comes home when it has lost most of its worth.
    fn attack(
        &mut self,
        view: &BotView<'_>,
        yard: (u16, u16),
        units: &[Unit],
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        self.army.retain(|id| view.entity(*id).is_some());
        self.joining.retain(|id| view.entity(*id).is_some());
        if self.army.is_empty() {
            self.joining.clear();
        }
        let worth = |id: u64| -> i32 {
            view.entity(id)
                .and_then(|e| view.object_type(e.type_ref()))
                .map_or(0, |t| t.cost.max(0))
        };
        let garrison = self.garrison(units);
        let minutes = (frame / 45 / 60) as i32;
        // Playtest, 2026-11-22: "the enemy still had four buildings, nothing
        // else, and the bot attacked none of them for four hours". Mop-up
        // (knob MOPUP, enemy buildings; MOPUP_MIN, minutes): with no armed
        // enemy unit seen and few enemy buildings known (or none: they have
        // to be found), whatever is at home goes and finishes them, anti-air
        // too while no enemy aircraft is about, once it outweighs the
        // defenses there; no wave size, no waiting for long-range units.
        let mop_up = tune::MOPUP > 0
            && minutes >= tune::MOPUP_MIN
            && view
                .enemy_objects()
                .filter(|e| {
                    is_structure(e) && view.object_type(e.type_ref()).is_some_and(|t| !t.wall)
                })
                .count()
                <= tune::MOPUP as usize
            && !view.enemy_objects().any(|e| {
                !is_structure(e)
                    && armed(view, e)
                    && view
                        .object_type(e.type_ref())
                        .is_none_or(|t| !(t.harvester || t.resource_gatherer || t.slaved))
            });
        let air_now = view
            .enemy_objects()
            .any(|e| flies(view, e) && armed(view, e));
        let home: Vec<&Unit> = units
            .iter()
            .filter(|u| {
                !u.aircraft
                    // Anti-air stays home: a lone Kirov at minute 7 was the
                    // commonest yard killer while the IFVs marched with v36's waves.
                    && !(u.aa && tune::AA_HOME && !(mop_up && !air_now))
                    // GIs hold the base; discs raid on their own; the
                    // badly hurt (no depot to mend them) stay home.
                    && !u.stays_home()
                    && !u.disc
                    && u.health >= tune::REPAIR_PCT
                    && !self.army.contains(&u.id)
                    && !self.joining.contains(&u.id)
                    && !garrison.contains(&u.id)
                    && self.scout.is_none_or(|(id, _)| id != u.id)
            })
            .collect();
        let staging = self.staging(view, yard);
        self.notes.gathering = (!home.is_empty()).then(|| {
            let n = home.len() as i32;
            let (sx, sy) = home.iter().fold((0i32, 0i32), |(x, y), u| {
                (x + i32::from(u.at.0), y + i32::from(u.at.1))
            });
            (((sx / n) as u16, (sy / n) as u16), staging)
        });
        if self.army.is_empty() {
            let home_value: i32 = home.iter().map(|u| worth(u.id)).sum();
            let want = (tune::WAVE_VALUE + tune::WAVE_GROW * self.waves as i32).min(tune::WAVE_MAX);
            // A wave smaller than the defenses seen around its target dies
            // on them (v33 sent 7000 credits into 9 bunkers and 7 Tesla
            // coils): it waits until it outweighs them.
            let guard = target_from(view, staging).map_or(0, |t| guard_of(view, cell(t)));
            let want = want.max(guard * tune::GUARD_PCT / 100);
            self.notes.home_value = home_value;
            self.notes.want = want;
            // Right after a superweapon took the enemy's power, a smaller
            // wave goes in before the lights come back.
            let strike = self.strike > 0 && frame.saturating_sub(self.strike) < 45 * 40;
            let want = if strike { want / 2 } else { want };
            // Once long-range units can be built, a wave waits for a few:
            // they do the attacking, the rest guard them.
            let siege_home = home.iter().filter(|u| u.siege).count() as i32;
            let siege_buildable = view
                .build_options()
                .iter()
                .any(|o| o.enabled && SIEGE.contains(&view.name(o.type_id)));
            let siege_ready = !siege_buildable || siege_home >= tune::WAVE_SIEGE;
            let mop = mop_up && home_value * 100 >= guard * tune::GUARD_PCT;
            self.notes.mop_up = mop_up;
            if mop && home_value > 0
                || home_value >= want && home_value > 0 && siege_ready && minutes >= tune::FIRST_MIN
            {
                self.army = home.iter().map(|u| u.id).collect();
                self.launch_value = home_value;
                self.waves += 1;
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!("wave f{frame} n={} worth={home_value}", self.army.len());
                }
                self.last_attack = 0;
            } else {
                // Home units that wandered off (a defense) go back.
                if frame % 270 < THINK_EVERY {
                    for unit in home.iter().filter(|u| dist2(u.at, staging) > 7 * 7) {
                        orders.push(Command::AttackMove {
                            entity_id: unit.id,
                            target: None,
                            destination: Some(TargetKind::Cell(staging.0, staging.1)),
                        });
                    }
                }
                return;
            }
        }
        // v45 (knob REINFORCE): a wave in the field takes what gathered
        // at home since it left, once that is worth a few tanks: waves were
        // rare (98 in 48 games) and the wins came from pressure kept on.
        // They walk to the army's middle first and join it there.
        let reinforce = if mop_up { 1 } else { tune::REINFORCE };
        let army_value: i32 = self.army.iter().map(|&id| worth(id)).sum();
        if reinforce > 0 {
            let fresh: Vec<u64> = home
                .iter()
                .map(|u| u.id)
                .filter(|id| !self.army.contains(id))
                .collect();
            let home_value: i32 = fresh.iter().map(|&id| worth(id)).sum();
            if home_value >= reinforce {
                self.joining.extend(fresh);
                self.launch_value = self.launch_value.max(army_value) + home_value;
            }
            if let Some(center) = self.notes.army_center {
                let mut arrived = Vec::new();
                for &id in &self.joining {
                    let Some(e) = view.entity(id) else { continue };
                    if dist2(cell(e), center) <= 10 * 10 {
                        arrived.push(id);
                    } else if frame % 90 < THINK_EVERY {
                        orders.push(Command::AttackMove {
                            entity_id: id,
                            target: None,
                            destination: Some(TargetKind::Cell(center.0, center.1)),
                        });
                    }
                }
                self.joining.retain(|id| !arrived.contains(id));
                self.army.extend(arrived);
            }
        }
        let value: i32 = self.army.iter().map(|&id| worth(id)).sum();
        self.notes.home_value = value;
        if value * 100 < self.launch_value * tune::RETREAT_PCT {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!("retreat f{frame} n={} worth={value}", self.army.len());
            }
            for id in self.army.clone() {
                if units.iter().any(|u| u.id == id && u.deployed && u.siege) {
                    // A landed Siege Chopper takes off first.
                    self.toggle_deploy(id, false, frame, orders);
                    continue;
                }
                orders.push(Command::Move {
                    entity_id: id,
                    target_rx: staging.0,
                    target_ry: staging.1,
                });
            }
            self.army.clear();
            self.joining.clear();
            return;
        }
        if frame.saturating_sub(self.last_attack) < 90 {
            return;
        }
        self.last_attack = frame;
        let cells: Vec<(u64, (u16, u16), bool)> = self
            .army
            .iter()
            .filter_map(|id| view.entity(*id))
            .map(|e| {
                (
                    e.stable_id(),
                    cell(e),
                    e.category() == EntityCategory::Infantry,
                )
            })
            .collect();
        if cells.is_empty() {
            return;
        }
        // The body's middle: the median cell (stragglers do not drag it).
        let mut xs: Vec<u16> = cells.iter().map(|c| c.1.0).collect();
        let mut ys: Vec<u16> = cells.iter().map(|c| c.1.1).collect();
        xs.sort_unstable();
        ys.sort_unstable();
        let center = (xs[xs.len() / 2], ys[ys.len() / 2]);
        self.notes.army_center = Some(center);
        // Where the army's ground units can drive (one of each movement
        // zone, from where it stands).
        let mut zones: Vec<(crate::rules::locomotor_type::MovementZone, (u16, u16))> = Vec::new();
        for &(id, c, _) in &cells {
            let Some(e) = view.entity(id) else { continue };
            let Some(t) = view.object_type(e.type_ref()) else {
                continue;
            };
            if !flies(view, e) && !zones.iter().any(|z| z.0 == t.movement_zone) {
                zones.push((t.movement_zone, c));
            }
        }
        let drives = |c: (u16, u16)| {
            zones.is_empty() || zones.iter().any(|&(mz, from)| view.can_reach(mz, from, c))
        };
        let Some(target) = reachable_target(view, center, &drives) else {
            // Nothing in sight: the whole body searches.
            self.notes.army_target = None;
            self.notes.searching = Self::search_cell(view, center, &drives);
            if let Some(goal) = self.notes.searching {
                for &(id, _, _) in &cells {
                    orders.push(Command::AttackMove {
                        entity_id: id,
                        target: None,
                        destination: Some(TargetKind::Cell(goal.0, goal.1)),
                    });
                }
            }
            return;
        };
        let at = cell(target);
        self.notes.army_target = Some((
            view.object_type(target.type_ref())
                .and_then(|t| t.name.clone())
                .unwrap_or_else(|| view.name(target.type_ref()).to_owned()),
            at,
        ));
        self.notes.searching = None;
        let mut goals = Vec::new();
        let near = dist2(center, at) <= 10 * 10;
        if log::log_enabled!(log::Level::Debug) && frame % 450 < 90 {
            log::debug!(
                "army f{frame} n={} worth={value} center={center:?} target={}@{at:?}",
                cells.len(),
                view.name(target.type_ref())
            );
            for &(id, c, _) in cells.iter().take(2) {
                if let Some(e) = view.entity(id) {
                    let goal = goal_for(view, &mut goals, e, target, center);
                    log::debug!(
                        "  unit {} {c:?} goal={goal:?} nav={:?}",
                        view.name(e.type_ref()),
                        e.navigation.nav_com()
                    );
                }
            }
        }
        // Long-range units in the army stop short of the first defense in
        // their way and shoot it from beyond its reach; the rest screen them.
        if self.siege(view, units, &cells, at, orders) {
            return;
        }
        self.notes.siege = None;
        // Enemy units near the body first: focus fire on them.
        let focused = if tune::FOCUS {
            let enemies: Vec<u64> = view
                .enemy_objects()
                .filter(|e| !is_structure(e) && armed(view, e))
                .filter(|e| dist2(cell(e), center) <= 14 * 14)
                .map(|e| e.stable_id())
                .collect();
            let shooters: Vec<&Unit> = units
                .iter()
                .filter(|u| cells.iter().any(|c| c.0 == u.id))
                .collect();
            self.focus_fire(view, &shooters, &enemies, orders)
        } else {
            Vec::new()
        };
        // Playtest, 2026-11-14: "keep that entire army out of enemy range while
        // using the long range units (choppers sieged) to slowly kill
        // everything in range. when nothing is in range anymore, slowly
        // advance" (knob CREEP). The army holds a front: while anything is
        // in a member's reach nobody steps on (the shooting is `engage`'s:
        // a move with an enemy in reach becomes an attack on it); with
        // nothing in reach and the body gathered on the front, the front
        // steps CREEP cells toward the target, but not into a seen enemy
        // building's reach while long-range units are along (the siege
        // above shells those from outside it), at most for 30 s.
        if tune::CREEP > 0 {
            let fighting = cells
                .iter()
                .filter_map(|&(id, _, _)| view.entity(id))
                .any(|e| super::chores::enemy_in_reach(view, e));
            let (mut front, mut since) = match self.front {
                Some((f, t)) if dist2(f, center) <= 14 * 14 => (f, t),
                _ => (center, frame),
            };
            let gathered = cells
                .iter()
                .filter(|&&(_, c, _)| dist2(c, front) <= 5 * 5)
                .count()
                * 3
                >= cells.len() * 2;
            if !fighting && gathered {
                let step = toward_dist(front, at, tune::CREEP);
                let walls = super::chores::Danger::seen_but(view, 1, |e| !is_structure(e));
                let has_siege = units
                    .iter()
                    .any(|u| u.siege && cells.iter().any(|c| c.0 == u.id));
                if !(has_siege && walls.at(step)) || frame.saturating_sub(since) > 45 * 30 {
                    front = step;
                    since = frame;
                }
            }
            self.front = Some((front, since));
            let width = (cells.len() as f64).sqrt().ceil() as i32;
            for (i, &(id, c, _)) in cells.iter().enumerate() {
                if focused.contains(&id) {
                    continue;
                }
                let i = i as i32;
                let slot = (
                    (i32::from(front.0) + i % width - width / 2).max(1) as u16,
                    (i32::from(front.1) + i / width - width / 2).max(1) as u16,
                );
                let Some(unit) = units.iter().find(|u| u.id == id) else {
                    continue;
                };
                let chopper = unit.siege
                    && view
                        .entity(id)
                        .and_then(|e| view.object_type(e.type_ref()))
                        .is_some_and(|t| t.is_simple_deployer);
                if chopper {
                    // A Siege Chopper lands to shell what is within its
                    // landed reach (12 cells) and takes off to follow the
                    // front once nothing is left there.
                    let prey = view.enemy_objects().any(|e| {
                        !e.in_limbo()
                            && e.category() != EntityCategory::Aircraft
                            && dist2(cell(e), c) <= 11 * 11
                            && view.object_type(e.type_ref()).is_some_and(|t| !t.wall)
                    });
                    if unit.deployed {
                        if !prey && dist2(c, slot) > 4 * 4 {
                            self.toggle_deploy(id, false, frame, orders);
                        }
                        continue;
                    }
                    if prey && !unit.moving {
                        self.toggle_deploy(id, false, frame, orders);
                        continue;
                    }
                }
                // Re-sent every think: `engage` turns it into an attack on
                // whatever is in reach.
                orders.push(Command::AttackMove {
                    entity_id: id,
                    target: None,
                    destination: Some(TargetKind::Cell(slot.0, slot.1)),
                });
            }
            return;
        }
        // Everyone heads for the goal (the path finder takes the way);
        // whoever runs well ahead of the body holds until it catches up.
        let far = |c: (u16, u16)| dist2(c, at).isqrt();
        let mut ds: Vec<i32> = cells.iter().map(|c| far(c.1)).collect();
        ds.sort_unstable();
        let median = ds[ds.len() / 2];
        let lead = tune::LEAD;
        for &(id, c, infantry) in &cells {
            // v46c (knob PRIMES): a Yuri Prime in reach of an enemy
            // Construction Yard or factory takes it over.
            if units.iter().any(|u| u.id == id && u.prime)
                && let Some(prize) = view
                    .enemy_objects()
                    .filter(|e| is_structure(e) && dist2(cell(e), c) <= 12 * 12)
                    .filter(|e| {
                        view.object_type(e.type_ref())
                            .is_some_and(|t| t.construction_yard || t.factory_type().is_some())
                    })
                    .min_by_key(|e| {
                        let yard = view
                            .object_type(e.type_ref())
                            .is_some_and(|t| t.construction_yard);
                        (!yard, dist2(cell(e), c), e.stable_id())
                    })
            {
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id: prize.stable_id(),
                });
                continue;
            }
            if focused.contains(&id) {
                continue;
            }
            if units.iter().any(|u| u.id == id && u.siege && u.deployed) {
                // A landed Siege Chopper takes off to follow the body.
                self.toggle_deploy(id, false, frame, orders);
                continue;
            }
            if near && !infantry && dist2(c, at) <= 9 * 9 {
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id: target.stable_id(),
                });
            } else if !near && far(c) < median - lead {
                orders.push(Command::Stop { entity_id: id });
            } else if let Some(e) = view.entity(id) {
                let goal = goal_for(view, &mut goals, e, target, center);
                orders.push(Command::AttackMove {
                    entity_id: id,
                    target: None,
                    destination: Some(TargetKind::Cell(goal.0, goal.1)),
                });
            }
        }
    }
}

/// The enemy building an army at `from` goes for: Construction Yards first,
/// then factories, the rest, captured tech last; rank weighs like 20 cells.
fn target_from<'a>(
    view: &BotView<'a>,
    from: (u16, u16),
) -> Option<&'a crate::sim::game_entity::GameEntity> {
    view.enemy_objects()
        .filter(|e| is_structure(e))
        .min_by_key(|e| target_rank(view, e, from))
}

/// [`target_from`] for an army that drives: the best building with a cell
/// within 6 cells of it the army can reach (`drives`); one on a cliff or an
/// island held a whole army at its foot for good. Without any, the best.
fn reachable_target<'a>(
    view: &BotView<'a>,
    from: (u16, u16),
    drives: &dyn Fn((u16, u16)) -> bool,
) -> Option<&'a crate::sim::game_entity::GameEntity> {
    let mut all: Vec<_> = view
        .enemy_objects()
        .filter(|e| is_structure(e))
        .map(|e| (target_rank(view, e, from), e))
        .collect();
    all.sort_by_key(|&(key, _)| key);
    let (mw, mh) = view.map_size();
    all.iter()
        .find(|(_, e)| {
            let (w, h) = view
                .object_type(e.type_ref())
                .map_or((1, 1), |t| t.foundation.dimensions());
            let at = cell(e);
            let (x0, y0) = (i32::from(at.0), i32::from(at.1));
            (2..=6).any(|ring| {
                (x0 - ring..x0 + i32::from(w) + ring).any(|x| {
                    (y0 - ring..y0 + i32::from(h) + ring).any(|y| {
                        let edge = x == x0 - ring
                            || x == x0 + i32::from(w) + ring - 1
                            || y == y0 - ring
                            || y == y0 + i32::from(h) + ring - 1;
                        edge && x >= 1
                            && y >= 1
                            && x < i32::from(mw)
                            && y < i32::from(mh)
                            && drives((x as u16, y as u16))
                    })
                })
            })
        })
        .or(all.first())
        .map(|&(_, e)| e)
}

/// The order [`target_from`] picks buildings in (lowest first).
fn target_rank(
    view: &BotView<'_>,
    e: &crate::sim::game_entity::GameEntity,
    from: (u16, u16),
) -> (i64, u64) {
    let ty = view.object_type(e.type_ref());
    let rank = ty.map_or(3, |t| {
        if t.construction_yard || (tune::WAVE_SW && role(view, t) == Role::Super) {
            // v47 (knob WAVE_SW): the enemy's nuke or storm too:
            // half of v45's lost yards had no enemy within 6 cells.
            0
        } else if t.factory_type().is_some() || (is_lab(&t.id) && tune::WAVE_LAB) {
            1
        } else if t.capturable && t.needs_engineer {
            5
        } else if t.refinery {
            // v45c (knob WAVE_REF): the AI is production-bound on
            // its $50k bank; its income ends it only once that runs out.
            tune::WAVE_REF
        } else {
            3
        }
    });
    (
        dist2(cell(e), from) as i64 + i64::from(rank) * i64::from(tune::RANK),
        e.stable_id(),
    )
}

/// What the enemy defenses seen within 12 cells of `at` cost.
fn guard_of(view: &BotView<'_>, at: (u16, u16)) -> i32 {
    view.enemy_objects()
        .filter(|e| is_structure(e) && dist2(cell(*e), at) <= 12 * 12)
        .filter_map(|e| view.object_type(e.type_ref()))
        .filter(|t| t.weapon_list.iter().any(Option::is_some) || t.max_number_occupants > 0)
        .map(|t| t.cost.max(0))
        .sum()
}

/// [`approach`] once per movement zone in a look (`goals` caches it).
fn goal_for(
    view: &BotView<'_>,
    goals: &mut Vec<(
        Option<crate::rules::locomotor_type::MovementZone>,
        (u16, u16),
    )>,
    unit: &crate::sim::game_entity::GameEntity,
    target: &crate::sim::game_entity::GameEntity,
    toward: (u16, u16),
) -> (u16, u16) {
    let mz = view.object_type(unit.type_ref()).map(|t| t.movement_zone);
    if let Some(&(_, goal)) = goals.iter().find(|(m, _)| *m == mz) {
        return goal;
    }
    let goal = approach(view, target, toward, cell(unit), mz);
    goals.push((mz, goal));
    goal
}

/// Aircraft, and what counts as one (`ConsideredAircraft=`: the Kirov is a
/// VehicleType).
fn flies(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity) -> bool {
    e.category() == EntityCategory::Aircraft
        || view
            .object_type(e.type_ref())
            .is_some_and(|t| t.considered_aircraft)
}

fn armed(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity) -> bool {
    view.object_type(e.type_ref())
        .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
}

/// An experiment knob: `VG_<NAME>` from the environment (arena runs only),
/// else the default. Every peer of a match must see the same values.

/// A cell to drive to for a building: on a ring around its footprint (the
/// footprint itself is no destination), nearest `from`, that a unit moving
/// like `mz` can reach from `from` (the ring next to a base is often cliff,
/// water or other buildings). The nearest ring with such a cell wins; none
/// within 10 cells: the plain ring cell toward `from`. Reachability is
/// judged from `start` (a unit's own cell).
fn approach(
    view: &BotView<'_>,
    building: &crate::sim::game_entity::GameEntity,
    from: (u16, u16),
    start: (u16, u16),
    mz: Option<crate::rules::locomotor_type::MovementZone>,
) -> (u16, u16) {
    let at = cell(building);
    let (w, h) = view
        .object_type(building.type_ref())
        .map_or((1, 1), |t| t.foundation.dimensions());
    let (mw, mh) = view.map_size();
    let x0 = i32::from(at.0);
    let y0 = i32::from(at.1);
    let (x1, y1) = (x0 + i32::from(w) - 1, y0 + i32::from(h) - 1);
    let clamp = |v: u16, lo: i32, hi: i32| i32::from(v).clamp(lo, hi).max(1) as u16;
    let fallback = (clamp(from.0, x0 - 2, x1 + 2), clamp(from.1, y0 - 2, y1 + 2));
    let Some(mz) = mz else { return fallback };
    for ring in 2..=10 {
        let mut best: Option<(i32, (u16, u16))> = None;
        for x in x0 - ring..=x1 + ring {
            for y in y0 - ring..=y1 + ring {
                let edge = x == x0 - ring || x == x1 + ring || y == y0 - ring || y == y1 + ring;
                if !edge || x < 1 || y < 1 || x >= i32::from(mw) || y >= i32::from(mh) {
                    continue;
                }
                let c = (x as u16, y as u16);
                let d = dist2(c, from);
                if best.is_some_and(|(bd, _)| bd <= d) {
                    continue;
                }
                if view.can_reach(mz, start, c) {
                    best = Some((d, c));
                }
            }
        }
        if let Some((_, c)) = best {
            return c;
        }
    }
    fallback
}

fn pick_named(
    view: &BotView<'_>,
    options: &[BuildOption],
    category: ProductionCategory,
    names: &[&str],
) -> Option<InternedId> {
    names
        .iter()
        .filter(|n| !skipped_special(n))
        .find_map(|name| {
            options
                .iter()
                .find(|o| {
                    o.enabled
                        && o.queue_category == category
                        && view.name(o.type_id).eq_ignore_ascii_case(name)
                })
                .map(|o| o.type_id)
        })
}

/// Arena knob SPECIALS=0: no country-only units (Germany's Tank Destroyer,
/// Iraq's Desolator).
fn skipped_special(name: &str) -> bool {
    matches!(name, "TNKD" | "DESO") && !tune::SPECIALS
}

/// The strongest affordable fighting unit of the category.
fn best_unit(
    view: &BotView<'_>,
    options: &[BuildOption],
    category: ProductionCategory,
) -> Option<InternedId> {
    options
        .iter()
        .filter(|o| o.enabled && o.queue_category == category)
        .filter(|o| !skipped_special(view.name(o.type_id)))
        .filter_map(|o| Some((o, view.object_type(o.type_id)?)))
        .filter(|(_, ty)| {
            !ty.harvester
                && !ty.engineer
                && ty.deploys_into.is_none()
                && ty.weapon_list.iter().any(Option::is_some)
                && ty.cost <= 1500
        })
        .max_by_key(|(o, ty)| (ty.strength * 10 / o.cost.max(1), o.cost))
        .map(|(o, _)| o.type_id)
}

/// Own fighting infantry on the field.
fn units_infantry(view: &BotView<'_>) -> usize {
    view.own()
        .filter(|e| e.category() == EntityCategory::Infantry)
        .filter(|e| {
            view.object_type(e.type_ref()).is_some_and(|t| {
                !t.engineer && !t.slaved && t.weapon_list.iter().any(Option::is_some)
            })
        })
        .count()
}

/// An armed enemy building (or a bunker), as a siege target.
fn is_defense(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity) -> bool {
    view.object_type(e.type_ref()).is_some_and(|t| {
        t.is_base_defense
            && (t.weapon_list.iter().any(Option::is_some) || t.max_number_occupants > 0)
            || t.weapon_list.iter().any(Option::is_some) && !t.construction_yard
    })
}

/// The cell `dist` cells from `from` toward `to` (`to` itself when nearer).
fn toward_dist(from: (u16, u16), to: (u16, u16), dist: i32) -> (u16, u16) {
    let dx = i32::from(to.0) - i32::from(from.0);
    let dy = i32::from(to.1) - i32::from(from.1);
    let d = (dx * dx + dy * dy).isqrt();
    if d <= dist || d == 0 {
        return to;
    }
    (
        (i32::from(from.0) + dx * dist / d).max(1) as u16,
        (i32::from(from.1) + dy * dist / d).max(1) as u16,
    )
}

/// Where a superweapon takes the most enemy power: the seen power plant with
/// the most power output within 4 cells of it.
/// A Battle Lab (the stock AI's Kirov and Apocalypse teams need one).
fn is_lab(name: &str) -> bool {
    matches!(name, "NATECH" | "GATECH" | "YATECH")
}

/// The middle of the nearest-to-the-map-origin enemy Battle Lab seen.
fn lab_target(view: &BotView<'_>) -> Option<(u16, u16)> {
    view.enemy_objects()
        .filter(|e| is_structure(e) && is_lab(view.name(e.type_ref())))
        .filter_map(|e| {
            let (w, h) = view.object_type(e.type_ref())?.foundation.dimensions();
            let at = cell(e);
            Some((at.0 + w / 2, at.1 + h / 2))
        })
        .min()
}

fn power_target(view: &BotView<'_>) -> Option<(u16, u16)> {
    let plants: Vec<((u16, u16), i32)> = view
        .enemy_objects()
        .filter(|e| is_structure(e))
        .filter_map(|e| {
            let t = view.object_type(e.type_ref())?;
            (t.power > 0 && !t.construction_yard).then(|| {
                let (w, h) = t.foundation.dimensions();
                let at = cell(e);
                ((at.0 + w / 2, at.1 + h / 2), t.power)
            })
        })
        .collect();
    plants
        .iter()
        .map(|&(at, _)| {
            let near: i32 = plants
                .iter()
                .filter(|(b, _)| dist2(*b, at) <= 4 * 4)
                .map(|(_, p)| *p)
                .sum();
            (std::cmp::Reverse(near), at)
        })
        .min()
        .map(|(_, at)| at)
}

/// The longest weapon reach (cells) of a type.
/// The strike group's unit for this House: Apocalypse tanks when it can
/// build them, else Prism Tanks (`strike_group`).
fn group_type(view: &BotView<'_>) -> Option<&'static str> {
    let options = view.build_options();
    ["APOC", "SREF"]
        .into_iter()
        .find(|n| options.iter().any(|o| view.name(o.type_id) == *n))
}

fn reach_of(view: &BotView<'_>, type_id: InternedId) -> i32 {
    view.object_type(type_id).map_or(0, |t| {
        t.weapon_list
            .iter()
            .flatten()
            .filter_map(|w| view.rules.weapon(w))
            .map(|w| w.range_leptons / 256)
            .max()
            .unwrap_or(0)
    })
}

/// Tuning values, set by headless arena rounds against the stock AI.
mod tune {
    pub const AA_EARLY: i32 = 2;
    pub const AA_FROM: i32 = 2;
    pub const AA_HOME: bool = true;
    pub const AA_MID: i32 = 4;
    pub const AA_PEAK: bool = true;
    pub const AA_PER: i32 = 2;
    pub const AA_REACH: i32 = 5;
    pub const AIR_ALARM: i32 = 24;
    pub const BAIT: i32 = 0;
    pub const CLONES: i32 = 0;
    pub const CREEP: i32 = 2;
    pub const DEF_CAP: i32 = 14;
    pub const DEF_HOLD: i32 = 3;
    pub const DEPOT_MIN: i32 = 4;
    pub const DESOS: i32 = 0;
    pub const DISCS: i32 = 4;
    pub const DISC_GROUP: i32 = 3;
    pub const DISC_LOW: i32 = 50;
    pub const EARLY_REF3: bool = true;
    pub const ENG_ALARM: i32 = 28;
    pub const FIRST_MIN: i32 = 4;
    pub const FLEE_FRAMES: i32 = 900;
    pub const FLEE_PCT: i32 = 35;
    pub const FOCUS: bool = false;
    pub const GIS: i32 = 2;
    pub const GIS_PER_2MIN: i32 = 1;
    pub const GI_CAP: i32 = 12;
    pub const GRIND: bool = true;
    pub const GRINDER_MIN: i32 = 3;
    pub const GROUP: i32 = 9;
    pub const GUARD_PCT: i32 = 150;
    pub const HARVS: i32 = 16;
    pub const HARV_SKIP: bool = true;
    pub const HOLD: bool = true;
    pub const HOME_TANKS: i32 = 0;
    pub const IC_DODGE: bool = true;
    pub const IC_REACH: i32 = 12;
    pub const INF_CAP: i32 = 20;
    pub const KITE: bool = true;
    pub const KITE_NEAR: i32 = 5;
    pub const LAB_MIN: i32 = 8;
    pub const LEAD: i32 = 7;
    pub const MCV_WAIT: i32 = 14;
    pub const MINDS: i32 = 2;
    pub const MOPUP: i32 = 10;
    pub const MOPUP_MIN: i32 = 15;
    pub const PARK: bool = false;
    pub const PRIMES: i32 = 0;
    pub const PRISM_CLUSTER: bool = true;
    pub const RANK: i32 = 400;
    pub const RECALL: bool = true;
    pub const REFS: i32 = 8;
    pub const REF_FIRST: i32 = 0;
    pub const REF_SERVE: i32 = 8;
    pub const REINFORCE: i32 = 3000;
    pub const REPAIR_PCT: i32 = 45;
    pub const RETREAT_PCT: i32 = 50;
    pub const ROCKETEERS: i32 = 4;
    pub const SCREEN: i32 = 3;
    pub const SIEGE: bool = true;
    pub const SIEGE_OF_4: i32 = 2;
    pub const SIEGE_REACH: i32 = 16;
    pub const SMINS: i32 = 0;
    pub const SPECIALS: bool = true;
    pub const SPREAD: i32 = -1;
    pub const SUPER: bool = true;
    pub const SUPPORT: bool = true;
    pub const SW_LAB: bool = true;
    pub const SW_POWER: bool = true;
    pub const TROOPERS: i32 = 24;
    pub const TROOPERS_PER_COIL: i32 = 3;
    pub const WALLS: bool = true;
    pub const WAVE_GROW: i32 = 1500;
    pub const WAVE_LAB: bool = true;
    pub const WAVE_MAX: i32 = 16000;
    pub const WAVE_REF: i32 = 3;
    pub const WAVE_SIEGE: i32 = 3;
    pub const WAVE_SW: bool = false;
    pub const WAVE_VALUE: i32 = 10000;
    pub const WF2_MIN: i32 = 5;
    pub const WF3_MIN: i32 = 10;
    pub const YARD2: bool = true;
    pub const YARD2_CASH: i32 = 0;
    pub const YARD2_DIST: i32 = 10;
    pub const YARD2_MIN: i32 = 5;
    pub const YARD_FLEE: bool = false;
    pub const YARD_GIS: bool = true;
    pub const ZEPS: i32 = 6;
    pub const ZEP_GROUP: i32 = 3;
}
