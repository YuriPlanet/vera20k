//! Per-match lighting lifetime and ordered publication before drawing.
//! Simulation supplies source/global events; this owner retains cell sampling
//! history and publishes each affected area across install, restore and refresh.
use crate::map::lighting::{
    self, CellLightGrid, CellRelightProfile, LightingConfig, LightingProfileUnits, PointLight,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// An ordinary Techno draw's light: the intensity of the cell under it (its
/// `+0x10A` word, the top scalar) plus `extra`, the class's
/// [`body_extra_light`], through a ColorScheme Convert selected
/// independently of the cell scalar: 00705D70 -> 0070720E; Init_Theater
/// 00534D77 gives schemes neutral RGB. Ion propagation 0053C280 -> 0053AD00
/// substitutes global RGB. Trigger-driven palette rebuild history remains
/// unresolved.
pub(crate) fn body_palette_light(
    grid: &CellLightGrid,
    scenario: &crate::sim::scenario_session::ScenarioLightingState,
    cell: (u16, u16),
    extra: i32,
) -> crate::render::palette_light::PaletteLight {
    let top = grid.cell_light_at(cell).map_or(1000, |l| l.top_scalar);
    crate::render::palette_light::PaletteLight::color_scheme(
        color_scheme_rgb(Some(scenario)),
        top.wrapping_add(extra),
    )
}

/// The light a Techno's body draw adds to its cell's intensity:
/// ExtraUnitLight= for a unit (UnitClass::DrawIt `0x0073D0C3`),
/// ExtraInfantryLight= for infantry (`0x00518F90`), [`aircraft_extra_light`]
/// for an aircraft, and none for a building's voxel turret.
///
/// RESIDUAL (YuriPlanet/vera20k#1215): UnitClass::DrawIt's arms ahead of
/// its extra. On a bridge it adds four of the lighting profile's Level
/// (`0x0073CFA7..0x0073D073`), and on a cell whose `+0x140` holds 0x10000 a
/// unit that is not high-flying takes 500 off (`0x0073D078..0x0073D0BB`).
/// Trigger: a vehicle on a bridge deck or on such a cell, at every crossing.
/// Effect: VERA draws it four Levels darker (128 at Level=.032) or 500
/// brighter. Downstream: presentation only.
pub(crate) fn body_extra_light(
    entity: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
    sim: &Simulation,
    terrain: Option<&ResolvedTerrainGrid>,
) -> i32 {
    use crate::map::entities::EntityCategory;
    match entity.category() {
        EntityCategory::Unit => rules.general.extra_unit_light,
        EntityCategory::Infantry => rules.general.extra_infantry_light,
        EntityCategory::Aircraft => aircraft_extra_light(
            rules.general.extra_aircraft_light,
            crate::sim::movement::air_movement::current_fly_height(entity, terrain),
            crate::sim::superweapon::lightning_storm::raging(sim),
            &sim.session.lighting,
        ),
        _ => 0,
    }
}

/// What AircraftClass::Draw_It (`0x004148D1..0x0041493C`) adds to the
/// intensity of the cell under the aircraft's Location before its voxel
/// draw (FootClass::Draw_A_VXL `0x004DAF10` -> TechnoClass::Draw
/// `0x00706640`): ExtraAircraftLight= (Rules `+0x17DC`) and a Level for each
/// two levels of `height` (GetHeight `0x005F5F40`), the quotient truncated
/// toward zero. The Level is IonLevel= (Scenario `+0x355C`) while a
/// lightning storm rages (`LightningStorm::IsActive @ 0x0053A100`), else
/// Level= (`+0x3544`). The level step (`[0x00889EC8]`) is the 104 its static
/// initializer `0x00413B90` leaves. The sums and the product wrap at 32
/// bits. Executed: `tools/superweapon_oracle.py` `aircraft_light`.
pub(crate) fn aircraft_extra_light(
    extra_aircraft_light: i32,
    height: i32,
    storm: bool,
    scenario: &crate::sim::scenario_session::ScenarioLightingState,
) -> i32 {
    let level = if storm {
        scenario.ion.level_units
    } else {
        scenario.normal.level_units
    };
    let levels = height / (2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS);
    extra_aircraft_light.wrapping_add(levels.wrapping_mul(level))
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

/// TechnoClass's flash and curtain arms on a draw's intensity. The signed
/// flash countdown's bit 1 selects the flash at vt+0x464: TechnoClass
/// `0x0070D190` chooses 2000/500; BuildingClass `0x00456F80` adds/subtracts
/// 500. Both test the incoming intensity against 1500. Original executed
/// draw controls, including their ordering, are in
/// `tools/superweapon_oracle.json` and its metadata sidecar.
/// While time is left on
/// the curtain (`IsIronCurtained @ 0x0041BF40`, vt+0x160), GetEffectTintIntensity
/// (`0x0070E360`) scales it before the blit picks its LightConvert row.
/// UnitClass::DrawVoxelBody (`0x0073BF9C..0x0073BFB8`), TechnoClass::DrawSHP
/// (`0x0070631F..0x00706389`: building bodies, bibs and buildup, SHP
/// vehicles) and TechnoClass::Draw (`0x0070678D..0x007067E2`: voxel
/// aircraft, and a building's voxel turret and barrel) apply it. A draw
/// reads the frame Main_Tick
/// renders under; its render (`0x0055DBBE`) precedes the logic
/// (`0x0055DC9E`) and the increment (`0x0055DE81`), so that is `sim`'s
/// committed `binary_frame`. VERA's compatibility tint, for translucent and
/// effect draws, is linear in the intensity and takes the same ratio.
///
/// RESIDUAL: DrawSHP and TechnoClass::Draw also scale a building an airstrike
/// aims at that is off
/// the curtain (`0x0070633E..0x00706375`), with the airstrike's tint
/// (`sim/superweapon/invulnerability.rs`).
pub(crate) fn techno_effect_light(
    entity: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    sim: &Simulation,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    techno_effect_light_at(entity, tint, light, sim.session.binary_frame)
}

/// The light BuildingClass::UpdateAnimation gives a building's slot anims
/// whose type has ShouldUseCellDrawer= (`0x00450A47..0x00450A77`): the cell's
/// intensity through the building's arm (`0x00456FB0`: the flash arm, then
/// [`techno_effect_light`]'s) and cut to 16 bits (`0x00450A69`), which AnimClass::
/// DrawIt then draws them at unless their type has UseNormalLight=
/// (`0x004232A3..0x004232C5`). The update runs in the logic before the draw,
/// at the frame before `sim`'s committed one, and ahead of the frame's
/// UpdateIronTint (BuildingClass::Update `0x0043FE22`, TechnoClass::AI
/// `0x0043FE56`).
///
/// RESIDUAL: VERA reads the tint stage and timer UpdateIronTint left at that
/// frame, where native reads the ones before it. The stages' scales meet at
/// each boundary (`InvulnerabilityState::effect_tint_intensity`'s oracle
/// rows) but stage 3's start, ten frames into the curtain, whose
/// `RandomRanged(-5, 5)` offset moves its first scale off 512: there VERA
/// draws the anims for one frame at 396..627/256 where native draws 512/256.
/// Above a light of 2000 the cap the scaled stages apply also moves by that
/// frame at stage 1's start and stage 10's. The event relights that call
/// GetEffectTintIntensity too
/// (CreateAnimForSlot `0x00451AEE`, UpdateAnimLighting `0x00452073`, Flash
/// `0x00456E8C`, placement `0x0043FA12`, ChangeOwner `0x00448E10`) hold
/// until the next update, at most a frame.
pub(crate) fn building_anim_light(
    building: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    sim: &Simulation,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    let (tint, light) = techno_effect_light_at(
        building,
        tint,
        light,
        sim.session.binary_frame.wrapping_sub(1),
    );
    (tint, light.with_brightness(light.brightness() & 0xFFFF))
}

fn techno_effect_light_at(
    entity: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    frame: u32,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    let intensity = light.brightness();
    let flashed = if !entity.flashing.flashed() {
        intensity
    } else if entity.category() == crate::map::entities::EntityCategory::Structure {
        if intensity <= 1500 {
            intensity.wrapping_add(500)
        } else {
            intensity.wrapping_sub(500)
        }
    } else if intensity <= 1500 {
        2000
    } else {
        500
    };
    let curtain = entity.invulnerability.as_ref().filter(|curtain| {
        crate::sim::superweapon::invulnerability::is_invulnerable(Some(curtain), frame)
    });
    let tinted = curtain.map_or(flashed, |curtain| {
        curtain.effect_tint_intensity(flashed, frame as i32)
    });
    with_intensity(tint, light, tinted)
}

/// UnitClass's DrawVoxelBody after its curtain arm (`0x0073C143..0x0073C158`):
/// a deactivated Unit (`+0x1C8`, `sim::powered_unit`) draws at half the
/// intensity, a signed division toward zero.
pub(crate) fn deactivated_unit_light(
    entity: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    if entity.deactivated {
        with_intensity(tint, light, light.brightness() / 2)
    } else {
        (tint, light)
    }
}

/// `light` at `intensity`, with `tint` scaled by the same ratio.
fn with_intensity(
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    intensity: i32,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    let current = light.brightness();
    let ratio = if current == 0 {
        1.0
    } else {
        intensity as f32 / current as f32
    };
    (
        tint.map(|channel| channel * ratio),
        light.with_brightness(intensity),
    )
}

/// The colour word a building's draws hand their blits, as
/// BuildingClass_DrawBody (`0x0043D386..0x0043D544`), BuildingClass::Draw
/// (`0x0043DC1C..0x0043DDF1`) and AnimClass::DrawIt for the building at a
/// slot anim's cell (`0x004233EE..0x00423630`) compute it: the
/// `ForceShieldColor=` `[ColorAdd]` word while time is left on a Force
/// Shield (`IsIronCurtained`, and IronCurtain's byte `+0x1C4` at 1), none
/// under the Iron Curtain, and none when the local map shrouds the cell the
/// draw tests (`0x00487950`; `shrouded`, asked only of a shielded building).
/// Which blits OR it into their pixels is the blitter's
/// ([`crate::render::tactical_draw_plan::BlitPolicy::ors_colour_word`]).
///
/// RESIDUAL: an airstrike's target (`+0x294`) ORs the `LaserTargetColor=`
/// word (retail HighRed) the same way; it comes with the airstrike.
pub(crate) fn building_colour_word(
    building: &crate::sim::game_entity::GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
    shrouded: impl FnOnce() -> bool,
) -> u16 {
    let shielded = building.invulnerability.as_ref().is_some_and(|curtain| {
        curtain.kind == crate::sim::superweapon::invulnerability::InvulnKind::ForceShield
            && crate::sim::superweapon::invulnerability::is_invulnerable(
                Some(curtain),
                sim.session.binary_frame,
            )
    });
    if !shielded || shrouded() {
        return 0;
    }
    rules
        .color_add
        .rgb565_word(rules.general.force_shield_color)
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
    pub(crate) fn saved_config(&self) -> &LightingConfig {
        &self.config
    }

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
            crate::sim::radiation_light::collect_radiation_lights(sim, rules)
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
#[path = "curtain_tint_tests.rs"]
mod curtain_tint_tests;

#[cfg(test)]
mod palette_producer_tests {
    use super::*;
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
        let unit = body_palette_light(&grid, &scenario, (4, 7), 100);
        let infantry = body_palette_light(&grid, &scenario, (4, 7), 200);
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
        let unit = body_palette_light(&grid, &ScenarioLightingState::default(), (4, 7), 200);
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
        let voxel = body_palette_light(&grid, &scenario, (4, 7), 0);
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

    /// Each class's draw adds its own extra light, so no class's key reaches
    /// another's draw: ExtraUnitLight= for a unit, ExtraInfantryLight= for
    /// infantry, ExtraAircraftLight= and the height's Levels for an aircraft
    /// (IonLevel= while a storm rages), and none for a building.
    #[test]
    fn each_class_adds_its_own_extra_light() {
        use crate::map::entities::EntityCategory;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nFixtureOnly=1\n[AudioVisual]\nExtraUnitLight=.1\nExtraInfantryLight=.2\n\
             ExtraAircraftLight=.3\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.session.lighting.normal.level_units = 32;
        sim.session.lighting.ion.level_units = 20;
        let extra = |sim: &Simulation, category| {
            let mut entity = crate::sim::game_entity::GameEntity::test_default_of_category(
                1,
                "HTNK",
                "Americans",
                10,
                10,
                category,
            );
            entity.position.exact_z_leptons = Some(1500);
            body_extra_light(&entity, &rules, sim, None)
        };
        assert_eq!(extra(&sim, EntityCategory::Unit), 100);
        assert_eq!(extra(&sim, EntityCategory::Infantry), 200);
        assert_eq!(extra(&sim, EntityCategory::Aircraft), 300 + 7 * 32);
        assert_eq!(extra(&sim, EntityCategory::Structure), 0);
        let owner = sim.interner.intern("Americans");
        sim.lightning_storm =
            crate::sim::superweapon::lightning_storm::LightningStorm::raging_for_test(
                owner,
                (0, 0),
            );
        assert_eq!(extra(&sim, EntityCategory::Aircraft), 300 + 7 * 20);
    }

    /// Each `aircraft_light` row (AircraftClass::Draw_It `0x004148D1..
    /// 0x0041493E` run natively): the cell's word plus [`aircraft_extra_light`]
    /// of the row's ExtraAircraftLight=, height, storm, Level= and IonLevel=
    /// is the intensity the draw hands its voxel draw. The aircraft unit's
    /// initializers leave the level step at 104 under the startup and the
    /// process control words.
    #[test]
    fn the_aircraft_light_matches_native() {
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        let section = &oracle["aircraft_light"];
        let steps = section["level_height"].as_object().unwrap();
        assert_eq!(steps.len(), 2);
        for step in steps.values() {
            assert_eq!(
                step.as_i64(),
                Some(i64::from(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS))
            );
        }
        let rows = section["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 36);
        let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
        let mut scenario = ScenarioLightingState::default();
        for row in rows {
            scenario.normal.level_units = int(&row["level"]);
            scenario.ion.level_units = int(&row["ion_level"]);
            let extra = aircraft_extra_light(
                int(&row["extra"]),
                int(&row["height"]),
                row["storm"].as_bool().unwrap(),
                &scenario,
            );
            assert_eq!(
                int(&row["word"]).wrapping_add(extra),
                int(&row["light"]),
                "{row}"
            );
        }
    }
}
