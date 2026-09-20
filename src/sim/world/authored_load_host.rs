//! Simulation-owned effects for the fresh authored OverlayPack/Recalc corridor.

use crate::assets::asset_manager::AssetManager;
use crate::map::authored_overlay::{
    AuthoredOverlayCellRef, AuthoredOverlayLoadHost, MapLoadDirtyKind, NativeOverlayCellTarget,
};
use crate::map::resolved_terrain::{
    AutomaticTubeAllocation, AutomaticTubeRequest, TerrainTileAnimation,
};
use crate::rules::art_data::{AnimAssetBindError, ArtRegistry};
use crate::sim::anim_class::{AnimDrawRuntime, AnimSpawnError, AnimWorldCoord};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;

use super::load_object_lifecycle::{LoadOverlayHandle, LoadOverlayLifecycleError};

const TILE_ANIM_DRAW_FLAGS: u32 = 0x1600;
const CELL_CENTRE_LEPTONS: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32 / 2;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SimulationAuthoredLoadError {
    #[error("fresh authored load has no native-ID cursor")]
    MissingNativeIdentity,
    #[error(transparent)]
    OverlayLifecycle(#[from] LoadOverlayLifecycleError),
    #[error(transparent)]
    Anim(#[from] AnimSpawnError),
    #[error(transparent)]
    AnimAsset(#[from] AnimAssetBindError),
}

/// Narrow host over the one staged Simulation. Geometry, overlay identity, and
/// Recalc projection remain map-owned by `AuthoredOverlayFinalizer`.
pub(crate) struct SimulationAuthoredLoadHost<'a> {
    sim: &'a mut Simulation,
    art: &'a mut ArtRegistry,
    assets: &'a AssetManager,
    theater_ext: &'a str,
    theater_name: &'a str,
}

impl<'a> SimulationAuthoredLoadHost<'a> {
    pub(crate) fn new(
        sim: &'a mut Simulation,
        art: &'a mut ArtRegistry,
        assets: &'a AssetManager,
        theater_ext: &'a str,
        theater_name: &'a str,
    ) -> Self {
        Self {
            sim,
            art,
            assets,
            theater_ext,
            theater_name,
        }
    }

    fn next_native_id(&mut self) -> Result<i32, SimulationAuthoredLoadError> {
        self.sim
            .native_unique_ids
            .as_mut()
            .map(|cursor| cursor.next_id())
            .ok_or(SimulationAuthoredLoadError::MissingNativeIdentity)
    }

    fn real_coord(cell: AuthoredOverlayCellRef) -> Option<(u16, u16)> {
        matches!(cell.target, NativeOverlayCellTarget::Real(_))
            .then_some((cell.coord.0 as u16, cell.coord.1 as u16))
    }

    fn spawn_load_anim(
        &mut self,
        anim_name: &str,
        world: AnimWorldCoord,
        mut descriptor: AnimClassSpawnDescriptor,
    ) -> Result<u64, SimulationAuthoredLoadError> {
        let mut roots: Vec<String> = self.art.scheduler_anim_types().iter().cloned().collect();
        roots.push(anim_name.to_ascii_uppercase());
        roots.sort();
        roots.dedup();
        // VERA resolves the exact newly reached map animation root lazily,
        // before the native constructor can consume an ID. This preserves the
        // load failure boundary without binding unused theater declarations.
        //
        // RESIDUAL: the roots are the whole bound set, tolerantly bound types
        // included, and this pass is strict over their `Next=`/`TrailerAnim=`
        // chains. A tolerantly bound type whose chained type has no sprite
        // would fail the load here. Trigger: modded art only; none of retail's
        // nine chain keys starts from a tolerant root. Effect: map load error.
        self.art.bind_scheduler_anim_assets(
            &roots,
            self.assets,
            self.theater_ext,
            self.theater_name,
        )?;
        descriptor.type_name = self.sim.interner.intern(anim_name);
        let native_unique_id = self.next_native_id()?;
        self.sim
            .spawn_load_anim_at_world(self.art, descriptor, world, native_unique_id)
            .map_err(Into::into)
    }
}

impl AuthoredOverlayLoadHost for SimulationAuthoredLoadHost<'_> {
    type Handle = LoadOverlayHandle;
    type Error = SimulationAuthoredLoadError;

    fn try_construct_overlay(
        &mut self,
        overlay_id: u8,
        cell: (u16, u16),
    ) -> Result<Option<Self::Handle>, Self::Error> {
        if self.sim.native_unique_ids.is_none() {
            return Err(SimulationAuthoredLoadError::MissingNativeIdentity);
        }
        let stable_id = self.sim.allocate_stable_id();
        let (objects, cursor) = (
            &mut self.sim.load_objects,
            self.sim
                .native_unique_ids
                .as_mut()
                .expect("native cursor checked above"),
        );
        objects
            .construct_overlay(stable_id, overlay_id, cell, || cursor.next_id())
            .map(Some)
            .map_err(Into::into)
    }

    fn begin_mark(
        &mut self,
        handle: Self::Handle,
        _anchor: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error> {
        self.sim.load_objects.begin_mark(handle)?;
        Ok(())
    }

    fn next_scenario_raw(&mut self) -> u32 {
        self.sim.scenario_rng.next_u32()
    }

    fn allocate_automatic_tube(
        &mut self,
        _request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error> {
        Ok(AutomaticTubeAllocation::Allocated {
            native_unique_id: self.next_native_id()?,
            registry_append_allowed: true,
        })
    }

    fn publish_dirty(
        &mut self,
        kind: MapLoadDirtyKind,
        cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error> {
        let Some(coord) = Self::real_coord(cell) else {
            return Ok(());
        };
        match kind {
            MapLoadDirtyKind::BaseMarkTactical | MapLoadDirtyKind::WallTactical => {
                self.sim.tactical_dirty_cells.push(coord);
            }
            MapLoadDirtyKind::WallRadar => self.sim.mark_radar_terrain_dirty_cells([coord]),
        }
        Ok(())
    }

    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error> {
        let descriptor = AnimClassSpawnDescriptor {
            type_name: Default::default(),
            rx: request.rx,
            ry: request.ry,
            sub_x: crate::util::fixed_math::SimFixed::from_num(
                request
                    .world_x
                    .wrapping_sub(i32::from(request.rx).wrapping_mul(256)),
            ),
            sub_y: crate::util::fixed_math::SimFixed::from_num(
                request
                    .world_y
                    .wrapping_sub(i32::from(request.ry).wrapping_mul(256)),
            ),
            z: u8::try_from(
                request
                    .world_z
                    .div_euclid(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
            )
            .unwrap_or(0),
            delay: 0,
            loop_count: -1,
            draw_flags: TILE_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            use_cell_drawer: true,
            terrain_attached: true,
            draw_runtime: AnimDrawRuntime::default(),
        };
        let id = self.spawn_load_anim(
            &request.anim_name,
            AnimWorldCoord {
                x: request.world_x,
                y: request.world_y,
                z: request.world_z,
            },
            descriptor,
        )?;
        let applied = self
            .sim
            .set_terrain_anim_z_adjust_after_construction(id, request.z_adjust);
        debug_assert!(applied);
        Ok(())
    }

    fn merge_wall_zone(&mut self, _cell: AuthoredOverlayCellRef) -> Result<(), Self::Error> {
        // The map owner already applies the synchronous cell-zone merge. The
        // global zone/connectivity rebuild is deliberately deferred until the
        // final post-object sweep.
        Ok(())
    }

    fn observe_blocker_count_increment(
        &mut self,
        _cell: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn spawn_cell_anim(
        &mut self,
        _handle: Self::Handle,
        anim_name: &str,
        cell: AuthoredOverlayCellRef,
        world_z: i32,
    ) -> Result<(), Self::Error> {
        let Some((rx, ry)) = Self::real_coord(cell) else {
            return Ok(());
        };
        let world = AnimWorldCoord {
            x: i32::from(rx)
                .wrapping_mul(crate::util::lepton::LEPTONS_PER_CELL_I32)
                .wrapping_add(CELL_CENTRE_LEPTONS),
            y: i32::from(ry)
                .wrapping_mul(crate::util::lepton::LEPTONS_PER_CELL_I32)
                .wrapping_add(CELL_CENTRE_LEPTONS),
            z: world_z,
        };
        let descriptor = AnimClassSpawnDescriptor::new(
            Default::default(),
            rx,
            ry,
            crate::util::fixed_math::SimFixed::from_num(CELL_CENTRE_LEPTONS),
            crate::util::fixed_math::SimFixed::from_num(CELL_CENTRE_LEPTONS),
            u8::try_from(world_z.div_euclid(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS))
                .unwrap_or(0),
        );
        self.spawn_load_anim(anim_name, world, descriptor)?;
        Ok(())
    }

    fn finish_common(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
        self.sim.load_objects.finish_common(handle)?;
        Ok(())
    }

    fn finish_slope_survivor(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
        self.sim.load_objects.finish_slope_survivor(handle)?;
        Ok(())
    }

    fn drain_deferred(&mut self) -> Result<(), Self::Error> {
        self.sim.load_objects.drain_deferred()?;
        Ok(())
    }
}
