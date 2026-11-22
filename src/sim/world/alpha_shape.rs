//! AlphaShapeClass: the light glow an object with a loaded `AlphaImage=`
//! casts into the tactical A plane (`render::shroud_buffer`).
//!
//! `ObjectClass::Unlimbo @ 0x005F4EC0` constructs one (`0x005F5045..0x005F5100`)
//! after its Logic registration, for a live object whose type's AlphaImage
//! pointer (+0xAC) loaded: GetCoords (vt+0x48) in absolute tactical pixels
//! (`CoordsToClient2` plus ViewOrigin), minus half the SHP canvas. It never
//! moves. The ctor `0x00420960` appends it to the vector at `0x0088A0F0` and
//! to the PointerExpired listeners; `PointerExpired @ 0x00420E70` disables it
//! for its object whatever the control, so Limbo (`0x005F4D61`), a cloak's
//! Detach_All and UnInit all do. `PurgeDisabled @ 0x00420E90` deletes the
//! disabled ones, keeping order, from the LogicClass tick (`0x0055B650`).
//!
//! Its base ctor `0x00410170` leaves UniqueID -1, and nothing here draws RNG,
//! writes a timer or detaches anything. The vector is saved
//! (`Save_Game_Content_To_Stream`, `0x0067E22E`), but no sync CRC walks it,
//! so the world hash leaves it out.
//!
//! RESIDUAL: only Technos, particles and projectiles make glows here. Native `ObjectClass::Unlimbo`
//! makes one for any object whose type's AlphaImage loaded, so another object
//! kind that reaches it (this port traces none) glows in gamemd and not in
//! VERA. Retail loads no AlphaImage; maps and mods can. Nothing in gameplay
//! reads a glow.
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
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

/// The shape vector, in construction order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AlphaShapes {
    shapes: Vec<AlphaShape>,
    /// Object to its most recently constructed shape. Rebuilt after a purge
    /// moves slots or a save is loaded. Draw order comes from `shapes`.
    #[serde(skip)]
    slots: Option<HashMap<u64, usize>>,
}

impl AlphaShapes {
    fn slot_for(&mut self, object: u64) -> Option<usize> {
        if self.shapes.is_empty() {
            return None;
        }
        let slots = self.slots.get_or_insert_with(|| {
            self.shapes
                .iter()
                .enumerate()
                .map(|(i, shape)| (shape.object, i))
                .collect()
        });
        slots.get(&object).copied()
    }

    pub(super) fn contains(&mut self, object: u64) -> bool {
        self.slot_for(object).is_some()
    }

    /// `ObjectClass::Unlimbo @ 0x005F5045..0x005F5100`: a bound AlphaImage
    /// creates a glow centred on the object's virtual GetCoords.
    pub(super) fn construct(
        &mut self,
        object: u64,
        name: &str,
        rules: &RuleSet,
        get_coords: impl FnOnce() -> [i32; 3],
    ) {
        let Some(image) = rules.alpha_image(name) else {
            return;
        };
        let [x, y, z] = get_coords();
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
        let [width, height] = image.canvas();
        if let Some(slots) = &mut self.slots {
            slots.insert(object, self.shapes.len());
        }
        self.shapes.push(AlphaShape {
            object,
            origin: [x as i32 - width / 2, y as i32 - height / 2],
            image: name.to_owned(),
            disabled: false,
        });
    }

    /// Ares `ObjectClass_Update_AlphaLight` hooks the entry to Object AI
    /// (0x005F3E70), then resumes its stock body. UpdateAlphaShape (War of
    /// Dawn Ares.dll 0x10009F80, 0x1000A187) places the shape on odd frames
    /// using GetCoords at the time of the visit and rounds odd canvases up.
    pub(in crate::sim) fn update_at(
        &mut self,
        object: u64,
        rules: &RuleSet,
        get_coords: impl FnOnce() -> [i32; 3],
    ) {
        let Some(slot) = self.slot_for(object) else {
            return;
        };
        let last = self.shapes.len() - 1;
        let shape = &mut self.shapes[slot];
        if shape.disabled {
            return;
        }
        let Some(image) = rules.alpha_image(&shape.image) else {
            return;
        };
        let [x, y, z] = get_coords();
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
        let [width, height] = image.canvas();
        shape.origin = [x as i32 - (width + 1) / 2, y as i32 - (height + 1) / 2];
        // Ares' ctor (0x10048100) replaces the old shape; it now draws
        // after intervening glows. Move the name and reclaim the old slot
        // at PurgeDisabled so overlapping lights keep that order.
        if slot != last {
            shape.disabled = true;
            let replacement = AlphaShape {
                object,
                origin: shape.origin,
                image: std::mem::take(&mut shape.image),
                disabled: false,
            };
            if let Some(slots) = &mut self.slots {
                slots.insert(object, last + 1);
            }
            self.shapes.push(replacement);
        }
    }

    /// `AlphaShapeClass::PointerExpired @ 0x00420E70`.
    pub(super) fn pointer_expired(&mut self, expired_id: u64) {
        // Limbo expired any previous placement before this object's next
        // Unlimbo. Replaced Ares slots are disabled at replacement as well.
        if let Some(slot) = self.slot_for(expired_id) {
            self.shapes[slot].disabled = true;
        }
    }

    /// `AlphaShapeClass::PurgeDisabled @ 0x00420E90`.
    pub(super) fn purge_disabled(&mut self) {
        let old_len = self.shapes.len();
        self.shapes.retain(|shape| !shape.disabled);
        if self.shapes.len() != old_len
            && let Some(slots) = &mut self.slots
        {
            slots.clear();
            slots.extend(
                self.shapes
                    .iter()
                    .enumerate()
                    .map(|(i, shape)| (shape.object, i)),
            );
        }
    }
}

impl Simulation {
    /// Older saves kept Bullet FireCoord (+0x134) but no projectile glows.
    /// Recover stock placement only when the map still knows the launch
    /// ground. A replaced cell or the mutable dummy has no historical floor.
    /// These missing shapes also have no saved order relative to other lights.
    /// Ares instead obtains the exact location on its next odd Bullet AI.
    pub(crate) fn restore_missing_projectile_alpha_shapes(&mut self, rules: &RuleSet) {
        use crate::map::{cell_index::NativeCellIdentity, resolved_terrain::NativeCellQuery};
        use crate::sim::projectile::{ProjectileCoord, bullet_unlimbo_coord_at_floor};

        if rules.ares() || self.projectiles.is_empty() {
            return;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let cells = NativeCellQuery::isolated(terrain);
        for (&id, bullet) in self.projectiles.iter() {
            let origin = bullet.launch_origin();
            if !bullet.in_logic_vector() || origin == ProjectileCoord::new(0, 0, 0) {
                continue;
            }
            let Some(kind) = bullet
                .payload()
                .projectile_type()
                .and_then(|id| self.interner.try_resolve(id))
                .and_then(|name| rules.projectile(name))
            else {
                continue;
            };
            if kind.alpha_image.is_empty() || self.alpha_shapes.contains(id) {
                continue;
            }
            let cell = cells.lookup_world(origin.x, origin.y);
            let NativeCellIdentity::Real(_) = cell else {
                continue;
            };
            let (x, y) = cells.coord(cell);
            if self
                .dynamic_terrain_cells
                .get(&(x as u16, y as u16))
                .is_some()
            {
                continue;
            }
            let (level, slope) = cells.ground_fields(cell);
            let Ok(floor) =
                crate::util::lepton::ground_height_leptons(level, slope, origin.x, origin.y)
            else {
                continue;
            };
            self.alpha_shapes
                .construct(id, &kind.alpha_image, rules, || {
                    let at = bullet_unlimbo_coord_at_floor(origin, floor);
                    [at.x, at.y, at.z]
                });
        }
    }

    /// The AlphaShapeClass block of `ObjectClass::Unlimbo`
    /// (`0x005F5045..0x005F5100`). The ctor's `DirtyScreenRect` has no port:
    /// the A plane redraws whenever the shape list changes.
    pub(super) fn construct_alpha_shape(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let Some(object) = self.entity_object_type(entity, rules) else {
            return;
        };
        self.alpha_shapes
            .construct(stable_id, &object.alpha_image, rules, || {
                let coord = ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
                [coord.x, coord.y, coord.z]
            });
    }

    /// Each shape's origin and AlphaImage name, in vector order.
    /// `DrawAll_NoMask @ 0x00421350` draws disabled shapes too, until the
    /// purge.
    pub fn alpha_shapes(&self) -> impl Iterator<Item = ([i32; 2], &str)> + '_ {
        self.alpha_shapes
            .shapes
            .iter()
            .filter(|shape| !shape.image.is_empty())
            .map(|shape| (shape.origin, shape.image.as_str()))
    }

    /// Called on entry to the ObjectClass::AI part of Techno's mission visit,
    /// before the Foot locomotor advances. Particle AI does not enter this hook.
    ///
    /// RESIDUAL: Ares also hides and restores shapes as cloak, power, depth
    /// and terrain disguises change. Here a shape removed on cloak stays
    /// gone, and deactivation alone leaves its glow visible.
    pub(super) fn visit_techno_alpha_shape(&mut self, id: u64, rules: &RuleSet) {
        if rules.ares()
            && self.session.binary_frame() & 1 != 0
            && let Some(entity) = self.substrate.entities.get(id)
        {
            self.alpha_shapes.update_at(id, rules, || {
                let coord = ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
                [coord.x, coord.y, coord.z]
            });
        }
    }
}

impl Simulation {
    /// AlphaShapeClass::DrawAll_WithMask/NoMask. Ares hooks 420F75/421371
    /// skip disabled shapes and call GetVisualCharacter(1, owner) for Technos
    /// (Ares.dll 10045090), also checking IsDisguised (+1D8). The frame hooks
    /// use the live primary facing for these objects and frame zero otherwise.
    pub fn alpha_shape_draw_frames<'a>(
        &'a self,
        rules: &'a RuleSet,
    ) -> impl Iterator<
        Item = (
            [i32; 2],
            &'a std::sync::Arc<crate::rules::alpha_image_assets::AlphaImage>,
            usize,
        ),
    > + 'a {
        let ares = rules.ares();
        let now = self.session.binary_frame();
        self.alpha_shapes.shapes.iter().filter_map(move |shape| {
            if ares && shape.disabled {
                return None;
            }
            let image = rules.alpha_image(&shape.image)?;
            let mut frame = 0;
            if ares && let Some(entity) = self.substrate.entities.get(shape.object) {
                let object = self.entity_object_type(entity, rules)?;
                let invisible = object.invisible
                    || entity.category() == crate::map::entities::EntityCategory::Structure
                        && object.invisible_in_game;
                let sensed = self.fog.senses_object_for_house(entity.owner(), entity);
                if entity
                    .seldom
                    .disguise
                    .as_ref()
                    .is_some_and(|d| d.is_disguised())
                    || entity.visual_character(
                        object.cloaking_stages(rules.general.cloaking_stages),
                        invisible,
                        entity.seldom.discovery.owned_by_current_house(),
                        crate::sim::cloak_disguise::VisualCharacterQuery::sensor(
                            true, sensed, false,
                        ),
                    ) != 0
                {
                    return None;
                }
                frame = image.facing_frame(entity.body_facing_current(now));
            }
            Some((shape.origin, image, frame))
        })
    }
}

#[cfg(all(test, tests_sim_world_a_b))]
#[path = "alpha_shape_tests.rs"]
mod tests;
