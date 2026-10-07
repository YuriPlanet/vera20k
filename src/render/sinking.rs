//! Presentation-owned Techno+3CA waterline. The native constructor writes zero;
//! Unit73BEA4..73BF7B captures a signed world row on the first sinking draw and
//! clips subsequent draws against it. Rendering never mutates simulation state.
//! Native integer execution: tools/spatial_oracle/naval_sinking_clip.py.

use std::collections::BTreeMap;

use crate::render::draw_state::{DrawState, FX_SINKING_CLIP};

/// Retained draw history, keyed by the same stable identity as Display layers.
/// The app persists this separately from simulation and its deterministic hash.
#[derive(Debug, Default)]
pub(crate) struct SinkingWaterlines {
    rows: BTreeMap<u64, i16>,
}

impl SinkingWaterlines {
    /// `world_bottom` is the bottom of the native composite raster rectangle,
    /// not the atlas allocation. Its anchor already includes Object.Z. Native
    /// cached coordinates include a128px center; native_draw_bounds removes it.
    /// The first capture leaves this draw unclipped, even after writing +3CA.
    /// A retained positive row clips regardless of the current sinking byte.
    pub(crate) fn unit_draw(&mut self, id: u64, sinking: bool, world_bottom: i32) -> Option<i16> {
        if let Some(row) = self.retained_clip(id) {
            return Some(row);
        }
        if sinking {
            self.rows.insert(id, world_bottom as i16);
        }
        None
    }

    pub(crate) fn retained_clip(&self, id: u64) -> Option<i16> {
        self.rows.get(&id).copied().filter(|&row| row > 0)
    }

    pub(crate) fn saved(&self) -> Vec<(u64, i16)> {
        self.rows.iter().map(|(&id, &row)| (id, row)).collect()
    }

    pub(crate) fn restore(&mut self, rows: impl IntoIterator<Item = (u64, i16)>) {
        self.rows = rows.into_iter().collect();
    }

    pub(crate) fn clear(&mut self) {
        self.rows.clear();
    }

    pub(crate) fn retain(&mut self, mut live: impl FnMut(u64) -> bool) {
        self.rows.retain(|&id, _| live(id));
    }
}

/// The voxel shader's unused tint W lane transports this signed world row.
/// A separate flag preserves all ordinary tint/opacity semantics and avoids
/// changing SpriteInstance's cross-platform vertex ABI for a rare draw arm.
pub(crate) fn apply_waterline_clip(draw: &mut DrawState, row: Option<i16>) {
    draw.fx_flags &= !FX_SINKING_CLIP;
    if let Some(row) = row {
        draw.fx_flags |= FX_SINKING_CLIP;
        draw.effect_tint[3] = f32::from(row);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::Value;

    pub(crate) fn unit_rows() -> Vec<Value> {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/naval_sinking_clip.json",
        ))
        .unwrap();
        corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["input"]["kind"] == "unit")
            .cloned()
            .collect()
    }

    pub(crate) fn draw_input(cache: &mut SinkingWaterlines, input: &Value) -> Option<i16> {
        let n = |name: &str| input[name].as_i64().unwrap() as i32;
        cache.unit_draw(
            1,
            n("sinking") != 0,
            n("camera_world_y")
                .wrapping_add(n("draw_y"))
                .wrapping_add(n("raster_y"))
                .wrapping_sub(128)
                .wrapping_add(n("raster_height")),
        )
    }

    #[test]
    fn cached_unit_waterline_matches_original_capture_and_repeated_draws() {
        let mut visits = 0;
        for row in unit_rows() {
            let mut cache = SinkingWaterlines::default();
            cache.restore([(1, row["input"]["waterline"].as_i64().unwrap() as i16)]);
            let steps = row["sequence"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![row.clone()]);
            for step in steps {
                let clip = draw_input(&mut cache, &step["input"]);
                let expected = &step["output"];
                assert_eq!(
                    i64::from(cache.saved()[0].1),
                    expected["waterline_after"].as_i64().unwrap(),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    clip.map(i64::from),
                    (!expected["calls"].as_array().unwrap().is_empty())
                        .then(|| expected["waterline_before"].as_i64().unwrap()),
                    "{}",
                    row["name"]
                );
                visits += 1;
            }
        }
        assert_eq!(visits, 16);
    }
}
