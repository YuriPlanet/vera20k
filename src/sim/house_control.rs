//! *(VERA)* Who controls a House, changed at any frame of a running match.
//!
//! gamemd fixes control at Create_Houses: a House is either a network
//! player's (`IsHuman`, its sender is the House index) or the computer's.
//! VERA adds one ordered event, [`Command::SetHouseControl`], that every
//! peer executes at the same frame, so control can move while the match runs:
//!
//! - [`HouseControl::Players`]: human; with a sender, that network sender
//!   now commands this House (and no longer the one it commanded before).
//!   Several senders may command one House (VERA shared control).
//! - [`HouseControl::Nobody`]: human, but no sender commands it; the House
//!   stands where it is (its objects still defend themselves).
//! - [`HouseControl::StockAi`]: gamemd's own computer player at that
//!   difficulty takes over, the way Execute30 (Auto Base Building on,
//!   `0x006DE206`) hands a House's base to it.
//!
//! A second event, [`Command::AddHouse`], adds a House to the running match.
//! Both are offered on the in-game Diplomacy page (`docs/house-control.md`).
//!
//! Senders: the network layer still sees one sender per peer. The sender to
//! House map lives in the session (`shared_house_sources`); a sender whose
//! House was handed to nobody or the computer is muted, so its commands are
//! dropped at execution while its frames keep the lockstep going.
//!
//! [`Command::SetHouseControl`]: crate::sim::command::Command::SetHouseControl
//! [`Command::AddHouse`]: crate::sim::command::Command::AddHouse

use serde::{Deserialize, Serialize};

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{CommandRecord, CommandRecordError};
use crate::sim::house_state::HouseDifficulty;
use crate::sim::world::{FrameEffects, Simulation};

/// VERA record for [`crate::sim::command::Command::SetHouseControl`]:
/// house index, control kind, control argument, sender.
pub const VERA_HOUSE_CONTROL_OPCODE: u8 = 0x33;
pub const VERA_HOUSE_CONTROL_PAYLOAD_LEN: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HouseControl {
    Players,
    Nobody,
    StockAi(HouseDifficulty),
}

impl HouseControl {
    fn encode(self) -> (u8, u8) {
        match self {
            Self::Players => (0, 0),
            Self::Nobody => (1, 0),
            Self::StockAi(difficulty) => (2, difficulty as u8),
        }
    }

    fn decode(kind: u8, arg: u8) -> Option<Self> {
        Some(match kind {
            0 => Self::Players,
            1 => Self::Nobody,
            2 => Self::StockAi(HouseDifficulty::from_native(i32::from(arg))?),
            _ => return None,
        })
    }
}

/// VERA record for [`crate::sim::command::Command::AddHouse`]: country
/// (u16), ally (House index or -1), control kind, control argument, sender.
pub const VERA_ADD_HOUSE_OPCODE: u8 = 0x34;
pub const VERA_ADD_HOUSE_PAYLOAD_LEN: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddHouseRecord {
    pub house_id: i8,
    pub frame: i32,
    pub country: u16,
    pub ally: i8,
    pub control: HouseControl,
    pub sender: i8,
}

impl AddHouseRecord {
    pub fn encode(self) -> Result<CommandRecord, CommandRecordError> {
        let (kind, arg) = self.control.encode();
        let [lo, hi] = self.country.to_le_bytes();
        CommandRecord::encode(
            VERA_ADD_HOUSE_OPCODE,
            i32::from(self.house_id),
            self.frame,
            &[lo, hi, self.ally as u8, kind, arg, self.sender as u8],
        )
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != VERA_ADD_HOUSE_OPCODE {
            return None;
        }
        let p = record.payload();
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            country: u16::from_le_bytes([p[0], p[1]]),
            ally: p[2] as i8,
            control: HouseControl::decode(p[3], p[4])?,
            sender: p[5] as i8,
        })
    }
}

/// The record's fields; `house_id` is the issuing sender, as in every record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HouseControlRecord {
    pub house_id: i8,
    pub frame: i32,
    pub house: u8,
    pub control: HouseControl,
    pub sender: i8,
}

impl HouseControlRecord {
    pub fn encode(self) -> Result<CommandRecord, CommandRecordError> {
        let (kind, arg) = self.control.encode();
        CommandRecord::encode(
            VERA_HOUSE_CONTROL_OPCODE,
            i32::from(self.house_id),
            self.frame,
            &[self.house, kind, arg, self.sender as u8],
        )
    }

    pub fn decode(record: &CommandRecord) -> Option<Self> {
        if record.opcode() != VERA_HOUSE_CONTROL_OPCODE {
            return None;
        }
        let payload = record.payload();
        Some(Self {
            house_id: record.house_id(),
            frame: record.frame_stamp(),
            house: payload[0],
            control: HouseControl::decode(payload[1], payload[2])?,
            sender: payload[3] as i8,
        })
    }
}

impl Simulation {
    /// Who controls House `owner` now.
    pub fn house_control(&self, owner: crate::sim::intern::InternedId) -> Option<HouseControl> {
        let house = self.houses.get(&owner)?;
        if !house.is_human() {
            return Some(HouseControl::StockAi(house.difficulty()));
        }
        let index = self.houses.index_of(owner)?;
        let commanded = self
            .session
            .network_sources(self.houses.order().len())
            .any(|(sender, house)| house == index && !self.session.sender_muted(sender));
        // Offline the local House is the only one with a sender.
        let offline_local = !self.session.game_mode().is_network()
            && self.session.current_house() == Some(owner)
            && !self.session.local_sender_muted();
        Some(if commanded || offline_local {
            HouseControl::Players
        } else {
            HouseControl::Nobody
        })
    }

    /// The sender this process commands with: its shared-control source,
    /// else its House's index (the identity map); 0 offline.
    pub fn local_house_control_sender(&self) -> i8 {
        if !self.session.game_mode().is_network() {
            return 0;
        }
        self.session
            .local_shared_house_source()
            .or_else(|| {
                let current = self.session.current_house()?;
                i8::try_from(self.houses.index_of(current)?).ok()
            })
            .unwrap_or(0)
    }

    /// Execute [`crate::sim::command::Command::SetHouseControl`]. Rejected
    /// (false) for a House that does not play: passive, defeated or gone.
    pub(crate) fn apply_house_control(
        &mut self,
        house_index: u8,
        control: HouseControl,
        sender: i8,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(&owner) = self.houses.order().get(usize::from(house_index)) else {
            return false;
        };
        let Some(house) = self.houses.get(&owner) else {
            return false;
        };
        if house.multiplay_passive() || house.is_defeated() {
            return false;
        }
        let house_count = self.houses.order().len();
        let index = usize::from(house_index);
        let network = self.session.game_mode().is_network();
        let local_index = self
            .session
            .current_house()
            .and_then(|current| self.houses.index_of(current));
        let is_local = self.session.current_house() == Some(owner);
        let release = |sim: &mut Self| {
            if network {
                sim.session.mute_senders_of(index, house_count, local_index);
            } else if is_local {
                sim.session.set_local_sender_muted(true);
            }
        };
        match control {
            HouseControl::Players => {
                self.houses.get_mut(&owner).expect("checked").make_human();
                if network && sender >= 0 {
                    self.session
                        .assign_sender(sender, index, house_count, local_index);
                    // The local node taking its own House back holds
                    // PlayerControl again (a computer House had none).
                    if is_local && self.session.local_shared_house_source() == Some(sender) {
                        self.houses
                            .get_mut(&owner)
                            .expect("checked")
                            .assign_player_control(true);
                    }
                } else if !network && is_local {
                    self.houses
                        .get_mut(&owner)
                        .expect("checked")
                        .assign_player_control(true);
                    self.session.set_local_sender_muted(false);
                } else if !network && sender >= 0 {
                    // Offline the one player takes this House over.
                    if let Some(old) = self.session.current_house()
                        && let Some(house) = self.houses.get_mut(&old)
                    {
                        house.assign_player_control(false);
                    }
                    self.session.set_current_house(Some(owner));
                    self.session.set_local_sender_muted(false);
                    self.houses
                        .get_mut(&owner)
                        .expect("checked")
                        .assign_player_control(true);
                }
            }
            HouseControl::Nobody => {
                self.houses.get_mut(&owner).expect("checked").make_human();
                release(self);
            }
            HouseControl::StockAi(difficulty) => {
                release(self);
                self.make_house_computer(owner, difficulty, rules, effects);
            }
        }
        self.refresh_local_house();
        log::info!(
            "House control: {} -> {control:?} (sender {sender}) at frame {}",
            self.interner.resolve(owner),
            self.session.binary_frame()
        );
        true
    }

    /// The computer takes House `owner` over at `difficulty`: the
    /// Create_Houses writes for a computer slot (SetDifficulty `0x006882B9`,
    /// IQ from Rules.MaxIQLevels) and Execute30's base hand-over, which picks
    /// the base from its Construction Yard when its plan is empty.
    fn make_house_computer(
        &mut self,
        owner: crate::sim::intern::InternedId,
        difficulty: HouseDifficulty,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let country = self.houses[&owner]
            .country()
            .map(|id| self.interner.resolve(id).to_owned());
        let house_count = self.houses.order().len() as i32;
        let frame = self.session.binary_frame() as i32;
        let nonzero = self.session.game_mode_nonzero();
        let house = self.houses.get_mut(&owner).expect("checked");
        house.make_computer();
        // PlayerControl is the local node's flag; a computer House has none,
        // so EventClass admission stays the same on every peer.
        house.assign_player_control(false);
        house.set_difficulty(
            difficulty,
            &rules.general,
            rules.country_difficulty_biases(country.as_deref().unwrap_or("")),
            nonzero,
            house_count,
            frame,
        );
        house.stamp_computer_iq(rules.general.max_iq_levels);
        house.enable_ai_deploy_latches();
        let _ = self.trigger_auto_base_building(owner, 1, rules, None, effects);
    }

    /// The local process's House follows its sender: a sender now assigned
    /// to another House views and commands that one. A muted local sender
    /// keeps the view of the House it had.
    fn refresh_local_house(&mut self) {
        if let Some(sender) = self.session.local_shared_house_source()
            && !self.session.sender_muted(sender)
            && let Some(index) = self.session.network_source_house(sender)
            && let Some(&owner) = self.houses.order().get(index)
            && self.session.current_house() != Some(owner)
        {
            if let Some(old) = self.session.current_house()
                && let Some(house) = self.houses.get_mut(&old)
            {
                house.assign_player_control(false);
            }
            self.session.set_current_house(Some(owner));
            if let Some(house) = self.houses.get_mut(&owner) {
                house.assign_player_control(true);
            }
        }
    }
}

impl Simulation {
    /// Execute [`crate::sim::command::Command::AddHouse`]: a new House of
    /// `country` joins the running match with its side's MCV and the
    /// match's starting credits, on the free start farthest from everybody,
    /// allied with House `ally` (and its allies) or at war with all, and is
    /// handed to `control`. The House constructor and its Supers are the
    /// launch's own (`populate_launch_houses`).
    pub(crate) fn add_house(
        &mut self,
        country: u16,
        ally: i8,
        control: HouseControl,
        sender: i8,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(country_name) = rules
            .country_name(crate::rules::ruleset::CountryIdx(country))
            .map(str::to_owned)
        else {
            return false;
        };
        if self.houses.order().len() >= 120 {
            return false;
        }
        let mut n = self.houses.order().len();
        let name = loop {
            let name = format!("Added{n}");
            if self
                .interner
                .get(&name)
                .is_none_or(|id| !self.houses.contains_key(&id))
            {
                break name;
            }
            n += 1;
        };
        let slot = crate::sim::scenario_bootstrap::NormalizedSkirmishSlot {
            owner_name: name.clone(),
            country: crate::skirmish_launch::LaunchCountry::from_country_index(u32::from(country)),
            color_index: (self.houses.order().len() % 8) as u8,
            start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
            team: crate::skirmish_launch::LaunchTeam::None,
            is_human: true,
            observer: false,
            controllers: Vec::new(),
            handle: None,
            difficulty: HouseDifficulty::Normal,
            commander: None,
        };
        crate::sim::scenario_bootstrap::populate_launch_houses(
            self,
            std::slice::from_ref(&slot),
            rules,
        );
        let Some(owner) = self.interner.get(&name) else {
            return false;
        };
        // The free start farthest from every building on the map.
        let buildings: Vec<(i32, i32)> = self
            .entities()
            .values()
            .filter(|e| {
                e.is_alive()
                    && e.category() == crate::map::entities::EntityCategory::Structure
                    && self
                        .houses
                        .get(&e.owner())
                        .is_some_and(|h| !h.multiplay_passive())
            })
            .map(|e| (i32::from(e.position.rx()), i32::from(e.position.ry())))
            .collect();
        // Open ground far from every building: the free starts first, then
        // a coarse grid of the map, nearest-to-farthest last.
        let far = |x: i32, y: i32| {
            buildings
                .iter()
                .map(|&(bx, by)| (bx - x).pow(2) + (by - y).pow(2))
                .min()
                .unwrap_or(i32::MAX)
        };
        let mut candidates: Vec<(i32, i32, i32)> = (0..8)
            .filter_map(|i| self.session.waypoint_cell(i))
            .map(|(x, y)| (i32::from(x), i32::from(y)))
            .map(|(x, y)| (far(x, y).saturating_add(1 << 20).min(i32::MAX), x, y))
            .filter(|&(d, _, _)| d > (1 << 20) + 400)
            .collect();
        let (w, h) = (
            self.session.map_width() as i32,
            self.session.map_height() as i32,
        );
        for y in (4..h - 4).step_by(4) {
            for x in (4..w - 4).step_by(4) {
                candidates.push((far(x, y), x, y));
            }
        }
        candidates.retain(|&(_, x, y)| {
            [(0, 0), (4, 0), (-4, 0), (0, 4), (0, -4)]
                .iter()
                .all(|(dx, dy)| self.map_cell_in_bounds(((x + dx) as i16, (y + dy) as i16)))
        });
        candidates.sort_by_key(|&(d, x, y)| (std::cmp::Reverse(d), x, y));
        let mcv = rules
            .general
            .base_unit_types
            .iter()
            .find(|unit| {
                rules.object(unit).is_some_and(|o| {
                    o.owner
                        .iter()
                        .any(|owner| owner.eq_ignore_ascii_case(&country_name))
                })
            })
            .or_else(|| rules.general.base_unit_types.first())
            .cloned();
        if let Some(mcv) = mcv {
            let placed = candidates
                .iter()
                .take(64)
                .find_map(|&(_, x, y)| match overlay_registry {
                    Some(registry) => self.spawn_object_with_overlay_registry(
                        &mcv, &name, x as u16, y as u16, 0, rules, registry,
                    ),
                    None => self.spawn_object(&mcv, &name, x as u16, y as u16, 0, rules),
                });
            if placed.is_none() {
                log::warn!("AddHouse: no room for {mcv}");
            }
        }
        // Alliances: with `ally` and everybody allied with it.
        if let Some(&friend) = usize::try_from(ally)
            .ok()
            .and_then(|i| self.houses.order().get(i))
        {
            let friends: Vec<_> = self
                .houses
                .order()
                .iter()
                .copied()
                .filter(|&other| {
                    other != owner
                        && (other == friend
                            || self
                                .house_alliances()
                                .is_allied(&self.interner, friend, other))
                        && self
                            .houses
                            .get(&other)
                            .is_some_and(|h| !h.multiplay_passive())
                })
                .collect();
            for other in friends {
                self.make_ally(owner, other, rules, overlay_registry, effects, false);
                self.make_ally(other, owner, rules, overlay_registry, effects, false);
            }
        }
        let index = self.houses.index_of(owner).unwrap_or(0) as u8;
        log::info!(
            "House added: {name} ({country_name}) at frame {}",
            self.session.binary_frame()
        );
        self.apply_house_control(index, control, sender, rules, effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::command::CommandEnvelope;

    /// Offline, the player hands their own House to the stock AI, then takes
    /// it back (the Diplomacy page). The record is sent as that House, now the
    /// computer's, which the DoList's House scan skipped (`house_is_eligible`):
    /// the Take was never executed.
    #[test]
    fn offline_take_back_from_the_stock_ai_executes() {
        let rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(""),
            &crate::rules::ini_parser::IniFile::from_str(""),
        )
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        let me = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house(Some(me));
        let index = sim.houses.index_of(me).unwrap() as u8;
        let issue = |sim: &mut crate::sim::world::Simulation, control, sender| {
            let record = sim
                .encode_house_control_record(me, index, control, sender)
                .unwrap();
            let tick = sim.session.tick();
            let command = CommandEnvelope::synchronized(record, tick);
            sim.apply_due_commands(
                &[command],
                &rules,
                tick,
                None,
                crate::sim::world::FrameEffects::default(),
            );
        };
        let ai = HouseControl::StockAi(HouseDifficulty::Hard);
        issue(&mut sim, ai, -1);
        assert_eq!(sim.house_control(me), Some(ai));
        issue(&mut sim, HouseControl::Players, 0);
        assert_eq!(sim.house_control(me), Some(HouseControl::Players));
    }

    /// An AddHouse record adds a computer House to the running match,
    /// at war with everybody when it names no ally.
    #[test]
    fn add_house_joins_the_match_as_the_computer() {
        let rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(
                "[Countries]\n0=Americans\n1=Russians\n2=Germans\n",
            ),
            &crate::rules::ini_parser::IniFile::from_str(""),
        )
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        let me = sim.interner.get("Americans").unwrap();
        sim.session.set_current_house(Some(me));
        let before = sim.houses.order().len();
        let ai = HouseControl::StockAi(HouseDifficulty::Normal);
        let germans = rules.country_index("Germans").unwrap().0;
        let record = sim
            .encode_add_house_record(me, germans, -1, ai, -1)
            .unwrap();
        let tick = sim.session.tick();
        sim.apply_due_commands(
            &[CommandEnvelope::synchronized(record, tick)],
            &rules,
            tick,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.houses.order().len(), before + 1);
        let added = *sim.houses.order().last().unwrap();
        assert_eq!(sim.house_control(added), Some(ai));
        assert!(!sim.house_alliances().is_allied(&sim.interner, added, me));
    }
}
