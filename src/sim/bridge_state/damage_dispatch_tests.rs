//! Native489E87..48A2C4 comparison. Callback returns/writes come from the
//! corpus; this compares the production outer dispatcher, not driver bodies.

use super::{CellFields, DamageHost, dispatch, select_driver};
use crate::sim::bridge_state::DispatchPath;
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tools/spatial_oracle/bridge_damage_admission.json"
    )))
    .unwrap()
}

fn int(case: &Value, key: &str, default: i32) -> i32 {
    case.get(key)
        .and_then(Value::as_i64)
        .map_or(default, |value| value as i32)
}

fn boolean(case: &Value, key: &str, default: bool) -> bool {
    case.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn path_name(path: DispatchPath) -> &'static str {
    match path {
        DispatchPath::HighStateMachine => "A",
        DispatchPath::LowStateMachine => "B",
        DispatchPath::LowDirect => "C",
        DispatchPath::HighDirect => "D",
    }
}

struct Host<'a> {
    input: &'a Value,
    fields: [CellFields; 2],
    rng: SimRng,
    events: Vec<Value>,
    calls: Vec<&'static str>,
    counts: [usize; 4],
}

impl<'a> Host<'a> {
    fn new(input: &'a Value) -> Self {
        Self {
            input,
            fields: [
                CellFields {
                    flags: int(input, "flags", 0x100) as u32,
                    tile: int(input, "tile", -1),
                    overlay: int(input, "overlay", -1),
                    level: int(input, "level", 0) as u8,
                },
                CellFields {
                    flags: 0,
                    tile: -1,
                    overlay: int(input, "anchor_overlay", 0x18),
                    level: 0,
                },
            ],
            rng: SimRng::new(int(input, "seed", 31) as u64),
            // The production caller owns the initial impact lookup and outer
            // SpecialFlags/Wall gates. Native performs this lookup even when
            // an outer gate refuses every bridge block.
            events: vec![json!({"kind": "lookup", "coord": [10, 20]})],
            calls: Vec::new(),
            counts: [0; 4],
        }
    }
}

impl DamageHost for Host<'_> {
    type Cell = usize;

    fn fields(&self, cell: Self::Cell) -> CellFields {
        self.fields[cell]
    }

    fn resolve_anchor(&mut self, cell: Self::Cell) -> Option<Self::Cell> {
        let anchor = if self.fields[cell].flags & 0x80 != 0 {
            cell
        } else {
            1
        };
        self.events.push(json!({
            "kind": "lookup", "coord": if anchor == 0 { [10, 20] } else { [9, 20] }
        }));
        Some(anchor)
    }

    fn tile_bases(&self) -> [i32; 2] {
        [
            int(self.input, "bridge_base", 1000),
            int(self.input, "wood_base", 2000),
        ]
    }

    fn middle_tiles(&self) -> Option<[i32; 2]> {
        Some([20, 40])
    }

    fn roll_strength(&mut self) -> i32 {
        let high = int(self.input, "strength", 1500);
        let result = self.rng.next_range_i32_inclusive(1, high);
        self.events
            .push(json!({"kind": "rng", "low": 1, "high": high, "result": result}));
        result
    }

    fn apply(&mut self, path: DispatchPath) -> bool {
        let block = path_name(path);
        let index = usize::from(block.as_bytes()[0] - b'A');
        let invocation = self.counts[index];
        self.counts[index] += 1;
        let returned = self.input["returns"][block]
            .as_array()
            .map(|returns| {
                returns[invocation.min(returns.len() - 1)]
                    .as_bool()
                    .unwrap()
            })
            .unwrap_or(false);
        self.calls.push(block);
        self.events
            .push(json!({"kind": "driver", "block": block, "returned": returned}));
        if let Some(mutations) = self.input["mutations"].as_array() {
            for mutation in mutations {
                if mutation["after_block"] != block
                    || int(mutation, "after_call", 0) as usize != invocation
                {
                    continue;
                }
                let target = usize::from(mutation["target"] != "cell");
                let value = mutation["value"].as_i64().unwrap() as i32;
                match mutation["field"].as_str().unwrap() {
                    "overlay" => self.fields[target].overlay = value,
                    "flags" => self.fields[target].flags = value as u32,
                    "tile" => self.fields[target].tile = value,
                    "level" => self.fields[target].level = value as u8,
                    field => panic!("unsupported native callback field {field}"),
                }
                let mut event = mutation.clone();
                event["kind"] = json!("supplied_callback_write");
                self.events.push(event);
            }
        }
        returned
    }

    fn detach(&mut self, cell: Self::Cell) {
        assert_eq!(cell, 0, "native keeps the original impact cell receiver");
        self.events
            .push(json!({"kind": "detach", "block": self.calls.last().unwrap()}));
    }

    fn dirty(&mut self, path: DispatchPath) {
        self.events
            .push(json!({"kind": "dirty", "block": path_name(path)}));
    }
}

fn semantic_events(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| {
            let mut event = event.clone();
            let fields = event.as_object_mut().unwrap();
            match fields["kind"].as_str().unwrap() {
                "project" => return None,
                "rng" => {
                    // The shared RNG callback has no path argument. Its place
                    // between lookups/drivers/dirties establishes order.
                    fields.remove("block");
                }
                "driver" => {
                    fields.remove("entry");
                    fields.remove("coord");
                }
                "dirty_rect" => {
                    fields.insert("kind".to_owned(), json!("dirty"));
                    fields.remove("rect");
                }
                _ => {}
            }
            Some(event)
        })
        .collect()
}

#[test]
fn all_original_bridge_damage_blocks_match_live_callbacks_and_rng_continuation() {
    let native = corpus();
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 149);
    for row in cases {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut host = Host::new(input);
        if boolean(input, "destroyable", true) && boolean(input, "wall", true) {
            dispatch(
                &mut host,
                0,
                int(input, "damage", 2000),
                int(input, "impact_z", 416),
                boolean(input, "ion", false),
            );
        }
        assert_eq!(json!(host.calls), row["driver_calls"], "{name}: drivers");
        assert_eq!(
            host.events,
            semantic_events(row["events"].as_array().unwrap()),
            "{name}: callback order"
        );
        let state = host.rng.logical_view();
        assert_eq!(
            json!([state.index_a, state.index_b]),
            row["rng_indices"],
            "{name}: RNG cursor"
        );
        assert_eq!(
            state.index_a as u64,
            row["raw_draw_count"].as_u64().unwrap(),
            "{name}: raw draws"
        );
        let continuation: Vec<u32> = (0..4).map(|_| host.rng.next_u32()).collect();
        assert_eq!(
            json!(continuation),
            row["next_rng"],
            "{name}: RNG continuation"
        );
        assert_eq!(
            json!({"overlay": host.fields[0].overlay, "flags": host.fields[0].flags,
                   "anchor_overlay": host.fields[1].overlay}),
            row["final"],
            "{name}: callback state"
        );
    }
}

#[test]
fn inner_driver_reselects_live_overlay_and_family_like_original_587180() {
    let native = corpus();
    let rows = native["selector_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 63);
    for row in rows {
        let input = &row["input"];
        let mut host = Host::new(input);
        let selected = select_driver(&mut host, 0).map(path_name);
        assert_eq!(json!(selected), row["driver"], "{}", input["name"]);
        assert_eq!(host.rng.logical_view().index_a, 0);
        assert!(host.calls.is_empty());
    }
}

#[test]
fn bridge_strength_reads_signed_constructor_and_layer_values_from_native_corpus() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    let native = corpus();
    let default = RuleSet::from_ini(&IniFile::from_str("[General]\n"))
        .unwrap()
        .bridge_rules
        .strength;
    assert_eq!(
        i64::from(default),
        native["strength"]["constructor_default"].as_i64().unwrap()
    );
    let mut checked = 0;
    for row in native["strength"]["cases"].as_array().unwrap() {
        let raw = row["raw"].as_str();
        // The oracle intentionally accepts a pre-existing empty INI entry,
        // which ReadInt turns into0. The physical loader does not insert an
        // empty value, so this state has no from_rules_layers equivalent.
        if raw == Some("") {
            assert_eq!(
                crate::rules::ini_value::parse_read_int_value(""),
                Some(row["stored"].as_i64().unwrap() as i32)
            );
            continue;
        }
        let initial = row["initial"].as_i64().unwrap();
        let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
            "[CombatDamage]\nBridgeStrength={initial}\n"
        )));
        let patch = raw.map_or_else(
            || "[CombatDamage]\nUnrelated=1\n".to_owned(),
            |raw| format!("[CombatDamage]\nBridgeStrength={raw}\n"),
        );
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&patch));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            i64::from(rules.bridge_rules.strength),
            row["stored"].as_i64().unwrap(),
            "{row}"
        );
        checked += 1;
    }
    assert_eq!(checked, 45);
}
