//! IonBlastClass: the screen ripple a Psychic Dominator strike leaves.
//!
//! Native owner: the vector `0x00AA0118` (items `0x00AA011C`, count
//! `0x00AA0128`) of 0x14-byte blasts: a coordinate (+0..+8), a frame (+0xC)
//! and a flag (+0x10) that skips the forces. Here the list is
//! `Simulation::ion_blasts`; only this module writes it.
//!
//! - The constructor (`0x0053CB10`) takes the coordinate, zeroes the frame and
//!   the flag and appends the blast. `PsyDom::MindControlArea` constructs one
//!   at the target cell's GetCoords and sets its flag
//!   (`0x0053B087..0x0053B0D4`), before the second anim.
//! - `IonBlastClass::UpdateAll @ 0x0053D310`, from LogicClass::PerTickUpdate
//!   after the object loop (`0x0055B64B`, before the crate timers), visits the
//!   blasts last to first (`0x0053CBE0`): one whose frame is 79 or more is
//!   removed and deleted, and a flagged one advances its frame. A blast made in
//!   the frame's superweapon pass is at frame 1 by the draw, draws frames 1 to
//!   79 and goes at its 80th update.
//! - `IonBlastClass::DrawAll @ 0x0053D850` reads the list for the tactical
//!   draw ([`Simulation::ion_blasts`], `render::terrain_ion_blast`).
//!
//! Scenario draws, timer writes and detach calls: none. Not saved (no save
//! code reads the vector) and not hashed: a flagged blast changes no other
//! state.
//!
//! Evidence: `tools/superweapon_oracle.py` section `ion_blast_update` runs the
//! original UpdateAll; `tests::update_all_matches_native` replays it. The
//! constructor and MindControlArea's call are read from the instructions.
//!
//! RESIDUALS:
//! - The unflagged update (`0x0053CC3A..0x0053D2FC`: at frame 0 the splash or
//!   `IonBlast=` anim, `IonBeam=`, `IonCannonDamage=` area damage and its
//!   flash; every frame the ripple rocks the voxel objects within three cells)
//!   is not ported. Only map trigger actions make an unflagged blast
//!   (`TActionClass::Execute`'s call at `0x006DD9AB`, `TActionClass::
//!   IonBlastAtWP @ 0x006E3380`), and VERA ports neither.
//! - Native never clears the vector at a scenario start or a saved game's
//!   load, so a blast alive then draws on in the next game's first frames.
//!   VERA's blasts belong to the match and are not saved.

use super::Simulation;

/// UpdateAll removes a blast whose frame is at least this (`CMP EAX,0x4F` at
/// `0x0053CBF5`).
const LAST_FRAME: i32 = 79;

/// One blast: its coordinate in leptons and its frame. Every blast VERA makes
/// is flagged (module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IonBlast {
    coord: [i32; 3],
    frame: i32,
}

impl Simulation {
    /// MindControlArea's blast at `coord` (`0x0053B087..0x0053B0D4`): the
    /// constructor appends it at frame 0, and the flag spares it the forces.
    pub(crate) fn add_dominator_ion_blast(&mut self, coord: [i32; 3]) {
        self.ion_blasts.push(IonBlast { coord, frame: 0 });
    }

    /// `IonBlastClass::UpdateAll @ 0x0053D310` for flagged blasts. Native
    /// walks last to first and shifts the later items down on a removal, so
    /// the survivors keep their order.
    pub(super) fn update_ion_blasts(&mut self) {
        self.ion_blasts.retain_mut(|blast| {
            if blast.frame >= LAST_FRAME {
                return false;
            }
            blast.frame += 1;
            true
        });
    }

    /// Each blast's coordinate and frame, in vector order; DrawAll visits them
    /// last to first.
    pub fn ion_blasts(&self) -> impl DoubleEndedIterator<Item = ([i32; 3], i32)> + '_ {
        self.ion_blasts
            .iter()
            .map(|blast| (blast.coord, blast.frame))
    }
}

#[cfg(test)]
#[path = "ion_blast_tests.rs"]
mod tests;
