//! Render-composite LineTrail history; never simulation or snapshot state.
//!
//! Original ObjectUnlimbo5F5155 ->556A20 allocates a plain ring (no Abstract
//! identity/RNG), 556B30 detaches, and Tactical6D4673 ->556D40 updates then
//! draws in reverse registry order. ObjectLoad5F5EED discards the backlink.
//! Executable evidence: tools/projectile_oracle/line_trail.{py,json,md}.

use std::collections::HashMap;

use crate::render::line_trail::LineTrailSegment;
use crate::sim::projectile::ProjectileCoord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    coord: ProjectileCoord,
    strength: i32,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            coord: ProjectileCoord::new(0, 0, 0),
            strength: 0,
        }
    }
}

struct Trail {
    owner: Option<u64>,
    color: [u8; 3],
    decrement: i32,
    head: usize,
    samples: [Sample; 32],
    retired: bool,
}

/// Presentation owns the allocation order and all history. Stable runtime
/// handles identify attached projectiles; native signed Abstract IDs do not.
#[derive(Default)]
pub(crate) struct LineTrails {
    trails: Vec<Trail>,
    attached: HashMap<u64, usize>,
    segments: Vec<LineTrailSegment>,
}

impl LineTrails {
    /// Called at admitted launch, not first visibility. RGB and decrement are
    /// copied once (5F5190..5207); later type/options changes do not rewrite them.
    pub(crate) fn attach(&mut self, owner: u64, color: [u8; 3], decrement: i32, detail: i32) {
        assert!(
            !self.attached.contains_key(&owner),
            "one trail per admitted lifetime"
        );
        self.attached.insert(owner, self.trails.len());
        self.trails.push(Trail {
            owner: Some(owner),
            color,
            decrement: if detail == 0 {
                decrement.wrapping_mul(2)
            } else {
                decrement
            },
            head: 0,
            samples: [Sample::default(); 32],
            retired: false,
        });
    }

    /// Original556B30 keeps the ring alive until a later composite drains it.
    pub(crate) fn detach(&mut self, owner: u64) {
        if let Some(index) = self.attached.remove(&owner) {
            self.trails[index].owner = None;
        }
    }

    /// OriginalObjectLoad5F5EED clears+A8; history is intentionally not restored.
    pub(crate) fn clear_on_load(&mut self) {
        self.trails.clear();
        self.attached.clear();
        self.segments.clear();
    }

    /// Exactly one call per tactical composite, even when no sim frame passed.
    /// Owner absence also detaches, covering a presentation handoff after removal.
    /// Registry compaction is stable and linear; reverse visit order is retained.
    pub(crate) fn composite(
        &mut self,
        mut coordinate: impl FnMut(u64) -> Option<ProjectileCoord>,
        mut visible: impl FnMut(Option<u64>, ProjectileCoord) -> bool,
    ) -> &[LineTrailSegment] {
        self.segments.clear();
        let mut compact = false;
        for trail in self.trails.iter_mut().rev() {
            if let Some(owner) = trail.owner {
                if let Some(coord) = coordinate(owner) {
                    if coord != trail.samples[trail.head].coord {
                        trail.head = trail.head.wrapping_sub(1) & 31;
                        trail.samples[trail.head] = Sample {
                            coord,
                            strength: 255,
                        };
                    }
                } else {
                    self.attached.remove(&owner);
                    trail.owner = None;
                }
            }
            // Original556B70 subtracts every slot, including the just-added one.
            for sample in &mut trail.samples {
                sample.strength = sample.strength.wrapping_sub(trail.decrement).max(0);
            }
            if trail.owner.is_none() && trail.samples[trail.head].strength == 0 {
                trail.retired = true;
                compact = true;
                continue;
            }
            for offset in 0..31 {
                let newer = trail.samples[(trail.head + offset) & 31];
                let older = trail.samples[(trail.head + offset + 1) & 31];
                if newer.coord == ProjectileCoord::new(0, 0, 0)
                    || older.coord == ProjectileCoord::new(0, 0, 0)
                    || newer.strength == 0
                {
                    break;
                }
                // LineTrailMgr::UpdateAndDrawAll556D40 updates556B70 before
                // drawing556C00. Fog conceals output, never owner sampling,
                // ring advancement or the normal556B30 detach lifecycle.
                if visible(trail.owner, newer.coord) {
                    self.segments.push(LineTrailSegment {
                        from: newer.coord,
                        to: older.coord,
                        color: trail.color,
                        strength: newer.strength,
                    });
                }
            }
        }
        if compact {
            self.trails.retain(|trail| !trail.retired);
            self.attached.clear();
            for (index, trail) in self.trails.iter().enumerate() {
                if let Some(owner) = trail.owner {
                    self.attached.insert(owner, index);
                }
            }
        }
        &self.segments
    }
}

#[cfg(test)]
#[path = "line_trail_tests.rs"]
mod tests;
