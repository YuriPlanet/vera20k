use super::*;
use crate::sim::intern::test_intern;

fn house(index: u32) -> InternedId {
    InternedId::from_index(index)
}

fn view(owner: InternedId, sw: &str, frames: i32, ready: bool) -> SuperTimerView {
    SuperTimerView::for_test(owner, test_intern(sw), frames, false, 0, 0).with_ready_for_test(ready)
}

/// A house -> launch team lookup, as the caller builds from the launch session.
fn teams(entries: Vec<(InternedId, Option<u8>)>) -> impl Fn(InternedId) -> Option<u8> {
    move |house| {
        entries
            .iter()
            .find(|(h, _)| *h == house)
            .and_then(|(_, t)| *t)
    }
}

/// The default player-count lookup for tests that do not exercise `(N)`.
fn no_players(_team: Option<u8>) -> usize {
    0
}

#[test]
fn team_labels() {
    assert_eq!(TeamKey::Named(0).label(0), "A");
    assert_eq!(TeamKey::Named(3).label(0), "D");
    assert_eq!(TeamKey::Named(14).label(0), "O");
    assert_eq!(TeamKey::Named(25).label(0), "Z");
    assert_eq!(TeamKey::Named(26).label(0), "T26");
    assert_eq!(
        TeamKey::Unteamed.label(7),
        "(7)",
        "no-team row shows player count"
    );
    assert_eq!(TeamKey::Unteamed.label(0), "(0)");
    assert!(
        TeamKey::Unteamed > TeamKey::Named(25),
        "unteamed rows sort last"
    );
}

/// Launch teams A and B (0 and 1), e on no team ("-"). The team letters come
/// from the real launch team numbers, NOT house order or the alliance graph.
#[test]
fn groups_by_launch_team_and_aggregates_per_type() {
    let [a, b, c, d, e] = [house(0), house(1), house(2), house(3), house(4)];
    let houses = [a, b, c, d, e];
    // houses registered a,b,c,d,e but teams assigned out of order: b+c are
    // launch team A, a+d are team B, e is unteamed.
    let links = teams(vec![
        (a, Some(1)),
        (b, Some(0)),
        (c, Some(0)),
        (d, Some(1)),
        (e, None),
    ]);
    let views = vec![
        view(b, "Nuke", 0, true),
        view(c, "Nuke", 45, false), // 45 / 15 = 3s, shortest pending in team A
        view(a, "Nuke", 0, true),
        view(d, "Nuke", 0, true),
        view(e, "Nuke", 0, true),
        // Team A also has two Lightning Storm holders, one ready.
        view(b, "LStorm", 0, true),
        view(c, "LStorm", 90, false),
    ];

    let rows = grouped_super_rows(&houses, links, &views, None, no_players);
    assert_eq!(rows.len(), 4, "team A: Nuke+LStorm, team B: Nuke, -: Nuke");

    let nuke_a = rows
        .iter()
        .find(|r| r.team == TeamKey::Named(0) && r.sw_type == test_intern("Nuke"))
        .unwrap();
    assert_eq!((nuke_a.ready, nuke_a.total), (1, 2));
    assert_eq!(nuke_a.nearest_seconds, 3);
    assert_eq!(
        nuke_a.representative, b,
        "stable representative is the first house in the team"
    );

    let storm_a = rows
        .iter()
        .find(|r| r.team == TeamKey::Named(0) && r.sw_type == test_intern("LStorm"))
        .unwrap();
    assert_eq!((storm_a.ready, storm_a.total), (1, 2));
    assert_eq!(storm_a.nearest_seconds, 6);

    let nuke_b = rows.iter().find(|r| r.team == TeamKey::Named(1)).unwrap();
    assert_eq!(
        (nuke_b.ready, nuke_b.total),
        (2, 2),
        "a+d are launch team B"
    );
    assert_eq!(nuke_b.nearest_seconds, 0, "all ready -> 0:00");

    let nuke_e = rows.iter().find(|r| r.team == TeamKey::Unteamed).unwrap();
    assert_eq!((nuke_e.ready, nuke_e.total), (1, 1), "e is the - group");
    assert_eq!(nuke_e.nearest_seconds, 0);
}

/// The reported bug: two houses launched on team A, but a live-alliance
/// inference split them into "-" and "D". Real launch teams keep them together.
#[test]
fn same_launch_team_stays_together_regardless_of_alliance() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = teams(vec![(a, Some(0)), (b, Some(0))]);
    let views = vec![view(a, "Nuke", 0, true), view(b, "Nuke", 30, false)];

    let rows = grouped_super_rows(&houses, links, &views, None, no_players);
    assert_eq!(rows.len(), 1, "both houses are launch team A");
    assert_eq!(rows[0].team, TeamKey::Named(0));
    assert_eq!((rows[0].ready, rows[0].total), (1, 2));
    assert_eq!(rows[0].nearest_seconds, 2);
}

/// Every unteamed house shares the single "-" bucket, counted together.
#[test]
fn unteamed_houses_share_one_bucket() {
    let [a, b, c] = [house(0), house(1), house(2)];
    let houses = [a, b, c];
    let links = teams(vec![(a, None), (b, None), (c, Some(1))]);
    let views = vec![
        view(a, "Nuke", 0, true),
        view(b, "Nuke", 30, false),
        view(c, "Nuke", 45, false),
    ];

    let rows = grouped_super_rows(&houses, links, &views, None, no_players);
    let unteamed = rows.iter().find(|r| r.team == TeamKey::Unteamed).unwrap();
    assert_eq!(
        (unteamed.ready, unteamed.total),
        (1, 2),
        "both unteamed rivals counted in -"
    );
    assert_eq!(unteamed.nearest_seconds, 2);
    assert_eq!(
        unteamed.representative, a,
        "first unteamed holder in house order"
    );
    assert!(
        rows.iter().any(|r| r.team == TeamKey::Named(1)),
        "c is team B"
    );
}

/// The local player's own Supers are never shown here, whether it is on a
/// named team or unteamed. It reads its own countdowns on the sidebar.
#[test]
fn local_supers_are_always_hidden() {
    let [a, b, c] = [house(0), house(1), house(2)];
    let houses = [a, b, c];
    let links = teams(vec![(a, Some(0)), (b, Some(0)), (c, None)]);
    let views = vec![
        view(a, "Nuke", 0, true),
        view(b, "Nuke", 30, false),
        view(c, "Nuke", 60, false),
    ];

    // Local on a named team: its Super still hidden, its team's tally is b's.
    let rows = grouped_super_rows(&houses, &links, &views, Some(a), no_players);
    let team_a = rows.iter().find(|r| r.team == TeamKey::Named(0)).unwrap();
    assert_eq!((team_a.ready, team_a.total), (0, 1), "only b counted");
    assert_eq!(team_a.nearest_seconds, 2);
    assert_eq!(team_a.representative, b);

    // Local unteamed: its Super hidden, c remains in the - bucket.
    let rows = grouped_super_rows(&houses, links, &views, Some(c), no_players);
    assert!(
        !rows.iter().any(|r| r.team == TeamKey::Unteamed),
        "local c hidden, no other unteamed holder"
    );
    assert!(
        rows.iter().any(|r| r.team == TeamKey::Named(0)),
        "a+b still shown"
    );
}

/// A local house not present in `houses` hides nothing.
#[test]
fn unknown_local_does_not_suppress_rows() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = teams(vec![(a, Some(0)), (b, Some(0))]);
    let views = vec![view(a, "Nuke", 0, true), view(b, "Nuke", 30, false)];

    let rows = grouped_super_rows(&houses, links, &views, Some(house(99)), no_players);
    let team_a = rows.iter().find(|r| r.team == TeamKey::Named(0)).unwrap();
    assert_eq!((team_a.ready, team_a.total), (1, 2));
}

/// A house whose team cannot be mapped falls into "-" rather than vanishing.
#[test]
fn unmapped_house_joins_unteamed_bucket() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = teams(vec![(b, Some(2))]); // a has no entry -> None
    let views = vec![view(a, "Nuke", 30, false), view(b, "Nuke", 45, false)];

    let rows = grouped_super_rows(&houses, links, &views, None, no_players);
    let unteamed = rows.iter().find(|r| r.team == TeamKey::Unteamed).unwrap();
    assert_eq!((unteamed.ready, unteamed.total), (0, 1));
    assert_eq!(unteamed.nearest_seconds, 2);
    assert!(
        rows.iter().any(|r| r.team == TeamKey::Named(2)),
        "b is team C"
    );
}

/// No Super views produce no rows.
#[test]
fn houses_without_super_views_produce_no_rows() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = teams(vec![(a, Some(0)), (b, Some(0))]);
    let rows = grouped_super_rows(&houses, links, &[], None, no_players);
    assert!(rows.is_empty());
}

/// Player counts are taken per launch team and stored on each row; the
/// no-team row's `(N)` caption reads from this field.
#[test]
fn player_counts_follow_the_launch_team() {
    let [a, b, c] = [house(0), house(1), house(2)];
    let houses = [a, b, c];
    // a on team A, b on team B, c unteamed. Team A has 2 players, B has 1,
    // and 4 players share the no-team bucket.
    let links = teams(vec![(a, Some(0)), (b, Some(1)), (c, None)]);
    let views = vec![
        view(a, "Nuke", 0, true),
        view(b, "Nuke", 0, true),
        view(c, "Nuke", 0, true),
    ];
    let counts = |team: Option<u8>| match team {
        Some(0) => 2,
        Some(1) => 1,
        None => 4,
        _ => 0,
    };

    let rows = grouped_super_rows(&houses, links, &views, None, counts);
    assert_eq!(
        rows.iter()
            .find(|r| r.team == TeamKey::Named(0))
            .unwrap()
            .players,
        2
    );
    assert_eq!(
        rows.iter()
            .find(|r| r.team == TeamKey::Named(1))
            .unwrap()
            .players,
        1
    );
    let unteamed = rows.iter().find(|r| r.team == TeamKey::Unteamed).unwrap();
    assert_eq!(unteamed.players, 4);
    assert_eq!(unteamed.team.label(unteamed.players), "(4)");
}

#[test]
fn frames_round_down_and_negative_clamp_to_zero() {
    let a = house(0);
    let houses = [a];
    let links = teams(vec![(a, Some(0))]);
    let views = vec![view(a, "Nuke", 14, false), view(a, "LStorm", -1, false)];
    let rows = grouped_super_rows(&houses, links, &views, None, no_players);
    let nuke = rows
        .iter()
        .find(|r| r.sw_type == test_intern("Nuke"))
        .unwrap();
    assert_eq!(nuke.nearest_seconds, 0, "14 frames truncates to 0 seconds");
    let storm = rows
        .iter()
        .find(|r| r.sw_type == test_intern("LStorm"))
        .unwrap();
    assert_eq!(storm.nearest_seconds, 0, "negative clamps to 0");
}
