//! Test-only callee answers and call records for the Drive/Ship fresh-arm
//! replay of `tools/spatial_oracle/track_fresh_response`, which substitutes
//! the same four callees: Unit `Can_Enter_Cell` (0x73F0A0) and Foot
//! `Find_Path` (0x4D3920) answer from supplied queues; Cell `Scatter_Objects`
//! (0x481670) and `Foot::Override_Mission` (0x4D8F40) are recorded and, like
//! the oracle's substitutions, their bodies do not run. Nothing is supplied,
//! recorded or skipped unless a test installs the queues.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::sim::combat::TargetKind;

/// A supplied `Find_Path` answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SuppliedPath {
    /// AL = 1 after writing these words to Foot+5E0.
    Found(Vec<u8>),
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
        forced: bool,
        deck: bool,
    },
    Override {
        target: TargetKind,
    },
}

#[derive(Default)]
struct Seam {
    codes: VecDeque<u8>,
    paths: VecDeque<SuppliedPath>,
    records: Vec<FreshCallRecord>,
    core_null: bool,
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
            records: Vec::new(),
            core_null: false,
        })
    });
}

/// Remove the seam, returning the records and any unused answers.
pub(crate) fn finish() -> (Vec<FreshCallRecord>, usize) {
    SEAM.with(|seam| {
        seam.borrow_mut().take().map_or((Vec::new(), 0), |seam| {
            (
                seam.records,
                seam.codes.len() + seam.paths.len() + usize::from(seam.core_null),
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

pub(crate) fn supplied_path(cell: (i32, i32), urgency: u8) -> Option<SuppliedPath> {
    SEAM.with(|seam| {
        let mut seam = seam.borrow_mut();
        let seam = seam.as_mut()?;
        let path = seam.paths.pop_front().expect("unsupplied Find_Path");
        seam.records
            .push(FreshCallRecord::FindPath { cell, urgency });
        Some(path)
    })
}

/// Record a substituted call; true when a seam is installed, so the caller
/// skips the callee body.
pub(crate) fn substitute(call: FreshCallRecord) -> bool {
    SEAM.with(|seam| {
        seam.borrow_mut()
            .as_mut()
            .map(|seam| seam.records.push(call))
            .is_some()
    })
}
