//! `Simulation` map-cell services: smudge and tiberium reduction, sound
//! anchors, outcome checks, lighting and ore growth, map bounds and the visible
//! map area, trigger polling and fog-view preparation.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::dirty_cells::DirtyCells;
use crate::sim::intern::InternedId;
use crate::sim::ore_growth::OreGrowthConfig;
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::smudge_grid::SmudgeGrid;
use crate::sim::terrain_object::TerrainObjectStore;
use crate::sim::trigger_runtime::TriggerEffect;

use super::FrameAdvanceError;
use super::FrameEffects;
#[cfg(any(test, feature = "test-support"))]
use super::HouseAiActivationOrderTestEvent;
use super::ObjectSubstrate;
use super::Simulation;
use super::UninitContext;
use super::building_working_sound;
use super::dispatch_smudge_inline;
use super::lifecycle;
use super::turret_rotate_sound;

impl Simulation {
    pub(crate) fn flush_smudge_dirty(
        smudge_grid: &mut Option<SmudgeGrid>,
        dirty_cells: &mut DirtyCells,
    ) {
        let dirty = smudge_grid
            .as_mut()
            .map(crate::sim::smudge_grid::SmudgeGrid::drain_dirty)
            .unwrap_or_default();
        for cell in dirty {
            dirty_cells.mark_cell(cell);
        }
    }

    /// Commit an AnimClass smudge at its producer boundary, using the live
    /// world map, resource, occupation and dirty-state authorities.
    pub(crate) fn commit_smudge_request_inline(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        request: crate::sim::combat::SmudgeSpawnRequest,
    ) {
        let binary_frame = self.session.binary_frame();
        let spread_enabled = self.ore_growth_config.spreads();
        let priority = self.object_placement_scope_active();
        let mut construction = crate::sim::smudge_grid::SmudgeConstruction::new(
            &mut self.substrate.next_stable_object_id,
            &self.substrate.entities,
            &mut self.native_unique_ids,
            priority,
        );
        let result = dispatch_smudge_inline(
            &request,
            rules,
            overlay_registry,
            &self.substrate.occupancy,
            &self.substrate.raw_cell_occupation,
            &mut self.scenario_rng,
            self.overlay_grid.as_mut(),
            self.resolved_terrain.as_mut(),
            self.smudge_grid.as_mut(),
            &mut self.ore_growth_state,
            self.terrain_objects.spawning_cells(),
            self.terrain_objects.cell_index(),
            binary_frame,
            spread_enabled,
            &mut self.dirty_cells,
            &mut construction,
        );
        if let Some(id) = construction.constructed_id() {
            // Full SmudgeCtor6B4A50 retires after its Mark/Place, before the
            // producer can make another RNG draw or construct the next object.
            self.uninit_with_context(id, lifecycle::UninitContext::new(rules));
        }
        Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
        if let Err(error) = result {
            // Safe refusal of this optional mark on native-invalid input;
            // preserve its earlier constructor/scorch effects and stop it.
            log::error!("Smudge placement stopped: {error}");
        }
    }

    /// Execute59 at6DE313 uses +90 as a waypoint, not the +44 tail or an
    /// amount, and reduces around it ([`Self::reduce_tiberium_in_radius`]).
    pub(crate) fn trigger_reduce_tiberium_at_waypoint(
        terrain_objects: &TerrainObjectStore,
        ore_growth_config: &OreGrowthConfig,
        ore_growth_state: &mut OreGrowthState,
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        substrate: &ObjectSubstrate,
        resolved_terrain: &mut Option<ResolvedTerrainGrid>,
        overlay_grid: &mut Option<OverlayGrid>,
        dirty_cells: &mut DirtyCells,
        waypoint: i32,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> bool {
        let Some(cell) = session.waypoint_cell(waypoint) else {
            return false;
        };
        if resolved_terrain.is_none() || overlay_grid.is_none() {
            return false;
        }
        if let Err(error) = Self::reduce_tiberium_in_radius(
            terrain_objects,
            ore_growth_config,
            ore_growth_state,
            session,
            scenario_rng,
            substrate,
            resolved_terrain,
            overlay_grid,
            dirty_cells,
            (cell.0 as u16, cell.1 as u16),
            rules,
            Some(overlay_registry),
        ) {
            log::error!("Tiberium trigger action failed: {error}");
            return false;
        }
        true
    }

    /// `MapClass::Reduce_Tiberium_In_Radius @ 0x0057B790`, called by trigger
    /// action 59 and team script action 28. Map57B790..57B860 tests121
    /// offsets with sqrtApprox/ftol and always reduces the unchanged centre
    /// pointer, never the candidate. Seven original executions (Steam
    /// gamemd, real and Dummy, signed extremes) each observed109 ordered
    /// calls480A80(receiver,1). Packed subtraction leaves offsets
    /// independent of the centre, so reuse that constant count without
    /// adding another floating distance kernel. Every call is kept, even
    /// after depletion; the shared owner performs each native lookup and
    /// owns RNG/queues/radar/cleanup. A malformed native Image stops the
    /// remaining calls: prior reductions stay, no later RNG is replayed.
    pub(crate) fn reduce_tiberium_in_radius(
        terrain_objects: &TerrainObjectStore,
        ore_growth_config: &OreGrowthConfig,
        ore_growth_state: &mut OreGrowthState,
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        substrate: &ObjectSubstrate,
        resolved_terrain: &mut Option<ResolvedTerrainGrid>,
        overlay_grid: &mut Option<OverlayGrid>,
        dirty_cells: &mut DirtyCells,
        cell: (u16, u16),
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<(), crate::rules::overlay_types::TiberiumImageError> {
        for _ in 0..109 {
            Self::reduce_tiberium_at_with_native_context(
                terrain_objects,
                ore_growth_config,
                ore_growth_state,
                session,
                scenario_rng,
                substrate,
                resolved_terrain,
                overlay_grid,
                dirty_cells,
                cell,
                1,
                rules,
                overlay_registry,
            )?;
        }
        Ok(())
    }

    pub(crate) fn reduce_tiberium_at_with_native_context<A: Into<i32>>(
        terrain_objects: &TerrainObjectStore,
        ore_growth_config: &OreGrowthConfig,
        world_ore_growth_state: &mut OreGrowthState,
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        substrate: &ObjectSubstrate,
        world_resolved_terrain: &mut Option<ResolvedTerrainGrid>,
        world_overlay_grid: &mut Option<OverlayGrid>,
        world_dirty_cells: &mut DirtyCells,
        cell: (u16, u16),
        amount: A,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<
        crate::sim::tiberium::ReduceTiberiumOutcome,
        crate::rules::overlay_types::TiberiumImageError,
    > {
        let mut ctx = crate::sim::tiberium::ReduceTiberiumContext {
            overlay_grid: world_overlay_grid.as_mut(),
            ore_growth_state: &mut *world_ore_growth_state,
            overlay_registry,
            tiberium_types: Some(&rules.tiberium_types),
            resolved_terrain: world_resolved_terrain.as_mut(),
            source_object_cells: Some(terrain_objects.spawning_cells()),
            live_objects: Some(crate::sim::tiberium::NativeCellObjectView::new(
                &substrate.occupancy,
                terrain_objects.cell_index(),
            )),
            // ore growth/spread — scenario stream. Direct field: this literal
            // co-borrows other &mut self fields, so an all-self accessor conflicts.
            rng: Some(scenario_rng),
            binary_frame: session.binary_frame(),
            spread_enabled: ore_growth_config.spreads(),
            dirty_cells: Some(world_dirty_cells),
        };
        crate::sim::tiberium::reduce_tiberium(&mut ctx, cell, amount.into())
    }

    /// Intern a name for an app command payload or harness, returning its
    /// InternedId; a known name keeps its id.
    #[inline]
    pub fn intern(&mut self, s: &str) -> crate::sim::intern::InternedId {
        self.interner.intern(s)
    }

    /// Current world coordinate of whatever object holds one app-side loop
    /// handle, whether that is an anim or a `MoveSound`-carrying entity.
    ///
    /// Read-only, and audio-agnostic: `sim/` never learns that a sound exists.
    /// It exists because gamemd's owner re-drives its handle every update with
    /// its own coordinate (`AnimClass::UpdateLoopingSound @ 0x00750D40`, whose
    /// first argument is the owner's coords), and the app-side owner needs the
    /// same value. Anim ids and entity stable ids come from one
    /// `allocate_stable_id` counter — `spawn_anim_at_world` rejects a
    /// collision against both maps — so one id names at most one object.
    pub fn looping_sound_owner_coord(
        &self,
        id: crate::sim::anim_class::AnimId,
    ) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        if let Some(carrier) = crate::sim::bomb::ticking_sound_carrier(id) {
            return self.bomb_ticking_coord(carrier);
        }
        // `TechnoClass::AI_Update` re-drives a gattling type's loop at its
        // Location every frame (`0x006F9E76..0x006F9EA8`).
        if let Some(techno) = crate::sim::combat::gattling::gattling_sound_techno(id) {
            return self
                .substrate
                .entities
                .get(techno)
                .map(Self::movement_sound_world);
        }
        // The same per-visit re-drive for a building's turret rotation loop on
        // its `+0x488` handle (`0x006FA04F`).
        if let Some(techno) = turret_rotate_sound::turret_rotate_sound_techno(id) {
            return self
                .substrate
                .entities
                .get(techno)
                .map(Self::movement_sound_world);
        }
        // `BuildingClass::Update` re-drives the `+0x6B4` working sound on an
        // unchanged visit (`building_working_sound`).
        if building_working_sound::working_sound_building(id).is_some() {
            return self.working_sound_owner_coord(id);
        }
        if let Some(coord) = self.anim_absolute_coord(id) {
            return Some(coord);
        }
        self.substrate
            .entities
            .get(id)
            .map(Self::movement_sound_world)
    }

    /// The object's coordinate for its sounds. Sound owners hold no terrain,
    /// so an object without an exact coordinate uses its stored level.
    /// The object's Location (`Object+0x9C`), where its sounds play.
    pub(crate) fn movement_sound_world(
        entity: &crate::sim::game_entity::GameEntity,
    ) -> crate::sim::anim_class::AnimWorldCoord {
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        crate::sim::anim_class::AnimWorldCoord {
            x,
            y,
            z: crate::sim::movement::ground_pose::object_world_z_leptons(entity, None),
        }
    }

    /// Accepted House result after its serialized SavourDelay has expired.
    /// Simulation termination and app outcome presentation share this query.
    pub fn ready_outcome_for_owner(
        &self,
        owner: InternedId,
    ) -> Option<crate::sim::house_state::HouseOutcomeState> {
        // Main_Tick @0x0055D360 completes Queue_AI/its network wait before
        // Main_Game @0x0048CE8A..0x0048CEA1 routes the result. A House may
        // publish its request earlier in that suspended frame; the remaining
        // command tail and terminal score/RNG commit must finish first.
        // Source: original gamemd.exe instructions.
        if self.frame_continuation.is_some()
            || self.session.current_house() != Some(owner)
            || !self.houses.contains_key(&owner)
        {
            return None;
        }
        self.session
            .outcome_request()
            .map(|kind| crate::sim::house_state::HouseOutcomeState { kind })
    }

    pub(super) fn natural_outcome_exit_ready(&self) -> bool {
        self.session.outcome_request().is_some()
    }

    pub(super) fn termination_frame_requested(&self) -> bool {
        self.quit_requested || self.connection_lost || self.natural_outcome_exit_ready()
    }

    /// Advance the global ambient scalar before ore and active superweapons.
    /// A storm selecting Ion later in this frame can first move it next frame.
    /// `LogicClass::PerTickUpdate` (`0x0055B33D..0x0055B4D7`) restarts the
    /// fade timer with `NukeAmbientChangeRate=` while the nuke flash fades in
    /// or out (`0x0055B389`, `0x0055B392`), else with
    /// `DominatorAmbientChangeRate=` while the Psychic Dominator is active
    /// (`PsyDom::Active`, `0x0055B3A4`), else with `AmbientChangeRate=`. The
    /// chrono screen's test (`0x0055B39B`) shares the nuke arm. Original
    /// controls: superweapon_oracle.json/ambient_step, chrono=1.
    pub(crate) fn tick_scenario_lighting_transition(&mut self, rules: &RuleSet) {
        let (rate_nonzero, normal_interval, ambient_step) =
            self.session.lighting().ambient_change_terms(rules);
        let interval_frames = if self.session.lighting().nuke_flash_running()
            || self.session.chrono_screen().active()
        {
            self.session.lighting().nuke_change_rate()
        } else if crate::sim::superweapon::psychic_dominator::active(self) {
            self.session.lighting().dominator_change_rate()
        } else {
            normal_interval
        };
        let frame = self.session.binary_frame();
        if self.session.lighting_mut().advance_transition_if_due(
            frame,
            rate_nonzero,
            interval_frames,
            ambient_step,
        ) {
            self.trigger_runtime.mark_ambient_light_changed();
            //55B4C6 refreshes Cell scalars without calling palette53AD00.
            self.publish_global_lighting(crate::sim::light_sources::PaletteUpdate::Preserve);
        }
    }

    pub(super) fn tick_ore_growth_rungs(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Result<(), FrameAdvanceError> {
        let native_growth_ready = !rules.tiberium_types.is_empty()
            && self.ore_growth_state.native_tiberium_state().classes.len()
                == rules.tiberium_types.len()
            && self.overlay_grid.is_some();
        if native_growth_ready && let Some(registry) = overlay_registry {
            let dirty_start = self
                .overlay_grid
                .as_ref()
                .unwrap()
                .pending_dirty_cells()
                .len();
            self.tick_native_growth_driver(rules, registry, effects)
                .map_err(|error| {
                    FrameAdvanceError::new(
                        self.session.tick(),
                        self.session.binary_frame(),
                        0,
                        error.to_string(),
                    )
                })?;
            self.tick_native_spread_driver(rules, registry, effects)
                .map_err(|error| {
                    FrameAdvanceError::new(
                        self.session.tick(),
                        self.session.binary_frame(),
                        0,
                        error.to_string(),
                    )
                })?;
            // Preserve the existing ordinary-ore Recalc batch. Structural and
            // CellAnim Mark paths have already completed synchronously. The
            // tail is walked by index: a deferred Recalc writes the cell's
            // terrain and navigation receipts, never this dirty list.
            if let (Some(grid), Some(terrain)) =
                (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
            {
                for index in dirty_start..grid.pending_dirty_cells().len() {
                    let cell = grid.pending_dirty_cells()[index];
                    recalculate_tiberium_cell(grid, terrain, registry, cell);
                }
            }
            self.finish_terrain_navigation_changes(rules, &[]);
        }
        Ok(())
    }

    /// Overlay Mark5FC570 ->Recalc47D2B0 completes before its Logic caller
    /// returns. Both the global ore rung and a Terrain AI slot must publish
    /// the path view before the next object; retain render dirty receipts.
    /// The Recalc's neighbour reads stayed inside each Mark
    /// (`OverlayRecalc::DeferOrdinary`), so the batch makes none.
    pub(crate) fn publish_tiberium_cells(
        &mut self,
        rules: &RuleSet,
        registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        cells: &[(u16, u16)],
    ) {
        if let (Some(grid), Some(terrain)) =
            (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
        {
            for &cell in cells {
                recalculate_tiberium_cell(grid, terrain, registry, cell);
            }
        }
        self.finish_terrain_navigation_changes(rules, &[]);
    }

    /// `MapClass::In_Bounds @ 0x00568300` — the active diamond test against
    /// `MapClass+0xF4` (size width) and `MapClass+0xF8` (size height). No map
    /// size (a headless fixture) admits nothing.
    pub fn map_cell_in_bounds(&self, cell: (i16, i16)) -> bool {
        self.map_size_diamond().is_some_and(|(width, height)| {
            crate::map::playfield::size_diamond_contains(width, height, cell)
        })
    }

    /// The cell addressing of this map. The loaded terrain owns it; before
    /// Fill the scenario's Size selects it.
    pub(crate) fn cell_grid(&self) -> crate::map::cell_index::CellGrid {
        self.resolved_terrain
            .as_ref()
            .map(|terrain| terrain.cell_grid())
            .or_else(|| {
                self.map_size_diamond()
                    .map(|(w, h)| crate::map::cell_index::CellGrid::for_map_size(w, h))
            })
            .unwrap_or_default()
    }

    /// `MapClass+0xF4/+0xF8`, the `In_Bounds` diamond's width and height.
    pub fn map_size_diamond(&self) -> Option<(i32, i32)> {
        self.playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
    }

    /// MapRect, `MapClass+0x124..+0x130` as `[left, top, width, height]`:
    /// `MapClass::Resize @ 0x00565C10` writes `(1, 1, SizeW + SizeH - 1,
    /// SizeW + SizeH - 1)`. Random cell draws read it
    /// (`MapClass::PlaceCrateAtRandomCell @ 0x0056BD8B`,
    /// `LightningStorm::Start @ 0x00539EE6`).
    pub(crate) fn map_rect(&self) -> Option<[i32; 4]> {
        let (width, height) = self.map_size_diamond()?;
        let extent = width.wrapping_add(height).wrapping_sub(1);
        Some([1, 1, extent, extent])
    }

    /// Install the initial normalized MapClass playfield authority.
    ///
    /// `MapClass::Set_Clipped_LocalSize @ 0x00567230` establishes the five
    /// predicate fields. Size height is retained separately because later
    /// action-40 writers normalize another raw LocalSize against the same Size.
    pub fn install_playfield_from_map_header(&mut self, header: &crate::map::map_file::MapHeader) {
        self.playfield_size_height = Some(header.height as i32);
        self.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds::from_map_header(
            header,
        ));
        self.playfield_revision = 0;
    }

    /// Apply YR trigger action 0x28's mutable visible-map-area authority.
    ///
    /// `FUN_006E21E0` writes LocalSize, normalizes it, recalculates all cells,
    /// rebuilds zone connectivity/all levels, and refreshes radar before it
    /// returns to `TriggerAction__Execute @ 0x006DD8B0`. Rust performs every
    /// corresponding owned update synchronously here, so later actions and
    /// the same master frame observe one coherent authority. `context`
    /// carries the trigger's rules, overlay table and frame effects into the
    /// See callbacks, whose discoveries may Grand_Open a building.
    pub fn change_visible_map_area(
        &mut self,
        raw_local_size: [i32; 4],
        mut context: UninitContext<'_>,
    ) -> bool {
        let (Some(current), Some(size_height)) =
            (self.playfield_bounds, self.playfield_size_height)
        else {
            // A live scenario always has MapClass Size authority. Headless
            // fixtures without a map must not invent a rectangular fallback.
            return false;
        };
        let bounds = crate::sim::cell_rect::PlayfieldBounds::from_raw_local_size(
            current.base,
            size_height,
            raw_local_size,
        );
        self.playfield_bounds = Some(bounds);
        // FUN_006E21E0 runs the complete radar-surface rebuild on every
        // execution. Do not deduplicate equal normalized writers.
        self.playfield_revision = self.playfield_revision.wrapping_add(1);

        // `MapClass::Set_Clipped_LocalSize @ 0x00567230` exact-recomputes the
        // canonical TechnoClass+0x3D5 byte for every represented Techno. This
        // must precede later action consumers and may both demote and promote;
        // ordinary per-cell movement's writer is deliberately only 0 -> 1.
        // RESIDUAL: native stores, gates and Sees one object at a time
        // (0x0056730E..0x00567351); VERA gates every object and stores every
        // byte before the first See. They differ only if a See's discovery
        // springs a Tag that changes a later object's owner, IsAlive or Limbo
        // within the same action.
        let membership_updates = self
            .substrate
            .entities
            .iter_sorted()
            .map(|(stable_id, entity)| {
                let member = crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (
                        i32::from(entity.position.rx()),
                        i32::from(entity.position.ry()),
                    ),
                    Some(bounds),
                    self.resolved_terrain.as_ref(),
                );
                // 0x00567314..0x00567347: a 0 -> 1 flip, an owner passing
                // IsHumanPlayer 0x0050B6F0, not a Building, IsAlive (+0x90)
                // and not InLimbo (+0x81). No Health test, so an infantry
                // corpse still Sees; a dying SHP wreck does not, since its
                // native UnInit already cleared IsAlive (`is_active`). The
                // clients are those on which 0x0050B6F0 passes.
                let see = !entity.in_playfield()
                    && member
                    && entity.category() != EntityCategory::Structure
                    && entity.is_active()
                    && !entity.lifecycle.in_limbo();
                let clients = if see {
                    self.human_player_clients(entity.owner())
                } else {
                    Vec::new()
                };
                (stable_id, member, clients)
            })
            .collect::<Vec<_>>();
        // Set_Clipped_LocalSize's exact store (`0x0056730E`).
        for &(stable_id, member, _) in &membership_updates {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.in_playfield.store(member);
            }
        }
        // The false->true callback is Object See(0, 0) (vt+0x120 at
        // 0x00567351): Aircraft 0x0041ADF0 or Techno 0x0070ADC0. A network
        // game's 0x0050B6F0 passes only on the owner's own client
        // (0x0050B716), so See writes only that map, as Display See's
        // projection does (`refresh_techno_shroud`); other modes keep one
        // shared reveal. It commits before this action returns.
        let network = self.session.execution_topology().is_lockstep();
        for (stable_id, _, clients) in membership_updates {
            if network {
                for client in clients {
                    self.reveal_uncached_object_sight(
                        stable_id,
                        false,
                        false,
                        Some(client),
                        context.reborrow(),
                    );
                }
            } else if !clients.is_empty() {
                self.reveal_uncached_object_sight(
                    stable_id,
                    false,
                    false,
                    None,
                    context.reborrow(),
                );
            }
        }

        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.recalc_playfield_attributes(bounds);
        }

        // Rebuild from the retained structure-blocked PathGrid. PathGrid does
        // not cache the outside flag; ZoneGrid does, so a full rebuild covers
        // native connectivity plus every retained hierarchy level without
        // discarding dynamic building blockers.
        if let Some(path_grid) = self.path_grid.clone() {
            self.rebuild_zone_grid_full(path_grid);
        }

        // Native calls RefreshRadar after the cell/zone rebuild. The distinct
        // playfield revision is the global geometry invalidation seam; the
        // presentation-acknowledged dirty-cell batch remains cell-local.
        // Action40's final reverse BuildingClass array walk
        // (6E225C..6E227C) calls virtual+4A0(1) synchronously. The radar
        // geometry follows the revision; its simulation discovery prelude
        // must finish here even when no object AI follows this action.
        for index in (0..self.substrate.entities.building_registry_len()).rev() {
            if let Some(id) = self.substrate.entities.building_registry_at(index) {
                self.update_techno_discovery_from_shroud(id);
            }
        }
        true
    }

    pub(super) fn poll_triggers_for_master_frame(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        // YR LogicClass::Update polls scenario triggers before the live-object walk.
        let effects = self.advance_triggers(rules, overlay_registry, frame_effects);
        self.publish_trigger_effects(effects);
    }

    /// Commit synchronous Tag outcomes to the existing frame-output boundary.
    /// Object callbacks and the master-frame poll share this single queue; a
    /// single effect comes as a one-element array.
    pub(crate) fn publish_trigger_effects(
        &mut self,
        effects: impl IntoIterator<Item = TriggerEffect>,
    ) {
        self.trigger_effects.extend(effects);
    }

    /// Drain app-owned outcomes after their authoritative trigger actions ran.
    /// Production consumes trigger effects through `SimFrameOutput`; only
    /// fixture replay and trigger tests drain them directly.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn drain_trigger_effects(&mut self) -> Vec<TriggerEffect> {
        std::mem::take(&mut self.trigger_effects)
    }

    /// Tell the fog that presentation prepared a view for the named owner
    /// (F10 boundary method: the app requests, sim owns the write). Bumps the
    /// view generation render dirty-gates on. Returns false, and bumps
    /// nothing, when the owner name is not interned yet.
    pub fn prepare_fog_view_for(&mut self, owner: &str) -> bool {
        if self.interner.get(owner).is_none() {
            return false;
        }
        self.fog.bump_view_generation();
        true
    }

    /// Configure the command input delay (F10 boundary method; the app pushes
    /// its configured value once at match install).
    pub fn set_input_delay_ticks(&mut self, ticks: u64) {
        self.input_delay_ticks = ticks;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn trace_house_ai_activation_order(
        &mut self,
        event: HouseAiActivationOrderTestEvent,
    ) {
        self.house_ai_activation_order_test_trace.push(event);
    }
}

/// One cell of [`Simulation::publish_tiberium_cells`]'s batch: the deferred
/// Recalc of a cell that still holds an overlay.
fn recalculate_tiberium_cell(
    grid: &mut crate::sim::overlay_grid::OverlayGrid,
    terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    cell: (u16, u16),
) {
    if grid.cell(cell.0, cell.1).overlay_id.is_some() {
        grid.recalculate_deferred_cell(terrain, registry, cell);
    }
}
