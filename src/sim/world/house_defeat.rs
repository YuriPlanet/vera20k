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
//! - TechnoClass::Array order stands on stable-id order (construction order),
//!   which matches for every source VERA constructs in native order.
//! - Slave release: a Slave Miner killed with no attacker hands its slaves on
//!   the map to the Civilian-side house and UnInits those in limbo (the
//!   ReceiveDamage death arm's `0x006B0AE0` call at `0x00702065`,
//!   `sim::slave_manager`). A master constructed before its slaves (its own
//!   constructor builds them) is reached first, and the sweep then skips
//!   them, as they no longer belong to the defeated house; a refinery
//!   deployed from a Slave Miner comes after the slaves it took over, so the
//!   sweep kills those first as the defeated house's own, in the same
//!   TechnoClass::Array order.
//! - The IsToDie path (`Flag_To_Die @ 0x004FC980`: DESTRUCT, REMOVEPLAYER, a
//!   last human's EXIT) has no VERA producer; offline skirmish cannot reach
//!   it.
//! - The trigger-held original owner (`+0x2CC`/`+0x2E0`,
//!   HouseClass::TransferUnitsTo) is not ported.
//! - MPlayer_Defeated's capture-the-flag cleanup (Flag_Remove `0x004FBE40` at
//!   `0x004FC112..0x004FC15E`, Scenario flag `0x10`) and the Harvester-Truce
//!   UnInit loop (`0x004FC163`, Scenario flag `0x800`) are not ported; their
//!   Scenario flags are off in stock skirmish.
//! - The local-player branch (`0x004FC1B0..0x004FC307`) is ported for the
//!   whole-map reveal, the radar and Session `+0x300` (see `mplayer_defeated`;
//!   `local_player_defeated` names the ported readers). Its client UI is not:
//!   beacons deleted (`0x00431410`), the command bar closed (`0x006D1660`),
//!   planning and beacon modes left (`0x00637A10`, `0x004AC960`), and the
//!   bytes `0x00A8D108..0x00A8D10F` that Taunt and dialog `0x73` read set and
//!   that dialog refreshed (`0x004FC239..0x004FC25D`). Nor are Session
//!   `+0x300`'s UI readers: band-box and type selection (`0x004AC03F`,
//!   `0x00732950`), the planning, beacon, cheer and taunt commands
//!   (`0x00731A50`, `0x00731A30`, `0x00730F30`, `0x0053636C`), the sidebar tab
//!   (`0x006D08F5`), `RadarClass::SetRadarMode` (`0x00656CCC`), MakeAlly's
//!   alliance line (`0x004F9E49`) and the dialog and session readers
//!   (`0x004F1534`, `0x00657F4A`, `0x0078D52A`, `0x004E20BA`, `0x004F10FD`,
//!   `0x0055EE60` and others). Trigger: the local player is defeated. Effect:
//!   for the savour delay (retail `SavourDelay=.1`) those UI states and
//!   commands stay as they were; a defeated house owns nothing left to command.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::intern::InternedId;
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation, UninitContext};

/// Native House cleanup leaves4FC6D0/4FC790/4FC820/4FC8D0 share the
/// registry walk and receiver; only their admission differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HouseBlowupSubset {
    All,
    Buildings,
    Land,
    Naval,
}

impl Simulation {
    /// The house rung with its defeat pass: see [`Self::house_rung`].
    #[cfg(test)]
    pub(super) fn check_defeat(
        &mut self,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let generated = self.house_rung(
            rules,
            registry,
            true,
            crate::sim::world::FrameEffects::default(),
        );
        self.queue_commands(generated);
    }

    /// The house rung's per-house steps in HouseClass::Array order: each
    /// house's team creation (`0x004F8A00..0x004F8B08`,
    /// `sim::ai_team_creation`), its defeat gate (see the module doc), its
    /// Tags' event-8 raise (`sim::trigger_runtime`), then its strategy tick (`0x004F8FBE..0x004F9032`, `sim::house_strategy`) and
    /// production choices (`0x004F9038..0x004F9265`, `sim::ai_base_building`)
    /// and tech recheck (`0x004F9265..0x004F9302`, `sim::production`);
    /// then the game-over scan. The native result head4F85BE precedes teams.
    /// Returned events join the post-Logic dispatcher without executing during
    /// a House visit. Without `defeat_pass`
    /// (VERA's first tick) the defeat gates and the game-over scan are
    /// skipped.
    pub(super) fn house_rung(
        &mut self,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        defeat_pass: bool,
        frame_effects: FrameEffects<'_>,
    ) -> Vec<crate::sim::command::CommandEnvelope> {
        let mut generated = Vec::new();
        // 0x004F8E86..0x004F8EB7: not a campaign, past frame zero.
        let defeat_gate =
            defeat_pass && self.session.game_mode_nonzero && (self.session.binary_frame as i32) > 0;
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
        // Logic55B68D reloads the live House count after each complete body.
        // A House's power/radar and activation must precede its own teams,
        // EVA, defeat and production; no global half-body sweeps intervene.
        let mut index = 0;
        while index < self.session.house_order.len() {
            let owner = self.session.house_order[index];
            if let Some(rules) = rules {
                self.assess_house_derived_state(owner, rules);
            }
            self.update_house_anger_and_activation(owner, rules);
            // House4F85BE/4F86FE precedes team creation, defeat and this
            // House's Tags. Result countdown and AllowWin-tag teardown share
            // the sole private +298 CdTimer; use NativeFrame, not Rust tick+1.
            if let Some(house) = self.houses.get_mut(&owner) {
                house.advance_outcome_savour(
                    self.session.binary_frame as i32,
                    self.session.game_mode_nonzero,
                );
            }
            if let Some(power) = self.power_states.get_mut(&owner) {
                power.clamp_negative_totals();
            }
            if let Some(rules) = rules {
                crate::sim::ai_team_creation::update_team_creation(
                    self,
                    rules,
                    owner,
                    frame_effects,
                );
                crate::sim::house_eva::update_house_eva(self, rules, owner);
                crate::sim::superweapon::tick_house_superweapon_instances(self, rules, owner);
            }
            if defeat_gate && self.house_holds_nothing(owner, &base_units, build_refinery_2) {
                if let Some(rules) = rules {
                    self.house_blowup(
                        owner,
                        HouseBlowupSubset::All,
                        rules,
                        registry,
                        frame_effects,
                    );
                }
                self.mplayer_defeated(
                    owner,
                    UninitContext::new(rules, registry).with_effects(frame_effects),
                );
            }
            self.raise_house_tags(owner, rules, registry, frame_effects);
            if let Some(rules) = rules {
                crate::sim::house_strategy::update_strategy(
                    self,
                    rules,
                    owner,
                    registry,
                    frame_effects,
                );
                crate::sim::ai_base_building::update_production_choices(
                    self, rules, owner, registry,
                );
                // House4F927C/4F92ED clears1FC before either pass; a status
                // change inside50AF10 can retain a request for the next frame.
                if self
                    .houses
                    .get_mut(&owner)
                    .is_some_and(|house| house.take_tech_recheck())
                {
                    // Strip recalculation precedes4F92F6/4F92FD's Super
                    // revoke/grant passes. Carry generated ABANDON events
                    // to MainTick's post-Logic647260 dispatcher.
                    generated.extend(crate::sim::production::recalculate_strips(
                        self, rules, owner,
                    ));
                    crate::sim::superweapon::update_owned_supers(self, rules, owner);
                    crate::sim::superweapon::grant_provided_supers(self, rules, owner);
                }
            }
            if let Some(house) = self.houses.get_mut(&owner) {
                house.release_repair_latch(self.session.binary_frame);
                #[cfg(test)]
                if self
                    .house_update_append_after_test
                    .is_some_and(|(after, _)| after == owner)
                {
                    let (_, appended) = self
                        .house_update_append_after_test
                        .take()
                        .expect("matched append");
                    self.session.house_order.push(appended);
                }
            }
            index += 1;
        }
        if !defeat_pass {
            return generated;
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
            self.flag_house_to_win(alive[0], rules, false, frame_effects);
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
                    self.flag_house_to_win(owner, rules, false, frame_effects);
                }
            }
        }
        generated
    }
    /// The defeat gate's test (`0x004F8E92..0x004F8F77`): a house neither
    /// defeated nor `MultiplayPassive=` whose counts say it holds nothing.
    fn house_holds_nothing(
        &self,
        owner: InternedId,
        base_units: &[Option<InternedId>; 3],
        build_refinery_2: Option<InternedId>,
    ) -> bool {
        let Some(house) = self.houses.get(&owner) else {
            return false;
        };
        if house.is_defeated || house.multiplay_passive {
            return false;
        }
        if self.session.game_options.short_game {
            !house.tracking.short_game_alive(base_units)
        } else {
            !house.tracking.normal_game_alive(build_refinery_2)
        }
    }

    /// `HouseClass::MPlayer_Defeated @ 0x004FC0B0`, the represented part:
    /// - the Defeated flag (`+0x1F5`, `0x004FC0C2`);
    /// - for a computer house at `[IQ] MaxIQLevels` under `[AI] Paranoid`,
    ///   Computer_Paranoid (`0x004FC10D`, `sim::house_diplomacy`);
    /// - for the current house (`PlayerPtr`, `0x004FC1B0..0x004FC1B7`), the
    ///   whole-map reveal ([`Self::reveal_entire_map`] at `0x004FC20F`, which
    ///   also writes the map-clear byte `+0x241`) and the radar switched on
    ///   (`RadarClass::SetTacticalMapAvailable(1)` at `0x004FC21B`), which
    ///   Session `+0x300` keeps on ([`Self::local_player_defeated`]);
    /// - for any other house, the map-clear byte alone (`0x004FC328`), behind
    ///   a `MultiplayPassive=` test (`0x004FC30F`) that the defeat gate's own
    ///   ([`Self::house_holds_nothing`]) already passed;
    /// - the announcement (`0x004FC262..0x004FC2EA` for the current house,
    ///   `0x004FC343..0x004FC3BC` for another; the app decides which) and the
    ///   loss.
    fn mplayer_defeated(&mut self, owner: InternedId, context: UninitContext<'_>) {
        if let Some(house) = self.houses.get_mut(&owner) {
            house.is_defeated = true;
        }
        // 0x004FC0BC..0x004FC10D: a computer house at the skirmish IQ, defeated
        // under `[AI] Paranoid` in a nonzero game mode, turns the computers
        // left paranoid.
        let game_mode_nonzero = self.session.game_mode_nonzero;
        if let Some(rules) = context.rules()
            && game_mode_nonzero
            && rules.general.paranoid
            && self.houses.get(&owner).is_some_and(|house| {
                house.current_iq == rules.general.max_iq_levels
                    && !house.is_controlled_by_human(game_mode_nonzero)
            })
        {
            self.computer_paranoid(rules, context.registry(), context.effects());
        }
        if self.session.current_house == Some(owner) {
            self.reveal_entire_map(Some(owner), context);
            self.power_states
                .entry(owner)
                .or_default()
                .set_tactical_map_available();
        } else if let Some(house) = self.houses.get_mut(&owner) {
            house.map_is_clear = true;
        }
        self.sound_events
            .push(SimSoundEvent::PlayerDefeated { house: owner });
        self.flag_house_to_lose(owner, context.rules(), false, context.effects());
    }

    /// `HouseClass::Flag_To_Win4FC9E0`, common offline campaign/skirmish path.
    /// +298 is the same timer AllowWin Tags mutate; action67's argument1
    /// preserves it. Numeric conversion uses the existing native minutes
    /// reader/conversion owner. Network modes' ten-frame alignment is outside
    /// this route (the active offline nonzero mode is5).
    pub(crate) fn flag_house_to_win(
        &mut self,
        owner: InternedId,
        rules: Option<&RuleSet>,
        skip_timer: bool,
        effects: FrameEffects<'_>,
    ) -> bool {
        let savour = crate::rules::ruleset::native_minutes_to_frames(
            rules.map_or(0.03, |rules| rules.general.savour_delay_minutes),
        );
        let accepted = self.houses.get_mut(&owner).is_some_and(|house| {
            house.accept_win_result(self.session.binary_frame as i32, savour, skip_timer)
        });
        if accepted {
            self.publish_accepted_house_outcome(
                owner,
                crate::sim::house_state::HouseOutcomeKind::Victory,
                rules,
                effects,
            );
        }
        // Native returns the current +1F7 byte, including a repeated call.
        self.houses.get(&owner).is_some_and(|house| house.has_won())
    }

    /// `HouseClass::Flag_To_Lose4FCBD0`: clear Win first, accept Lost only
    /// once, then use the same timer/effect owners as Flag_To_Win.
    pub(crate) fn flag_house_to_lose(
        &mut self,
        owner: InternedId,
        rules: Option<&RuleSet>,
        skip_timer: bool,
        effects: FrameEffects<'_>,
    ) -> bool {
        let savour = crate::rules::ruleset::native_minutes_to_frames(
            rules.map_or(0.03, |rules| rules.general.savour_delay_minutes),
        );
        let accepted = self.houses.get_mut(&owner).is_some_and(|house| {
            house.accept_lose_result(self.session.binary_frame as i32, savour, skip_timer)
        });
        if accepted {
            self.publish_accepted_house_outcome(
                owner,
                crate::sim::house_state::HouseOutcomeKind::Defeat,
                rules,
                effects,
            );
        }
        self.houses
            .get(&owner)
            .is_some_and(|house| house.has_lost())
    }

    /// Shared presentation boundary of the two native Flag bodies. Their
    /// state/timer decisions stay in HouseState; no separate trigger result
    /// or announcement state is retained. Native684240 is campaign-only.
    fn publish_accepted_house_outcome(
        &mut self,
        owner: InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let campaign = !self.session.game_mode_nonzero;
        let announce = if campaign {
            self.houses
                .get(&owner)
                .is_some_and(|house| house.is_controlled_by_human(false))
        } else {
            self.session.current_house == Some(owner)
        };
        if !announce {
            return;
        }
        if campaign {
            let was_locked = self.session.input_locked();
            self.session.set_input_locked(true);
            self.publish_trigger_effects(vec![
                crate::sim::trigger_runtime::TriggerEffect::InputLockChanged {
                    locked: true,
                    was_locked,
                },
            ]);
        }
        self.sound_events
            .push(SimSoundEvent::MatchOutcome { owner, kind });
        //752700 is reached synchronously before the Flag call returns.
        effects.flush_sound(self, rules);
    }

    /// TAction1/2/67/68/69 dispatch6DEA37/6DEA67/6DDD77/6DDD93/6DDDAF.
    /// All target PlayerPtr, not the Trigger's incoming House.1/2 compare
    /// +90 with that HouseType's Country ordinal;67/68 pass skip-timer1.
    pub(crate) fn trigger_house_outcome(
        &mut self,
        action_kind: i32,
        country_index: i32,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(owner) = self
            .session
            .current_house
            .filter(|owner| self.houses.contains_key(owner))
        else {
            return false;
        };
        match action_kind {
            1 | 2 => {
                let Some(rules) = rules else {
                    return false;
                };
                let native_country = self.houses[&owner]
                    .country
                    .and_then(|country| rules.country_index(self.interner.resolve(country)))
                    .map(|index| i32::from(index.0));
                let Some(native_country) = native_country else {
                    return false;
                };
                let win = (native_country == country_index) == (action_kind == 1);
                if win {
                    self.flag_house_to_win(owner, Some(rules), false, effects);
                } else {
                    self.flag_house_to_lose(owner, Some(rules), false, effects);
                }
            }
            67 => {
                self.flag_house_to_win(owner, rules, true, effects);
            }
            68 => {
                self.flag_house_to_lose(owner, rules, true, effects);
            }
            69 => {
                //4FCDC0 calls the existing FlagWin(0) if neither result is
                //set; otherwise only resume the same timer's paused anchor.
                if self.houses[&owner].has_won() || self.houses[&owner].has_lost() {
                    self.houses
                        .get_mut(&owner)
                        .unwrap()
                        .resume_result_timer(self.session.binary_frame as i32);
                } else {
                    self.flag_house_to_win(owner, rules, false, effects);
                }
            }
            _ => return false,
        }
        // Each original TAction arm returns AL1 independently of Flag AL.
        true
    }

    /// Session `+0x300` (`0x00A8B538`), written when the current house is
    /// defeated (`0x004FC205`; the other writer, `0x005C3ABE`, is the
    /// cooperative mode's). VERA reads it as the current house's Defeated
    /// flag, which only [`Self::mplayer_defeated`] sets. Its readers here are
    /// the House radar recheck (`0x004F84FB`), the Lightning Storm's outages
    /// (`0x00539FE0`), the superweapon EVA lines (`0x004468DA`, `0x006CCCF0`
    /// and the other `SuperClass::Launch` cases) and selection navigation.
    pub(crate) fn local_player_defeated(&self) -> bool {
        self.session
            .current_house
            .and_then(|house| self.houses.get(&house))
            .is_some_and(|house| house.is_defeated)
    }

    /// HouseClass cleanup4FC6D0/4FC790/4FC820/4FC8D0. Original Steam
    ///3e81a617..08600; tools/spatial_oracle/house_blowup_all is the oracle.
    /// One owner for defeat and trigger119..122, including limbo/RTTI/Naval
    /// admission, dynamic registry order and same-callback receiver effects.
    pub(crate) fn house_blowup(
        &mut self,
        house: InternedId,
        subset: HouseBlowupSubset,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        //All four bodies re-read the current slot/count after ReceiveDamage.
        //The last-pointer guard advances an unchanged slot; immediate removal
        //exposes its successor at that same ordinal. Do not snapshot/sort IDs.
        let mut ordinal = 0;
        let mut last = None;
        while ordinal < self.substrate.entities.techno_registry_len() {
            let id = self.substrate.entities.techno_registry_at(ordinal).unwrap();
            if !self.blown_up_with(id, house, subset, rules) || last == Some(id) {
                ordinal += 1;
                continue;
            }
            //4FC731/4FC7D1/4FC879/4FC929 reads Health before Temporal
            //release; callbacks do not replace that already captured damage.
            let Some(health) = self.substrate.entities.get(id).map(|e| e.health.current()) else {
                ordinal += 1;
                continue;
            };
            // 0x004FC742: the chain warping it lets go first.
            if let Some(head) = self
                .substrate
                .entities
                .get(id)
                .and_then(|entity| entity.temporal.chain_head())
            {
                self.temporal_release_chain_no_idle(head, rules, frame_effects);
            }
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
            self.commit_direct_damage_receiver(rules, registry, event, frame_effects);
            last = Some(id);
        }
    }

    /// Blowup_All's predicate (`0x004FC6F1..0x004FC731`) for one Techno:
    /// its original owner (`TechnoClass::GetOriginalOwner @ 0x0070F820`) is
    /// `house`, and either it still belongs to `house` or no Civilian-side
    /// house takes it over from the house controlling it.
    fn blown_up_with(
        &mut self,
        id: u64,
        house: InternedId,
        subset: HouseBlowupSubset,
        rules: &RuleSet,
    ) -> bool {
        if self.techno_original_owner(id) != Some(house) {
            return false;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if subset != HouseBlowupSubset::All {
            //4FC7C3/4FC857/4FC907: typed cleanup excludes limbo, not dead
            //or unmarked objects. Building RTTI is6; other Technos use the
            //production type's native Naval read, even Aircraft/Infantry.
            if entity.lifecycle.in_limbo() {
                return false;
            }
            let building = entity.category() == crate::map::entities::EntityCategory::Structure;
            return match subset {
                HouseBlowupSubset::Buildings => building,
                HouseBlowupSubset::Land | HouseBlowupSubset::Naval => {
                    !building
                        && self
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|t| t.naval == (subset == HouseBlowupSubset::Naval))
                }
                HouseBlowupSubset::All => unreachable!(),
            };
        }
        let owner = entity.owner();
        let Some(controller) = entity.mind_control.controller() else {
            return owner == house;
        };
        // CaptureManagerClass::GetOriginalOwner @ 0x004722F0 (null without a
        // node).
        if owner == house {
            return true;
        }
        !self.set_original_owner_to_civilian(controller, id, rules)
    }

    /// TechnoGetOriginalOwner70F820: MindControlledBy's CaptureManager is
    /// read before the current Owner. No manager node means native NULL.
    /// The trigger-transfer+2CC/+2E0 producer remains an explicit module
    /// residual; ordinary ChangeOwner/Engineer capture does not set that latch.
    pub(crate) fn techno_original_owner(&self, id: u64) -> Option<InternedId> {
        let entity = self.substrate.entities.get(id)?;
        if let Some(controller) = entity.mind_control.controller() {
            self.substrate
                .entities
                .get(controller)?
                .capture_manager
                .as_ref()?
                .original_owner(id)
        } else {
            Some(entity.owner())
        }
    }

    /// Whether `house`'s HouseType side (`[house+0x34]+0xBC`) equals
    /// `SideClass::Find_Index("Civilian")` (`0x006A46D0`). `Can_Ally` asks it
    /// of the other house (`0x00501581`); [`Self::civilian_side_house`] of
    /// each house in turn. No caller guards Find_Index's -1, and a HouseType
    /// outside every `[Sides]` list keeps its constructor's -1, so without a
    /// `Civilian` side a sideless type matches (native row
    /// `missing_side_matches_negative_one_house`,
    /// `tools/garrison_oracle/allegiance.json`). `HouseState::side_index` is
    /// not asked: its fallbacks name a side for a country without one.
    pub(crate) fn house_type_is_civilian(&self, house: InternedId, rules: &RuleSet) -> bool {
        self.houses.get(&house).is_some_and(|state| {
            rules.country_side_index(self.interner.resolve(state.house_type_id()))
                == rules.side_index("Civilian")
        })
    }

    /// The first house in HouseClass::Array order whose type is of the
    /// `Civilian` side ([`Self::house_type_is_civilian`]). Five native callers
    /// walk the array this way: AI trigger condition 7 (`0x0041ECB5`),
    /// `SetOriginalOwnerToCivilian` (`0x00472341`), AnimClass::AI's
    /// MakeInfantry owner (`0x004249A7`), `CheckAutoSellOrCivilian`
    /// (`0x0045823D`) and FreeSlaves (`0x006B0B0B`).
    pub(crate) fn civilian_side_house(&self, rules: &RuleSet) -> Option<InternedId> {
        self.session
            .house_order
            .iter()
            .copied()
            .find(|&owner| self.house_type_is_civilian(owner, rules))
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
        let Some(civilian) = self.civilian_side_house(rules) else {
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
