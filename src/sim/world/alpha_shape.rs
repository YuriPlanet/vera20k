//! AlphaShapeClass: the light glow an object with a loaded `AlphaImage=`
//! casts into the tactical A plane (`render::shroud_buffer`).
//!
//! `ObjectClass::Unlimbo @ 0x005F4EC0` constructs one (`0x005F5045..0x005F5100`)
//! after its Logic registration, for a live object whose type's AlphaImage
//! pointer (+0xAC) loaded, whatever its class: Technos (`0x006F6CB1`), Terrain
//! (`0x0071D012`), a fired Bullet (`BulletClass::Fire`, `0x00468684`), and the
//! Anim (`0x004222E0`, `0x00422433`, `0x004224E0`), Particle (`0x0062BB0E`),
//! ParticleSystem (`0x0062DE68`) and VoxelAnim (`0x007494E9`, `0x00749794`)
//! constructors. Overlays and Smudges never get one: their Mark UnInits them
//! first. The shape sits at GetCoords (vt+0x48) in absolute tactical pixels
//! (`CoordsToClient2` plus ViewOrigin), minus half the SHP canvas, and in
//! stock never moves. The ctor `0x00420960` appends it to the vector at
//! `0x0088A0F0` and to the PointerExpired listeners (`0x00B0F720`);
//! `PointerExpired @ 0x00420E70` disables it for its object whatever the
//! control, so Limbo (`0x005F4D61`), a cloak's Detach_All and UnInit all do.
//! An Anim announces its expiry only to the Anim listeners (`0x00725A16`), so
//! in stock an Anim's glow is never disabled and stays where the Anim began.
//! `PurgeDisabled @ 0x00420E90` deletes the disabled ones, keeping order,
//! from the LogicClass tick (`0x0055B650`).
//!
//! Ares 3.0 Moving Alpha Lights (`Ext/Techno/Hooks.Alpha.cpp`) keeps one shape
//! per object in a map (`Ares.dll 100C1924`):
//! - `AlphaShapeClass_CTOR` (hook `0x00420960`, `100474B0`) destroys an
//!   object's earlier shape before the new one is appended;
//!   `AlphaShapeClass_SDDTOR` (`0x00421730`, `10047610`) drops the map entry.
//! - `AlphaShapeClass_CTOR_Anims` (`0x00420A71`, `100475A0`) also puts an
//!   Anim's shape on the Anim listeners (`0x00B0F5B8`), so the Anim's expiry
//!   disables it; `AlphaShapeClass_SDDTOR_Anims` (`0x00421798`, `10047670`)
//!   takes it off.
//! - `ObjectClass_DTOR` (`0x005F3D65`, `100476E0`) deletes an object's shape
//!   when the object is destroyed ([`Simulation::delete_alpha_shape`]).
//! - `ObjectClass_Update_AlphaLight` (`0x005F3E70`, `10047730`) and
//!   `AnimClass_Update_AlphaLight` (`0x00423B0B`, `100476C0`) run
//!   `UpdateAlphaShape` at the head of each ObjectClass::AI
//!   ([`Simulation::update_alpha_shape`]), so a glow follows its object.
//!   Particles, ParticleSystems and VoxelAnims never reach ObjectClass::AI and
//!   keep their Unlimbo spot.
//! - The draw hooks skip disabled shapes and a Techno that is cloaking or
//!   disguised, and draw a Techno's facing frame
//!   ([`Simulation::alpha_shape_draws`]).
//! - `TeleportLocomotionClass_ILocomotion_Process` (`0x0071944E`, `10047810`)
//!   only dirties the screen rect around a warped object's glow
//!   (`0x006D2790`) for gamemd's partial ABuffer redraw; VERA redraws the
//!   whole plane whenever the glow list changes, so it has nothing to port.
//!
//! Its base ctor `0x00410170` leaves UniqueID -1, and nothing here draws RNG,
//! writes a timer or detaches anything; Ares' update only reads the rotation
//! timer. The vector is saved (`Save_Game_Content_To_Stream`, `0x0067E22E`),
//! but no sync CRC walks it, so the world hash leaves it out.
//!
//! RESIDUAL: the A plane redraws whenever the glow list changes, so a glow
//! appears and goes at once. gamemd redraws only dirtied areas of its ABuffer,
//! and Limbo dirties the AlphaImage-sized rect at the object's current
//! GetCoords (`0x005F4E0B..0x005F4E8F`), not where its glow was drawn. So the
//! glow of a cloaked object, or of one that moved after Unlimbo and then died
//! or left the map, stays until something redraws that area.
//! A dirtied cell's redraw (`Tactical_layer_shroud_edges`, 0x006D3723)
//! re-applies the glows inside its 60x30 diamond through
//! `AlphaShapeClass::DrawAll_WithMask @ 0x00420F40`. VERA's whole-plane pass
//! (`DrawAll_NoMask`) applies the same scaling once per pixel.

use super::Simulation;
use super::substrate::ObjectKind;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AlphaShape {
    /// pObject (+0x24).
    object: u64,
    /// nX/nY (+0x28/+0x2C), in absolute world pixels.
    origin: [i32; 2],
    /// The AlphaImage the ctor takes from the object's type (+0x38).
    image: String,
    /// fDisabled (+0x3C).
    disabled: bool,
    /// On the Anim expiry listeners (`0x00B0F5B8`): an Anim's shape under
    /// Ares (`AlphaShapeClass_CTOR_Anims`).
    #[serde(default)]
    anim_listener: bool,
}

/// The shape vector, in construction order.
///
/// A deque, so that Ares' per-frame replacement stays linear: each odd
/// frame's updates ([`Simulation::update_alpha_shape`]) find their shapes at
/// the front, in the order the previous odd frame's updates appended them,
/// where Ares' vector erase shifts every later shape. The order is the same.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AlphaShapes {
    shapes: VecDeque<AlphaShape>,
}

impl AlphaShapes {
    /// The ctor `0x00420960`: append a shape of `image` for `object`. With
    /// Ares (`AlphaShapeClass_CTOR`), the object's earlier shape is
    /// destroyed first, so an object has at most one and a new one goes to
    /// the end.
    fn construct(
        &mut self,
        object: u64,
        origin: [i32; 2],
        image: &str,
        anim_listener: bool,
        ares: bool,
    ) {
        let earlier = if ares { self.take(object) } else { None };
        // The name string of an earlier shape of the same image is reused.
        let image = earlier
            .map(|shape| shape.image)
            .filter(|earlier| earlier == image)
            .unwrap_or_else(|| image.to_owned());
        self.shapes.push_back(AlphaShape {
            object,
            origin,
            image,
            disabled: false,
            anim_listener,
        });
    }

    /// Destroy `object`'s shape.
    fn delete(&mut self, object: u64) {
        self.take(object);
    }

    /// Remove `object`'s shape: only Ares does, and its map holds at most
    /// one per object.
    ///
    /// A linear scan, as Ares' vector erase is. Many withheld glowing
    /// Technos, or many updates behind many glows that never move
    /// (Particles, ParticleSystems, VoxelAnims), cost their product per
    /// frame. Retail data has one glow type (`TSTLAMP`); an index from
    /// object to shape would remove the cost for glow-heavy mods.
    fn take(&mut self, object: u64) -> Option<AlphaShape> {
        let index = self
            .shapes
            .iter()
            .position(|shape| shape.object == object)?;
        self.shapes.remove(index)
    }

    /// `AlphaShapeClass::PointerExpired @ 0x00420E70`.
    pub(super) fn pointer_expired(&mut self, expired_id: u64) {
        for shape in &mut self.shapes {
            if shape.object == expired_id {
                shape.disabled = true;
            }
        }
    }

    /// The same PointerExpired reached through the Anim listeners
    /// (`0x00725A16..0x00725A4C`), which only an Ares Anim shape joins.
    pub(super) fn anim_pointer_expired(&mut self, expired_id: u64) {
        for shape in &mut self.shapes {
            if shape.anim_listener && shape.object == expired_id {
                shape.disabled = true;
            }
        }
    }

    /// See [`Simulation::delete_anim_alpha_shape`].
    pub(super) fn anim_destroyed(&mut self, anim: u64) {
        self.shapes
            .retain(|shape| !(shape.anim_listener && shape.object == anim));
    }

    /// `AlphaShapeClass::PurgeDisabled @ 0x00420E90`.
    pub(super) fn purge_disabled(&mut self) {
        self.shapes.retain(|shape| !shape.disabled);
    }
}

/// What a glow reads from its object: GetCoords (vt+0x48), in leptons, and
/// the name of its type's (`GetType`, vt+0x88) AlphaImage.
struct AlphaSource<'r> {
    coord: [i32; 3],
    image: &'r str,
}

/// One glow as the A plane draws it: its origin in absolute world pixels,
/// its AlphaImage name and frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphaShapeDraw<'a> {
    pub origin: [i32; 2],
    pub image: &'a str,
    pub frame: u16,
}

impl Simulation {
    #[cfg(any(test, feature = "test-support"))]
    pub fn alpha_shape_slots_for_test(&self) -> Vec<(u64, [i32; 2], &str, bool, bool)> {
        self.alpha_shapes
            .shapes
            .iter()
            .map(|shape| {
                (
                    shape.object,
                    shape.origin,
                    shape.image.as_str(),
                    shape.disabled,
                    shape.anim_listener,
                )
            })
            .collect()
    }

    /// GetCoords and the AlphaImage name of `object`, of `kind`, for a type
    /// that names one. The name is read first: UpdateAlphaShape (`10009F40`)
    /// returns before GetCoords for a type without an AlphaImage, and it runs
    /// on every AI visit under Ares.
    fn alpha_source<'r>(
        &self,
        object: u64,
        kind: ObjectKind,
        rules: &'r RuleSet,
    ) -> Option<AlphaSource<'r>> {
        let named = |image: &'r str| (!image.is_empty()).then_some(image);
        let (image, coord) = match kind {
            ObjectKind::Entity => {
                let entity = self.substrate.entities.get(object)?;
                let image = named(&self.entity_object_type(entity, rules)?.alpha_image)?;
                let coord = ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
                (image, [coord.x, coord.y, coord.z])
            }
            // `AnimClass::GetCoords @ 0x00422BE0`: Location plus its owner's.
            ObjectKind::Anim => {
                let anim = self.anim(object)?;
                let config = self
                    .interner
                    .anim_type_config_handle(anim.type_id(), rules)?;
                let image = named(&rules.art().anim_config(config).alpha_image)?;
                let coord = self.anim_absolute_coord(object)?;
                (image, [coord.x, coord.y, coord.z])
            }
            // Terrain, Bullets, ParticleSystems and VoxelAnims inherit
            // `ObjectClass::GetCoords @ 0x005F65A0`: Location (+0x9C).
            ObjectKind::Terrain => {
                let terrain = self.terrain_objects.get(object)?;
                let terrain_type = rules.terrain_object_type_case_insensitive(
                    self.interner.resolve(terrain.type_ref()),
                )?;
                let coord = terrain.world_coord();
                (
                    named(&terrain_type.alpha_image)?,
                    [coord.x, coord.y, coord.z],
                )
            }
            ObjectKind::Projectile => {
                let bullet = self.projectiles.get(object)?;
                let bullet_type = self
                    .interner
                    .projectile_type(bullet.payload().projectile_type()?, rules)?;
                let coord = bullet.position();
                (
                    named(&bullet_type.alpha_image)?,
                    [coord.x, coord.y, coord.z],
                )
            }
            ObjectKind::ParticleSystem => {
                let system = self.substrate.particle_systems.get(object)?;
                (
                    named(&rules.particle_system_type(system.type_id()).alpha_image)?,
                    system.coords().to_array(),
                )
            }
            ObjectKind::VoxelAnim => {
                let piece = self.substrate.voxel_anims.get(object)?;
                (
                    named(&rules.find_voxel_anim_type(piece.type_id())?.alpha_image)?,
                    piece.world_coord().to_array(),
                )
            }
            // A Particle is read where its constructor reveals it
            // ([`Self::unlimbo_particle_alpha_shape`]); Spotlights, Waves and
            // Smudges never construct one.
            ObjectKind::Particle
            | ObjectKind::Spotlight
            | ObjectKind::Wave
            | ObjectKind::Smudge => return None,
        };
        Some(AlphaSource { coord, image })
    }

    /// The AlphaShapeClass block of `ObjectClass::Unlimbo`
    /// (`0x005F5045..0x005F5100`) for `object`, of `kind`, once its Unlimbo
    /// succeeded. The ctor's `DirtyScreenRect` has no port: the A plane
    /// redraws whenever the shape list changes.
    pub(crate) fn unlimbo_alpha_shape(&mut self, object: u64, kind: ObjectKind, rules: &RuleSet) {
        if let Some(source) = self.alpha_source(object, kind, rules) {
            self.construct_unlimbo_alpha_shape(object, kind, source, rules);
        }
    }

    /// [`Self::unlimbo_alpha_shape`] for a Particle its constructor
    /// (`ParticleClass::Constructor`, `0x0062BB0E`) reveals before the
    /// particle reaches its system's vector.
    pub(crate) fn unlimbo_particle_alpha_shape(
        &mut self,
        particle: &crate::sim::particles::Particle,
        rules: &RuleSet,
    ) {
        let image = rules.particle_type(particle.type_id()).alpha_image.as_str();
        if image.is_empty() {
            return;
        }
        let source = AlphaSource {
            coord: particle.coords().to_array(),
            image,
        };
        self.construct_unlimbo_alpha_shape(
            particle.stable_id(),
            ObjectKind::Particle,
            source,
            rules,
        );
    }

    fn construct_unlimbo_alpha_shape(
        &mut self,
        object: u64,
        kind: ObjectKind,
        source: AlphaSource<'_>,
        rules: &RuleSet,
    ) {
        // Unlimbo refuses 0,0,0 before its glow block (`0x005F4ECD`), for
        // every kind.
        if source.coord == [0, 0, 0] {
            return;
        }
        let Some(image) = rules.alpha_image(source.image) else {
            return;
        };
        let [x, y] = Self::alpha_screen(source.coord);
        let [width, height] = image.canvas();
        self.alpha_shapes.construct(
            object,
            [x - width / 2, y - height / 2],
            source.image,
            rules.ares() && kind == ObjectKind::Anim,
            rules.ares(),
        );
    }

    /// `CoordsToClient2` plus ViewOrigin: GetCoords in absolute tactical
    /// pixels.
    fn alpha_screen([x, y, z]: [i32; 3]) -> [i32; 2] {
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
        [x as i32, y as i32]
    }

    /// Ares `ObjectClass_DTOR` (hook `0x005F3D65`, `Ares.dll 100476E0`):
    /// destroying an object deletes its glow at once, whether or not its
    /// UnInit disabled it. Stock leaves the shape to PurgeDisabled.
    pub(crate) fn delete_alpha_shape(&mut self, object: u64, rules: &RuleSet) {
        if rules.ares() {
            self.alpha_shapes.delete(object);
        }
    }

    /// [`Self::delete_alpha_shape`] for an Anim destroyed outside the common
    /// destructor: a scalar slot delete, or the local list's drain. Only Ares
    /// puts an Anim's shape on the Anim listeners, so that flag stands for
    /// the Ares switch here.
    pub(crate) fn delete_anim_alpha_shape(&mut self, anim: u64) {
        self.alpha_shapes.anim_destroyed(anim);
    }

    /// Ares 3.0 `AlphaShapeExt::UpdateAlphaShape` (`Ares.dll 10009F40`), at
    /// the head of `ObjectClass::AI @ 0x005F3E70` (hook
    /// `ObjectClass_Update_AlphaLight`, `10047730`) for Technos, Terrain and
    /// Bullets, and in `AnimClass::AI` (`AnimClass_Update_AlphaLight`,
    /// `0x00423B0B`, `100476C0`; a flaming guy's reaches ObjectClass::AI at
    /// `0x00423B1E` instead). For an object whose type's AlphaImage loaded:
    /// - while it is withheld ([`Self::alpha_shape_withheld`]) its shape is
    ///   deleted;
    /// - otherwise, on an odd frame (`[0x00A8ED84] & 1`), a new shape is made
    ///   at its current GetCoords less `(size + 1) / 2`, replacing the old
    ///   one at the end of the vector (`AlphaShapeClass_CTOR`). This also
    ///   gives a glow back once an object stops being withheld. An even frame
    ///   leaves the shape where it is.
    ///
    /// The dirty rects it marks (around the shape, and around a moving
    /// Foot's last cell) serve gamemd's partial redraw and have no port.
    pub(crate) fn update_alpha_shape(&mut self, object: u64, kind: ObjectKind, rules: &RuleSet) {
        let Some(source) = self.alpha_source(object, kind, rules) else {
            return;
        };
        let Some(image) = rules.alpha_image(source.image) else {
            return;
        };
        if self.alpha_shape_withheld(object, kind, rules) {
            self.alpha_shapes.delete(object);
            return;
        }
        if self.session.binary_frame() % 2 == 0 {
            return;
        }
        let [x, y] = Self::alpha_screen(source.coord);
        let [width, height] = image.canvas();
        self.alpha_shapes.construct(
            object,
            [x - (width + 1) / 2, y - (height + 1) / 2],
            source.image,
            kind == ObjectKind::Anim,
            true,
        );
    }

    /// UpdateAlphaShape's removal test (`10009F40` at `0x1000A09E..0x1000A145`):
    /// the object is in Limbo (`+0x81`), or it is a Techno that is
    /// deactivated (`+0x1C8`), cloaked (CloakState `+0x220` = 2), deeper than
    /// 10 leptons below the ground (GetHeight vt+0x1C8), disguised (vt+0xC4,
    /// the `+0x1D8` flag) as a TerrainType (GetDisguise(1) vt+0xCC, the
    /// `+0x518` type of a Unit or Infantry), or a Building outside
    /// Construction (vt+0x184 not `0x12`) that is not operational
    /// (`BuildingClass::Is_Operational_For_Output` vt+0x350, `0x004555D0`).
    ///
    /// Only a Techno's Limbo is read: the other kinds run their AI only from
    /// Logic, which Limbo leaves. RESIDUAL: a destroyed local Anim keeps its
    /// local-list visit until the frame's drain, so under Ares that visit can
    /// make its glow again; the drain deletes it before any draw.
    fn alpha_shape_withheld(&self, object: u64, kind: ObjectKind, rules: &RuleSet) -> bool {
        if kind != ObjectKind::Entity {
            return false;
        }
        let Some(entity) = self.substrate.entities.get(object) else {
            return false;
        };
        if entity.in_limbo()
            || entity.deactivated.is_offline()
            || entity
                .cloak
                .as_ref()
                .is_some_and(|cloak| cloak.state() == 2)
            || crate::sim::movement::air_movement::current_fly_height(
                entity,
                self.resolved_terrain.as_ref(),
            ) < -10
        {
            return true;
        }
        let disguised_as_terrain = matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        ) && entity
            .seldom
            .disguise
            .as_ref()
            .filter(|disguise| disguise.is_disguised())
            .and_then(|disguise| disguise.type_id())
            .is_some_and(|disguise| {
                rules
                    .terrain_object_type_case_insensitive(self.interner.resolve(disguise))
                    .is_some()
            });
        disguised_as_terrain
            || (entity.category() == EntityCategory::Structure
                && entity.mission.effective().raw() != 0x12
                && self.building_operational_state(object, rules) == Some(false))
    }

    /// The glows the A plane draws, in vector order.
    ///
    /// Stock `DrawAll_NoMask @ 0x00421350` draws every shape, disabled ones
    /// too until the purge, with frame 0. With Ares the
    /// `TacticalClass_UpdateAlphasInRectangle_ShouldDraw` hook (`0x00421371`,
    /// `Ares.dll 100477D0`; `AlphaLightClass_UpdateScreen_ShouldDraw`
    /// `0x00420F75`, `10047470`, for the masked redraw) skips a disabled shape
    /// and a Techno's whose VisualCharacter (vt+0x68) for (1, its owner) is
    /// not Normal or whose `+0x1D8` disguise flag is up (`100445B0`), and the
    /// Header and Body hooks (`0x0042146E`/`0x0042152C`, and
    /// `0x004210AC`/`0x004211AC`) draw a Techno's PrimaryFacing frame
    /// ([`AlphaImage::facing_frame`]).
    ///
    /// So under Ares an `InvisibleInGame=yes` building the player does not
    /// own draws no glow: BuildingType ReadINI (`0x00460E09`) sets its
    /// Invisible byte (`+0xC9A`), and `TechnoClass::VisualCharacter @
    /// 0x00703860` answers Hidden for an Invisible type unless `+0x41A`
    /// (owned by the player) is set. No retail type has both: the one retail
    /// AlphaImage, `[TSTLAMP]`'s `ALPHATST`, is on a visible lamp.
    ///
    /// [`AlphaImage::facing_frame`]: crate::rules::alpha_image_assets::AlphaImage::facing_frame
    pub fn alpha_shape_draws<'a>(
        &'a self,
        rules: &'a RuleSet,
    ) -> impl Iterator<Item = AlphaShapeDraw<'a>> + 'a {
        let ares = rules.ares();
        let frame = self.session.binary_frame();
        self.alpha_shapes.shapes.iter().filter_map(move |shape| {
            let mut draw = AlphaShapeDraw {
                origin: shape.origin,
                image: &shape.image,
                frame: 0,
            };
            if !ares {
                return Some(draw);
            }
            if shape.disabled {
                return None;
            }
            if let Some(techno) = self.substrate.entities.get(shape.object) {
                let object = self.entity_object_type(techno, rules);
                let cloaking_stages = object.map_or(rules.general.cloaking_stages, |object| {
                    object.cloaking_stages(rules.general.cloaking_stages)
                });
                let invisible = object.is_some_and(|object| {
                    object.invisible
                        || (techno.category() == EntityCategory::Structure
                            && object.invisible_in_game)
                });
                // A forced query's zero answer reads neither of the sensor
                // inputs.
                let character = techno.visual_character(
                    cloaking_stages,
                    invisible,
                    techno.seldom.discovery.owned_by_current_house(),
                    crate::sim::cloak_disguise::VisualCharacterQuery::sensor(true, false, false),
                );
                let disguised = techno
                    .seldom
                    .disguise
                    .as_ref()
                    .is_some_and(|disguise| disguise.is_disguised());
                if character != 0 || disguised {
                    return None;
                }
                // The frame picker (Ares.dll `0x100431A0`) reads PrimaryFacing
                // through Ares' own FacingClass::Current (`0x10009CC0`).
                draw.frame = rules.alpha_image(&shape.image).map_or(0, |image| {
                    image.facing_frame(techno.body_facing_current_ares(frame))
                });
            }
            Some(draw)
        })
    }
}

#[cfg(all(test, tests_sim_world_a_b))]
#[path = "alpha_shape_tests.rs"]
mod tests;
