//! Test-only callee answers and call records for the Drive/Ship fresh-arm
//! replay of `tools/spatial_oracle/track_fresh_response`, which substitutes
//! the same four callees: Unit `Can_Enter_Cell` (0x73F0A0) and Foot
//! `Find_Path` (0x4D3920) answer from supplied queues; Cell `Scatter_Objects`
//! (0x481670) and `Foot::Override_Mission` (0x4D8F40) are recorded and, like
//! the oracle's substitutions, their bodies do not run. Nothing is supplied,
//! recorded or skipped unless a test installs the queues. Walk's response
//! corpus opts into live effects: the same boundaries are recorded while the
//! real Scatter and Override bodies run. The Teleport resolver's replay
//! (`tools/spatial_oracle/cmin_dock`) supplies only Find_Nearby_Passable_Cell's
//! answer at `0x00719185`, which that harness stubs; Can_Enter_Cell, Find_Path
//! and every effect run for real.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::combat::TargetKind;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint};

/// A supplied `Find_Path` answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SuppliedPath {
    /// AL = 1 after writing these words to Foot+5E0.
    Found(Vec<u8>),
    /// AL = 1 with no writes, as the Walk decoder's direct code-2 controls.
    FoundUnchanged,
    /// AL = 0 with no writes.
    Failed,
    /// The original wrapper runs; only its AStar core (0x4CBBA0) is NULL.
    CoreNull,
}

/// One substituted call, with the caller's arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FreshCallRecord {
    CanEnter {
        cell: (i16, i16),
        direction: i32,
        height: i32,
        code: u8,
    },
    FindPath {
        cell: (i32, i32),
        urgency: u8,
    },
    Scatter {
        cell: (i16, i16),
        no_kidding: bool,
        deck: bool,
    },
    Override {
        target: TargetKind,
    },
    UncloakContacts {
        cell: (i16, i16),
    },
    /// A supplied Find_Nearby_Passable_Cell answer, with the seed and every
    /// argument the caller passed.
    NearbyCell {
        seed: (i32, i32),
        speed_type: SpeedType,
        zone: Option<u32>,
        movement_zone: MovementZone,
        bridge_aware_zone: bool,
        footprint: NearbyFootprint,
        allow_bridge_cells: bool,
        check_height: bool,
        check_occupancy: bool,
        target_cell: Option<(i32, i32)>,
        reject_any_overlay: bool,
    },
}

#[derive(Default)]
struct Seam {
    codes: VecDeque<u8>,
    paths: VecDeque<SuppliedPath>,
    /// Find_Nearby_Passable_Cell answers; `None` runs the real search.
    nearby: Option<VecDeque<Option<(u16, u16)>>>,
    records: Vec<FreshCallRecord>,
    core_null: bool,
    live_effects: bool,
    live_can_enter: bool,
    live_paths: bool,
}

thread_local! {
    static SEAM: RefCell<Option<Seam>> = const { RefCell::new(None) };
}

/// Install the supplied answers for one replayed row.
pub(crate) fn install(codes: Vec<u8>, paths: Vec<SuppliedPath>) {
    SEAM.with(|seam| {
        *seam.borrow_mut() = Some(Seam {
            codes: codes.into(),
            paths: paths.into(),
            nearby: None,
            records: Vec::new(),
            core_null: false,
            live_effects: false,
            live_can_enter: false,
            live_paths: false,
        })
    });
}

/// Walk's response corpus executes the real Scatter and Override bodies.
/// Keep recording their boundaries without substituting those effects.
pub(crate) fn install_with_live_effects(codes: Vec<u8>, paths: Vec<SuppliedPath>) {
    install(codes, paths);
    SEAM.with(|seam| seam.borrow_mut().as_mut().unwrap().live_effects = true);
}

/// A composed native comparison supplies only Find_Path's route, allowing
/// the real Unit admission owner to classify the marked world and occupants.
pub(crate) fn install_path_only(paths: Vec<SuppliedPath>) {
    install(Vec::new(), paths);
    SEAM.with(|seam| {
        let mut seam = seam.borrow_mut();
        let seam = seam.as_mut().unwrap();
        seam.live_can_enter = true;
        seam.live_effects = true;
    });
}

/// A composed native comparison that supplies only Find_Nearby_Passable_Cell's
/// answers; `None` is the null cell. Find_Path searches for real.
pub(crate) fn install_nearby_only(answers: Vec<Option<(u16, u16)>>) {
    install_path_only(Vec::new());
    SEAM.with(|seam| {
        let mut seam = seam.borrow_mut();
        let seam = seam.as_mut().unwrap();
        seam.nearby = Some(answers.into());
        seam.live_paths = true;
    });
}

/// Remove the seam, returning the records and any unused answers.
pub(crate) fn finish() -> (Vec<FreshCallRecord>, usize) {
    SEAM.with(|seam| {
        seam.borrow_mut().take().map_or((Vec::new(), 0), |seam| {
            (
                seam.records,
                seam.codes.len()
                    + seam.paths.len()
                    + seam.nearby.as_ref().map_or(0, VecDeque::len)
                    + usize::from(seam.core_null),
            )
        })
    })
}

/// A `CoreNull` answer arms this for the core search of the same call.
pub(crate) fn arm_core_null() {
    SEAM.with(|seam| {
        if let Some(seam) = seam.borrow_mut().as_mut() {
            seam.core_null = true;
        }
    });
}

/// True once per armed `CoreNull`: the core search answers NULL.
pub(crate) fn take_core_null() -> bool {
    SEAM.with(|seam| {
        seam.borrow_mut()
            .as_mut()
            .is_some_and(|seam| std::mem::take(&mut seam.core_null))
    })
}

pub(crate) fn supplied_can_enter(cell: (i16, i16), direction: i32, height: i32) -> Option<u8> {
    SEAM.with(|seam| {
        let mut seam = seam.borrow_mut();
        let seam = seam.as_mut()?;
        if seam.live_can_enter {
            return None;
        }
        let code = seam.codes.pop_front().expect("unsupplied Can_Enter_Cell");
        seam.records.push(FreshCallRecord::CanEnter {
            cell,
            direction,
            height,
            code,
        });
        Some(code)
    })
}

/// The supplied answer for one Find_Nearby_Passable_Cell call, recorded with
/// its arguments; `None` when no answers are installed.
pub(crate) fn supplied_nearby_cell(
    seed: (i32, i32),
    args: NearbyArgs,
) -> Option<Option<(u16, u16)>> {
    SEAM.with(|seam| {
        let mut seam = seam.borrow_mut();
        let seam = seam.as_mut()?;
        let answer = seam
            .nearby
            .as_mut()?
            .pop_front()
            .expect("unsupplied Find_Nearby_Passable_Cell");
        seam.records.push(FreshCallRecord::NearbyCell {
            seed,
            speed_type: args.passability.speed_type,
            zone: args.passability.required_zone_id,
            movement_zone: args.passability.movement_zone,
            bridge_aware_zone: args.passability.bridge_aware_zone,
            footprint: args.footprint,
            allow_bridge_cells: args.allow_bridge_cells,
            check_height: args.check_height,
            check_occupancy: args.check_occupancy,
            target_cell: args.target_cell,
            reject_any_overlay: args.options.reject_any_overlay,
        });
        Some(answer)
    })
}

pub(crate) fn supplied_path(cell: (i32, i32), urgency: u8) -> Option<SuppliedPath> {
    SEAM.with(|seam| {
        let mut seam = seam.borrow_mut();
        let seam = seam.as_mut().filter(|seam| !seam.live_paths)?;
        let path = seam.paths.pop_front().expect("unsupplied Find_Path");
        seam.records
            .push(FreshCallRecord::FindPath { cell, urgency });
        Some(path)
    })
}

/// Additional read/callback ordering in the live-effects Walk corpus.
pub(crate) fn observe_live(call: FreshCallRecord) {
    SEAM.with(|seam| {
        if let Some(seam) = seam.borrow_mut().as_mut()
            && seam.live_effects
        {
            seam.records.push(call);
        }
    });
}

/// Record a substituted call; true when a seam is installed, so the caller
/// skips the callee body.
pub(crate) fn substitute(call: FreshCallRecord) -> bool {
    SEAM.with(|seam| {
        seam.borrow_mut().as_mut().is_some_and(|seam| {
            seam.records.push(call);
            !seam.live_effects
        })
    })
}
