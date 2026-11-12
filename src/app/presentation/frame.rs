//! The presentation frame (docs/sim-thread.md): what drawing reads from the
//! world, captured once per world state instead of read from it while
//! drawing.
//!
//! A frame holds a record for every Display member (`DisplayLayers`), keyed
//! by its stable id. It is built from the world the draw would otherwise
//! read: the simulation host captures it when a draw or a lent tick finds
//! the world changed since the last capture ([`super::super::match_runtime::
//! sim_host::SimHost::ensure_frame`]), so a frame always equals the world it
//! stands for. Records hold only results that depend on the world and the
//! committed binary frame, never on the camera, the cursor or render caches.

use std::collections::HashMap;

use crate::app::presentation::render_pose::RenderPose;
use crate::sim::game_entity::GameEntity;
use crate::sim::runtime::SimRuntime;

/// One Display member's draw record.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ObjectDraw {
    pub(crate) id: u64,
    /// [`RenderPose::read`] at the committed binary frame.
    pub(crate) pose: RenderPose,
}

impl ObjectDraw {
    fn capture(entity: &GameEntity, binary_frame: u32) -> Self {
        Self {
            id: entity.stable_id(),
            pose: RenderPose::read(entity, binary_frame),
        }
    }
}

/// Every Display member's record, for one world state.
#[derive(Debug, Default)]
pub(crate) struct PresentationFrame {
    binary_frame: u32,
    objects: Vec<ObjectDraw>,
    /// Stable id -> index into `objects`. Only looked up, never iterated.
    slots: HashMap<u64, u32>,
}

impl PresentationFrame {
    /// The records of `runtime`'s Display members, in Display order.
    pub(crate) fn capture(runtime: &SimRuntime) -> Self {
        let view = runtime.view();
        let binary_frame = view.session().binary_frame();
        let entities = view.entities();
        let objects: Vec<ObjectDraw> = view
            .display_layers()
            .members_in_order()
            .filter_map(|(_, id)| entities.get(id))
            .map(|entity| ObjectDraw::capture(entity, binary_frame))
            .collect();
        let slots = objects
            .iter()
            .enumerate()
            .map(|(slot, object)| (object.id, slot as u32))
            .collect();
        Self {
            binary_frame,
            objects,
            slots,
        }
    }

    pub(crate) fn object(&self, id: u64) -> Option<&ObjectDraw> {
        self.slots
            .get(&id)
            .and_then(|slot| self.objects.get(*slot as usize))
    }
}

/// `entity`'s pose at `binary_frame`: its record's when `frame` holds one
/// for that binary frame (a carried cargo is no Display member, and tools
/// may draw another frame), else read from the entity.
pub(crate) fn pose(
    frame: Option<&PresentationFrame>,
    entity: &GameEntity,
    binary_frame: u32,
) -> RenderPose {
    let recorded = frame
        .filter(|frame| frame.binary_frame == binary_frame)
        .and_then(|frame| frame.object(entity.stable_id()))
        .map(|object| object.pose);
    let pose = recorded.unwrap_or_else(|| RenderPose::read(entity, binary_frame));
    debug_assert!(
        recorded.is_none_or(|pose| pose == RenderPose::read(entity, binary_frame)),
        "presentation frame pose of {} is stale",
        entity.stable_id()
    );
    pose
}
