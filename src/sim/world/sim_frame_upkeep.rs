//! `Simulation` frame upkeep: house vision, spy-sat and fog refresh, super
//! weapon announcements and building-down visits, wakes, state-hash cadence,
//! profiling and unit voice visits.

#[cfg(any(test, feature = "test-support"))]
use std::sync::Arc;

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::pathfinding::PathGrid;
use crate::sim::production;
use crate::sim::vision;

use super::FrameEffects;
#[cfg(any(test, feature = "test-support"))]
use super::HouseAiActivationOrderTestEvent;
use super::SimSoundEvent;
use super::Simulation;
use super::WAKE_DRAW_FLAGS;
use super::wake_anchor_for;

impl Simulation {
    pub(crate) fn default_vision_range_for_category(category: EntityCategory) -> u16 {
        match category {
            EntityCategory::Infantry => 5,
            EntityCategory::Unit => 6,
            EntityCategory::Aircraft => 8,
            EntityCategory::Structure => 7,
        }
    }

    /// Find whether this House has a native-eligible SpySat provider. This is
    /// the House-rung edge detector; consumers read the persisted house latch.
    /// `HouseClass::Update_SpySat @ 0x00508F60` walks the House's building
    /// list (House+0x68, items `+0x6C`, count `+0x78`) in its order.
    fn house_has_spy_sat_provider(&self, owner: InternedId, rules: &RuleSet) -> bool {
        let selling =
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Selling);
        let house = &self.houses[&owner];
        let require_discovery = self.building_services_require_discovery(house);
        for &stable_id in house.base_projection().buildings() {
            let Some(entity) = self.substrate.entities.get(stable_id) else {
                continue;
            };
            let coarse_candidate = !entity.lifecycle.in_limbo()
                && entity.lifecycle.cell_marked()
                && (!require_discovery || entity.seldom.discovery.discovered_by_current_house())
                && entity.mission.current() != selling
                && entity.mission.queued() != selling
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.spy_sat);
            if !coarse_candidate {
                continue;
            }
            // Ares SpySat_Update_CheckEligible10024620 (hook508F91) skips a
            // jammed or EMPed uplink to the next one.
            if rules.ares()
                && (entity.seldom.ares.radar_jam.is_jammed() || entity.emp_remaining() > 0)
            {
                continue;
            }
            // The first coarse candidate decides the house. Warp-out is a
            // blocking result, not a reason to continue to a later uplink.
            return !entity.is_warped_out();
        }
        false
    }

    /// HouseClass::Update_SpySat508F60, reached inside its own House's
    /// dirty-radar suffix (4F850A). The receiver clears +5779 at508F67,
    /// before the Map callback and its possible discovery/dirty writes.
    /// Source: original gamemd instructions; map/latch/cue owners are shared.
    pub(super) fn assess_house_spy_sat(&mut self, owner: InternedId, rules: &RuleSet) {
        self.power_states.house_mut(owner).skip_radar_recheck();
        let active = self.house_has_spy_sat_provider(owner, rules);
        if self.houses[&owner].spy_sat_active() != active {
            //509064/509012 call Map while577A still has its OLD value.
            if active {
                self.reveal_map_for_owner(owner, rules);
            } else {
                self.reset_shroud_for_owner(owner, rules);
            }
            if let Some(house) = self.houses.get_mut(&owner) {
                house.record_spy_sat(active);
                // House509069/509017 stores577A after the Map callback;
                //509090/50903F then requests the current PlayerPtr's cue.
                // Native750920 receives center8192, volume1f and NULL handle.
                // Other map callers (crates/infiltration) do not reach this
                // aggregate edge. Source: tools/input_oracle/spy_sat_audio.md.
                if self.session.current_house() == Some(owner) {
                    let sound = if active {
                        &rules.general.spy_sat_activation_sound
                    } else {
                        &rules.general.spy_sat_deactivation_sound
                    };
                    if let Some(sound_id) = sound {
                        self.sound_events.emit(SimSoundEvent::VocCentered {
                            sound_id: rules.sound_name(sound_id),
                        });
                    }
                }
            }
        }
    }

    /// Raw entity fixtures bypass lifecycle dirty writers and the House rung;
    /// explicitly assess their supplied providers through the same SpySat owner.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn reconcile_active_vision_structures(&mut self, rules: &RuleSet) {
        for owner in self.houses.order().to_vec() {
            self.assess_house_spy_sat(owner, rules);
        }
    }

    /// Early mechanisms inside one reached House update.
    ///
    /// gamemd-derived: `LogicClass__PerTickUpdate @ 0x0055AFB0`, House loop
    /// `0x0055B68D..0x0055B6B1`, walks the House array forward, skips nulls,
    /// and reloads the live count after every `HouseClass__Update @ 0x004F8440`
    /// call. The bounded Rust mechanisms are anger decay before the verified
    /// AI-activation block `0x004F8564..0x004F85B7`.
    pub(super) fn update_house_anger_and_activation(&mut self, owner: InternedId, rules: &RuleSet) {
        let current_frame = self.session.binary_frame() as i32;
        let policy = self.session.rule_policy();
        let iq_production = Some(rules.general.iq_production);
        let represented = self.houses.contains_key(&owner);
        if let Some((house, house_order)) = self.houses.get_mut_with_order(&owner) {
            house.decay_anger_scores(house_order, current_frame);
        }
        if represented {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_house_ai_activation_order(HouseAiActivationOrderTestEvent::HouseAngerDecay(
                owner,
            ));
            if let Some(iq_production) = iq_production {
                self.houses
                    .get_mut(&owner)
                    .expect("represented House remains registered during its update")
                    .update_ai_activation(policy, iq_production);
                #[cfg(any(test, feature = "test-support"))]
                self.trace_house_ai_activation_order(
                    HouseAiActivationOrderTestEvent::HouseActivation(owner),
                );
            }
        }
    }

    /// The per-frame fog pass: ends last frame's sight and marks the retained
    /// footprints again ([`Self::refresh_sight_sources`]). It reveals and
    /// releases no source sight or Gap Generator deposits:
    /// footprints open, move and close only at their native events
    /// ([`crate::sim::vision`] lists them).
    pub(super) fn refresh_fog(&mut self, config: &vision::VisionConfig, rules: &RuleSet) {
        self.refresh_sight_sources(config, rules);

        // Gap events publish their retained cell counters synchronously; a
        // view refresh neither reclassifies nor replays an active generator.

        // Diagnostic: log fog grid stats on first tick to debug coverage issues.
        if self.session.tick() == 1 {
            log::info!(
                "Fog grid: {}x{}, {} owners",
                self.fog.width(),
                self.fog.height(),
                self.fog.by_owner().len()
            );
            for (owner, vis) in self.fog.by_owner().iter() {
                let total = vis.width() as u32 * vis.height() as u32;
                let visible_count = vis.cells_raw().iter().filter(|c| **c & 0x02 != 0).count();
                let revealed_count = vis.cells_raw().iter().filter(|c| **c & 0x01 != 0).count();
                log::info!(
                    "  Owner '{}': {}/{} visible, {}/{} revealed",
                    owner,
                    visible_count,
                    total,
                    revealed_count,
                    total
                );
            }
            use std::collections::BTreeMap as DiagMap;
            let mut entity_stats: DiagMap<String, (u32, u16, u16, u16, u16)> = DiagMap::new();
            for entity in self.substrate.entities.values() {
                let entry = entity_stats
                    .entry(self.interner.resolve(entity.owner()).to_string())
                    .or_insert((0, u16::MAX, u16::MAX, 0, 0));
                entry.0 += 1;
                entry.1 = entry.1.min(entity.position.rx());
                entry.2 = entry.2.min(entity.position.ry());
                entry.3 = entry.3.max(entity.position.rx());
                entry.4 = entry.4.max(entity.position.ry());
            }
            for (owner, (count, min_rx, min_ry, max_rx, max_ry)) in &entity_stats {
                log::info!(
                    "  Entities '{}': {} units, rx={}..{}, ry={}..{}",
                    owner,
                    count,
                    min_rx,
                    max_rx,
                    min_ry,
                    max_ry
                );
            }
        }
    }

    /// `BuildingClass::OnConstructionComplete @ 0x00445F80`, the
    /// `SuperWeapon=` announce block (`0x004468AD..0x00446995`): a completed
    /// building whose type names a `[SuperWeaponTypes]` section
    /// (`Type+0x16F0 != -1`) reports itself. Only the owner-independent half
    /// lives here; the listener gates (local player, `IsAlliedWith(PlayerPtr)`,
    /// `[0xA8B538]`, `GameMode`, `AuxBuilding` ownership) are the app's.
    /// Ares's hook there (4468F4, Ares.dll 100349D0) announces the first Super
    /// of the type its owner can use (BuildingTypeExt 1000D610), including
    /// `SuperWeapon2=` and `SuperWeapons=`.
    pub(crate) fn announce_super_weapon_building_complete(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) {
        let Some(building) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|e| e.category() == EntityCategory::Structure && !e.death.is_dying())
        else {
            return;
        };
        let owner = building.owner();
        let sw_type = if rules.ares() {
            crate::sim::superweapon::first_available_building_super(self, rules, building)
                .map(|instance| instance.type_id())
        } else {
            self.interner
                .object_type(building.type_ref(), rules)
                .and_then(|obj| obj.super_weapon.as_deref())
                .map(|section| self.interner.intern(section))
        };
        let Some(sw_type) = sw_type else {
            return;
        };
        self.sound_events
            .emit(SimSoundEvent::SuperWeaponDetected { owner, sw_type });
    }

    /// The Selling mission's visit in the building's own LogicVector slot
    /// (`BuildingClass::Update`: UpdateAnimation `0x0043FE22`, then Sell
    /// through the mission dispatch in `TechnoClass::Update` `0x0043FE56`), so
    /// stage 1's Scenario draws and the completion keep native order among the
    /// frame's objects. `sim::building_construction` steps the pack-up and
    /// `production::production_sell` runs Sell's stages. Every visit stops the
    /// repair (`ToggleRepair(0)`, `0x00449C41`) and re-arms the mission for
    /// the next frame (Sell returns 1). A building at Health 0 runs no mission
    /// handler (`MissionClass::AI 0x005B30A7`), a warped one no AI
    /// (`GameEntity::ai_frozen`). An object that enters the map (the crew, an
    /// undeploy's unit) marks the frame's spawn refresh; the object turn
    /// reports a building that left it.
    pub(crate) fn visit_building_down(
        &mut self,
        sid: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        use crate::sim::building_construction::PackUpFrame;
        let now = self.session.binary_frame() as i32;
        let visit = {
            let Some(entity) = self.substrate.entities.get(sid) else {
                return;
            };
            // The visit is the Selling mission's handler; Assign_Mission
            // (which Selling does not refuse) can end it early.
            if !entity.building_down()
                || entity.mission.current().known()
                    != Some(crate::sim::mission::MissionType::Selling)
                || entity.ai_frozen()
                || !entity.is_ai_alive()
                || entity.lifecycle.in_limbo()
            {
                return;
            }
            let Some(entity) = self.substrate.entities.get_mut(sid) else {
                return;
            };
            let mut status = entity.mission.handler_state();
            let visit = entity.advance_building_down(&mut status);
            entity.mission.set_handler_state(status);
            entity.mission.write_dispatch_epilogue(now, 1);
            visit
        };
        // Every visit stops a repair first (`ToggleRepair(0)`, `0x00449C41`).
        production::toggle_repair(self, rules, sid, production::RepairControl::Stop);
        let spawned = match visit {
            PackUpFrame::StageZero => {
                production::sell_stage_zero(self, rules, sid, effects);
                false
            }
            PackUpFrame::StageOne => {
                production::sell_stage_one(self, rules, overlay_registry, sid, effects)
            }
            PackUpFrame::Complete => {
                production::sell_complete(self, rules, overlay_registry, sid, effects)
            }
            PackUpFrame::Waiting => false,
        };
        if spawned {
            *self.mission_spawned_entities = true;
        }
    }

    /// The Drive, Hover and Ship locomotors each spawn `Rules->Wake`
    /// (Rules+0x94) from their own process; [`wake_anchor_for`] holds the
    /// three gates. All three build `AnimClass(Wake, Location, 0, 1, 0x600,
    /// 0, 0)` at the unit's exact `Location` (`+0x9C`), not the cell centre,
    /// so each wake stays where the hull was and the trail forms behind it as
    /// the unit advances. The Location keeps its Z: the Anim's Unlimbo
    /// (`0x005F4EC0`) stores it through `AnimTypeClass` vt+0x6C (`0x0041CF80`,
    /// unchanged) and `SetLocation` (`0x005F6940`), so a Hover's wake starts
    /// at its hovering hull, not on the water below.
    ///
    /// RESIDUAL: this one pass builds every wake after movement, where Drive
    /// (`0x004B078C..0x004B0823`), Hover (`0x00514A21..0x00514AC3`) and Ship
    /// (`0x0069FE97..0x0069FEEB`) build theirs inside each unit's Process. A
    /// wake draws no random number, so three things differ, none visible:
    /// its creation order (its AbstractClass ID) relative to objects made
    /// later in the same frame; the gate (alive, moving now, cell, bridge)
    /// is read after the whole LogicVector, not in the unit's own Process;
    /// and its first AI comes a frame late, since a native wake made
    /// mid-walk joins that frame's walk.
    pub(crate) fn spawn_wakes_for_frame(
        &mut self,
        rules: &RuleSet,
        tube_turns: &std::collections::BTreeSet<u64>,
    ) {
        if !self.session.binary_frame().is_multiple_of(10)
            && !self.session.binary_frame().is_multiple_of(8)
        {
            return;
        }
        let binary_frame = self.session.binary_frame();
        let terrain = self.resolved_terrain.as_ref();
        let rules_context =
            crate::sim::movement::SpeedRules::new(rules, &self.interner, &self.houses);
        let wake_positions: Vec<crate::sim::anim_class::AnimWorldCoord> = self
            .substrate
            .entities
            .values()
            .filter_map(|entity| {
                // Unit AI 7363A4 and Infantry AI 51BAB8 took TubeMovement
                // in place of their locomotor, even on the exit turn.
                if tube_turns.contains(&entity.stable_id()) {
                    return None;
                }
                let underwater = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.underwater);
                wake_anchor_for(entity, rules_context, underwater, terrain, binary_frame)
            })
            .collect();
        if wake_positions.is_empty() {
            return;
        }
        // `new AnimClass(Wake, &coord, 0, 1, 0x600, 0, 0)`: delay 0, one
        // loop, draw flags 0x600, no ZAdjust, not reversed. Going through the
        // AnimClass constructor puts the wake on the sorted ground layer with
        // its art `YSortAdjust=-288`, so it sorts under the hull instead of
        // over it.
        let wake_name = self.interner.intern(&rules.general.wake.name);
        for world in wake_positions {
            let (rx, ry, sub_x, sub_y, level) = world.to_cell_sub_z();
            let descriptor = AnimClassSpawnDescriptor {
                loop_count: 1,
                draw_flags: WAKE_DRAW_FLAGS,
                ..AnimClassSpawnDescriptor::new(wake_name, rx, ry, sub_x, sub_y, level)
            };
            if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
                log::debug!(
                    "wake [{}] did not construct: {error}",
                    rules.general.wake.name
                );
            }
        }
    }

    /// A frame reads only the canonical `path_grid`. A fixture that builds its
    /// grid beside the simulation seeds it here once; after that the installed
    /// grid (and any republish of it) wins over the fixture's copy.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_fixture_path_grid(&mut self, path_grid: Option<&PathGrid>) {
        if self.path_grid.is_none()
            && let Some(grid) = path_grid
        {
            *self.path_grid = Some(Arc::new(grid.clone()));
        }
    }

    /// From the next frame on, hash the state for the TickResult only after
    /// frames whose tick is a multiple of `frames` (1: every frame), and after
    /// every frame that ends the match (an EXIT, a quit, the current house's
    /// win or loss, a lost connection), whose `frame_committed` is false. A
    /// fixed frame count, so the same commands always hash the same frames.
    pub fn hash_state_every(&mut self, frames: std::num::NonZeroU64) {
        *self.state_hash_interval = Some(frames);
    }

    /// Whether the next frame's tick falls on the interval
    /// [`Self::hash_state_every`] last set, so that frame hashes the state.
    /// A frame that ends the match hashes too, which is not known beforehand.
    pub fn next_frame_hashes_state(&self) -> bool {
        self.state_hash_interval.is_some_and(|frames| {
            self.session
                .tick()
                .saturating_add(1)
                .is_multiple_of(frames.get())
        })
    }

    /// Start or stop diagnostic wall-clock measurement. A transition starts a
    /// fresh sample window; repeated requests preserve the accumulated profile.
    /// New and deserialized simulations start disabled with an empty profile.
    /// The telemetry build and the in-game performance panel turn it on.
    pub fn set_frame_profiling(&mut self, enabled: bool) {
        if *self.frame_profiling != enabled {
            self.frame_profile.reset();
            *self.frame_profiling = enabled;
        }
    }

    pub fn frame_profile(&self) -> &crate::sim::frame_profile::FrameProfile {
        &self.frame_profile
    }

    /// Start a fresh sample window while measurement stays on. Report points
    /// close one window per capture, so each point covers the time since the
    /// previous point instead of the whole elapsed match.
    #[cfg(feature = "telemetry")]
    pub fn reset_frame_profile(&mut self) {
        self.frame_profile.reset();
    }

    /// Install the sole audio owner's read-only pending-interest projection
    /// immediately before an admitted frame. No voice names/state live here.
    pub fn prepare_unit_voice_visits(&mut self, owners: std::collections::BTreeSet<u64>) {
        *self.unit_voice_visit_interest = owners;
    }

    /// Emit only interested reached heads, in the existing ordered channel.
    pub(super) fn record_unit_voice_visit(&mut self, owner: u64) {
        if self.unit_voice_visit_interest.contains(&owner) {
            self.sound_events
                .emit(SimSoundEvent::UnitVoiceVisit { owner });
        }
    }

    /// A deploy conversion's voice hand-off (UnitClass::Deploy
    /// `0x00739971..0x007399B2`, BuildingClass::Mission_Selling
    /// `0x0044A0D4..0x0044A115`). The audio owner moves the handle and latch;
    /// a pending line's visit interest follows it to the new object.
    pub(crate) fn move_unit_voice(&mut self, from: u64, to: u64) {
        if self.unit_voice_visit_interest.remove(&from) {
            self.unit_voice_visit_interest.insert(to);
        }
        self.sound_events
            .emit(SimSoundEvent::UnitVoiceMoved { from, to });
    }
}
