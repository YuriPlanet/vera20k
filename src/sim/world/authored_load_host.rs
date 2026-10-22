//! Simulation-owned effects for the fresh authored OverlayPack/Recalc corridor.

use crate::assets::asset_manager::AssetManager;
use crate::map::authored_overlay::{
    AuthoredOverlayCellRef, AuthoredOverlayLoadHost, MapLoadDirtyKind, NativeOverlayCellTarget,
};
use crate::map::overlay_mark::OverlayCellAnim;
use crate::map::resolved_terrain::{
    AutomaticTubeAllocation, AutomaticTubeRequest, ResolvedTerrainGrid, TerrainTileAnimation,
};
use crate::rules::art_data::AnimAssetBindError;
use crate::sim::anim_class::{
    AnimConstructorInput, AnimDrawRuntime, AnimSpawnError, AnimWorldCoord,
};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::overlay_mark_host::OverlayCellAnimGround;
use crate::sim::world::Simulation;

use super::load_object_lifecycle::{LoadOverlayHandle, LoadOverlayLifecycleError};

const TILE_ANIM_DRAW_FLAGS: u32 = 0x1600;

#[derive(Debug, thiserror::Error)]
pub enum SimulationAuthoredLoadError {
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
    rules: &'a mut crate::rules::ruleset::RuleSet,
    assets: &'a AssetManager,
    theater_ext: &'a str,
    theater_name: &'a str,
}

impl<'a> SimulationAuthoredLoadHost<'a> {
    pub(crate) fn new(
        sim: &'a mut Simulation,
        rules: &'a mut crate::rules::ruleset::RuleSet,
        assets: &'a AssetManager,
        theater_ext: &'a str,
        theater_name: &'a str,
    ) -> Self {
        Self {
            sim,
            rules,
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
        self.rules.bind_authored_load_anim(
            anim_name,
            self.assets,
            self.theater_ext,
            self.theater_name,
        )?;
        descriptor.type_name = self.sim.interner.intern(anim_name);
        let native_unique_id = self.next_native_id()?;
        self.sim
            .spawn_anim_at_world_with_constructor(
                self.rules,
                descriptor,
                world,
                AnimConstructorInput::AssignedIdentity(native_unique_id),
            )
            .map_err(Into::into)
    }
}

impl AuthoredOverlayLoadHost for SimulationAuthoredLoadHost<'_> {
    type Handle = LoadOverlayHandle;
    type Error = SimulationAuthoredLoadError;

    fn try_construct_overlay(
        &mut self,
        _overlay_id: u8,
        cell: (u16, u16),
    ) -> Result<Option<Self::Handle>, Self::Error> {
        if self.sim.native_unique_ids.is_none() {
            return Err(SimulationAuthoredLoadError::MissingNativeIdentity);
        }
        let stable_id = self.sim.substrate.allocate_stable_id();
        let (objects, cursor) = (
            &mut self.sim.load_objects,
            self.sim
                .native_unique_ids
                .as_mut()
                .expect("native cursor checked above"),
        );
        objects
            .construct_overlay(stable_id, cell, || cursor.next_id())
            .map(Some)
            .map_err(Into::into)
    }

    fn begin_mark(
        &mut self,
        handle: Self::Handle,
        _anchor: AuthoredOverlayCellRef,
    ) -> Result<(), Self::Error> {
        self.sim.load_objects.begin_mark(handle, None)?;
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
                self.sim.dirty_cells.mark_tactical(coord);
            }
            MapLoadDirtyKind::WallRadar => self.sim.dirty_cells.mark_radar([coord]),
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
            owner: None,
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
            .substrate
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
        request: OverlayCellAnim<'_>,
        terrain: &ResolvedTerrainGrid,
    ) -> Result<(), Self::Error> {
        self.rules.bind_authored_load_anim(
            request.anim,
            self.assets,
            self.theater_ext,
            self.theater_name,
        )?;
        let ground = OverlayCellAnimGround::read(terrain, &request);
        let native_unique_id = self.next_native_id()?;
        // The common constructor publishes the original cell's light read
        // into the same ordered initialization timeline as later buildings.
        self.sim.construct_overlay_cell_anim(
            self.rules,
            request,
            ground,
            AnimConstructorInput::AssignedIdentity(native_unique_id),
        )?;
        Ok(())
    }

    fn finish_common(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
        self.sim
            .load_objects
            .finish_common(handle, &mut self.sim.substrate.pending_delete)?;
        Ok(())
    }

    fn finish_slope_survivor(&mut self, handle: Self::Handle) -> Result<(), Self::Error> {
        self.sim.load_objects.finish_slope_survivor(handle)?;
        Ok(())
    }

    fn drain_deferred(&mut self) -> Result<(), Self::Error> {
        self.sim.process_pending_delete_with(
            self.rules,
            None,
            crate::sim::world::FrameEffects::default(),
            0,
            false,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::{
        art_data::ArtRegistry, ini_parser::IniFile, native_processing::RulesLayerStack,
        ruleset::RuleSet,
    };
    use crate::sim::world::display_layers::DisplayLayer;
    use crate::sim::{native_identity::NativeUniqueIdCursor, snapshot::GameSnapshot};

    fn load_rules() -> RuleSet {
        let art_ini = IniFile::from_str(
            "[LATE]\nLayer=ground\nShadow=no\nRate=900\nStartSound=TileStart\n\
             [MISSING]\nImage=ABSENT\n",
        );
        let processed =
            RulesLayerStack::new(IniFile::from_str("[Animations]\n0=LATE\n1=MISSING\n"))
                .process_with_fixed_art(&art_ini)
                .expect("process canonical ART read admission");
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art_ini));
        rules
    }

    #[test]
    fn lazy_authored_binding_reaches_constructor_display_sound_and_snapshot_owner() {
        let root = TestDirectory::new("authored-art-owner");
        // The bounds binder consumes this SHP header; no pixel decode is
        // involved in this synthetic constructor/ownership regression.
        root.write("LATE.SHP", &[0, 0, 1, 0, 1, 0, 2, 0]);
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let request = TerrainTileAnimation {
            rx: 1,
            ry: 2,
            anim_name: "LATE".into(),
            world_x: 384,
            world_y: 640,
            world_z: 208,
            z_adjust: -7,
        };
        let mut rules = load_rules();
        assert!(rules.art().scheduler_anim_types().is_empty());
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("LATE")
                .unwrap()
                .raw_shp_frame_count,
            None
        );
        let before_rules_hash = rules.simulation_config_hash();
        // Native snapshot load resets Scenario RNG to Seed(0). This fixture
        // consumes no Scenario draws, so use that seed to compare the full
        // world across the ordinary in-scenario restore handoff.
        let mut sim = Simulation::with_seed(0);
        sim.session
            .set_map_name_for_test("authored ART owner".into());
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(700));
        let before_rng = sim.rng_state();
        SimulationAuthoredLoadHost::new(&mut sim, &mut rules, &assets, "TEM", "TEMPERATE")
            .construct_terrain_attached_anim(&request)
            .expect("bind the map-discovered root before construction");

        let (&id, anim) = sim.anims().next().expect("one constructed terrain Anim");
        assert_eq!(sim.anims().count(), 1);
        assert_eq!(anim.native_unique_id(), 701);
        assert_eq!(anim.effective_end(), 2);
        assert_eq!(anim.z_adjust(), -7);
        assert!(anim.terrain_attached() && anim.use_cell_drawer() && anim.start_sound_active());
        assert_eq!(
            sim.anim_display_layer(id, &rules),
            Some(DisplayLayer::GROUND)
        );
        assert_eq!(sim.substrate.display.members(DisplayLayer::GROUND), &[id]);
        assert_eq!(sim.live_object_order_snapshot(), vec![id]);
        assert_eq!(
            sim.rng_state(),
            before_rng,
            "no RandomRate means no constructor draw"
        );
        assert!(sim.sound_events.iter().any(|event| matches!(event,
            crate::sim::world::SimSoundEvent::AnimationStarted { anim_id, .. } if *anim_id == id)));
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("LATE")
                .unwrap()
                .raw_shp_frame_count,
            Some(2)
        );
        assert!(rules.art().scheduler_anim_types().contains("LATE"));
        let bound_rules_hash = rules.simulation_config_hash();
        assert_ne!(
            bound_rules_hash, before_rules_hash,
            "bound assets enter the authoritative rules fingerprint"
        );

        let bytes =
            GameSnapshot::save_validated(&sim, 1234, bound_rules_hash, "authored ART owner", 0);
        assert!(
            GameSnapshot::load_validated(&bytes, 1234, before_rules_hash, "authored ART owner")
                .is_err()
        );
        let mut restored =
            GameSnapshot::load_validated(&bytes, 1234, bound_rules_hash, "authored ART owner")
                .expect("snapshot accepts the final bound owner")
                .sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
        assert_eq!(restored.rng_state(), sim.rng_state());
        assert_eq!(restored.state_hash(), sim.state_hash());
        assert_eq!(
            restored.anim_display_layer(id, &rules),
            Some(DisplayLayer::GROUND)
        );

        let before_failure = sim.state_hash();
        let mut missing = request.clone();
        missing.anim_name = "MISSING".into();
        assert!(
            SimulationAuthoredLoadHost::new(&mut sim, &mut rules, &assets, "TEM", "TEMPERATE")
                .construct_terrain_attached_anim(&missing)
                .is_err()
        );
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            701,
            "strict load failure precedes native ID allocation, unlike runtime spawn"
        );
        assert_eq!(sim.state_hash(), before_failure);
        assert_eq!(sim.rng_state(), before_rng);
        assert_eq!(rules.simulation_config_hash(), bound_rules_hash);

        // A fresh staged load does not inherit the previous match's bound set.
        let mut reload_rules = load_rules();
        let mut reload = Simulation::with_seed(0);
        reload
            .session
            .set_map_name_for_test("authored ART owner".into());
        reload.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(700));
        SimulationAuthoredLoadHost::new(
            &mut reload,
            &mut reload_rules,
            &assets,
            "TEM",
            "TEMPERATE",
        )
        .construct_terrain_attached_anim(&request)
        .unwrap();
        assert_eq!(reload_rules.simulation_config_hash(), bound_rules_hash);
        assert_eq!(reload.state_hash(), sim.state_hash());
    }
}
