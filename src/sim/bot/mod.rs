//! *(VERA)* Rust bots: computer players written in
//! Rust that play a House through the commands a human player sends.
//!
//! A bot runs inside the simulation, once per frame on every peer, right
//! after the Houses' own AI (`run_late_region`), so it needs no network
//! traffic and stays in lockstep: it may read only the [`BotView`] (what its
//! House's player sees, and the rules) and must decide only from that and its
//! own saved state. Its orders execute at once through the ordinary command
//! executor, as the native DoList would execute a player's events.
//!
//! The House stays human for every rule (no computer bonus money, build
//! speed or cheats). A bot gets more only by declaring a [`Cheat`] in its
//! [`BotInfo`]; the game names it in the match's chat whenever the bot plays
//! a House (`announcements`). Order rate is not limited. A bot takes its
//! House at the launch ([`crate::skirmish_launch::AiDifficulty::bot`]).
//!
//! Adding a bot: implement [`Bot`] for a `Serialize + Deserialize + Clone`
//! type, add a [`BotBrain`] variant and a [`BOTS`] row. Any number of houses
//! may each run their own instance of any bot.
//!
//! Determinism: no `HashMap` iteration, no clocks, no thread-locals, no
//! floating point in decisions; random choices from the bot's own seeded
//! generator ([`BotRng`]), which is saved with it.

use serde::{Deserialize, Serialize};

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

pub(crate) mod chores;
pub mod vanguard;
pub mod view;

pub(crate) use chores::{
    CrateHunt, CrateScouts, Garrisons, PrismSquad, RocketeerSquad, SlaveMiners, Spies,
    TechBuildings, deploy_spare_mcvs, fight_drones, mastermind_overload, repair_buildings,
    sell_for_miner, tend_miners, wall_site, wall_to_build,
};
pub use view::{BotView, Cheat};

/// A bot's interface: once per frame, look at the world and push orders.
pub trait Bot {
    fn think(&mut self, view: &BotView<'_>, orders: &mut Vec<Command>);
    /// A few lines on what the bot is doing and why, for spectators.
    fn status(&self) -> Vec<String> {
        Vec::new()
    }
    /// Its plans as lines on the map, for spectators.
    fn markers(&self) -> Vec<BotMarker> {
        Vec::new()
    }
    /// Units on an errand that [`chores::engage`] leaves as ordered
    /// (scouts, spies, crate runners, repair trips, captives on their way
    /// to the Grinder).
    fn errand_units(&self) -> Vec<u64> {
        Vec::new()
    }
}

/// A plan of a bot drawn on the map: a line from `from` to `to` (cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BotMarker {
    pub from: (u16, u16),
    pub to: (u16, u16),
    pub kind: MarkerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// The army and what it attacks.
    Attack,
    /// The army or a scout looking for the enemy.
    Search,
    /// Units at home and where they gather for the next wave.
    Gather,
}

/// Index into [`BOTS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotKind(pub u8);

impl BotKind {
    pub fn new(index: u8) -> Option<Self> {
        (usize::from(index) < BOTS.len()).then_some(Self(index))
    }
    pub fn info(self) -> &'static BotInfo {
        &BOTS[usize::from(self.0)]
    }
}

pub struct BotInfo {
    /// Command-line and save name.
    pub name: &'static str,
    /// Shown in menus.
    pub title: &'static str,
    /// Advantages over a player this bot uses; nothing else is granted.
    pub cheats: &'static [Cheat],
    make: fn(seed: u32) -> BotBrain,
}

impl BotInfo {
    /// The chat line naming the bot's cheats; `None` for a fair bot.
    pub fn announcement(&self) -> Option<String> {
        (!self.cheats.is_empty()).then(|| {
            let list: Vec<&str> = self.cheats.iter().map(|c| c.describe()).collect();
            format!("{} (bot) uses: {}", self.title, list.join(", "))
        })
    }
}

/// Every bot this build knows, in menu order. The index is the wire id.
pub const BOTS: &[BotInfo] = &[
    BotInfo {
        name: "idle",
        title: "Idle bot",
        cheats: &[],
        make: |_| BotBrain::Idle,
    },
    BotInfo {
        name: "vanguard",
        title: "Vanguard",
        cheats: &[],
        make: |seed| BotBrain::Vanguard(Box::new(vanguard::Vanguard::new(seed))),
    },
];

pub fn kind_by_name(name: &str) -> Option<BotKind> {
    BOTS.iter()
        .position(|info| info.name.eq_ignore_ascii_case(name))
        .and_then(|index| BotKind::new(index as u8))
}

pub fn kind_names() -> Vec<&'static str> {
    BOTS.iter().map(|info| info.name).collect()
}

/// One bot instance, saved with the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BotBrain {
    /// Does nothing (a placeholder player).
    Idle,
    Vanguard(Box<vanguard::Vanguard>),
}

impl BotBrain {
    fn status(&self) -> Vec<String> {
        match self {
            Self::Idle => vec!["idle".into()],
            Self::Vanguard(bot) => bot.status(),
        }
    }

    fn markers(&self) -> Vec<BotMarker> {
        match self {
            Self::Idle => Vec::new(),
            Self::Vanguard(bot) => bot.markers(),
        }
    }

    fn errand_units(&self) -> Vec<u64> {
        match self {
            Self::Idle => Vec::new(),
            Self::Vanguard(bot) => bot.errand_units(),
        }
    }

    fn think(&mut self, view: &BotView<'_>, orders: &mut Vec<Command>) {
        match self {
            Self::Idle => {}
            Self::Vanguard(bot) => bot.think(view, orders),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BotSlot {
    house: InternedId,
    kind: BotKind,
    brain: BotBrain,
    /// Long-range units walking to their stand cell (`chores::RangeKeep`).
    #[serde(default)]
    range_keep: chores::RangeKeep,
}

/// The bots playing in this match, in installation order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BotHost {
    slots: Vec<BotSlot>,
}

impl BotHost {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    /// The bot of `house` describes its state (spectator overlay).
    pub fn status_of(&self, house: InternedId) -> Option<Vec<String>> {
        self.slots
            .iter()
            .find(|s| s.house == house)
            .map(|s| s.brain.status())
    }
    /// Every bot House's map markers (spectator overlay).
    pub fn markers(&self) -> Vec<(InternedId, BotMarker)> {
        self.slots
            .iter()
            .flat_map(|s| s.brain.markers().into_iter().map(move |m| (s.house, m)))
            .collect()
    }
    pub fn kind_of(&self, house: InternedId) -> Option<BotKind> {
        self.slots.iter().find(|s| s.house == house).map(|s| s.kind)
    }
    /// Every bot House whose bot declares cheats, with its chat line.
    pub fn announcements(&self) -> impl Iterator<Item = (InternedId, BotKind, String)> + '_ {
        self.slots
            .iter()
            .filter_map(|s| Some((s.house, s.kind, s.kind.info().announcement()?)))
    }
    pub(crate) fn remove(&mut self, house: InternedId) {
        self.slots.retain(|s| s.house != house);
    }
    pub(crate) fn install(&mut self, house: InternedId, kind: BotKind, seed: u32) {
        self.remove(house);
        self.slots.push(BotSlot {
            house,
            kind,
            brain: (kind.info().make)(seed),
            range_keep: Default::default(),
        });
    }
}

/// A small deterministic generator a bot keeps in its saved state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BotRng(u64);

impl BotRng {
    pub fn new(seed: u32) -> Self {
        Self(u64::from(seed) ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u32(&mut self) -> u32 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }
}

impl Simulation {
    /// The bots playing in this match.
    pub fn house_bots(&self) -> &BotHost {
        &self.house_bots
    }

    /// A launch slot played by a Rust bot
    /// ([`crate::skirmish_launch::SkirmishAiSlot::bot`]): the freshly created
    /// computer House turns human and the bot takes it. Called from
    /// `populate_launch_houses`, so on every peer alike and before
    /// Post_Map_Init's computer money (`apply_skirmish_ai_opening_credits`):
    /// the bot starts with a player's money.
    pub(crate) fn install_launch_bot(&mut self, owner: InternedId, kind: BotKind) {
        let Some(index) = self.houses.index_of(owner) else {
            return;
        };
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        house.make_human();
        house.set_ui_name(crate::sim::house_state::HouseUiName::handle(
            kind.info().title,
        ));
        let seed = self.session.binary_frame() ^ ((index as u32) << 24);
        self.house_bots.install(owner, kind, seed);
        log::info!(
            "Bot: {} is played by {}",
            self.interner.resolve(owner),
            kind.info().title
        );
    }

    /// Every bot thinks once, in installation order, and its orders execute
    /// at once. A defeated House's bot is idle. True when an order spawned
    /// an object (the dispatcher's spawn-refresh report).
    pub(crate) fn run_house_bots(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        if self.house_bots.is_empty() {
            return false;
        }
        let mut spawned = false;
        let mut host = std::mem::take(&mut self.house_bots);
        let execute_tick = self.session.tick().saturating_add(1);
        let mut orders = Vec::new();
        for slot in &mut host.slots {
            if self.houses.get(&slot.house).is_none_or(|h| h.is_defeated()) {
                continue;
            }
            orders.clear();
            let cheats = slot.kind.info().cheats;
            if cheats.contains(&Cheat::ExtraMoney)
                && self.session.binary_frame() % view::EXTRA_MONEY_EVERY == 0
                && let Some(house) = self.houses.get_mut(&slot.house)
            {
                house
                    .economy_mut()
                    .add_credits(view::EXTRA_MONEY_PER_MINUTE);
            }
            let view = BotView::new(self, rules, overlay_registry, slot.house, cheats);
            // A player's sidebar sends PLACE for a finished unit by itself
            // (StripClass::AI `0x006A8EB8`); a bot's does the same.
            if view.frame() % 15 == 0 {
                use crate::sim::production::ProductionCategory as C;
                for category in [C::Infantry, C::Vehicle, C::Aircraft, C::Ship] {
                    if view
                        .factory(category)
                        .is_some_and(|f| f.complete_object().is_some())
                    {
                        orders.push(Command::PlaceProducedMobile { category });
                    }
                }
            }
            slot.brain.think(&view, &mut orders);
            chores::detour(&view, &mut orders);
            let errands = slot.brain.errand_units();
            if !orders.is_empty() {
                chores::engage(&view, &errands, &mut orders);
            }
            slot.range_keep.rewrite(&view, &mut orders);
            slot.range_keep.run(&view, &mut orders);
            chores::hold_back(&view, &errands, &mut orders);
            for order in orders.drain(..) {
                // A bot never changes control itself.
                if matches!(order, Command::ExitMatch | Command::RemovePlayer { .. }) {
                    continue;
                }
                let envelope = CommandEnvelope::new(slot.house, execute_tick, order);
                let (_, spawn) =
                    self.apply_one_due_command(None, &envelope, rules, overlay_registry, effects);
                spawned |= spawn;
            }
        }
        // A command cannot install bots, so nothing was added meanwhile.
        debug_assert!(self.house_bots.is_empty());
        self.house_bots = host;
        spawned
    }
}
