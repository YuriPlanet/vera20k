//! Per-match lighting lifetime and ordered publication before drawing.
//! Simulation supplies source/global events; this owner retains cell sampling
//! history and publishes each affected area across install, restore and refresh.
use crate::map::lighting::{
    self, CellLightGrid, CellRelightProfile, LightingConfig, LightingProfileUnits, PointLight,
};
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// An ordinary Techno draw's light: the intensity of the cell under it (its
/// `+0x10A` word, the top scalar) plus `extra`, the class's
/// [`body_extra_light`], through a ColorScheme Convert selected
/// independently of the cell scalar: 00705D70 -> 0070720E; Init_Theater
/// 00534D77 gives schemes neutral RGB. Ion propagation 0053C280 -> 0053AD00
/// substitutes global RGB. The grid owns the normal/alternate Convert caches,
/// including trigger-driven retints, independently of the current cell hue.
pub(crate) fn body_palette_light(
    grid: &CellLightGrid,
    cell: (u16, u16),
    extra: i32,
) -> crate::render::palette_light::PaletteLight {
    let top = grid.cell_light_at(cell).map_or(1000, |l| l.top_scalar);
    crate::render::palette_light::PaletteLight::color_scheme(
        grid.palette_rgb(grid.profiles().default_profile_id()),
        top.wrapping_add(extra),
    )
}

/// The light a Techno's body draw adds to its cell's intensity:
/// UnitClass::DrawIt's bridge/side-cell arm and ExtraUnitLight=,
/// ExtraInfantryLight= for infantry (`0x00518F90`), [`aircraft_extra_light`]
/// for an aircraft, and none for a building's voxel turret.
///
/// The caller supplies its frame's isolated query; drawing never stamps the
/// simulation's shared fallback Cell. Native controls: tools/unit_light.json.
pub(crate) fn body_extra_light(
    entity: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
    sim: &Simulation,
    cells: Option<&NativeCellQuery<'_>>,
) -> i32 {
    use crate::map::entities::EntityCategory;
    match entity.category() {
        EntityCategory::Unit => {
            use crate::map::bridge_facts::{
                BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_EXTRA_SIDE, BRIDGE_FLAG_STRUCTURAL,
            };
            use crate::sim::movement::{air_movement, ground_pose};

            let extra = rules.general.extra_unit_light;
            let Some(cells) = cells else {
                return extra;
            };
            // Unit73CF1D retains Object41BEA0's packed cell before the
            // GetHeight73CF74 and world-coordinate lookup73CFA0. Keep the
            // two query forms distinct, including their signed narrowing.
            let [x, y] = ground_pose::position_world_xy(&entity.position);
            let packed = (
                crate::util::lepton::lepton_to_cell_packed(x),
                crate::util::lepton::lepton_to_cell_packed(y),
            );
            let height = air_movement::queried_flight_height(entity, Some(cells));
            let current = cells.lookup_world(x, y);
            let flags = cells.flags(current);
            let above_deck = if entity.on_bridge() {
                height >= 0
            } else {
                height >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
            };
            if above_deck && flags & BRIDGE_FLAG_STRUCTURAL != 0 {
                // Cell481810 chooses just one neighbor: direction0 when
                // bit800 is set, otherwise6. Failure never tries the other.
                let direction = if flags & BRIDGE_FLAG_DIRECTION_ZERO != 0 {
                    0
                } else {
                    6
                };
                let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
                let (cx, cy) = cells.coord(current);
                let neighbor =
                    cells.lookup((cx.wrapping_add(dx as i16), cy.wrapping_add(dy as i16)));
                if cells.flags(neighbor) & BRIDGE_FLAG_STRUCTURAL != 0 {
                    //73D00E..73D076 has the same Ion/Dominator/Nuke/Normal
                    //precedence as cell relighting, including Dominator fade
                    //back. This arm skips the side-cell subtraction entirely.
                    let level = sim
                        .session()
                        .lighting()
                        .profile(sim.lighting_cell_profile())
                        .level_units;
                    return extra.wrapping_add(level.wrapping_mul(4));
                }
            }
            //73D078..73D0BB asks high-flight only on a flagged side cell.
            //Unit's actual vt54/4DE620 forwards to the common Object owner.
            let draw_cell = cells.lookup(packed);
            if cells.flags(draw_cell) & BRIDGE_FLAG_EXTRA_SIDE != 0
                && !air_movement::is_high_flying_in_query(entity, cells, None)
            {
                extra.wrapping_sub(500)
            } else {
                extra
            }
        }
        EntityCategory::Infantry => rules.general.extra_infantry_light,
        EntityCategory::Aircraft => aircraft_extra_light(
            rules.general.extra_aircraft_light,
            crate::sim::movement::air_movement::queried_flight_height(entity, cells),
            crate::sim::superweapon::lightning_storm::raging(sim),
            &sim.session().lighting(),
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
        scenario.ion().level_units
    } else {
        scenario.normal().level_units
    };
    let levels = height / (2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS);
    extra_aircraft_light.wrapping_add(levels.wrapping_mul(level))
}

/// Ordinary building DrawBody 0043D812..0043D85F; buildup 0043D644;
/// TerrainPalette overrides both scalar and Convert in 00705EC7..00705F52.
pub(crate) fn building_palette_light(
    grid: &CellLightGrid,
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
    PaletteLight::color_scheme(
        grid.palette_rgb(grid.profiles().default_profile_id()),
        top.wrapping_add(extra),
    )
}

/// AnimClass DrawIt 00423280..00423354: explicit cell drawer selects cell
/// common; otherwise global ANIM Convert selects top. AltPalette selects the
/// first ColorScheme (one row), not the player's 53-row scheme.
pub(crate) fn anim_palette_light(
    grid: &CellLightGrid,
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
        // Default AltPalette uses B054D4[0], the neutral N1 ColorScheme.
        //68C710 supplies RGB1000;53AD4D skips default schemes with N<=1.
        // This differs from the cell Convert cache and N53 house schemes.
        PaletteLight::new([1000; 3], 1, brightness, true)
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
/// DrawSHP and TechnoClass::Draw also enter this arm for a building with an
/// Airstrike manager that targets it. GetEffectTintIntensity
/// applies Iron then Airstrike stages, each with its own cap.
pub(crate) fn techno_effect_light(
    entity: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    sim: &Simulation,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    techno_effect_light_at(
        entity,
        tint,
        light,
        sim.session().binary_frame(),
        sim.airstrike_tint_active(entity.stable_id()),
    )
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
pub(crate) fn building_anim_light(
    building: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    sim: &Simulation,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    let (tint, light) = if let Some(sample) = building.building_art.tint_sample() {
        effect_light(
            tint,
            light,
            TintLightState {
                building: true,
                flashed: sample.flashed(),
                curtain: sample.invulnerability(),
                airstrike: sample.airstrike(),
                airstrike_active: sample.airstrike_active(),
                frame: sample.frame(),
            },
        )
    } else {
        techno_effect_light_at(
            building,
            tint,
            light,
            sim.session().binary_frame().wrapping_sub(1),
            sim.airstrike_tint_active(building.stable_id()),
        )
    };
    (tint, light.with_brightness(light.brightness() & 0xFFFF))
}

fn techno_effect_light_at(
    entity: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    frame: u32,
    airstrike_active: bool,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    effect_light(
        tint,
        light,
        TintLightState {
            building: entity.category() == crate::map::entities::EntityCategory::Structure,
            flashed: entity.flashing.flashed(),
            curtain: entity.invulnerability.as_ref(),
            airstrike: &entity.airstrike_tint,
            airstrike_active,
            frame,
        },
    )
}

struct TintLightState<'a> {
    building: bool,
    flashed: bool,
    curtain: Option<&'a crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    airstrike: &'a crate::sim::superweapon::airstrike_tint::AirstrikeTintState,
    airstrike_active: bool,
    frame: u32,
}

fn effect_light(
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    state: TintLightState<'_>,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    let intensity = light.brightness();
    let flashed = if !state.flashed {
        intensity
    } else if state.building {
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
    let curtained =
        crate::sim::superweapon::invulnerability::is_invulnerable(state.curtain, state.frame);
    let tinted = if curtained || (state.building && state.airstrike_active) {
        let iron = state.curtain.map_or(flashed, |curtain| {
            curtain.effect_tint_intensity(flashed, state.frame as i32)
        });
        state
            .airstrike
            .effect_tint_intensity(iron, state.frame as i32)
    } else {
        flashed
    };
    with_intensity(tint, light, tinted)
}

/// UnitClass::DrawVoxelBody73BF9C..73C15F after its shared flash/curtain
/// intensity: OR IronCurtainColor while IsIronCurtained41BF40, then
/// BerserkColor while Techno+298 is active, into the caller's colour word.
/// Composite73C1A5 passes that word to the existing packed blitter owner.
/// A deactivated Unit (+1C8) then halves intensity, signed toward zero.
///
/// The voxel builder also draws Aircraft through Techno::Draw70678D, which
/// has no Unit colour arms. The Unit gate preserves that class boundary.
/// Original controls: superweapon_oracle.json::curtain_draw_arm and
/// berserk_color.json::mtnk (physical stock Image-resolved GTNK/DarkRed).
/// No simulation state, timer, RNG or detach is written here.
pub(crate) fn unit_voxel_effect_light(
    entity: &crate::sim::game_entity::GameEntity,
    tint: [f32; 3],
    light: crate::render::palette_light::PaletteLight,
    rules: Option<&RuleSet>,
    frame: u32,
) -> ([f32; 3], crate::render::palette_light::PaletteLight) {
    let mut light = light;
    if entity.category() == crate::map::entities::EntityCategory::Unit {
        let word = rules.map_or(0, |rules| {
            let curtain = if crate::sim::superweapon::invulnerability::is_invulnerable(
                entity.invulnerability.as_ref(),
                frame,
            ) {
                rules
                    .color_add
                    .rgb565_word(rules.general.iron_curtain_color)
            } else {
                0
            };
            let berserk = if entity.berserk.is_active() {
                rules.color_add.rgb565_word(rules.general.berserk_color)
            } else {
                0
            };
            curtain | berserk
        });
        light = light.with_colour_word(light.colour_word() | word);
    }
    if entity.deactivated.is_offline() {
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

/// The caller colour word for SHP bodies and a building's attached draws.
/// InfantryClass::DrawIt (`0x00518FBC..0x005190DE`) selects BerserkColor while
/// Techno+298 is active, packs the retained ColorAdd entry through
/// `0x00519008..0x0051907E`, and clears it when Cell487950 is shrouded.
/// Original raw/RLE GI controls: procedural_drawing_oracle/infantry_berserk_body.json.
/// SHP vehicles pass no word (`0x0073CE3E`, `0x0073CE7D`).
///
/// A building's draws hand their blits the word that
/// BuildingClass_DrawBody (`0x0043D386..0x0043D544`), BuildingClass::Draw
/// (`0x0043DC1C..0x0043DDF1`) and AnimClass::DrawIt for the building at a
/// slot anim's cell (`0x004233EE..0x00423630`) compute it: the
/// `ForceShieldColor=` `[ColorAdd]` word while time is left on a Force
/// Shield (`IsIronCurtained`, and IronCurtain's byte `+0x1C4` at 1); an Iron
/// Curtain contributes no colour word. Neither effect colours a shrouded cell the
/// draw tests (`0x00487950`; `shrouded`, asked only of a shielded building).
/// Which blits OR it into their pixels is the blitter's
/// ([`crate::render::tactical_draw_plan::BlitPolicy::ors_colour_word`]).
///
/// Any nonnull Airstrike pointer (`+0x294`) ORs LaserTargetColor too,
/// independently of its manager target and of the Force Shield colour.
pub(crate) fn shp_colour_word(
    entity: &crate::sim::game_entity::GameEntity,
    sim: &Simulation,
    rules: &RuleSet,
    shrouded: impl FnOnce() -> bool,
) -> u16 {
    match entity.category() {
        crate::map::entities::EntityCategory::Infantry => {
            return if entity.berserk.is_active() && !shrouded() {
                rules.color_add.rgb565_word(rules.general.berserk_color)
            } else {
                0
            };
        }
        crate::map::entities::EntityCategory::Structure => {}
        _ => return 0,
    }
    let shielded = entity.invulnerability.as_ref().is_some_and(|curtain| {
        curtain.kind() == crate::sim::superweapon::invulnerability::InvulnKind::ForceShield
            && crate::sim::superweapon::invulnerability::is_invulnerable(
                Some(curtain),
                sim.session().binary_frame(),
            )
    });
    let marked = entity.has_airstrike_marker();
    if !(shielded || marked) || shrouded() {
        return 0;
    }
    let shield = if shielded {
        rules
            .color_add
            .rgb565_word(rules.general.force_shield_color)
    } else {
        0
    };
    let laser = if marked {
        rules
            .color_add
            .rgb565_word(rules.general.laser_target_color)
    } else {
        0
    };
    shield | laser
}

use crate::sim::light_sources::{LightingEvent, PaletteUpdate};
use crate::sim::scenario_session::ScenarioLightingState;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct MatchLighting {
    grid: CellLightGrid,
    config: LightingConfig,
    /// Replica of the simulation's building light sources
    /// (`LightingSources::buildings`) as last applied to `grid`. The frame's
    /// lighting events update it in native order ([`Self::apply_events`]);
    /// [`Self::install`] and [`Self::refresh`] re-derive it from the simulation
    /// and relight only what differs.
    buildings: BTreeMap<u64, PointLight>,
    /// Replica of the simulation's radiation light sites, kept like `buildings`.
    radiation: BTreeMap<(u16, u16), PointLight>,
    profile: CellRelightProfile,
    /// Replica of the scenario lighting state (`ScenarioSession::lighting`) as
    /// last applied, kept like `buildings`.
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
        live: Option<(&ResolvedTerrainGrid, &Simulation)>,
    ) {
        *self = Self {
            grid,
            config,
            detail_level: detail_level.min(2),
            ..Self::default()
        };
        self.profile = lighting::normal_profile_units(&self.config).into();
        if let Some((terrain, sim)) = live {
            let view = derive_lighting_view(&self.config, Some(sim), detail_level);
            self.grid = build_lighting_grid_from_view(terrain, &view);
            self.profile = view.profile;
            self.scenario = Some(*sim.session().lighting());
            (self.buildings, self.radiation) = source_maps(sim);
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
                    palette,
                    constructor_rgb,
                } => {
                    self.commit_source_cells(terrain, &mut dirty);
                    self.profile = scenario_profile(state, *cell_profile);
                    match palette {
                        PaletteUpdate::Preserve => {}
                        PaletteUpdate::RestoreNormal => self.grid.set_alternate_rgb(None),
                        PaletteUpdate::Normal(rgb) => self.grid.retint_normal_rgb(*rgb),
                        PaletteUpdate::Alternate(rgb) => self.grid.set_alternate_rgb(Some(*rgb)),
                    }
                    //53AD00 updates Converts before4AE4C0 refreshes Cells.
                    self.grid.refresh_retained_scalars(
                        terrain.iter().map(|cell| ((cell.rx, cell.ry), cell.level)),
                        self.profile.units,
                    );
                    self.grid.set_constructor_alternate_rgb(*constructor_rgb);
                    self.scenario = Some(*state);
                }
                LightingEvent::RelightProfile {
                    cell_profile,
                    constructor_rgb,
                } => {
                    self.commit_source_cells(terrain, &mut dirty);
                    if let Some(state) = &self.scenario {
                        self.profile = scenario_profile(state, *cell_profile);
                    }
                    self.grid.set_constructor_alternate_rgb(*constructor_rgb);
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
        detail_level: u32,
    ) {
        if self.detail_level != detail_level.min(2) {
            // Existing detail-change full reconstruction is retained as a
            // compatibility boundary; no claim to native option-history parity.
            self.install(
                CellLightGrid::new(),
                self.config.clone(),
                detail_level,
                Some((terrain, sim)),
            );
            return;
        }
        let (buildings, radiation) = source_maps(sim);
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
        if self.scenario != Some(*sim.session().lighting()) {
            events.push(LightingEvent::Global {
                state: *sim.session().lighting(),
                cell_profile,
                // Target/timer/normal-ambient writes do not retint palettes.
                // An explicit profile selection's ordered event owns that
                // operation; retain the compatibility fallback for a newly
                // installed or externally supplied profile.
                palette: if self.scenario.as_ref().is_some_and(|previous| {
                    previous.selected_profile() == sim.session().lighting().selected_profile()
                }) {
                    PaletteUpdate::Preserve
                } else {
                    PaletteUpdate::from_scenario(&sim.session().lighting())
                },
                constructor_rgb: sim.palette_constructor_rgb(),
            });
        } else if self.profile != scenario_profile(&sim.session().lighting(), cell_profile)
            || self.grid.constructor_alternate_rgb() != sim.palette_constructor_rgb()
        {
            events.push(LightingEvent::RelightProfile {
                cell_profile,
                constructor_rgb: sim.palette_constructor_rgb(),
            });
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

fn source_maps(sim: &Simulation) -> (BTreeMap<u64, PointLight>, BTreeMap<(u16, u16), PointLight>) {
    (
        sim.lighting_sources().buildings().clone(),
        sim.radiation()
            .sites()
            .filter_map(|site| site.light_source().map(|source| (site.center(), source)))
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
            ambient_percent: state.current_ambient(),
            red_percent: state.normal().red_percent,
            green_percent: state.normal().green_percent,
            blue_percent: state.normal().blue_percent,
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
    pub(crate) constructor_rgb: Option<[i32; 3]>,
    pub(crate) point_lights: Vec<PointLight>,
    pub(crate) detail_level: u32,
}

/// Derive the complete visible lighting input from one committed world view.
pub(crate) fn derive_lighting_view(
    lighting_config: &LightingConfig,
    simulation: Option<&Simulation>,
    detail_level: u32,
) -> DerivedLightingView {
    let profile = simulation.map_or_else(
        || lighting::normal_profile_units(lighting_config).into(),
        |sim| scenario_profile(&sim.session().lighting(), sim.lighting_cell_profile()),
    );
    let alternate_rgb = simulation.and_then(|sim| sim.session().lighting().alternate_rgb());

    let building_lights = collect_live_building_lights(simulation, detail_level);
    let radiation_lights = simulation
        .map(|sim| {
            sim.radiation()
                .sites()
                .filter_map(|site| site.light_source())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut point_lights = Vec::with_capacity(building_lights.len() + radiation_lights.len());
    point_lights.extend(building_lights);
    point_lights.extend(radiation_lights);

    DerivedLightingView {
        alternate_rgb,
        constructor_rgb: simulation.and_then(Simulation::palette_constructor_rgb),
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
    grid.set_constructor_alternate_rgb(view.constructor_rgb);
    grid
}

/// Rebuild transient app lighting from the selected scenario profile plus the
/// current live building and radiation sources.
pub(crate) fn rebuild_lighting_grid_from_sim(
    resolved_terrain: &ResolvedTerrainGrid,
    lighting_config: &LightingConfig,
    simulation: Option<&Simulation>,
    detail_level: u32,
) -> CellLightGrid {
    let view = derive_lighting_view(lighting_config, simulation, detail_level);
    build_lighting_grid_from_view(resolved_terrain, &view)
}

fn collect_live_building_lights(
    simulation: Option<&Simulation>,
    detail_level: u32,
) -> Vec<PointLight> {
    let Some(sim) = simulation else {
        return Vec::new();
    };
    sim.lighting_sources()
        .buildings()
        .values()
        .filter(|source| source.active && source.detail && detail_level >= 2)
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "curtain_tint_tests.rs"]
mod curtain_tint_tests;

#[cfg(test)]
#[path = "lighting_tint_tests.rs"]
mod tint_tests;

#[cfg(test)]
#[path = "unit_light_tests.rs"]
mod unit_light_tests;

#[cfg(test)]
mod palette_producer_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::scenario_session::ScenarioLightingState;

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
        let unit = body_palette_light(&grid, (4, 7), 200);
        assert_eq!((cell.rows(), cell.brightness()), (27, 799));
        assert_eq!((unit.rows(), unit.brightness()), (53, 1200));
        assert_eq!(unit.0[0] & 0x3ffff, 65535);
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
            scenario.normal_mut_for_test().level_units = int(&row["level"]);
            scenario.ion_mut_for_test().level_units = int(&row["ion_level"]);
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
