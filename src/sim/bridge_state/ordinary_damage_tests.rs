use super::super::ordinary::test_host::Host;
use super::*;
use serde_json::{Value, json};

impl OrdinaryDamageHost for Host {
    fn notify_span(&mut self, first: CellCoord, end: CellCoord) -> Result<(), ()> {
        self.trace
            .push(json!({"kind":"notify", "endpoints":[first, end]}));
        super::super::rim::visit_span_cells(first, end, |point| {
            let cell = self.lookup(point);
            self.coord(cell)
        });
        Ok(())
    }
}

#[test]
fn concrete_damage_matches_original_all_overlay_states_and_width_entries() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_ordinary_damage.json",
    ))
    .unwrap();
    compare_cases(&corpus, Family::High);
}

#[test]
fn wooden_damage_matches_original_all_overlay_states_width_entries_and_physical_sequence() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/shrapnel_damage/scalar_test_vectors.json",
    ))
    .unwrap();
    compare_cases(&corpus, Family::Low);
    for sequence in corpus["physical_sequences"].as_array().unwrap() {
        compare_sequence(
            &sequence["input"],
            sequence["steps"].as_array().unwrap(),
            Family::Low,
        );
    }
}

fn compare_cases(corpus: &Value, family: Family) {
    for case in corpus["cases"].as_array().unwrap() {
        compare_sequence(
            &case["input"],
            std::slice::from_ref(&case["result"]),
            family,
        );
    }
}

fn compare_sequence(input: &Value, results: &[Value], family: Family) {
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
            .map(|row| {
                (
                    coord(row),
                    row[5].as_i64().map_or(-1, |overlay| overlay as i32),
                )
            })
            .collect(),
        dummy: ((0, 0), -1),
        variant: 0,
        trace: vec![],
    };
    for (step, expected) in results.iter().enumerate() {
        host.trace.clear();
        let returned = damage(&mut host, coord(&input["start"]), family).unwrap();
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
            json!({"returned":u8::from(returned), "trace":host.trace, "final":final_cells,
            "dummy":[i32::from(host.dummy.0.0),i32::from(host.dummy.0.1),host.dummy.1]}),
            *expected,
            "{} step{step}",
            input["name"]
        );
    }
}
