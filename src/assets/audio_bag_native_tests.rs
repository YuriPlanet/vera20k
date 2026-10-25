//! Synthetic inputs/results executed by the installed native DLL component oracle.
use super::*;

fn vectors() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/rules_oracle/ares_audio_bags.json")
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn stock_index_keeps_version1_and_strict_version_admission() {
    let controls = vectors();
    let v1 = controls["open"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "first_dword1_32byte")
        .unwrap();
    let bytes = unhex(v1["inputs"]["probe.idx"].as_str().unwrap());
    let parsed = AudioIndex::from_idx_bag(&bytes, vec![0; 9]).unwrap();
    assert_eq!(parsed.get("V1").unwrap().0.chunk_size, 0);
    let unknown = controls["open"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "gaba_version77_36byte")
        .unwrap();
    assert!(
        AudioIndex::from_idx_bag(
            &unhex(unknown["inputs"]["probe.idx"].as_str().unwrap()),
            vec![]
        )
        .is_none()
    );
}

#[cfg(feature = "ares")]
#[test]
fn ares_index_layout_sort_and_rejection_match_native_open() {
    for row in vectors()["open"].as_array().unwrap() {
        let files = row["inputs"].as_object().unwrap();
        let parsed = files
            .get("probe.idx")
            .zip(files.get("probe.bag"))
            .and_then(|(idx, bag)| {
                AudioIndex::from_ares_idx_bag(
                    &unhex(idx.as_str().unwrap()),
                    unhex(bag.as_str().unwrap()),
                )
            });
        assert_eq!(
            parsed.is_some(),
            row["output"]["file_retained"].as_bool().unwrap(),
            "{}",
            row["name"]
        );
        if let Some(index) = parsed {
            let expected = row["output"]["entries"].as_array().unwrap();
            assert_eq!(index.len(), expected.len());
            for (actual, expected) in index.entries().iter().zip(expected) {
                assert_eq!(
                    actual.name,
                    expected["name"].as_str().unwrap().to_ascii_uppercase()
                );
                assert_eq!(actual.offset, expected["offset"].as_u64().unwrap() as u32);
                assert_eq!(actual.size, expected["size"].as_u64().unwrap() as u32);
                assert_eq!(
                    actual.sample_rate,
                    expected["sample_rate"].as_u64().unwrap() as u32
                );
                assert_eq!(actual.flags, expected["flags"].as_u64().unwrap() as u32);
                assert_eq!(
                    actual.chunk_size,
                    expected["chunk_size"].as_u64().unwrap() as u32
                );
            }
        }
    }
}

#[cfg(feature = "ares")]
#[test]
fn ares_merge_and_exact_find_match_native_metadata_and_file_tokens() {
    for control in vectors()["merge"].as_array().unwrap() {
        let mut bags = Vec::new();
        let mut file_tokens = std::collections::HashMap::new();
        for (i, source) in control["inputs"].as_array().unwrap().iter().enumerate() {
            let entries = source["entries"].as_array().unwrap();
            let fixture = entries
                .iter()
                .map(|row| {
                    (
                        row[0].as_str().unwrap(),
                        row[1].as_u64().unwrap() as u32,
                        8,
                        22050,
                        4,
                    )
                })
                .collect::<Vec<_>>();
            bags.push(
                AudioIndex::from_ares_idx_bag(
                    &test_support::build_idx(&fixture),
                    vec![i as u8; 600],
                )
                .unwrap(),
            );
            file_tokens.insert(source["stem"].as_str().unwrap(), i as u8);
        }
        let index = AudioIndex::merge_ares(bags);
        assert_eq!(index.len(), control["rows"].as_array().unwrap().len());
        for (actual, expected) in index
            .entries()
            .iter()
            .zip(control["rows"].as_array().unwrap())
        {
            assert_eq!(
                actual.name,
                expected["name"].as_str().unwrap().to_ascii_uppercase()
            );
            assert_eq!(actual.offset, expected["offset"].as_u64().unwrap() as u32);
            let (_, bytes) = index.get(&actual.name).unwrap();
            assert_eq!(
                bytes,
                &[file_tokens[expected["file_stem"].as_str().unwrap()]; 8]
            );
        }
        for query in control["lookup"].as_array().unwrap() {
            let name = query["query"].as_str().unwrap();
            let ordinal = query["index"].as_i64().unwrap();
            assert_eq!(index.get(name).is_some(), ordinal >= 0);
            if ordinal >= 0 {
                assert_eq!(
                    index.get(name).unwrap().0.name,
                    index.entries()[ordinal as usize].name
                );
            }
        }
        assert!(
            index.get("voice.aud").is_none(),
            "original Find has no extension fallback"
        );
    }
}
