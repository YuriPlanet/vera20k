//! Pure fresh Process_Movement caller decisions: Drive4B2630 / Ship6A1C80.
//!
//! This owner does not classify a cell or execute a response. The world caller
//! must preserve Mark0/query/saved-code/Mark1, callback reloads and each named
//! response body. Original-byte comparison: tools/spatial_oracle/
//! track_fresh_admission.{py,json,meta.json}; its callbacks are supplied returns
//! and it stops before response bodies. The separate TrackProcess chain policy
//! is deliberately outside this fresh caller owner.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FreshStage {
    First,
    Second,
}

/// Arguments2/3 of the recursive Process_Movement call. Argument1 remains the
/// same caller-owned retirement out-byte, independently of the returned AL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FreshRetry {
    pub allow_retry: bool,
    pub force_single_direction: bool,
}

/// Common continuation after the gate/scatter receiver, if that receiver's
/// native stop/next-destination arm has not already returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FreshRejectedTail {
    ///4B3607..3649 then4B3AA1..3C81: clear head/valid, optional blocked voice,
    /// clear the voice latch and retire selector. This is not code2's delay.
    FirstRetireSelector,
    ///4B41B3..41E7 then4B460C: clear live queue head/selector and LOCAL candidate
    /// XYZ, then run final publication/cleanup. Retained head clears later.
    SecondClearTrack,
}

/// A required response body, not a completed effect or a movement permission.
/// Retry flags on effect-bearing variants apply only at the native retry arm;
/// the caller must not skip the earlier redraw/head-clear/scatter decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FreshDispatch {
    /// First: terrain/speed/entering receiver. Second: two-node queue shift.
    Accept,
    /// The second query/coercions have completed; owner+90 is now false.
    OwnerNotAlive,
    /// Redraw483480 first, then retry if present; otherwise stop/next destination.
    Redraw { retry: Option<FreshRetry> },
    /// First code2: clear head/valid, then blocked and movement timer/FindPath
    /// ladder. It neither scatters nor uses the code7 retry/stop response.
    BlockedDelay,
    /// Gate578AD0 before the stage-specific rejected tail.
    Gate { tail: FreshRejectedTail },
    /// First code4/5: clear head/valid. The retry arm clears the live queue head
    /// and movement timer; otherwise resolve blocking object/wall and Override.
    WallOrObject { retry: Option<FreshRetry> },
    /// Code6: retry if allowed, otherwise evaluate the native XYZ/height/terrain
    /// stop gates before whole-cell Scatter. A single-blocker scatter is not
    /// this body. The tail follows only the non-returning branch.
    ScatterOrStop {
        retry: Option<FreshRetry>,
        tail: FreshRejectedTail,
    },
    /// First code7: head clear/optional voice, then retry or stop/next destination.
    FirstOtherBlocked { retry: Option<FreshRetry> },
    /// Second code7: queue/timer-clear retry or head-clear stop/next destination.
    SecondRetryOrStop { retry: Option<FreshRetry> },
    /// Second code2: recurse immediately, without the rejected-second clears.
    Retry(FreshRetry),
    /// Second code4/5: rejected-second queue/selector/local-XYZ clears precede
    /// recursion. It must not execute first-candidate Override directly.
    ClearSecondThenRetry(FreshRetry),
}

/// Fresh caller coercions AFTER CanEnter (and, for candidate1, AFTER Mark1).
/// Drive4B34D7..3525/4B4128..4177; Ship6A2B26..2B74/6A3754..37A3.
/// The second producer overwrites its saved packed cell at4B410B/6A3736, so
/// overlay_index must refer to the actual queried candidate at either stage.
/// Only the native predicate's0..7 domain is admitted by this interface.
#[cfg(test)]
pub(super) const fn coerce_entry_code(
    code: u8,
    is_train: bool,
    is_crusher: bool,
    overlay_index: i32,
) -> Option<u8> {
    if code > 7 {
        return None;
    }
    if (is_train && code < 7) || (matches!(code, 4 | 5) && is_crusher && overlay_index == 0) {
        Some(0)
    } else {
        Some(code)
    }
}

/// Select the required caller body from the already saved/coerced query code.
/// owner_alive is sampled after second-query coercions4B4179/6A37A5; there is
/// no corresponding first-query test here. This is not TrackHost::track_survives.
#[cfg(test)]
pub(super) const fn dispatch_entry(
    stage: FreshStage,
    effective_code: u8,
    allow_retry: bool,
    owner_alive: bool,
) -> Option<FreshDispatch> {
    if effective_code > 7 {
        return None;
    }
    if matches!(stage, FreshStage::Second) && !owner_alive {
        return Some(FreshDispatch::OwnerNotAlive);
    }
    let retry = if allow_retry {
        Some(FreshRetry {
            allow_retry: false,
            force_single_direction: false,
        })
    } else {
        None
    };
    let single = FreshRetry {
        allow_retry,
        force_single_direction: true,
    };
    let tail = match stage {
        FreshStage::First => FreshRejectedTail::FirstRetireSelector,
        FreshStage::Second => FreshRejectedTail::SecondClearTrack,
    };
    Some(match (stage, effective_code) {
        (_, 0) => FreshDispatch::Accept,
        // Redraw before recursion:4B394D..3984 /4B444A..4485.
        (_, 1) => FreshDispatch::Redraw { retry },
        (FreshStage::First, 2) => FreshDispatch::BlockedDelay,
        //4B420B..4219 /6A3837..3845 retain arg2 and publish arg3=1.
        (FreshStage::Second, 2) => FreshDispatch::Retry(single),
        (_, 3) => FreshDispatch::Gate { tail },
        (FreshStage::First, 4 | 5) => FreshDispatch::WallOrObject { retry },
        //4B41B3..41F7 /6A37DF..3823 clear before the same arg3=1 retry.
        (FreshStage::Second, 4 | 5) => FreshDispatch::ClearSecondThenRetry(single),
        (_, 6) => FreshDispatch::ScatterOrStop { retry, tail },
        (FreshStage::First, 7) => FreshDispatch::FirstOtherBlocked { retry },
        (FreshStage::Second, 7) => FreshDispatch::SecondRetryOrStop { retry },
        _ => return None,
    })
}

///4B3F7D..3F93 /6A35CC..35E2, before the later overlay/CrusherAll checks.
/// Native queue words are signed; retain the signed domain and do not truncate
/// a supplied word before these comparisons. The caller validates direction1.
#[cfg(test)]
pub(super) const fn normalize_second_direction(
    first: i32,
    second: i32,
    force_single_direction: bool,
) -> i32 {
    if second == 8 || second == -1 || force_single_direction {
        first
    } else {
        second
    }
}

#[cfg(test)]
#[path = "track_fresh_dispatch_tests.rs"]
mod tests;
