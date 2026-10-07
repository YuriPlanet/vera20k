//! Original FireAt/AI instructions establish expectations; no Rust goldens.
use super::*;
use crate::rules::ini_parser::IniSection;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/voxel_oracle/recoil.json")).unwrap()
}

fn config(native: &Value, name: &str) -> RecoilConfig {
    let row = if name.starts_with("control-") || name == "refire" {
        native["readers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == if name == "refire" { "control-3" } else { name })
            .unwrap()
    } else {
        &native["physical"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()["layers"][0]
    };
    let keys = row["sections"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let mut section = IniSection::new(name.into());
    for (key, value) in keys.as_object().unwrap() {
        section.set(key, value.as_str().unwrap());
    }
    RecoilConfig::from_ini_section(&section)
}

fn assert_part(part: &Recoil, native: &Value, context: &str) {
    assert_eq!(
        part.state,
        native["state"].as_i64().unwrap() as i32,
        "state {context}"
    );
    assert_eq!(
        part.left,
        native["left"].as_i64().unwrap() as i32,
        "countdown {context}"
    );
    let expected: Vec<_> = native["config"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    assert_eq!(part.control.parameters().as_slice(), expected);
    // Rendering only. Original0E7F chops stock8/3 at each f32 store; Rust
    // rounds to nearest. Pin the observed bound without emulating x87.
    for (actual, key) in [(part.step, "step_bits"), (part.travel, "travel_bits")] {
        let expected = f32::from_bits(native[key].as_u64().unwrap() as u32);
        // Deliberate rapid refires accumulate another invisible float delta.
        let tolerance = if context.starts_with("refire ") {
            0.0001
        } else {
            0.00002
        };
        assert!(
            (actual - expected).abs() <= tolerance,
            "{context} {key}: {actual} != {expected}"
        );
    }
}

#[test]
fn recoil_original_histories_cover_fire_hold_recovery_and_refire() {
    let native = corpus();
    for row in native["histories"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let mut entity = GameEntity::test_default(1, name, "French", 10, 10);
        entity.initialize_voxel_recoil(config(&native, name));
        entity.fire_voxel_recoil(true);
        for (frame, expected) in row["frames"].as_array().unwrap().iter().enumerate() {
            if frame > 0 {
                entity.update_voxel_recoil();
                if row["refires"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_u64() == Some(frame as u64))
                {
                    entity.fire_voxel_recoil(true);
                }
            }
            if let Some(recoil) = &entity.voxel_recoil {
                for (part, expected) in recoil.parts.iter().zip(expected.as_array().unwrap()) {
                    assert_part(part, expected, &format!("{name} update{frame}"));
                }
            } else {
                assert!(!row["config"]["enabled"].as_bool().unwrap());
                assert_eq!(entity.voxel_recoil(), ([0.0; 2], false));
            }
        }
    }
}

#[test]
fn recoil_original_fireat_gates() {
    let native = corpus();
    for row in native["gates"].as_array().unwrap() {
        let mut section = IniSection::new("T".into());
        section.set(
            "TurretRecoil",
            if row["enabled"].as_bool().unwrap() {
                "yes"
            } else {
                "no"
            },
        );
        let mut entity = GameEntity::test_default(1, "T", "French", 10, 10);
        entity.initialize_voxel_recoil(RecoilConfig::from_ini_section(&section));
        entity.fire_voxel_recoil(row["has_turret"].as_bool().unwrap());
        for phase in ["armed", "tick"] {
            if phase == "tick" {
                entity.update_voxel_recoil();
            }
            if let Some(recoil) = &entity.voxel_recoil {
                for (part, expected) in recoil.parts.iter().zip(row[phase].as_array().unwrap()) {
                    assert_part(part, expected, phase);
                }
            } else {
                assert_eq!(entity.voxel_recoil(), ([0.0; 2], false));
            }
        }
    }
}
