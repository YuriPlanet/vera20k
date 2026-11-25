//! `vera.vars` parsing and its load-time checks.

use super::{VarDecl, VarScope, Visibility, parse_vars};
use crate::{Limits, LoadError, SimModule};

/// One entry: name, scope byte, visibility byte, default.
fn entry(name: &str, scope: u8, visibility: u8, default: i64) -> Vec<u8> {
    let mut bytes = (name.len() as u16).to_le_bytes().to_vec();
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(&[scope, visibility]);
    bytes.extend_from_slice(&default.to_le_bytes());
    bytes
}

fn section(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        bytes.extend_from_slice(entry);
    }
    bytes
}

/// A module whose custom sections named `vera.vars` hold `contents`.
fn module(contents: &[Vec<u8>]) -> Vec<u8> {
    let escape = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>()
    };
    let sections: String = contents
        .iter()
        .map(|bytes| format!(r#"(@custom "vera.vars" "{}")"#, escape(bytes)))
        .collect();
    wat::parse_str(format!(
        r#"(module {sections} (memory (export "memory") 1))"#
    ))
    .expect("test WAT parses")
}

#[test]
fn a_well_formed_section_declares_its_variables_in_order() {
    let bytes = section(&[entry("Wave", 0, 3, 7), entry("Panicked", 2, 2, -1)]);
    assert_eq!(
        parse_vars(&bytes),
        Ok(vec![
            VarDecl {
                name: "Wave".into(),
                scope: VarScope::Global,
                visibility: Visibility::Public,
                default: 7,
            },
            VarDecl {
                name: "Panicked".into(),
                scope: VarScope::Object,
                visibility: Visibility::InSight,
                default: -1,
            },
        ])
    );
    let loaded = SimModule::load(&module(&[bytes]), Limits::MAP).expect("module loads");
    assert_eq!(loaded.vars().len(), 2);
}

#[test]
fn a_malformed_section_is_rejected() {
    let good = section(&[entry("Wave", 0, 0, 0)]);
    let mut format = good.clone();
    format[0] = 2;
    let truncated = good[..good.len() - 1].to_vec();
    let mut trailing = good.clone();
    trailing.push(0);
    let cases = [
        format,
        truncated,
        trailing,
        section(&[entry("Wäve", 0, 0, 0)]),
        section(&[entry("", 0, 0, 0)]),
        section(&[entry("Wave", 0, 0, 0), entry("WAVE", 1, 0, 0)]),
        section(&[entry("Wave", 3, 0, 0)]),
        // InSight is for object scope only.
        section(&[entry("Wave", 1, 2, 0)]),
    ];
    for bytes in cases {
        assert!(parse_vars(&bytes).is_err(), "{bytes:?}");
        assert!(matches!(
            SimModule::load(&module(&[bytes]), Limits::MAP),
            Err(LoadError::Invalid(_))
        ));
    }
    // Two sections, as two declaration blocks would give, are refused too.
    assert!(matches!(
        SimModule::load(&module(&[good.clone(), good]), Limits::MAP),
        Err(LoadError::Invalid(_))
    ));
}
