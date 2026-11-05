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
//! RESIDUAL: only Technos make glows here. Native `ObjectClass::Unlimbo`
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

use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose;
use serde::{Deserialize, Serialize};

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
}

impl AlphaShapes {
    /// `AlphaShapeClass::PointerExpired @ 0x00420E70`.
    pub(super) fn pointer_expired(&mut self, expired_id: u64) {
        for shape in &mut self.shapes {
            if shape.object == expired_id {
                shape.disabled = true;
            }
        }
    }

    /// `AlphaShapeClass::PurgeDisabled @ 0x00420E90`.
    pub(super) fn purge_disabled(&mut self) {
        self.shapes.retain(|shape| !shape.disabled);
    }
}

impl Simulation {
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
        let Some(image) = rules.alpha_image(&object.alpha_image) else {
            return;
        };
        let coord = ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        let [width, height] = image.canvas();
        self.alpha_shapes.shapes.push(AlphaShape {
            object: stable_id,
            origin: [x as i32 - width / 2, y as i32 - height / 2],
            image: object.alpha_image.clone(),
            disabled: false,
        });
    }

    /// Each shape's origin and AlphaImage name, in vector order.
    /// `DrawAll_NoMask @ 0x00421350` draws disabled shapes too, until the
    /// purge.
    pub fn alpha_shapes(&self) -> impl Iterator<Item = ([i32; 2], &str)> + '_ {
        self.alpha_shapes
            .shapes
            .iter()
            .map(|shape| (shape.origin, shape.image.as_str()))
    }
}

#[cfg(test)]
#[path = "alpha_shape_tests.rs"]
mod tests;
