//! Binding ART and asset data to a `RuleSet`: the ART registry, animation,
//! voxel, terrain, buildup, rubble and alpha-image assets, and their test
//! setters.

use std::collections::BTreeMap;

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::voxel_anim_type::VoxelAnimTypeId;
#[cfg(any(test, feature = "test-support"))]
use crate::rules::weapon_type::WeaponType;

use super::{RuleSet, TypeHandle};

impl RuleSet {
    /// Bind ART metadata and project QueueingCell/DockingOffset and presentation.
    /// Building Foundation already came from the ordered native rules processor;
    /// binding assets cannot execute a ReadINI for a constructor-only type.
    pub fn install_art_data(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.art_registry
            .apply_anim_type_read_states(&self.anim_type_art_read_states);
        self.art_registry
            .apply_anim_type_image_states(&self.anim_type_images);
        self.art_registry
            .apply_anim_type_references(&self.anim_type_references);
        self.project_art_data();
    }

    fn project_art_data(&mut self) {
        self.voxel_assets = Default::default();
        let art = &self.art_registry;
        // Projectile ART is already projected from the per-pass registry owner.
        // A final-image reread here would lose omission/cache/default semantics.
        let ai_base_spacing = self.ai_base_spacing;
        let mut dock_patched: u32 = 0;
        let mut buildings_checked: u32 = 0;
        for obj in self.object_list.iter_mut() {
            obj.project_silo_damage(art);
            // Resolve the art.ini section: use Image= override if present,
            // otherwise fall back to the object ID itself.
            let art_key: &str = &obj.image;
            let entry = art.get(art_key).or_else(|| art.get(&obj.id));
            // Every TechnoType's ReadINI reads CanBeHidden from the ART
            // section of its own ID (0x007121EB); art is not layered.
            obj.can_be_hidden = art.can_be_hidden(&obj.id);
            // Infantry Crawls and discharge frames come from the per-pass
            // ART reads (`InfantryArt`), not this final-image projection.
            if obj.category != crate::rules::object_type::ObjectCategory::Building {
                continue;
            }
            buildings_checked += 1;
            obj.hidden_occupancy = art.building_hidden_occupancy_profile(&obj.id, art_key);
            // +EF0 and ToOverlay are supplied by the sole pass owner in
            // from_processed_rules. Repeating461225 here changes late
            // constructor-only types and loses retained state when
            // installation ART differs from the pass.
            if let Some(entry) = entry {
                // Merge QueueingCell from art.ini (TibSun legacy dock system).
                if entry.queueing_cell != [0, 0] {
                    obj.queueing_cell = entry.queueing_cell;
                    dock_patched += 1;
                }
                // Multi-pad merge: when art declares at least one DockingOffset,
                // size pads to NumberOfDocks (from rules.ini), zero-padding missing
                // indices and truncating excess. Mirrors the original game's
                // memory layout where the array is sized by NumberOfDocks and
                // unspecified DockingOffset%d slots default to (0,0,0).
                //
                // When art declares ZERO DockingOffset entries (retail refineries
                // like GAREFN/NAREFN/YAREFN), obj.pads is left empty so existing
                // fallback paths (e.g. refinery_pad_cell's rightmost-column
                // anchor) keep firing. Otherwise zero-padding would silently
                // shift refinery dock positions, which is out of scope here.
                //
                // Only slots below the contact capacity can be selected, so
                // padding stops at VERA's `DOCK_CONTACT_LIMIT`.
                if !entry.pads.is_empty() {
                    let n = obj
                        .number_of_docks
                        .clamp(0, crate::rules::object_type::DOCK_CONTACT_LIMIT)
                        as usize;
                    obj.pads = entry.pads.iter().take(n).copied().collect();
                    while obj.pads.len() < n {
                        obj.pads.push(crate::rules::object_type::DockPad {
                            lepton_offset: (0, 0, 0),
                        });
                    }
                }
            }
            // The native reveal-time gate sees the final loaded foundation.
            // Rules parsing runs before ART supplies stock Building foundations,
            // so overwrite the provisional profile after effective ART resolution.
            obj.base_reservation_spacing = obj
                .base_reservation_writer_eligible()
                .then_some(ai_base_spacing);
        }
        log::info!(
            "Merged art.ini → RuleSet: {} dock cells ({} buildings checked)",
            dock_patched,
            buildings_checked,
        );
        self.rebuild_animation_sequences();
        // The anims' start and stop sounds came with the ART.
        self.rebuild_sound_names();
    }

    /// Read the same bound ART owner used during construction and gameplay.
    pub fn art(&self) -> &crate::rules::art_data::ArtRegistry {
        &self.art_registry
    }

    /// Registered AnimType identities selected by Phobos' gather overlay.
    /// Keep the type identity: its existing image owner resolves retained load
    /// requests, Image aliases and theater filenames.
    #[cfg(feature = "phobos")]
    pub fn harvest_overlay_anim_types(&self) -> Vec<&str> {
        let mut anims = Vec::new();
        for object in &self.object_list {
            if object.category != ObjectCategory::Vehicle {
                continue;
            }
            for anim in &object.ore_gathering().anims {
                if !anims.contains(&anim.as_str()) {
                    anims.push(anim.as_str());
                }
            }
        }
        anims
    }

    /// AnimTypeArray8B4154's retained constructor order, including types
    /// discovered before the master list and by later rules/ART readers.
    /// TAction41 indexes this registry at6E2357; `[Animations]` numeric
    /// labels and the membership-only BTreeSet are not ordinal catalogs.
    pub fn anim_type_name_by_index(&self, index: i32) -> Option<&str> {
        self.anim_type_art_read_states
            .get(usize::try_from(index).ok()?)
            .map(|(name, _)| name.as_str())
    }

    /// Identity consumed by the shared animation/asset owners for an indexed
    /// pointer (Pickup483371, TAction6E2357, Team6EF610). Unlike the stored
    /// name, this preserves distinct slots with colliding24-byte IDs.
    pub fn anim_type_runtime_key_by_index(&self, index: i32) -> Option<std::borrow::Cow<'_, str>> {
        crate::rules::native_processing::anim_type_runtime_key(
            self.anim_type_art_read_states
                .iter()
                .map(|(name, _)| name.as_str()),
            usize::try_from(index).ok()?,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn anim_type_art_read_states(&self) -> &[(String, bool)] {
        &self.anim_type_art_read_states
    }

    pub fn bind_scheduler_anim_assets(
        &mut self,
        roots: &[String],
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        self.art_registry
            .bind_scheduler_anim_assets(roots, assets, theater_ext, theater_name);
    }

    pub fn bind_anim_class_assets(
        &mut self,
        roots: &[String],
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> usize {
        self.art_registry
            .bind_anim_class_assets(roots, assets, theater_ext, theater_name)
    }

    pub fn populate_anim_frame_dims(
        &mut self,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> (u32, u32) {
        self.art_registry
            .populate_anim_frame_dims(assets, theater_ext, theater_name)
    }

    /// Extend the bound closure before an authored-load constructor spends its
    /// native ID. The constructor then borrows this same owner for Reveal/Start
    /// (AnimClass421EA0, Unlimbo5F4EC0, Start424CE0).
    pub fn bind_authored_load_anim(
        &mut self,
        name: &str,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        let mut roots: Vec<String> = self
            .art_registry
            .scheduler_anim_types()
            .iter()
            .cloned()
            .collect();
        roots.push(name.to_ascii_uppercase());
        roots.sort();
        roots.dedup();
        self.bind_scheduler_anim_assets(&roots, assets, theater_ext, theater_name);
    }

    /// Synthetic fixtures that previously projected ART then restored the raw
    /// registry deliberately bypass native read admission. Move that fixture
    /// once while sharing the production projection, without clone publication.
    /// The AnimTypes' Type references still come from the ordered Rules
    /// processor, as in [`Self::install_art_data`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_art_fixture(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.art_registry
            .apply_anim_type_references(&self.anim_type_references);
        self.project_art_data();
    }

    /// Raw synthetic fixture replacement: no native admission or field
    /// projection, except the derived `SiloDamage=` each type keeps, which
    /// follows the new ART.
    #[cfg(any(test, feature = "test-support"))]
    pub fn replace_art_registry_for_test(&mut self, art: crate::rules::art_data::ArtRegistry) {
        self.art_registry = art;
        self.voxel_assets = Default::default();
        for object in &mut self.object_list {
            object.project_silo_damage(&self.art_registry);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_anim_frame_count_for_test(&mut self, name: &str, raw_count: i32) {
        self.art_registry
            .bind_anim_frame_count_for_test(name, raw_count);
    }

    /// The SHP header height `+4` asset binding would read for `name`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn bind_anim_shp_height_for_test(&mut self, name: &str, raw_height: i32) {
        self.art_registry
            .bind_anim_shp_height_for_test(name, raw_height);
    }

    /// A weapon's stored speed (after Process's postpass), as a native
    /// fixture writes it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_weapon_speed_for_test(&mut self, id: &str, speed: i32) {
        if let Some(weapon) = self.weapon_mut_for_test(id) {
            weapon.speed = speed;
        }
    }

    /// The weapon [`Self::weapon`] answers for `id`, to rewrite. Its handle
    /// stays the same, so the types' resolved records stay current.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn weapon_mut_for_test(&mut self, id: &str) -> Option<&mut WeaponType> {
        let handle = self.weapon_handle(id)?;
        Some(&mut self.weapon_types[handle.0 as usize])
    }

    /// Supply original fixture vectors that the real sound reader cannot
    /// produce (notably a nonempty vector containing native sound ID -1,
    /// represented by an empty name). This is an explicit input boundary,
    /// not ReadSoundList or sound-catalog binding evidence.
    #[cfg(any(test, feature = "test-support"))]
    pub fn supply_selection_voice_vectors_for_test(
        &mut self,
        type_id: &str,
        normal: Vec<String>,
        enslaved: Vec<String>,
    ) {
        let handle = self
            .type_handle(type_id)
            .expect("native fixture type exists");
        let object = &mut self.object_list[handle.0 as usize];
        object.voice_select = normal;
        object.voice_select_enslaved = enslaved;
    }

    /// Apply a native post-reader TechnoType Size override to a test fixture.
    /// The fixture owns the exact binary64 value written to `Type+0x380`;
    /// this does not model INI parsing or change the production Rules reader.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_object_size_for_test(&mut self, type_id: &str, size: f64) {
        let handle = self
            .type_handle(type_id)
            .expect("native fixture type exists");
        self.object_list[handle.0 as usize].size = size;
    }

    /// Write a type's slot-9 names (`BuildingType +0x11B0`, `+0x11C0`) as the
    /// native slot corpora do, past ReadINI's empty-damaged copy.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_turret_anim_names_for_test(&mut self, id: &str, normal: &str, damaged: &str) {
        let name = |name: &str| (!name.is_empty()).then(|| name.to_owned());
        let object = self
            .object_list
            .iter_mut()
            .find(|object| object.id == id)
            .unwrap();
        object.turret_anim = name(normal);
        object.turret_anim_damaged = name(damaged);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn art_entry_mut_for_test(
        &mut self,
        name: &str,
    ) -> Option<&mut crate::rules::art_data::ArtEntry> {
        self.voxel_assets = Default::default();
        self.art_registry.get_mut(name)
    }

    /// The overlay images available for authored map placement. Declared
    /// pictures use the same bound admission that multiplayer certifies.
    pub fn resolved_overlay_shp_ids(
        &self,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> std::collections::BTreeSet<u8> {
        crate::rules::effect_asset_catalog::resolved_overlay_shp_ids(
            self.overlay_registry(),
            &self.effect_assets,
            assets,
            theater_ext,
            theater_name,
        )
    }

    /// Resolve particle image SHP frame counts from the active theater assets
    /// without constructing a renderer atlas.
    pub fn bind_effect_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        self.effect_assets = crate::rules::effect_asset_catalog::EffectAssetCatalog::bind(
            self,
            asset_manager,
            theater_ext,
            theater_name,
        );
        self.voxel_assets =
            crate::rules::voxel_asset_catalog::VoxelAssetCatalog::bind(self, asset_manager);
    }

    pub fn loaded_voxel_model(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::voxel_asset_catalog::LoadedVoxelModel> {
        let object = self.object(type_id)?;
        self.voxel_assets.model(object.category, &object.id)
    }

    pub fn voxel_anim_model(
        &self,
        id: VoxelAnimTypeId,
    ) -> Option<&crate::rules::voxel_asset_catalog::VoxelPart> {
        self.voxel_assets.voxel_anim(id.0 as usize)
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): the declared
    /// model building `type_id` draws for its voxel guns. None when its art
    /// declares none, so the VXL path draws; `Some(None)` when the model was
    /// refused and the guns draw nothing.
    pub fn building_gun_model(
        &self,
        type_id: &str,
    ) -> Option<Option<&std::sync::Arc<crate::rules::voxel_asset_catalog::DeclaredModel>>> {
        self.voxel_assets.building_gun_model(type_id)
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): whether the
    /// voxel unit, aircraft or building gun `type_id` draws from a declared
    /// `Model=` (admitted or refused) and never from its VXL, so the unit
    /// atlas need not load it. False for retail art.
    pub fn draws_declared_model(&self, type_id: &str) -> bool {
        if self.building_gun_model(type_id).is_some() {
            return true;
        }
        self.object(type_id).is_some_and(|object| {
            object.voxel_image_load().is_some_and(|image| {
                self.art()
                    .resolve_declared_model(&object.id, image)
                    .is_some()
            })
        })
    }

    /// [`Self::building_gun_model`] for the voxel projectile type `type_id`.
    pub fn projectile_model(
        &self,
        type_id: &str,
    ) -> Option<Option<&std::sync::Arc<crate::rules::voxel_asset_catalog::DeclaredModel>>> {
        self.voxel_assets.projectile_model(type_id)
    }

    /// The declared model the VoxelAnim type `id` draws in place of its VXL:
    /// its own `Image=` art's, or the part it shares of a `ShareSource=`
    /// type's ([`crate::rules::voxel_asset_catalog::DebrisModel`]). None
    /// for the VXL path; a refused model draws nothing.
    pub fn voxel_anim_declared_model(
        &self,
        id: VoxelAnimTypeId,
    ) -> Option<&crate::rules::voxel_asset_catalog::DebrisModel> {
        self.voxel_assets.voxel_anim_declared_model(id.0 as usize)
    }

    pub(crate) fn voxel_models_bound(&self) -> bool {
        self.voxel_assets.is_bound()
    }

    /// VERA-only, experimental (`docs/picture-model-art.md`): whether any
    /// type draws an admitted `Model=` ([`crate::rules::voxel_asset_catalog::LoadedVoxelModel::declared_model`],
    /// building guns, voxel projectiles and debris included).
    /// False for retail data, so the renderer does no model work.
    pub fn declares_models(&self) -> bool {
        self.voxel_assets.declares_models()
    }

    /// Techno6F9E10's loaded-main-VXL predicate for the type `object` names,
    /// never an ART declaration or renderer-cache query. Unbound/missing/
    /// malformed models answer false.
    pub(crate) fn object_has_loaded_voxel(&self, object: TypeHandle) -> bool {
        self.voxel_assets.contains(object)
    }

    /// Original main-VXL half sizes retained by the same immutable binding as
    /// IsVoxel6F9E10. Draw_Matrix and GetFLH consume this input independently
    /// of the renderer's model or pose caches.
    pub(crate) fn object_voxel_half_sizes(&self, type_id: &str) -> Option<[f32; 2]> {
        self.voxel_assets.half_sizes(type_id)
    }

    /// Explicit loaded model prior for headless component fixtures. Production
    /// callers bind immutable admission from the active physical assets.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn supply_loaded_voxel_for_test(&mut self, type_id: &str) {
        let object = self
            .type_handle(type_id)
            .expect("loaded type fixture must exist");
        self.voxel_assets.supply_loaded(type_id, object);
    }

    /// Bind native TerrainType frame-0 RadarColor inputs and raw SHP frame
    /// counts for TIBTRE timing from the active theater's asset bytes.
    pub fn bind_terrain_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        phase: crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase,
    ) {
        self.terrain_assets = crate::rules::terrain_asset_catalog::TerrainAssetCatalog::bind(
            &mut self.terrain_object_types,
            &self.art_registry,
            asset_manager,
            theater_ext,
            phase,
        );
    }

    pub fn terrain_shape_filename(&self, name: &str) -> Option<&str> {
        self.terrain_assets.shape_filename(name)
    }

    /// Resolve every building type's Buildup SHP from the active theater's
    /// assets into its construction control, without a renderer atlas.
    pub fn bind_building_buildup_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
    ) {
        self.buildup_assets = crate::rules::buildup_asset_catalog::BuildupAssetCatalog::bind(
            self,
            asset_manager,
            theater_name,
        );
    }

    /// Resolve every `LeaveRubble=yes` building type's rubble frames from the
    /// active theater's assets, without a renderer atlas.
    pub fn bind_building_rubble_assets(
        &mut self,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) {
        self.rubble_assets = crate::rules::building_rubble::RubbleAssetCatalog::bind(
            self,
            asset_manager,
            theater_ext,
            theater_name,
        );
    }

    /// A building type's rubble art; `None` when it leaves no rubble
    /// ([`crate::rules::building_rubble`]).
    pub fn building_rubble_art(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::building_rubble::RubbleArt> {
        self.rubble_assets.art(type_id)
    }

    /// Every bound rubble art, for presentation preloading.
    pub fn building_rubble_arts(
        &self,
    ) -> impl Iterator<Item = &crate::rules::building_rubble::RubbleArt> {
        self.rubble_assets.iter()
    }

    /// Install one building type's rubble art (fixtures without assets).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_building_rubble_art_for_test(
        &mut self,
        type_id: &str,
        art: crate::rules::building_rubble::RubbleArt,
    ) {
        self.rubble_assets.insert_for_test(type_id, art);
    }

    /// A building type's construction control (`Type+0xF04`: first frame,
    /// frame count, rate); the constructor's `{0, 1, 0}` when its Buildup SHP
    /// is unbound.
    pub fn buildup_control(&self, type_id: &str) -> [i32; 3] {
        self.buildup_assets.control(type_id)
    }

    /// Ares: a building type's sale frame rate (`SellFrameDuration`, 0 while
    /// its Buildup SHP is unbound).
    pub fn buildup_sell_rate(&self, type_id: &str) -> i32 {
        self.buildup_assets.sell_rate(type_id)
    }

    /// Whether a building type's Buildup SHP is bound (`+0x6E9`,
    /// [`crate::rules::buildup_asset_catalog::BuildupAssetCatalog::has_buildup`]).
    pub fn has_buildup(&self, type_id: &str) -> bool {
        self.buildup_assets.has_buildup(type_id)
    }

    /// Install one building type's construction control (fixtures without
    /// assets).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_buildup_control_for_test(&mut self, type_id: &str, control: [i32; 3]) {
        self.buildup_assets.insert_for_test(type_id, control);
    }

    /// Install one building type's Ares sale rate (fixtures without assets).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_buildup_sell_rate_for_test(&mut self, type_id: &str, rate: i32) {
        self.buildup_assets.insert_sell_rate_for_test(type_id, rate);
    }

    /// Raw SHP header count used by `TerrainClass::AI` midpoint timing.
    pub fn terrain_spawner_frame_count(&self, name: &str) -> Option<u16> {
        self.terrain_assets.frame_count(name)
    }

    /// Load every ObjectType `AlphaImage=` SHP the folded rules name
    /// ([`crate::rules::alpha_image_assets`]).
    pub fn bind_alpha_images(&mut self, assets: &crate::assets::asset_manager::AssetManager) {
        self.alpha_images = crate::rules::alpha_image_assets::AlphaImageCatalog::bind(self, assets);
    }

    /// The loaded AlphaImage a type's `AlphaImage=` names: the type's +0xAC
    /// pointer, `None` where it stays null.
    pub fn alpha_image(
        &self,
        name: &str,
    ) -> Option<&std::sync::Arc<crate::rules::alpha_image_assets::AlphaImage>> {
        self.alpha_images.get(name)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn supply_alpha_image_for_test(
        &mut self,
        name: &str,
        image: crate::rules::alpha_image_assets::AlphaImage,
    ) {
        self.alpha_images.insert_for_test(name, image);
    }

    pub(super) fn rebuild_animation_sequences(&mut self) {
        self.animation_sequences =
            crate::rules::animation_sequence::build_animation_sequence_catalog(self);
        self.index_animation_sequences();
    }

    fn index_animation_sequences(&mut self) {
        self.animation_sequence_by_handle = self
            .object_list
            .iter()
            .map(|object| {
                // Native InfantryType+E3C belongs to this concrete type, even
                // when a Building/Unit registry contains the same stored ID.
                object
                    .infantry_sequence_bank()
                    .map(|bank| bank.sequence_set())
                    .or_else(|| self.animation_sequences.get(&object.id).cloned())
            })
            .collect();
    }

    pub(crate) fn animation_sequences(
        &self,
    ) -> &BTreeMap<String, crate::rules::animation_sequence::SequenceSet> {
        &self.animation_sequences
    }

    pub fn animation_sequence(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
        self.animation_sequence_by_handle(self.type_handle(type_id)?)
    }

    /// Select a concrete class's retained bank, including duplicate names in
    /// another registry. Native Infantry51D707/518DCF uses that Type pointer.
    pub fn animation_sequence_in_category(
        &self,
        category: ObjectCategory,
        type_id: &str,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
        self.animation_sequence_by_handle(self.type_handle_in_category(category, type_id)?)
    }

    /// The sequences of the object `handle` names, as
    /// [`Self::animation_sequence`] answers for its name.
    pub fn animation_sequence_by_handle(
        &self,
        handle: TypeHandle,
    ) -> Option<&crate::rules::animation_sequence::SequenceSet> {
        self.animation_sequence_by_handle[handle.0 as usize].as_ref()
    }

    pub(crate) fn unit_shp_read_state(
        &self,
        type_id: &str,
    ) -> Option<&crate::rules::shp_vehicle_sequence::UnitShpReadState> {
        self.unit_shp_read_state_of(self.object(type_id)?)
    }

    /// [`Self::unit_shp_read_state`] for a type already in hand.
    pub fn unit_shp_read_state_of(
        &self,
        object: &ObjectType,
    ) -> Option<&crate::rules::shp_vehicle_sequence::UnitShpReadState> {
        self.unit_shp_read_states.get(&object.id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_effect_frame_count_for_test(&mut self, name: &str, raw: u16, available: u16) {
        self.effect_assets.set_for_test(name, raw, available);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_terrain_spawner_frame_count_for_test(
        &mut self,
        name: &str,
        frame_count: u16,
    ) {
        self.terrain_assets.set_for_test(name, frame_count);
    }
}
