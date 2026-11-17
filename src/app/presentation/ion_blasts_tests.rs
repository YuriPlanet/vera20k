//! DrawAll's admission against `tools/superweapon_oracle.json` `ion_blast_draw`:
//! the points the original `CoordsToClient2 @ 0x006D2140` returned for each
//! row's blasts.

use super::*;
use serde_json::Value;

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

/// Each row's blasts through [`ion_blast_draws`]: the frames are centred on
/// the original's points, last blast first, and detail level 1 draws none.
/// VERA's world rows carry +15 over native's, so the camera is the view
/// origin 15 rows down.
#[test]
fn draws_centre_on_the_original_points() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let rows = oracle["ion_blast_draw"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let blasts: Vec<([i32; 3], i32)> = row["blasts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|blast| {
                (
                    [int(&blast[0]), int(&blast[1]), int(&blast[2])],
                    int(&blast[3]),
                )
            })
            .collect();
        let detail = int(&row["detail"]);
        let origin = &row["view_origin"];
        let draws = ion_blast_draws(
            blasts.into_iter(),
            [int(&origin[0]) as f32, (int(&origin[1]) + 15) as f32],
            [0.0, 0.0],
            [192.0, 128.0],
            detail as u32,
        );
        let centres: Vec<[i32; 2]> = draws
            .iter()
            .map(|draw| [draw.origin[0] + 256, draw.origin[1] + 128])
            .collect();
        let native: Vec<[i32; 2]> = row["points"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .filter(|point| detail == 2 && int(&point[2]) != 0)
            .map(|point| [int(&point[0]), int(&point[1])])
            .collect();
        assert_eq!(centres, native, "{}", row["name"]);
    }
}
