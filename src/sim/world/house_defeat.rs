//! A defeated house's end: `HouseClass::Update`'s multiplayer defeat gate,
//! `HouseClass::Blowup_All` and `MPlayer_Defeated`.
//!
//! Native owner: `HouseClass`. In every non-campaign game, once per house per
//! frame in HouseClass::Array order (`0x004F8E86..0x004F8F82`): a house that
//! is not defeated, not `MultiplayPassive=` and past frame zero, and whose
//! counts (`house_tracking`) say it holds nothing, has every object it
//! originally owns blown up (`Blowup_All @ 0x004FC6D0`) and is then marked
//! defeated (`MPlayer_Defeated @ 0x004FC0B0`, its only caller). With the retail
//! `ShortGame=yes`, losing the last counted building while no `BaseUnit=` is
//! tracked destroys the whole remaining army in that one call.
//!
//! Blowup_All walks TechnoClass::Array, limbo objects included, re-reading the
//! count each step. An object whose original owner
//! (`TechnoClass::GetOriginalOwner @ 0x0070F820`) is the house and that the
//! house still owns dies; one another house mind-controls is instead handed to
//! the Civilian-side house when its controller's CaptureManager finds one
//! (`SetOriginalOwnerToCivilian @ 0x00472330`), and dies only when none
//! exists. An object being erased first has its Temporal chain released
//! (`0x0071AD40`). Each dies through its own ReceiveDamage with its Health as
//! `C4Warhead=` damage, no attacker, ignoring defenses and passenger escape
//! (vtable `+0x16C`, called at `0x004FC766`): no kill credit, no survivors.
//!
//! Scenario draws (read, not executed): none in the gate or the sweep's own
//! code; each death draws in its own receiver (death sounds, debris, death
//! anims), in array order. The Temporal release idles the released attackers
//! (ClearLinkedList); VERA queues those idles, so any draw they make comes at
//! the idle's turn.
//!
//! Evidence: `tools/spatial_oracle/house_blowup_all.py` runs the original
//! Blowup_All with GetOriginalOwner, CaptureManager GetOriginalOwner and
//! SetOriginalOwnerToCivilian (10 cases; the Civilian side lookup is
//! supplied); `house_defeat_gate.py` the gate (21 cases). Control flow and
//! ordering read from the disassembly.
//!
//! RESIDUALS:
//! - VERA runs the gate in its own house pass after the anger rung and before
//!   every house's AI, not inside each house's Update between its other steps
//!   (ledger T2-28). Trigger: every defeat. Effect: natively the houses before
//!   the defeated one in HouseClass::Array ran their AI before its sweep, so
//!   they saw its objects alive; in VERA every AI sees them dead.
//! - TechnoClass::Array order stands on stable-id order (construction order),
//!   which matches for every source VERA constructs in native order.
//! - Slave release: a Slave Miner killed with no attacker hands its slaves on
//!   the map to the Civilian-side house and UnInits those in limbo (the
//!   ReceiveDamage death arm's `0x006B0AE0` call at `0x00702065`); the sweep
//!   reaches the miner before its slaves and then skips them. VERA has no slave
//!   release on any master's death. Trigger: a Yuri house defeated while it
//!   owns a Slave Miner with slaves. Effect: its slaves die in the sweep, with
//!   their receivers' death effects and draws, instead of staying on the map
//!   as civilians.
//! - The IsToDie path (`Flag_To_Die @ 0x004FC980`: DESTRUCT, REMOVEPLAYER, a
//!   last human's EXIT) has no VERA producer; offline skirmish cannot reach
//!   it.
//! - The trigger-held original owner (`+0x2CC`/`+0x2E0`,
//!   HouseClass::TransferUnitsTo) is not ported.
//! - MPlayer_Defeated's local-player branch (`0x004FC1E7..0x004FC307`: the
//!   whole-map reveal `0x00577F30` and the defeat UI), its capture-the-flag
//!   cleanup (Flag_Remove `0x004FBE40` at `0x004FC112..0x004FC15E`, Scenario
//!   flag `0x10`), the Harvester-Truce UnInit loop (`0x004FC163`, Scenario flag
//!   `0x800`) and Computer_Paranoid (`0x00501640`) are not ported; the flags
//!   are off in stock skirmish. Of the local branch only the map-clear byte is
//!   kept (see `mplayer_defeated`). Trigger: the local player is defeated.
//!   Effect: the shroud stays in place for the loser.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

impl Simulation {
    /// The house rung's defeat pass (see the module doc), then the game-over
    /// scan and the result timers.
    pub(super) fn check_defeat(
        &mut self,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let outcome_tick = self.session.tick.saturating_add(1);
        let savour_frames = crate::rules::ruleset::savour_delay_frames(
            rules
                .map(|rules| rules.general.savour_delay_minutes)
                // RulesClass__Constructor @ 0x00665650 stores the exact f64
                // default 0.03 before any optional INI ReadDouble override.
                .unwrap_or(0.03),
        );
        // 0x004F8E86..0x004F8EB7: not a campaign, past frame zero.
        if self.session.game_mode_nonzero && (self.session.binary_frame as i32) > 0 {
            // The interner resolves names case-insensitively, as native type
            // lookups do.
            let base_units = rules.map_or([None; 3], |rules| {
                std::array::from_fn(|slot| {
                    rules
                        .general
                        .base_unit_types
                        .get(slot)
                        .and_then(|name| self.interner.get(name))
                })
            });
            let build_refinery_2 = rules
                .and_then(|rules| rules.build_refinery_types.get(2))
                .and_then(|name| self.interner.get(name));
            for owner in self.session.house_order.clone() {
                let Some(house) = self.houses.get(&owner) else {
                    continue;
                };
                if house.is_defeated || house.multiplay_passive {
                    continue;
                }
                let alive = if self.session.game_options.short_game {
                    house.tracking.short_game_alive(&base_units)
                } else {
                    house.tracking.normal_game_alive(build_refinery_2)
                };
                if alive {
                    continue;
                }
                if let Some(rules) = rules {
                    self.house_blowup_all(owner, rules, registry);
                }
                self.mplayer_defeated(owner, outcome_tick, savour_frames);
            }
        }

        // Check if all remaining alive houses are mutually allied → game over.
        // The native alive scan counts only houses that are neither defeated nor
        // passive; the Civilian/JP houses present in every skirmish own map
        // objects forever, so including them would keep the alive set above one
        // and the victory screen would never appear.
        let alive: Vec<InternedId> = self
            .houses
            .iter()
            .filter(|(_, h)| !h.is_defeated && !h.multiplay_passive)
            .map(|(k, _)| *k)
            .collect();

        // VERA-internal developer policy, gamemd equivalent UNCHECKED: an
        // authored solo sandbox must not create a victory state or EVA merely
        // for being the only contender. Keep accepted explicit outcomes and
        // their timers below independent of this automatic-creation gate.
        let automatic_victory_allowed = self.contending_house_count() > 1;
        if automatic_victory_allowed && alive.len() == 1 {
            // Last player standing.
            if let Some(h) = self.houses.get_mut(&alive[0])
                && h.flag_to_win(outcome_tick, savour_frames)
            {
                self.sound_events.push(SimSoundEvent::MatchOutcome {
                    owner: alive[0],
                    kind: crate::sim::house_state::HouseOutcomeKind::Victory,
                });
            }
        } else if automatic_victory_allowed && !alive.is_empty() {
            // O(n^2) mutual-alliance check. Native alliance is directional — each
            // house owns its own ally bits — and the game-over scan requires BOTH
            // houses of a pair to name the other, so a one-way alliance must not end
            // the match.
            let all_allied = alive.iter().all(|a| {
                alive.iter().all(|b| {
                    a == b
                        || crate::map::houses::are_houses_mutually_allied(
                            &self.house_alliances,
                            self.interner.resolve(*a),
                            self.interner.resolve(*b),
                        )
                })
            });

            if all_allied {
                for &owner in &alive {
                    if let Some(h) = self.houses.get_mut(&owner)
                        && h.flag_to_win(outcome_tick, savour_frames)
                    {
                        self.sound_events.push(SimSoundEvent::MatchOutcome {
                            owner,
                            kind: crate::sim::house_state::HouseOutcomeKind::Victory,
                        });
                    }
                }
            }
        }

        // HouseClass::Update @ 0x004F8440 advances the accepted result timer
        // in the house rung. The expiry frame is terminal and therefore skips
        // the wrapping frame commit below, matching Main_Tick's early return.
        for house in self.houses.values_mut() {
            house.advance_outcome_savour(outcome_tick);
        }
    }

    /// `HouseClass::MPlayer_Defeated @ 0x004FC0B0`, the represented part: the
    /// Defeated flag, the announcement (`0x004FC30F..0x004FC3BC`, any
    /// non-passive house; the app decides local or other), the map-clear byte
    /// `+0x241` (written at `0x004FC328` for another player's house, through
    /// the whole-map reveal at `0x00577F48` for the local player's) and the
    /// loss.
    fn mplayer_defeated(&mut self, owner: InternedId, outcome_tick: u64, savour_frames: u64) {
        self.sound_events
            .push(SimSoundEvent::PlayerDefeated { house: owner });
        let accepted = self.houses.get_mut(&owner).is_some_and(|house| {
            house.is_defeated = true;
            house.map_is_clear = true;
            house.flag_to_lose(outcome_tick, savour_frames)
        });
        if accepted {
            self.sound_events.push(SimSoundEvent::MatchOutcome {
                owner,
                kind: crate::sim::house_state::HouseOutcomeKind::Defeat,
            });
        }
    }

    /// `HouseClass::Blowup_All @ 0x004FC6D0` (see the module doc).
    pub(crate) fn house_blowup_all(
        &mut self,
        house: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        // The array is re-read each step (items `0x004FC6EC`, count
        // `0x004FC771`): an object added during the sweep is visited too,
        // after every one that preceded it.
        let mut visited_below = 0u64;
        loop {
            let mut order: Vec<u64> = self
                .substrate
                .entities
                .values()
                .filter(|entity| entity.stable_id() >= visited_below)
                .map(|entity| entity.stable_id())
                .collect();
            if order.is_empty() {
                break;
            }
            order.sort_unstable();
            visited_below = order[order.len() - 1] + 1;
            for id in order {
                if !self.blown_up_with(id, house, rules) {
                    continue;
                }
                // 0x004FC742: the chain warping it lets go first.
                if let Some(head) = self
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|entity| entity.temporal.chain_head())
                {
                    self.temporal_release_chain_no_idle(head, rules);
                }
                let Some(health) = self
                    .substrate
                    .entities
                    .get(id)
                    .map(|entity| entity.health.current)
                else {
                    continue;
                };
                let event = EntityDamageEvent::direct_receiver(
                    id,
                    health,
                    0,
                    RAD_NO_ATTACKER,
                    None,
                    c4,
                    ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: true,
                    },
                );
                #[cfg(test)]
                BLOWUP_TRACE.with(|trace| trace.borrow_mut().push(event));
                self.commit_direct_damage_receiver(rules, registry, event);
            }
        }
    }

    /// Blowup_All's predicate (`0x004FC6F1..0x004FC731`) for one Techno:
    /// its original owner (`TechnoClass::GetOriginalOwner @ 0x0070F820`) is
    /// `house`, and either it still belongs to `house` or no Civilian-side
    /// house takes it over from the house controlling it.
    fn blown_up_with(&mut self, id: u64, house: InternedId, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let owner = entity.owner();
        let Some(controller) = entity.mind_control.controller() else {
            return owner == house;
        };
        // CaptureManagerClass::GetOriginalOwner @ 0x004722F0 (null without a
        // node).
        let original = self
            .substrate
            .entities
            .get(controller)
            .and_then(|controller| controller.capture_manager.as_ref())
            .and_then(|manager| manager.original_owner(id));
        if original == Some(owner) {
            return owner == house;
        }
        if original != Some(house) {
            return false;
        }
        !self.set_original_owner_to_civilian(controller, id, rules)
    }

    /// `CaptureManagerClass::SetOriginalOwnerToCivilian @ 0x00472330`: the
    /// first house in HouseClass::Array order whose side is `Civilian`
    /// (`0x006A46D0`) becomes the victim's original owner in every node of
    /// the controller's manager. False when no such house exists.
    fn set_original_owner_to_civilian(
        &mut self,
        controller: u64,
        victim: u64,
        rules: &RuleSet,
    ) -> bool {
        let Some(civilian_side) = rules.side_index("Civilian") else {
            return false;
        };
        let Some(civilian) = self.session.house_order.iter().copied().find(|owner| {
            self.houses
                .get(owner)
                .is_some_and(|house| house.side_index == civilian_side.0)
        }) else {
            return false;
        };
        if let Some(manager) = self
            .substrate
            .entities
            .get_mut(controller)
            .and_then(|controller| controller.capture_manager.as_mut())
        {
            manager.set_original_owner(victim, civilian);
        }
        true
    }
}

#[cfg(test)]
thread_local! {
    /// Blowup_All's receiver calls, in order, for the native comparison.
    static BLOWUP_TRACE: std::cell::RefCell<Vec<EntityDamageEvent>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn take_blowup_trace() -> Vec<EntityDamageEvent> {
    BLOWUP_TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

#[cfg(test)]
#[path = "house_defeat_tests.rs"]
mod tests;
