//! Persistent YR `WaveClass` runtime state and type-0 CellClass sampling.
//!
//! Wave lifecycle is simulation authority: Sonic Waves own a live firer/target
//! relationship, mixed binary32/double fade state, and a pointer-identity cell
//! vector consumed synchronously from the mixed LogicClass order.

use std::collections::BTreeMap;
use std::hash::Hash;

use crate::map::cell_index::CELL_ROW_STRIDE;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::retail_trig::{AcosTable, TrigTable};
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
use crate::sim::combat::TargetKind;
use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value, adjust_for_z_standard,
    distance_3d_leptons, sqrt_approx_f32,
};

const STEP_F32: NativeF32Bits = NativeF32Bits::from_bits(0x3d4c_cccd);
const HALF_STEP_F32: NativeF32Bits = NativeF32Bits::from_bits(0x3ccc_cccd);
const SNAP_FADE_F32: NativeF32Bits = NativeF32Bits::from_bits(0x3f7a_e148);
const AUTO_DECAY_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
const SONIC_TARGET_Z_ADJUST: i32 = 50;
const CONSTRUCTOR_MIN_XY_DISTANCE: i64 = 240;
const MAX_TRACKING_DISTANCE_LEPTONS: i32 = 2172;

/// Stable serialization of a native CellClass pointer retained by WaveClass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WaveCellIdentity {
    Real { fixed_stride_index: u32 },
    SharedDummy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WaveRecordedCell {
    pub identity: WaveCellIdentity,
}

impl WaveRecordedCell {
    pub const fn real(rx: u16, ry: u16) -> Self {
        Self {
            identity: WaveCellIdentity::Real {
                fixed_stride_index: ry as u32 * CELL_ROW_STRIDE as u32 + rx as u32,
            },
        }
    }

    pub const fn shared_dummy() -> Self {
        Self {
            identity: WaveCellIdentity::SharedDummy,
        }
    }

    pub fn current_cell(self, terrain: Option<&ResolvedTerrainGrid>) -> WaveCellView {
        match self.identity {
            WaveCellIdentity::Real { fixed_stride_index } => {
                let rx = (fixed_stride_index % CELL_ROW_STRIDE as u32) as u16;
                let ry = (fixed_stride_index / CELL_ROW_STRIDE as u32) as u16;
                let cell = terrain.and_then(|grid| grid.cell(rx, ry));
                WaveCellView {
                    rx: i32::from(rx),
                    ry: i32::from(ry),
                    level: cell.map_or(0, |cell| cell.level as i8),
                    structural_bridge: cell
                        .is_some_and(|cell| cell.bridge_facts.has_structural_bridge()),
                    real: true,
                }
            }
            WaveCellIdentity::SharedDummy => {
                let snapshot = terrain
                    .map(|grid| grid.shared_cell_dummy().snapshot())
                    .unwrap_or(crate::map::resolved_terrain::SharedCellDummySnapshot {
                        coord: (0, 0),
                        level: 0,
                        slope_type: 0,
                        bridge_flags_0x1180: 0,
                    });
                WaveCellView {
                    rx: snapshot.coord.0,
                    ry: snapshot.coord.1,
                    level: snapshot.level,
                    structural_bridge: snapshot.bridge_flags_0x1180 & 0x100 != 0,
                    real: false,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveCellView {
    pub rx: i32,
    pub ry: i32,
    pub level: i8,
    pub structural_bridge: bool,
    pub real: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveDamageRequest {
    pub wave_id: u64,
    pub firer_id: u64,
    pub recorded_cells: Vec<WaveRecordedCell>,
    pub wave_z: i32,
}

/// Compatibility record used by direct receiver helpers. WaveClass itself no
/// longer stores one: DamageArea resolves GetWeapon(0) once per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WaveDamagePayload {
    pub firer_id: u64,
    pub base_damage: i32,
    pub warhead: crate::sim::intern::InternedId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveDamageEvent {
    pub wave_id: u64,
    pub target_id: u64,
    pub payload: WaveDamagePayload,
}

pub const WAVE_DISPLAY_REGISTRATION_BUCKET: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WaveColorMode {
    FramebufferSonicDistortion,
    FixedLaserChannelAdd,
    FramebufferMagnetronDistortion,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WaveEdgeGeometry {
    pub firer_a: ProjectileCoord,
    pub firer_b: ProjectileCoord,
    pub target_a: ProjectileCoord,
    pub target_b: ProjectileCoord,
}

impl WaveEdgeGeometry {
    const fn collapsed(source: ProjectileCoord, target: ProjectileCoord) -> Self {
        Self {
            firer_a: source,
            firer_b: source,
            target_a: target,
            target_b: target,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WaveUpdateContext {
    pub owner_position: Option<ProjectileCoord>,
    pub owner_current_target: Option<TargetKind>,
    pub target_position: Option<ProjectileCoord>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Wave {
    pub id: u64,
    #[serde(skip)]
    pub in_logic_vector: bool,
    pub wave_type: u8,
    pub owner_id: Option<u64>,
    pub target_ref: Option<TargetKind>,
    /// Live firer-side endpoint (+0xC0).
    pub source: ProjectileCoord,
    /// Live target-side endpoint (+0xB4), including Sonic's Z adjustment.
    pub target: ProjectileCoord,
    pub edge_geometry: WaveEdgeGeometry,
    pub active_geometry: bool,
    pub decaying: bool,
    pub lifetime: i32,
    pub type3_cycle_count: i32,
    pub fade_in: NativeF64Bits,
    pub fade_out: NativeF64Bits,
    pub direction_octant: i32,
    pub intensity: i32,
    pub recorded_cells: Vec<WaveRecordedCell>,
}

impl Wave {
    pub const DEFAULT_LIFETIME: i32 = 100;
    pub const DEFAULT_LASER_INTENSITY: i32 = 160;

    /// gamemd-derived: `WaveClass::Constructor @ 0x0075E950`.
    pub const fn new(wave_type: u8, source: ProjectileCoord, target: ProjectileCoord) -> Self {
        Self {
            id: 0,
            in_logic_vector: false,
            wave_type,
            owner_id: None,
            target_ref: None,
            source,
            target,
            edge_geometry: WaveEdgeGeometry::collapsed(source, target),
            active_geometry: true,
            decaying: false,
            lifetime: Self::DEFAULT_LIFETIME,
            type3_cycle_count: 0,
            fade_in: NativeF64Bits::POSITIVE_ZERO,
            fade_out: NativeF64Bits::POSITIVE_ZERO,
            direction_octant: 0,
            intensity: Self::DEFAULT_LASER_INTENSITY,
            recorded_cells: Vec::new(),
        }
    }

    pub const fn new_owned(
        wave_type: u8,
        owner_id: u64,
        target_ref: TargetKind,
        source: ProjectileCoord,
        target: ProjectileCoord,
    ) -> Self {
        let mut wave = Self::new(wave_type, source, target);
        wave.owner_id = Some(owner_id);
        wave.target_ref = Some(target_ref);
        wave
    }

    #[cfg(test)]
    pub fn replace_recorded_cells(&mut self, cells: Vec<WaveRecordedCell>) {
        self.recorded_cells = cells;
    }

    pub fn constructor_distance_is_live(&self) -> bool {
        let dx = i64::from(self.target.x) - i64::from(self.source.x);
        let dy = i64::from(self.target.y) - i64::from(self.source.y);
        dx * dx + dy * dy >= CONSTRUCTOR_MIN_XY_DISTANCE * CONSTRUCTOR_MIN_XY_DISTANCE
    }

    /// No reader yet; kept because the wave-type mapping is recorded nowhere else.
    pub const fn color_mode(&self) -> WaveColorMode {
        match self.wave_type {
            0 => WaveColorMode::FramebufferSonicDistortion,
            1 | 2 => WaveColorMode::FixedLaserChannelAdd,
            3 => WaveColorMode::FramebufferMagnetronDistortion,
            _ => WaveColorMode::None,
        }
    }

    /// No reader yet; see `color_mode`.
    pub const fn registration_bucket(&self) -> u8 {
        WAVE_DISPLAY_REGISTRATION_BUCKET
    }

    /// Constructor tail: lifecycle/geometry runs once, but DamageArea does not.
    pub fn initialize(
        &mut self,
        context: WaveUpdateContext,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> bool {
        // `WaveClass::Constructor @ 0x0075E950` builds the initial quad and
        // stores +0x1CC before Logic registration invokes the lifecycle once.
        // Later lifecycle refreshes must never overwrite this selector.
        if self.wave_type == 0 {
            let geometry = type0_nonmagnetic_geometry(self.source, self.target);
            self.apply_type0_geometry(geometry);
            self.direction_octant = geometry.direction_octant;
        }
        self.update_geometry_and_cells(context, terrain)
    }

    /// gamemd-derived: `WaveClass::AI @ 0x00760F50` and lifecycle owner
    /// `0x00762AF0`.
    pub fn advance(
        &mut self,
        context: WaveUpdateContext,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> WaveTickResult {
        match self.wave_type {
            0 | 3 => {
                let terminal_from_fade = self.update_geometry_and_cells(context, terrain);
                let damage_recorded_cells = self.wave_type == 0;
                self.lifetime = self.lifetime.wrapping_sub(1);
                let terminal_from_lifetime = self.lifetime < 0;
                WaveTickResult {
                    alive: !terminal_from_fade && !terminal_from_lifetime,
                    damage_recorded_cells,
                    call_object_ai: !terminal_from_lifetime,
                    uninitialized: terminal_from_fade || terminal_from_lifetime,
                }
            }
            1 | 2 => {
                self.intensity = self.intensity.wrapping_sub(6);
                let alive = self.intensity > 31;
                WaveTickResult {
                    alive,
                    damage_recorded_cells: false,
                    call_object_ai: false,
                    uninitialized: !alive,
                }
            }
            _ => WaveTickResult {
                alive: true,
                damage_recorded_cells: false,
                call_object_ai: false,
                uninitialized: false,
            },
        }
    }

    fn update_geometry_and_cells(
        &mut self,
        context: WaveUpdateContext,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> bool {
        let fade_delta = X87Chop53::sub(load_f64(self.fade_in), load_f64(self.fade_out));
        if self.wave_type == 0
            && X87Chop53::compare(load_f64(AUTO_DECAY_F64), fade_delta) == X87Ordering::Less
        {
            self.decaying = true;
        }

        if self.wave_type == 3 && self.lifetime == 20 {
            self.lifetime = 64;
            self.type3_cycle_count = self.type3_cycle_count.wrapping_add(1);
        }

        if context.target_position.is_none()
            || context.owner_position.is_none()
            || self.lifetime == 20
            || self.target_ref != context.owner_current_target
        {
            self.active_geometry = false;
            self.decaying = true;
        }

        if self.wave_type != 3
            && let (Some(owner), Some(target)) =
                (context.owner_position, context.target_position)
            && distance_3d_leptons(
                [owner.x, owner.y, owner.z],
                [target.x, target.y, target.z],
            ) > MAX_TRACKING_DISTANCE_LEPTONS
        {
            self.active_geometry = false;
            self.decaying = true;
        }

        if self.active_geometry
            && let (Some(owner), Some(target)) = (context.owner_position, context.target_position)
        {
            if self.wave_type == 0 {
                self.apply_type0_geometry(type0_nonmagnetic_geometry(owner, target));
            } else {
                // Type 3 is outside the exactified 0x00761640 slice and keeps
                // its established projection until 0x00762070 is researched.
                self.source = owner;
                self.target = target;
                self.edge_geometry = legacy_nonmagnetic_edges(owner, target);
                let yaw = crate::sim::movement::homing_movement::atan2_bam(
                    crate::util::fixed_math::SimFixed::from_num(target.y.wrapping_sub(owner.y)),
                    crate::util::fixed_math::SimFixed::from_num(target.x.wrapping_sub(owner.x)),
                );
                self.direction_octant = i32::from((yaw >> 13) & 7);
            }
        }

        self.fade_in = add_f32_step_to_f64(self.fade_in);
        if X87Chop53::compare(
            load_f32(f64_to_f32(self.fade_in)),
            load_f32(SNAP_FADE_F32),
        ) == X87Ordering::Greater
        {
            self.fade_in = NativeF64Bits::ONE;
        }

        if self.decaying && f32_le_sum(self.fade_out, self.fade_in, HALF_STEP_F32) {
            self.fade_out = add_f32_step_to_f64(self.fade_out);
            if X87Chop53::compare(
                load_f32(f64_to_f32(self.fade_out)),
                load_f32(f64_to_f32(self.fade_in)),
            ) != X87Ordering::Less
            {
                // Native returns before UpdateCells, leaving the previous vector.
                return true;
            }
        }

        self.update_cells(terrain);
        false
    }

    /// gamemd-derived: `WaveClass::UpdateCells @ 0x007610F0`.
    fn update_cells(&mut self, terrain: Option<&ResolvedTerrainGrid>) {
        self.recorded_cells.clear();
        if self.active_geometry {
            let mut previous = lepton_cell(self.target);
            let mut t = f32_to_f64(STEP_F32);
            while f32_le_sum(t, self.fade_in, HALF_STEP_F32) {
                let cell = lepton_cell(interpolate(self.target, self.source, t));
                if cell != previous {
                    previous = cell;
                    self.lookup_and_append(terrain, cell);
                }
                t = add_f32_step_to_f64(t);
            }
            return;
        }

        let edges = self.edge_geometry;
        let mut previous_a = lepton_cell(edges.firer_a);
        let mut previous_center = lepton_cell(self.source);
        let mut previous_b = lepton_cell(edges.firer_b);
        let mut t = self.fade_out;
        while f32_le_sum(t, self.fade_in, HALF_STEP_F32) {
            if self.direction_octant < 4 {
                let cell = lepton_cell(interpolate(edges.target_a, edges.firer_a, t));
                if cell != previous_a {
                    previous_a = cell;
                    self.lookup_and_append(terrain, cell);
                }
            }
            let cell = lepton_cell(interpolate(self.target, self.source, t));
            if cell != previous_center {
                previous_center = cell;
                self.lookup_and_append(terrain, cell);
            }
            if self.direction_octant >= 4 {
                let cell = lepton_cell(interpolate(edges.target_b, edges.firer_b, t));
                if cell != previous_b {
                    previous_b = cell;
                    self.lookup_and_append(terrain, cell);
                }
            }
            t = add_f32_step_to_f64(t);
        }
    }

    fn lookup_and_append(&mut self, terrain: Option<&ResolvedTerrainGrid>, cell: (i32, i32)) {
        // GetCellClass runs before pointer lookup, so repeated misses restamp
        // the dummy even when its pointer is already present in the vector.
        let identity = match get_cellclass_fallback(terrain, cell.0, cell.1) {
            CellRef::Real(real) => WaveCellIdentity::Real {
                fixed_stride_index: u32::from(real.ry) * CELL_ROW_STRIDE as u32
                    + u32::from(real.rx),
            },
            CellRef::Dummy { .. } => WaveCellIdentity::SharedDummy,
        };
        if self
            .recorded_cells
            .iter()
            .any(|recorded| recorded.identity == identity)
        {
            return;
        }
        if self.recorded_cells.try_reserve(1).is_ok() {
            self.recorded_cells.push(WaveRecordedCell { identity });
        }
    }

    fn apply_type0_geometry(&mut self, geometry: Type0NonmagneticGeometry) {
        self.source = geometry.source;
        self.target = geometry.target;
        self.edge_geometry = geometry.edges;
    }

    pub const fn visible_through_fog(
        &self,
        scenario_fog_gate: bool,
        source_fogged: bool,
        target_fogged: bool,
    ) -> bool {
        !scenario_fog_gate || !source_fogged || !target_fogged
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveTickResult {
    pub alive: bool,
    pub damage_recorded_cells: bool,
    pub call_object_ai: bool,
    pub uninitialized: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WaveStore {
    waves: BTreeMap<u64, Wave>,
}

impl std::hash::Hash for Wave {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.wave_type.hash(state);
        self.owner_id.hash(state);
        self.target_ref.hash(state);
        self.source.hash(state);
        self.target.hash(state);
        self.edge_geometry.hash(state);
        self.active_geometry.hash(state);
        self.decaying.hash(state);
        self.lifetime.hash(state);
        self.type3_cycle_count.hash(state);
        self.fade_in.hash(state);
        self.fade_out.hash(state);
        self.direction_octant.hash(state);
        self.intensity.hash(state);
        self.recorded_cells.hash(state);
    }
}

impl WaveStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.waves.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Wave)> {
        self.waves.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (&u64, &mut Wave)> {
        self.waves.iter_mut()
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Wave> {
        self.waves.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: u64) -> Option<&mut Wave> {
        self.waves.get_mut(&id)
    }

    pub(crate) fn remove(&mut self, id: u64) -> Option<Wave> {
        self.waves.remove(&id)
    }

    /// gamemd-derived: `WaveClass::PointerExpired @ 0x0075F610` clears only
    /// exact object-pointer matches. Cell targets are CellClass pointers and
    /// therefore cannot match an expiring ObjectClass identity.
    pub(crate) fn pointer_expired(&mut self, id: u64, expired_id: u64) -> Option<(bool, bool)> {
        let wave = self.waves.get_mut(&id)?;
        let owner_cleared = wave.owner_id == Some(expired_id);
        let target_cleared = matches!(
            wave.target_ref,
            Some(TargetKind::Entity(target_id)) if target_id == expired_id
        );
        if owner_cleared {
            wave.owner_id = None;
        }
        if target_cleared {
            wave.target_ref = None;
        }
        Some((owner_cleared, target_cleared))
    }

    pub fn spawn(&mut self, id: u64, mut wave: Wave) -> u64 {
        wave.id = id;
        wave.in_logic_vector = false;
        self.waves.insert(id, wave);
        id
    }

    pub(crate) fn advance_one(
        &mut self,
        id: u64,
        context: WaveUpdateContext,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Option<(Option<WaveDamageRequest>, WaveTickResult)> {
        let wave = self.waves.get_mut(&id)?;
        let result = wave.advance(context, terrain);
        let request = if result.damage_recorded_cells {
            wave.owner_id.map(|firer_id| WaveDamageRequest {
                wave_id: id,
                firer_id,
                recorded_cells: wave.recorded_cells.clone(),
                wave_z: wave.target.z,
            })
        } else {
            None
        };
        Some((request, result))
    }
}

fn load_f32(bits: NativeF32Bits) -> crate::util::native_x87::X87Value {
    X87Chop53::load_f32(bits).expect("Wave finite binary32 state")
}

fn load_f64(bits: NativeF64Bits) -> crate::util::native_x87::X87Value {
    X87Chop53::load_f64(bits).expect("Wave finite double state")
}

fn f64_to_f32(bits: NativeF64Bits) -> NativeF32Bits {
    // The active x87 path keeps 53-bit arithmetic precision but uses the
    // ordinary nearest-even store rule for the explicit binary32 narrowing.
    // Integer coordinate conversion is the separate toward-zero operation.
    NativeF32Bits::from_bits((f64::from_bits(bits.bits()) as f32).to_bits())
}

fn f32_to_f64(bits: NativeF32Bits) -> NativeF64Bits {
    X87Chop53::store_f64(load_f32(bits)).expect("finite binary32 widens to double")
}

fn add_f32_step_to_f64(value: NativeF64Bits) -> NativeF64Bits {
    let sum = X87Chop53::add(load_f32(f64_to_f32(value)), load_f32(STEP_F32));
    X87Chop53::store_f64(sum).expect("Wave fade step remains finite double")
}

fn f32_le_sum(lhs: NativeF64Bits, rhs: NativeF64Bits, addend: NativeF32Bits) -> bool {
    let rhs_sum = X87Chop53::add(load_f32(f64_to_f32(rhs)), load_f32(addend));
    X87Chop53::compare(load_f32(f64_to_f32(lhs)), rhs_sum) != X87Ordering::Greater
}

fn interpolate(a: ProjectileCoord, b: ProjectileCoord, t: NativeF64Bits) -> ProjectileCoord {
    let t = load_f64(t);
    let one_minus_t = X87Chop53::sub(load_f64(NativeF64Bits::ONE), t);
    let axis = |a: i32, b: i32| {
        X87Chop53::ftol_i64(X87Chop53::add(
            X87Chop53::mul(one_minus_t, X87Chop53::load_i32(a)),
            X87Chop53::mul(t, X87Chop53::load_i32(b)),
        ))
        .expect("Wave interpolation remains in map coordinate range") as i32
    };
    ProjectileCoord::new(axis(a.x, b.x), axis(a.y, b.y), axis(a.z, b.z))
}

fn lepton_cell(coord: ProjectileCoord) -> (i32, i32) {
    (coord.x / 256, coord.y / 256)
}

const NEGATIVE_2048_F32: NativeF32Bits = NativeF32Bits::from_bits(0xc500_0000);
const TRIG_SCALE_F32: NativeF32Bits = NativeF32Bits::from_bits(0x4522_f983);
const PI_OVER_TWO_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3ff9_21fb_5444_2d18);
const TAN_PI_OVER_EIGHT_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3fda_8279_a061_eb64);
const INV_TAN_PI_OVER_EIGHT_F64: NativeF64Bits = NativeF64Bits::from_bits(0x4003_504f_2e96_fc59);

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // machine-fixture diagnostics retained beside the behavior fields
struct Type0NonmagneticGeometry {
    source: ProjectileCoord,
    target: ProjectileCoord,
    edges: WaveEdgeGeometry,
    direction_octant: i32,
    horizontal: i32,
    sqrt_bits: NativeF32Bits,
    acos_index: usize,
    angle_bits: NativeF32Bits,
    trig_units: i32,
    sin_index: usize,
    cos_index: usize,
    target_screen: (i32, i32),
    firer_b_screen: (i32, i32),
}

#[derive(Debug, Clone, Copy)]
struct TransformedVertex {
    x: NativeF32Bits,
    y: NativeF32Bits,
    z: NativeF32Bits,
}

fn x87_store_f32(value: X87Value) -> NativeF32Bits {
    X87Chop53::store_f32(value).expect("verified Wave geometry stays in finite binary32 range")
}

fn x87_ftol_i32(value: X87Value) -> i32 {
    X87Chop53::ftol_i64(value).expect("verified Wave geometry stays in signed integer range") as i32
}

fn project_wave_point(coord: ProjectileCoord) -> (i32, i32) {
    let (x, planar_y) = crate::util::lepton::project_absolute_lepton_xy(coord.x, coord.y);
    (x, planar_y.wrapping_sub(adjust_for_z_standard(coord.z)))
}

fn direction_from_projected(from: (i32, i32), to: (i32, i32)) -> i32 {
    // `0x0075F230`: dx=to.x-from.x and n=from.y-to.y. The four equality
    // boundaries are intentionally asymmetric.
    let dx = to.0.wrapping_sub(from.0);
    let n = from.1.wrapping_sub(to.1);
    if dx == 0 {
        return if n > 0 { 0 } else { 4 };
    }

    let slope = X87Chop53::div(X87Chop53::load_i32(n), X87Chop53::load_i32(dx))
        .expect("nonzero projected dx divides exactly in the verified finite domain");
    let tan = load_f64(TAN_PI_OVER_EIGHT_F64);
    let inv_tan = load_f64(INV_TAN_PI_OVER_EIGHT_F64);
    let neg_tan = X87Chop53::neg(tan);
    let neg_inv_tan = X87Chop53::neg(inv_tan);

    let band = if X87Chop53::compare(slope, tan) != X87Ordering::Less
        && X87Chop53::compare(slope, inv_tan) == X87Ordering::Less
    {
        Some(5)
    } else if X87Chop53::compare(slope, neg_tan) != X87Ordering::Less
        && X87Chop53::compare(slope, tan) == X87Ordering::Less
    {
        Some(6)
    } else if X87Chop53::compare(slope, neg_inv_tan) != X87Ordering::Less
        && X87Chop53::compare(slope, neg_tan) == X87Ordering::Less
    {
        Some(7)
    } else {
        None
    };
    match band {
        Some(base) if dx > 0 => base - 4,
        Some(base) => base,
        None if n > 0 => 0,
        None => 4,
    }
}

fn transform_type0_vertex(
    local_x: i32,
    local_y: i32,
    local_z: NativeF32Bits,
    sin: NativeF32Bits,
    cos: NativeF32Bits,
) -> TransformedVertex {
    // `Matrix3x4::TransformPoint @ 0x005AFB80`, including the native
    // non-fused operation order and explicit binary32 stores.
    let zero = X87Chop53::load_i32(0);
    let one = load_f32(NativeF32Bits::ONE);
    let x = X87Chop53::load_i32(local_x);
    let y = X87Chop53::load_i32(local_y);
    let z = load_f32(local_z);
    let sin = load_f32(sin);
    let cos = load_f32(cos);

    let transformed_x = X87Chop53::add(
        X87Chop53::add(
            X87Chop53::mul(z, zero),
            X87Chop53::mul(y, X87Chop53::neg(sin)),
        ),
        X87Chop53::mul(x, cos),
    );
    let transformed_y = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(x, sin), X87Chop53::mul(z, zero)),
        X87Chop53::mul(y, cos),
    );
    let transformed_z = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(x, zero), X87Chop53::mul(z, one)),
        X87Chop53::mul(y, zero),
    );
    TransformedVertex {
        x: x87_store_f32(transformed_x),
        y: x87_store_f32(transformed_y),
        z: x87_store_f32(transformed_z),
    }
}

fn type0_nonmagnetic_geometry_with_tables(
    source: ProjectileCoord,
    raw_target: ProjectileCoord,
    trig: &TrigTable,
    acos: &AcosTable,
) -> Type0NonmagneticGeometry {
    // Type 0's endpoint factor is exactly 1.0: +C0 is the owner, +B4 is the
    // target with its post-ftol Sonic Z adjustment.
    let target = ProjectileCoord::new(
        raw_target.x,
        raw_target.y,
        raw_target.z.wrapping_add(SONIC_TARGET_Z_ADJUST),
    );
    let first_dx = X87Chop53::load_i32(target.x.wrapping_sub(source.x));
    let first_dy = X87Chop53::load_i32(target.y.wrapping_sub(source.y));
    let first_squared = X87Chop53::add(
        X87Chop53::mul(first_dx, first_dx),
        X87Chop53::mul(first_dy, first_dy),
    );
    let sqrt_bits =
        sqrt_approx_f32(first_squared).expect("type-0 Wave horizontal squared length is finite");
    let horizontal = x87_ftol_i32(load_f32(sqrt_bits));
    let local_x = [
        horizontal.wrapping_sub(30),
        horizontal.wrapping_sub(30),
        30,
        30,
    ];
    let local_y = [-100, 100, -100, 100];

    let local_z = if horizontal == 0 {
        // The native masked divide/convert path produces NaN/Inf, then the low
        // dword of integer-indefinite zeroes each cached Z. native_x87 rejects
        // nonfinite values by design, so preserve the verified observable here.
        [NativeF32Bits::POSITIVE_ZERO; 4]
    } else {
        let dz = X87Chop53::load_i32(source.z.wrapping_sub(target.z));
        let divisor = X87Chop53::load_i32(horizontal);
        local_x.map(|x| {
            let slope = X87Chop53::div(X87Chop53::mul(X87Chop53::load_i32(x), dz), divisor)
                .expect("nonzero Wave horizontal length divides in finite x87 domain");
            x87_store_f32(X87Chop53::add(X87Chop53::load_i32(target.z), slope))
        })
    };

    let angle_dx_i32 = source.x.wrapping_sub(target.x);
    let angle_dy_i32 = source.y.wrapping_sub(target.y);
    let angle_dx_f32 = x87_store_f32(X87Chop53::load_i32(angle_dx_i32));
    let angle_squared = X87Chop53::add(
        X87Chop53::mul(load_f32(angle_dx_f32), load_f32(angle_dx_f32)),
        X87Chop53::mul(
            X87Chop53::load_i32(angle_dy_i32),
            X87Chop53::load_i32(angle_dy_i32),
        ),
    );
    let angle_length_bits =
        sqrt_approx_f32(angle_squared).expect("type-0 Wave angle squared length is finite");
    let angle_length = load_f32(angle_length_bits);

    let (acos_index, mut angle) = if horizontal == 0 {
        let entry = NativeF32Bits::from_bits(acos.entry(0).to_bits());
        (
            0,
            X87Chop53::sub(load_f64(PI_OVER_TWO_F64), load_f32(entry)),
        )
    } else {
        let mut numerator = X87Chop53::load_i32(angle_dx_i32);
        if X87Chop53::compare(numerator, angle_length) == X87Ordering::Greater {
            numerator = angle_length;
        }
        let negative_length = X87Chop53::neg(angle_length);
        if X87Chop53::compare(numerator, negative_length) == X87Ordering::Less {
            numerator = negative_length;
        }
        let ratio = X87Chop53::div(numerator, angle_length)
            .expect("nonzero Wave angle length divides in finite x87 domain");
        let scaled = X87Chop53::mul(
            X87Chop53::add(ratio, load_f32(NativeF32Bits::ONE)),
            load_f32(NEGATIVE_2048_F32),
        );
        let raw_index = x87_ftol_i32(scaled).wrapping_neg() as usize;
        let entry = NativeF32Bits::from_bits(acos.entry(raw_index).to_bits());
        (
            raw_index,
            X87Chop53::sub(load_f64(PI_OVER_TWO_F64), load_f32(entry)),
        )
    };
    if target.y > source.y {
        angle = X87Chop53::neg(angle);
    }
    let angle_bits = x87_store_f32(angle);
    let trig_units = x87_ftol_i32(X87Chop53::mul(
        load_f32(angle_bits),
        load_f32(TRIG_SCALE_F32),
    ));
    let sin_index = trig.sin_index(trig_units);
    let cos_index = trig.cos_index(trig_units);
    let sin = NativeF32Bits::from_bits(trig.entry(sin_index).to_bits());
    let cos = NativeF32Bits::from_bits(trig.entry(cos_index).to_bits());

    // Native transforms all four local points before translating/storing any
    // world field. Keep that boundary visible.
    let transformed = [
        transform_type0_vertex(local_x[0], local_y[0], local_z[0], sin, cos),
        transform_type0_vertex(local_x[1], local_y[1], local_z[1], sin, cos),
        transform_type0_vertex(local_x[2], local_y[2], local_z[2], sin, cos),
        transform_type0_vertex(local_x[3], local_y[3], local_z[3], sin, cos),
    ];
    let target_y_f32 = x87_store_f32(X87Chop53::load_i32(target.y));
    let mut world = [ProjectileCoord::new(0, 0, 0); 4];
    for (index, vertex) in transformed.into_iter().enumerate() {
        let world_x = x87_ftol_i32(X87Chop53::add(
            X87Chop53::load_i32(target.x),
            load_f32(vertex.x),
        ));
        let target_y = if index == 0 {
            X87Chop53::load_i32(target.y)
        } else {
            load_f32(target_y_f32)
        };
        let world_y = x87_ftol_i32(X87Chop53::add(target_y, load_f32(vertex.y)));
        let world_z = x87_ftol_i32(load_f32(vertex.z));
        world[index] = ProjectileCoord::new(world_x, world_y, world_z);
    }

    let edges = WaveEdgeGeometry {
        firer_a: world[0],
        firer_b: world[1],
        target_a: world[2],
        target_b: world[3],
    };
    let target_screen = project_wave_point(target);
    let firer_b_screen = project_wave_point(edges.firer_b);
    Type0NonmagneticGeometry {
        source,
        target,
        edges,
        direction_octant: direction_from_projected(target_screen, firer_b_screen),
        horizontal,
        sqrt_bits,
        acos_index,
        angle_bits,
        trig_units,
        sin_index,
        cos_index,
        target_screen,
        firer_b_screen,
    }
}

fn installed_wave_math_tables() -> (&'static TrigTable, &'static AcosTable) {
    crate::map::retail_trig::required_math_tables()
}

fn type0_nonmagnetic_geometry(
    source: ProjectileCoord,
    raw_target: ProjectileCoord,
) -> Type0NonmagneticGeometry {
    let (trig, acos) = installed_wave_math_tables();
    type0_nonmagnetic_geometry_with_tables(source, raw_target, trig, acos)
}

fn legacy_nonmagnetic_edges(source: ProjectileCoord, target: ProjectileCoord) -> WaveEdgeGeometry {
    // Preserve the pre-existing type-3 projection until its separate helper at
    // 0x00762070 is exactified. Type 0 never reaches this host-float path.
    let dx = f64::from(target.x.wrapping_sub(source.x));
    let dy = f64::from(target.y.wrapping_sub(source.y));
    let dz = f64::from(target.z.wrapping_sub(source.z));
    let horizontal = dx.hypot(dy);
    if horizontal == 0.0 {
        return WaveEdgeGeometry::collapsed(source, target);
    }
    let angle_magnitude = (dx / horizontal).clamp(-1.0, 1.0).asin();
    let angle = if source.y > target.y {
        -angle_magnitude
    } else {
        angle_magnitude
    };
    let (sin, cos) = angle.sin_cos();
    let vertex = |offset_x: i32, offset_y: i32| {
        let local_x = horizontal + f64::from(offset_x);
        let local_y = f64::from(offset_y);
        ProjectileCoord::new(
            (f64::from(source.x) + local_x * cos - local_y * sin).trunc() as i32,
            (f64::from(source.y) + local_x * sin + local_y * cos).trunc() as i32,
            (local_x * dz / horizontal + f64::from(source.z)).trunc() as i32,
        )
    };
    WaveEdgeGeometry {
        firer_a: vertex(-30, -100),
        firer_b: vertex(-30, 100),
        target_a: vertex(30, -100),
        target_b: vertex(30, 100),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_cell(rx: u16, ry: u16) -> crate::map::resolved_terrain::ResolvedTerrainCell {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
            speed_costs: crate::rules::terrain_rules::SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            height_in_pixels: 0,
            variant: 0,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
            base_speed_costs: crate::rules::terrain_rules::SpeedCostProfile::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let cells = (0..height)
            .flat_map(|ry| (0..width).map(move |rx| flat_cell(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    fn point(x: i32, y: i32, z: i32) -> ProjectileCoord {
        ProjectileCoord::new(x, y, z)
    }

    fn valid_context(target_ref: TargetKind) -> WaveUpdateContext {
        WaveUpdateContext {
            owner_position: Some(point(0, 0, 0)),
            owner_current_target: Some(target_ref),
            target_position: Some(point(1024, 0, 0)),
        }
    }

    fn exact_retail_wave_tables() -> Option<(TrigTable, AcosTable)> {
        let dir = std::env::var_os("RA2_DIR")?;
        let image = std::fs::read(std::path::PathBuf::from(dir).join("gamemd.exe")).ok()?;
        let trig = TrigTable::from_executable(&image).ok()?;
        let acos = AcosTable::from_executable(&image).ok()?;
        (trig.matches_retail() && acos.matches_retail()).then_some((trig, acos))
    }

    #[test]
    fn constructor_uses_strict_239_240_xy_edge() {
        assert!(!Wave::new(0, point(0, 0, 0), point(239, 0, 999)).constructor_distance_is_live());
        assert!(Wave::new(0, point(0, 0, 0), point(240, 0, -999)).constructor_distance_is_live());
    }

    #[test]
    fn type0_nonmagnetic_geometry_matches_all_eight_machine_octants() {
        let Some((trig, acos)) = exact_retail_wave_tables() else {
            eprintln!("skipped: set RA2_DIR to the retail install to run exact Wave fixtures");
            return;
        };
        struct Fixture {
            target: (i32, i32),
            horizontal: i32,
            sqrt_bits: u32,
            acos_index: usize,
            angle_bits: u32,
            edges: [(i32, i32, i32); 4],
            screens: ((i32, i32), (i32, i32)),
        }
        let fixtures = [
            Fixture {
                target: (4352, 4224),
                horizontal: 286,
                sqrt_bits: 0x438f_1bbc,
                acos_index: 216,
                angle_bits: 0xc02b_6743,
                edges: [
                    (4078, 4198, 5),
                    (4167, 4019, 5),
                    (4280, 4299, 44),
                    (4370, 4121, 44),
                ],
                screens: ((15, 495), (17, 478)),
            },
            Fixture {
                target: (4352, 4352),
                horizontal: 362,
                sqrt_bits: 0x43b5_04f3,
                acos_index: 599,
                angle_bits: 0xc016_d574,
                edges: [
                    (4046, 4187, 4),
                    (4188, 4046, 4),
                    (4260, 4401, 45),
                    (4401, 4260, 45),
                ],
                screens: ((0, 503), (16, 481)),
            },
            Fixture {
                target: (4096, 4352),
                horizontal: 256,
                sqrt_bits: 0x4380_0000,
                acos_index: 2048,
                angle_bits: 0xbfc9_0fda,
                edges: [
                    (3996, 4125, 5),
                    (4196, 4126, 5),
                    (3996, 4321, 44),
                    (4196, 4322, 44),
                ],
                screens: ((-30, 488), (8, 486)),
            },
            Fixture {
                target: (3840, 4096),
                horizontal: 256,
                sqrt_bits: 0x4380_0000,
                acos_index: 4096,
                angle_bits: 0x33a2_2168,
                edges: [
                    (4066, 3996, 5),
                    (4066, 4196, 5),
                    (3870, 3996, 44),
                    (3870, 4196, 44),
                ],
                screens: ((-30, 458), (-15, 483)),
            },
            Fixture {
                target: (3840, 3968),
                horizontal: 286,
                sqrt_bits: 0x438f_1bbc,
                acos_index: 3879,
                angle_bits: 0x3eed_d3be,
                edges: [
                    (4113, 3993, 5),
                    (4024, 4172, 5),
                    (3911, 3892, 44),
                    (3821, 4070, 44),
                ],
                screens: ((-15, 450), (-17, 479)),
            },
            Fixture {
                target: (3968, 3840),
                horizontal: 286,
                sqrt_bits: 0x438f_1bbc,
                acos_index: 2963,
                angle_bits: 0x3f8d_c707,
                edges: [
                    (4171, 4024, 5),
                    (3992, 4113, 5),
                    (4070, 3822, 44),
                    (3891, 3911, 44),
                ],
                screens: ((15, 450), (-14, 473)),
            },
            Fixture {
                target: (4096, 3840),
                horizontal: 256,
                sqrt_bits: 0x4380_0000,
                acos_index: 2048,
                angle_bits: 0x3fc9_0fda,
                edges: [
                    (4196, 4065, 5),
                    (3996, 4066, 5),
                    (4196, 3869, 44),
                    (3996, 3870, 44),
                ],
                screens: ((30, 458), (-8, 471)),
            },
            Fixture {
                target: (4352, 4096),
                horizontal: 256,
                sqrt_bits: 0x4380_0000,
                acos_index: 0,
                angle_bits: 0x4049_0fda,
                edges: [
                    (4126, 4196, 5),
                    (4126, 3996, 5),
                    (4322, 4196, 44),
                    (4322, 3996, 44),
                ],
                screens: ((30, 488), (15, 474)),
            },
        ];

        for (direction, fixture) in fixtures.into_iter().enumerate() {
            let geometry = type0_nonmagnetic_geometry_with_tables(
                point(4096, 4096, 0),
                point(fixture.target.0, fixture.target.1, 0),
                &trig,
                &acos,
            );
            assert_eq!(
                geometry.horizontal, fixture.horizontal,
                "direction {direction}"
            );
            assert_eq!(
                geometry.sqrt_bits.bits(),
                fixture.sqrt_bits,
                "direction {direction}"
            );
            assert_eq!(
                geometry.acos_index, fixture.acos_index,
                "direction {direction}"
            );
            assert_eq!(
                geometry.angle_bits.bits(),
                fixture.angle_bits,
                "direction {direction}"
            );
            assert_eq!(
                [
                    geometry.edges.firer_a,
                    geometry.edges.firer_b,
                    geometry.edges.target_a,
                    geometry.edges.target_b
                ]
                .map(|coord| (coord.x, coord.y, coord.z)),
                fixture.edges,
                "direction {direction}"
            );
            assert_eq!(
                (geometry.target_screen, geometry.firer_b_screen),
                fixture.screens
            );
            assert_eq!(geometry.direction_octant, direction as i32);
        }
    }

    #[test]
    fn type0_nonmagnetic_geometry_matches_signed_edges_and_nonflat_z() {
        let Some((trig, acos)) = exact_retail_wave_tables() else {
            eprintln!("skipped: set RA2_DIR to the retail install to run exact Wave fixtures");
            return;
        };
        let fixtures = [
            (
                point(1024, 1024, 0),
                point(255, 255, 0),
                1087,
                0x4487_f0f0,
                [
                    (1073, 931, 1),
                    (931, 1073, 1),
                    (346, 205, 48),
                    (205, 346, 48),
                ],
                [(4, 3), (3, 4), (1, 0), (0, 1)],
                4,
            ),
            (
                point(1024, 1024, 0),
                point(256, 256, 0),
                1086,
                0x4487_c3b6,
                [
                    (1073, 931, 1),
                    (931, 1073, 1),
                    (347, 206, 48),
                    (206, 347, 48),
                ],
                [(4, 3), (3, 4), (1, 0), (0, 1)],
                4,
            ),
            (
                point(512, 512, 0),
                point(-255, -255, 0),
                1084,
                0x4487_966d,
                [
                    (561, 419, 1),
                    (419, 561, 1),
                    (-163, -304, 48),
                    (-304, -163, 48),
                ],
                [(2, 1), (1, 2), (0, -1), (-1, 0)],
                4,
            ),
            (
                point(512, 512, 0),
                point(-256, -256, 0),
                1086,
                0x4487_c3b6,
                [
                    (561, 419, 1),
                    (419, 561, 1),
                    (-164, -305, 48),
                    (-305, -164, 48),
                ],
                [(2, 1), (1, 2), (0, -1), (-1, 0)],
                4,
            ),
            (
                point(513, -257, 180),
                point(-260, 769, 646),
                1284,
                0x44a0_92ef,
                [
                    (414, -292, 192),
                    (574, -172, 192),
                    (-321, 684, 683),
                    (-162, 805, 683),
                ],
                [(1, -1), (2, 0), (-1, 2), (0, 3)],
                2,
            ),
        ];
        for (source, target, horizontal, sqrt_bits, expected, cells, direction) in fixtures {
            let geometry = type0_nonmagnetic_geometry_with_tables(source, target, &trig, &acos);
            let edges = [
                geometry.edges.firer_a,
                geometry.edges.firer_b,
                geometry.edges.target_a,
                geometry.edges.target_b,
            ];
            assert_eq!(geometry.horizontal, horizontal);
            assert_eq!(geometry.sqrt_bits.bits(), sqrt_bits);
            assert_eq!(edges.map(|coord| (coord.x, coord.y, coord.z)), expected);
            assert_eq!(edges.map(lepton_cell), cells);
            assert_eq!(geometry.direction_octant, direction);
        }
    }

    #[test]
    fn converged_live_type0_geometry_keeps_native_exceptional_quad() {
        let Some((trig, acos)) = exact_retail_wave_tables() else {
            eprintln!("skipped: set RA2_DIR to the retail install to run exact Wave fixtures");
            return;
        };
        let geometry = type0_nonmagnetic_geometry_with_tables(
            point(4096, 4096, 0),
            point(4096, 4096, 0),
            &trig,
            &acos,
        );
        let edges = [
            geometry.edges.firer_a,
            geometry.edges.firer_b,
            geometry.edges.target_a,
            geometry.edges.target_b,
        ];
        assert_eq!(geometry.horizontal, 0);
        assert_eq!(geometry.acos_index, 0);
        assert_eq!(geometry.angle_bits.bits(), 0x4049_0fda);
        assert_eq!(geometry.trig_units, 8191);
        assert_eq!((geometry.sin_index, geometry.cos_index), (4096, 6144));
        assert_eq!(edges.map(|coord| coord.z), [0; 4]);
        assert_eq!(
            edges
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn type0_live_refresh_preserves_constructor_projected_direction() {
        let target_ref = TargetKind::Cell(17, 16);
        let source = point(4096, 4096, 0);
        let initial_target = point(4352, 4096, 0);
        let mut wave = Wave::new_owned(0, 7, target_ref, source, initial_target);
        assert!(!wave.initialize(
            WaveUpdateContext {
                owner_position: Some(source),
                owner_current_target: Some(target_ref),
                target_position: Some(initial_target),
            },
            None,
        ));
        assert_eq!(wave.direction_octant, 7);

        let moved_target = point(3840, 4096, 0);
        let _ = wave.advance(
            WaveUpdateContext {
                owner_position: Some(source),
                owner_current_target: Some(target_ref),
                target_position: Some(moved_target),
            },
            None,
        );
        assert_eq!(wave.direction_octant, 7);
        assert_eq!(
            wave.edge_geometry,
            type0_nonmagnetic_geometry(source, moved_target).edges
        );
    }

    #[test]
    fn inactive_update_cells_uses_exact_fc_108_114_120_identities() {
        let terrain = flat_terrain(8, 8);
        let geometry = type0_nonmagnetic_geometry(point(1024, 1024, 0), point(255, 255, 0));
        let mut wave = Wave::new(0, geometry.source, geometry.target);
        wave.apply_type0_geometry(geometry);
        wave.active_geometry = false;
        wave.fade_in = f32_to_f64(STEP_F32);
        wave.fade_out = f32_to_f64(STEP_F32);

        // direction<4 traces +0x114 -> +0xFC before the centerline.
        wave.direction_octant = 3;
        wave.update_cells(Some(&terrain));
        assert_eq!(
            wave.recorded_cells,
            vec![WaveRecordedCell::real(1, 0), WaveRecordedCell::real(1, 1)]
        );

        // direction>=4 traces the centerline before +0x120 -> +0x108.
        wave.direction_octant = 4;
        wave.update_cells(Some(&terrain));
        assert_eq!(
            wave.recorded_cells,
            vec![WaveRecordedCell::real(1, 1), WaveRecordedCell::real(0, 1)]
        );
    }

    #[test]
    fn type_zero_mixed_precision_fade_yields_21_passes_and_stale_terminal_cells() {
        let target_ref = TargetKind::Cell(4, 0);
        let ctx = valid_context(target_ref);
        let mut wave = Wave::new_owned(0, 7, target_ref, point(0, 0, 0), point(1024, 0, 0));
        assert!(!wave.initialize(ctx, None));
        assert_eq!(wave.fade_in.bits(), 0x3fa9_9999_a000_0000);
        let mut passes = 0;
        let mut ai20_cells = Vec::new();
        loop {
            let result = wave.advance(ctx, None);
            if result.damage_recorded_cells {
                passes += 1;
            }
            if passes == 20 {
                ai20_cells = wave.recorded_cells.clone();
            }
            if !result.alive {
                assert_eq!(passes, 21);
                assert_eq!(wave.lifetime, 79);
                assert_eq!(wave.recorded_cells, ai20_cells);
                break;
            }
        }
    }

    #[test]
    fn negative_sampling_deduplicates_shared_dummy_and_keeps_final_restamp() {
        let terrain = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        let mut wave = Wave::new(0, point(-1024, -1024, 0), point(-256, -256, 50));
        wave.fade_in = NativeF64Bits::ONE;
        wave.update_cells(Some(&terrain));
        assert_eq!(wave.recorded_cells, vec![WaveRecordedCell::shared_dummy()]);
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (-4, -4));
    }

    #[test]
    fn active_sampling_excludes_the_target_seed_cell() {
        let terrain = flat_terrain(8, 1);
        let mut wave = Wave::new(0, point(0, 0, 0), point(4 * 256, 0, 50));
        wave.fade_in = f32_to_f64(STEP_F32);
        wave.update_cells(Some(&terrain));
        assert_eq!(wave.recorded_cells, vec![WaveRecordedCell::real(3, 0)]);
        assert!(!wave.recorded_cells.contains(&WaveRecordedCell::real(4, 0)));
    }

    #[test]
    fn inactive_sampling_selects_only_the_direction_owned_edge() {
        let terrain = flat_terrain(12, 1);
        let mut wave = Wave::new(0, point(0, 0, 0), point(0, 0, 50));
        wave.active_geometry = false;
        wave.fade_in = f32_to_f64(STEP_F32);
        wave.fade_out = f32_to_f64(STEP_F32);
        wave.edge_geometry = WaveEdgeGeometry {
            firer_a: point(0, 0, 0),
            firer_b: point(0, 0, 0),
            target_a: point(4 * 256, 0, 0),
            target_b: point(8 * 256, 0, 0),
        };

        wave.direction_octant = 3;
        wave.update_cells(Some(&terrain));
        assert_eq!(wave.recorded_cells, vec![WaveRecordedCell::real(3, 0)]);

        wave.direction_octant = 4;
        wave.update_cells(Some(&terrain));
        assert_eq!(wave.recorded_cells, vec![WaveRecordedCell::real(7, 0)]);
    }

    #[test]
    fn fixed_stride_alias_and_dummy_reentry_dedupe_by_cellclass_identity() {
        let terrain = flat_terrain(512, 1);
        let mut wave = Wave::new(0, point(0, 0, 0), point(1024, 0, 0));
        wave.lookup_and_append(Some(&terrain), (511, 0));
        wave.lookup_and_append(Some(&terrain), (-1, 1));
        wave.lookup_and_append(Some(&terrain), (-20, -30));
        wave.lookup_and_append(Some(&terrain), (510, 0));
        wave.lookup_and_append(Some(&terrain), (-40, -50));

        assert_eq!(
            wave.recorded_cells,
            vec![
                WaveRecordedCell::real(511, 0),
                WaveRecordedCell::shared_dummy(),
                WaveRecordedCell::real(510, 0),
            ],
        );
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (-40, -50));
    }

    #[test]
    fn type_three_cycles_20_to_64_and_never_requests_damage() {
        let target_ref = TargetKind::Cell(4, 0);
        let ctx = valid_context(target_ref);
        let mut wave = Wave::new_owned(3, 1, target_ref, point(0, 0, 0), point(1024, 0, 0));
        wave.lifetime = 20;
        let result = wave.advance(ctx, None);
        assert_eq!(wave.lifetime, 63);
        assert_eq!(wave.type3_cycle_count, 1);
        assert!(!result.damage_recorded_cells);
    }

    #[test]
    fn non_type_three_tracking_invalidates_only_when_native_distance_exceeds_2172() {
        let target_ref = TargetKind::Cell(8, 0);
        assert_eq!(distance_3d_leptons([0, 0, 0], [2173, 0, 0]), 2172);
        assert_eq!(distance_3d_leptons([0, 0, 0], [2174, 0, 0]), 2173);
        let context_at_limit = WaveUpdateContext {
            owner_position: Some(point(0, 0, 0)),
            owner_current_target: Some(target_ref),
            target_position: Some(point(2173, 0, 0)),
        };
        let mut at_limit =
            Wave::new_owned(0, 1, target_ref, point(0, 0, 0), point(2173, 0, 0));
        let _ = at_limit.advance(context_at_limit, None);
        assert!(at_limit.active_geometry);

        let context_beyond = WaveUpdateContext {
            target_position: Some(point(2174, 0, 0)),
            ..context_at_limit
        };
        let mut beyond =
            Wave::new_owned(0, 1, target_ref, point(0, 0, 0), point(2174, 0, 0));
        let _ = beyond.advance(context_beyond, None);
        assert!(!beyond.active_geometry);
        assert!(beyond.decaying);

        let mut magnetic =
            Wave::new_owned(3, 1, target_ref, point(0, 0, 0), point(2174, 0, 0));
        let _ = magnetic.advance(context_beyond, None);
        assert!(magnetic.active_geometry);
    }
}
