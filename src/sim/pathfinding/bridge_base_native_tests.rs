#[test]
fn native_bridge_base_edges_match_original_executable() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_base_edges.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 86);
    for case in cases {
        let records: Vec<_> = case["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let coord = |key: &str| {
                    (
                        r[key][0].as_i64().unwrap() as u16,
                        r[key][1].as_i64().unwrap() as u16,
                    )
                };
                BridgeEndpointRecord {
                    endpoint_a: coord("a"),
                    endpoint_b: coord("b"),
                    active: r["active"].as_bool().unwrap(),
                    bridge_kind: if r["kind"].as_u64().unwrap() == 0 {
                        BridgeRecordKind::High
                    } else {
                        BridgeRecordKind::Low
                    },
                }
            })
            .collect();
        let zones: Vec<_> = case["zones"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as u16)
            .collect();
        let size = (
            case["size"][0].as_i64().unwrap() as i32,
            case["size"][1].as_i64().unwrap() as i32,
        );
        let mut buckets = BaseEdgeBuckets::new();
        register_bridge_base_edges(
            &mut buckets,
            &zones,
            &records,
            case["width"].as_u64().unwrap() as u16,
            Some(size),
        );
        let actual: Vec<_> = buckets
            .buckets
            .iter()
            .flatten()
            .map(|&(a, b)| [a, b])
            .collect();
        assert_eq!(serde_json::json!(actual), case["pairs"], "{}", case["name"]);
        let adjacency = buckets.into_adjacency(*zones.iter().max().unwrap());
        for pair in actual {
            assert!(adjacency[pair[0] as usize].contains(&pair[1]));
            assert!(adjacency[pair[1] as usize].contains(&pair[0]));
        }
    }
}
