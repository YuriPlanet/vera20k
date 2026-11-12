//! VERA's endless build: Ctrl + left click on an infantry, vehicle or
//! aircraft cameo keeps that type building until it is cancelled.
//!
//! Not native: gamemd's `SelectClass::Action @ 0x006AAD00` ignores modifiers
//! on a left press. Like Shift-Click Batch Queue it is part of the player's
//! `[VERA] ShiftQueue=` option and off by default.
//!
//! The repeat lives only on the clicking machine. It issues ordinary PRODUCE
//! events through [`super::commands::queue_build_by_type`], so other players,
//! replays and the sync hash see nothing but repeated clicks, and the
//! factory's own `[General] MaximumQueuedObjects=` and build-limit checks still
//! decide every copy.

use crate::app::AppState;
use crate::sim::production::{BuildQueueState, ProductionCategory, QueueItemView};

/// Copies of a repeated type kept in its strip: the running build plus one
/// waiting behind it, so the factory never idles between two units while the
/// next PRODUCE crosses the network.
const KEPT_COPIES: usize = 2;

/// Ticks to wait for an issued PRODUCE to show up in the queue before trying
/// again. A refused copy (build limit, full queue, lost prerequisite) is thus
/// retried about every three seconds instead of every frame.
const RETRY_TICKS: u64 = 45;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Repeat {
    type_id: String,
    /// The tick and the type's queue count when the last copy was issued.
    issued: Option<(u64, usize)>,
}

/// The types the local player has set to build endlessly.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct BuildRepeat {
    types: Vec<Repeat>,
}

impl BuildRepeat {
    pub(crate) fn is_repeating(&self, type_id: &str) -> bool {
        self.types.iter().any(|repeat| repeat.type_id == type_id)
    }

    /// Turn the repeat for `type_id` on or off; answers whether it is now on.
    pub(crate) fn toggle(&mut self, type_id: &str) -> bool {
        if self.stop(type_id) {
            return false;
        }
        self.types.push(Repeat {
            type_id: type_id.to_string(),
            issued: None,
        });
        true
    }

    /// End the repeat for `type_id`; answers whether it was on.
    pub(crate) fn stop(&mut self, type_id: &str) -> bool {
        let before = self.types.len();
        self.types.retain(|repeat| repeat.type_id != type_id);
        self.types.len() != before
    }

    /// The types that need one more PRODUCE this tick, given the local
    /// player's queue. Each answer is recorded as issued.
    fn due(
        &mut self,
        tick: u64,
        queue: &[QueueItemView],
        interned: impl Fn(&str) -> Option<crate::sim::intern::InternedId>,
    ) -> Vec<String> {
        let mut due = Vec::new();
        for repeat in &mut self.types {
            let Some(type_iid) = interned(&repeat.type_id) else {
                continue;
            };
            let copies: Vec<_> = queue
                .iter()
                .filter(|item| item.type_id == type_iid)
                .collect();
            // A held build stays held: the player paused it on purpose.
            if copies
                .iter()
                .any(|item| item.state == BuildQueueState::Paused)
            {
                continue;
            }
            let count = copies.len();
            if let Some((at, count_then)) = repeat.issued
                && count <= count_then
                && tick < at + RETRY_TICKS
            {
                continue;
            }
            repeat.issued = None;
            if count < KEPT_COPIES {
                repeat.issued = Some((tick, count));
                due.push(repeat.type_id.clone());
            }
        }
        due
    }
}

/// Whether Ctrl + left click may repeat a type of this strip: the mobile
/// strips, the same ones Shift-Click Batch Queue fills.
pub(crate) fn repeats(category: ProductionCategory) -> bool {
    matches!(
        category,
        ProductionCategory::Infantry | ProductionCategory::Vehicle | ProductionCategory::Aircraft
    )
}

/// Top up every repeated type once per game frame, before the frame's logic.
pub(crate) fn top_up(state: &mut AppState) {
    if state.match_state.input.build_repeat.types.is_empty() {
        return;
    }
    let Some(tick) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|runtime| runtime.simulation.session().tick())
    else {
        return;
    };
    let queue = super::dispatch::local_queue_view(state);
    let due = {
        let sim = state
            .match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.simulation);
        let interned = |name: &str| sim.and_then(|sim| sim.interner().get(name));
        // Split borrow: the repeat set is in `input`, the sim in `sim_runtime`.
        let repeat = &mut state.match_state.input.build_repeat;
        repeat.due(tick, &queue, interned)
    };
    for type_id in due {
        super::commands::queue_build_by_type(state, &type_id, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::intern::InternedId;

    fn item(type_id: InternedId, state: BuildQueueState) -> QueueItemView {
        QueueItemView {
            type_id,
            display_name: String::new(),
            queue_category: ProductionCategory::Infantry,
            state,
            progress: 0,
        }
    }

    #[test]
    fn keeps_two_copies_and_waits_for_the_issued_one() {
        let gi = InternedId::from_index(7);
        let interned = |name: &str| (name == "E1").then_some(gi);
        let mut repeat = BuildRepeat::default();
        assert!(repeat.toggle("E1"));

        // Idle strip: one copy now, then nothing until it arrives.
        assert_eq!(repeat.due(10, &[], interned), vec!["E1".to_string()]);
        assert!(repeat.due(11, &[], interned).is_empty());
        // It arrived: the second copy follows at once.
        let one = [item(gi, BuildQueueState::Building)];
        assert_eq!(repeat.due(12, &one, interned), vec!["E1".to_string()]);
        let two = [
            item(gi, BuildQueueState::Building),
            item(gi, BuildQueueState::Queued),
        ];
        assert!(repeat.due(13, &two, interned).is_empty());
        // A refused copy is retried only after RETRY_TICKS.
        assert_eq!(repeat.due(14, &one, interned), vec!["E1".to_string()]);
        assert!(repeat.due(14 + RETRY_TICKS - 1, &one, interned).is_empty());
        assert_eq!(
            repeat.due(14 + RETRY_TICKS, &one, interned),
            vec!["E1".to_string()]
        );
    }

    #[test]
    fn a_held_build_or_a_stopped_repeat_queues_nothing() {
        let gi = InternedId::from_index(7);
        let interned = |name: &str| (name == "E1").then_some(gi);
        let mut repeat = BuildRepeat::default();
        repeat.toggle("E1");
        let held = [item(gi, BuildQueueState::Paused)];
        assert!(repeat.due(1, &held, interned).is_empty());

        assert!(!repeat.toggle("E1"), "a second Ctrl-click turns it off");
        assert!(repeat.due(2, &[], interned).is_empty());
        repeat.toggle("E1");
        assert!(repeat.stop("E1"));
        assert!(!repeat.is_repeating("E1"));
    }
}
