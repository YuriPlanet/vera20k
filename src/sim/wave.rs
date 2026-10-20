//! Persistent YR `WaveClass` runtime state, geometry and CellClass sampling.
//!
//! Wave lifecycle is simulation authority: Waves own a live firer/target
//! relationship, double fade state stepped by a binary32 interval, and a pointer-identity cell
//! vector consumed synchronously from the mixed LogicClass order.

use std::collections::BTreeMap;
use std::hash::Hash;

use crate::map::cell_index::CELL_ROW_STRIDE;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
use crate::sim::combat::TargetKind;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::util::native_coord::interpolate as interpolate_axes;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value, adjust_for_z_standard,
    distance_3d_leptons, sqrt_approx_f32,
};
use crate::util::retail_trig::{AcosTable, TrigTable};

const STEP_F32: NativeF32Bits = NativeF32Bits::from_bits(0x3d4c_cccd);
const SNAP_FADE_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3fef_5c28_f5c2_8f5c);
const AUTO_DECAY_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
const ENDPOINT_Z_ADJUST: i32 = 50;
const CONSTRUCTOR_MIN_XY_DISTANCE: i32 = 240;
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
    #[cfg(any(test, feature = "test-support"))]
    pub const fn real(rx: u16, ry: u16) -> Self {
        Self {
            identity: WaveCellIdentity::Real {
                fixed_stride_index: ry as u32 * CELL_ROW_STRIDE as u32 + rx as u32,
            },
        }
    }

    #[cfg(test)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WaveEdgeGeometry {
    /// Native aCorners[0]. The inherited field names describe Sonic's near
    /// and far edges, not magnetic Unit-target ownership.
    pub firer_a: ProjectileCoord,
    /// Native aCorners[1].
    pub firer_b: ProjectileCoord,
    /// Native aCorners[2].
    pub target_a: ProjectileCoord,
    /// Native aCorners[3].
    pub target_b: ProjectileCoord,
}

/// Derived type3 polygon in unzoomed logical screen pixels. Projection and
/// camera translation precede the native signed integer interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagneticWaveGeometry {
    pub polygon: [[i32; 2]; 4],
    pub source_point: [i32; 2],
}

/// Derived type0 polygon and pixel-table origin in the same screen frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SonicWaveGeometry {
    pub polygon: [[i32; 2]; 6],
    pub source_point: [i32; 2],
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

#[derive(Debug, Clone, Copy, Default)]
pub struct WaveUpdateContext {
    /// Owner Object Location: only the nonmagnetic range-stop query reads it.
    pub owner_position: Option<ProjectileCoord>,
    /// Owner GetFLH(0, null): the geometry reads this distinct virtual.
    pub owner_fire_coordinate: Option<ProjectileCoord>,
    pub owner_current_target: Option<TargetKind>,
    pub target_position: Option<ProjectileCoord>,
    /// Native What_Am_I==1 reverses type3's geometry arguments.
    pub target_is_unit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Wave {
    id: u64,
    #[serde(skip)]
    in_logic_vector: bool,
    wave_type: u8,
    owner_id: Option<u64>,
    target_ref: Option<TargetKind>,
    /// Geometry's first endpoint (native +0xB4); the Wave's Location follows
    /// it (vt+0x1B4). FireAt's constructor seeds it with the target
    /// coordinate. Update_Wave then stores the owner's GetFLH here, except
    /// for a magnetic Unit target, which keeps the held Unit's centre.
    source: ProjectileCoord,
    /// Geometry's second endpoint (+0xC0), including its native +50 Z: the
    /// launch source at birth, then the target (or the owner's GetFLH for a
    /// magnetic Unit target).
    target: ProjectileCoord,
    edge_geometry: WaveEdgeGeometry,
    active_geometry: bool,
    decaying: bool,
    lifetime: i32,
    type3_cycle_count: i32,
    fade_in: NativeF64Bits,
    fade_out: NativeF64Bits,
    direction_octant: i32,
    intensity: i32,
    recorded_cells: Vec<WaveRecordedCell>,
}

impl Wave {
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_active_geometry_for_test(&mut self, value: bool) {
        self.active_geometry = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_decaying_for_test(&mut self, value: bool) {
        self.decaying = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_direction_octant_for_test(&mut self, value: i32) {
        self.direction_octant = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_fade_in_for_test(&mut self, value: NativeF64Bits) {
        self.fade_in = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_fade_out_for_test(&mut self, value: NativeF64Bits) {
        self.fade_out = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_logic_vector_for_test(&mut self, value: bool) {
        self.in_logic_vector = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_lifetime_for_test(&mut self, value: i32) {
        self.lifetime = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owner_id_for_test(&mut self, value: Option<u64>) {
        self.owner_id = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_target_ref_for_test(&mut self, value: Option<TargetKind>) {
        self.target_ref = value;
    }

    /// LogicClass membership: RegisterObject sets it (`0x0055BAC6`),
    /// UnregisterObject clears it (`0x0055BB27`). Only the logic vector's
    /// owner writes it, and a load rebuilds it from the saved vector.
    pub(crate) fn mark_logic_membership(&mut self, member: bool) {
        self.in_logic_vector = member;
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn in_logic_vector(&self) -> bool {
        self.in_logic_vector
    }

    pub const fn wave_type(&self) -> u8 {
        self.wave_type
    }

    pub const fn owner_id(&self) -> Option<u64> {
        self.owner_id
    }

    pub const fn target_ref(&self) -> Option<TargetKind> {
        self.target_ref
    }

    pub const fn source(&self) -> ProjectileCoord {
        self.source
    }

    pub const fn target(&self) -> ProjectileCoord {
        self.target
    }

    pub const fn edge_geometry(&self) -> WaveEdgeGeometry {
        self.edge_geometry
    }

    pub const fn active_geometry(&self) -> bool {
        self.active_geometry
    }

    pub const fn decaying(&self) -> bool {
        self.decaying
    }

    pub const fn lifetime(&self) -> i32 {
        self.lifetime
    }

    pub const fn type3_cycle_count(&self) -> i32 {
        self.type3_cycle_count
    }

    pub const fn fade_in(&self) -> NativeF64Bits {
        self.fade_in
    }

    pub const fn fade_out(&self) -> NativeF64Bits {
        self.fade_out
    }

    pub const fn direction_octant(&self) -> i32 {
        self.direction_octant
    }

    pub const fn intensity(&self) -> i32 {
        self.intensity
    }

    pub fn recorded_cells(&self) -> &[WaveRecordedCell] {
        &self.recorded_cells
    }

    pub const DEFAULT_LIFETIME: i32 = 100;
    pub const DEFAULT_LASER_INTENSITY: i32 = 160;

    /// The ids [`WaveStore::pointer_expired`] compares: the owner and an
    /// object target. VERA-only: feeds the pointer-expiry listener index
    /// (`ExpiryListeners`); a field that callback compares must be named here
    /// too.
    pub(crate) fn expiry_names(&self) -> [Option<u64>; 2] {
        let target = match self.target_ref {
            Some(TargetKind::Entity(id)) => Some(id),
            _ => None,
        };
        [self.owner_id, target]
    }

    /// gamemd-derived: `WaveClass::Constructor @ 0x0075E950`. `first` and
    /// `second` are its two coordinate arguments, the geometry's +0xB4 and
    /// +0xC0 seeds.
    pub const fn new(wave_type: u8, first: ProjectileCoord, second: ProjectileCoord) -> Self {
        Self {
            id: 0,
            in_logic_vector: false,
            wave_type,
            owner_id: None,
            target_ref: None,
            source: first,
            target: second,
            edge_geometry: WaveEdgeGeometry::collapsed(first, second),
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
        first: ProjectileCoord,
        second: ProjectileCoord,
    ) -> Self {
        let mut wave = Self::new(wave_type, first, second);
        wave.owner_id = Some(owner_id);
        wave.target_ref = Some(target_ref);
        wave
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn replace_recorded_cells(&mut self, cells: Vec<WaveRecordedCell>) {
        self.recorded_cells = cells;
    }

    pub fn constructor_distance_is_live(&self) -> bool {
        // 75EA59..75EA91 squares in wrapping dwords, then compares the
        // original Sqrt_Approx result directly with240; no integer conversion.
        let dx = self.target.x.wrapping_sub(self.source.x);
        let dy = self.target.y.wrapping_sub(self.source.y);
        let squared = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
        let root = sqrt_approx_f32(X87Chop53::load_i32(squared))
            .expect("a signed dword fits the native approximate-root input");
        X87Chop53::compare(
            load_f32(root),
            X87Chop53::load_i32(CONSTRUCTOR_MIN_XY_DISTANCE),
        ) != X87Ordering::Less
    }

    /// Draw_Magnetic762070 establishes corners0/1/2/3. Update_Wave762AF0
    /// moves the polygon's front/back after projecting those corners, not by
    /// projecting an interpolated world coordinate. No new retained state.
    pub fn magnetic_draw_geometry(&self, camera: [i32; 2]) -> Option<MagneticWaveGeometry> {
        if self.wave_type != 3 {
            return None;
        }
        let screen = |coord| draw_point(coord, camera);
        let edges = self.edge_geometry;
        let [a, b, c, d] =
            [edges.firer_a, edges.firer_b, edges.target_a, edges.target_b].map(screen);
        let front = load_f64(self.fade_in);
        let back = load_f64(self.fade_out);
        Some(MagneticWaveGeometry {
            polygon: [
                interpolate_axes(c, a, front),
                interpolate_axes(d, b, front),
                interpolate_axes(d, b, back),
                interpolate_axes(c, a, back),
            ],
            source_point: screen(self.source),
        })
    }

    /// Native761640/762F60 six-point geometry. Pixel75EDF0 reads its retained
    /// source origin as well; both use this one projection/camera frame.
    pub fn sonic_draw_geometry(&self, camera: [i32; 2]) -> Option<SonicWaveGeometry> {
        if self.wave_type != 0 {
            return None;
        }
        let [a, b, c, d] = [
            self.edge_geometry.firer_a,
            self.edge_geometry.firer_b,
            self.edge_geometry.target_a,
            self.edge_geometry.target_b,
        ]
        .map(|coord| draw_point(coord, camera));
        let source = draw_point(self.source, camera);
        let target = draw_point(self.target, camera);
        let front = load_f64(self.fade_in);
        let back = load_f64(self.fade_out);
        Some(SonicWaveGeometry {
            polygon: [
                interpolate_axes(c, a, front),
                interpolate_axes(source, target, front),
                interpolate_axes(d, b, front),
                interpolate_axes(d, b, back),
                interpolate_axes(source, target, back),
                interpolate_axes(c, a, back),
            ],
            source_point: source,
        })
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
        if matches!(self.wave_type, 0 | 3) {
            let geometry = wave_geometry(self.wave_type, self.source, self.target);
            self.apply_geometry(geometry);
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

        // 762B29/762B7E: ftol(1.0 / binary32[7F6BB0]), not integer 20.
        let stop_phase = x87_ftol_i32(
            X87Chop53::div(load_f64(NativeF64Bits::ONE), load_f32(STEP_F32))
                .expect("the native Wave step is nonzero"),
        );
        if self.wave_type == 3 && self.lifetime == stop_phase {
            self.lifetime = 64;
            self.type3_cycle_count = self.type3_cycle_count.wrapping_add(1);
        }

        if context.target_position.is_none()
            || context.owner_position.is_none()
            || self.lifetime == stop_phase
            || self.target_ref != context.owner_current_target
        {
            self.active_geometry = false;
            self.decaying = true;
        }

        if self.wave_type != 3
            && let (Some(owner), Some(target)) = (context.owner_position, context.target_position)
            && distance_3d_leptons([owner.x, owner.z, owner.y], [target.x, target.z, target.y])
                > MAX_TRACKING_DISTANCE_LEPTONS
        {
            self.active_geometry = false;
            self.decaying = true;
        }

        if self.active_geometry {
            if let (Some(owner), Some(target)) =
                (context.owner_fire_coordinate, context.target_position)
            {
                let (first, second) = if self.wave_type == 3 && context.target_is_unit {
                    (target, owner)
                } else {
                    (owner, target)
                };
                self.apply_geometry(wave_geometry(self.wave_type, first, second));
            } else {
                self.active_geometry = false;
                self.decaying = true;
            }
        }

        // Raw 762D89/762F77: FLD float step; FADD double front;
        // FST double front. The decompiler's binary32 casts are spurious.
        if self.active_geometry
            && X87Chop53::compare(load_f64(self.fade_in), load_f64(NativeF64Bits::ONE))
                == X87Ordering::Less
        {
            self.fade_in = add_step(self.fade_in);
            if X87Chop53::compare(load_f64(self.fade_in), load_f64(SNAP_FADE_F64))
                == X87Ordering::Greater
            {
                self.fade_in = NativeF64Bits::ONE;
            }
        }

        if self.decaying && within_front(self.fade_out, self.fade_in) {
            self.fade_out = add_step(self.fade_out);
            if X87Chop53::compare(load_f64(self.fade_out), load_f64(self.fade_in))
                != X87Ordering::Less
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
            let mut previous = lepton_cell(self.source);
            let mut t = f32_to_f64(STEP_F32);
            while within_front(t, self.fade_in) {
                let cell = lepton_cell(interpolate(self.source, self.target, t));
                if cell != previous {
                    previous = cell;
                    self.lookup_and_append(terrain, cell);
                }
                t = add_step(t);
            }
            return;
        }

        let edges = self.edge_geometry;
        let mut previous_a = lepton_cell(edges.firer_a);
        let mut previous_center = lepton_cell(self.target);
        let mut previous_b = lepton_cell(edges.firer_b);
        let mut t = self.fade_out;
        while within_front(t, self.fade_in) {
            if self.direction_octant < 4 {
                let cell = lepton_cell(interpolate(edges.target_a, edges.firer_a, t));
                if cell != previous_a {
                    previous_a = cell;
                    self.lookup_and_append(terrain, cell);
                }
            }
            let cell = lepton_cell(interpolate(self.source, self.target, t));
            if cell != previous_center {
                previous_center = cell;
                self.lookup_and_append(terrain, cell);
            }
            if self.direction_octant >= 4 {
                // 7614C7 stores t to binary32 for CoordStruct::Lerp75F540;
                // the inlined left edge and centre above retain double t.
                let narrow_t = f32_to_f64(x87_store_f32(load_f64(t)));
                let cell = lepton_cell(interpolate(edges.target_b, edges.firer_b, narrow_t));
                if cell != previous_b {
                    previous_b = cell;
                    self.lookup_and_append(terrain, cell);
                }
            }
            t = add_step(t);
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

    fn apply_geometry(&mut self, geometry: WaveGeometry) {
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

/// Waves by id. Every `&mut Wave` goes through the store, which notes it in
/// a touch log.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WaveStore {
    waves: BTreeMap<u64, Wave>,
    /// Which waves may have changed since [`Self::take_touched`]. One reader:
    /// the pointer-expiry listener index. Transient: never saved, compared or
    /// hashed.
    #[serde(skip)]
    touched: TouchLogs<1>,
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

    pub fn is_empty(&self) -> bool {
        self.waves.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Wave)> {
        self.waves.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (&u64, &mut Wave)> {
        self.touched.note_all();
        self.waves.iter_mut()
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Wave> {
        self.waves.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: u64) -> Option<&mut Wave> {
        let stored = self.waves.len();
        let wave = self.waves.get_mut(&id)?;
        self.touched.note(id, stored);
        Some(wave)
    }

    pub(crate) fn remove(&mut self, id: u64) -> Option<Wave> {
        let removed = self.waves.remove(&id);
        if removed.is_some() {
            self.touched.note(id, self.waves.len());
        }
        removed
    }

    /// Take the touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self) -> Touched {
        self.touched.take(0)
    }

    /// gamemd-derived: `WaveClass::PointerExpired @ 0x0075F610` clears only
    /// exact object-pointer matches. Cell targets are CellClass pointers and
    /// therefore cannot match an expiring ObjectClass identity.
    pub(crate) fn pointer_expired(&mut self, id: u64, expired_id: u64) -> Option<(bool, bool)> {
        let wave = self.get_mut(id)?;
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
        self.touched.note(id, self.waves.len());
        self.waves.insert(id, wave);
        id
    }

    pub(crate) fn advance_one(
        &mut self,
        id: u64,
        context: WaveUpdateContext,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Option<(Option<WaveDamageRequest>, WaveTickResult)> {
        let wave = self.get_mut(id)?;
        let result = wave.advance(context, terrain);
        let request = if result.damage_recorded_cells {
            wave.owner_id.map(|firer_id| WaveDamageRequest {
                wave_id: id,
                firer_id,
                recorded_cells: wave.recorded_cells.clone(),
                // DamageArea75F3C6 reads B4.Z, not the C0+50 endpoint.
                wave_z: wave.source.z,
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

fn f32_to_f64(bits: NativeF32Bits) -> NativeF64Bits {
    X87Chop53::store_f64(load_f32(bits)).expect("finite binary32 widens to double")
}

fn add_step(value: NativeF64Bits) -> NativeF64Bits {
    let sum = X87Chop53::add(load_f64(value), load_f32(STEP_F32));
    X87Chop53::store_f64(sum).expect("Wave fade step remains finite double")
}

fn within_front(t: NativeF64Bits, front: NativeF64Bits) -> bool {
    let half_step = X87Chop53::mul(load_f32(STEP_F32), load_f64(NativeF64Bits::HALF));
    let end = X87Chop53::add(load_f64(front), half_step);
    X87Chop53::compare(load_f64(t), end) != X87Ordering::Greater
}

fn interpolate(a: ProjectileCoord, b: ProjectileCoord, t: NativeF64Bits) -> ProjectileCoord {
    let [x, y, z] = interpolate_axes([a.x, a.y, a.z], [b.x, b.y, b.z], load_f64(t));
    ProjectileCoord::new(x, y, z)
}

fn lepton_cell(coord: ProjectileCoord) -> (i32, i32) {
    (coord.x / 256, coord.y / 256)
}

// Direction75F230 calls Sin4CACB0/Cos4CAD00(pi/8): retail entries512/2560
// are 3EC3EF15/3F6C835E. Their Chop53 ratio and its reciprocal give these
// exact thresholds; analytic tan(pi/8) would change boundary comparisons.
const TAN_PI_OVER_EIGHT_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3fda_8279_a061_eb64);
const INV_TAN_PI_OVER_EIGHT_F64: NativeF64Bits = NativeF64Bits::from_bits(0x4003_504f_2e96_fc59);

/// Outputs shared by the two active Wave geometry helpers.
#[derive(Debug, Clone, Copy)]
struct WaveGeometry {
    source: ProjectileCoord,
    target: ProjectileCoord,
    edges: WaveEdgeGeometry,
    direction_octant: i32,
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

fn draw_point(coord: ProjectileCoord, camera: [i32; 2]) -> [i32; 2] {
    // The existing presentation projection owns the common tactical row
    // bias. Translate before the native signed interpolation, never after it.
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    [
        (x as i32).wrapping_sub(camera[0]),
        (y as i32).wrapping_sub(camera[1]),
    ]
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

/// Shared arithmetic in Draw_NonMagnetic761640 and Draw_Magnetic762070.
/// Both active types use endpoint factor 1.0. Type3 has zero longitudinal
/// offsets, half-width50 and a 3D Z divisor; Sonic has ±30/100 and an XY divisor.
fn wave_geometry(
    wave_type: u8,
    source: ProjectileCoord,
    raw_target: ProjectileCoord,
) -> WaveGeometry {
    debug_assert!(matches!(wave_type, 0 | 3));
    let (trig, acos) = (TrigTable::embedded(), AcosTable::retail());
    // Both helpers store firstarg at+B4 and secondarg(+50Z) at+C0.
    let target = ProjectileCoord::new(
        raw_target.x,
        raw_target.y,
        raw_target.z.wrapping_add(ENDPOINT_Z_ADJUST),
    );
    let first_dx = X87Chop53::load_i32(target.x.wrapping_sub(source.x));
    let first_dy = X87Chop53::load_i32(target.y.wrapping_sub(source.y));
    let first_squared = X87Chop53::add(
        X87Chop53::mul(first_dx, first_dx),
        X87Chop53::mul(first_dy, first_dy),
    );
    let sqrt_bits =
        sqrt_approx_f32(first_squared).expect("Wave horizontal squared length is finite");
    let horizontal = x87_ftol_i32(load_f32(sqrt_bits));
    let (longitudinal, half_width) = if wave_type == 3 { (0, 50) } else { (30, 100) };
    let local_x = [
        horizontal.wrapping_sub(longitudinal),
        horizontal.wrapping_sub(longitudinal),
        longitudinal,
        longitudinal,
    ];
    let local_y = [-half_width, half_width, -half_width, half_width];

    let z_divisor = if wave_type == 3 {
        distance_3d_leptons(
            [source.x, source.z, source.y],
            [target.x, target.z, target.y],
        )
    } else {
        horizontal
    };

    let world = if z_divisor == 0 {
        // Native761640/762070 retain a masked NaN/Inf through the matrix dot
        // products (including 0*NaN), so every XYZ becomes integer-indefinite
        // and its stored low dword is zero. Keep that native observed result
        // without passing nonfinite values into the shared affine owner.
        [ProjectileCoord::new(0, 0, 0); 4]
    } else {
        let dz = X87Chop53::load_i32(target.z.wrapping_sub(source.z));
        let divisor = X87Chop53::load_i32(z_divisor);
        let local_z = local_x.map(|x| {
            let slope = X87Chop53::div(X87Chop53::mul(X87Chop53::load_i32(x), dz), divisor)
                .expect("nonzero Wave horizontal length divides in finite x87 domain");
            x87_store_f32(X87Chop53::add(X87Chop53::load_i32(source.z), slope))
        });

        let angle_dx_i32 = target.x.wrapping_sub(source.x);
        let angle_dy_i32 = target.y.wrapping_sub(source.y);
        let angle_dx_f32 = x87_store_f32(X87Chop53::load_i32(angle_dx_i32));
        let angle_squared = X87Chop53::add(
            X87Chop53::mul(load_f32(angle_dx_f32), load_f32(angle_dx_f32)),
            X87Chop53::mul(
                X87Chop53::load_i32(angle_dy_i32),
                X87Chop53::load_i32(angle_dy_i32),
            ),
        );
        let angle_length_bits =
            sqrt_approx_f32(angle_squared).expect("Wave angle squared length is finite");
        let angle_length = load_f32(angle_length_bits);

        let (_, mut angle) = if horizontal == 0 {
            (0, acos.acos_lookup_nan())
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
            acos.acos_lookup(ratio)
        };
        if target.y < source.y {
            angle = X87Chop53::neg(angle);
        }
        // `Math::SinFromTable` / `CosFromTable` of the stored binary32 angle.
        let angle = load_f32(x87_store_f32(angle));
        let sin = x87_store_f32(trig.sin_from_table(angle));
        let cos = x87_store_f32(trig.cos_from_table(angle));

        // Native transforms all four local points before translating/storing any
        // world field. Keep that boundary visible.
        // Matrix primitives have one owner, shared with GetFLH and voxel/bounce
        // consumers. RotateZ5AF1A0 and TransformPoint5AFB80 keep explicit stores.
        let matrix = crate::util::native_affine::rotate_z(
            glam::Mat4::IDENTITY,
            f32::from_bits(sin.bits()),
            f32::from_bits(cos.bits()),
        );
        let transformed = std::array::from_fn::<_, 4, _>(|index| {
            let point = glam::Vec3::new(
                f32::from_bits(x87_store_f32(X87Chop53::load_i32(local_x[index])).bits()),
                f32::from_bits(x87_store_f32(X87Chop53::load_i32(local_y[index])).bits()),
                f32::from_bits(local_z[index].bits()),
            );
            crate::util::native_affine::transform_point(matrix, point)
                .expect("finite Wave vertices remain in native affine range")
        });
        let source_y_f32 = x87_store_f32(X87Chop53::load_i32(source.y));
        let mut world = [ProjectileCoord::new(0, 0, 0); 4];
        for (index, vertex) in transformed.into_iter().enumerate() {
            let world_x = x87_ftol_i32(X87Chop53::add(
                X87Chop53::load_i32(source.x),
                load_f32(NativeF32Bits::from_bits(vertex.x.to_bits())),
            ));
            let source_y = if index == 0 {
                X87Chop53::load_i32(source.y)
            } else {
                load_f32(source_y_f32)
            };
            let world_y = x87_ftol_i32(X87Chop53::add(
                source_y,
                load_f32(NativeF32Bits::from_bits(vertex.y.to_bits())),
            ));
            let world_z = x87_ftol_i32(load_f32(NativeF32Bits::from_bits(vertex.z.to_bits())));
            world[index] = ProjectileCoord::new(world_x, world_y, world_z);
        }

        world
    };
    let edges = WaveEdgeGeometry {
        firer_a: world[0],
        firer_b: world[1],
        target_a: world[2],
        target_b: world[3],
    };
    let source_screen = project_wave_point(source);
    let firer_b_screen = project_wave_point(edges.firer_b);
    WaveGeometry {
        source,
        target,
        edges,
        direction_octant: direction_from_projected(source_screen, firer_b_screen),
    }
}

#[cfg(test)]
#[path = "wave_magnetic_tests.rs"]
mod magnetic_tests;

#[cfg(test)]
#[path = "wave_native_tests.rs"]
mod native_tests;
