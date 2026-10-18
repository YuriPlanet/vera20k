//! Per-cell radiation field service: radiation sites spread an additive
//! linear-falloff level over a square of cells, step it back down on a
//! per-site countdown, and self-delete when their lifetime expires. Foot
//! units standing on irradiated cells take periodic damage through the
//! `[Radiation] RadSiteWarhead=` warhead (applied during each Foot AI, not
//! here — this module owns the field, sites and their retained light sources).
//!
//! Depends on `rules/` ([Radiation] constants), `map/resolved_terrain`
//! (cell ground height for 3D falloff distance) and `util/lepton`.
//!
//! ## Float exception
//! Cell levels retain native binary64 stores. The existing deterministic
//! integer x87 subset preserves the original PC53/chop operation and store
//! boundaries in falloff, accumulation and removal. This precision changes
//! gameplay: three stock DESO fields can leave a DOG at one HP instead of
//! killing it (`tools/spatial_oracle/radiation_overlap`). Site bookkeeping is
//! integer math; distance keeps its existing owner ([`cell_distance_leptons`]).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::map::lighting::{self, LIGHT_CLAMP_MAX, LIGHT_UNIT, PointLight};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RadiationRules;
use crate::sim::light_sources::LightingEvent;
use crate::sim::timer::CdTimer;
use crate::util::lepton::{UnsupportedGroundSlope, ground_height_leptons};
use crate::util::native_x87::sqrt_approx_length;
use crate::util::native_x87::{
    MaskedX87Chop53 as X87, MaskedX87Ordering, MaskedX87Value, NativeF64Bits,
};

/// Leptons per cell edge (cell-center to cell-center step).
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;

/// Fallback square bound when no terrain grid is loaded (headless tests):
/// the engine-native full grid extent.
const FALLBACK_BOUNDS: (u16, u16) = (512, 512);

/// One radiation site, keyed by its center cell. A second detonation on the
/// same center MERGES into the existing site (level/duration reset); different
/// centers stack additively per cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadSite {
    /// Center cell.
    pub center: (u16, u16),
    /// Spread in whole cells (truncated warhead CellSpread); the affected
    /// square is (2·spread+1)² around the center. Fixed at creation — a merge
    /// does NOT update it even if the new weapon's warhead differs.
    pub spread: i32,
    /// Falloff radius in leptons = spread × 256 + 128.
    pub radius_leptons: i32,
    /// Site level at the last (re)activation. Constant between activations;
    /// the decay step recomputes per-cell falloff from this value.
    pub level: i32,
    /// Number of decay steps over the lifetime = duration / level_delay
    /// (integer division — 0 is possible for short-lived sites and makes the
    /// per-step amount unbounded; zero-over-zero cells retain a masked NaN).
    pub level_steps: i32,
    /// Total lifetime in frames = duration_multiple × level.
    pub duration: i32,
    /// Frames left. Decremented once per tick; the site self-deletes when it
    /// drops below 1 (leaving residual cell levels in place — only the center
    /// marker is released).
    pub remaining: i32,
    /// The level-decay countdown (`RadSiteClass+0x28`/`+0x30`), restarted
    /// with `RadLevelDelay=` at each activation and decay step
    /// (`RadSiteClass::AI @ 0x0065B80E`).
    pub level_timer: CdTimer,
    /// Independent source clock and retained output, owned by this site.
    /// A before-AI activation decrements lifetime on its creation frame but
    /// does not advance this timer. Neither rendering nor load restarts it.
    light: RadiationLight,
}

impl RadSite {
    /// Current source parameters, without deriving age from the lifetime.
    /// Callers receive a value; the site remains the sole mutation owner.
    pub fn light_source(&self) -> Option<PointLight> {
        self.light.source.clone()
    }

    fn activate_light(&mut self, frame: u32, theater: &str, rules: &RadiationRules) -> bool {
        let light = &mut self.light;
        light.timer = CdTimer::started(frame as i32, rules.light_delay);
        if self.duration < 1 {
            return light.source.take().is_some();
        }
        // Retain the rules owner's SimFixed factors. Wide integer scaling
        // truncates toward zero without float work in a simulation update.
        // Native uses doubles; arbitrary factor quantization remains outside
        // the stock source controls in radiation_light_lifecycle.json.
        let scaled = |value: i32, factor: crate::util::fixed_math::SimFixed| {
            (i64::from(value) * i64::from(factor.to_bits()) / 65_536)
                .clamp(i64::from(i32::MIN), i64::from(LIGHT_CLAMP_MAX)) as i32
        };
        let intensity = scaled(self.level, rules.light_factor);
        // RadSite65B5EA..65B684: Scenario+1258 == SNOW1 substitutes signed
        // -128 for red/blue before the signed channel normalization. Theater
        // comes from ScenarioSession; Rules still owns the configured bytes.
        // Original activation/refresh/decay and Snow Cell/TMP controls:
        // tools/spatial_oracle/radiation_snow_light_lifecycle.json.
        let channels = if theater.eq_ignore_ascii_case("SNOW") {
            [-128, i32::from(rules.color.1), -128]
        } else {
            [rules.color.0, rules.color.1, rules.color.2].map(i32::from)
        };
        light.base_tint =
            channels.map(|channel| scaled(channel * LIGHT_UNIT / 255, rules.tint_factor));
        // The selected retail levels have at least one light step. Preserve
        // the previous defensive handling of shorter custom lifetimes; this
        // does not claim native divide-fault equivalence for that domain.
        light.decrement = intensity / (self.duration / rules.light_delay.max(1)).max(1);
        match &mut light.source {
            Some(source) => source.update_parameters(intensity, light.base_tint),
            None => {
                light.source = Some(lighting::radiation_point_light(
                    self.center.0,
                    self.center.1,
                    self.radius_leptons,
                    intensity,
                    light.base_tint,
                ));
                true
            }
        }
    }

    pub(crate) fn hash_into(&self, hasher: &mut impl Hasher) {
        self.center.hash(hasher);
        self.spread.hash(hasher);
        self.radius_leptons.hash(hasher);
        self.level.hash(hasher);
        self.level_steps.hash(hasher);
        self.duration.hash(hasher);
        self.remaining.hash(hasher);
        self.level_timer.start_frame().hash(hasher);
        self.level_timer.duration().hash(hasher);
        self.light.hash(hasher);
    }
}

/// RadSite65B580/65B800 and its retained LightSource. These integer outputs
/// are independent of viewer/detail settings and persist with the site.
#[derive(Debug, Clone, Default, Hash, Serialize, Deserialize)]
struct RadiationLight {
    timer: CdTimer,
    base_tint: [i32; 3],
    decrement: i32,
    source: Option<PointLight>,
}

impl RadiationLight {
    fn advance(
        &mut self,
        frame: u32,
        remaining: i32,
        duration: i32,
        rules: &RadiationRules,
    ) -> bool {
        if !self.timer.expired(frame as i32) {
            return false;
        }
        self.timer.start(frame as i32, rules.light_delay);
        let Some(source) = &mut self.source else {
            return false;
        };
        //65B871..65B8A4: remaining was already decremented by this AI.
        source.update_parameters(
            source.intensity.wrapping_sub(self.decrement),
            fade_rad_tint(self.base_tint, remaining, duration),
        )
    }
}

///65B874..65B898: three low-dword IMULs followed by signed IDIV. A wide
/// product changes long-lived stacked sources; native overflow/sign controls
/// in radiation_light_lifecycle.json exercise this exact shared operation.
fn fade_rad_tint(base: [i32; 3], remaining: i32, duration: i32) -> [i32; 3] {
    base.map(|channel| channel.wrapping_mul(remaining) / duration)
}

/// The per-cell radiation field + site registry. Persisted and state-hashed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RadiationState {
    /// Sparse per-cell radiation level. Entries are removed when they decay
    /// to exactly 0 (the subtraction clamps negative results to 0).
    cells: BTreeMap<(u16, u16), f64>,
    /// One site per center cell. For canonical real Cells, membership is the
    /// Cell487C70/487C80 center marker; deletion clears it while residual F0
    /// levels remain. This does not model arbitrary off-map/Dummy pointers.
    sites: BTreeMap<(u16, u16), RadSite>,
    /// RadSiteClass ctor65B27E appends to the process vector; Logic55B5D2
    /// visits it backwards. A merge retains its slot, and dtor65B36A removes
    /// it without reordering survivors. This order affects rounded subtraction.
    creation_order: Vec<(u16, u16)>,
}

/// A weapon detonation that emits radiation, recorded by the Bullet transaction and
/// folded into `RadiationState` the same tick.
#[derive(Debug, Clone, Copy)]
pub struct RadDetonation {
    pub rx: u16,
    pub ry: u16,
    /// Weapon `RadLevel=` (> 0, checked by the producer).
    pub rad_level: i32,
    /// Truncated warhead `CellSpread=` in whole cells.
    pub spread: i32,
}

/// Ground height of a cell center in leptons. Flat 0 when no terrain grid or
/// cell is loaded; unsupported raw slope bytes stay explicit.
fn cell_height_leptons(
    terrain: Option<&ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
) -> Result<i32, UnsupportedGroundSlope> {
    let Some(cell) = terrain.and_then(|t| t.cell(rx, ry)) else {
        return Ok(0);
    };
    let world_x = i32::from(rx).wrapping_mul(256).wrapping_add(128);
    let world_y = i32::from(ry).wrapping_mul(256).wrapping_add(128);
    ground_height_leptons(cell.level, cell.slope_type, world_x, world_y)
}

/// The distance a radiation site measures from its centre cell to a cell
/// (`RadSiteClass::GetRadLevelAt`, `SetCellRadLevels`,
/// `DecreaseCellRadLevels` and `DecayCellRadLevels` at `0x0065B93E`,
/// `0x0065BA6C`, `0x0065BC19`, `0x0065BDAC`): the centre's cell coordinate
/// less the cell's, as `(dx² + dz²) + dy²` through `Sqrt_Approx` and
/// `Math::ftol` ([`sqrt_approx_length`]).
fn cell_distance_leptons(
    terrain: Option<&ResolvedTerrainGrid>,
    centre: (u16, u16),
    cell: (u16, u16),
) -> Result<i32, UnsupportedGroundSlope> {
    let across = |from: u16, to: u16| (i32::from(from) - i32::from(to)) * LEPTONS_PER_CELL;
    let dz = cell_height_leptons(terrain, centre.0, centre.1)?
        - cell_height_leptons(terrain, cell.0, cell.1)?;
    Ok(sqrt_approx_length([
        across(centre.0, cell.0),
        dz,
        across(centre.1, cell.1),
    ]))
}

/// Linear falloff contribution of `site` at `cell`:
/// `(radius − dist) / radius × level` when `dist <= radius`, else 0.
fn falloff_at(
    site: &RadSite,
    terrain: Option<&ResolvedTerrainGrid>,
    cell: (u16, u16),
) -> Result<MaskedX87Value, UnsupportedGroundSlope> {
    let dist = cell_distance_leptons(terrain, site.center, cell)?;
    let radius = site.radius_leptons;
    if dist <= radius {
        // 65BAE2..65BAEA / 65BC87..65BC8F / 65BE1A..65BE22.
        // Keep the register value: removal divides it again before storing.
        Ok(X87::mul(
            X87::div(
                X87::load_i32(radius.wrapping_sub(dist)),
                X87::load_i32(radius),
            ),
            X87::load_i32(site.level),
        ))
    } else {
        Ok(X87::load_i32(0))
    }
}

/// Iterate the (2·spread+1)² square around the center, clamped to `bounds`.
fn for_each_square_cell(
    center: (u16, u16),
    spread: i32,
    bounds: (u16, u16),
    mut f: impl FnMut((u16, u16)),
) {
    let (cx, cy) = (center.0 as i32, center.1 as i32);
    for y in (cy - spread)..=(cy + spread) {
        if y < 0 || y >= bounds.1 as i32 {
            continue;
        }
        for x in (cx - spread)..=(cx + spread) {
            if x < 0 || x >= bounds.0 as i32 {
                continue;
            }
            f((x as u16, y as u16));
        }
    }
}

impl RadiationState {
    /// A native component fixture supplies retained site words without
    /// claiming to have executed a detonation or field activation.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_retained_site_for_test(
        center: (u16, u16),
        level: i32,
        duration: i32,
        remaining: i32,
    ) -> Self {
        let site = RadSite {
            center,
            spread: 0,
            radius_leptons: 128,
            level,
            level_steps: 0,
            duration,
            remaining,
            level_timer: CdTimer::default(),
            light: RadiationLight::default(),
        };
        Self {
            creation_order: vec![site.center],
            sites: BTreeMap::from([(site.center, site)]),
            cells: BTreeMap::new(),
        }
    }

    /// Raw level of one cell (un-clamped, may exceed `RadLevelMax`).
    pub fn cell_level(&self, cell: (u16, u16)) -> f64 {
        self.cells.get(&cell).copied().unwrap_or(0.0)
    }

    /// The level a cell DAMAGES as: `trunc(min(level, RadLevelMax))`. The
    /// clamp caps damage only — the stored level itself is unbounded.
    pub fn damaging_level(&self, cell: (u16, u16), level_max: i32) -> i32 {
        // Cell487CB0 keeps the stored operand for less-than or unordered;
        // otherwise it converts the cap. Math::ftol owns masked conversion.
        let level = X87::load_f64(NativeF64Bits::from_bits(self.cell_level(cell).to_bits()));
        let max = X87::load_i32(level_max);
        let clamped = match X87::compare(level, max) {
            MaskedX87Ordering::Less | MaskedX87Ordering::Unordered => level,
            MaskedX87Ordering::Equal | MaskedX87Ordering::Greater => max,
        };
        X87::ftol_i32_low_masked(clamped)
    }

    pub fn site_at(&self, center: (u16, u16)) -> Option<&RadSite> {
        self.sites.get(&center)
    }

    pub fn sites(&self) -> impl Iterator<Item = &RadSite> {
        self.sites.values()
    }

    pub(crate) fn sites_in_update_order(&self) -> impl Iterator<Item = &RadSite> {
        self.creation_order.iter().rev().map(|center| {
            self.sites
                .get(center)
                .expect("radiation order must refer to a retained site")
        })
    }

    pub(crate) fn validate_site_order(&self) -> Result<(), &'static str> {
        let mut seen = std::collections::BTreeSet::new();
        for center in &self.creation_order {
            if !seen.insert(*center) {
                return Err("duplicate radiation site in update order");
            }
            if !self
                .sites
                .get(center)
                .is_some_and(|site| site.center == *center)
            {
                return Err("radiation update order has no matching site");
            }
        }
        if seen.len() != self.sites.len() {
            return Err("radiation site is missing from update order");
        }
        Ok(())
    }

    /// Deterministic iteration over irradiated cells (sorted by coord), for
    /// state hashing and the render glow layer.
    pub fn iter_cells(&self) -> impl Iterator<Item = (&(u16, u16), &f64)> {
        self.cells.iter()
    }

    /// True when nothing is irradiated and no site is alive.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.sites.is_empty()
    }

    /// RadSite65B510, used by deployed Guard5213E7: signed wrapping integer
    /// `remaining × level / duration`, or zero for a nonpositive duration.
    /// Merge65B530 instead computes its old contribution in floating point.
    pub fn current_site_level(site: &RadSite) -> i32 {
        if site.duration < 1 {
            return 0;
        }
        site.remaining.wrapping_mul(site.level) / site.duration
    }

    fn add_cell(&mut self, cell: (u16, u16), amount: f64) {
        // Cell487CE0: FLD incoming, FADD stored, FSTP stored.
        let value = X87::store_f64_masked_chop(X87::add(
            X87::load_f64(NativeF64Bits::from_bits(amount.to_bits())),
            X87::load_f64(NativeF64Bits::from_bits(self.cell_level(cell).to_bits())),
        ));
        if value == NativeF64Bits::POSITIVE_ZERO {
            self.cells.remove(&cell);
        } else {
            self.cells.insert(cell, f64::from_bits(value.bits()));
        }
    }

    /// Subtract clamping at exactly 0 (negative results store 0; the entry is
    /// then dropped to keep the map sparse).
    fn sub_cell(&mut self, cell: (u16, u16), amount: f64) {
        // Cell487D00 compares the register result with zero after its first
        // store; only a negative result clamps. Unordered is retained.
        let difference = X87::sub(
            X87::load_f64(NativeF64Bits::from_bits(self.cell_level(cell).to_bits())),
            X87::load_f64(NativeF64Bits::from_bits(amount.to_bits())),
        );
        let value = if X87::compare(X87::load_i32(0), difference) == MaskedX87Ordering::Greater {
            NativeF64Bits::POSITIVE_ZERO
        } else {
            X87::store_f64_masked_chop(difference)
        };
        if value == NativeF64Bits::POSITIVE_ZERO {
            self.cells.remove(&cell);
        } else {
            self.cells.insert(cell, f64::from_bits(value.bits()));
        }
    }

    /// Spread the site's full falloff into the field (runs at every
    /// (re)activation — a merge re-spreads with the new level).
    fn spread_site(&mut self, center: (u16, u16), terrain: Option<&ResolvedTerrainGrid>) {
        let Some(site) = self.sites.get(&center).cloned() else {
            return;
        };
        let bounds = terrain
            .map(|t| (t.width(), t.height()))
            .unwrap_or(FALLBACK_BOUNDS);
        for_each_square_cell(site.center, site.spread, bounds, |cell| {
            if let Ok(amount) = falloff_at(&site, terrain, cell) {
                let stored = X87::store_f64_masked_chop(amount);
                self.add_cell(cell, f64::from_bits(stored.bits()));
            }
        });
    }

    /// Remove the site's OUTSTANDING contribution before a merge: per cell,
    /// `(falloff / level_steps) × (remaining/level_delay + 1)` — the per-step
    /// decay amount times the decay steps the site still had ahead of it.
    /// The division and final store retain native masked zero-step behavior;
    /// ordinary stock DESO sites have five level steps.
    fn decrease_site_levels(
        &mut self,
        center: (u16, u16),
        terrain: Option<&ResolvedTerrainGrid>,
        rules: &RadiationRules,
    ) {
        let Some(site) = self.sites.get(&center).cloned() else {
            return;
        };
        let bounds = terrain
            .map(|t| (t.width(), t.height()))
            .unwrap_or(FALLBACK_BOUNDS);
        let steps_ahead = X87::load_i32((site.remaining / rules.level_delay).wrapping_add(1));
        for_each_square_cell(site.center, site.spread, bounds, |cell| {
            let Ok(falloff) = falloff_at(&site, terrain, cell) else {
                return;
            };
            // 65BC92..65BCA3: divide and multiply the still-live falloff
            // register, then store once before the Cell subtraction call.
            let amount = X87::mul(
                X87::div(falloff, X87::load_i32(site.level_steps)),
                steps_ahead,
            );
            self.sub_cell(
                cell,
                f64::from_bits(X87::store_f64_masked_chop(amount).bits()),
            );
        });
    }

    /// (Re)arm the decay countdown and derive the per-lifetime step count.
    fn activate_site(
        &mut self,
        center: (u16, u16),
        frame: u32,
        theater: &str,
        rules: &RadiationRules,
    ) -> bool {
        if let Some(site) = self.sites.get_mut(&center) {
            site.level_steps = site.duration / rules.level_delay;
            site.level_timer = CdTimer::started(frame as i32, rules.level_delay);
            site.activate_light(frame, theater, rules)
        } else {
            false
        }
    }

    /// Fold one radiation-emitting detonation into the field: create a site
    /// on a fresh center, or merge into the existing site (current effective
    /// level + added level, lifetime reset, field re-spread with the new
    /// level after the old outstanding contribution is removed). The site's
    /// light goes to `lights`, the simulation's one ordered lighting queue.
    /// NO-DIFF (GSI-04.21) — radiation really is the only live environmental hazard,
    /// and pass 1's neighbours are dead in gamemd rather than unported.
    /// `TreeFlammability` (`Rules+0x588`) and `IsFlammable`
    /// (`TerrainTypeClass+0x2B2`) are parsed and have ZERO live readers — verified
    /// by an instruction search over all 1.16M instructions — and
    /// `TerrainClass::Catch_Fire @ 0x0071C5B0` has only a vtable DATA xref. YR has
    /// no burning-cell state at all. `Sparky=` does have a consumer
    /// (`BuildingClass::ReceiveDamage @ 0x00442230`, spawning `DamageFireTypes`
    /// anims) but no stock warhead sets it — all 29 are `Sparky=no`.
    ///
    /// RESIDUAL (GSI-04.21) — one live non-radiation hazard exists and belongs to a
    /// different owner: `AnimTypeClass Damage=` (`+0x2A8`) accumulates in
    /// `AnimClass::AI @ 0x00423AC0` and applies through `Apply_area_damage @
    /// 0x00489280`. In ordinary skirmish its only producers are building and bridge
    /// debris (`DBRIS*`, `Damage=10/20`, `Warhead=HE`), so the gap is owned by
    /// `sim/anim_class.rs`, not by this module.
    /// - Trigger: standing next to a collapsing building or bridge.
    /// - Player effect: debris that should hurt is inert.
    /// - Frequency: every structure and bridge death.
    /// - Downstream risk: it spawns damage events from the animation pass, so it
    ///   needs the same ordering care as any other non-combat damage commit.
    pub fn apply_detonation(
        &mut self,
        det: RadDetonation,
        frame: u32,
        theater: &str,
        rules: &RadiationRules,
        terrain: Option<&ResolvedTerrainGrid>,
        lights: &mut Vec<LightingEvent>,
    ) {
        let center = (det.rx, det.ry);
        let merged = self.sites.get(&center).map(|site| {
            // Merge65B534..65B541 multiplies before dividing, without the
            // signed-dword overflow of the distinct Guard query65B510.
            let effective = X87::ftol_i32_low_masked(X87::div(
                X87::mul(X87::load_i32(site.level), X87::load_i32(site.remaining)),
                X87::load_i32(site.duration),
            ));
            effective.wrapping_add(det.rad_level)
        });
        match merged {
            None => {
                let level = det.rad_level;
                let duration = rules.duration_multiple.wrapping_mul(level);
                self.sites.insert(
                    center,
                    RadSite {
                        center,
                        spread: det.spread,
                        radius_leptons: det.spread * LEPTONS_PER_CELL + 128,
                        level,
                        level_steps: 0,
                        duration,
                        remaining: duration,
                        level_timer: CdTimer::started(frame as i32, rules.level_delay),
                        light: RadiationLight::default(),
                    },
                );
                self.creation_order.push(center);
            }
            Some(merged) => {
                self.decrease_site_levels(center, terrain, rules);
                let site = self
                    .sites
                    .get_mut(&center)
                    .expect("site present in merge branch");
                site.level = merged;
                let duration = rules.duration_multiple.wrapping_mul(merged);
                site.duration = duration;
                site.remaining = duration;
            }
        }
        let light_changed = self.activate_site(center, frame, theater, rules);
        self.spread_site(center, terrain);
        // RadSite activation65B7CA/65B7E8 publishes source changes in this
        // interval, even if another change later returns to the same inputs.
        let source = self.sites.get(&center).and_then(RadSite::light_source);
        if light_changed {
            lights.push(LightingEvent::Radiation { center, source });
        }
    }

    /// Per-tick site evolution: lifetime countdown, the periodic per-cell
    /// decay step, and site self-deletion. Residual cell levels survive a
    /// site's death — only the center registration is released. Each changed
    /// site light goes to `lights` in native reverse-creation order.
    pub(crate) fn tick_decay(
        &mut self,
        frame: u32,
        rules: &RadiationRules,
        terrain: Option<&ResolvedTerrainGrid>,
        lights: &mut Vec<LightingEvent>,
    ) {
        if self.sites.is_empty() {
            return;
        }
        // Logic55B5CD..55B5E7 walks RadSiteClass's vector backwards before
        // ordinary Logic objects. Deleting the current site preserves every
        // earlier slot; cloning the identities retains that same visit order.
        let centers: Vec<_> = self.creation_order.iter().rev().copied().collect();
        for center in centers {
            // Lifetime ticks down every frame, independent of the decay timer.
            let (expired, dead) = {
                let Some(site) = self.sites.get_mut(&center) else {
                    continue;
                };
                site.remaining -= 1;
                (site.level_timer.expired(frame as i32), site.remaining < 1)
            };
            if expired {
                self.decay_step(center, terrain);
                if let Some(site) = self.sites.get_mut(&center) {
                    site.level_timer = CdTimer::started(frame as i32, rules.level_delay);
                }
            }
            if let Some(site) = self.sites.get_mut(&center)
                && site
                    .light
                    .advance(frame, site.remaining, site.duration, rules)
            {
                lights.push(LightingEvent::Radiation {
                    center,
                    source: site.light_source(),
                });
            }
            if dead {
                self.sites.remove(&center);
                self.creation_order.retain(|retained| *retained != center);
                lights.push(LightingEvent::Radiation {
                    center,
                    source: None,
                });
            }
        }
    }

    /// One decay step: subtract `falloff / level_steps` from every cell of
    /// the square (falloff recomputed from the activation-time level, so each
    /// step removes a constant amount; the subtraction clamps at 0).
    fn decay_step(&mut self, center: (u16, u16), terrain: Option<&ResolvedTerrainGrid>) {
        let Some(site) = self.sites.get(&center).cloned() else {
            return;
        };
        let bounds = terrain
            .map(|t| (t.width(), t.height()))
            .unwrap_or(FALLBACK_BOUNDS);
        for_each_square_cell(site.center, site.spread, bounds, |cell| {
            let Ok(falloff) = falloff_at(&site, terrain, cell) else {
                return;
            };
            // 65BE25..65BE32: no intermediate binary64 falloff spill.
            let amount = X87::div(falloff, X87::load_i32(site.level_steps));
            self.sub_cell(
                cell,
                f64::from_bits(X87::store_f64_masked_chop(amount).bits()),
            );
        });
    }
}

#[cfg(test)]
#[path = "radiation_native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain_cell(
        rx: u16,
        ry: u16,
        level: u8,
        slope_type: u8,
    ) -> crate::map::resolved_terrain::ResolvedTerrainCell {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            level,
            filled_clear: true,
            slope_type,
            terrain_class: Default::default(),
            accepts_smudge: true,
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    }

    #[test]
    fn gsi_04_03b_radiation_uses_signed_sloped_cell_center_heights() {
        let terrain = ResolvedTerrainGrid::from_cells(
            2,
            1,
            vec![terrain_cell(0, 0, 0xff, 0), terrain_cell(1, 0, 0, 1)],
        );
        assert_eq!(cell_height_leptons(Some(&terrain), 0, 0), Ok(-103));
        assert_eq!(cell_height_leptons(Some(&terrain), 1, 0), Ok(52));
        assert_eq!(
            cell_distance_leptons(Some(&terrain), (0, 0), (1, 0)),
            Ok(299)
        );
    }

    /// `RadSiteClass::GetRadLevelAt` (`0x0065B93E..0x0065B98F`) executed on
    /// each row's two cell coordinates (`tools/spatial_oracle/
    /// sqrt_approx_distances.py`). The terrain gives each cell the row's Z: a
    /// whole level, or 52 for a level-0 ramp centre.
    #[test]
    fn cell_distance_matches_original_rows() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/sqrt_approx_distances.json",
        ))
        .unwrap();
        let rows = corpus["radiation"].as_array().unwrap();
        for row in rows {
            let coords: [[i32; 3]; 2] = [row["centre"].clone(), row["cell"].clone()]
                .map(|v| serde_json::from_value(v).unwrap());
            let cell_of = |[x, y, _]: [i32; 3]| ((x / 256) as u16, (y / 256) as u16);
            let cells = (0..8)
                .flat_map(|ry| (0..8).map(move |rx| (rx, ry)))
                .map(
                    |(rx, ry)| match coords.iter().find(|&&coord| cell_of(coord) == (rx, ry)) {
                        Some(&[_, _, 52]) => terrain_cell(rx, ry, 0, 1),
                        Some(&[_, _, z]) => terrain_cell(rx, ry, (z / 104) as u8, 0),
                        None => terrain_cell(rx, ry, 0, 0),
                    },
                )
                .collect();
            let terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
            for coord in coords {
                let (rx, ry) = cell_of(coord);
                assert_eq!(cell_height_leptons(Some(&terrain), rx, ry), Ok(coord[2]));
            }
            assert_eq!(
                cell_distance_leptons(Some(&terrain), cell_of(coords[0]), cell_of(coords[1])),
                Ok(row["expected"].as_i64().unwrap() as i32),
                "{}",
                row["name"]
            );
        }
        assert_eq!(rows.len(), 4);
    }

    fn stock_rules() -> RadiationRules {
        RadiationRules {
            duration_multiple: 1,
            application_delay: 16,
            level_max: 500,
            level_delay: 90,
            light_delay: 90,
            level_factor: 0.2,
            light_factor: crate::util::fixed_math::sim_from_f32(0.1),
            tint_factor: crate::util::fixed_math::sim_from_f32(1.0),
            color: (0, 255, 0),
            site_warhead: "RadSite".to_string(),
        }
    }

    fn desolator_det(rx: u16, ry: u16) -> RadDetonation {
        // RadEruptionWeapon: RadLevel=500; RadEruptionWarhead: CellSpread=10.
        RadDetonation {
            rx,
            ry,
            rad_level: 500,
            spread: 10,
        }
    }

    /// Same-center re-detonation merges (effective + added), it does not
    /// stack to 2× full level.
    #[test]
    fn same_center_redetonation_merges_not_stacks() {
        let rules = stock_rules();
        let mut rad = RadiationState::default();
        rad.apply_detonation(
            desolator_det(50, 50),
            0,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        // Immediately re-detonate: effective is still the full 500 (no frames
        // elapsed), so the merged level is exactly 500 + 500.
        rad.apply_detonation(
            desolator_det(50, 50),
            0,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        let site = rad.site_at((50, 50)).expect("merged site survives");
        assert_eq!(site.level, 1000);
        assert_eq!(site.duration, 1000);
        assert_eq!(site.remaining, 1000);
        // Field: spread(500) − outstanding(500·(0/90+1 step)) + spread(1000)
        // = 1000 at the center.
        assert_eq!(rad.cell_level((50, 50)), 1000.0);
        // Exactly ONE site exists.
        assert_eq!(rad.sites().count(), 1);
    }

    /// The site counts down once per tick, steps the field down every
    /// `RadLevelDelay` frames, and self-deletes once `remaining < 1` —
    /// releasing only the center registration.
    #[test]
    fn site_self_deletes_and_clears_center_ptr() {
        let rules = stock_rules();
        let mut rad = RadiationState::default();
        rad.apply_detonation(
            desolator_det(10, 10),
            0,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        let full = rad.cell_level((10, 10));
        // duration 500, level_steps = 500/90 = 5, step every 90 frames.
        for frame in 1..=89 {
            rad.tick_decay(frame, &rules, None, &mut Vec::new());
        }
        assert_eq!(
            rad.cell_level((10, 10)),
            full,
            "no decay step before the 90-frame countdown expires"
        );
        rad.tick_decay(90, &rules, None, &mut Vec::new());
        let after_one = rad.cell_level((10, 10));
        assert_eq!(
            after_one,
            full - full / 5.0,
            "first step removes falloff/level_steps"
        );
        // Run out the rest of the lifetime.
        for frame in 91..=520 {
            rad.tick_decay(frame, &rules, None, &mut Vec::new());
        }
        assert!(
            rad.site_at((10, 10)).is_none(),
            "site self-deletes at remaining < 1"
        );
        // 5 steps of full/5 each have cleared the center cell exactly (its
        // per-step amount, 100.0, is representable). Off-center cells may
        // keep ±1-ulp float residue — the sequential double subtraction
        // leaves the same residue in the original engine — but nothing may
        // remain at a level that truncates to a damaging value.
        assert_eq!(rad.cell_level((10, 10)), 0.0);
        assert!(
            rad.iter_cells().all(|(_, &v)| v < 1e-9),
            "only sub-damaging float residue may survive the full decay"
        );
        assert_eq!(rad.sites().count(), 0);
    }

    /// The decay countdown is per-site (anchored at activation), not a global
    /// frame-modulo: a site created mid-stream steps 90 frames after ITS
    /// activation.
    #[test]
    fn decay_countdown_is_activation_anchored() {
        let rules = stock_rules();
        let mut rad = RadiationState::default();
        rad.apply_detonation(
            desolator_det(10, 10),
            37,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        let full = rad.cell_level((10, 10));
        for frame in 38..=126 {
            rad.tick_decay(frame, &rules, None, &mut Vec::new());
        }
        assert_eq!(rad.cell_level((10, 10)), full);
        rad.tick_decay(127, &rules, None, &mut Vec::new());
        assert!(rad.cell_level((10, 10)) < full);
    }

    /// Merge bookkeeping mid-life: outstanding contribution is removed with
    /// the old level before the new level is spread.
    #[test]
    fn midlife_merge_resets_to_effective_plus_added() {
        let rules = stock_rules();
        let mut rad = RadiationState::default();
        rad.apply_detonation(
            desolator_det(10, 10),
            0,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        // Advance 180 frames: two decay steps, remaining 500→320.
        for frame in 1..=180 {
            rad.tick_decay(frame, &rules, None, &mut Vec::new());
        }
        let site = rad.site_at((10, 10)).unwrap();
        assert_eq!(site.remaining, 320);
        // Effective = 320×500/500 = 320; merge adds 500 → 820.
        rad.apply_detonation(
            desolator_det(10, 10),
            180,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        let site = rad.site_at((10, 10)).unwrap();
        assert_eq!(site.level, 820);
        assert_eq!(site.duration, 820);
        assert_eq!(site.remaining, 820);
        // Field at center: 500 − 2×(500/5) [two steps] = 300, then the
        // outstanding removal (500/5)×(320/90+1) = 400 over-subtracts and
        // clamps at exactly 0 (the subtraction stores 0 on a negative
        // result), then the re-spread adds the full 820.
        assert_eq!(rad.cell_level((10, 10)), 820.0);
    }

    /// Square bounds clamp at the map edge without wrapping or panicking.
    #[test]
    fn detonation_near_origin_clamps_square() {
        let rules = stock_rules();
        let mut rad = RadiationState::default();
        rad.apply_detonation(
            desolator_det(2, 2),
            0,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        assert_eq!(rad.cell_level((2, 2)), 500.0);
        assert!(rad.cell_level((0, 0)) > 0.0);
    }

    /// Serde round-trip preserves the field and the sites bit-exactly.
    #[test]
    fn radiation_state_serde_round_trip() {
        let rules = stock_rules();
        let mut rad = RadiationState::default();
        rad.apply_detonation(
            desolator_det(10, 10),
            5,
            "TEMPERATE",
            &rules,
            None,
            &mut Vec::new(),
        );
        for frame in 6..=130 {
            rad.tick_decay(frame, &rules, None, &mut Vec::new());
        }
        let bytes = bincode::serialize(&rad).expect("serialize");
        let restored: RadiationState = bincode::deserialize(&bytes).expect("deserialize");
        let a: Vec<_> = rad.iter_cells().map(|(c, v)| (*c, v.to_bits())).collect();
        let b: Vec<_> = restored
            .iter_cells()
            .map(|(c, v)| (*c, v.to_bits()))
            .collect();
        assert_eq!(a, b);
        assert_eq!(
            restored.site_at((10, 10)).map(|s| (s.level, s.remaining)),
            rad.site_at((10, 10)).map(|s| (s.level, s.remaining)),
        );
    }
}
