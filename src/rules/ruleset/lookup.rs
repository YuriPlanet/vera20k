//! Lookups on a loaded `RuleSet`: object types, weapons, warheads and
//! projectiles by name or handle, costs, superweapons, particles, voxel
//! anims, prerequisites, the extension switches and the type counts.

use std::collections::HashMap;

use crate::rules::ares_prerequisites::GenericPrerequisite;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::particle_system_type::{ParticleSystemType, ParticleSystemTypeId};
use crate::rules::particle_type::{ParticleType, ParticleTypeId};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup};
use crate::rules::projectile_type::ProjectileType;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::voxel_anim_type::{VoxelAnimType, VoxelAnimTypeId};
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;

#[cfg(any(test, feature = "test-support"))]
use super::next_rule_set_serial;
use super::{
    HouseCostFactors, ProjectileHandle, ResolvedDebris, ResolvedWeapon, RuleSet, TypeHandle,
    TypeWeapons, WarheadHandle, WeaponHandle, WeaponParts, native_minutes_to_frames,
    upper_name_key,
};
#[cfg(any(test, feature = "test-support"))]
use super::{NAME_KEY_COPIES, TYPE_NAME_LOOKUPS};

impl RuleSet {
    /// Look up a game object by ID.
    /// Case-insensitive type-name lookup matching the original engine's
    /// find-or-allocate (stricmp-style) name resolution.
    ///
    /// Exact match first — O(1) and the normal path, since RA2 type IDs are
    /// consistently cased. Only on a case-mismatch miss does it scan for the
    /// unique case-insensitive match. Valid RA2 data never holds two names
    /// equal-ignoring-case (the original engine's case-insensitive find merges
    /// them), so the scan yields at most one hit and the result stays
    /// deterministic for lockstep.
    pub(super) fn lookup_ci<'a, T, S: std::hash::BuildHasher>(
        map: &'a HashMap<String, T, S>,
        id: &str,
    ) -> Option<&'a T> {
        map.get(id).or_else(|| {
            map.iter()
                .find_map(|(key, value)| key.eq_ignore_ascii_case(id).then_some(value))
        })
    }

    /// The C4 charge's fuse, `ftol(C4Delay * 900.0)`, which the plant starts
    /// the building's C4Timer with (`InfantryClass::Per_Cell_Process
    /// 0x0051A5E2..0x0051A608`). Under the process's chop control word retail
    /// `.03`, scanned through `float`, gives 26.
    pub fn c4_delay_frames(&self) -> i32 {
        native_minutes_to_frames(self.c4_delay)
    }

    /// The building's flash at a C4 plant, `ftol(C4Delay * 900.0 * 0.5)`
    /// (`0x0051A5B6..0x0051A5D0`): the 900.0 product stays on the x87 stack
    /// for the halving. Retail gives 13.
    pub fn c4_flash_frames(&self) -> i32 {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        X87::ftol_i32_low_masked(X87::mul(
            X87::mul(
                X87::load_f64(NativeF64Bits::from_bits(self.c4_delay.to_bits())),
                X87::load_i32(900),
            ),
            X87::load_f64(NativeF64Bits::HALF),
        ))
    }

    /// Test builds only: how many times this thread has looked a type's
    /// record up by name ([`Self::object`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn type_name_lookups() -> u64 {
        TYPE_NAME_LOOKUPS.with(std::cell::Cell::get)
    }

    /// Test builds only: how many uppercase copies of a type name this
    /// thread has made to look one up ([`upper_name_key`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn name_key_copies() -> u64 {
        NAME_KEY_COPIES.with(std::cell::Cell::get)
    }

    /// Resolve a type name to its handle, case-insensitively (engine parity).
    pub fn type_handle(&self, id: &str) -> Option<TypeHandle> {
        self.object_index.get(&*upper_name_key(id)).copied()
    }

    /// TAction106's active6DF5D1 and FlashCameo115's6E4150 compare IDs with exact case,
    /// scanning TechnoType::Array A8EB04/countA8EB10 backwards. This has
    /// different semantics from broad `object()` and subtype lookup.
    ///
    /// The old ObjectType projection merges repeated IDs in one category.
    /// Native can construct those from >24-byte factory inputs. If the last
    /// native match is one of these unrepresentable identities, return None;
    /// never redirect the mutation to an earlier/global broad-lookup winner.
    pub(crate) fn native_techno_type_exact(&self, id: &str) -> Option<&ObjectType> {
        self.native_techno_type_order
            .iter()
            .rev()
            .find(|(_, stored_id, _)| stored_id == id)
            .and_then(|(_, _, handle)| *handle)
            .map(|handle| self.object_by_handle(handle))
    }

    /// Ares 3.0 TechnoTypeClass::Find (Ares.dll 10007EA0): unless the name is
    /// `none` or `<none>` (CCINIClass::IsBlank 10003950), the first type of
    /// the same TechnoType array whose ID matches ignoring ASCII case
    /// (`strcmpi`). An unrepresentable first match answers None, as above.
    pub(crate) fn ares_find_techno_type(&self, id: &str) -> Option<&ObjectType> {
        if crate::rules::ini_parser::is_native_none_type_name(id) {
            return None;
        }
        // The first slot of the ID, indexed once: trigger events ask this
        // every frame (VERA-only; the same answer as the scan).
        self.ares_techno_type_first
            .get(&*upper_name_key(id))
            .and_then(|&slot| self.native_techno_type_order[slot].2)
            .map(|handle| self.object_by_handle(handle))
    }

    /// This set's identity: nonzero and distinct from every other `RuleSet`
    /// built in the process. It reaches no result; caches of handles compare
    /// it to know whose handles they hold.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// This set as a distinct set: a test's copy of a shared retail load.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_fresh_serial(mut self) -> Self {
        self.serial = next_rule_set_serial();
        self.resolve_handles();
        self.rebuild_sound_names();
        self
    }

    /// Dereference a handle to its object. Handles only originate from this
    /// `RuleSet`, so the index is always in bounds.
    #[inline]
    pub fn object_by_handle(&self, handle: TypeHandle) -> &ObjectType {
        &self.object_list[handle.0 as usize]
    }

    /// Look up a game object by ID (case-insensitive, engine parity).
    pub fn object(&self, id: &str) -> Option<&ObjectType> {
        #[cfg(any(test, feature = "test-support"))]
        TYPE_NAME_LOOKUPS.with(|count| count.set(count.get() + 1));
        self.type_handle(id).map(|h| self.object_by_handle(h))
    }

    /// Cameo10034E75/10034F3F and SPECIAL_PLACE10038EB9 use the same
    /// retained flag. Expose the fact without exporting mutable SWExt state.
    pub fn super_weapon_uses_ai_targeting(&self, id: &str) -> bool {
        if self.ares {
            self.super_weapon_target_policy(id)
                .is_some_and(|policy| policy.use_ai_targeting())
        } else {
            false
        }
    }

    /// The ordered SWTypeExt targeting fields of an Ares GenericWarhead or
    /// UnitDelivery type. Stock types keep their retail dispatch owners.
    pub(crate) fn super_weapon_target_policy(
        &self,
        id: &str,
    ) -> Option<crate::rules::superweapon_targeting::AresSuperWeaponTargetingState> {
        if !self.super_weapon(id)?.kind.has_ares_handler() {
            return None;
        }
        self.super_weapon_metadata(id)
            .map(|metadata| metadata.targeting())
    }

    /// `Type == Rules+0x498` (`[General] PrismType=`, `0x0044B2F8`). Ares
    /// asks the type's `PrismForwarding=` instead (`rules::ares_prism_forwarding`).
    pub(crate) fn is_prism_type(&self, object: &ObjectType) -> bool {
        self.general
            .prism_type
            .as_deref()
            .is_some_and(|prism_type| object.id.eq_ignore_ascii_case(prism_type))
    }

    /// The RocketStruct a Rocket owner of this type flies by and whether its
    /// Type is the CMisl type ([`MissileSpawnRules::rocket_block`]). A game
    /// Ares plays in takes a `Missile.Custom` AircraftType's own block first:
    /// hooks 0x6622E0 (Ares.dll 100610F0), 0x66305A (10060FA0), 0x6632F2
    /// (100610C0) and 0x6634F6 (10061090). That block's Type is the type
    /// itself, which Process's Boomer arms compare with CMislType.
    ///
    /// [`MissileSpawnRules::rocket_block`]: crate::rules::missile_spawn::MissileSpawnRules::rocket_block
    pub fn rocket_block(
        &self,
        type_name: &str,
    ) -> (&crate::rules::missile_spawn::MissileSpawnParams, bool) {
        if self.ares
            && let Some(block) = self
                .ares_missile(type_name)
                .and_then(|missile| missile.custom_block())
        {
            let cmisl = block
                .type_name
                .eq_ignore_ascii_case(&self.missile_spawn.cmisl.type_name);
            return (block, cmisl);
        }
        self.missile_spawn.rocket_block(type_name)
    }

    /// The puffs and impact weapon of a Rocket owner of this type. Ares
    /// reads them for every AircraftType (`Missile.TakeOffAnim`,
    /// `Missile.TrailerAnim`, `Missile.TrailerSeparation`, hooks 0x66238A,
    /// 0x662512, 0x6627E5 and 0x662D85; the weapons only for
    /// `Missile.Custom`, 0x663218).
    pub fn rocket_type_ext(
        &self,
        type_name: &str,
    ) -> crate::rules::missile_spawn::RocketTypeExt<'_> {
        if self.ares
            && let Some(missile) = self.ares_missile(type_name)
        {
            return crate::rules::missile_spawn::RocketTypeExt {
                takeoff_anim: missile.takeoff_anim(),
                trailer_anim: missile.trailer_anim(),
                trailer_separation: missile.trailer_separation(),
                weapons: [missile.weapon(false), missile.weapon(true)],
            };
        }
        crate::rules::missile_spawn::RocketTypeExt::STOCK
    }

    /// Frames a launched missile node of a `Spawns=` pool of this type waits
    /// before it expires its child, or `None` when the pool's nodes are not
    /// missiles. The node flag is set for the three Rules families
    /// (`0x006B6D60`, `0x006B78F8`) and the wait reads their frames
    /// (`0x006B7A72`); Ares adds a `Missile.Custom` type with its own
    /// PauseFrames + TiltFrames (SpawnManagerClass_CTOR_CustomMissile
    /// 10061320, Update_CustomMissile 100613A0, Update_CustomMissile2
    /// 100613C0).
    pub(crate) fn spawn_missile_wait_frames(&self, type_name: &str) -> Option<u32> {
        if self.ares
            && let Some(block) = self
                .ares_missile(type_name)
                .and_then(|missile| missile.custom_block())
        {
            return Some(block.tilt_frames.wrapping_add(block.pause_frames).max(0) as u32);
        }
        self.missile_spawn
            .family_of(type_name)
            .map(|family| self.missile_spawn.kamikaze_wait_frames(family))
    }

    fn ares_missile(
        &self,
        type_name: &str,
    ) -> Option<&crate::rules::ares_techno_type::CustomMissileRules> {
        self.object_in_category(ObjectCategory::Aircraft, type_name)?
            .techno_combat()
            .missile()
    }

    /// Resolve one name within a specific native TechnoType registry.
    /// Category plus case-insensitive ID is the stable analogue of the
    /// category-distinct pointer retained by native TaskForce entries.
    pub(crate) fn object_in_category(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> Option<&ObjectType> {
        self.type_handle_in_category(category, id)
            .map(|handle| self.object_by_handle(handle))
    }

    /// The handle of the object [`Self::object_in_category`] answers.
    pub(crate) fn type_handle_in_category(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> Option<TypeHandle> {
        self.object_category_index.get(category, id)
    }

    /// The computer's base defense candidates of a House's side
    /// (`HouseTypeClass+0xBC`): Allied for side 0, Soviet for 1, Third for any
    /// other (`0x00507BCA..0x00507BFA`).
    pub(crate) fn base_defense_types(&self, side_index: i32) -> &[String] {
        if self.ares
            && let Some(types) = self.side_base_defenses(side_index)
        {
            return types;
        }
        match side_index {
            0 => &self.allied_base_defense_types,
            1 => &self.soviet_base_defense_types,
            _ => &self.third_base_defense_types,
        }
    }

    /// The BuildingType at native array index `index` (`BuildingTypes[index]`,
    /// the index `ObjectType::base_plan_type_index` and BasePlan nodes hold).
    /// `None` for a negative index, one past the array, or a registered name
    /// without a section.
    pub(crate) fn building_type_at(&self, index: i32) -> Option<&ObjectType> {
        self.type_array_at(ObjectCategory::Building, index)
    }

    /// Resolve one scenario BasePlan token through the native BuildingType
    /// registry only, matching
    /// `BuildingTypeClass__FindIndexByName @ 0x0045E7B0` used by
    /// `FUN_0042EBE0`, and return its ordered index.
    pub(crate) fn building_type_index(&self, id: &str) -> Option<i32> {
        self.type_array_index(ObjectCategory::Building, id)
    }

    /// The ids of one native TechnoType array (`InfantryTypeClass::Array`
    /// `0xA8E34C`, `UnitTypeClass::Array` `0xA83CE4`, `AircraftTypeClass::
    /// Array` `0xA8B21C`, `BuildingTypeClass::Array` `0xA83C6C`), in index
    /// order: the registry's first spelling of each identity.
    pub(crate) fn type_array_ids(&self, category: ObjectCategory) -> &[String] {
        match category {
            ObjectCategory::Infantry => &self.infantry_ids,
            ObjectCategory::Vehicle => &self.vehicle_ids,
            ObjectCategory::Aircraft => &self.aircraft_ids,
            ObjectCategory::Building => &self.building_ids,
        }
    }

    /// The type at native array index `index` of `category`'s array; `None`
    /// for a negative index, one past the array, or a registered name
    /// without a section.
    pub(crate) fn type_array_at(
        &self,
        category: ObjectCategory,
        index: i32,
    ) -> Option<&ObjectType> {
        let id = self
            .type_array_ids(category)
            .get(usize::try_from(index).ok()?)?;
        self.object_in_category(category, id)
    }

    /// The native array index (`+0xDF8`) of `id` in `category`'s array.
    pub fn type_array_index(&self, category: ObjectCategory, id: &str) -> Option<i32> {
        self.type_array_indices.get(category, id)
    }

    /// Installed Ares Building Find10007E50, shared with BuiltAt's current-pass
    /// vector reader. Runtime upgrade queries use the current registered IDs,
    /// rejecting none/<none> before the first case-insensitive stored-ID hit.
    pub fn ares_find_building(&self, token: &str) -> Option<i32> {
        crate::rules::native_processing::ares_type_index_of_name(
            token,
            self.type_array_ids(ObjectCategory::Building)
                .iter()
                .map(String::as_str),
        )
        .map(|index| i32::try_from(index).expect("BuildingTypes exceeds native i32 index space"))
    }

    /// Resolve a TaskForce member through the exact native family order used
    /// by `gamemd.exe 0x004C4EF0`: Infantry, Unit, then Aircraft. BuildingType
    /// is never searched.
    pub(crate) fn task_force_member_object(&self, id: &str) -> Option<&ObjectType> {
        let key = upper_name_key(id);
        [
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Aircraft,
        ]
        .into_iter()
        .find_map(|category| self.object_in_category(category, &key))
    }

    /// Resolve AITrigger token 6 through the exact native family order used
    /// by `gamemd.exe 0x0041F77D..0x0041F7E4`: Infantry, Unit, Aircraft,
    /// then Building.
    pub(crate) fn ai_trigger_object(&self, id: &str) -> Option<&ObjectType> {
        let key = upper_name_key(id);
        [
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Aircraft,
            ObjectCategory::Building,
        ]
        .into_iter()
        .find_map(|category| self.object_in_category(category, &key))
    }

    /// First registered BuildingType whose merged ART `ToOverlay=` resolves to
    /// the requested overlay. Native stops on this first match even when that
    /// type is later rejected as `Unsellable=`.
    pub fn first_building_type_for_overlay(
        &self,
        overlay_id: u8,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Option<&ObjectType> {
        self.object_list.iter().find(|object| {
            object.category == crate::rules::object_type::ObjectCategory::Building
                && object
                    .to_overlay
                    .as_deref()
                    .and_then(|name| overlays.id_for_name(name))
                    == Some(overlay_id)
        })
    }

    /// TechnoType virtual `+0x84`, the cost a House pays for `object`.
    ///
    /// `TechnoTypeClass::Cost_Of @ 0x00711F00` (Infantry, Unit and Aircraft
    /// types) returns the type's virtual `+0xAC` cost ([`Self::type_cost`])
    /// for a null House; otherwise it stores the country factor
    /// (`0x0050BDF0`) and the FactoryPlant factor (`0x0050BEB0`) as floats
    /// and returns `ftol(cost * plant * country)` (`0x00711F35..0x00711F41`).
    /// The BuildingType override `0x0045EDD0` adjusts its actual cost
    /// (virtual `+0xAC` = `0x0045ED50`) the same way, adds the halved sum of
    /// both PadAircraft costs when it bundles them, and adds its FreeUnit's
    /// cost, clamping only that arm at zero (`0x0045EE47..0x0045EE50`).
    /// Native comparison: `tools/spatial_oracle/cost_of`.
    pub fn cost_of(&self, object: &ObjectType, house: Option<&HouseCostFactors>) -> i32 {
        let cost = self.adjusted_cost(object, house);
        if object.category != ObjectCategory::Building {
            return cost;
        }
        let mut cost = cost;
        if let Some((first, second)) = self.bundled_pad_aircraft(object) {
            let pads = self
                .adjusted_cost(second, house)
                .wrapping_add(self.adjusted_cost(first, house));
            cost = cost.wrapping_add(pads / 2);
        }
        match object
            .free_unit
            .as_deref()
            .and_then(|name| self.object(name))
        {
            Some(free) => cost.wrapping_add(self.adjusted_cost(free, house)).max(0),
            None => cost,
        }
    }

    /// `0x00711F00` over the type's virtual `+0xAC` cost ([`Self::type_cost`]).
    fn adjusted_cost(&self, object: &ObjectType, house: Option<&HouseCostFactors>) -> i32 {
        let cost = self.type_cost(object);
        if self.ares {
            house.map_or(cost, |house| {
                house.ares_adjust(
                    cost,
                    object.factor_slot(),
                    object.ares_production().factory_plant_multiplier(),
                )
            })
        } else {
            house.map_or(cost, |house| house.adjust(cost, object.factor_slot()))
        }
    }

    /// The two PadAircraft a BuildingType bundles into its price: with
    /// `SeparateAircraft=no`, when it is the first `Dock=` of the first
    /// PadAircraft (`0x0045ED63..0x0045ED7D`).
    fn bundled_pad_aircraft(&self, object: &ObjectType) -> Option<(&ObjectType, &ObjectType)> {
        if self.general.separate_aircraft {
            return None;
        }
        let [first_id, second_id, ..] = self.general.pad_aircraft_types.as_slice() else {
            return None;
        };
        let (first, second) = (self.object(first_id)?, self.object(second_id)?);
        first
            .dock
            .first()
            .is_some_and(|dock| dock.eq_ignore_ascii_case(&object.id))
            .then_some((first, second))
    }

    /// TechnoType virtual `+0xAC` (GetCost): `Cost=` (`0x00711EB0`), or a
    /// BuildingType's [`Self::building_actual_cost`] (`0x0045ED50`).
    pub fn type_cost(&self, object: &ObjectType) -> i32 {
        if object.category == ObjectCategory::Building {
            self.building_actual_cost(object)
        } else {
            object.cost
        }
    }

    /// BuildingType virtual `+0xAC` value. Wall sale invokes and discards it;
    /// receiver anger uses the same authority.
    pub(crate) fn building_actual_cost(&self, object: &ObjectType) -> i32 {
        let mut value = object.cost;
        if let Some((first, second)) = self.bundled_pad_aircraft(object) {
            value = value.wrapping_sub(first.cost.wrapping_add(second.cost) / 2);
        }
        if let Some(free_unit) = object.free_unit.as_deref() {
            value = value
                .wrapping_sub(self.object(free_unit).map_or(0, |free| free.cost))
                .max(0);
        }
        value
    }

    /// Look up a TerrainObjectType by section name, case-insensitive.
    /// Short names use a stack key instead of allocating a temporary string.
    pub fn terrain_object_type_case_insensitive(&self, name: &str) -> Option<&TerrainObjectType> {
        crate::util::native_string::with_ascii_uppercase(name, |key| {
            self.terrain_object_types.get(key)
        })
    }

    /// Incremental projection of the retained FindOrAllocate71E2A0 result.
    /// Existing types keep their bound assets; no global Rules/ART rebuild.
    pub(crate) fn install_map_terrain_type(&mut self, terrain: TerrainObjectType) {
        // This name-keyed projection covers distinct short stored IDs.
        // Repeated >24-byte inputs can allocate equal-ID native slots; their
        // separate per-slot identity remains outside this projection's scope.
        self.terrain_object_types
            .insert(terrain.name.to_ascii_uppercase(), terrain);
    }

    /// Look up a weapon by ID (case-insensitive, gamemd parity).
    pub fn weapon(&self, id: &str) -> Option<&WeaponType> {
        self.weapon_handle(id)
            .map(|handle| self.weapon_by_handle(handle))
    }

    /// The handle of the weapon [`Self::weapon`] answers for `id`.
    pub fn weapon_handle(&self, id: &str) -> Option<WeaponHandle> {
        Self::lookup_ci(&self.weapons, id).copied()
    }

    /// Weapon `id`'s position in this set's weapons, in the order the native
    /// Weapon family allocated them: the Weapon ID Wasm extension modules
    /// see (RFC 0001 appendix 8.1; provisional numbering).
    pub fn weapon_index(&self, id: &str) -> Option<u32> {
        self.weapon_handle(id).map(|handle| handle.0)
    }

    /// The weapon at [`Self::weapon_index`] `index`.
    pub fn weapon_at(&self, index: u32) -> Option<&WeaponType> {
        self.weapon_types.get(index as usize)
    }

    /// Warhead `id`'s position in this set's warheads, as
    /// [`Self::weapon_index`] numbers weapons.
    pub fn warhead_index(&self, id: &str) -> Option<u32> {
        self.warhead_handle(id).map(|handle| handle.0)
    }

    /// The warhead at [`Self::warhead_index`] `index`.
    pub fn warhead_at(&self, index: u32) -> Option<&WarheadType> {
        self.warhead_types.get(index as usize)
    }

    /// Projectile `id`'s position in this set's projectiles (their handle
    /// order, which follows the weapons that name them): the Projectile ID
    /// Wasm extension modules see (RFC 0001 appendix 8.1; provisional).
    pub fn projectile_index(&self, id: &str) -> Option<u32> {
        self.projectile_handle(id).map(|handle| handle.0)
    }

    /// The projectile at [`Self::projectile_index`] `index`.
    pub fn projectile_at(&self, index: u32) -> Option<&ProjectileType> {
        self.projectile_types.get(index as usize)
    }

    /// The weapon `handle` names in this set, which made it.
    #[inline]
    pub(crate) fn weapon_by_handle(&self, handle: WeaponHandle) -> &WeaponType {
        &self.weapon_types[handle.0 as usize]
    }

    /// [`Self::weapon`] of `id`, through `resolved` when this set resolved
    /// it from `id`; debug builds compare the handle with the name's.
    #[inline]
    pub(crate) fn resolved_weapon(
        &self,
        id: &str,
        resolved: ResolvedWeapon,
    ) -> Option<&WeaponType> {
        if resolved.rules_serial != self.serial {
            return self.weapon(id);
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || resolved.handle == self.weapon_handle(id),
            "weapon {id:?} resolved to {:?}, unlike its name",
            resolved.handle
        );
        resolved.handle.map(|handle| self.weapon_by_handle(handle))
    }

    /// Resolve every type's weapon records against this set's weapons
    /// ([`TypeWeapons`]), every weapon's warhead and projectile
    /// ([`WeaponParts`]), and every type's and warhead's `DebrisTypes=`
    /// against its `[VoxelAnims]` ([`ResolvedDebris`]). Runs when the set
    /// is built, after the records' last writer
    /// (`RulesTypeReferences::apply_to` sets the occupy weapons, and the
    /// ordered Rules pass the warheads' debris), and when a copy takes its
    /// own serial.
    pub(super) fn resolve_handles(&mut self) {
        let resolve = |id: Option<&str>| id.and_then(|id| self.weapon_handle(id));
        let resolved: Vec<TypeWeapons> = self
            .object_list
            .iter()
            .map(|object| TypeWeapons {
                rules_serial: self.serial,
                base: object
                    .weapon_list
                    .iter()
                    .map(|slot| resolve(slot.as_deref()))
                    .collect(),
                elite: object
                    .elite_weapon_list
                    .iter()
                    .map(|slot| resolve(slot.as_deref()))
                    .collect(),
                occupy: [
                    resolve(object.occupy_weapon.as_deref()),
                    resolve(object.elite_occupy_weapon.as_deref()),
                ],
            })
            .collect();
        for (object, weapons) in self.object_list.iter_mut().zip(resolved) {
            object.project_resolved_weapons(weapons);
        }
        let parts: Vec<WeaponParts> = self
            .weapon_types
            .iter()
            .map(|weapon| WeaponParts {
                rules_serial: self.serial,
                warhead: weapon
                    .warhead
                    .as_deref()
                    .and_then(|id| self.warhead_handle(id)),
                projectile: weapon
                    .projectile
                    .as_deref()
                    .and_then(|id| self.projectile_handle(id)),
            })
            .collect();
        for (weapon, parts) in self.weapon_types.iter_mut().zip(parts) {
            weapon.project_parts(parts);
        }
        let object_debris: Vec<ResolvedDebris> = self
            .object_list
            .iter()
            .map(|object| self.resolve_debris(&object.debris_types))
            .collect();
        for (object, debris) in self.object_list.iter_mut().zip(object_debris) {
            object.project_resolved_debris(debris);
        }
        let warhead_debris: Vec<ResolvedDebris> = self
            .warhead_types
            .iter()
            .map(|warhead| self.resolve_debris(&warhead.debris_types))
            .collect();
        for (warhead, debris) in self.warhead_types.iter_mut().zip(warhead_debris) {
            warhead.project_resolved_debris(debris);
        }
        self.radiation_site_warhead = self.warhead_handle(&self.radiation.site_warhead);
    }

    /// The warhead `[Radiation] RadSiteWarhead=` names, as
    /// [`Self::warhead`] answers for it, through the handle this set
    /// resolved; debug builds compare it with the name's.
    #[inline]
    pub(crate) fn radiation_site_warhead(&self) -> Option<&WarheadType> {
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || self.radiation_site_warhead == self.warhead_handle(&self.radiation.site_warhead),
            "RadSiteWarhead resolved to {:?}, unlike its name {:?}",
            self.radiation_site_warhead,
            self.radiation.site_warhead
        );
        self.radiation_site_warhead
            .map(|handle| self.warhead_by_handle(handle))
    }

    /// `names`, a `DebrisTypes=` list, as this set resolves it.
    fn resolve_debris(&self, names: &[String]) -> ResolvedDebris {
        ResolvedDebris {
            rules_serial: self.serial,
            types: names
                .iter()
                .map(|name| self.voxel_anim_type_id_by_name(name))
                .collect(),
        }
    }

    /// `object`'s `DebrisTypes=` as `[VoxelAnims]` types, in list order,
    /// `None` for a name none has: the ids this set resolved when it holds
    /// `object`, else each name looked up.
    pub(crate) fn object_debris_types<'a>(
        &self,
        object: &'a ObjectType,
    ) -> std::borrow::Cow<'a, [Option<VoxelAnimTypeId>]> {
        self.debris_types(&object.debris_types, object.resolved_debris())
    }

    /// `warhead`'s `DebrisTypes=`, as [`Self::object_debris_types`] reads a
    /// type's.
    pub(crate) fn warhead_debris_types<'a>(
        &self,
        warhead: &'a WarheadType,
    ) -> std::borrow::Cow<'a, [Option<VoxelAnimTypeId>]> {
        self.debris_types(&warhead.debris_types, warhead.resolved_debris())
    }

    /// `names` through `resolved` when this set resolved it; debug builds
    /// compare the ids with the names'.
    fn debris_types<'a>(
        &self,
        names: &[String],
        resolved: &'a ResolvedDebris,
    ) -> std::borrow::Cow<'a, [Option<VoxelAnimTypeId>]> {
        if resolved.rules_serial != self.serial {
            return std::borrow::Cow::Owned(self.resolve_debris(names).types.into_vec());
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || resolved.types == self.resolve_debris(names).types,
            "debris {names:?} resolved to {:?}, unlike their names",
            resolved.types
        );
        std::borrow::Cow::Borrowed(&resolved.types)
    }

    /// Number of WeaponType global88756C allocation slots, including refused
    /// ambiguous slots. Consumers must preserve these positions for Action42.
    pub fn weapon_array_len(&self) -> usize {
        self.weapon_array.len()
    }

    /// WeaponType global88756C's allocation index, used by Action42 at
    /// 6E2390. Negative/out-of-range indices and ambiguous truncated
    /// Weapon/Warhead names refuse admission rather than selecting an alias.
    pub fn weapon_by_array_index(&self, index: i32) -> Option<&WeaponType> {
        let (name, unambiguous) = self.weapon_array.get(usize::try_from(index).ok()?)?;
        unambiguous.then(|| self.weapon(name)).flatten()
    }

    /// Look up a warhead by ID (case-insensitive, gamemd parity).
    pub fn warhead(&self, id: &str) -> Option<&WarheadType> {
        self.warhead_handle(id)
            .map(|handle| self.warhead_by_handle(handle))
    }

    /// The handle of the warhead [`Self::warhead`] answers for `id`.
    pub(crate) fn warhead_handle(&self, id: &str) -> Option<WarheadHandle> {
        Self::lookup_ci(&self.warheads, id).copied()
    }

    /// The warhead `handle` names in this set, which made it.
    pub(crate) fn warhead_by_handle(&self, handle: WarheadHandle) -> &WarheadType {
        &self.warhead_types[handle.0 as usize]
    }

    /// The warhead [`Self::warhead`] answers for `id`, to rewrite. Its
    /// handle stays the same, so the weapons' resolved parts stay current.
    pub(super) fn warhead_mut(&mut self, id: &str) -> Option<&mut WarheadType> {
        let handle = self.warhead_handle(id)?;
        Some(&mut self.warhead_types[handle.0 as usize])
    }

    /// [`Self::warhead`] of `weapon`'s `Warhead=`, through the handle this
    /// set resolved when it holds `weapon`; debug builds compare the handle
    /// with the name's.
    #[inline]
    pub fn weapon_warhead(&self, weapon: &WeaponType) -> Option<&WarheadType> {
        let parts = weapon.parts();
        if parts.rules_serial != self.serial {
            return weapon.warhead.as_deref().and_then(|id| self.warhead(id));
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || parts.warhead
                    == weapon
                        .warhead
                        .as_deref()
                        .and_then(|id| self.warhead_handle(id)),
            "weapon {:?} resolved its warhead to {:?}, unlike its name",
            weapon.id,
            parts.warhead
        );
        parts.warhead.map(|handle| self.warhead_by_handle(handle))
    }
}

impl RuleSet {
    /// Look up a projectile by ID (case-insensitive, gamemd parity).
    pub fn projectile(&self, id: &str) -> Option<&ProjectileType> {
        self.projectile_handle(id)
            .map(|handle| self.projectile_by_handle(handle))
    }

    /// The handle of the projectile [`Self::projectile`] answers for `id`.
    pub(crate) fn projectile_handle(&self, id: &str) -> Option<ProjectileHandle> {
        Self::lookup_ci(&self.projectiles, id).copied()
    }

    /// The projectile `handle` names in this set, which made it.
    pub(crate) fn projectile_by_handle(&self, handle: ProjectileHandle) -> &ProjectileType {
        &self.projectile_types[handle.0 as usize]
    }

    /// The projectile [`Self::projectile`] answers for `id`, to rewrite. Its
    /// handle stays the same, so the weapons' resolved parts stay current.
    pub(super) fn projectile_mut(&mut self, id: &str) -> Option<&mut ProjectileType> {
        let handle = self.projectile_handle(id)?;
        Some(&mut self.projectile_types[handle.0 as usize])
    }

    /// [`Self::projectile`] of `weapon`'s `Projectile=`, through the handle
    /// this set resolved when it holds `weapon`; debug builds compare the
    /// handle with the name's.
    #[inline]
    pub fn weapon_projectile(&self, weapon: &WeaponType) -> Option<&ProjectileType> {
        let parts = weapon.parts();
        if parts.rules_serial != self.serial {
            return weapon
                .projectile
                .as_deref()
                .and_then(|id| self.projectile(id));
        }
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || parts.projectile
                    == weapon
                        .projectile
                        .as_deref()
                        .and_then(|id| self.projectile_handle(id)),
            "weapon {:?} resolved its projectile to {:?}, unlike its name",
            weapon.id,
            parts.projectile
        );
        parts
            .projectile
            .map(|handle| &self.projectile_types[handle.0 as usize])
    }

    /// Registered projectile types, including their retained animation references.
    pub fn projectiles_iter(&self) -> impl Iterator<Item = &ProjectileType> {
        self.projectiles
            .values()
            .map(|handle| &self.projectile_types[handle.0 as usize])
    }

    /// Every projectile with its ID, in the ID map's order.
    pub(super) fn projectile_entries(&self) -> impl Iterator<Item = (&String, &ProjectileType)> {
        self.projectiles
            .iter()
            .map(|(id, handle)| (id, &self.projectile_types[handle.0 as usize]))
    }

    /// Deterministic hash of the processed source INI this RuleSet was built from
    /// (RULESMD, optional LANGRULE, selected mode, then the map's rules-shaped
    /// pass). Stamped into
    /// diagnostic-log/snapshot headers so playback can detect a mismatched rules set —
    /// sensitive to scalar value overrides, not just the type-registry lists.
    pub fn source_ini_hash(&self) -> u64 {
        self.source_ini_hash
    }

    /// Whether Ares plays in this game: its Rules layers or map hold an Ares
    /// key, or the game ships `ares.mix` (`rules::ares_switch`). Sim and
    /// presentation read this one answer, in every build.
    #[inline]
    pub fn ares(&self) -> bool {
        self.ares
    }

    /// VERA: whether the player's Ares setting Off is what keeps Ares from
    /// playing in this game: its Rules layers or map hold a key that turns
    /// Ares on for a player set to Auto (`ares_switch::AresInputs::kept_off`).
    /// Their games are not the same game, which a refused network start says.
    pub fn ares_kept_off(&self) -> bool {
        self.ares_kept_off
    }

    /// Whether Phobos plays in this game: its Rules layers or map hold a key
    /// of a Phobos piece that runs in every build, or the build has the
    /// `phobos` feature (`rules::phobos_switch`). Sim and presentation read
    /// this one answer.
    #[inline]
    pub fn phobos(&self) -> bool {
        self.phobos
    }

    /// Whether Kratos plays in this game: its Rules, art, mode or map hold a
    /// key of a Kratos piece that runs in every build
    /// (`rules::kratos_switch`). Sim and presentation read this one answer.
    #[inline]
    pub fn kratos(&self) -> bool {
        self.kratos
    }

    /// Whether NPatch plays in this game: its Rules layers or map hold a key
    /// of an NPatch piece that runs in every build
    /// (`rules::npatch_switch`). Sim and presentation read this one answer.
    #[inline]
    pub fn npatch(&self) -> bool {
        self.npatch
    }

    /// VERA: whether this game plays RA2 Mode (docs/ra2-mode.md): original
    /// Red Alert 2's rules where they differ from Yuri's Revenge. Sim and
    /// rules read this one answer; app, asset and INI-root code read the
    /// launch's choice (`util::ra2_launch`) it comes from.
    #[inline]
    pub fn ra2(&self) -> bool {
        self.ra2
    }

    /// Whether this game's Rules were read the Ares way
    /// (`ares_switch::ares_way`). The game's readers after the projection (its
    /// fixed ART, the asset closures) open the same mode with
    /// `ares_switch::read_ares_way`.
    pub fn reads_ares_way(&self) -> bool {
        self.reads_ares_way
    }

    /// Whether the code that needs Ares' own Rules takes its Ares form in this
    /// game: the superweapons, production, power and money. Their Ares arms
    /// read data only Rules read the Ares way hold (the SWTypeExt metadata
    /// and handler kinds, BuildingTypeExt, the factory owners, the generic
    /// prerequisites, a House type's power plants), and a stock arm can
    /// misread such data (a handler type's absent PreDependent). So they
    /// follow how the Rules were read, not [`Self::ares`] alone. A game Ares
    /// turned on in by its keys, read the stock way, keeps the stock form of
    /// these until loading reads its Rules the Ares way.
    pub fn ares_form(&self) -> bool {
        self.reads_ares_way
    }

    /// Resolve one retained palette request. Ares715857->715876 removes
    /// only the original successful-group limit: a supplied request resolves
    /// its own file; a failed file never inherits another type's group.
    /// Without Ares, retain the original one-successful-group compatibility
    /// policy and admit a supplied request only if it matches that group.
    /// None selects the singleton for the original effect/legacy consumers.
    /// This is the sole resource lookup/decoder used by core and client.
    pub fn load_extra_building_palette<'a>(
        &'a self,
        assets: &crate::assets::asset_manager::AssetManager,
        theater_ext: &str,
        requested_name: Option<&'a str>,
    ) -> Option<(&'a str, crate::assets::pal_file::Palette)> {
        let load = |name: &'a str| {
            let data = assets.get_ref(&format!("{name}{theater_ext}.pal"))?;
            crate::assets::pal_file::Palette::from_bytes(data)
                .ok()
                .map(|palette| (name, palette))
        };
        if self.ares
            && let Some(name) = requested_name
        {
            return load(name);
        }
        let selected = self
            .object_list
            .iter()
            .find_map(|object| load(object.custom_palette_name()?));
        if requested_name.is_some_and(|name| {
            selected
                .as_ref()
                .is_none_or(|(loaded, _)| !name.eq_ignore_ascii_case(loaded))
        }) {
            return None;
        }
        selected
    }

    /// Techno705D70 reads this type's actual DF0 group, not DD0 text or
    /// another type's success. The immutable asset owner qualifies and
    /// returns the retained name for a producer to freeze on its animation.
    pub(crate) fn loaded_custom_palette_name(&self, object: &ObjectType) -> Option<&str> {
        self.effect_assets
            .loaded_extra_building_palette_name(object.custom_palette_name()?)
    }

    /// Sorted (upper-case ID, value) pairs for the config hash. They hash as
    /// the `BTreeMap` these walks once collected, without letting a map's
    /// iteration order (per process for `HashMap`, per target for
    /// `LookupMap`) pick which case variant survives: IDs longer than the
    /// registry's 0x18 stored bytes can differ only by case, and all are kept.
    pub(super) fn by_upper_id<'a, T>(
        entries: impl Iterator<Item = (&'a String, T)>,
    ) -> Vec<(String, T)> {
        let mut entries: Vec<_> = entries
            .map(|(id, value)| (id.to_ascii_uppercase(), id, value))
            .collect();
        entries.sort_unstable_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        entries
            .into_iter()
            .map(|(upper, _, value)| (upper, value))
            .collect()
    }
}

impl RuleSet {
    /// FireAt's Building Height input: native BuildingRead4610D8..461101
    /// reads ART[effective Image].Height, retaining constructor default2 for
    /// fresh missing data. Explicit Image does not fall back to type ID.
    /// Stock reload equivalence is bounded by the retail Image census; arbitrary
    /// image-changing reload history remains an open producer contract.
    pub(crate) fn building_launch_height(&self, object: &ObjectType) -> i32 {
        self.art_registry
            .get(&object.image)
            .map_or(2, |art| art.height)
    }

    /// BuildingType Dimension2 @0x00464AF0, virtual+7C: the foundation's
    /// width/height in 256-lepton cells and fixed ART Height times 104. Shares
    /// the existing Height reader/default with FireAt's distinct Height * 200
    /// launch calculation. Original execution: building_brackets.json and
    /// spatial_oracle/height_factor.json; consumers do not copy this query.
    pub fn building_dimensions(&self, object: &ObjectType) -> [i32; 3] {
        let [width, height] = object.foundation.dimensions_leptons();
        [
            width,
            height,
            self.building_launch_height(object)
                .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
        ]
    }
}

impl RuleSet {
    /// Whether `type_id` is in `[AI] BuildTech=` (`RulesClass+0x91C`, a
    /// vector of BuildingType pointers, so the match is by identity, here a
    /// case-insensitive name).
    pub fn is_build_tech(&self, type_id: &str) -> bool {
        self.build_tech_types
            .iter()
            .any(|tech| tech.eq_ignore_ascii_case(type_id))
    }

    /// Look up a superweapon type by ID (case-insensitive, gamemd parity).
    pub fn super_weapon(&self, id: &str) -> Option<&SuperWeaponType> {
        Self::lookup_ci(&self.super_weapons, id)
    }

    pub(crate) fn super_weapon_metadata(
        &self,
        id: &str,
    ) -> Option<&crate::rules::superweapon_type::AresSuperWeaponMetadata> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.ares_super_weapon_metadata.get(key)
        })
    }

    /// The Phobos techno gates for a superweapon, if the capability is
    /// compiled in and the type was registered.
    #[cfg(feature = "phobos")]
    pub(crate) fn phobos_super_weapon_metadata(
        &self,
        id: &str,
    ) -> Option<&crate::rules::superweapon_type::PhobosSuperWeaponAvailability> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.phobos_super_weapon_metadata.get(key)
        })
    }

    /// The `[GlobalControls]` parallel AI queue switches (Phobos extension).
    pub fn parallel_ai_queues(
        &self,
    ) -> &crate::rules::phobos_parallel_ai_queues::ParallelAiQueueRules {
        &self.parallel_ai_queues
    }

    /// Whether the Phobos parallel AI queue hooks act in this game: Phobos
    /// plays ([`Self::phobos`]) and Ares' `AllowParallelAIQueues=` gate is on.
    /// Each hook of that chain does nothing otherwise.
    pub fn parallel_ai_queues_act(&self) -> bool {
        self.phobos && self.parallel_ai_queues.allows_parallel()
    }

    /// Whether Ares' serial AI queues act in this game: Ares plays and
    /// `[GlobalControls] AllowParallelAIQueues=no`. Ares then lets one
    /// factory building per category start production at a time
    /// (`BuildingClass_Update_Factory`, hook `0x4502F4`, Ares.dll
    /// `10024E00`). The one key keeps it apart from the Phobos hooks
    /// ([`Self::parallel_ai_queues_act`]), so the two share the house's slots.
    pub fn serial_ai_queues_act(&self) -> bool {
        self.ares && !self.parallel_ai_queues.allows_parallel()
    }

    /// A superweapon type's `SuperWeaponTypeClass` array index (its
    /// `[SuperWeaponTypes]` position, case-insensitive), the value
    /// `SuperWeapon=` stores (BuildingType `+0x16F0`).
    pub fn super_weapon_index(&self, id: &str) -> Option<usize> {
        crate::rules::native_processing::super_weapon_index_of_name(
            id,
            self.super_weapon_order.iter().map(String::as_str),
        )
    }

    /// Look up a particle type by ID. Panics if `id` is out of range.
    pub fn particle_type(&self, id: ParticleTypeId) -> &ParticleType {
        &self.particle_types[id.0 as usize]
    }

    /// Iterate every parsed `[Particles]` definition.
    pub fn particle_types_iter(&self) -> impl Iterator<Item = &ParticleType> {
        self.particle_types.iter()
    }

    /// Look up a particle system type by ID. Panics if `id` is out of range.
    pub fn particle_system_type(&self, id: ParticleSystemTypeId) -> &ParticleSystemType {
        &self.particle_system_types[id.0 as usize]
    }

    /// Whether a particle system of type `id` changes nothing but its own
    /// picture, so a producer that keeps no hold on it may run it as a
    /// presentation object (`Simulation::spawn_effect_particle_system`).
    /// VERA-only.
    ///
    /// Such a type has `BehavesLike=Smoke` or `Spark`, whose system and
    /// particle AI read the world but write nothing in it beyond the system
    /// itself and a light the picture shows. `Gas`, `Fire` and the Firestorm
    /// `Web` damage or web what they reach, and `Railgun` and an unknown
    /// behaviour are refused. It has no `AlphaImage=`, whose glow the
    /// simulation keeps and saves. Every particle type its `HoldsWhat=`
    /// names, and each `NextParticle=` after that, has `BehavesLike=Smoke` or
    /// `Spark` too, `Damage=0`, no `Warhead=`, no `AlphaImage=`, and no Ares
    /// `DamageRange=`. A chain that comes back to a type it passed ends there.
    pub fn particle_system_presentation_only(&self, id: ParticleSystemTypeId) -> bool {
        use crate::rules::particle_system_type::ParticleSystemBehavesLike as System;
        use crate::rules::particle_type::ParticleBehavesLike as Particle;
        let Some(system) = self.particle_system_types.get(id.0 as usize) else {
            return false;
        };
        if !matches!(system.behaves_like, System::Smoke | System::Spark)
            || !system.alpha_image.is_empty()
        {
            return false;
        }
        let mut next = system.holds_what;
        // A chain that has not ended after visiting every type repeats one.
        for _ in 0..self.particle_types.len() {
            let Some(id) = next else {
                return true;
            };
            let Some(particle) = self.particle_types.get(id.0 as usize) else {
                return false;
            };
            if !matches!(particle.behaves_like, Particle::Smoke | Particle::Spark)
                || particle.damage != 0
                || particle.warhead.is_some()
                || !particle.alpha_image.is_empty()
                || particle.ares_damage_range.bits() & !(1 << 63) != 0
            {
                return false;
            }
            next = particle.next_particle;
        }
        true
    }

    /// Resolve an ordinal in the native ParticleSystemType registry A83D6C.
    pub fn particle_system_type_id_by_index(&self, index: i32) -> Option<ParticleSystemTypeId> {
        let index = usize::try_from(index).ok()?;
        self.particle_system_types
            .get(index)
            .map(|_| ParticleSystemTypeId(index as u32))
    }

    /// Resolve a particle type name to its ID (case-insensitive).
    pub fn p_type_id_by_name(&self, name: &str) -> Option<ParticleTypeId> {
        self.particle_types
            .iter()
            .position(|particle| particle.name.eq_ignore_ascii_case(name))
            .map(|index| ParticleTypeId(index as u32))
    }

    /// Resolve a particle system type name to its ID (case-insensitive).
    pub fn ps_type_id_by_name(&self, name: &str) -> Option<ParticleSystemTypeId> {
        crate::util::native_string::with_ascii_uppercase(name, |key| {
            self.particle_system_types_by_name.get(key).copied()
        })
    }

    /// Parsed VoxelAnim Types in their retained registry order.
    pub fn voxel_anim_types_iter(&self) -> impl Iterator<Item = &VoxelAnimType> {
        self.voxel_anim_types.iter()
    }

    /// Look up a `[VoxelAnims]` type by ID. Panics if `id` is out of range.
    pub fn voxel_anim_type(&self, id: VoxelAnimTypeId) -> &VoxelAnimType {
        &self.voxel_anim_types[id.0 as usize]
    }

    /// [`Self::voxel_anim_type`], or `None` for an id these rules do not hold.
    pub(crate) fn find_voxel_anim_type(&self, id: VoxelAnimTypeId) -> Option<&VoxelAnimType> {
        self.voxel_anim_types.get(id.0 as usize)
    }

    /// Resolve a `[VoxelAnims]` type name to its ID (case-insensitive).
    pub fn voxel_anim_type_id_by_name(&self, name: &str) -> Option<VoxelAnimTypeId> {
        crate::util::native_string::with_ascii_uppercase(name, |key| {
            self.voxel_anim_types_by_name.get(key).copied()
        })
    }

    /// Number of `[VoxelAnims]` types loaded, also used to validate saved IDs.
    pub fn voxel_anim_type_count(&self) -> usize {
        self.voxel_anim_types.len()
    }

    /// Number of particle types loaded from `[Particles]`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn particle_type_count(&self) -> usize {
        self.particle_types.len()
    }

    /// Number of particle system types loaded from `[ParticleSystems]`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn particle_system_type_count(&self) -> usize {
        self.particle_system_types.len()
    }

    /// `group`'s `[General] Prerequisite*` list.
    pub(crate) fn prerequisite_list(&self, group: PrerequisiteGroup) -> &[Prerequisite] {
        &self.prerequisite_lists[group.index()]
    }

    pub(crate) fn ares_prerequisite_group(
        &self,
        entry: Prerequisite,
    ) -> Option<&GenericPrerequisite> {
        self.ares_prerequisites
            .as_ref()?
            .group(entry.group_index()?)
    }

    /// Ares TechnoTypeExt::IsGenericPrerequisite1003DD00 and its six Foot
    /// lifecycle callers1004E750..1004E8B9. For a normal fresh match Types
    /// finish loading before these checks. Retain an immutable O(1) membership
    /// projection, rather than scanning every group for each live object.
    /// Original cache/live-reread limits: rules_oracle/ares_prerequisites.md.
    pub(crate) fn is_ares_prerequisite_alternate(
        &self,
        category: ObjectCategory,
        id: &str,
    ) -> bool {
        self.object_category_index
            .get(category, id)
            .is_some_and(|handle| self.ares_prerequisite_alternates.contains(&handle))
    }

    /// Resolve BuildingType FreeUnit, read by Grand_Opening446AA9. The native
    /// tail does not require Refinery; the type reader has already allocated
    /// a referenced UnitType, including names absent from VehicleTypes.
    pub fn building_free_unit(&self, structure_id: &str) -> Option<&str> {
        let obj = self.object(structure_id)?;
        let free_unit = obj.free_unit.as_deref()?;
        let resolved = self.object(free_unit)?;
        Some(resolved.id.as_str())
    }

    /// Whether a harvester type may dock at a specific structure according to Dock=.
    pub fn harvester_can_dock_at(&self, harvester_id: &str, structure_id: &str) -> bool {
        let Some(harvester) = self.object(harvester_id) else {
            return false;
        };
        let Some(_structure) = self.object(structure_id) else {
            return false;
        };
        harvester
            .dock
            .iter()
            .any(|dock| dock.eq_ignore_ascii_case(structure_id))
    }
}

impl RuleSet {
    /// Total number of game objects across all categories.
    #[cfg(test)]
    pub fn object_count(&self) -> usize {
        self.object_list.len()
    }

    /// Total number of weapons.
    #[cfg(test)]
    pub fn weapon_count(&self) -> usize {
        self.weapons.len()
    }

    /// Total number of warheads.
    #[cfg(test)]
    pub fn warhead_count(&self) -> usize {
        self.warheads.len()
    }

    /// Iterate all parsed warhead types.
    pub fn warheads_iter(&self) -> impl Iterator<Item = &WarheadType> {
        self.warheads
            .values()
            .map(|handle| &self.warhead_types[handle.0 as usize])
    }

    /// Every warhead with its ID, in the ID map's order.
    pub(super) fn warhead_entries(&self) -> impl Iterator<Item = (&String, &WarheadType)> {
        self.warheads
            .iter()
            .map(|(id, handle)| (id, &self.warhead_types[handle.0 as usize]))
    }

    /// Iterate all parsed weapon types.
    pub fn weapons_iter(&self) -> impl Iterator<Item = &WeaponType> {
        self.weapons
            .values()
            .map(|&handle| self.weapon_by_handle(handle))
    }

    /// Every weapon with its ID, in the ID map's order.
    pub(super) fn weapon_entries(&self) -> impl Iterator<Item = (&String, &WeaponType)> {
        self.weapons
            .iter()
            .map(|(id, &handle)| (id, self.weapon_by_handle(handle)))
    }

    /// Total number of projectiles.
    #[cfg(any(test, feature = "test-support"))]
    pub fn projectile_count(&self) -> usize {
        self.projectiles.len()
    }

    /// Iterate over all game objects in the registry.
    pub fn all_objects(&self) -> impl Iterator<Item = &ObjectType> {
        self.object_list.iter()
    }

    /// Visit AlphaImage names only for typed families whose native
    /// ObjectTypeClass reader has an allocated Rust owner. AnimType names are
    /// restricted to ART bodies admitted by the Rules processor.
    pub(crate) fn visit_alpha_image_names(&self, mut visit: impl FnMut(&str)) {
        for object in &self.object_list {
            visit(&object.alpha_image);
        }
        for projectile in self.projectiles_iter() {
            visit(&projectile.alpha_image);
        }
        for particle in &self.particle_types {
            visit(&particle.alpha_image);
        }
        for system in &self.particle_system_types {
            visit(&system.alpha_image);
        }
        for (_, smudge) in self.smudge_types.iter_with_id() {
            visit(&smudge.alpha_image);
        }
        for terrain in self.terrain_object_types.values() {
            visit(&terrain.alpha_image);
        }
        for voxel_anim in &self.voxel_anim_types {
            visit(&voxel_anim.alpha_image);
        }
        for image_name in self.overlay_registry().alpha_images() {
            visit(image_name);
        }
        for (name, admitted) in &self.anim_type_art_read_states {
            if *admitted
                && let Some(config) = self.art_registry.anim_runtime_config(name)
                && config.art_body_read
            {
                visit(&config.alpha_image);
            }
        }
    }
}
