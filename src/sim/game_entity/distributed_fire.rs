//! Techno's paired candidates/scores (+458/+440) and fired history (+470).
//! Native709550 selects without RNG;707BCE..707CA4 removes first occurrences.
//! Shared scoring, class target assignment and pointer broadcast remain with
//! their existing owners. Native executable controls: distributed_fire.json.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use super::GameEntity;
use crate::sim::combat::TargetKind;

/// One accepted EvaluateCandidate result, before GreatestThreat's best keep.
/// The optional identity preserves native NULL controls; actual scanners
/// append represented entities and FireAt can remember a Cell target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DistributedFireCandidate {
    pub(crate) target: Option<TargetKind>,
    pub(crate) score: i32,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct DistributedFireState {
    candidates: Vec<DistributedFireCandidate>,
    history: Vec<Option<TargetKind>>,
}

impl DistributedFireState {
    ///709550: empty means NO setter call; a nonempty list whose usable scores
    /// are <=0 calls AssignTarget(NULL). First equal scores retain their order.
    fn select(&mut self) -> Option<Option<TargetKind>> {
        if self.candidates.is_empty() {
            self.history.clear();
            return None;
        }
        // Only membership is read from these temporary sets, never iteration:
        // hash seeding therefore cannot change the ordered native decisions.
        let current: HashSet<_> = self.candidates.iter().map(|entry| entry.target).collect();
        self.history.retain(|target| current.contains(target));
        let visited: HashSet<_> = self.history.iter().copied().collect();
        let unused = self
            .candidates
            .iter()
            .any(|entry| !visited.contains(&entry.target));
        if !unused {
            self.history.clear();
        }
        let mut best = None;
        let mut best_score = 0;
        for entry in &self.candidates {
            if (!unused || !visited.contains(&entry.target)) && entry.score > best_score {
                best = entry.target;
                best_score = entry.score;
            }
        }
        Some(best)
    }

    fn pointer_expired(&mut self, expired: Option<TargetKind>) {
        if let Some(index) = self
            .candidates
            .iter()
            .position(|entry| entry.target == expired)
        {
            self.candidates.remove(index);
        }
        if let Some(index) = self.history.iter().position(|target| *target == expired) {
            self.history.remove(index);
        }
    }
}

impl GameEntity {
    pub(crate) fn distributed_fire_candidates(&self) -> &[DistributedFireCandidate] {
        &self.distributed_fire.candidates
    }

    #[cfg(test)]
    pub(crate) fn distributed_fire_history(&self) -> &[Option<TargetKind>] {
        &self.distributed_fire.history
    }

    /// GreatestThreat6F8F80..6F8FA5 replaces only candidates/scores, never history.
    pub(crate) fn replace_distributed_fire_candidates(
        &mut self,
        candidates: Vec<DistributedFireCandidate>,
    ) {
        self.distributed_fire.candidates = candidates;
    }

    pub(crate) fn select_distributed_fire_target(&mut self) -> Option<Option<TargetKind>> {
        self.distributed_fire.select()
    }

    /// FireAt6FF872..6FF8EB appends its argument before the class null setter.
    pub(crate) fn remember_distributed_fire_target(&mut self, target: TargetKind) {
        self.distributed_fire.history.push(Some(target));
    }

    /// Outside PointerExpired's control/sensor gate, after the Techno forwards.
    pub(crate) fn distributed_fire_pointer_expired(&mut self, expired: u64) {
        self.distributed_fire
            .pointer_expired(Some(TargetKind::Entity(expired)));
    }

    pub(crate) fn distributed_fire_references(&self) -> impl Iterator<Item = u64> + '_ {
        self.distributed_fire
            .candidates
            .iter()
            .map(|entry| entry.target)
            .chain(self.distributed_fire.history.iter().copied())
            .filter_map(|target| match target {
                Some(TargetKind::Entity(id)) => Some(id),
                _ => None,
            })
    }

    /// Successful Unit74451C/Aircraft41B4DC/Infantry521A0C/Building453ECF
    /// Load calls NoInit -> Techno6F43AB..6F4410, constructing empty vectors.
    /// Raw saved vector headers have no element swizzles. Clear BEFORE strong
    /// reference restoration; mechanical serde/Clone does not call this.
    pub(crate) fn clear_distributed_fire_after_load(&mut self) {
        self.distributed_fire = DistributedFireState::default();
    }

    /// Native CRC70C51A..70C599 includes paired order/scores and fired history.
    pub(crate) fn hash_distributed_fire(&self, hasher: &mut impl Hasher) {
        // Preserve existing idle streams; nonempty runtime state is independent.
        if !self.distributed_fire.candidates.is_empty() || !self.distributed_fire.history.is_empty()
        {
            0x0070_9550u32.hash(hasher);
            self.distributed_fire.candidates.hash(hasher);
            self.distributed_fire.history.hash(hasher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn target(value: &Value) -> Option<TargetKind> {
        let id = value.as_u64().unwrap();
        (id != 0).then_some(TargetKind::Entity(id))
    }

    /// Full original selector plus bounded expiry tail. The six artificial
    /// allocator/setter-memory mutations exercise supplied harness callbacks,
    /// not gameplay allocators or the real class setter, and are excluded.
    #[test]
    fn distributed_fire_selection_and_first_expiry_match_native() {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/distributed_fire.json",
        ))
        .unwrap();
        let mut counts = [0, 0, 0];
        for row in corpus["rows"].as_array().unwrap() {
            let name = row["input"]["name"].as_str().unwrap();
            if name.starts_with("setter_mutation_") || name.starts_with("allocation_") {
                counts[2] += 1;
                continue;
            }
            let before = &row["before"];
            let mut state = DistributedFireState {
                candidates: before["candidates"]["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(before["scores"]["values"].as_array().unwrap())
                    .map(|(id, score)| DistributedFireCandidate {
                        target: target(id),
                        score: score.as_i64().unwrap() as i32,
                    })
                    .collect(),
                history: before["history"]["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(target)
                    .collect(),
            };
            let mut current = target(&before["current_target"]);
            match row["input"]["entry"].as_str().unwrap() {
                "select" => {
                    counts[0] += 1;
                    let assignment = state.select();
                    let events: Vec<_> = row["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|event| event["name"] == "assign_target")
                        .collect();
                    assert_eq!(assignment.is_some(), !events.is_empty(), "{name}");
                    if let Some(assigned) = assignment {
                        assert_eq!(assigned, target(&events[0]["args"][0]), "{name}");
                        current = assigned;
                    }
                }
                "expiry" => {
                    counts[1] += 1;
                    state.pointer_expired(target(&row["input"]["expired"]));
                }
                other => panic!("unexpected entry {other}"),
            }
            let after = &row["after"];
            let expected: Vec<_> = after["candidates"]["values"]
                .as_array()
                .unwrap()
                .iter()
                .zip(after["scores"]["values"].as_array().unwrap())
                .map(|(id, score)| DistributedFireCandidate {
                    target: target(id),
                    score: score.as_i64().unwrap() as i32,
                })
                .collect();
            assert_eq!(state.candidates, expected, "{name}");
            assert_eq!(
                state.history,
                after["history"]["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(target)
                    .collect::<Vec<_>>(),
                "{name}"
            );
            assert_eq!(current, target(&after["current_target"]), "{name}");
        }
        assert_eq!(counts, [120, 25, 6]);
    }
}
