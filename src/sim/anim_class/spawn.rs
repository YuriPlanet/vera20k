//! Making anims: the AnimClass constructor and its draws, the producers that
//! call it (effects, combat explosions, trigger, meteor and drop-zone anims,
//! bullet warnings, named follow-ups), cell occupancy and load-time tile anims.

use super::ai::{middle_frame, native_loop_remaining};
use super::bounce::{BOUNCER_GRAVITY, bouncer_launch};
use super::{
    AnimDisplayState, AnimId, AnimObject, AnimOccupationOperation, AnimRemap, AnimRuntime,
    AnimSpawnError, AnimWorldCoord, COMBAT_EXPLOSION_DRAW_FLAGS, COMBAT_EXPLOSION_Z_ADJUST,
    TRAILER_DRAW_FLAGS, anim_world_coords, apply_anim_raw_occupation,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::overlay_types::TiberiumImageError;
use crate::rules::ruleset::RuleSet;
use crate::sim::bounce::BounceState;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::{InternKey, InternedId};
use crate::sim::occupancy::{RawCellKey, infantry_raw_occupation_mask};
use crate::sim::timer::CdTimer;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF64Bits, NativeX87Error};

impl Simulation {
    pub fn anim(&self, id: AnimId) -> Option<&AnimObject> {
        self.substrate.anims.get(id)
    }

    pub fn anims(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.substrate.anims.iter()
    }

    /// Phobos `AnimClass_DrawIt_Visibility @ 0x423061`
    /// (`Ext/Anim/Hooks.cpp:520-570`): the `RestrictVisibilityIfCloaked=`
    /// skip followed by the `VisibleTo` owner-house gate.
    /// `VisibleTo.ConsiderInvokerAsOwner` replaces the owner object's house
    /// with the anim's invoker (or invoker house) first.
    ///
    /// RESIDUAL: the cloak arm's trailer-anim fallback
    /// (`pExt->IsTechnoTrailerAnim` then `Invoker` when `OwnerObject` is not a
    /// techno) has no trailer-anim marker in VERA; such an anim keeps the
    /// non-techno answer.
    pub fn anim_visibility_skipped(
        &self,
        id: AnimId,
        rules: &RuleSet,
        viewer: Option<InternedId>,
    ) -> bool {
        use crate::rules::phobos_shields::AffectedHouse;
        if !rules.phobos() {
            return false;
        }
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let Some(config) = rules
            .art()
            .anim_runtime_config(self.interner.resolve(anim.type_id()))
        else {
            return false;
        };
        let visible_to = config.phobos_visible_to.unwrap_or(AffectedHouse::ALL);
        if !config.phobos_restrict_visibility_if_cloaked && visible_to == AffectedHouse::ALL {
            return false;
        }
        let Some(viewer) = viewer.or_else(|| self.session().current_house()) else {
            return false;
        };
        if self.session().is_observer_house(viewer) {
            return false;
        }
        if config.phobos_restrict_visibility_if_cloaked
            && let Some(owner_id) = anim.owner_entity()
            && let Some(owner) = self.entities().get(owner_id)
            && matches!(owner.cloak.as_ref().map_or(0, |cloak| cloak.state()), 1 | 2)
            && !self
                .house_alliances()
                .is_allied(self.interner(), viewer, owner.owner())
            // `pTechno->GetCell()->Sensors_InclHouse(current)`: a sensor over
            // the owner's cell reveals it regardless of the alliance.
            && !self
                .fog()
                .has_sensor_for_house(viewer, owner.position.rx(), owner.position.ry())
        {
            return true;
        }
        // `pOwner = OwnerObject ? OwnerObject->GetOwningHouse() : pThis->Owner`.
        let mut owner = anim
            .owner_entity()
            .and_then(|owner_id| self.entities().get(owner_id))
            .map(|entity| entity.owner())
            .or_else(|| anim.owner_house());
        if config.phobos_visible_to_consider_invoker_as_owner {
            if let Some(invoker) = anim.invoker() {
                owner = self.entities().get(invoker).map(|entity| entity.owner());
            } else if let Some(house) = anim.invoker_house() {
                owner = Some(house);
            }
        }
        let Some(owner) = owner else {
            return false;
        };
        !crate::sim::combat::can_target_house(
            visible_to,
            viewer,
            owner,
            self.houses(),
            self.house_alliances(),
            self.interner(),
        )
    }

    /// The anim's entity owner is Cloaking (1) or Cloaked (2): the
    /// `Translucency.Cloaked` gate in `AnimClass_DrawIt_Translucency`
    /// (`0x423183`, `Ext/Anim/Hooks.cpp:573-615`). The presentation caller
    /// must apply the `rules.phobos()` switch to this Phobos-only query.
    pub fn anim_owner_is_cloaked(&self, id: u64) -> bool {
        let Some(owner_id) = self.anim(id).and_then(AnimObject::owner_entity) else {
            return false;
        };
        self.entities().get(owner_id).is_some_and(|owner| {
            matches!(owner.cloak.as_ref().map_or(0, |cloak| cloak.state()), 1 | 2)
        })
    }

    /// An anim's stage (`AnimClass+0xAC`) and its image's frame count (the
    /// type's GetImage, vt+0x9C, SHP header `+0x6`; 0 for an image that never
    /// bound), or `None` once the anim has left the store. The superweapons
    /// pace themselves by it: the Psychic Dominator's followed anim
    /// (`PsyDom::Process @ 0x0053AF40`) and the Lightning Storm's clouds
    /// (`LightningStorm::Process @ 0x0053A6C0`).
    pub(crate) fn anim_stage_and_frames(&self, id: AnimId, rules: &RuleSet) -> Option<(i32, i32)> {
        let anim = self.anim(id)?;
        let frames = self
            .interner
            .anim_type_config(anim.type_id, rules)
            .and_then(|config| config.raw_shp_frame_count)
            .unwrap_or(0);
        Some((anim.runtime.current_frame, frames))
    }

    /// `AnimClass::MarkCellOccupancy` (vt+0xF0 = `0x00426270`) at the anim's
    /// stored Location, called directly: InfantryClass::ReceiveDamage's
    /// InfDeath 9 arm marks its new InfantryMutate anim (`0x00518B26`),
    /// whatever the anim's `MakeInfantry=`.
    pub(crate) fn anim_mark_cell_occupancy(
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: AnimId,
    ) {
        if let Some(location) = substrate.anims.get(id).map(|anim| anim.world_coord) {
            Self::apply_make_infantry_raw_occupation(
                substrate,
                resolved_terrain,
                location,
                AnimOccupationOperation::Mark,
            );
        }
    }

    /// `AnimClass::MarkCellOccupancy` (`0x00426270`) and `ClearCellOccupancy`
    /// (`0x00426300`) for a MakeInfantry anim at its raw Location: the sub-cell
    /// (`0x004810A0`), the cell through Map[coord] (`0x00565730`), whose miss
    /// is the shared dummy cell, and the floor there (`0x00578080`).
    pub(super) fn apply_make_infantry_raw_occupation(
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        location: AnimWorldCoord,
        operation: AnimOccupationOperation,
    ) {
        let mask = infantry_raw_occupation_mask(
            SimFixed::from_num(location.x & 0xff),
            SimFixed::from_num(location.y & 0xff),
        );
        let (key, deck_z, live_structural_bridge) = match resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let cell = cells.lookup_world(location.x, location.y);
                let point = crate::sim::components::DriveCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                };
                let ground_z =
                    crate::sim::movement::ground_pose::query_ground_height(&cells, point)
                        .unwrap_or(0);
                // The mark reads live Cell+140 bit100. A stamped side cell needs
                // no own overlay (constructor5FC380, bridge_constructor.json
                // cases8..11).
                let live_structural_bridge = cells.flags(cell) & 0x100 != 0;
                (
                    RawCellKey::from_native(terrain, cell),
                    crate::sim::movement::ground_pose::deck_surface_z(&cells, cell, ground_z),
                    live_structural_bridge,
                )
            }
            None => {
                let Some((rx, ry)) = Self::map_coord_cell(resolved_terrain, location.x, location.y)
                else {
                    return;
                };
                (
                    RawCellKey::Real(rx, ry),
                    crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
                    false,
                )
            }
        };
        apply_anim_raw_occupation(
            &mut substrate.raw_cell_occupation,
            key,
            mask,
            location.z,
            deck_z,
            live_structural_bridge,
            operation,
        );
    }

    /// Construct the `AnimClass` a producer described by cell, sub-cell and
    /// height level.
    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_anim_object(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
    ) -> Result<AnimId, AnimSpawnError> {
        let world_coord = AnimWorldCoord::from_cell_sub_z(
            descriptor.rx,
            descriptor.ry,
            descriptor.sub_x,
            descriptor.sub_y,
            descriptor.z,
        );
        self.spawn_anim_at_world(rules, descriptor, world_coord)
    }

    /// [`Self::spawn_anim_object`] for a muzzle flash, impact or trailer: it
    /// takes [`Self::effect_anim_constructor`], so after
    /// [`Self::present_effect_anims`] a picture-only type is a presentation
    /// anim, as in a match.
    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_effect_anim_object(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
    ) -> Result<AnimId, AnimSpawnError> {
        let world_coord = AnimWorldCoord::from_cell_sub_z(
            descriptor.rx,
            descriptor.ry,
            descriptor.sub_x,
            descriptor.sub_y,
            descriptor.z,
        );
        let constructor = self.effect_anim_constructor(rules, descriptor.type_name);
        self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            constructor,
            &mut FrameEffects::default(),
        )
    }

    /// Construct one combat explosion as a real `AnimClass` instance.
    ///
    /// gamemd-derived: the warhead's `AnimList=` pick reaches
    /// `AnimClass::AnimClass @ 0x00421EA0` from `BulletClass::DetonateAtCoord`
    /// at `0x00469C93` (and identically at `0x0046A2F6`) with the argument row
    /// `(type, &impactCoord, delay 0, loopCount 1, drawFlags 0x2600,
    /// zAdjust -15, reverse 0)`. Read as raw pushes over
    /// `0x00469C40..0x00469CA0`: `PUSH 0x0` (reverse) at `0x00469C65`, then the
    /// by-value `CoordStruct` the `RET 0xC` helper at `0x0048ACE0` consumes,
    /// then `PUSH EAX` (-15), `PUSH 0x2600`, `PUSH 0x1`, `PUSH 0x0`,
    /// `PUSH ECX` (coord), `PUSH EBX` (type).
    ///
    /// Because `delay` is zero, `AnimClass::AnimClass` calls `AnimClass::Start
    /// @ 0x00424CE0` before returning — that is where `Report=`/`StartSound=`
    /// is played, from the single `AnimTypeClass+0x2F8` slot. So an explosion's
    /// sound is a constructor-time effect, not a first-tick one.
    ///
    /// Returns `None` if the type or its required runtime bounds never bound
    /// (see `ArtRegistry::bind_anim_class_assets`). A missing SHP is a bound
    /// native null image: its zero-frame animation still constructs and runs
    /// Start/Middle effects, then retires through the existing AI lifecycle.
    ///
    /// The anim is constructed at the producer's exact impact Z (`world_z`);
    /// `z` is only the level byte of the descriptor.
    pub fn spawn_combat_explosion_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
        z: u8,
        world_z: i32,
    ) -> Option<AnimId> {
        self.spawn_combat_explosion_anim_with_z_adjust(
            rules,
            type_name,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            world_z,
            COMBAT_EXPLOSION_Z_ADJUST,
            None,
            true,
        )
    }

    /// [`Self::spawn_combat_explosion_anim`] with the Phobos warhead
    /// `AnimZAdjust` substituted for the stock combat row's -15 and the
    /// tail's invoker House installed after construction. Unless the caller
    /// `keeps` the anim, a picture-only type runs as a presentation anim
    /// ([`Self::effect_anim_constructor`]); [`Self::spawn_combat_explosion_anim`]
    /// returns an anim its callers may keep (the nuke and Ares' PreImpactAnim
    /// await theirs), so it always makes an ordinary one.
    pub(crate) fn spawn_combat_explosion_anim_with_z_adjust(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: crate::util::fixed_math::SimFixed,
        sub_y: crate::util::fixed_math::SimFixed,
        z: u8,
        world_z: i32,
        z_adjust: i32,
        owner_house: Option<InternedId>,
        keeps: bool,
    ) -> Option<AnimId> {
        let descriptor = AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: COMBAT_EXPLOSION_DRAW_FLAGS,
            z_adjust,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let world_coord = AnimWorldCoord {
            z: world_z,
            ..AnimWorldCoord::from_cell_sub_z(rx, ry, sub_x, sub_y, z)
        };
        let constructor = if keeps {
            AnimConstructorInput::Runtime
        } else {
            self.effect_anim_constructor(rules, type_name)
        };
        match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            constructor,
            &mut FrameEffects::default(),
        ) {
            Ok(id) => {
                if let Some(house) = owner_house {
                    self.install_anim_owner_house(rules, type_name, id, house);
                }
                Some(id)
            }
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("combat explosion refused an invalid Tiberium Image start: {error}");
                None
            }
            Err(error) => {
                log::debug!(
                    "combat explosion [{}] did not construct: {error}",
                    self.interner.resolve(type_name)
                );
                None
            }
        }
    }

    /// `AnimExt::CreateRandomAnim`'s construction half: a new anim at
    /// `world_coord` with the `AnimClass` constructor defaults (`loop=1`,
    /// flags `0x600`) and `owner_house` installed through the same
    /// `SetAnimOwnerHouseKind(..., false, true)` tail the combat producer
    /// `SetAnimOwnerObject` (native `ownedObject=true`). RESIDUAL: a
    /// `MakeInfantry=`/`CreateUnit=` type re-selects its own owner kind here;
    /// this owner keeps the invoker House, like the existing combat producer.
    /// `invoker` is `CreateRandomAnim`'s `pTechno` under its `invoker` flag
    /// (the same object may or may not also be the `ownedObject`).
    pub(crate) fn spawn_owned_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        world_coord: AnimWorldCoord,
        owner_house: InternedId,
        owner_object: Option<u64>,
        invoker: Option<u64>,
    ) -> Option<AnimId> {
        let (rx, ry, sub_x, sub_y, z) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            owner: owner_object,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let id = self
            .spawn_anim_at_world(rules, descriptor, world_coord)
            .ok()?;
        self.install_anim_owner_house(rules, type_name, id, owner_house);
        if let Some(invoker) = invoker {
            self.substrate
                .set_anim_invoker(id, Some(invoker), Some(owner_house));
        }
        Some(id)
    }

    /// Phobos `VoxelAnimClass_AI_Trailer @ 0x74A70E`
    /// (`Ext/VoxelAnim/Hooks.cpp:47-67`): `GameCreate<AnimClass>(TrailerAnim,
    /// Location, 1, 1)` then `SetAnimOwnerHouseKind(OwnerHouse, nullptr,
    /// false, true)`. A null owner house leaves the anim unowned, as the
    /// default kind does.
    ///
    /// RESIDUAL: a `MakeInfantry=`/`CreateUnit=` trailer type re-selects its
    /// own owner kind in native, as [`Self::spawn_owned_anim`] records; this
    /// keeps the invoker house like the combat producer.
    pub(crate) fn spawn_trailer_anim(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        world_coord: AnimWorldCoord,
        owner_house: Option<InternedId>,
    ) -> Option<AnimId> {
        let (rx, ry, sub_x, sub_y, z) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: 1,
            draw_flags: TRAILER_DRAW_FLAGS,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let id = self
            .spawn_anim_at_world(rules, descriptor, world_coord)
            .ok()?;
        if let Some(house) = owner_house {
            self.install_anim_owner_house(rules, type_name, id, house);
        }
        Some(id)
    }

    /// `AnimExt::CreateRandomAnim`'s name/random half
    /// (`Ext/Anim/Body.cpp:407-432`), shared by the Phobos consumers: an
    /// unresolvable `ValueableVector<AnimTypeClass*>` token never reached the
    /// native vector, so only names with a type take part in
    /// `RandomRanged(0, size - 1)`; an empty list draws nothing.
    pub(crate) fn resolve_random_anim_name<'n>(
        &mut self,
        rules: &RuleSet,
        names: &'n [String],
    ) -> Option<&'n str> {
        let anims: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|name| rules.has_anim_type(name))
            .collect();
        if anims.is_empty() {
            return None;
        }
        let pick = self
            .scenario_rng
            .next_range_i32_inclusive(0, anims.len() as i32 - 1) as usize;
        Some(anims[pick])
    }

    /// [`Self::resolve_random_anim_name`] plus construction with the owner
    /// house and owner object.
    pub(crate) fn spawn_random_owned_anim(
        &mut self,
        rules: &RuleSet,
        names: &[String],
        world_coord: AnimWorldCoord,
        owner_house: InternedId,
        owner_object: Option<u64>,
        invoker: Option<u64>,
    ) -> Option<AnimId> {
        let name = self.resolve_random_anim_name(rules, names)?;
        let type_name = self.interner.intern(name);
        self.spawn_owned_anim(
            rules,
            type_name,
            world_coord,
            owner_house,
            owner_object,
            invoker,
        )
    }

    /// `AnimExt::SetAnimOwnerHouseKind(anim, house, None, false, true)`'s
    /// post-construction tail: install the owner House and, for a
    /// `MakeInfantry=` or remapping `CreateUnit=` type, its light conversion.
    pub(crate) fn install_anim_owner_house(
        &mut self,
        rules: &RuleSet,
        type_name: InternedId,
        id: AnimId,
        owner_house: InternedId,
    ) {
        self.substrate.set_anim_owner_house(id, owner_house);
        let remaps = self.interner.resolve(type_name);
        let remaps = rules
            .art()
            .anim_runtime_config(remaps)
            .is_some_and(|config| {
                config.make_infantry > -1
                    || (rules.phobos()
                        && config
                            .phobos_create_unit
                            .as_ref()
                            .is_some_and(|settings| settings.remap_anim))
            });
        let defeated = self
            .houses
            .get(&owner_house)
            .is_some_and(|state| state.is_defeated());
        if remaps && !defeated {
            self.substrate.anims.set_anim_house_remap(id, owner_house);
        }
    }

    /// `0x6FF42B` marks the anim the muzzle block just built and counts it
    /// into `FiringAnimUpdateCount`; the anim also holds it for its own AI.
    pub(crate) fn attach_firing_anim(
        &mut self,
        id: AnimId,
        state: crate::sim::anim_class::FiringAnimState,
    ) {
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.set_firing_anim(state);
            *self.firing_anim_update_count = self.firing_anim_update_count.saturating_add(1);
        }
    }

    pub fn firing_anim_update_count(&self) -> u32 {
        *self.firing_anim_update_count
    }

    pub(crate) fn release_firing_anim_count(&mut self, anim: &AnimObject) {
        if anim.firing_anim().is_some() {
            *self.firing_anim_update_count = self.firing_anim_update_count.saturating_sub(1);
        }
    }

    /// The constructor a muzzle flash, an impact or a trailer of type
    /// `type_id` takes: a presentation anim when the match runs them
    /// ([`Self::present_effect_anims`]) and the type changes nothing but
    /// its picture ([`crate::rules::art_data::ArtRegistry::anim_presentation_only`]),
    /// else an ordinary one. Only producers that keep no hold on the anim ask.
    pub(crate) fn effect_anim_constructor(
        &self,
        rules: &RuleSet,
        type_id: InternedId,
    ) -> AnimConstructorInput {
        // The set of types that affect play is keyed by name: one probe with
        // the name uppercased on the stack, no allocation.
        if self.effect_anims_presented()
            && rules
                .art()
                .anim_presentation_only(self.interner.resolve(type_id))
        {
            AnimConstructorInput::Presentation
        } else {
            AnimConstructorInput::Runtime
        }
    }

    pub fn spawn_anim_at_world(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
        world_coord: AnimWorldCoord,
    ) -> Result<AnimId, AnimSpawnError> {
        self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            AnimConstructorInput::Runtime,
            &mut FrameEffects::default(),
        )
    }

    /// Original TAction41 PlayAnimAtWP6E2290, reached at6DECB0.
    /// Resolve8B4154[+90], use the shared signed waypoint/Ground/bridge
    /// query, then constructor421EA0 with delay0/loop1/flags600. Native
    /// sets+198 after the constructor returns, so its first Start effects
    /// remain admitted. Invalid Rust input safely fails instead of taking
    /// the original unchecked pointer access; stock operands are valid.
    ///
    /// Ares 3.0 ActionClass_PlayAnimAt 10025C20 (hook 0x6E232E) gives a
    /// `MakeInfantry=` type SetMakeInfOwner with the action's House as
    /// invoker, victim and killer.
    pub(crate) fn trigger_play_anim_at_waypoint(
        &mut self,
        rules: &RuleSet,
        anim_type_index: i32,
        waypoint: i32,
        house: Option<InternedId>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(type_name) = rules.anim_type_runtime_key_by_index(anim_type_index) else {
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        let world_coord = AnimWorldCoord { x, y, z };
        let type_id = self.interner.intern(&type_name);
        let (rx, ry, sub_x, sub_y, level) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
        };
        let id = match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            AnimConstructorInput::Runtime,
            frame_effects,
        ) {
            Ok(id) => id,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("waypoint animation refused an invalid Tiberium Image start: {error}");
                return false;
            }
            Err(_) => return false,
        };
        if rules.ares_form()
            && rules
                .art()
                .anim_runtime_config(&type_name)
                .is_some_and(|config| config.make_infantry > -1)
        {
            self.ares_set_make_inf_owner(id, type_id, house, house, house, rules);
        }
        self.substrate.set_anim_no_effects(id, true);
        true
    }

    /// The MoveFlash of `FootClass::ClickedAction_Cell`'s Move arm
    /// (`0x004D7EB5..0x004D7F98`): `AnimClass(Rules+0xBB4, coord, 0, 1,
    /// 0x600, 0, false)` at the clicked Cell's centre, on the ground
    /// (`0x00578080`) and raised by the bridge deck when the Cell at that
    /// coordinate (`0x00565730`) has flag 0x100. Campaign and skirmish
    /// (GameMode 0 and 5) keep it an ordinary anim. LAN and online games make
    /// it a local anim instead (`0x004D7F28..0x004D7F57`,
    /// `0x004D7FA6..0x004D8062`), which only the clicking game runs and
    /// draws. False when nothing is constructed.
    ///
    /// RESIDUAL: a local anim runs the whole AnimClass::AI, as gamemd's does.
    /// A MoveFlash type that damages, leaves a scorch or crater, spawns
    /// particles, a `TrailerAnim=` or a `Next=` anim or marks `MakeInfantry=`
    /// cells would change the clicking game alone, as in gamemd. A
    /// `Layer=Ground` one keeps its own list (`DisplayLayers::local_ground`),
    /// so it moves no shared member, but draws after every Ground member
    /// rather than at its sort key. Retail RING does none of these.
    pub(crate) fn spawn_move_flash(&mut self, rules: &RuleSet, cell: (u16, u16)) -> bool {
        if rules.general.move_flash.name.is_empty() {
            return false;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        // 0x004D7EC2..0x004D7EED: the signed Cell words, times 0x100, plus 0x80.
        let centre = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(cell.0 as i16),
                y: i32::from(cell.1 as i16),
            },
            0,
        );
        let Ok(mut z) = crate::sim::movement::ground_pose::query_ground_height(
            &cells,
            crate::sim::components::DriveCoord {
                x: centre.x,
                y: centre.y,
                z: 0,
            },
        ) else {
            return false;
        };
        let target = cells.lookup_world(centre.x, centre.y);
        if cells.flags(target) & 0x100 != 0 {
            z = crate::sim::movement::ground_pose::deck_surface_z(&cells, target, z);
        }
        let world_coord = AnimWorldCoord {
            x: centre.x,
            y: centre.y,
            z,
        };
        let local = !self.session.game_mode().is_single_player();
        let name = &rules.general.move_flash.name;
        // `resolve_type_handles` interned it; a local flash never adds a
        // name, which the other games would lack.
        let type_id = if local {
            let Some(type_id) = self.interner.get(name) else {
                log::debug!("MoveFlash [{name}] was not interned with the type handles");
                return false;
            };
            type_id
        } else {
            self.interner.intern(name)
        };
        let (rx, ry, sub_x, sub_y, level) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
        };
        let constructor = if local {
            AnimConstructorInput::Local
        } else {
            AnimConstructorInput::Runtime
        };
        match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            world_coord,
            constructor,
            &mut FrameEffects::default(),
        ) {
            Ok(_) => true,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("MoveFlash refused an invalid Tiberium Image start: {error}");
                false
            }
            Err(error) => {
                log::debug!("MoveFlash did not construct: {error}");
                false
            }
        }
    }

    /// Execute58 MeteorShower6E2C40..6E2E12 (Steam gamemd). Literal types
    /// are find-only; raw signed XY remainders precede each constructor, whose
    /// launch/Start draws are visible to the following selection. 842AFC holds
    /// [1,5,9,15]. No House, SuperWeapon, no-effects flag or per-piece delay.
    pub(crate) fn trigger_meteor_shower(
        &mut self,
        rules: &RuleSet,
        intensity: i32,
        waypoint: i32,
    ) -> bool {
        let Some(base) = usize::try_from(intensity)
            .ok()
            .and_then(|i| [1i32, 5, 9, 15].get(i).copied())
        else {
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        let count = base.wrapping_add(crate::sim::rng::abs_before_modulo(
            self.scenario_rng.next_u32(),
            3,
        ));
        let spread = count.wrapping_mul(70);
        for _ in 0..count {
            let dx = (self.scenario_rng.next_u32() as i32).wrapping_rem(spread);
            let dy = (self.scenario_rng.next_u32() as i32).wrapping_rem(spread);
            let mut coord = AnimWorldCoord {
                x: x.wrapping_add(dx),
                y: y.wrapping_add(dy),
                z,
            };
            coord.z = crate::sim::projectile::projectile_ground_z(
                self.resolved_terrain.as_ref(),
                &self.effective_shared_cell_dummy(),
                crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
            );
            let name = if crate::sim::rng::abs_before_modulo(self.scenario_rng.next_u32(), 2) == 0 {
                "METSMALL"
            } else {
                "METLARGE"
            };
            //422B20 is find-only. An orphan ART section is not a registered
            //AnimType; safe Rust refuses the native invalid array access.
            if !rules.anim_type_names.contains(name) {
                return false;
            }
            if !self.spawn_named_anim(rules, name, coord, 0, 0x600, 0) {
                return false;
            }
        }
        true
    }

    /// TAction8 DropZoneFlare6E1CC0: its tail waypoint, inline ground/bridge
    ///coordinate, Rules+147C, then Anim421EA0(delay0,loop1,flags600). The
    ///post-constructor +198 write leaves first Start and DropZone reveal live.
    pub(crate) fn trigger_drop_zone_flare(&mut self, rules: &RuleSet, waypoint: i32) -> bool {
        let Some(type_name) = rules.general.drop_zone_anim.as_deref() else {
            //Native would pass the null pointer to the constructor.
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        let world_coord = AnimWorldCoord { x, y, z };
        let type_id = self.interner.intern(type_name);
        let (rx, ry, sub_x, sub_y, level) = world_coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
        };
        let id = match self.spawn_anim_at_world(rules, descriptor, world_coord) {
            Ok(id) => id,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("DropZone flare refused an invalid Tiberium Image start: {error}");
                return false;
            }
            Err(_) => return false,
        };
        self.substrate.set_anim_no_effects(id, true);
        true
    }

    /// Anim ctor42264D..4226BF: the bound DropZone type reveals twice for
    ///the current House. Rules+14BC is converted from leptons by signed /256;
    ///the existing Map5678E0 owner handles height, allies and notifications.
    /// Every native client reveals for its own PlayerPtr, so a network game
    /// reveals for each human House ([`Self::represented_player_maps`]).
    fn reveal_drop_zone_from_anim(&mut self, rules: &RuleSet, world: AnimWorldCoord) {
        let owners = self.represented_player_maps();
        // Preserve large positive inputs through the narrower Rust parameter;
        // Map567A3E/the shared reveal owner still applies the native cap10.
        let radius = (rules.general.drop_zone_radius / 256).min(i32::from(u16::MAX));
        let Ok(radius) = u16::try_from(radius) else {
            //Negative radii index before native's spiral table; keep the
            //existing RevealArea owner's safe supported-range boundary.
            return;
        };
        let (x, y) = crate::sim::movement::coord_cell(crate::sim::components::DriveCoord {
            x: world.x,
            y: world.y,
            z: world.z,
        });
        let config = self.sight_reveal_config(rules);
        for owner in owners {
            let clients = self.fog_viewers();
            let viewers = self.reveal_viewers.direct(
                self.house_alliances.as_map(),
                owner,
                &config,
                &self.interner,
                &clients,
            );
            for final_pass in [false, true] {
                let _ = self.reveal_area(
                    &viewers,
                    (x as u16, y as u16),
                    world.z,
                    radius,
                    None,
                    final_pass,
                    &config,
                    crate::sim::world::UninitContext::new(rules),
                    crate::sim::vision::MapRevealProjection::Transient,
                    self.session.current_house(),
                );
            }
        }
    }

    /// One port of Anim421EA0 for runtime and authored-load producers. An
    /// assigned identity skips only42203D; preconsumed death-debris input also
    /// retains draws already taken at that producer's native boundary.
    pub(crate) fn spawn_anim_at_world_with_constructor(
        &mut self,
        rules: &RuleSet,
        descriptor: AnimClassSpawnDescriptor,
        world_coord: AnimWorldCoord,
        constructor: AnimConstructorInput,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<AnimId, AnimSpawnError> {
        // Anim42203D assigns before registry insertion and RandomRate4221F5.
        // Death debris may already have constructed at its pick/draw boundary.
        let native_unique_id = match constructor {
            AnimConstructorInput::AssignedIdentity(id) => id,
            AnimConstructorInput::Preconsumed(draws) => draws
                .native_unique_id
                .unwrap_or_else(|| self.next_native_runtime_id()),
            AnimConstructorInput::Runtime => self.next_native_runtime_id(),
            // ClickedAction_Cell sets Scenario+0x214 to -3 around the
            // constructor (0x004D7F42) and restores it after (0x004D7FCB), so
            // NextUniqueID's pre-increment (0x0068BCB0) gives the flash -2,
            // the id the sync checksum skips, and the counter stays put.
            AnimConstructorInput::Local | AnimConstructorInput::Presentation => {
                crate::sim::multiplayer_checksum::SYNC_EXEMPT_ANIM_ID
            }
        };
        let presentation = matches!(constructor, AnimConstructorInput::Presentation);
        let local = presentation || matches!(constructor, AnimConstructorInput::Local);
        // The MoveFlash alone runs in the clicking game only; a presentation
        // anim runs in every game alike, so it may name what it plays.
        let private = local && !presentation;
        // Borrowed from the rules, which outlive the spawn: the record is
        // only read. Found by the type id's handle, matched as the name is.
        let config_handle = self
            .interner
            .anim_type_config_handle(descriptor.type_name, rules)
            .ok_or(AnimSpawnError::MissingType(descriptor.type_name))?;
        let config = rules.art().anim_config(config_handle);
        // The refusals name the type in uppercase, as the ART key is written.
        let (effective_end, effective_loop_end) = effective_bounds(config).ok_or_else(|| {
            AnimSpawnError::UnboundType(
                self.interner
                    .resolve(descriptor.type_name)
                    .to_ascii_uppercase(),
            )
        })?;
        let reverse = descriptor.reverse || config.reverse;
        // VERA-only: a local anim never adds a name to the interner, which the
        // other games would lack; a StopSound name not yet interned is left out.
        let stop_sound_id = config.stop_sound.as_deref().and_then(|sound| {
            if private {
                self.interner.get(sound)
            } else {
                Some(self.interner.intern(sound))
            }
        });
        if presentation {
            self.intern_presentation_chain(rules, config);
        }
        let stable_id = if local {
            self.substrate.anims.allocate_local_id()
        } else {
            self.substrate.allocate_stable_id()
        };
        if self.substrate.anims.contains_key(stable_id)
            || self.substrate.entities.contains(stable_id)
        {
            return Err(AnimSpawnError::DuplicateId(stable_id));
        }
        let object = AnimObject {
            stable_id,
            native_unique_id,
            type_id: descriptor.type_name,
            world_coord,
            draw_flags: descriptor.draw_flags,
            // Anim42219E: a zero constructor argument reads Type+348;
            // explicit nonzero producer adjustments take precedence.
            z_adjust: if descriptor.z_adjust == 0 {
                config.z_adjust
            } else {
                descriptor.z_adjust
            },
            remap: None,
            effective_end,
            effective_loop_end,
            runtime: AnimRuntime {
                current_frame: if reverse {
                    effective_loop_end.wrapping_sub(1)
                } else {
                    0
                },
                frame_step: if reverse { -1 } else { 1 },
                delay_remaining: i32::from(descriptor.delay),
                rate_reload: 0,
                frame_timer: CdTimer::default(),
                loop_remaining: native_loop_remaining(config.loop_count, descriptor.loop_count),
                first_ai_guard: true,
                constructor_reverse: descriptor.reverse,
                inactive: false,
                paused: false,
                temporal_paused: false,
            },
            draw_runtime: descriptor.draw_runtime,
            viewer_hidden: None,
            use_cell_drawer: descriptor.use_cell_drawer,
            terrain_attached: descriptor.terrain_attached,
            no_effects: false,
            firing_anim: None,
            damage_accumulator: NativeF64Bits::ONE,
            in_logic_vector: false,
            owner_entity: None,
            building_slot: None,
            building_anim: false,
            damage_fire_slot: None,
            completed: false,
            start_sound_active: false,
            stop_sound_id,
            display: AnimDisplayState {
                marked_on_map: false,
                // 0x004D7F57 writes -5000 to the MoveFlash type's YSortAdjust
                // (+0x340) before a local flash's constructor. RESIDUAL: gamemd
                // keeps that write on the type, so later anims of the type
                // sort with it too; VERA gives it to the flash alone. Retail
                // MoveFlash is RING, which nothing else makes, in Air.
                y_sort_adjust: if private { -5000 } else { config.y_sort_adjust },
            },
            bounce: None,
            flaming_guy: None,
            owner_house: None,
            attached_bullet: None,
            invoker: None,
            invoker_house: None,
            from_death_unit: false,
            death_unit_facing: None,
            death_unit_has_turret: false,
            death_unit_turret_facing: None,
            owner_terrain: None,
            presentation: presentation.then_some(None),
            presentation_order: 0,
        };
        // The insert must run in every build profile: wrapped in
        // `debug_assert!` it was compiled out of release binaries and no
        // scheduler anim ever existed in a shipped build.
        let previous = self.substrate.anims.insert(object);
        debug_assert!(previous.is_none());
        let draws = match constructor {
            AnimConstructorInput::Preconsumed(draws) => draws,
            // A type whose constructor draws nothing needs no stream: the
            // local copy below would be thrown away unread.
            AnimConstructorInput::Local | AnimConstructorInput::Presentation
                if !constructor_draws(config) =>
            {
                AnimConstructorDraws::default()
            }
            AnimConstructorInput::Runtime
            | AnimConstructorInput::AssignedIdentity(_)
            | AnimConstructorInput::Local
            | AnimConstructorInput::Presentation => {
                // VERA-only: a local anim draws from a copy, so the other
                // games' Scenario RNG stays in step. gamemd draws from the
                // shared one; retail RING has no draw (no `RandomRate=`).
                let mut copy;
                let rng = if local {
                    copy = self.scenario_rng.clone();
                    &mut copy
                } else {
                    &mut self.scenario_rng
                };
                match anim_constructor_draws(config, world_coord, rng) {
                    Ok(draws) => draws,
                    Err(error) => {
                        // VERA's explicit unsupported-arithmetic failure precedes
                        // Reveal. The original constructor ID remains spent.
                        self.substrate.anims.remove(stable_id);
                        return Err(AnimSpawnError::LaunchOutOfDomain(
                            self.interner
                                .resolve(descriptor.type_name)
                                .to_ascii_uppercase(),
                            error,
                        ));
                    }
                }
            }
        };
        let rate_reload = Self::anim_rate(&self.session, config, draws.random_rate);
        let frame_timer =
            CdTimer::started(self.session.binary_frame() as i32, i32::from(rate_reload));
        let registered = self
            .substrate
            .anims
            .get_mut(stable_id)
            .expect("new Anim registry entry");
        registered.runtime.rate_reload = rate_reload;
        registered.runtime.frame_timer = frame_timer;
        registered.bounce = draws.bounce;
        // Meteor422433 moves Object.Location to the computed launch point
        // before Reveal/Start; ordinary Bouncer keeps the supplied location
        // while its physics starts ten leptons higher.
        if config.is_meteor
            && let Some(body) = registered.bounce
        {
            let start = body.position_leptons();
            registered.world_coord = AnimWorldCoord {
                x: start.x,
                y: start.y,
                z: start.z,
            };
        }
        let initial_world_coord = registered.world_coord;
        // Native registry insertion precedes Reveal, and Reveal precedes the
        // delay-zero constructor-time Start call. A producer's attach then
        // removes the Anim from Display and submits it to Ground again, so
        // Reveal's own submit is left out when nothing before the attach can
        // read or change Display. Start's mineral tail can construct children
        // and dispatch damage before the producer attaches its receiver.
        let start_reaches_display = descriptor.delay == 0
            && (config.tiberium_chain_reaction
                || (middle_frame(config).is_none()
                    && (config.scorch
                        || config.crater
                        || (config.spawns_particle.is_some() && config.num_particles > 0))));
        let attaches = descriptor
            .owner
            .is_some_and(|owner| self.anim_owner_coords(owner).is_some());
        self.reveal_anim(stable_id, rules, !attaches || start_reaches_display);
        //42264D..4226BF reveals DropZone before the constructor-time Start.
        if self.interner.is_drop_zone_anim(config_handle, rules) {
            self.reveal_drop_zone_from_anim(rules, initial_world_coord);
        }
        // RESIDUAL (VERA-only refusal): Start can refuse a mod's invalid
        // Tiberium Image start after the anim was registered, revealed and
        // given its start sound and Middle. The refusal returns with the
        // anim still stored, so a producer that writes after construction
        // (a map tile anim's ZAdjust, `runtime::spawn_terrain_tile_animations`)
        // leaves it without that write. Removing it here would not undo what
        // Start already did.
        if descriptor.delay == 0 {
            self.anim_start(
                stable_id,
                config,
                rules,
                Some(rules.overlay_registry()),
                frame_effects,
            )?;
        }
        if let Some(owner) = descriptor.owner {
            self.set_anim_owner_object(stable_id, Some(owner), rules);
        }
        Ok(stable_id)
    }

    /// Exact final-Init scalar deletion selector for first-sweep tile Anims.
    /// Removal rechecks the compacted registry slot and never enters the
    /// ordinary Destroy/UnInit pending-delete path.
    pub(crate) fn scalar_delete_load_terrain_anims(&mut self) -> usize {
        let mut index = 0;
        let mut removed = 0;
        while let Some(id) = self.substrate.anims.key_at(index) {
            let terrain_attached = self
                .substrate
                .anims
                .get(id)
                .is_some_and(|anim| anim.terrain_attached);
            if !terrain_attached {
                index += 1;
                continue;
            }
            // MapClass::InitCellAttributes568BB0 uses the same scalar
            // destructor4228E0: Release406060, without StopSound.
            self.scalar_delete_anim(id);
            removed += 1;
        }
        removed
    }

    /// One Map565730 lookup and Cell+44/+11E read. Live IsAnimatedTiberium
    /// AI424402 and post-load426224 use this same real/dummy field owner.
    pub(super) fn anim_cell_overlay_fields(&self, location: AnimWorldCoord) -> (Option<u8>, u8) {
        let grid_fields = |(rx, ry): (u16, u16)| {
            self.overlay_grid
                .as_ref()
                .filter(|grid| rx < grid.width() && ry < grid.height())
                .map(|grid| {
                    let cell = grid.cell(rx, ry);
                    (cell.overlay_id, cell.overlay_data)
                })
        };
        match self.resolved_terrain.as_ref() {
            Some(terrain) => {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                match cells.lookup_world(location.x, location.y) {
                    cell @ crate::map::cell_index::NativeCellIdentity::Real(_) => {
                        let (rx, ry) = cells.coord(cell);
                        grid_fields((rx as u16, ry as u16)).unwrap_or((None, 0))
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => {
                        cells.dummy().overlay_fields()
                    }
                }
            }
            None => Self::map_coord_cell(&self.resolved_terrain, location.x, location.y)
                .and_then(grid_fields)
                .unwrap_or_else(|| self.effective_shared_cell_dummy().overlay_fields()),
        }
    }

    /// Cell485020 value from raw Location via Object5F6960: two Map565730
    /// lookups. Keep its first lookup separate from the shared field read.
    pub(super) fn anim_cell_tiberium_value(
        &self,
        location: AnimWorldCoord,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Result<i32, TiberiumImageError> {
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let _ = cells.lookup_world(location.x, location.y);
        }
        let (overlay_id, overlay_data) = self.anim_cell_overlay_fields(location);
        crate::map::tiberium_cell::tiberium_value(
            overlay_id,
            overlay_data,
            overlay_registry,
            &rules.tiberium_types,
        )
    }

    /// Anims_Refresh_Tiberium_LightConverts4261D0, after user-load lighting:
    /// all flagged stored Anims sample raw Location, including inactive and
    /// attached entries. Non-resource cells keep the prior Convert. This is not
    /// the live AI's virtual coordinate minus384, and does not alter timers,
    /// intensity, depth, attachment or RNG.
    pub(crate) fn refresh_animated_tiberium_after_load(
        &mut self,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Result<(), TiberiumImageError> {
        // Remap writes have no callbacks. Snapshot the selected coordinates
        // in storage order so the shared setter retains touch invalidation.
        let candidates: Vec<_> = self
            .anims()
            .filter(|(_, anim)| {
                self.interner
                    .anim_type_config(anim.type_id, rules)
                    .is_some_and(|config| config.is_animated_tiberium)
            })
            .map(|(&id, anim)| (id, anim.world_coord))
            .collect();
        for (id, location) in candidates {
            let (Some(overlay), _) = self.anim_cell_overlay_fields(location) else {
                continue;
            };
            let Some(kind) =
                overlay_registry.tiberium_type_for_overlay(&rules.tiberium_types, overlay)?
            else {
                continue;
            };
            let tiberium = rules
                .tiberium_types
                .get(kind)
                .ok_or(TiberiumImageError::MissingType(kind))?;
            if let Ok(index) = u8::try_from(tiberium.color_scheme() / 2) {
                assert!(self.substrate.anims.set_anim_remap(
                    id,
                    AnimRemap::ColorScheme(crate::rules::house_colors::HouseColorIndex(index)),
                ));
            }
        }
        Ok(())
    }

    /// Original43B5E0: caller supplies the target's GetCoords and the name;
    /// construct delay0/loop1/flags600, then SetBullet424C90, SetHouse424CA0
    /// and hidden+19D. PointerExpired425150 owns the eventual bullet detach.
    pub(crate) fn spawn_bullet_warning(
        &mut self,
        rules: &RuleSet,
        name: &str,
        world: AnimWorldCoord,
        bullet: u64,
        house: InternedId,
    ) -> Option<AnimId> {
        let type_name = self.interner.intern(name);
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
        };
        let id = match self.spawn_anim_at_world(rules, descriptor, world) {
            Ok(id) => id,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!("bullet warning refused an invalid Tiberium Image start: {error}");
                return None;
            }
            Err(_) => return None,
        };
        self.substrate.set_anim_attached_bullet(id, Some(bullet));
        self.substrate.set_anim_owner_house(id, house);
        self.substrate.set_anim_hidden(id, true);
        Some(id)
    }

    /// `0x0043B4C0`, the test `AnimClass::AI` runs every frame for an anim
    /// whose type sets `PsiWarning=` (`AnimTypeClass+0x373`,
    /// `0x00423B29..0x00423B5C`) and stores negated as the hidden byte
    /// `+0x19D`: does `viewer` (native asks for the local player,
    /// `0x00A83D4C`) detect the anim? Not when it is allied with the anim's
    /// House (`HouseClass::IsAlliedWith @ 0x004F9A50`, null not allied).
    /// Otherwise, when one of its PsychicDetection buildings
    /// ([`Simulation::psychic_detection_sites`]) is operational and its
    /// GetCoords lies within the radius in leptons (`<< 8`, `jle` at
    /// `0x0043B5A9`) of the centre of the anim's cell (`MapClass::operator[]
    /// @ 0x00565730`, `CellClass::Get_Center_Coords @ 0x00480A30`), measured
    /// as `CoordStruct::Distance3D` (inline at `0x0043B547..0x0043B593`).
    /// The answer depends on the local player, so the presentation asks it
    /// in place of the hidden flag. Evidence: instruction reading only; no
    /// native run compares the distance's rounding at the radius edge.
    pub fn psi_warning_detected_by(
        &self,
        viewer: InternedId,
        anim: &AnimObject,
        rules: &RuleSet,
    ) -> bool {
        if anim.owner_house.is_some_and(|house| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                viewer,
                house,
            )
        }) {
            return false;
        }
        let terrain = self.resolved_terrain.as_ref();
        let at = anim_world_coords(anim, &self.substrate.entities, terrain);
        // The lookup stamps the fallback CellClass on a miss, as native's
        // does. The presentation asks this, so the stamp stays private to
        // the query: drawing must not change the simulation.
        let cell = self.effective_shared_cell_dummy().detached(|| {
            crate::sim::projectile::cell_ground_coord(
                terrain,
                (at.x / 256) as u16,
                (at.y / 256) as u16,
            )
        });
        self.psychic_detection_sites(viewer, rules)
            .any(|(site, radius)| {
                crate::util::native_x87::distance_3d_leptons(
                    [site.x, site.y, site.z],
                    [cell.x, cell.y, cell.z],
                ) <= radius
            })
    }

    /// `new AnimClass(AnimTypes[FindIndex(name)], &coord, delay, 1, flags,
    /// zAdjust, 0)`: a landing chunk's follow-up anims and the rocket puffs of
    /// Rocket Process and the spawn manager's Boomer launch. A type that never
    /// bound constructs nothing. This optional-effect boundary also reports
    /// and refuses an invalid Tiberium Image reached during the child's Start;
    /// the core constructor and the live Anim visitor return that error.
    pub(crate) fn spawn_named_anim<'k>(
        &mut self,
        rules: &RuleSet,
        type_name: impl Into<InternKey<'k>>,
        coord: AnimWorldCoord,
        delay: u16,
        draw_flags: u32,
        z_adjust: i32,
    ) -> bool {
        let type_name = type_name.into();
        let type_id = self.interner.intern_key(type_name);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay,
            loop_count: 1,
            draw_flags,
            z_adjust,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
        };
        match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(_) => true,
            Err(AnimSpawnError::Tiberium(error)) => {
                log::warn!(
                    "optional animation [{}] refused an invalid Tiberium Image start: {error}",
                    self.interner.key_name(type_name)
                );
                false
            }
            Err(error) => {
                log::debug!(
                    "anim [{}] did not construct: {error}",
                    self.interner.key_name(type_name)
                );
                false
            }
        }
    }
}

/// The type's effective End and LoopEnd, or `None` when its ART body was read
/// but no SHP frame count was bound.
pub(super) fn effective_bounds(config: &AnimTypeRuntimeConfig) -> Option<(i32, i32)> {
    if !config.art_body_read {
        return Some((config.end, config.loop_end));
    }
    let raw = config.raw_shp_frame_count?;
    let effective_end = if config.end == -1 {
        if config.shadow { raw / 2 } else { raw }
    } else {
        config.end
    };
    let effective_loop_end = if config.loop_end == -1 {
        effective_end
    } else {
        config.loop_end
    };
    Some((effective_end, effective_loop_end))
}

/// The Scenario draws `AnimClass::AnimClass @ 0x00421EA0` takes, in order:
/// the `RandomRate=` pick (`0x004221F5`; none when the clamped bounds are
/// equal, as for every stock debris chunk), then the `Bouncer=` arm's launch
/// (`0x004224D9..0x00422648`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnimConstructorDraws {
    /// Present when a producer has already crossed Anim42203D. Admission must
    /// retain that constructor identity rather than consume the cursor again.
    pub(crate) native_unique_id: Option<i32>,
    /// The drawn `RandomRate=` delay in logic frames, before normalization.
    pub random_rate: Option<u16>,
    pub bounce: Option<BounceState>,
}

/// Transient entry state for the single Anim constructor; never simulation
/// state. Fresh map loading may already own the Abstract ID, while death debris
/// may have crossed both identity and RNG boundaries before registry admission.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AnimConstructorInput {
    Runtime,
    AssignedIdentity(i32),
    Preconsumed(AnimConstructorDraws),
    /// A local anim ([`AnimStore`]'s `local`): the LAN and online MoveFlash
    /// of [`Simulation::spawn_move_flash`].
    Local,
    /// A presentation anim: a local anim of a type that changes nothing but
    /// its own picture ([`crate::rules::art_data::ArtRegistry::anim_presentation_only`]),
    /// which every game makes alike. It takes no Display slot, no Logic slot,
    /// no unique id and no Scenario draw, and nothing in the simulation holds
    /// it; the picture draws it where Display would hold it
    /// (`Simulation::picture_draw_order`).
    ///
    /// VERA-only sim-scale deviation, on in real matches
    /// ([`Simulation::present_effect_anims`]): gamemd keeps every muzzle flash and
    /// impact anim in Display (Ground's sorted vector) and Logic and gives each
    /// a unique id. Leaving them out changes the unique ids and the Ground order
    /// later objects take, and the state hash and sync checksum with them; a
    /// player sees the same picture. RESIDUAL: such an anim does not raise its
    /// owner's attached-Anim flag (Object+84, `SetOwnerObject @ 0x00424B50`),
    /// whose only reader caps the fall rate (`ObjectFall`); trigger: a Techno
    /// that starts falling within a few frames of firing; effect: it falls at
    /// the ordinary rate; frequency: rare (a bridge collapsing under a firing
    /// unit). It ends with its owner's UnInit, where gamemd detaches it in
    /// place; an owner that enters Limbo (a transport, a garrison) keeps it
    /// drawing until its own end, a few frames.
    Presentation,
}

/// Take the constructor's draws for an anim of `config` at `coord`.
///
/// IsMeteor4222FF takes the shared meteor launch prefix (three raw draws),
/// then Bounce4397E0's three axis draws, with no angular velocity. RandomRate
/// still precedes those calls; equal rate bounds make a request without a draw.
pub(crate) fn anim_constructor_draws(
    config: &AnimTypeRuntimeConfig,
    coord: AnimWorldCoord,
    rng: &mut crate::sim::rng::SimRng,
) -> Result<AnimConstructorDraws, NativeX87Error> {
    let random_rate = anim_random_rate(config, rng);
    let bounce = if config.is_meteor {
        let launch = crate::sim::bounce::meteor_launch(
            glam::IVec3::new(coord.x, coord.y, coord.z),
            // 4223D8: a literal lifetime that only places the start.
            70,
            config.min_z_vel,
            config.max_xy_vel,
            rng,
        )?;
        Some(BounceState::init(
            launch.start,
            config.elasticity,
            BOUNCER_GRAVITY,
            NativeF64Bits::POSITIVE_ZERO,
            launch.velocity,
            NativeF64Bits::POSITIVE_ZERO,
            rng,
        )?)
    } else if config.bouncer {
        Some(bouncer_launch(config, coord, rng)?)
    } else {
        None
    };
    Ok(AnimConstructorDraws {
        native_unique_id: None,
        random_rate,
        bounce,
    })
}

/// Whether [`anim_constructor_draws`] takes any draw for `config`: only
/// `RandomRate=`, `IsMeteor=` and `Bouncer=` draw. Without them it returns
/// [`AnimConstructorDraws::default`] and leaves the stream as it was.
pub(super) fn constructor_draws(config: &AnimTypeRuntimeConfig) -> bool {
    config.random_rate_logic_frames.is_some() || config.is_meteor || config.bouncer
}

pub(super) fn anim_random_rate(
    config: &AnimTypeRuntimeConfig,
    rng: &mut crate::sim::rng::SimRng,
) -> Option<u16> {
    config
        .random_rate_logic_frames
        .map(|(low, high)| rng.next_range_u32_inclusive(u32::from(low), u32::from(high)) as u16)
}
