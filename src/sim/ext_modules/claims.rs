//! House domain claims (RFC 0001 section 6i, "Driving a house"): the house
//! property `ControlDomains` and the native AI decisions a claim makes
//! yield.
//!
//! [`DomainClaims`] is the one owner: per house array index, the claimed
//! `Domain` bits (appendix 8.3, enum 25) and which module source owns each.
//! It is saved with the modules and folded into the state hash and the
//! multiplayer checksum only while it holds a claim, so stock play hashes as
//! before. [`Simulation::module_claims`] is the read the native AI asks; with
//! no claim it costs one emptiness check.
//!
//! Writes (`set` of `ControlDomains` on a house reference): the value is the
//! set of bits the writing module wants to hold on that house, inside
//! `DOMAIN_ALL` (anything else is an invalid argument). It is `Refused` for a
//! human house, a defeated house or the local player's house (the condition
//! `order` uses), and when it names a bit the other source holds. Bits the
//! value leaves out are released, but only the writer's own; the other
//! source's bits stay. A module that stops releases every bit it held
//! (`Simulation::disable_ext_module`); the native AI takes the domain back
//! at its next entry.
//!
//! "Mod AI" seats: the launch session marks computer seats
//! (`SkirmishLaunchSession::mod_ai_slots`). Once the launch houses exist
//! (`initialize_skirmish_launch_houses`, which runs after
//! `install_scenario_ext_modules`: the modules are installed before the
//! houses are built), [`Simulation::seat_mod_ai_houses`] gives each such
//! house all four bits, owned by the mod module, when the mod says
//! `[VeraWasm] Controller=yes` and its module runs. Otherwise the seat keeps
//! the native AI and the log says so.
//!
//! Where the native AI yields (each gate asks [`Simulation::module_claims`]
//! at the decision's entry; what the units do on their own is untouched):
//! - `Production`:
//!   - `HouseClass::Update`'s chooser block (`0x004F9038..0x004F9265`,
//!     `ai_base_building::update_production_choices`: `AI_Choose_Building`
//!     and the unit choosers);
//!   - `BuildingClass::Factory_AI @ 0x004500F0`'s start of production
//!     (`production::factory_ai::factory_ai`, before `start_factory`; the exit
//!     of an object already finished still runs);
//!   - the strategy tick's `AI_Manage_Build_Queue @ 0x004FDD10`
//!     (`house_strategy::building_strategy`) and its `Fire_Sale @
//!     0x005013A0` (`house_strategy::sell_off_and_hunt`,
//!     `phobos_last_legs_actions`);
//!   - the house AI's MCV deploy (`mcv_deploy::owner_builds_its_own_base`,
//!     read by `UnitClass::AI`'s Hunt queue `0x007363DE..0x0073645B` and
//!     `Mission_Guard`'s Unload arm `0x007408C7..`); the Hunt that
//!     `Mission_Unload` queues after a refused deploy (`0x0073DDCB..`,
//!     `mcv_deploy::finish_initial_attempt`) becomes Guard; and when the
//!     claim starts, an MCV the house AI already sent to Hunt queues Guard
//!     (`mcv_deploy::yield_base_hunts`), so Mission_Hunt's deploy arm does
//!     not deploy it;
//!   - a computer harvester's return from Guard to Harvest (`UnitClass::
//!     Mission_Guard`'s arm `0x00740880..0x0074092C`,
//!     `mission_handlers::harvester_guard_override_requeues_harvest`): the
//!     house's economy.
//! - `Forces`:
//!   - the team block of `HouseClass::Update` (`0x004F8A00..0x004F8B08`,
//!     `ai_team_creation::update_team_creation`): AI trigger evaluation
//!     (`0x006F0AB0`) and team creation;
//!   - `TeamClass::AI @ 0x006E9140` for the house's teams
//!     (`team_script_vm::team_ai::team_ai`): recruiting, regrouping, the
//!     under-strength retreat and the team script. The teams stay as they
//!     are until the claim goes;
//!   - the strategy tick's `All_To_Hunt @ 0x00501400`
//!     (`house_strategy::sell_off_and_hunt`, `phobos_last_legs_actions`);
//!   - the regroup after an MCV deploys (`move_ground_forces_to_base`, the
//!     hook at `0x739920`, in `world_spawn`'s deploy);
//!   - where the units' missions place them for the house: the computer
//!     infantry's idle stance pick (`InfantryClass::Enter_Idle_Mode`
//!     `0x0051CCB9..0x0051CD75`, which then takes the units' own rule),
//!     a campaign hunter's march on the player (`Mission_Hunt` `0x004D5506`),
//!     Area Guard's move of a post off an own building (`0x004D6B8D`), the
//!     computer's repair seek (`UnitClass::AI` `0x0073671F`) and a captured
//!     unit's fate (`CaptureManagerClass::DecideUnitFate @ 0x004723B0`, whose
//!     roll is still drawn; the unit keeps its orders as a human's does).
//! - `Abilities` (the computer-only arms of the unit missions that act for
//!   the house AI):
//!   - the AI auto-deploy of `Deployer=` infantry (Infantry Guard
//!     `0x005214F7..0x005216B6`, `mission_handlers::infantry_automatic_guard_delay`);
//!   - `InfantryClass::Mission_Hunt`'s computer undeploy (`0x0051F5C0..0x0051F609`);
//!   - the computer capture arm of `Mission_Attack`/`Mission_Hunt`
//!     (`mission_handlers::computer_capture_building`): engineer capture,
//!     spy infiltration and garrisoning a building the unit targets;
//!   - the computer demolition arm of `Mission_Guard` and `Mission_AreaGuard`
//!     (`0x004D523A`, `0x004D6DBE`, `mission_handlers::guard_queues_sabotage`):
//!     a C4 infantryman holding a building target queues Sabotage.
//! - `SuperWeapons`: `HouseClass::AI_TryFireSW @ 0x005098F0`
//!   (`superweapon::ai_fire::try_fire`), choosing and firing.
//!
//! Residuals:
//! - The strategy tick still runs for a claimed house: the enemy search, the
//!   anger nodes and its timer draw keep their place; only the decisions
//!   above yield. So does the emergency state's own counter.
//! - A building the yard already had in production when `Production` was
//!   claimed is still placed by `Exit_Object` (`ai_base_building::exit_building`).
//!   The production mode (`EconomyStateMachine @ 0x00509700`,
//!   `ai_base_building::economy_state_machine`) still steps at an exit; it
//!   only steers the choosers, which yield.
//! - `Abilities` has no house-level entry: the house AI's own ability orders
//!   mostly come through team scripts (`Forces`). Ungarrisoning
//!   (occupants leaving under fire) and units' automatic deployment stay the
//!   units' own. Computer-owned targeting that picks a building for an
//!   engineer (`greatest_threat`) is target acquisition and stays.
//! - A capturer's Hunt after capture (`mission::capture`) and the base
//!   defense response (`combat::base_defense_response`) are not gated.
//! - A frozen team of a house that claimed `Forces` is not dissolved while
//!   the claim lasts, even when emptied.

use std::collections::BTreeMap;

use vera20k_wasm_ext::{DOMAIN_ALL, DOMAIN_PRODUCTION, Invalid, Status};

use super::{Host, ModuleSource};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// One house's claimed bits, by owner.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
struct HouseClaim {
    /// Bits the mod module holds.
    by_mod: u8,
    /// Bits the map module holds.
    by_map: u8,
}

impl HouseClaim {
    const fn of(self, source: ModuleSource) -> u8 {
        match source {
            ModuleSource::Mod => self.by_mod,
            ModuleSource::Map => self.by_map,
        }
    }

    fn set(&mut self, source: ModuleSource, bits: u8) {
        match source {
            ModuleSource::Mod => self.by_mod = bits,
            ModuleSource::Map => self.by_map = bits,
        }
    }

    const fn mask(self) -> u8 {
        self.by_mod | self.by_map
    }
}

/// The claim store (module docs). A house without a claim has no entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DomainClaims {
    /// By house array index.
    houses: BTreeMap<u32, HouseClaim>,
}

impl DomainClaims {
    pub(crate) fn is_empty(&self) -> bool {
        self.houses.is_empty()
    }

    /// House `index`'s `ControlDomains`: every claimed bit, whoever holds it.
    pub(crate) fn mask(&self, index: usize) -> i64 {
        u32::try_from(index)
            .ok()
            .and_then(|index| self.houses.get(&index))
            .map_or(0, |claim| i64::from(claim.mask()))
    }

    /// The houses in which `source` holds any bit: its "seats" (RFC 6i,
    /// "Budget and memory").
    pub(crate) fn seats(&self, source: ModuleSource) -> usize {
        self.houses
            .values()
            .filter(|claim| claim.of(source) != 0)
            .count()
    }

    /// `source` holds exactly `bits` (within `DOMAIN_ALL`) on house `index`;
    /// `Refused`, changing nothing, when the other source holds one of them.
    fn write(&mut self, index: u32, source: ModuleSource, bits: u8) -> Status {
        let mut claim = self.houses.get(&index).copied().unwrap_or_default();
        let other = match source {
            ModuleSource::Mod => claim.by_map,
            ModuleSource::Map => claim.by_mod,
        };
        if bits & other != 0 {
            return Status::Refused;
        }
        claim.set(source, bits);
        if claim.mask() == 0 {
            self.houses.remove(&index);
        } else {
            self.houses.insert(index, claim);
        }
        Status::Ok
    }

    /// Releases every bit `source` holds.
    pub(crate) fn release(&mut self, source: ModuleSource) {
        for claim in self.houses.values_mut() {
            claim.set(source, 0);
        }
        self.houses.retain(|_, claim| claim.mask() != 0);
    }

    /// The store as checksum bytes, in house order.
    pub(crate) fn state_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.houses.iter().flat_map(|(index, claim)| {
            index
                .to_le_bytes()
                .into_iter()
                .chain([claim.by_mod, claim.by_map])
        })
    }
}

impl super::ExtModules {
    /// The houses in which modules of `source` claim any domain (RFC 6i
    /// seats), from the claim state now.
    pub fn claimed_seats(&self, source: ModuleSource) -> usize {
        self.claims.seats(source)
    }
}

impl Simulation {
    /// Whether a module claimed `domain` (a `Domain` bit, e.g.
    /// `vera20k_wasm_ext::DOMAIN_PRODUCTION`) of house `house`, so the native
    /// AI yields that decision (module docs).
    pub(crate) fn module_claims(&self, house: InternedId, domain: i64) -> bool {
        let claims = &self.ext_modules.claims;
        !claims.is_empty()
            && self
                .houses
                .index_of(house)
                .is_some_and(|index| claims.mask(index) & domain != 0)
    }

    /// `source` holds exactly `bits` on house `owner` (array index `index`;
    /// [`DomainClaims::write`]). A claim that newly covers `Production`
    /// turns the house AI's MCV Hunt into Guard
    /// ([`crate::sim::mcv_deploy::yield_base_hunts`]).
    fn write_claim(
        &mut self,
        owner: InternedId,
        index: u32,
        source: ModuleSource,
        bits: u8,
        rules: &RuleSet,
    ) -> Status {
        let claims = &mut self.ext_modules.claims;
        let had_production = claims.mask(index as usize) & DOMAIN_PRODUCTION != 0;
        let status = claims.write(index, source, bits);
        if !had_production && claims.mask(index as usize) & DOMAIN_PRODUCTION != 0 {
            crate::sim::mcv_deploy::yield_base_hunts(self, owner, rules);
        }
        status
    }

    /// The mod module claims `bits` of house `name`, as its `ControlDomains`
    /// write does.
    #[cfg(test)]
    pub(crate) fn claim_for_test(&mut self, name: &str, bits: u8, rules: &RuleSet) -> Status {
        let owner = self.interner.get(name).expect("house");
        let index = self.houses.index_of(owner).expect("house index") as u32;
        self.write_claim(owner, index, ModuleSource::Mod, bits, rules)
    }

    /// "Mod AI" seats (module docs): each house named in `houses` starts
    /// with every domain claimed by the mod module when the mod is a
    /// controller and its module runs; otherwise the native AI plays it and
    /// the log says so.
    pub fn seat_mod_ai_houses(&mut self, houses: &[String]) {
        if houses.is_empty() {
            return;
        }
        let modules = &self.ext_modules;
        let controller = modules.controller
            && modules
                .slots
                .iter()
                .any(|slot| slot.source == ModuleSource::Mod && slot.disabled.is_none());
        for name in houses {
            let index = self
                .interner
                .get(name)
                .filter(|id| self.houses.get(id).is_some_and(|house| !house.is_human()))
                .and_then(|id| self.houses.index_of(id))
                .and_then(|index| u32::try_from(index).ok());
            let notice = match index {
                Some(index) if controller => {
                    let bits = DOMAIN_ALL as u8;
                    let _ = self
                        .ext_modules
                        .claims
                        .write(index, ModuleSource::Mod, bits);
                    continue;
                }
                Some(_) => format!(
                    "mod AI seat {name}: the mod has no controller module \
                     ([VeraWasm] Controller=yes and a rules.wasm); the native AI plays it"
                ),
                None => format!("mod AI seat {name}: no such computer house"),
            };
            log::warn!(target: "ext_module", "{notice}");
            self.ext_modules.keep_log(notice);
        }
    }
}

impl Host<'_, '_> {
    /// `set` of `ControlDomains` on house `index` (module docs).
    pub(super) fn write_control_domains(
        &mut self,
        index: usize,
        value: i64,
    ) -> Result<Status, Invalid> {
        if !(0..=DOMAIN_ALL).contains(&value) {
            return Err(Invalid(format!(
                "ControlDomains {value:#x} is not a set of Domain bits"
            )));
        }
        let Some(owner) = self.sim.houses.order().get(index).copied() else {
            return Ok(Status::Gone);
        };
        let Some(state) = self.sim.houses.get(&owner) else {
            return Ok(Status::Gone);
        };
        // RFC 6i "Claims": the condition `order` uses (6g "Houses").
        // `is_human` covers the local player's house; the local house
        // itself is per-client and never decides lockstep state.
        if state.is_human() || state.is_defeated() {
            return Ok(Status::Refused);
        }
        let Ok(index) = u32::try_from(index) else {
            return Ok(Status::Gone);
        };
        Ok(self
            .sim
            .write_claim(owner, index, self.source, value as u8, self.rules))
    }
}
