use super::super::ordinary::test_host::Host;
use super::*;
use serde_json::{Value, json};

impl OrdinaryRepairHost for Host {
    fn variant(&mut self) -> u8 {
        self.trace
            .push(json!({"kind":"random","minimum":0,"maximum":3,"result":self.variant}));
        self.variant
    }
}

#[test]
fn ordinary_repair_matches_original_overlay_and_callback_corpus() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_ordinary_repair.json",
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["input"];
        let coord = |row: &Value| {
            (
                row[0].as_i64().unwrap() as i16,
                row[1].as_i64().unwrap() as i16,
            )
        };
        let mut host = Host {
            cells: input["cells"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| (coord(row), row[5].as_i64().unwrap() as i32))
                .collect(),
            dummy: ((0, 0), -1),
            variant: input["variant"].as_u64().unwrap() as u8,
            trace: vec![],
        };
        repair(
            &mut host,
            coord(&input["start"]),
            if input["family"] == "low" {
                Family::Low
            } else {
                Family::High
            },
        )
        .unwrap();
        assert_eq!(
            json!(host.trace),
            case["result"]["trace"],
            "{}",
            input["name"]
        );
        let final_cells: Vec<_> = input["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let (x, y) = coord(row);
                [i32::from(x), i32::from(y), host.cells[&(x, y)]]
            })
            .collect();
        assert_eq!(
            json!(final_cells),
            case["result"]["final"],
            "{}",
            input["name"]
        );
        assert_eq!(
            json!([
                i32::from(host.dummy.0.0),
                i32::from(host.dummy.0.1),
                host.dummy.1
            ]),
            case["result"]["dummy"],
            "{}",
            input["name"]
        );
    }
}
