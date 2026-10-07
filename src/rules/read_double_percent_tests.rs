//! Original ReadDouble (`0x005283D0`) against the shared reader. Native rows:
//! tools/rules_oracle/read_double_percent.{py,json,meta.json}, group A.

use super::ini_parser::IniFile;
use super::ini_value::parse_read_double;
use serde_json::Value;

fn bits(value: &Value) -> u64 {
    u64::from_str_radix(value.as_str().unwrap(), 16).unwrap()
}

#[test]
fn read_double_percent_product_matches_original() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/read_double_percent.json",
    ))
    .unwrap();
    let reader = &native["reader"];
    let mut rows = Vec::new();
    for group in ["retail_tokens", "strings"] {
        for row in reader[group].as_array().unwrap() {
            rows.push((row["raw"].as_str().unwrap().to_owned(), bits(&row["bits"])));
        }
    }
    for (percent, row) in reader["percent_sweep"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        rows.push((format!("{percent}%"), bits(row)));
    }
    assert_eq!(rows.len(), 34 + 20 + 1001);
    for (raw, native_bits) in &rows {
        assert_eq!(
            parse_read_double(raw).to_bits(),
            *native_bits,
            "ReadDouble({raw:?})"
        );
    }

    // The section reader the Rules owners call, on the retail tokens.
    let tokens = reader["retail_tokens"].as_array().unwrap();
    let mut text = String::from("[S]\n");
    for (index, row) in tokens.iter().enumerate() {
        text.push_str(&format!("K{index}={}\n", row["raw"].as_str().unwrap()));
    }
    let ini = IniFile::from_str(&text);
    let section = ini.section("S").unwrap();
    for (index, row) in tokens.iter().enumerate() {
        assert_eq!(
            section.read_double(&format!("K{index}"), -1.0).to_bits(),
            bits(&row["bits"]),
            "read_double {}",
            row["raw"]
        );
    }
}
