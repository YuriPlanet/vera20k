//! Trigger graph diagnostics for map-authored trigger data.
//!
//! This is not trigger execution. It is a structural linker that answers:
//! - which `CellTags` resolve to actual `Tags`
//! - which `Tags` reference actual `Triggers`
//! - which `Triggers` have matching `Events` / `Actions`
//!
//! That gives us a practical sanity check before implementing the runtime
//! event/action system.

use crate::map::actions::ActionMap;
use crate::map::cell_tags::CellTagMap;
use crate::map::events::EventMap;
use crate::map::tags::TagMap;
use crate::map::triggers::TriggerMap;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LinkedTrigger {
    pub trigger_id: String,
    pub tag_ids: Vec<String>,
    pub tagged_cells: Vec<(u16, u16)>,
    pub event_id: Option<String>,
    pub action_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriggerGraph {
    pub triggers: Vec<LinkedTrigger>,
    pub dangling_cell_tags: Vec<String>,
    pub dangling_tag_trigger_refs: Vec<String>,
    pub untagged_triggers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerGraphDiagnostics {
    pub cell_tags_total: usize,
    pub cell_tags_resolved: usize,
    pub dangling_cell_tags: Vec<String>,
    pub tags_total: usize,
    pub tags_with_trigger_ref: usize,
    pub tags_resolved_to_triggers: usize,
    pub dangling_tag_trigger_refs: Vec<String>,
    pub triggers_total: usize,
    pub triggers_with_event: usize,
    pub triggers_with_action: usize,
    pub triggers_missing_event: Vec<String>,
    pub triggers_missing_action: Vec<String>,
}

pub fn build_trigger_graph(
    cell_tags: &CellTagMap,
    tags: &TagMap,
    triggers: &TriggerMap,
    events: &EventMap,
    actions: &ActionMap,
) -> TriggerGraph {
    let mut linked_triggers: Vec<LinkedTrigger> = Vec::new();
    let mut dangling_cell_tags: Vec<String> = Vec::new();
    let mut dangling_tag_trigger_refs: Vec<String> = Vec::new();

    for tag_id in cell_tags.values() {
        let normalized = tag_id.trim().to_ascii_uppercase();
        if !tags.contains_key(&normalized) {
            dangling_cell_tags.push(normalized);
        }
    }

    for trigger in triggers.values() {
        let mut tag_ids: Vec<String> = Vec::new();
        for tag in tags.values() {
            let trigger_ref = tag.fields.get(2).map(|s| s.trim()).unwrap_or("");
            if trigger_ref.is_empty() {
                continue;
            }
            let normalized = trigger_ref.to_ascii_uppercase();
            if normalized == trigger.id {
                tag_ids.push(tag.id.clone());
            }
        }
        tag_ids.sort();
        tag_ids.dedup();

        let mut tagged_cells: Vec<(u16, u16)> = cell_tags
            .iter()
            .filter_map(|(&(rx, ry), tag_id)| {
                let normalized = tag_id.trim().to_ascii_uppercase();
                tag_ids.contains(&normalized).then_some((rx, ry))
            })
            .collect();
        tagged_cells.sort();
        tagged_cells.dedup();

        linked_triggers.push(LinkedTrigger {
            trigger_id: trigger.id.clone(),
            tag_ids,
            tagged_cells,
            event_id: events.contains_key(&trigger.id).then(|| trigger.id.clone()),
            action_id: actions
                .contains_key(&trigger.id)
                .then(|| trigger.id.clone()),
        });
    }

    for tag in tags.values() {
        let trigger_ref = tag.fields.get(2).map(|s| s.trim()).unwrap_or("");
        if trigger_ref.is_empty() {
            continue;
        }
        let normalized = trigger_ref.to_ascii_uppercase();
        if !triggers.contains_key(&normalized) {
            dangling_tag_trigger_refs.push(format!("{} -> {}", tag.id, normalized));
        }
    }

    let mut untagged_triggers: Vec<String> = linked_triggers
        .iter()
        .filter(|linked| linked.tag_ids.is_empty())
        .map(|linked| linked.trigger_id.clone())
        .collect();

    linked_triggers.sort_by(|a, b| a.trigger_id.cmp(&b.trigger_id));
    dangling_cell_tags.sort();
    dangling_cell_tags.dedup();
    dangling_tag_trigger_refs.sort();
    dangling_tag_trigger_refs.dedup();
    untagged_triggers.sort();

    TriggerGraph {
        triggers: linked_triggers,
        dangling_cell_tags,
        dangling_tag_trigger_refs,
        untagged_triggers,
    }
}

pub fn analyze_trigger_graph(
    cell_tags: &CellTagMap,
    tags: &TagMap,
    triggers: &TriggerMap,
    events: &EventMap,
    actions: &ActionMap,
) -> TriggerGraphDiagnostics {
    let graph = build_trigger_graph(cell_tags, tags, triggers, events, actions);
    let mut diag = TriggerGraphDiagnostics {
        cell_tags_total: cell_tags.len(),
        tags_total: tags.len(),
        triggers_total: triggers.len(),
        ..TriggerGraphDiagnostics::default()
    };

    diag.cell_tags_resolved = cell_tags
        .len()
        .saturating_sub(graph.dangling_cell_tags.len());
    diag.dangling_cell_tags = graph.dangling_cell_tags;

    for tag in tags.values() {
        let trigger_ref = tag.fields.get(2).map(|s| s.trim()).unwrap_or("");
        if trigger_ref.is_empty() {
            continue;
        }
        diag.tags_with_trigger_ref += 1;
        let normalized = trigger_ref.to_ascii_uppercase();
        if triggers.contains_key(&normalized) {
            diag.tags_resolved_to_triggers += 1;
        } else {
            diag.dangling_tag_trigger_refs
                .push(format!("{} -> {}", tag.id, normalized));
        }
    }

    for linked in &graph.triggers {
        if linked.event_id.is_some() {
            diag.triggers_with_event += 1;
        } else {
            diag.triggers_missing_event.push(linked.trigger_id.clone());
        }
        if linked.action_id.is_some() {
            diag.triggers_with_action += 1;
        } else {
            diag.triggers_missing_action.push(linked.trigger_id.clone());
        }
    }

    diag.triggers_missing_event.sort();
    diag.triggers_missing_action.sort();

    diag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::actions::ActionMap;
    use crate::map::events::{EventMap, MapEvent};
    use crate::map::tags::{MapTag, TagMap};
    use crate::map::triggers::{MapTrigger, TriggerDifficulty, TriggerMap};

    fn make_trigger(id: &str) -> MapTrigger {
        MapTrigger {
            id: id.to_string(),
            fields: vec![
                "Neutral".to_string(),
                "<none>".to_string(),
                id.to_string(),
                "1".to_string(),
                "1".to_string(),
                "1".to_string(),
                "1".to_string(),
                "0".to_string(),
            ],
            owner: Some("Neutral".to_string()),
            linked_trigger_id: None,
            name: Some(id.to_string()),
            enabled: true,
            difficulty: TriggerDifficulty {
                easy: true,
                medium: true,
                hard: true,
            },
        }
    }

    #[test]
    fn test_build_trigger_graph_links_tags_cells_and_records() {
        let cell_tags: CellTagMap = [
            ((10, 10), "TAG_A".to_string()),
            ((11, 10), "TAG_A".to_string()),
            ((15, 15), "TAG_B".to_string()),
        ]
        .into_iter()
        .collect();
        let tags: TagMap = [
            (
                "TAG_A".to_string(),
                MapTag {
                    id: "TAG_A".to_string(),
                    fields: vec!["0".to_string(), "1".to_string(), "TRIG_A".to_string()],
                },
            ),
            (
                "TAG_B".to_string(),
                MapTag {
                    id: "TAG_B".to_string(),
                    fields: vec!["0".to_string(), "1".to_string(), "TRIG_B".to_string()],
                },
            ),
        ]
        .into_iter()
        .collect();
        let triggers: TriggerMap = [
            ("TRIG_A".to_string(), make_trigger("TRIG_A")),
            ("TRIG_B".to_string(), make_trigger("TRIG_B")),
        ]
        .into_iter()
        .collect();
        let events: EventMap = [(
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec!["1".to_string()],
                conditions: Vec::new(),
            },
        )]
        .into_iter()
        .collect();
        let actions: ActionMap = ActionMap::new();

        let graph = build_trigger_graph(&cell_tags, &tags, &triggers, &events, &actions);
        assert_eq!(graph.triggers.len(), 2);
        let trig_a = graph
            .triggers
            .iter()
            .find(|linked| linked.trigger_id == "TRIG_A")
            .expect("TRIG_A linked");
        assert_eq!(trig_a.tag_ids, vec!["TAG_A".to_string()]);
        assert_eq!(trig_a.tagged_cells, vec![(10, 10), (11, 10)]);
        assert_eq!(trig_a.event_id.as_deref(), Some("TRIG_A"));
        assert!(trig_a.action_id.is_none());
        assert!(graph.untagged_triggers.is_empty());
    }

    #[test]
    fn test_analyze_trigger_graph_counts_resolved_and_dangling_refs() {
        let cell_tags: CellTagMap = [
            ((10, 10), "TAG_A".to_string()),
            ((11, 10), "MISSING_TAG".to_string()),
        ]
        .into_iter()
        .collect();
        let tags: TagMap = [
            (
                "TAG_A".to_string(),
                MapTag {
                    id: "TAG_A".to_string(),
                    fields: vec!["0".to_string(), "1".to_string(), "TRIG_A".to_string()],
                },
            ),
            (
                "TAG_B".to_string(),
                MapTag {
                    id: "TAG_B".to_string(),
                    fields: vec!["0".to_string(), "1".to_string(), "TRIG_MISSING".to_string()],
                },
            ),
        ]
        .into_iter()
        .collect();
        let triggers: TriggerMap = [("TRIG_A".to_string(), make_trigger("TRIG_A"))]
            .into_iter()
            .collect();
        let events: EventMap = [(
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec!["0".to_string()],
                conditions: Vec::new(),
            },
        )]
        .into_iter()
        .collect();
        let actions: ActionMap = ActionMap::new();

        let diag = analyze_trigger_graph(&cell_tags, &tags, &triggers, &events, &actions);
        assert_eq!(diag.cell_tags_total, 2);
        assert_eq!(diag.cell_tags_resolved, 1);
        assert_eq!(diag.dangling_cell_tags, vec!["MISSING_TAG".to_string()]);
        assert_eq!(diag.tags_with_trigger_ref, 2);
        assert_eq!(diag.tags_resolved_to_triggers, 1);
        assert_eq!(
            diag.dangling_tag_trigger_refs,
            vec!["TAG_B -> TRIG_MISSING".to_string()]
        );
        assert_eq!(diag.triggers_with_event, 1);
        assert_eq!(diag.triggers_with_action, 0);
        assert_eq!(diag.triggers_missing_event, Vec::<String>::new());
        assert_eq!(diag.triggers_missing_action, vec!["TRIG_A".to_string()]);
    }
}
