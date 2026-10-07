//! Per-match lighting lifetime and ordered publication before drawing.
//! Simulation supplies source/global events; this owner retains cell sampling
//! history and publishes each affected area across install, restore and refresh.
use crate::map::lighting::{
    self, CellLightGrid, CellRelightProfile, LightingConfig, LightingProfileUnits, PointLight,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// Ordinary Techno drawers select a ColorScheme Convert independently of
/// the cell scalar: 00705D70 -> 0070720E; Init_Theater 00534D77 gives schemes
/// neutral RGB. Ion propagation 0053C280 -> 0053AD00 substitutes global RGB.
/// Aircraft altitude brightness and trigger-driven palette rebuild history
/// remain unresolved; the former retains its existing compatibility path.
pub(crate) fn body_palette_light(
    grid: &CellLightGrid,
    scenario: &crate::sim::scenario_session::ScenarioLightingState,
    cell: (u16, u16),
    category: crate::map::entities::EntityCategory,
    extra_unit: i32,
    extra_infantry: i32,
) -> crate::render::palette_light::PaletteLight {
    use crate::map::entities::EntityCategory;
    use crate::render::palette_light::PaletteLight;
    if category == EntityCategory::Aircraft {
        return PaletteLight::default();
    }
    let light = grid.cell_light_at(cell);
    let brightness = match category {
        EntityCategory::Unit => light
            .map_or(1000, |l| l.top_scalar)
            .wrapping_add(extra_unit),
        EntityCategory::Infantry => light
            .map_or(1000, |l| l.top_scalar)
            .wrapping_add(extra_infantry),
        _ => light.map_or(1000, |l| l.top_scalar),
    };
    let rgb = color_scheme_rgb(Some(scenario));
    PaletteLight::color_scheme(rgb, brightness)
}

/// Ordinary building DrawBody 0043D812..0043D85F; buildup 0043D644;
/// TerrainPalette overrides both scalar and Convert in 00705EC7..00705F52.
pub(crate) fn building_palette_light(
    grid: &CellLightGrid,
    scenario: &crate::sim::scenario_session::ScenarioLightingState,
    cell: (u16, u16),
    art: Option<&crate::rules::art_data::ArtEntry>,
    buildup: bool,
) -> crate::render::palette_light::PaletteLight {
    use crate::render::palette_light::PaletteLight;
    if art.is_some_and(|a| a.terrain_palette) {
        return PaletteLight::cell(grid, cell, false);
    }
    let top = grid.cell_light_at(cell).map_or(1000, |l| l.top_scalar);
    let extra = if buildup {
        0
    } else {
        art.map_or(0, |a| i32::from(a.extra_light as i16))
    };
    PaletteLight::color_scheme(color_scheme_rgb(Some(scenario)), top.wrapping_add(extra))
}

/// AnimClass DrawIt 00423280..00423354: explicit cell drawer selects cell
/// common; otherwise global ANIM Convert selects top. AltPalette selects the
/// first ColorScheme (one row), not the player's 53-row scheme.
pub(crate) fn anim_palette_light(
    grid: &CellLightGrid,
    scenario: Option<&crate::sim::scenario_session::ScenarioLightingState>,
    cell: (u16, u16),
    config: Option<&crate::rules::art_data::AnimTypeRuntimeConfig>,
    cell_drawer: bool,
) -> crate::render::palette_light::PaletteLight {
    use crate::render::palette_light::PaletteLight;
    let brightness = if config.is_some_and(|c| c.use_normal_light) {
        1000
    } else {
        grid.cell_light_at(cell).map_or(1000, |l| {
            if cell_drawer {
                l.common_scalar
            } else {
                l.top_scalar
            }
        })
    };
    if cell_drawer {
        PaletteLight::cell(grid, cell, false).with_brightness(brightness)
    } else if config.is_some_and(|c| c.alt_palette) {
        PaletteLight::new(color_scheme_rgb(scenario), 1, brightness, true)
    } else {
        PaletteLight::plain(53, brightness)
    }
}

pub(crate) fn color_scheme_rgb(
    scenario: Option<&crate::sim::scenario_session::ScenarioLightingState>,
) -> [i32; 3] {
    scenario
        .and_then(ScenarioLightingState::alternate_rgb)
        .unwrap_or([1000; 3])
}

use crate::sim::light_sources::LightingEvent;
use crate::sim::scenario_session::ScenarioLightingState;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct MatchLighting {
    grid: CellLightGrid,
    config: LightingConfig,
    buildings: BTreeMap<u64, PointLight>,
    radiation: BTreeMap<(u16, u16), PointLight>,
    profile: CellRelightProfile,
    scenario: Option<ScenarioLightingState>,
    detail_level: u32,
}

impl Default for MatchLighting {
    fn default() -> Self {
        Self {
            grid: CellLightGrid::new(),
            config: LightingConfig::default(),
            buildings: BTreeMap::new(),
            radiation: BTreeMap::new(),
            profile: lighting::normal_profile_units(&LightingConfig::default()).into(),
            scenario: None,
            detail_level: 2,
        }
    }
}

impl MatchLighting {
    pub(crate) fn grid(&self) -> &CellLightGrid {
        &self.grid
    }

    pub(crate) fn install(
        &mut self,
        grid: CellLightGrid,
        config: LightingConfig,
        detail_level: u32,
        live: Option<(&ResolvedTerrainGrid, &Simulation, &RuleSet)>,
    ) {
        *self = Self {
            grid,
            config,
            detail_level: detail_level.min(2),
            ..Self::default()
        };
        self.profile = lighting::normal_profile_units(&self.config).into();
        if let Some((terrain, sim, rules)) = live {
            let view = derive_lighting_view(&self.config, Some(sim), Some(rules), detail_level);
            self.grid = build_lighting_grid_from_view(terrain, &view);
            self.profile = view.profile;
            self.scenario = Some(sim.session.lighting);
            (self.buildings, self.radiation) = source_maps(sim, rules);
        }
    }

    /// Preserve the existing eager restore compatibility, discarding every
    /// outgoing derived input. Native lazy Cell34 reinitialization is separate.
    pub(crate) fn restore(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        sim: &Simulation,
        rules: &RuleSet,
        detail_level: u32,
    ) {
        self.install(
            CellLightGrid::new(),
            self.config.clone(),
            detail_level,
            Some((terrain, sim, rules)),
        );
    }

    /// Replay native source/global ordering before a draw. Source changes can
    /// coalesce only between global operations; their entire dirty-area union
    /// survives even if the source registry returns to identical final values.
    pub(crate) fn apply_events(&mut self, terrain: &ResolvedTerrainGrid, events: &[LightingEvent]) {
        let mut dirty = BTreeSet::new();
        for event in events {
            match event {
                LightingEvent::Building { id, source } => {
                    let old = match source {
                        Some(source) => self.buildings.insert(*id, source.clone()),
                        None => self.buildings.remove(id),
                    };
                    queue_area(old.as_ref(), terrain, self.detail_level, &mut dirty);
                    queue_area(source.as_ref(), terrain, self.detail_level, &mut dirty);
                }
                LightingEvent::Radiation { center, source } => {
                    let old = match source {
                        Some(source) => self.radiation.insert(*center, source.clone()),
                        None => self.radiation.remove(center),
                    };
                    queue_area(old.as_ref(), terrain, 2, &mut dirty);
                    queue_area(source.as_ref(), terrain, 2, &mut dirty);
                }
                LightingEvent::Global {
                    state,
                    cell_profile,
                } => {
                    self.commit_source_cells(terrain, &mut dirty);
                    self.profile = scenario_profile(state, *cell_profile);
                    self.grid.refresh_retained_scalars(
                        terrain.iter().map(|cell| ((cell.rx, cell.ry), cell.level)),
                        self.profile.units,
                    );
                    self.grid.set_alternate_rgb(state.alternate_rgb());
                    self.scenario = Some(*state);
                }
                LightingEvent::RelightProfile { cell_profile } => {
                    self.commit_source_cells(terrain, &mut dirty);
                    if let Some(state) = &self.scenario {
                        self.profile = scenario_profile(state, *cell_profile);
                    }
                }
            }
        }
        self.commit_source_cells(terrain, &mut dirty);
    }

    fn commit_source_cells(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        dirty: &mut BTreeSet<(u16, u16)>,
    ) {
        if dirty.is_empty() {
            return;
        }
        let heights = std::mem::take(dirty)
            .into_iter()
            .filter_map(|cell| terrain.cell(cell.0, cell.1).map(|data| (cell, data.level)));
        let sources = self
            .buildings
            .values()
            .filter(|_| self.detail_level >= 2)
            .chain(self.radiation.values())
            .cloned()
            .collect();
        // Active retail wrappers pass mode0 to554AF0. Reuse the sampling
        // primitive, but finish every affected cell now: no app-frame budget.
        let mut update = lighting::DeferredCellLightRefresh::new_with_profile(
            heights,
            self.profile,
            self.detail_level,
            sources,
        );
        update.gather_all();
        assert!(update.commit_into(&mut self.grid));
    }

    /// Reconcile explicit tool/fixture mutations and the detail option. Normal
    /// production frames first replay their ordered events via apply_events.
    pub(crate) fn refresh(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        sim: &Simulation,
        rules: &RuleSet,
        detail_level: u32,
    ) {
        if self.detail_level != detail_level.min(2) {
            // Existing detail-change full reconstruction is retained as a
            // compatibility boundary; no claim to native option-history parity.
            self.install(
                CellLightGrid::new(),
                self.config.clone(),
                detail_level,
                Some((terrain, sim, rules)),
            );
            return;
        }
        let (buildings, radiation) = source_maps(sim, rules);
        let mut events = Vec::new();
        for id in self
            .buildings
            .keys()
            .chain(buildings.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if self.buildings.get(&id) != buildings.get(&id) {
                events.push(LightingEvent::Building {
                    id,
                    source: buildings.get(&id).cloned(),
                });
            }
        }
        for center in self
            .radiation
            .keys()
            .chain(radiation.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if self.radiation.get(&center) != radiation.get(&center) {
                events.push(LightingEvent::Radiation {
                    center,
                    source: radiation.get(&center).cloned(),
                });
            }
        }
        let cell_profile = sim.lighting_cell_profile();
        if self.scenario != Some(sim.session.lighting) {
            events.push(LightingEvent::Global {
                state: sim.session.lighting,
                cell_profile,
            });
        } else if self.profile != scenario_profile(&sim.session.lighting, cell_profile) {
            events.push(LightingEvent::RelightProfile { cell_profile });
        }
        self.apply_events(terrain, &events);
    }
}

fn queue_area(
    source: Option<&PointLight>,
    terrain: &ResolvedTerrainGrid,
    detail: u32,
    dirty: &mut BTreeSet<(u16, u16)>,
) {
    let Some(source) = source.filter(|source| source.active && source.detail && detail >= 2) else {
        return;
    };
    dirty.extend(
        lighting::point_light_area_cells(source, terrain.width(), terrain.height(), |x, y| {
            terrain.cell(x, y).map(|cell| cell.level)
        })
        .into_iter()
        .map(|(cell, _)| cell),
    );
}

fn source_maps(
    sim: &Simulation,
    rules: &RuleSet,
) -> (BTreeMap<u64, PointLight>, BTreeMap<(u16, u16), PointLight>) {
    (
        sim.lighting_sources.buildings.clone(),
        sim.radiation
            .sites()
            .filter_map(|site| {
                crate::sim::radiation_light::radiation_site_light(site, &rules.radiation)
                    .map(|source| (site.center, source))
            })
            .collect(),
    )
}

/// The cells' sampling profile: the current ambient, the ordinary RGB, the
/// Ground/Level of `cell_profile`, and the Level a full relight's top scalar
/// reads ([`ScenarioLightingState::relight_top_level`]).
fn scenario_profile(
    state: &ScenarioLightingState,
    cell_profile: crate::sim::scenario_session::ScenarioLightingProfile,
) -> CellRelightProfile {
    let cell = state.profile(cell_profile);
    CellRelightProfile {
        units: LightingProfileUnits {
            ambient_percent: state.current_ambient,
            red_percent: state.normal.red_percent,
            green_percent: state.normal.green_percent,
            blue_percent: state.normal.blue_percent,
            ground_units: cell.ground_units,
            level_units: cell.level_units,
        },
        top_level_units: state.relight_top_level(cell_profile),
    }
}

/// Fully-derived render-facing lighting view. The simulation owns only the
/// scenario controller and source inputs; the per-cell grid remains app state.
#[derive(Debug, PartialEq)]
pub(crate) struct DerivedLightingView {
    pub(crate) profile: CellRelightProfile,
    pub(crate) alternate_rgb: Option<[i32; 3]>,
    pub(crate) point_lights: Vec<PointLight>,
    pub(crate) detail_level: u32,
}

/// Derive the complete visible lighting input from one committed world view.
pub(crate) fn derive_lighting_view(
    lighting_config: &LightingConfig,
    simulation: Option<&Simulation>,
    rules: Option<&RuleSet>,
    detail_level: u32,
) -> DerivedLightingView {
    let profile = simulation.map_or_else(
        || lighting::normal_profile_units(lighting_config).into(),
        |sim| scenario_profile(&sim.session.lighting, sim.lighting_cell_profile()),
    );
    let alternate_rgb = simulation.and_then(|sim| sim.session.lighting.alternate_rgb());

    let building_lights = collect_live_building_lights(simulation, detail_level);
    let radiation_lights = match (simulation, rules) {
        (Some(sim), Some(rules)) => {
            crate::app::presentation::radiation_light::collect_radiation_lights(sim, rules)
        }
        _ => Vec::new(),
    };

    let mut point_lights = Vec::with_capacity(building_lights.len() + radiation_lights.len());
    point_lights.extend(building_lights);
    point_lights.extend(radiation_lights);

    DerivedLightingView {
        alternate_rgb,
        profile,
        point_lights,
        detail_level: detail_level.min(2),
    }
}

/// Build the cell grid for an already-derived complete view.
pub(crate) fn build_lighting_grid_from_view(
    resolved_terrain: &ResolvedTerrainGrid,
    view: &DerivedLightingView,
) -> CellLightGrid {
    let mut grid = lighting::build_cell_light_grid_from_heights_and_units_with_detail(
        resolved_terrain
            .iter()
            .map(|cell| ((cell.rx, cell.ry), cell.level)),
        view.profile,
        view.detail_level,
    );
    lighting::accumulate_point_lights(&mut grid, &view.point_lights);
    grid.set_alternate_rgb(view.alternate_rgb);
    grid
}

/// Rebuild transient app lighting from the selected scenario profile plus the
/// current live building and radiation sources.
pub(crate) fn rebuild_lighting_grid_from_sim(
    resolved_terrain: &ResolvedTerrainGrid,
    lighting_config: &LightingConfig,
    simulation: Option<&Simulation>,
    rules: Option<&RuleSet>,
    detail_level: u32,
) -> CellLightGrid {
    let view = derive_lighting_view(lighting_config, simulation, rules, detail_level);
    build_lighting_grid_from_view(resolved_terrain, &view)
}

fn collect_live_building_lights(
    simulation: Option<&Simulation>,
    detail_level: u32,
) -> Vec<PointLight> {
    let Some(sim) = simulation else {
        return Vec::new();
    };
    sim.lighting_sources
        .buildings
        .values()
        .filter(|source| source.active && source.detail && detail_level >= 2)
        .cloned()
        .collect()
}

#[cfg(test)]
mod palette_producer_tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::scenario_session::ScenarioLightingState;

    fn split_cell() -> CellLightGrid {
        let mut grid = CellLightGrid::new();
        let id = grid.profiles().default_profile_id();
        grid.insert_light(
            (4, 7),
            lighting::CellLight::new(
                id,
                [512, 640, 768],
                [512, 640, 768],
                65536,
                0,
                0,
                1200,
                800,
                1200,
                900,
                800,
            ),
        );
        grid
    }

    #[test]
    fn techno_selected_scheme_does_not_inherit_cell_hue_or_row_count() {
        let grid = split_cell();
        let scenario = ScenarioLightingState::default();
        let unit = body_palette_light(&grid, &scenario, (4, 7), EntityCategory::Unit, 100, 200);
        let infantry =
            body_palette_light(&grid, &scenario, (4, 7), EntityCategory::Infantry, 100, 200);
        let cell = crate::render::palette_light::PaletteLight::cell(&grid, (4, 7), false);
        assert_eq!((unit.rows(), unit.brightness()), (53, 1300));
        assert_eq!((infantry.rows(), infantry.brightness()), (53, 1400));
        assert_eq!((cell.rows(), cell.brightness()), (27, 900));
        assert_eq!(unit.0[0] & 0x3ffff, 65535);
        assert_ne!(unit.0[0] & 0x3ffff, cell.0[0] & 0x3ffff);
    }

    #[test]
    fn palette_producers_preserve_normalized_common_from_real_map_grid() {
        let profile = lighting::parse_lighting_profiles(&IniFile::from_str(
            "[Lighting]\nAmbient=1\nGround=0\nLevel=0\nRed=.24\nGreen=.48\nBlue=.80\n",
        ))
        .normal;
        let grid = lighting::build_cell_light_grid_from_heights_and_units([((4, 7), 0)], profile);
        let light = grid.cell_light_at((4, 7)).unwrap();
        // Original 4845A2/5558E0/555AC0 fixture for this RGB triple.
        assert_eq!(light.rgb_key, [288, 576, 992]);
        assert_eq!((light.top_scalar, light.common_scalar), (1000, 799));
        let cell = crate::render::palette_light::PaletteLight::cell(&grid, (4, 7), false);
        let unit = body_palette_light(
            &grid,
            &ScenarioLightingState::default(),
            (4, 7),
            EntityCategory::Unit,
            200,
            200,
        );
        assert_eq!((cell.rows(), cell.brightness()), (27, 799));
        assert_eq!((unit.rows(), unit.brightness()), (53, 1200));
        assert_eq!(unit.0[0] & 0x3ffff, 65535);
    }

    #[test]
    fn building_art_selects_cell_palette_or_top_plus_signed_extra() {
        let grid = split_cell();
        let scenario = ScenarioLightingState::default();
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[BODY]\nExtraLight=350\n[ISO]\nTerrainPalette=yes\nExtraLight=350\n[NEG]\nExtraLight=65535\n",
        ));
        let body = art.resolve_metadata_entry("BODY", "BODY");
        assert_eq!(
            building_palette_light(&grid, &scenario, (4, 7), body, false).brightness(),
            1550
        );
        assert_eq!(
            building_palette_light(&grid, &scenario, (4, 7), body, true).brightness(),
            1200
        );
        let iso = building_palette_light(
            &grid,
            &scenario,
            (4, 7),
            art.resolve_metadata_entry("ISO", "ISO"),
            false,
        );
        assert_eq!((iso.rows(), iso.brightness()), (27, 900));
        // Building VXL uses the independent selected scheme/top producer even
        // when the SHP body above uses TerrainPalette or ExtraLight.
        let voxel = body_palette_light(
            &grid,
            &scenario,
            (4, 7),
            EntityCategory::Structure,
            100,
            200,
        );
        assert_eq!((voxel.rows(), voxel.brightness()), (53, 1200));
        assert_eq!(voxel.0[0] & 0x3ffff, 65535);
        assert_eq!(
            building_palette_light(
                &grid,
                &scenario,
                (4, 7),
                art.resolve_metadata_entry("NEG", "NEG"),
                false
            )
            .brightness(),
            1199
        );
    }

    #[test]
    fn animation_explicit_cell_plain_global_and_first_scheme_keep_distinct_rows() {
        let grid = split_cell();
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[PLAIN]\nUseNormalLight=no\n[ALT]\nAltPalette=yes\n[FULL]\nUseNormalLight=yes\n",
        ));
        let plain =
            anim_palette_light(&grid, None, (4, 7), art.anim_runtime_config("PLAIN"), false);
        let cell = anim_palette_light(&grid, None, (4, 7), art.anim_runtime_config("PLAIN"), true);
        let alt = anim_palette_light(&grid, None, (4, 7), art.anim_runtime_config("ALT"), false);
        assert_eq!(
            (plain.rows(), plain.brightness(), plain.0[1] & (1 << 30)),
            (53, 1200, 1 << 30)
        );
        assert_eq!((cell.rows(), cell.brightness()), (27, 900));
        assert_eq!((alt.rows(), alt.brightness()), (1, 1200));
        assert_eq!(
            anim_palette_light(&grid, None, (4, 7), art.anim_runtime_config("FULL"), true)
                .brightness(),
            1000
        );
    }
}
