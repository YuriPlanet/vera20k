use super::*;
use crate::app::match_runtime::sim_host::SimWrite;

/// The random-map seed file the `.SED` launch branch recognises. Written into
/// VERA's folder for it (`map::source::random_map_dir`), where `map_load`
/// reads it; gamemd keeps it in the game folder.
pub(crate) const RANDMAP_SED_FILE: &str = "RandMap.Sed";
/// Description the setup dialog stamps onto a randomized configuration; it also
/// becomes the sentinel row's displayed name.
pub(super) const RANDOM_MAP_DESCRIPTION_KEY: &str = "TXT_RANDOM_MAP_DESCRIPTION";
pub(super) const RANDOM_MAP_DESCRIPTION_FALLBACK: &str = "Random Map";
/// The players slider is the last of the setup dialog's six option rows.
const SETUP_PLAYERS_ROW: usize = 5;
/// Retail's `[General] CliffBackImpassability=`; the preview only needs it
/// because terrain resolution takes it, not because cliffs affect the image.
const RANDOM_MAP_PREVIEW_CLIFF_BACK_IMPASSABILITY: u8 = 2;
/// Where the generated preview is written. The chooser's sentinel row reads this
/// back, so writing it is what makes the random-map thumbnail appear there.
const RANDMAP_PREVIEW_FILE: &str = "RandMap.img";

/// Native RMG map-storage identity used by its guarded Resize predicate.
///
/// `RandomMapGenerator @ 0x00599D48` compares the normalized current theater,
/// player count, width, and height against its cached MapSeed clone at
/// `0x00599B8E..0x00599BD3`. Other options can regenerate content in the same
/// storage and therefore do not participate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RandomMapStorageKey {
    theater: i32,
    num_players: i32,
    width: i32,
    height: i32,
}

impl RandomMapStorageKey {
    fn from_options(options: &crate::map::rmg::RmgOptions) -> Self {
        let mut normalized = options.clone();
        normalized.normalize();
        Self {
            theater: normalized.theater,
            num_players: normalized.num_players,
            width: normalized.width,
            height: normalized.height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RandomMapStorageDecision {
    key: RandomMapStorageKey,
    reinitialize: bool,
}

impl RandomMapStorageDecision {
    #[cfg(test)]
    pub(super) fn reinitializes(self) -> bool {
        self.reinitialize
    }

    /// Apply only the guarded native Resize side effect. Preview redraws never
    /// receive this decision and cannot reconstruct the process dummy.
    fn apply_resize_to(&self, shared_cell_dummy: &crate::map::resolved_terrain::SharedCellDummy) {
        if self.reinitialize {
            // The JZ at 0x00599D48..0x00599D95 skips this call when the cached
            // tuple matches. On a miss, MapClass::Resize @ 0x00565C10 invokes
            // CellClass::Constructor @ 0x0047BBF0 on the fixed dummy in place.
            shared_cell_dummy.reconstruct_for_map_resize();
        }
    }
}

/// Resolve one RMG preview without touching process-global MapClass state.
fn build_random_map_preview_grid(
    map_file: &crate::map::map_file::MapFile,
    theater: Option<&crate::map::theater::TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&crate::rules::terrain_rules::TerrainRules>,
    main: &mut crate::sim::rng::MainRngDraws,
    selector_cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    let mut raw_draw = || main.next_u32();
    let mut selector = selector_cache.begin_load(&mut raw_draw);
    // RMG InitMap supplies explicit Clear cells. Its preview never borrows a
    // Scenario cursor; equal-bound Fill remains zero-cost.
    let mut scenario_fill_ranged = |low, high| {
        debug_assert_eq!((low, high), (0, 0));
        0
    };
    crate::map::resolved_terrain::ResolvedTerrainGrid::build_with_variant_selector(
        map_file,
        theater,
        asset_manager,
        terrain_rules,
        None,
        None,
        false,
        RANDOM_MAP_PREVIEW_CLIFF_BACK_IMPASSABILITY,
        &mut scenario_fill_ranged,
        &mut selector,
    )
}

/// Immutable projection of the process owner's Rules/ART for one generation.
/// Dropping the job retires only this view, not the live Rules registry.
struct RandomMapPreviewRules {
    rules: crate::rules::ruleset::RuleSet,
}

impl RandomMapPreviewRules {
    fn from_processed(
        owner: &crate::rules::process_owner::NativeRulesProcessOwner,
        processed: &crate::rules::native_processing::ProcessedRulesLayers,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
    ) -> Option<Self> {
        let mut rules = crate::rules::ruleset::RuleSet::from_processed_rules(processed).ok()?;
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
            owner.fixed_art(),
        ));
        // TerrainRead71DFD0..71E02A retains historical SHP/header defaults,
        // then Rules RadarColor. Preview641602 consumes that final type RGB,
        // independently of the image InitTheater later makes resident.
        rules.bind_terrain_assets(
            assets,
            theater_ext,
            crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
        );
        Some(Self { rules })
    }
}

/// GenerateTerrainPreview6413C9..6416EF: select the Overlay color/density
/// suffix, or the ground head's retained TerrainType color when no overlay exists.
/// Device words stay in the app/render layer; core supplies the TMP color pair.
fn random_map_preview_cells(
    map_file: &crate::map::map_file::MapFile,
    resolved_terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    preview_rules: Option<&RandomMapPreviewRules>,
    assets: Option<&crate::assets::asset_manager::AssetManager>,
    theater_ext: &str,
) -> Vec<crate::map::rmg::preview::PreviewCell> {
    let headers = match (preview_rules, assets) {
        (Some(rules), Some(assets)) => {
            let registry = rules.rules.overlay_registry();
            let names = map_file
                .overlays
                .iter()
                .filter_map(|overlay| {
                    Some((
                        overlay.overlay_id,
                        registry.name(overlay.overlay_id)?.to_owned(),
                    ))
                })
                .collect();
            crate::render::overlay_atlas::compute_overlay_radar_colors(
                assets,
                registry,
                crate::render::overlay_atlas::OverlayRadarColorSource::OverlayType,
                &names,
                theater_ext,
                &map_file.header.theater,
                rules.rules.art(),
            )
        }
        _ => Default::default(),
    };
    let format = crate::render::native_surface_format::RGB565;
    let overlay_radar = |id: u8, stage: u8, ground: ([u8; 3], [u8; 3])| {
        let flags = preview_rules?.rules.overlay_registry().flags(id)?;
        let declared = format.pack_rgb8(flags.radar_color.unwrap_or([0; 3]));
        let header = format.pack_rgb8(headers.get(&(id, stage)).copied().unwrap_or([0; 3]));
        let (source, amount) = if flags.tiberium {
            let source = if declared > 5 { declared } else { header };
            (
                source,
                if stage < 2 {
                    25
                } else if stage < 5 {
                    50
                } else {
                    75
                },
            )
        } else {
            let source = if header > 5 { header } else { declared };
            if source == 0 {
                return None;
            }
            (source, 100)
        };
        if amount == 100 {
            let rgb = format.unpack_rgb8(source);
            return Some((rgb, rgb));
        }
        // 6414E4 narrows the first mix against Cell's low/right word;
        // 64166E/64167F mix again against its high(0)/low halves.
        let first = format.mix_packed_rgb16(source, format.pack_rgb8(ground.1), amount);
        let left = format.mix_packed_rgb16(source, 0, amount);
        let right = format.mix_packed_rgb16(source, first, amount);
        Some((format.unpack_rgb8(left), format.unpack_rgb8(right)))
    };
    // This is a fresh derived projection of the generator's chronological
    // placements and fixed type foundations, discarded after each preview.
    // MapPlaceDown5684BB / CellAddContent47E91C prepend Terrain. The only
    // other RMG objects are Buildings, whose RTTI6 arm47E8D2 appends instead;
    // therefore the latest Terrain occupying a cell is its ground head.
    let mut terrain_heads = std::collections::HashMap::new();
    if let Some(rules) = preview_rules {
        for object in &map_file.terrain_objects {
            let Some(kind) = rules
                .rules
                .terrain_object_type_case_insensitive(&object.name)
            else {
                continue;
            };
            let offsets = match kind.occupied_cell_offsets() {
                Ok(offsets) => offsets,
                Err(error) => {
                    // Actual Terrain construction rejects the native
                    // unterminated list; preview must not invent a footprint.
                    log::warn!("Cannot project random-map Terrain: {error}");
                    continue;
                }
            };
            // RMG placements are positive map cells. The constructor's zero
            // sentinel71BC34 skips Unlimbo; PlaceDown guards each allocation.
            if (object.rx, object.ry) == (0, 0) {
                continue;
            }
            for (dx, dy) in offsets {
                let cell = (
                    object.rx.wrapping_add_signed(dx),
                    object.ry.wrapping_add_signed(dy),
                );
                if let Some(index) =
                    resolved_terrain.native_fixed_cell_index(cell.0 as i16, cell.1 as i16)
                {
                    let cell = &resolved_terrain.cells()[index];
                    terrain_heads.insert((cell.rx, cell.ry), kind.radar_color);
                }
            }
        }
    }
    let terrain_radar = |x, y, ground: ([u8; 3], [u8; 3])| {
        // TerrainClass::WhatAmI71D300 / GenerateTerrainPreview6415E1..641689:
        // use the head's type RGB, without the Overlay density arm's first mix.
        // FUN4BF650's75 mode has its own packed-word semantics, not RGB lerp.
        let source = format.pack_rgb8(*terrain_heads.get(&(x, y))?);
        let left = format.mix_packed_rgb16(source, format.pack_rgb8(ground.0), 75);
        let right = format.mix_packed_rgb16(source, format.pack_rgb8(ground.1), 75);
        Some((format.unpack_rgb8(left), format.unpack_rgb8(right)))
    };
    let mut cells = crate::map::rmg::preview::preview_cells_from_map(
        map_file,
        resolved_terrain,
        &overlay_radar,
        &terrain_radar,
    );
    for cell in &mut cells {
        cell.left = format.unpack_rgb8(format.pack_rgb8(cell.left));
        cell.right = format.unpack_rgb8(format.pack_rgb8(cell.right));
    }
    cells
}

/// A random-map generation handed to a worker thread.
///
/// The worker only *generates*; colouring a preview needs the terrain resolver
/// and therefore the asset manager, so the main thread rasterises everything the
/// worker hands it. What matters is that the expensive part is off the UI
/// thread — that is what lets frames render while it runs.
pub(crate) struct RandomMapGenerationJob {
    receiver: std::sync::mpsc::Receiver<RandomMapUpdate>,
    /// Kept back from the worker because rasterising needs it: the resolver
    /// reads theater data to decide each cell's final tile.
    theater: Box<crate::map::theater::TheaterData>,
    terrain_rules: Box<crate::rules::terrain_rules::TerrainRules>,
    preview_rules: Option<RandomMapPreviewRules>,
    /// Set when OK started this generation. Accept cannot run until the map
    /// exists, so it is deferred to whoever collects the result.
    accept_on_finish: bool,
}

/// Provenance-bearing copy of the eight Scenario start-staging cells written
/// by the accepted random-map setup run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AcceptedRmgStartStaging {
    starts: Vec<crate::map::waypoints::Waypoint>,
}

impl AcceptedRmgStartStaging {
    /// A guest's staging: Full_Init copies the host's `'W'` cells into
    /// waypoints 0..7 for a random map (`0x006873E6..0x0068740E`).
    pub(crate) fn from_scenario_staging(
        staging: &crate::map::preview::ScenarioStartStaging,
    ) -> Self {
        Self {
            starts: staging
                .defined_cells()
                .map(|(slot, (rx, ry))| crate::map::waypoints::Waypoint {
                    index: slot as u32,
                    rx: rx as u16,
                    ry: ry as u16,
                })
                .collect(),
        }
    }

    /// Staging from the shell's retained Scenario `+0x11C0` cells
    /// (`0x0058B830`), which Full_Init copies again (`0x006873E6`): a network
    /// guest's launch and a Skirmish rematch after the accepted setup was
    /// consumed. `None` unless `RandMap.Sed` is selected and the
    /// shell holds a start.
    pub(crate) fn from_retained_staging(
        staging: &crate::map::preview::ScenarioStartStaging,
        selected_map_file: Option<&str>,
    ) -> Option<Self> {
        let random_map = selected_map_file.is_some_and(|file| {
            file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED)
        });
        (random_map && staging.defined_cells().next().is_some())
            .then(|| Self::from_scenario_staging(staging))
    }

    fn from_generated(generated: &crate::map::rmg::GeneratedMap) -> Self {
        // VERA: every start of a random map for up to thirty players.
        let mut slots = [None; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT];
        for &(slot, rx, ry) in &generated.start_waypoints {
            if let Some(entry) = slots.get_mut(usize::from(slot)) {
                *entry = Some(crate::map::waypoints::Waypoint {
                    index: u32::from(slot),
                    rx,
                    ry,
                });
            }
        }
        Self {
            starts: slots.into_iter().flatten().collect(),
        }
    }

    pub(crate) fn into_waypoint_table(
        self,
    ) -> std::collections::HashMap<u32, crate::map::waypoints::Waypoint> {
        self.starts
            .into_iter()
            .map(|waypoint| (waypoint.index, waypoint))
            .collect()
    }

    /// Borrow-only fact projection used to validate the complete prefix before
    /// surrendering the unique accepted-staging token.
    pub(crate) fn waypoint_table_for_admission(
        &self,
    ) -> std::collections::HashMap<u32, crate::map::waypoints::Waypoint> {
        self.starts
            .iter()
            .copied()
            .map(|waypoint| (waypoint.index, waypoint))
            .collect()
    }
}

/// Accepted random-map artifacts split at the app/loading boundary. The large
/// generated map remains presentation-only; only `start_staging` may supply
/// gameplay start cells.
#[derive(Debug)]
pub(crate) struct AcceptedRandomMapLaunch {
    presentation_preview: crate::map::rmg::GeneratedMap,
    start_staging: AcceptedRmgStartStaging,
}

impl AcceptedRandomMapLaunch {
    pub(crate) fn into_parts(self) -> (crate::map::rmg::GeneratedMap, AcceptedRmgStartStaging) {
        (self.presentation_preview, self.start_staging)
    }
}

/// Accepted launch-artifact ownership across setup and loading composition.
///
/// The candidate belongs only to the open setup dialog. The accepted bundle
/// carries both presentation fallback and exact start staging for the matching
/// `.SED`; gameplay map data still regenerates through the seed-file reader
/// after the match reseed.
/// gamemd provenance: accepted caller 0x005E8590 persists `RandMap.Sed`, while
/// Scenario read 0x00684620 unconditionally reaches the generator again.
#[derive(Default)]
pub(crate) struct RandomMapGenerationRetention {
    candidate: Option<crate::map::rmg::GeneratedMap>,
    accepted_launch: Option<(String, AcceptedRandomMapLaunch)>,
    /// The native MapSeed+0x178 clone survives repeated Generate/OK work while
    /// the dialog is open. A four-field mismatch replaces its backing storage,
    /// and common dialog teardown destroys it before a later reopen.
    map_storage_key: Option<RandomMapStorageKey>,
    /// Immutable process cursor after successful generation, while no live
    /// Simulation exists. Inactive load attempts receive draw-free snapshots;
    /// only successful Simulation installation retires this transport.
    pending_process_mapgen: Option<crate::rng_continuation::MapGenRngContinuation>,
    /// The globals `ComputeScenarioHeaderFromMap @ 0x0058B820` fills at every
    /// Generate (count `0x00ABED0C`, cells `0x00ABE304`, pairs `0x00ABE328`,
    /// bounds `0x00ABDFD0..0x00ABECD0`). OK and Cancel leave them; they live
    /// for the process, empty before the first Generate. `0x0058BB30` copies
    /// them back into Scenario when the random map is selected, and
    /// `DrawStartPositions 0x00640710` numbers the starts from them.
    generated_header: crate::map::preview::ScenarioStartStaging,
}

impl RandomMapGenerationRetention {
    /// Publish the completed generator's exact resident cursor before any
    /// preview choice or fallible load admission. Both dialog preview and
    /// launch-time .SED regeneration use this owner selection.
    pub(crate) fn publish_mapgen_continuation(
        &mut self,
        resident: Option<&mut crate::sim::world::Simulation>,
        continuation: crate::rng_continuation::MapGenRngContinuation,
    ) {
        match resident {
            Some(simulation) => {
                assert!(
                    self.pending_process_mapgen.is_none(),
                    "resident Simulation cannot coexist with pending cold MapGen transport"
                );
                simulation.install_mapgen_continuation(continuation);
            }
            None => self.pending_process_mapgen = Some(continuation),
        }
    }

    /// Snapshot the sole previous process cursor for an inactive load attempt.
    /// The selected authored load has no MapGen draws before installation:
    /// ReadScenario684620→ReadScenarioINI686730→Full_Init/Fill/postload.
    /// Later Engineer repairs draw through the live Simulation. Native
    /// evidence: tools/input_oracle/campaign_start.py and its meta coverage.
    pub(crate) fn snapshot_mapgen_continuation_for_fresh_load(
        &self,
        resident: Option<&crate::sim::world::Simulation>,
    ) -> Option<crate::rng_continuation::MapGenRngContinuation> {
        match resident {
            Some(simulation) => {
                assert!(
                    self.pending_process_mapgen.is_none(),
                    "resident Simulation cannot coexist with pending cold MapGen transport"
                );
                Some(simulation.mapgen_continuation_for_fresh_load())
            }
            None => self
                .pending_process_mapgen
                .as_ref()
                .map(crate::rng_continuation::MapGenRngContinuation::snapshot_for_fresh_load),
        }
    }

    /// Retire cold transport only at successful incoming-Simulation install.
    /// Error/cancel and incoming-None fallback do not call this method. The
    /// app transports an outgoing resident cursor before dropping that owner.
    pub(crate) fn discard_pending_mapgen_on_simulation_install(&mut self) {
        self.pending_process_mapgen = None;
    }

    pub(super) fn map_storage_decision(
        &self,
        options: &crate::map::rmg::RmgOptions,
    ) -> RandomMapStorageDecision {
        let key = RandomMapStorageKey::from_options(options);
        RandomMapStorageDecision {
            key,
            reinitialize: self.map_storage_key != Some(key),
        }
    }

    pub(super) fn begin_map_storage_decision(
        &mut self,
        options: &crate::map::rmg::RmgOptions,
    ) -> RandomMapStorageDecision {
        let decision = self.map_storage_decision(options);
        if decision.reinitialize {
            // A later Rules/asset failure must not admit the old A cache key
            // against the partially rebuilt B registry on the next attempt.
            self.map_storage_key = None;
        }
        decision
    }

    pub(super) fn commit_map_storage_decision(&mut self, decision: RandomMapStorageDecision) {
        self.map_storage_key = Some(decision.key);
    }

    pub(super) fn destroy_map_storage(&mut self) {
        // RandomMapSetupDialog__Run @ 0x00595BC0 destroys DAT_00ABE150 and
        // nulls it at 0x00595CB2..0x00595CC2 whenever the modal returns.
        self.map_storage_key = None;
    }

    pub(super) fn begin_generation(&mut self) {
        self.candidate = None;
        self.accepted_launch = None;
    }

    pub(super) fn finish_generation(&mut self, generated: crate::map::rmg::GeneratedMap) {
        // RandomMapGenerator__CreateStartingPoints calls 0x0058B820 at
        // 0x00594EE3, inside the generate block.
        self.generated_header = crate::app::loading::composition::scenario_start_staging(
            &generated.map_file,
            &generated.start_waypoints,
        );
        self.candidate = Some(generated);
    }

    /// VERA extension: the generated map the open setup dialog shows, for
    /// Export Map.
    pub(super) fn candidate(&self) -> Option<&crate::map::rmg::GeneratedMap> {
        self.candidate.as_ref()
    }

    /// The last Generate's start header (`0x0058B820`'s globals), which a
    /// random map's preview numbers its starts from.
    pub(crate) fn generated_header(&self) -> crate::map::preview::ScenarioStartStaging {
        self.generated_header
    }

    fn cancel_setup(&mut self) {
        self.candidate = None;
        self.accepted_launch = None;
    }

    pub(super) fn accept_setup(&mut self, selected_map_file: &str) {
        self.accepted_launch = self.candidate.take().map(|generated| {
            let start_staging = AcceptedRmgStartStaging::from_generated(&generated);
            (
                selected_map_file.to_owned(),
                AcceptedRandomMapLaunch {
                    presentation_preview: generated,
                    start_staging,
                },
            )
        });
    }

    pub(super) fn select_map(&mut self, selected_map_file: &str) {
        if self
            .accepted_launch
            .as_ref()
            .is_some_and(|(accepted_file, _)| {
                !accepted_file.eq_ignore_ascii_case(selected_map_file)
            })
        {
            self.accepted_launch = None;
        }
    }

    pub(super) fn take_acceptance_for_loading(
        &mut self,
        selected_map_file: Option<&str>,
    ) -> Option<AcceptedRandomMapLaunch> {
        let (accepted_file, accepted) = self.accepted_launch.take()?;
        selected_map_file
            .is_some_and(|selected| accepted_file.eq_ignore_ascii_case(selected))
            .then_some(accepted)
    }

    /// Presentation-only compatibility accessor used by lifecycle tests.
    /// Production transfers the complete accepted value so staging cannot be
    /// dropped or reconstructed from the preview.
    #[cfg(test)]
    pub(super) fn take_preview_for_loading(
        &mut self,
        selected_map_file: Option<&str>,
    ) -> Option<crate::map::rmg::GeneratedMap> {
        self.take_acceptance_for_loading(selected_map_file)
            .map(|accepted| accepted.presentation_preview)
    }
}

/// What the generator worker sends back as it goes.
pub(super) enum RandomMapUpdate {
    /// Cross-thread receipt emitted immediately before the worker enters the
    /// real generator. Unlike a test-thread counter, this follows the run that
    /// the production receiver actually owns.
    Started,
    /// The map at one of the boundaries the original redraws its preview at.
    Progress(Box<crate::map::rmg::build::GenerationSnapshot>),
    /// The finished map.
    Finished(Box<crate::map::rmg::GeneratedMap>),
}

/// Plain generation inputs resolved on the app thread before the worker is
/// spawned. This is the production preparation boundary used by both live UI
/// generation and the retail lifecycle proof.
pub(super) struct PreparedRandomMapGeneration {
    pub(super) theater: crate::map::theater::TheaterData,
    pub(super) terrain_rules: crate::rules::terrain_rules::TerrainRules,
    preview_rules: Option<RandomMapPreviewRules>,
    pub(super) settings: crate::map::rmg::RmgSettings,
    pub(super) resolved_inputs: crate::map::rmg::build::ResolvedTheaterInputs,
    pub(super) blocks: crate::map::rmg::theater_blocks::TheaterTileBlocks,
    pub(super) tech_types: Vec<crate::map::rmg::phases::tech_buildings::TechType>,
    /// The resolved Ares `[Urban]` lists.
    #[cfg(feature = "ares")]
    pub(super) urban_catalog: crate::map::rmg::phases::urban_areas::UrbanCatalog,
}

/// What a generation worker may construct, owned so it can cross threads.
pub(super) struct RandomMapObjects {
    tech_types: Vec<crate::map::rmg::phases::tech_buildings::TechType>,
    /// The Ares `[Urban]` catalog and the process Main stream its foot picks
    /// draw (`Randomizer::Global()`), when the urban switch is set.
    #[cfg(feature = "ares")]
    urban: Option<(
        crate::map::rmg::phases::urban_areas::UrbanCatalog,
        crate::sim::rng::MainRngDraws,
    )>,
}

impl RandomMapObjects {
    /// Tech buildings only.
    pub(super) fn tech(tech_types: Vec<crate::map::rmg::phases::tech_buildings::TechType>) -> Self {
        Self {
            tech_types,
            #[cfg(feature = "ares")]
            urban: None,
        }
    }

    /// Run `generate` over the borrowed generation objects.
    fn generate<R>(
        &self,
        generate: impl FnOnce(&crate::map::rmg::build::GenerationObjects<'_>) -> R,
    ) -> R {
        #[cfg(feature = "ares")]
        if let Some((catalog, main)) = &self.urban {
            let main_draw = || main.next_u32();
            return generate(&crate::map::rmg::build::GenerationObjects {
                tech_types: &self.tech_types,
                urban_areas: Some(crate::map::rmg::phases::urban_areas::UrbanInputs {
                    catalog,
                    main_draw: &main_draw,
                }),
            });
        }
        generate(&crate::map::rmg::build::GenerationObjects::tech(
            &self.tech_types,
        ))
    }
}

impl PreparedRandomMapGeneration {
    /// These are the same resolved generation inputs, including the process
    /// Powerups table that the generated scenario inherits. Projection consumes
    /// no RNG and never mutates the live Rules owner.
    pub(super) fn multiplayer_digest(&self) -> String {
        let mut bytes = b"VERA20k RMG dependencies v2\0".to_vec();
        bytes.extend_from_slice(self.settings.multiplayer_digest().as_bytes());
        // Retained rows may differ despite identical current source text.
        // These are the land-derived inputs that generation actually consumes.
        bytes.extend(self.resolved_inputs.wheel_impassable.map(u8::from));
        for facts in &self.resolved_inputs.overlay_zones {
            bytes.extend([
                facts.land,
                u8::from(facts.early_land),
                facts.zone_type.unwrap_or(0xFF),
            ]);
        }
        bytes.extend_from_slice(crate::util::sha256::sha256_hex(&self.theater.ini_data).as_bytes());
        bytes.extend_from_slice(self.blocks.multiplayer_digest().as_bytes());
        for technology in &self.tech_types {
            bytes.extend_from_slice(&(technology.name.len() as u64).to_le_bytes());
            bytes.extend_from_slice(technology.name.as_bytes());
            bytes.extend_from_slice(&(technology.footprint.len() as u64).to_le_bytes());
            for (x, y) in &technology.footprint {
                bytes.extend_from_slice(&x.to_le_bytes());
                bytes.extend_from_slice(&y.to_le_bytes());
            }
        }
        bytes.push(u8::from(self.preview_rules.is_some()));
        if let Some(preview) = &self.preview_rules {
            let powerups = &preview.rules.powerups;
            for slot in 0..crate::rules::powerups::POWERUP_COUNT {
                bytes.extend_from_slice(&powerups.weights[slot].to_le_bytes());
                bytes.extend_from_slice(&powerups.animation_index(slot).to_le_bytes());
                bytes.push(u8::from(powerups.over_water[slot]));
                bytes.extend_from_slice(&powerups.magnitudes[slot].bits().to_le_bytes());
            }
        }
        crate::util::sha256::sha256_hex(&bytes)
    }
}

/// Actual dialog generation mutates the one Rules owner at the native cache
/// boundary. Network agreement only inspects prospective generation inputs.
pub(super) enum RandomMapRulesPreparation<'a> {
    ReadOnly(Option<&'a crate::rules::process_owner::NativeRulesProcessOwner>),
    Generation {
        owner: Option<&'a mut crate::rules::process_owner::NativeRulesProcessOwner>,
        reinitialize: bool,
    },
}

pub(super) fn prepare_random_map_generation(
    asset_manager: &mut crate::assets::asset_manager::AssetManager,
    options: &crate::map::rmg::RmgOptions,
    preparation: RandomMapRulesPreparation<'_>,
    mode_rules_override: Option<&crate::rules::ini_parser::IniFile>,
) -> Option<PreparedRandomMapGeneration> {
    let mut settings = crate::map::rmg::RmgSettings::load(asset_manager);
    let theater_name = crate::map::rmg::emit::theater_name(options.theater);
    let Some(theater) = crate::map::theater::load_theater(asset_manager, theater_name) else {
        log::warn!("random map: theater {theater_name} unavailable");
        return None;
    };
    let prospective_rules = match &preparation {
        RandomMapRulesPreparation::ReadOnly(Some(owner)) => Some(
            owner
                .random_map_preview_projection(mode_rules_override, theater_name)
                .ok()?,
        ),
        _ => None,
    };
    let native_rules = match preparation {
        RandomMapRulesPreparation::ReadOnly(owner) => owner,
        RandomMapRulesPreparation::Generation {
            owner,
            reinitialize,
        } => {
            if let Some(owner) = owner {
                if let Err(error) = owner.prepare_random_map_preview(
                    mode_rules_override,
                    theater_name,
                    reinitialize,
                ) {
                    log::warn!("Random-map Rules preparation failed: {error}");
                    return None;
                }
                Some(&*owner)
            } else {
                None
            }
        }
    };
    let processed = prospective_rules
        .as_ref()
        .or_else(|| native_rules.and_then(|owner| owner.prepared_random_map_rules()));
    let tech_types = match (native_rules, processed) {
        (Some(owner), Some(processed)) => crate::map::rmg::tech_catalog::resolve(
            processed.neutral_tech_buildings(),
            owner.fixed_art(),
        ),
        _ => Vec::new(),
    };
    #[cfg(feature = "ares")]
    let urban_catalog = match (native_rules, processed) {
        (Some(owner), Some(processed)) => crate::map::rmg::tech_catalog::resolve_ares_urban(
            &settings,
            processed,
            owner.fixed_art(),
        ),
        _ => Default::default(),
    };
    let preview_rules = if let Some((owner, processed)) = native_rules.zip(processed) {
        Some(RandomMapPreviewRules::from_processed(
            owner,
            processed,
            asset_manager,
            theater.extension,
        )?)
    } else {
        None
    };
    // VERA: the theater's civilian buildings, for an opted-in Urban Presence;
    // the load resolves the same list from the same Rules and art.
    if options.wants_vera_catalog()
        && let Some((owner, preview)) = native_rules.zip(preview_rules.as_ref())
    {
        settings.vera_urban = crate::map::rmg::tech_catalog::resolve_vera_urban(
            &preview.rules,
            owner.fixed_art(),
            &tech_types,
            &settings.extras,
            theater.extension,
            theater_name,
            options.seed_u16(),
            |file| asset_manager.contains(file),
        );
    }
    let terrain_rules = preview_rules
        .as_ref()
        .map(|preview| preview.rules.terrain_rules.clone())
        .unwrap_or_default();
    let cliff_back_impassability = preview_rules.as_ref().map_or_else(
        || crate::rules::ruleset::GeneralRules::default().cliff_back_impassability,
        |preview| preview.rules.general.cliff_back_impassability,
    );
    let empty_overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    let resolved_inputs = crate::map::rmg::build::ResolvedTheaterInputs::from_theater(
        &theater,
        &terrain_rules,
        preview_rules
            .as_ref()
            .map_or(&empty_overlays, |preview| preview.rules.overlay_registry()),
        cliff_back_impassability,
        &settings.extras,
    );
    let blocks =
        crate::map::rmg::theater_blocks::TheaterTileBlocks::build(&theater.lookup, |name| {
            asset_manager.get(name)
        });
    Some(PreparedRandomMapGeneration {
        theater,
        terrain_rules,
        preview_rules,
        settings,
        resolved_inputs,
        blocks,
        tech_types,
        #[cfg(feature = "ares")]
        urban_catalog,
    })
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(super) fn spawn_random_map_generation_worker(
    _options: crate::map::rmg::RmgOptions,
    _settings: crate::map::rmg::RmgSettings,
    _resolved_inputs: crate::map::rmg::build::ResolvedTheaterInputs,
    _blocks: crate::map::rmg::theater_blocks::TheaterTileBlocks,
    _objects: RandomMapObjects,
    _shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    _map_storage_decision: RandomMapStorageDecision,
) -> std::io::Result<std::sync::mpsc::Receiver<RandomMapUpdate>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Random map generation is unavailable in the single-threaded Web preview",
    ))
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(super) fn spawn_random_map_generation_worker(
    options: crate::map::rmg::RmgOptions,
    settings: crate::map::rmg::RmgSettings,
    resolved_inputs: crate::map::rmg::build::ResolvedTheaterInputs,
    blocks: crate::map::rmg::theater_blocks::TheaterTileBlocks,
    objects: RandomMapObjects,
    shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    map_storage_decision: RandomMapStorageDecision,
) -> std::io::Result<std::sync::mpsc::Receiver<RandomMapUpdate>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("random-map-generate".to_string())
        .spawn(move || {
            map_storage_decision.apply_resize_to(&shared_cell_dummy);
            // This receipt is ordered directly ahead of the call, on the same
            // worker. It therefore proves a production generation entry even
            // when the caller and worker use different test threads.
            let _ = sender.send(RandomMapUpdate::Started);
            let generated = objects.generate(|objects| {
                crate::map::rmg::build::generate_map_observed(
                    &options,
                    &settings,
                    &resolved_inputs,
                    &blocks,
                    objects,
                    // A closed receiver means the dialog went away; dropping
                    // what we produce is the correct outcome, not an error.
                    &mut |view| {
                        if !draws_preview(view.point()) {
                            return;
                        }
                        let _ = sender.send(RandomMapUpdate::Progress(Box::new(view.snapshot())));
                    },
                )
            });
            if generated.unfilled_start_slots > 0 {
                log::warn!(
                    "Random map is short of spawns: {} start slot(s) could \
                     not be filled; those players have no start position",
                    generated.unfilled_start_slots
                );
            }
            let _ = sender.send(RandomMapUpdate::Finished(Box::new(generated)));
        })?;
    Ok(receiver)
}

/// Apply the finished worker receipt to the same shell owners the live poll
/// mutates. The return value tells the caller whether deferred OK may now run.
pub(super) fn finish_random_map_generation_owners(
    resident_simulation: Option<&mut crate::sim::world::Simulation>,
    runtime: &mut crate::app::frontend::skirmish_session::MultiplayerShellRuntime,
    retention: &mut RandomMapGenerationRetention,
    modal: &mut crate::ui::skirmish_shell::RandomMapSetupModalState,
    mut generated: crate::map::rmg::GeneratedMap,
    preview: Option<crate::map::rmg::preview::PreviewImage>,
    accept_on_finish: bool,
) -> bool {
    if resident_simulation.is_some() {
        assert!(
            retention.pending_process_mapgen.is_none(),
            "resident Simulation cannot coexist with pending cold MapGen transport"
        );
    }
    // Original598960 seeds and advances the resident g_MapGenRng, returning
    // without restoring it (59951F..599527). The dialog's OK/Cancel branches
    // only select a modal result, so publish before preview disposition.
    let continuation = generated.take_mapgen_continuation();
    retention.publish_mapgen_continuation(resident_simulation, continuation);
    runtime.replay_random_map_preview_construction(&generated.construction_trace);
    // VERA: the map's fairness report, for the dialog's readout.
    let fairness = generated.fairness;
    retention.finish_generation(generated);
    modal.finish_generate(preview);
    modal.fairness = fairness;
    accept_on_finish
        && matches!(
            modal.accept(),
            crate::ui::skirmish_shell::AcceptOutcome::Commit(_)
        )
}

/// The non-presentation state transition at each Generate/implicit-OK entry.
/// It is shared by the live App entry and the retail lifecycle harness so a
/// repeated run always invalidates the same candidate and accepted launch.
pub(super) fn begin_random_map_generation_owners(
    runtime: &mut crate::app::frontend::skirmish_session::MultiplayerShellRuntime,
    retention: &mut RandomMapGenerationRetention,
    options: &crate::map::rmg::RmgOptions,
) {
    runtime.remember_random_map_options(options);
    retention.begin_generation();
}

/// VERA extension: the file of the player's own preset in `slot` (from 0):
/// the options in the `.SED` text, under a name the seed browser does not
/// list.
fn custom_preset_path(dir: &std::path::Path, slot: usize) -> std::path::PathBuf {
    dir.join(format!("VeraPreset{}.vpr", slot + 1))
}

/// Write the preset in `slot`; false when the file could not be written.
fn save_custom_preset(
    dir: &std::path::Path,
    slot: usize,
    options: &crate::map::rmg::RmgOptions,
) -> bool {
    let path = custom_preset_path(dir, slot);
    let written = crate::util::file_system::create_dir_all(dir)
        .and_then(|()| crate::util::file_system::write(&path, options.to_sed_bytes()));
    if let Err(error) = &written {
        log::warn!(
            "random map: could not write preset {}: {error}",
            path.display()
        );
    }
    written.is_ok()
}

/// The player's own presets in `dir`, by slot, up to the first slot without
/// a readable file.
pub(super) fn load_custom_presets(dir: &std::path::Path) -> Vec<crate::map::rmg::RmgOptions> {
    (0..crate::ui::skirmish_shell::CUSTOM_PRESET_SLOTS)
        .map_while(|slot| {
            let bytes = crate::util::file_system::read(custom_preset_path(dir, slot)).ok()?;
            let ini = crate::rules::ini_parser::IniFile::from_bytes(&bytes).ok()?;
            let mut options = crate::map::rmg::RmgOptions::default();
            options.apply_sed(&ini);
            options.normalize();
            Some(options)
        })
        .collect()
}

/// Serialize the completed dialog preview at the native common-teardown
/// boundary into `dir` (`App::random_map_dir`). Passing no directory models
/// startup without a configured retail install; teardown itself still proceeds.
fn write_random_map_preview_to_dir(
    dir: Option<&std::path::Path>,
    preview: &crate::map::rmg::preview::PreviewImage,
) {
    let Some(dir) = dir else {
        return;
    };
    let (Ok(width), Ok(height)) = (u16::try_from(preview.width), u16::try_from(preview.height))
    else {
        log::warn!(
            "random map: preview {}x{} does not fit a PCX header",
            preview.width,
            preview.height
        );
        return;
    };
    let rgb: Vec<u8> = preview
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
                .storage_roundtrip_rgb8([px[0], px[1], px[2]])
        })
        .collect();
    match crate::assets::pcx_file::encode_direct_rgb(width, height, &rgb) {
        Ok(encoded) => {
            let path = dir.join(RANDMAP_PREVIEW_FILE);
            if let Err(err) = crate::util::file_system::create_dir_all(dir)
                .and_then(|()| crate::util::file_system::write(&path, encoded))
            {
                log::warn!("random map: could not write {}: {err}", path.display());
            }
        }
        Err(err) => log::warn!("random map: could not encode the preview: {err}"),
    }
}

/// The setup runner's production-owned common teardown: publish the last
/// completed preview, destroy the modal, and release the cached MapClass
/// storage. Both Cancel and accepted OK use this exact owner.
///
/// gamemd-derived: `RandomMapSetupDialog__Run @ 0x00595BC0` writes
/// `RandMap.img` at 0x00595C17 before destroying `DAT_00ABE150` at
/// 0x00595CB2..0x00595CC2; only afterward does accepted caller 0x005E8590
/// write `RandMap.Sed` and install the chooser sentinel.
fn dismiss_random_map_setup_owners(
    modal: &mut Option<crate::ui::skirmish_shell::RandomMapSetupModalState>,
    retention: &mut RandomMapGenerationRetention,
    dir: Option<&std::path::Path>,
) {
    if let Some(preview) = modal
        .as_ref()
        .and_then(|modal| modal.generated_preview.as_ref())
    {
        write_random_map_preview_to_dir(dir, preview);
    }
    *modal = None;
    retention.destroy_map_storage();
}

pub(super) fn cancel_random_map_setup_owners(
    runtime: &mut crate::app::frontend::skirmish_session::MultiplayerShellRuntime,
    modal: &mut Option<crate::ui::skirmish_shell::RandomMapSetupModalState>,
    retention: &mut RandomMapGenerationRetention,
    dir: Option<&std::path::Path>,
) -> Option<crate::ui::skirmish_shell::ChooseMapSelection> {
    let modal_ref = modal.as_ref()?;
    runtime.remember_random_map_options(&modal_ref.options);
    let previous_selection = modal_ref.cancel();
    dismiss_random_map_setup_owners(modal, retention, dir);
    retention.cancel_setup();
    previous_selection
}

/// Select the candidate, run common teardown, and return the normalized seed
/// options that the accepted caller writes only afterward.
pub(super) fn accept_random_map_setup_owners(
    runtime: &mut crate::app::frontend::skirmish_session::MultiplayerShellRuntime,
    modal: &mut Option<crate::ui::skirmish_shell::RandomMapSetupModalState>,
    retention: &mut RandomMapGenerationRetention,
    dir: Option<&std::path::Path>,
) -> Option<crate::map::rmg::RmgOptions> {
    let crate::ui::skirmish_shell::AcceptOutcome::Commit(options) = modal.as_ref()?.accept() else {
        return None;
    };
    runtime.remember_random_map_options(&options);
    // 0x0058B820 writes the 'W' staging at each Generate. VERA stages it
    // from the accepted candidate, the host's own load authority, so the 'W'
    // and the host's start cells always agree.
    // RESIDUAL: natively a later Generate that is cancelled still overwrites
    // Scenario +0x11C0, which both the host's load and the 'W' then read.
    if retention.candidate.is_some() {
        runtime.stage_generated_starts(retention.generated_header());
    }
    retention.accept_setup(RANDMAP_SED_FILE);
    dismiss_random_map_setup_owners(modal, retention, dir);
    Some(*options)
}

/// Whether the original redraws its preview at this generation boundary.
///
/// It draws eight times while generating, reporting 55, 60, 70, 80, 85, 90 and
/// 95 percent on the seven after the first. Two of those pairs have no
/// generation between them at all — only a progress-report helper runs — so the
/// 60 and 85 redraws reproduce the image already on screen and are dropped here:
/// eight calls, six distinct pictures.
///
/// The percentages are the anchor the boundaries below were chosen from; they
/// have no home in the port yet, because the dialog's progress bar is still
/// drawn empty.
fn draws_preview(point: crate::map::rmg::build::GenerationPoint) -> bool {
    use crate::map::rmg::Stage;
    use crate::map::rmg::build::GenerationPoint;
    matches!(
        point,
        // Clears the box before any terrain exists.
        GenerationPoint::Initial
            // 55 (and again at 60): the water is in.
            | GenerationPoint::After(Stage::WaterFinalize)
            // 70: regions, island passes and the green spread.
            | GenerationPoint::After(Stage::RecalcAfterTerrain)
            // 80 (and again at 85): starts, tech buildings and tiberium.
            | GenerationPoint::After(Stage::RecalcAfterTiberium)
            // 90: the hills.
            | GenerationPoint::After(Stage::Hills)
            // 95: LAT patches, trees and rocks.
            | GenerationPoint::After(Stage::Rocks)
    )
}

impl App {
    /// VERA extension: keep the dialog's generated map as `rmap<seed>.yrm` in
    /// VERA's Maps folder through [`crate::app::map_library::keep_map`],
    /// never the game folder, so Choose Map lists it at once; a name another
    /// map holds takes the map's hash suffix. False when there is no map or
    /// the write fails.
    fn export_generated_map(state: &mut AppState) -> bool {
        let Some(generated) = state.frontend.random_map_retention.candidate() else {
            return false;
        };
        let modal = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_ref();
        let seed = modal.map_or(0, |modal| modal.options.seed_u16());
        // The dialog's own preview becomes the file's Choose Map thumbnail.
        let preview = modal
            .and_then(|modal| modal.generated_preview.as_ref())
            .map(|image| crate::map::map_export::ExportPreview {
                width: image.width,
                height: image.height,
                rgba: &image.rgba,
            });
        let players = generated.start_waypoints.len().max(2) as u32;
        let text = crate::map::map_export::multiplayer_map_text(
            &generated.map_file,
            &format!("Random Map {seed}"),
            2,
            players,
            preview,
            state
                .frontend
                .random_map_retention
                .generated_header()
                .preview_source_bounds()
                .as_ref(),
        );
        let file = format!("rmap{seed}.yrm");
        match crate::app::map_library::keep_map(state, &file, text.as_bytes()) {
            Ok(path) => {
                log::info!("random map: exported {}", path.display());
                true
            }
            Err(err) => {
                log::warn!("random map: could not export {file}: {err:#}");
                false
            }
        }
    }

    /// The folder of `RandMap.Sed`, `RandMap.img` and the saved seeds
    /// (`map::source::random_map_dir`), never the game folder.
    pub(super) fn random_map_dir(state: &AppState) -> Option<std::path::PathBuf> {
        state
            .platform
            .game_config
            .as_ref()
            .map(|config| crate::map::source::random_map_dir(&config.paths.ra2_dir).to_path_buf())
    }

    /// Kick off generation on a worker and return immediately.
    ///
    /// Everything that needs the asset manager is done here, up front; only
    /// plain data crosses to the worker.
    pub(super) fn start_random_map_generation(
        state: &mut AppState,
        options: &crate::map::rmg::RmgOptions,
        accept_on_finish: bool,
    ) -> bool {
        // The ordinary Generate path also stamps the localized working label
        // immediately before generator construction (0x00595BC0).
        let mut generation_options = options.clone();
        generation_options.description = Self::csf_label(
            state,
            RANDOM_MAP_DESCRIPTION_KEY,
            RANDOM_MAP_DESCRIPTION_FALLBACK,
        )
        .into();
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        {
            modal.options.description = generation_options.description.clone();
        }
        let options = &generation_options;
        // A second Generate makes the previous dialog result stale immediately,
        // even when setup cannot progress far enough to spawn the worker.
        begin_random_map_generation_owners(
            &mut state.frontend.multiplayer_shell_runtime,
            &mut state.frontend.random_map_retention,
            options,
        );
        // Reset668B8E reads the committed Session mode. ChooseMap only
        // commits its draft after UseMap5E7367; RMG runs before that call.
        let mode_file = if state.frontend.shell_route.network_setup() {
            state
                .match_state
                .session
                .network_setup()
                .map(|launch| launch.mode.override_file.clone())
        } else {
            crate::skirmish_modes::mode_by_id(
                &state.frontend.skirmish_modes,
                state.frontend.skirmish_shell_state.selected_mode_id,
            )
            .map(|mode| mode.override_file.clone())
        };
        let map_storage_decision = state
            .frontend
            .random_map_retention
            .begin_map_storage_decision(options);
        let (manager, mut native_rules, _) =
            state.process_assets.manager_and_rules_with_tile_cache();
        let Some(asset_manager) = manager else {
            return false;
        };
        let mode_rules = match crate::rules::retail_sources::select_mode_ini(
            asset_manager,
            mode_file.as_deref(),
        ) {
            Ok(mode) => mode,
            Err(error) => {
                log::warn!("Random-map preview mode unavailable: {error}");
                return false;
            }
        };
        if map_storage_decision.reinitialize
            && let Some(owner) = native_rules.as_deref_mut()
            && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle)
        {
            // The outgoing Simulation can remain resident for shell drawing.
            // Drain its live Rules writes before59A0F0 destroys those Types;
            // a later Full_Init must not replay them onto the preview registry.
            runtime.simulation.retire_scenario_started();
            if let Err(error) = crate::app::process_assets::transfer_outgoing_simulation_rules(
                owner,
                &mut runtime.simulation,
            ) {
                log::warn!("Random-map outgoing Rules transfer failed: {error}");
                return false;
            }
        }
        let Some(PreparedRandomMapGeneration {
            theater,
            terrain_rules,
            preview_rules,
            settings,
            resolved_inputs,
            blocks,
            tech_types,
            #[cfg(feature = "ares")]
            urban_catalog,
        }) = prepare_random_map_generation(
            asset_manager,
            options,
            RandomMapRulesPreparation::Generation {
                owner: native_rules,
                reinitialize: map_storage_decision.reinitialize,
            },
            mode_rules.as_ref(),
        )
        else {
            return false;
        };
        let options = options.clone();
        let shared_cell_dummy = state.process_assets.shared_cell_dummy.clone();
        #[cfg(not(feature = "ares"))]
        let objects = RandomMapObjects::tech(tech_types);
        // The dialog's Generate draws Ares' foot picks from process Main.
        #[cfg(feature = "ares")]
        let objects = RandomMapObjects {
            tech_types,
            urban: options.urban_areas.then(|| {
                (
                    urban_catalog,
                    crate::app::state::process_main_draws(&state.frontend.frontend_main_rng),
                )
            }),
        };
        // Generation stays single-threaded and seed-driven; the thread changes
        // only where it runs, never the order it consumes its RNG in.
        match spawn_random_map_generation_worker(
            options,
            settings,
            resolved_inputs,
            blocks,
            objects,
            shared_cell_dummy,
            map_storage_decision,
        ) {
            Ok(receiver) => {
                state
                    .frontend
                    .random_map_retention
                    .commit_map_storage_decision(map_storage_decision);
                state.frontend.random_map_generation = Some(RandomMapGenerationJob {
                    receiver,
                    theater: Box::new(theater),
                    terrain_rules: Box::new(terrain_rules),
                    preview_rules,
                    accept_on_finish,
                });
                true
            }
            Err(err) => {
                log::warn!("random map: could not spawn the generator thread: {err}");
                false
            }
        }
    }

    /// Collect whatever the generator has produced since the last frame.
    ///
    /// Called every frame while a job is in flight. Returns true when the dialog
    /// changed, so the caller knows to redraw.
    ///
    /// Only the newest of several progress snapshots is rasterised. Colouring is
    /// the expensive half, and an image the worker overtook before a frame was
    /// drawn was never on screen to be seen.
    pub(crate) fn poll_random_map_generation(state: &mut AppState) -> bool {
        if state.frontend.random_map_generation.is_some()
            && state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .is_none()
        {
            // The dialog went away without the job going with it. Drop it here
            // rather than trusting every close path to remember: a job with no
            // dialog has nowhere to deliver, and letting it finish would write
            // a preview file for a map nobody asked for.
            state.frontend.random_map_generation = None;
            return false;
        }
        let Some(job) = state.frontend.random_map_generation.as_ref() else {
            return false;
        };
        let mut latest_progress = None;
        let mut finished = None;
        let mut died = false;
        loop {
            match job.receiver.try_recv() {
                Ok(RandomMapUpdate::Started) => {}
                Ok(RandomMapUpdate::Progress(snapshot)) => latest_progress = Some(snapshot),
                Ok(RandomMapUpdate::Finished(generated)) => {
                    finished = Some(generated);
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    died = true;
                    break;
                }
            }
        }

        if let Some(generated) = finished {
            let job = state
                .frontend
                .random_map_generation
                .take()
                .expect("checked present above");
            let preview = Self::rasterise_generated_map(state, &job, &generated);
            let generated = *generated;
            let accept = {
                let resident_simulation = state
                    .match_state
                    .sim_runtime
                    .write_sim(SimWrite::Lifecycle)
                    .map(|runtime| &mut runtime.simulation);
                let frontend = &mut state.frontend;
                match frontend
                    .skirmish_shell_state
                    .random_map_setup_modal
                    .as_mut()
                {
                    Some(modal) => finish_random_map_generation_owners(
                        resident_simulation,
                        &mut frontend.multiplayer_shell_runtime,
                        &mut frontend.random_map_retention,
                        modal,
                        generated,
                        preview,
                        job.accept_on_finish,
                    ),
                    None => false,
                }
            };
            if accept {
                // The generate block ran inside Use Map; the dialog then
                // ends through its teardown slide.
                Self::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::RandomMapUse,
                );
            }
            return true;
        }

        if died {
            // The worker ended without a result. Clear the job so the dialog
            // does not sit disabled forever waiting on it.
            log::warn!("random map: the generator thread ended without a result");
            state.frontend.random_map_generation = None;
            if let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_mut()
            {
                modal.fail_generate();
            }
            return true;
        }

        let Some(snapshot) = latest_progress else {
            return false;
        };
        // Lifted out and put straight back: rasterising reads the job and the
        // rest of the app state at once, and the job lives inside that state.
        let job = state
            .frontend
            .random_map_generation
            .take()
            .expect("checked present above");
        let preview =
            Self::rasterise_map(state, &job, &snapshot.map_file, &snapshot.start_waypoints);
        state.frontend.random_map_generation = Some(job);
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
            && let Some(preview) = preview
        {
            modal.show_progress_preview(preview);
        }
        true
    }

    /// Cancel the setup dialog, abandoning any generation and every retained
    /// result associated with this random-map selection.
    ///
    /// Dropping the job drops the receiver, so a worker still going finds a
    /// closed channel on its next send and its remaining output goes nowhere.
    /// That matters beyond tidiness: a late finish would otherwise overwrite
    /// `RandMap.img`, changing the chooser's thumbnail to a map the player
    /// walked away from.
    pub(super) fn cancel_random_map_setup(state: &mut AppState) {
        let dir = Self::random_map_dir(state);
        state.frontend.random_map_generation = None;
        let frontend = &mut state.frontend;
        let _ = cancel_random_map_setup_owners(
            &mut frontend.multiplayer_shell_runtime,
            &mut frontend.skirmish_shell_state.random_map_setup_modal,
            &mut frontend.random_map_retention,
            dir.as_deref(),
        );
        Self::show_choose_map_after_random_map(state);
    }

    /// The random-map run has returned to the hidden chooser: unless its
    /// Use Map closed it or asks to eject AI players, it shows again and its
    /// first paint slides it in (`0x005E6B47`, `0x005E6B51`).
    fn show_choose_map_after_random_map(state: &mut AppState) {
        let shell = &mut state.frontend.skirmish_shell_state;
        if shell.random_map_setup_modal.is_some() {
            return;
        }
        if let Some(chooser) = shell.choose_map_modal.as_mut()
            && chooser.message_box.is_none()
        {
            chooser.show();
        }
    }

    /// Commit the dialog's options and close it. Shared by the immediate accept
    /// and the one deferred behind a generation.
    pub(super) fn accept_random_map_setup(state: &mut AppState) {
        let dir = Self::random_map_dir(state);
        let options = {
            let frontend = &mut state.frontend;
            accept_random_map_setup_owners(
                &mut frontend.multiplayer_shell_runtime,
                &mut frontend.skirmish_shell_state.random_map_setup_modal,
                &mut frontend.random_map_retention,
                dir.as_deref(),
            )
        };
        let Some(options) = options else {
            return;
        };
        state.frontend.random_map_generation = None;
        match Self::commit_random_map_setup(state, &options) {
            Ok(()) => {
                // Teardown retained both accepted start staging and its
                // presentation preview; map construction remains owned by the
                // `.SED` reader.
            }
            Err(err) => {
                // The native dialog has already torn down at this point. Do
                // not leave an accepted launch attached when the seed/options
                // commit failed and no random-map selection was installed.
                state.frontend.random_map_retention.cancel_setup();
                log::error!("random map: could not write {RANDMAP_SED_FILE}: {err}");
                if let Some(chooser) = state
                    .frontend
                    .skirmish_shell_state
                    .choose_map_modal
                    .as_mut()
                {
                    chooser.show_random_map_failure();
                }
            }
        }
        Self::show_choose_map_after_random_map(state);
    }

    /// Rasterise the finished map into the dialog-owned preview surface.
    /// Persistence belongs to common teardown, not generation completion.
    fn rasterise_generated_map(
        state: &mut AppState,
        job: &RandomMapGenerationJob,
        generated: &crate::map::rmg::GeneratedMap,
    ) -> Option<crate::map::rmg::preview::PreviewImage> {
        Self::rasterise_map(state, job, &generated.map_file, &generated.start_waypoints)
    }

    /// Colour and rasterise a map. Main thread only: the resolver reads theater
    /// data and the Overlay type/header colors share the retained asset inputs.
    ///
    /// Mid-generation snapshots go through here too, so an in-progress preview
    /// is coloured by exactly the path that colours the finished one.
    fn rasterise_map(
        state: &mut AppState,
        job: &RandomMapGenerationJob,
        map_file: &crate::map::map_file::MapFile,
        start_waypoints: &[(u8, u16, u16)],
    ) -> Option<crate::map::rmg::preview::PreviewImage> {
        // LAT defaults off for runtime maps, so resolve the same way the load
        // path will; a different setting here would colour cells the player
        // never sees.
        let resolved_terrain = {
            let mut main = crate::app::state::process_main_draws(&state.frontend.frontend_main_rng);
            let (manager, _, selector_cache) =
                state.process_assets.manager_and_rules_with_tile_cache();
            let asset_manager = manager.map(|m| &*m);
            build_random_map_preview_grid(
                map_file,
                Some(&job.theater),
                asset_manager,
                Some(&job.terrain_rules),
                &mut main,
                selector_cache,
            )
        };
        let cells = random_map_preview_cells(
            map_file,
            &resolved_terrain,
            job.preview_rules.as_ref(),
            state.process_assets.manager(),
            job.theater.extension,
        );
        let waypoints = crate::map::rmg::preview::marker_waypoints(start_waypoints);
        crate::map::rmg::preview::render_preview(&cells, &waypoints)
    }

    /// Persist accepted random-map setup, refresh the sentinel record, and
    /// select it so launch generates from it.
    ///
    /// A failed write is fatal to the commit: `map_load` treats a missing seed
    /// file as "use defaults", so committing anyway would silently start a
    /// different map than the one the player configured.
    fn commit_random_map_setup(
        state: &mut AppState,
        options: &crate::map::rmg::RmgOptions,
    ) -> anyhow::Result<()> {
        let dir = Self::random_map_dir(state)
            .ok_or_else(|| anyhow::anyhow!("no game config; cannot locate VERA's Maps folder"))?;
        crate::util::file_system::create_dir_all(&dir)?;
        crate::util::file_system::write(dir.join(RANDMAP_SED_FILE), options.to_sed_bytes())?;

        let description_text = options.description.display_text();
        let display = if options.description.is_empty() {
            RANDOM_MAP_DESCRIPTION_FALLBACK
        } else {
            &description_text
        };
        // Reuse the modal helper: it upserts the single sentinel, honours the
        // mode's random-map admission, and refreshes the filtered record list.
        let chooser_layout = crate::app::frontend::shell_layout::choose_map_layout(state);
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
        else {
            return Ok(());
        };
        // F11: the catalog's mutation guard re-projects the shell map entries
        // on drop, so the loadable-map projection can never drift from the
        // records — the old hand-patch by name-position is gone.
        let index = {
            let mut records = state.frontend.scenario_catalog.records_mut();
            modal.create_random_map(
                &mut records,
                &state.frontend.skirmish_modes,
                display,
                options.num_players,
                &chooser_layout,
            )
        };
        let mode_id = modal.selected_mode_id;
        let _ = modal;
        if let Some(index) = index {
            let selection = crate::ui::skirmish_shell::ChooseMapSelection {
                mode_id,
                record_index: Some(index),
            };
            // Success runs the hidden chooser's Use Map (`0x005E6B2F`): its
            // eject box can keep the chooser (`0x005E6B47`).
            if Self::prompt_choose_map_eject(state, selection) {
                return Ok(());
            }
            Self::commit_choose_map_use(state, selection);
        }
        Ok(())
    }

    fn skirmish_random_map_setup_layout(
        state: &AppState,
    ) -> crate::ui::skirmish_shell::RandomMapSetupLayout {
        crate::app::frontend::shell_layout::random_map_setup_layout(state)
    }

    pub(super) fn handle_random_map_setup_mouse_down(state: &mut AppState) -> bool {
        let layout = Self::skirmish_random_map_setup_layout(state);
        // VERA: a press on the preview enlarges it, as on the other setup
        // screens (`preview_zoom`), while the dialog rests: no list open and
        // no generation running.
        if state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_ref()
            .is_some_and(|modal| !modal.generating && modal.open_combo.is_none())
            && crate::app::frontend::preview_zoom::press_on_box(state, layout.preview)
        {
            return true;
        }
        let (x, y) = state.match_state.input.cursor_px();
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return false;
        };
        // The combos act on press, ahead of the buttons: an open list takes
        // every press, and an enabled face never arms a button press.
        if let Some(cue) = modal.press_combos(&layout, x, y) {
            Self::play_skirmish_shell_ui_sound(state, cue);
            return true;
        }
        if let Some(control) = crate::ui::skirmish_shell::random_map_setup_control_at_view(
            &layout,
            x,
            y,
            modal.extended,
            modal.advanced,
            modal.extras_page.is_some(),
        ) {
            // VERA: an Advanced slider acts on press, like the Players one.
            if let crate::ui::skirmish_shell::RandomMapSetupControl::Advanced(field) = control {
                let rect = layout.advanced_control_rects[field.row()];
                if modal.is_enabled(control)
                    && modal.press_advanced(field, x, y, rect, layout.scale)
                {
                    Self::play_skirmish_shell_generic_click_sound(state);
                }
                return true;
            }
            // The players slider is not a button: it acts on press, not on
            // release, so it never arms a pressed control.
            if control == crate::ui::skirmish_shell::RandomMapSetupControl::Players0x3eb {
                let rect = layout.control_rects[SETUP_PLAYERS_ROW];
                if modal.is_enabled(control) && modal.press_players(x, y, rect, layout.scale) {
                    Self::play_skirmish_shell_generic_click_sound(state);
                }
                return true;
            }
            // A disabled control swallows the click without arming a press, so
            // releasing over it cannot fire.
            if modal.is_enabled(control) {
                modal.pressed_control = Some(control);
                Self::play_main_menu_button_sound(state);
            }
            return true;
        }
        layout.dialog.contains(x, y)
    }

    pub(super) fn handle_random_map_setup_mouse_move(state: &mut AppState) {
        let layout = Self::skirmish_random_map_setup_layout(state);
        let (x, y) = state.match_state.input.cursor_px();
        // `0x105`'s proc runs the common handler first (`0x0059631A`): every
        // move writes the help of the control under the pointer, or an empty
        // one, and repaints the status line. An open list holds the mouse
        // capture instead (`ComboDropWin` `0x0060E0F1`); its own move handler
        // (`0x0060E262`) asks the dialog for the row's help (`0x4E9`), which
        // `0x105` never answers, so the status line keeps its text. So does a
        // Players slider holding the mouse since a press on it.
        let mouse_held = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_ref()
            .is_some_and(|modal| {
                modal.open_combo.is_some()
                    || modal.players_hold.is_some()
                    || modal.advanced_hold.is_some()
            });
        let (extended, advanced, extras) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_ref()
            .map_or((false, false, false), |modal| {
                (modal.extended, modal.advanced, modal.extras_page.is_some())
            });
        if !mouse_held {
            let help = crate::ui::skirmish_shell::random_map_setup_control_at_view(
                &layout, x, y, extended, advanced, extras,
            )
            .map(|control| {
                // VERA: an Extras row's help is its entry's, not the row's.
                let key = match control {
                    crate::ui::skirmish_shell::RandomMapSetupControl::ExtrasRow(row) => state
                        .frontend
                        .skirmish_shell_state
                        .random_map_setup_modal
                        .as_ref()
                        .and_then(|modal| modal.extras_row(row))
                        .map_or("", crate::ui::skirmish_shell::ExtrasRow::help),
                    control => {
                        crate::ui::skirmish_shell::status_help_key_for_random_map_setup(control)
                    }
                };
                Self::localized_status_help_text(state, key)
            })
            .unwrap_or_default();
            if let Some(setup) = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_mut()
                && setup
                    .statics
                    .hover(&help, crate::util::time::Instant::now())
            {
                state.platform.window.request_redraw();
            }
        }
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return;
        };
        if let Some(hold) = modal.advanced_hold {
            let rect = layout.advanced_control_rects[hold.id.row()];
            if modal.drag_advanced(x, rect, layout.scale) {
                Self::play_skirmish_shell_generic_click_sound(state);
            }
            state.platform.window.request_redraw();
            return;
        }
        if modal.players_hold.is_none() {
            return;
        }
        let rect = layout.control_rects[SETUP_PLAYERS_ROW];
        if modal.drag_players(x, rect, layout.scale) {
            Self::play_skirmish_shell_generic_click_sound(state);
        }
        state.platform.window.request_redraw();
    }

    pub(super) fn handle_random_map_setup_mouse_up(state: &mut AppState) -> bool {
        use crate::ui::skirmish_shell::RandomMapSetupControl as Control;

        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return false;
        };
        // The release goes to a slider holding the mouse; freeing it lets the
        // dialog's hit test run again.
        if modal.players_hold.take().is_some() | modal.advanced_hold.take().is_some() {
            Self::handle_random_map_setup_mouse_move(state);
            return true;
        }
        let layout = Self::skirmish_random_map_setup_layout(state);
        let (x, y) = state.match_state.input.cursor_px();
        // RMGMD.INI drives the randomizer's vegetation bounds; without it the
        // derived vegetation collapses to zero and randomized maps lose trees.
        let settings = state
            .process_assets
            .manager()
            .map(crate::map::rmg::RmgSettings::load)
            .unwrap_or_default();
        let description = state
            .process_assets
            .csf
            .as_ref()
            .map(|csf| csf.text(RANDOM_MAP_DESCRIPTION_KEY).into_owned())
            .unwrap_or_else(|| RANDOM_MAP_DESCRIPTION_FALLBACK.to_string());
        // VERA: where the Extras view keeps the player's own presets.
        let preset_dir = Self::random_map_dir(state);
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return false;
        };
        let pressed = modal.pressed_control.take();
        let released = crate::ui::skirmish_shell::random_map_setup_control_at_view(
            &layout,
            x,
            y,
            modal.extended,
            modal.advanced,
            modal.extras_page.is_some(),
        );
        if pressed.is_none() || pressed != released {
            return layout.dialog.contains(x, y) || pressed.is_some();
        }

        let mut close_setup = false;
        // Generating needs the whole app state, so it cannot run while the modal
        // is mutably borrowed; the actions below only record what to do.
        let mut generate_requested = false;
        let mut accept_requested = false;
        let mut open_browser: Option<SavedSeedMode> = None;
        let mut export_requested = false;
        let mut main = crate::ui::skirmish_shell::MainRngRanged(
            crate::app::state::process_main_draws(&state.frontend.frontend_main_rng),
        );
        match released.expect("checked equal to pressed control") {
            Control::Randomize0x621 => {
                modal.randomize_options(&settings, &mut main, &description);
            }
            Control::AdvancedToggle => {
                modal.toggle_advanced();
            }
            // The sliders act on press and never arm a release.
            Control::Advanced(_) => {}
            Control::ExportMap => export_requested = modal.is_enabled(Control::ExportMap),
            Control::FairStartsToggle => modal.toggle_fair_starts(),
            Control::PresetCycle => modal.cycle_preset(),
            // VERA: an Extras row steps its choice or turns the page in the
            // dialog's own state; the share-code tools reach the clipboard.
            Control::ExtrasRow(row) => {
                use crate::ui::skirmish_shell::{ExtrasAction, ExtrasOutcome};
                const NO_CLIPBOARD: (&str, &str) = (
                    "VERA:RandomMapExtraClipboardFailed",
                    "The clipboard could not be reached",
                );
                // A code read, like a variant choice, generates at once and
                // without Generate's rolls, as Load does (`0x00596963`).
                let mut regenerate = false;
                modal.extras_note = match modal.release_extras_row(row) {
                    Some(ExtrasOutcome::Tool(ExtrasAction::CopyCode)) => Some(
                        if crate::app::os_clipboard::set_text(&modal.options.share_code()) {
                            ("VERA:RandomMapExtraCodeCopied", "Code copied")
                        } else {
                            NO_CLIPBOARD
                        },
                    ),
                    Some(ExtrasOutcome::Tool(ExtrasAction::PasteCode)) => {
                        use crate::map::rmg::options::ShareVersion;
                        Some(match crate::app::os_clipboard::text() {
                            Some(text) => match modal.adopt_share_code(&text) {
                                Some(made_by) => {
                                    regenerate = true;
                                    let this = ShareVersion::this_build();
                                    if made_by == this {
                                        ("VERA:RandomMapExtraCodeTaken", "Code read")
                                    } else {
                                        // Another generator may build another
                                        // map from the same options; say so.
                                        log::info!(
                                            "Map code from VERA20k {made_by}; this is {this}"
                                        );
                                        (
                                            "VERA:RandomMapExtraCodeOtherVersion",
                                            "Code read. It is from another VERA20k version, so the map may differ",
                                        )
                                    }
                                }
                                None => (
                                    "VERA:RandomMapExtraCodeBad",
                                    "The clipboard holds no map code",
                                ),
                            },
                            None => NO_CLIPBOARD,
                        })
                    }
                    Some(ExtrasOutcome::Tool(ExtrasAction::SavePreset)) => {
                        let slot = modal.save_custom_preset();
                        let saved = preset_dir
                            .as_deref()
                            .is_some_and(|dir| save_custom_preset(dir, slot, &modal.options));
                        Some(if saved {
                            (
                                "VERA:RandomMapExtraPresetSaved",
                                "Saved: the Presets button now offers it",
                            )
                        } else {
                            (
                                "VERA:RandomMapExtraPresetFailed",
                                "The preset is kept until you leave, but its file could not be written",
                            )
                        })
                    }
                    // The dialog goes back by itself and asks only for the map.
                    Some(ExtrasOutcome::Tool(ExtrasAction::PreviousMap)) => None,
                    Some(ExtrasOutcome::Regenerate) => {
                        regenerate = true;
                        None
                    }
                    None => None,
                };
                if regenerate {
                    modal.begin_generate();
                    generate_requested = true;
                }
            }
            Control::Generate0x620 => {
                modal.reroll_derived_for_generate(&settings, &mut main);
                modal.begin_generate();
                generate_requested = true;
            }
            Control::Ok0x6c5 => {
                // Accept generates first when nothing has been generated yet,
                // so the committed options always describe a map that exists.
                if matches!(
                    modal.accept(),
                    crate::ui::skirmish_shell::AcceptOutcome::NeedsGenerate
                ) {
                    modal.reroll_derived_for_generate(&settings, &mut main);
                    modal.begin_generate();
                    generate_requested = true;
                }
                accept_requested = true;
            }
            Control::Cancel0x5c0 => {
                // Result 2 in the original: no seed file, no sentinel, no
                // selection change. The chooser underneath is left untouched.
                close_setup = true;
            }
            Control::Load0x6c2 => open_browser = Some(SavedSeedMode::Load),
            Control::Save0x6c3 => open_browser = Some(SavedSeedMode::Save),
            Control::Delete0x6c4 => open_browser = Some(SavedSeedMode::Delete),
            #[cfg(feature = "ares")]
            Control::UrbanAreas5000 => modal.toggle_urban_areas(),
            // The combos act on press (`press_combos`) and never arm a press;
            // dragging the players slider is a separate input mode.
            Control::MapType0x405
            | Control::Time0x3ea
            | Control::Theater0x407
            | Control::Size0x406
            | Control::Resources0x408
            | Control::Players0x3eb => {}
        }

        if let Some(mode) = open_browser {
            Self::open_saved_seed_browser(state, mode);
            return true;
        }
        if export_requested {
            let exported = Self::export_generated_map(state);
            if let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_mut()
            {
                modal.exported = exported;
            }
            return true;
        }
        if generate_requested {
            let options = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_ref()
                .map(|modal| modal.options.clone());
            let started = options.is_some_and(|options| {
                Self::start_random_map_generation(state, &options, accept_requested)
            });
            if !started {
                // Nothing will arrive, so the dialog must not be left sitting
                // in its generating state with every control disabled.
                log::warn!("random map: could not start generation for the configured options");
                if let Some(modal) = state
                    .frontend
                    .skirmish_shell_state
                    .random_map_setup_modal
                    .as_mut()
                {
                    modal.fail_generate();
                }
            }
            // Accept, if it was asked for, is now the job's responsibility.
            return true;
        }
        // Both results end the dialog through its teardown slide.
        if accept_requested {
            Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::RandomMapUse,
            );
        }
        if close_setup {
            Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::RandomMapCancel,
            );
        }
        true
    }
}

#[cfg(test)]
#[path = "shell_random_map_terrain_preview_tests.rs"]
mod terrain_preview_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Restarting the same random map after a Skirmish match: the shell's
    /// retained staging supplies every start.
    #[test]
    fn a_skirmish_rematch_reuses_the_retained_random_map_starts() {
        let mut staging = crate::map::preview::ScenarioStartStaging {
            width: 400,
            height: 200,
            count: 4,
            ..Default::default()
        };
        for slot in 0..4 {
            staging.cells[slot] = (slot as i16 + 10, 20);
        }
        let rematch = AcceptedRmgStartStaging::from_retained_staging(&staging, Some("randmap.sed"))
            .expect("random map rematch staging");
        let table = rematch.waypoint_table_for_admission();
        assert_eq!(table.len(), 4);
        assert_eq!((table[&3].rx, table[&3].ry), (13, 20));

        assert!(
            AcceptedRmgStartStaging::from_retained_staging(&staging, Some("large500.yrm"))
                .is_none()
        );
        let empty = crate::map::preview::ScenarioStartStaging::default();
        assert!(
            AcceptedRmgStartStaging::from_retained_staging(&empty, Some(RANDMAP_SED_FILE))
                .is_none()
        );
    }

    #[test]
    fn generated_digest_covers_retained_powerups_without_mutating_rules() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut owner =
            crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                crate::rules::ini_parser::IniFile::from_str("[Animations]\n0=BASE\n"),
                None,
                crate::rules::ini_parser::IniFile::empty(),
                std::sync::Arc::default(),
                std::sync::Arc::default(),
            )
            .unwrap();
        let options = crate::map::rmg::RmgOptions::default();
        let original = prepare_random_map_generation(
            &mut assets,
            &options,
            RandomMapRulesPreparation::ReadOnly(Some(&owner)),
            None,
        )
        .unwrap()
        .multiplayer_digest();
        owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &crate::rules::ini_parser::IniFile::from_str(
                    "[Animations]\n0=HISTORY\n[Powerups]\nMoney=7,HISTORY,yes,125\n",
                ),
            )
            .unwrap();
        let live = owner.saved_scenario_rules().unwrap().powerups().clone();
        let after_history = prepare_random_map_generation(
            &mut assets,
            &options,
            RandomMapRulesPreparation::ReadOnly(Some(&owner)),
            None,
        )
        .unwrap()
        .multiplayer_digest();
        assert_ne!(
            original, after_history,
            "same sources do not erase Powerups history"
        );
        assert_eq!(owner.saved_scenario_rules().unwrap().powerups(), &live);
        assert!(owner.prepared_random_map_rules().is_none());
    }

    #[test]
    fn network_generated_seed_keeps_a_complete_admissible_start_table() {
        let Some((retail_dir, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut assets = crate::assets::asset_manager::AssetManager::new(
            &retail_dir,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
        let (_, _, rules) = crate::rules::retail_sources::load_startup_rules(
            &assets,
            std::sync::Arc::clone(audio.sounds()),
            std::sync::Arc::clone(audio.eva()),
        )
        .unwrap()
        .into_parts();
        let options = crate::map::rmg::RmgOptions {
            seed: 26702,
            num_players: 2,
            region_size: 46,
            ruggedness: 76,
            accessibility: 84,
            water_amount: 21,
            tiberium: 20,
            tiberium_layout: 90,
            vegetation: 62,
            urban_presence: 2,
            urban_enabled: false,
            fair_starts: false,
            ..Default::default()
        };
        let prepared = prepare_random_map_generation(
            &mut assets,
            &options,
            RandomMapRulesPreparation::ReadOnly(Some(&rules)),
            None,
        )
        .unwrap();
        let generated = crate::map::rmg::build::generate_map(
            &options,
            &prepared.settings,
            &prepared.resolved_inputs,
            &prepared.blocks,
            &crate::map::rmg::build::GenerationObjects::tech(&prepared.tech_types),
        );
        let staging = crate::app::loading::composition::scenario_start_staging(
            &generated.map_file,
            &generated.start_waypoints,
        );
        assert!(
            crate::app::shell_network_setup::maps::checked_staging(
                &staging.to_w_text(),
                options.num_players,
            )
            .is_ok(),
            "{staging:?}; starts={:?}; unfilled={}",
            generated.start_waypoints,
            generated.unfilled_start_slots,
        );
    }

    #[test]
    fn generated_raised_clear_preview_matches_original_tmp_level_colors() {
        let Some((retail_dir, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let native = crate::test_fixture::json("tools/spatial_oracle/tmp_preview.json");
        let selected = &native["inputs"]["selected_tmp"];
        let mut assets = crate::assets::asset_manager::AssetManager::new(
            &retail_dir,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .expect("retail preview assets");
        let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
        let (_, _, mut rules) = crate::rules::retail_sources::load_startup_rules(
            &assets,
            std::sync::Arc::clone(audio.sounds()),
            std::sync::Arc::clone(audio.eva()),
        )
        .expect("retail preview Rules")
        .into_parts();
        // The existing .SED lifecycle recipe, not a claim about Generate's
        // preceding parameter-reroll RNG. No cell height or color is supplied.
        let options = crate::map::rmg::RmgOptions {
            theater: 0,
            map_type: 3,
            resources: 2,
            ruggedness: 35,
            water_amount: 50,
            num_players: 2,
            tiberium: 35,
            vegetation: 30,
            urban_presence: 40,
            urban_enabled: false,
            fair_starts: false,
            seed: 4242,
            ..Default::default()
        };
        let mode_rules =
            crate::rules::retail_sources::select_mode_ini(&assets, Some("MPBattleMD.ini")).unwrap();
        let prepared = prepare_random_map_generation(
            &mut assets,
            &options,
            RandomMapRulesPreparation::ReadOnly(Some(&rules)),
            mode_rules.as_ref(),
        )
        .expect("registered production theater");
        let tech_footprints: std::collections::HashMap<_, _> = prepared
            .tech_types
            .iter()
            .map(|kind| (kind.name.clone(), kind.footprint.clone()))
            .collect();
        let retention = RandomMapGenerationRetention::default();
        let decision = retention.map_storage_decision(&options);
        let receiver = spawn_random_map_generation_worker(
            options,
            prepared.settings,
            prepared.resolved_inputs,
            prepared.blocks,
            RandomMapObjects::tech(prepared.tech_types),
            crate::map::resolved_terrain::SharedCellDummy::fresh(),
            decision,
        )
        .expect("production generation worker");
        let mut entries = 0;
        let generated = loop {
            match receiver
                .recv_timeout(std::time::Duration::from_secs(60))
                .expect("production generation receipt")
            {
                RandomMapUpdate::Started => entries += 1,
                RandomMapUpdate::Progress(_) => {}
                RandomMapUpdate::Finished(map) => break map,
            }
        };
        assert_eq!(entries, 1);
        let main_rng = crate::sim::rng::MainRng::new(0x0401_599d);
        let mut main = crate::app::state::process_main_draws(&main_rng);
        let mut selector = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let resolved = build_random_map_preview_grid(
            &generated.map_file,
            Some(&prepared.theater),
            Some(&assets),
            Some(&prepared.terrain_rules),
            &mut main,
            &mut selector,
        );
        let cells = crate::map::rmg::preview::preview_cells_from_map(
            &generated.map_file,
            &resolved,
            &|_, _, _| None,
            &|_, _, _| None,
        );
        let occupied: std::collections::HashSet<_> = generated
            .map_file
            .entities
            .iter()
            .flat_map(|entity| {
                // An Ares build's unvalidated decks (hook 0x58FA51) can add
                // a one-cell CABHUT, which is no tech building.
                tech_footprints
                    .get(&entity.type_id)
                    .cloned()
                    .unwrap_or_else(|| vec![(0, 0)])
                    .into_iter()
                    .map(move |(dx, dy)| {
                        (
                            entity.cell_x.wrapping_add_signed(dx),
                            entity.cell_y.wrapping_add_signed(dy),
                        )
                    })
            })
            .collect();
        let cell = cells
            .iter()
            .find(|cell| {
                let tile = resolved.cell(cell.x, cell.y).expect("preview Cell");
                tile.final_tile_index == selected["tile"].as_i64().unwrap() as i32
                    && tile.final_sub_tile == selected["subtile"].as_u64().unwrap() as u8
                    && tile.variant == 0
                    && (1..=12).contains(&tile.level)
                    && tile.slope_type == 0
                    && !tile.has_bridge_deck
                    && tile.terrain_object_occupation.is_none()
                    && !occupied.contains(&(cell.x, cell.y))
                    && !generated
                        .map_file
                        .terrain_objects
                        .iter()
                        .any(|object| object.rx == cell.x && object.ry == cell.y)
                    && !generated
                        .map_file
                        .overlays
                        .iter()
                        .any(|overlay| overlay.rx == cell.x && overlay.ry == cell.y)
            })
            .expect("seed4242 must actually generate an unobscured raised pristine Clear Cell");
        let tile = resolved.cell(cell.x, cell.y).unwrap();
        let presentation_tile = resolved.presentation_tile(tile);
        assert_eq!(presentation_tile, (0, 0));
        let name = prepared
            .theater
            .lookup
            .filename_for_variant(presentation_tile.0, tile.variant)
            .expect("registered selected TMP");
        assert!(name.eq_ignore_ascii_case(selected["name"].as_str().unwrap()));
        let (bytes, source) = assets.get_with_source_ref(name).expect("active TMP winner");
        assert_eq!(
            crate::util::sha256::sha256_hex(bytes),
            selected["physical_sha256"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::json!([tile.radar_left, tile.radar_right]),
            selected["raw_pair_rgb"]
        );
        let row = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["level"].as_u64() == Some(u64::from(tile.level)))
            .unwrap();
        println!(
            "RMG preview Cell=({},{}), level={}, tile={}, sub={}, variant={}, file={name}, source={source}",
            cell.x, cell.y, tile.level, tile.final_tile_index, tile.final_sub_tile, tile.variant
        );
        assert_eq!(
            serde_json::json!([cell.left, cell.right]),
            row["output"]["preview_rgb_pair"],
            "production raised-cell colors must consume original549E90/661190/661020 outputs"
        );
        let format = crate::render::native_surface_format::RGB565;
        assert_eq!(
            (u32::from(format.pack_rgb8(cell.left)) << 16)
                | u32::from(format.pack_rgb8(cell.right)),
            row["output"]["cell_packed"].as_u64().unwrap() as u32,
            "native Cell puts the left word in the high half"
        );

        let preview_rules = prepared
            .preview_rules
            .as_ref()
            .expect("cold RMG rules projection");
        let registry = preview_rules.rules.overlay_registry();
        let generated_colored = random_map_preview_cells(
            &generated.map_file,
            &resolved,
            Some(preview_rules),
            Some(&assets),
            prepared.theater.extension,
        );
        let kernel = crate::test_fixture::json("tools/spatial_oracle/packed_color_mix.json");
        for name in ["TIB01", "GEM01"] {
            let base = registry.id_for_name(name).unwrap();
            let overlay = generated
                .map_file
                .overlays
                .iter()
                .find(|overlay| {
                    (base..base + 12).contains(&overlay.overlay_id)
                        && overlay.frame < 2
                        && generated_colored
                            .iter()
                            .any(|cell| (cell.x, cell.y) == (overlay.rx, overlay.ry))
                })
                .expect("seed4242 must produce a visible low-density resource cell");
            let source = format.pack_rgb8(
                registry
                    .flags(overlay.overlay_id)
                    .unwrap()
                    .radar_color
                    .unwrap(),
            );
            let expected = kernel
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row["format"] == "rgb565"
                        && row["first"].as_u64() == Some(u64::from(source))
                        && row["amount"] == 25
                })
                .expect("original numeric control uses this retail type color")["output"]
                .as_u64()
                .unwrap() as u16;
            let cell = generated_colored
                .iter()
                .find(|cell| (cell.x, cell.y) == (overlay.rx, overlay.ry))
                .unwrap();
            assert_eq!(
                (format.pack_rgb8(cell.left), format.pack_rgb8(cell.right)),
                (expected, expected),
                "{name} at {},{} density{} must use native type color and packed suffix",
                overlay.rx,
                overlay.ry,
                overlay.frame
            );
        }

        // Supply a flat copy of the generated Clear Cell for the existing
        // original level0 control, plus the density boundary inputs. These
        // levels/stages are controlled inputs, not seed4242 output claims.
        let flat = cell;
        let mut control = generated.map_file;
        control
            .cells
            .retain(|cell| (cell.rx, cell.ry) == (flat.x, flat.y));
        control.cells[0].z = 0;
        let resolved = build_random_map_preview_grid(
            &control,
            Some(&prepared.theater),
            Some(&assets),
            Some(&prepared.terrain_rules),
            &mut main,
            &mut selector,
        );
        let ground = crate::map::rmg::preview::preview_cells_from_map(
            &control,
            &resolved,
            &|_, _, _| None,
            &|_, _, _| None,
        );
        assert_eq!(format.pack_rgb8(ground[0].right), 42282);
        control.overlays = vec![crate::map::overlay::OverlayEntry {
            rx: flat.x,
            ry: flat.y,
            overlay_id: registry.id_for_name("TIB01").unwrap(),
            frame: 1,
        }];
        let output = |first: u16, second: u16, amount: u32| {
            kernel
                .as_array()
                .unwrap()
                .iter()
                .find(|row| {
                    row["format"] == "rgb565"
                        && row["first"] == first
                        && row["second"] == second
                        && row["amount"] == amount
                })
                .expect("original mixer control")["output"]
                .as_u64()
                .unwrap() as u16
        };
        let quarter = output(56896, 42282, 25);
        let half_first = output(56896, 42282, 50);
        for (density, expected) in [
            (1, (quarter, quarter)),
            (2, (output(56896, 0, 50), output(56896, half_first, 50))),
            (5, (quarter, quarter)),
        ] {
            control.overlays[0].frame = density;
            let cell = random_map_preview_cells(
                &control,
                &resolved,
                Some(preview_rules),
                Some(&assets),
                prepared.theater.extension,
            )[0];
            assert_eq!(
                (format.pack_rgb8(cell.left), format.pack_rgb8(cell.right)),
                expected,
                "density{density}"
            );
        }

        // Real Process passes must choose the committed mode's color and
        // discard an outgoing scenario's color before the first new preview.
        rules
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
                    mode_rules.as_ref(),
                ),
                &crate::rules::ini_parser::IniFile::from_str("[TIB01]\nRadarColor=1,2,3\n"),
            )
            .unwrap();
        let mode = crate::rules::ini_parser::IniFile::from_str("[TIB01]\nRadarColor=140,0,140\n");
        rules
            .prepare_random_map_preview(Some(&mode), "TEMPERATE", true)
            .unwrap();
        let mode_projection = RandomMapPreviewRules::from_processed(
            &rules,
            rules.prepared_random_map_rules().unwrap(),
            &assets,
            prepared.theater.extension,
        )
        .unwrap();
        control.overlays[0].frame = 1;
        let colored = random_map_preview_cells(
            &control,
            &resolved,
            Some(&mode_projection),
            Some(&assets),
            prepared.theater.extension,
        );
        let expected = output(34833, 42282, 25);
        assert_eq!(
            (
                format.pack_rgb8(colored[0].left),
                format.pack_rgb8(colored[0].right)
            ),
            (expected, expected),
            "mode overrides ROOT, outgoing map does not survive reset"
        );

        // Use the production common teardown writer, then the chooser's PCX
        // decoder, so the two colored halves survive the persisted thumbnail.
        let preview = crate::map::rmg::preview::render_preview(&generated_colored, &[]).unwrap();
        let dir = crate::map::source::test_support::TestDirectory::new("overlay-preview");
        let mut modal = Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
            crate::map::rmg::RmgOptions {
                seed: 4242,
                ..Default::default()
            },
            None,
            false,
            true,
        ));
        modal
            .as_mut()
            .unwrap()
            .finish_generate(Some(preview.clone()));
        let mut retention = RandomMapGenerationRetention::default();
        dismiss_random_map_setup_owners(&mut modal, &mut retention, Some(dir.path()));
        assert!(modal.is_none());
        let path = dir.path().join(RANDMAP_PREVIEW_FILE);
        let pcx =
            crate::assets::pcx_file::PcxFile::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
        let expected_rgb: Vec<u8> = preview
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|rgba| rgba[..3].iter().copied())
            .collect();
        assert_eq!(pcx.pixels, expected_rgb);
    }

    #[test]
    fn gsi_04_12_preview_file_is_teardown_owned_and_precedes_accepted_commit() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let profile = std::env::temp_dir().join(format!(
            "vera20k-rmg-teardown-{}-{unique}",
            std::process::id()
        ));
        // VERA's Maps folder, which a new profile does not have yet.
        let base = profile.join("Maps");
        let img_path = base.join(RANDMAP_PREVIEW_FILE);
        let sed_path = base.join(RANDMAP_SED_FILE);
        let options = crate::map::rmg::RmgOptions {
            seed: 0x412,
            ..Default::default()
        };
        let preview = crate::map::rmg::preview::PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![0x12, 0x34, 0x56, 0xFF],
        };
        let mut runtime =
            crate::app::frontend::skirmish_session::MultiplayerShellRuntime::initialize(
                0x0412_959B,
                &mut crate::app::process_assets::ProcessAssets::new(
                    crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
                    false,
                ),
                None,
                None,
                None,
                crate::app::frontend::skirmish_session::skirmish_global_defaults(
                    &crate::ui::skirmish_shell::SkirmishShellState::default(),
                ),
            );
        let mut retention = RandomMapGenerationRetention::default();
        retention.finish_generation(generated_preview(0x412, 10));
        let mut modal = Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
            options, None, false, true,
        ));
        modal
            .as_mut()
            .expect("accepted modal")
            .finish_generate(Some(preview.clone()));

        // Worker completion only fills dialog memory. Until common teardown,
        // the chooser file remains absent. The production accepted owner then
        // publishes it before returning options to the `.SED` caller.
        assert!(!img_path.exists());
        let committed =
            accept_random_map_setup_owners(&mut runtime, &mut modal, &mut retention, Some(&base))
                .expect("generated modal accepts");
        assert!(img_path.exists(), "common teardown must publish .img first");
        // OK stages the accepted map's starts for the LAN 'W' (0x0058B820).
        let accepted_staging = runtime.start_staging();
        assert_eq!(
            accepted_staging.defined_cells().collect::<Vec<_>>(),
            vec![(0, (10, 20))]
        );
        assert!(!sed_path.exists(), ".SED cannot precede common teardown");
        std::fs::write(&sed_path, committed.to_sed_bytes()).expect("write accepted seed");
        assert!(sed_path.exists());

        // Cancel runs the same teardown writer but never reaches the accepted
        // caller, so it updates only .img.
        std::fs::remove_file(&sed_path).expect("remove accepted seed fixture");
        let mut cancel_retention = RandomMapGenerationRetention::default();
        cancel_retention.finish_generation(generated_preview(0x413, 12));
        let mut cancel_modal = Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
            crate::map::rmg::RmgOptions {
                seed: 0x413,
                ..Default::default()
            },
            None,
            false,
            true,
        ));
        cancel_modal
            .as_mut()
            .expect("cancel modal")
            .finish_generate(Some(preview));
        let _ = cancel_random_map_setup_owners(
            &mut runtime,
            &mut cancel_modal,
            &mut cancel_retention,
            Some(&base),
        );
        assert!(img_path.exists());
        assert!(!sed_path.exists());
        assert_eq!(
            runtime.start_staging(),
            accepted_staging,
            "a cancelled Generate leaves the 'W' staging on the accepted map"
        );

        std::fs::remove_file(&img_path).expect("remove preview fixture");
        std::fs::remove_dir(&base).expect("remove teardown fixture directory");
        std::fs::remove_dir(&profile).expect("remove teardown profile directory");
    }

    #[test]
    fn six_preview_boundaries_cover_the_originals_eight_redraws() {
        use crate::map::rmg::STAGE_ORDER;
        use crate::map::rmg::build::GenerationPoint;

        let drawn: Vec<GenerationPoint> = std::iter::once(GenerationPoint::Initial)
            .chain(
                STAGE_ORDER
                    .iter()
                    .map(|stage| GenerationPoint::After(*stage)),
            )
            .filter(|point| draws_preview(*point))
            .collect();
        // Eight redraws in the original, two of them repeats of the image
        // already on screen.
        assert_eq!(drawn.len(), 6, "{drawn:?}");
        assert_eq!(drawn[0], GenerationPoint::Initial);
        // The last one precedes the final recalc, so the finished map still
        // differs from it and the closing draw is not a repeat either.
        assert_eq!(
            drawn[5],
            GenerationPoint::After(crate::map::rmg::Stage::Rocks)
        );
    }

    fn apply_storage_decision_for_test(
        retention: &mut RandomMapGenerationRetention,
        options: &crate::map::rmg::RmgOptions,
        process_dummy: &crate::map::resolved_terrain::SharedCellDummy,
    ) -> bool {
        let decision = retention.begin_map_storage_decision(options);
        decision.apply_resize_to(process_dummy);
        retention.commit_map_storage_decision(decision);
        decision.reinitialize
    }

    #[test]
    fn gsi_04_01_rmg_resize_predicate_matches_cached_native_tuple() {
        let mut retention = RandomMapGenerationRetention::default();
        let process_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
        process_dummy.set_level_slope(-7, 11);
        process_dummy.stamp_coord(7, 9);
        let options = crate::map::rmg::RmgOptions::default();

        retention.begin_generation();
        assert!(apply_storage_decision_for_test(
            &mut retention,
            &options,
            &process_dummy
        ));
        assert_eq!(
            process_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );

        process_dummy.set_level_slope(-3, 5);
        process_dummy.stamp_coord(12, -4);
        let expected = process_dummy.snapshot();
        let mut same_storage = options.clone();
        same_storage.seed = 0x401;
        same_storage.map_type = 4;
        same_storage.resources = 3;
        retention.begin_generation();
        assert!(!apply_storage_decision_for_test(
            &mut retention,
            &same_storage,
            &process_dummy
        ));
        assert_eq!(
            process_dummy.snapshot(),
            expected,
            "repeated Generate in one open dialog reuses the cached RMG map storage"
        );

        type OptionsChange = fn(&mut crate::map::rmg::RmgOptions);
        let changes: [(&str, OptionsChange); 4] = [
            ("theater", |changed| changed.theater = 1),
            ("num_players", |changed| changed.num_players = 3),
            ("width", |changed| changed.width = 1),
            ("height", |changed| changed.height = 1),
        ];
        for (field, change) in changes {
            let mut isolated_retention = RandomMapGenerationRetention::default();
            let isolated_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
            assert!(apply_storage_decision_for_test(
                &mut isolated_retention,
                &options,
                &isolated_dummy
            ));
            isolated_dummy.set_level_slope(-6, 9);
            isolated_dummy.stamp_coord(8, -10);
            let mut changed = options.clone();
            change(&mut changed);
            assert_ne!(
                RandomMapStorageKey::from_options(&changed),
                RandomMapStorageKey::from_options(&options),
                "{field} fixture must remain distinct after native normalization"
            );
            assert!(apply_storage_decision_for_test(
                &mut isolated_retention,
                &changed,
                &isolated_dummy
            ));
            assert_eq!(
                isolated_dummy.snapshot(),
                crate::map::resolved_terrain::SharedCellDummySnapshot {
                    coord: (0, 0),
                    level: 0,
                    slope_type: 0,
                    bridge_flags_0x1180: 0,
                }
            );
        }

        let mut failed_attempt = options.clone();
        failed_attempt.width = 1;
        assert!(
            retention
                .begin_map_storage_decision(&failed_attempt)
                .reinitialize
        );
        assert!(
            retention.map_storage_decision(&options).reinitialize,
            "a failed miss cannot reuse the previous key against a partially rebuilt registry"
        );
    }

    #[test]
    fn gsi_04_01_rmg_dialog_teardown_destroys_cached_storage_on_ok_and_cancel() {
        let options = crate::map::rmg::RmgOptions::default();

        for accepted in [true, false] {
            let close_path = if accepted { "OK" } else { "Cancel" };
            let mut retention = RandomMapGenerationRetention::default();
            let process_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();

            retention.begin_generation();
            assert!(apply_storage_decision_for_test(
                &mut retention,
                &options,
                &process_dummy
            ));
            retention.finish_generation(generated_preview(0x401, 10));
            if accepted {
                retention.accept_setup(RANDMAP_SED_FILE);
            } else {
                retention.cancel_setup();
            }
            // The common owner destroys native map storage after preview
            // disposition has already been selected.
            retention.destroy_map_storage();

            process_dummy.set_level_slope(-4, 7);
            process_dummy.stamp_coord(13, -9);
            retention.begin_generation();
            assert!(
                apply_storage_decision_for_test(&mut retention, &options, &process_dummy),
                "the first Generate after {close_path} must allocate fresh RMG map storage"
            );
            assert_eq!(
                process_dummy.snapshot(),
                crate::map::resolved_terrain::SharedCellDummySnapshot {
                    coord: (0, 0),
                    level: 0,
                    slope_type: 0,
                    bridge_flags_0x1180: 0,
                },
                "the first Generate after {close_path} must run MapClass::Resize"
            );
        }
    }

    #[test]
    fn gsi_04_01_rmg_preview_resolution_does_not_reconstruct_dummy() {
        let process_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
        process_dummy.set_level_slope(-3, 5);
        process_dummy.stamp_coord(12, -4);
        let expected = process_dummy.snapshot();
        let map = generated_preview(11, 10).map_file;
        let frontend_main_rng = crate::sim::rng::MainRng::new(0x0401_599D);
        let mut selector_cache =
            crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        for _ in 0..6 {
            let mut main = crate::app::state::process_main_draws(&frontend_main_rng);
            let preview_grid = build_random_map_preview_grid(
                &map,
                None,
                None,
                None,
                &mut main,
                &mut selector_cache,
            );
            assert!(
                !preview_grid
                    .shared_cell_dummy()
                    .same_identity(&process_dummy)
            );
            assert_eq!(
                process_dummy.snapshot(),
                expected,
                "preview rasterization must not run another MapClass Resize"
            );
        }
    }

    #[test]
    fn every_generate_records_the_start_header_and_cancel_keeps_it() {
        let mut retention = RandomMapGenerationRetention::default();
        assert_eq!(retention.generated_header().count, 0, "before any Generate");
        retention.finish_generation(generated_preview(0x501, 10));
        let header = retention.generated_header();
        assert_eq!(header.count, 1);
        assert_eq!(header.cells[0], (10, 20));
        retention.cancel_setup();
        assert_eq!(
            retention.generated_header(),
            header,
            "0x0058B820's globals outlive Cancel"
        );
    }

    fn generated_preview(seed: i32, start_x: u16) -> crate::map::rmg::GeneratedMap {
        let options = crate::map::rmg::RmgOptions {
            seed,
            ..Default::default()
        };
        crate::map::rmg::GeneratedMap {
            map_file: crate::map::rmg::emit::empty_map_file(&options, 32, 32),
            mapgen_continuation: Some(
                crate::map::rmg::RmgRng::new(seed as u16).into_continuation(),
            ),
            construction_trace: crate::map::rmg::RmgConstructionTrace::default(),
            start_waypoints: vec![(0, start_x, 20)],
            stages_run: Vec::new(),
            unfilled_start_slots: 0,
            fairness: None,
        }
    }

    fn complete_process_preview(
        retention: &mut RandomMapGenerationRetention,
        resident: Option<&mut crate::sim::world::Simulation>,
        seed: u16,
    ) -> crate::sim::rng::SimRngLogicalState {
        let mut rng = crate::map::rmg::RmgRng::new(seed);
        for _ in 0..353 {
            let _ = rng.next_u32();
        }
        let expected =
            crate::sim::rng::SimRng::from_mapgen_continuation(rng.clone().into_continuation())
                .logical_state();
        let mut generated = generated_preview(i32::from(seed), 10);
        generated.mapgen_continuation = Some(rng.into_continuation());
        let options = crate::map::rmg::RmgOptions {
            seed: i32::from(seed),
            ..Default::default()
        };
        let mut modal =
            crate::ui::skirmish_shell::RandomMapSetupModalState::open(options, None, false, true);
        modal.begin_generate();
        let mut runtime =
            crate::app::frontend::skirmish_session::MultiplayerShellRuntime::initialize(
                0x52FC_594C,
                &mut crate::app::process_assets::ProcessAssets::new(
                    crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
                    false,
                ),
                None,
                None,
                None,
                crate::app::frontend::skirmish_session::skirmish_global_defaults(
                    &crate::ui::skirmish_shell::SkirmishShellState::default(),
                ),
            );
        assert!(!finish_random_map_generation_owners(
            resident,
            &mut runtime,
            retention,
            &mut modal,
            generated,
            None,
            false,
        ));
        assert!(
            retention
                .candidate
                .as_ref()
                .expect("completed preview retained")
                .mapgen_continuation
                .is_none(),
            "presentation must no longer own the process cursor"
        );
        expected
    }

    #[test]
    fn completed_preview_publishes_only_mapgen_to_resident_simulation() {
        let mut simulation = crate::sim::world::Simulation::from_descriptor(
            &crate::sim::scenario_session::ScenarioDescriptor {
                seed: 0x52FC_1000,
                ..Default::default()
            },
        );
        let before = simulation.rng_state();
        let mut retention = RandomMapGenerationRetention::default();
        let expected = complete_process_preview(&mut retention, Some(&mut simulation), 0xBEEF);
        let after = simulation.rng_state();

        assert_eq!(after.mapgen, expected);
        assert_ne!(after.mapgen, before.mapgen);
        assert_eq!(after.scenario, before.scenario);
        assert_eq!(after.main, before.main);
        assert!(retention.pending_process_mapgen.is_none());
        let transported = retention
            .snapshot_mapgen_continuation_for_fresh_load(Some(&simulation))
            .expect("resident cursor snapshot");
        assert_eq!(
            crate::sim::rng::SimRng::from_mapgen_continuation(transported).logical_state(),
            expected
        );
        assert_eq!(simulation.rng_state(), after, "snapshot cannot advance RNG");
    }

    #[test]
    fn cold_completed_preview_cursor_survives_presentation_disposition() {
        for disposition in 0..5 {
            let mut retention = RandomMapGenerationRetention::default();
            let expected = complete_process_preview(&mut retention, None, 0xC01D);
            match disposition {
                0 => retention.begin_generation(),
                1 => retention.cancel_setup(),
                2 => {
                    retention.accept_setup(RANDMAP_SED_FILE);
                    let _ = retention.take_acceptance_for_loading(Some(RANDMAP_SED_FILE));
                }
                3 => {
                    retention.accept_setup(RANDMAP_SED_FILE);
                    retention.select_map("OTHER.MAP");
                }
                4 => retention.destroy_map_storage(),
                _ => unreachable!(),
            }
            let transported = retention
                .snapshot_mapgen_continuation_for_fresh_load(None)
                .expect("completed cold process cursor survives preview disposition");
            assert_eq!(
                crate::sim::rng::SimRng::from_mapgen_continuation(transported).logical_state(),
                expected,
                "disposition {disposition}"
            );
            let second_snapshot = retention
                .snapshot_mapgen_continuation_for_fresh_load(None)
                .expect("an inactive attempt cannot retire the process cursor");
            assert_eq!(
                crate::sim::rng::SimRng::from_mapgen_continuation(second_snapshot).logical_state(),
                expected
            );
            retention.discard_pending_mapgen_on_simulation_install();
            assert!(
                retention
                    .snapshot_mapgen_continuation_for_fresh_load(None)
                    .is_none()
            );
        }

        let mut retention = RandomMapGenerationRetention::default();
        let first = complete_process_preview(&mut retention, None, 0xC01D);
        retention.begin_generation();
        let second = complete_process_preview(&mut retention, None, 0xC02D);
        assert_ne!(first, second);
        let transported = retention
            .snapshot_mapgen_continuation_for_fresh_load(None)
            .expect("latest completed run wins");
        assert_eq!(
            crate::sim::rng::SimRng::from_mapgen_continuation(transported).logical_state(),
            second
        );
    }

    #[test]
    fn cold_mapgen_survives_failed_staging_and_retires_only_on_successful_install() {
        let mut retention = RandomMapGenerationRetention::default();
        let first = complete_process_preview(&mut retention, None, 0xC01D);
        let descriptor = crate::sim::scenario_session::ScenarioDescriptor {
            seed: 123,
            ..Default::default()
        };
        let stage_attempt = |retention: &RandomMapGenerationRetention| {
            let mut bootstrap = crate::sim::scenario_bootstrap::ScenarioBootstrapRng::new(123);
            bootstrap.install_process_mapgen_continuation(
                retention
                    .snapshot_mapgen_continuation_for_fresh_load(None)
                    .expect("retained process receipt"),
            );
            bootstrap.into_simulation(&descriptor)
        };

        let failed_staging = stage_attempt(&retention);
        assert_eq!(failed_staging.rng_state().mapgen, first);
        // Loading cancellation, request replacement and any later authored
        // loader error discard inactive staging. None retires this owner.
        drop(failed_staging);
        retention.cancel_setup();
        retention.select_map("all01umd.map");
        let retry = stage_attempt(&retention);
        assert_eq!(retry.rng_state().mapgen, first);
        drop(retry);

        // Launch-time .SED generation publishes through the same seam before
        // fallible admission. Its newer native cursor survives another error.
        let second = complete_process_preview(&mut retention, None, 0xC02D);
        assert_ne!(first, second);
        let failed_regenerated_load = stage_attempt(&retention);
        assert_eq!(failed_regenerated_load.rng_state().mapgen, second);
        drop(failed_regenerated_load);
        let installed = stage_attempt(&retention);
        retention.discard_pending_mapgen_on_simulation_install();
        assert!(
            retention
                .snapshot_mapgen_continuation_for_fresh_load(None)
                .is_none()
        );
        let live_snapshot = retention
            .snapshot_mapgen_continuation_for_fresh_load(Some(&installed))
            .expect("success leaves the resident Simulation authoritative");
        assert_eq!(
            crate::sim::rng::SimRng::from_mapgen_continuation(live_snapshot).logical_state(),
            second
        );
    }

    #[test]
    fn resident_and_unconsumed_cold_mapgen_are_rejected_before_mutation() {
        let mut retention = RandomMapGenerationRetention::default();
        let cold = complete_process_preview(&mut retention, None, 0xC01D);
        let mut simulation = crate::sim::world::Simulation::from_descriptor(
            &crate::sim::scenario_session::ScenarioDescriptor::default(),
        );
        let before = simulation.rng_state();
        let completion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            complete_process_preview(&mut retention, Some(&mut simulation), 0xBEEF)
        }));
        assert!(completion.is_err());
        assert_eq!(simulation.rng_state(), before);

        let load = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            retention.snapshot_mapgen_continuation_for_fresh_load(Some(&simulation))
        }));
        assert!(load.is_err());
        let transported = retention
            .snapshot_mapgen_continuation_for_fresh_load(None)
            .expect("rejected duplicate owner cannot drop cold transport");
        assert_eq!(
            crate::sim::rng::SimRng::from_mapgen_continuation(transported).logical_state(),
            cold
        );
    }

    #[test]
    fn gsi_04_12_random_map_preview_invalidates_and_transfers_exactly_once() {
        let mut regenerated = RandomMapGenerationRetention::default();
        regenerated.finish_generation(generated_preview(11, 10));
        regenerated.accept_setup("RandMap.Sed");
        regenerated.begin_generation();
        assert!(
            regenerated
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "starting a genuine regeneration invalidates accepted map A"
        );

        let mut reopened_then_cancelled_without_generate = RandomMapGenerationRetention::default();
        reopened_then_cancelled_without_generate.finish_generation(generated_preview(12, 11));
        reopened_then_cancelled_without_generate.accept_setup("RandMap.Sed");
        reopened_then_cancelled_without_generate.cancel_setup();
        assert!(
            reopened_then_cancelled_without_generate
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "a genuine setup Cancel invalidates accepted map A"
        );

        let mut reopened_then_cancelled = RandomMapGenerationRetention::default();
        reopened_then_cancelled.finish_generation(generated_preview(13, 12));
        reopened_then_cancelled.accept_setup("RandMap.Sed");
        reopened_then_cancelled.begin_generation();
        reopened_then_cancelled.finish_generation(generated_preview(14, 13));
        reopened_then_cancelled.cancel_setup();
        assert!(
            reopened_then_cancelled
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "reopen, regenerate, then Cancel cannot resurrect accepted map A"
        );

        let mut cancelled = RandomMapGenerationRetention::default();
        cancelled.finish_generation(generated_preview(22, 20));
        cancelled.cancel_setup();
        cancelled.accept_setup("RandMap.Sed");
        assert!(
            cancelled
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none()
        );

        let mut selected_elsewhere = RandomMapGenerationRetention::default();
        selected_elsewhere.finish_generation(generated_preview(33, 30));
        selected_elsewhere.accept_setup("RandMap.Sed");
        selected_elsewhere.select_map("mp01t4.map");
        assert!(
            selected_elsewhere
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none()
        );

        let mut alternate_seed = RandomMapGenerationRetention::default();
        alternate_seed.finish_generation(generated_preview(34, 35));
        alternate_seed.accept_setup("RandMap.Sed");
        // This is the retention authority used by apply_selected_shell_map_index:
        // another seed selection must invalidate, not merely refuse this call.
        alternate_seed.select_map("Other.Sed");
        assert!(
            alternate_seed
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none()
        );
        assert!(
            alternate_seed
                .take_preview_for_loading(Some("Other.Sed"))
                .is_none()
        );

        let mut mismatched_launch = RandomMapGenerationRetention::default();
        mismatched_launch.finish_generation(generated_preview(35, 36));
        mismatched_launch.accept_setup("RandMap.Sed");
        assert!(
            mismatched_launch
                .take_preview_for_loading(Some("Other.Sed"))
                .is_none()
        );
        assert!(
            mismatched_launch
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "a refused nonmatching launch invalidates the prior accepted map"
        );

        let mut accepted_after_successful_close = RandomMapGenerationRetention::default();
        accepted_after_successful_close.finish_generation(generated_preview(44, 40));
        accepted_after_successful_close.accept_setup("RandMap.Sed");
        // Common dialog teardown destroys only the cached RMG map storage; it
        // must not discard the accepted launch bundle awaiting loading.
        accepted_after_successful_close.destroy_map_storage();
        accepted_after_successful_close.select_map("RANDMAP.SED");
        let transferred = accepted_after_successful_close
            .take_preview_for_loading(Some("randmap.sed"))
            .expect("successful accept close preserves the accepted preview");
        assert_eq!(transferred.start_waypoints, vec![(0, 40, 20)]);
        assert!(
            accepted_after_successful_close
                .take_preview_for_loading(Some("RandMap.Sed"))
                .is_none(),
            "successful accept preview still transfers exactly once"
        );
    }
}

#[cfg(test)]
#[path = "random_map_ramp_end_tests.rs"]
mod ramp_end_tests;
