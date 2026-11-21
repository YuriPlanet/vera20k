use super::*;

fn rank_only(parent: u64, index: u32, insignia: Option<&str>) -> ObjectExtrasRun {
    ObjectExtrasRun::Status {
        parent,
        bomb_clock: None,
        wrench: None,
        power_off: None,
        rank: Some(index),
        rank_insignia: insignia.map(str::to_owned),
        background: None,
        shield_background: None,
        fills: 0..0,
        scale: 0..0,
        self_heal: None,
        group_rectangles: 0..0,
        group_text: 0..0,
        digital_text: Vec::new(),
        digital_shapes: Vec::new(),
        talk_bubble: None,
    }
}

/// One rank draw binds one texture: a custom insignia rank never joins an adjacent stock PIPS rank's batch, which would
/// stretch the PIPS strip over its quad.
#[test]
fn rank_batches_split_where_the_rank_shape_changes() {
    let runs = [
        rank_only(1, 0, None),
        rank_only(2, 1, Some("herorank.shp")),
        rank_only(3, 2, Some("herorank.shp")),
        rank_only(4, 3, Some("spyins.shp")),
    ];
    assert_eq!(rank_run_end(&runs, 0), 1);
    assert_eq!(rank_run_end(&runs, 1), 3);
    assert_eq!(rank_run_end(&runs, 3), 4);
}
