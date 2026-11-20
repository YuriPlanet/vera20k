use super::*;
use crate::sim::intern::test_intern;

fn house(index: u32) -> InternedId {
    InternedId::from_index(index)
}

fn view(owner: InternedId, sw: &str, frames: i32, ready: bool) -> SuperReadyView {
    SuperReadyView::for_test(owner, test_intern(sw), frames, ready)
}

fn allies(pairs: &[(InternedId, InternedId)], count: usize) -> Vec<BTreeSet<InternedId>> {
    let mut sets = vec![BTreeSet::new(); count];
    for &(a, b) in pairs {
        sets[a.index() as usize].insert(b);
        sets[b.index() as usize].insert(a);
    }
    sets
}

#[test]
fn team_labels() {
    assert_eq!(TeamKey::Named(0).label(), "A");
    assert_eq!(TeamKey::Named(3).label(), "D");
    assert_eq!(TeamKey::Named(25).label(), "Z");
    assert_eq!(TeamKey::Named(26).label(), "T26");
    assert_eq!(TeamKey::Unteamed.label(), "-");
    assert!(TeamKey::Unteamed > TeamKey::Named(25), "unteamed rows sort last");
}

/// a+b allied (team A), d+e allied (team B, they start later in house order),
/// c alone ("-"). Each holds a nuke; ready counts and shortest pending
/// countdowns aggregate per team. Team letters follow house registration order.
#[test]
fn groups_by_alliance_and_aggregates_per_type() {
    let [a, b, c, d, e] = [house(0), house(1), house(2), house(3), house(4)];
    let houses = [a, b, c, d, e];
    let links = allies(&[(a, b), (d, e)], houses.len());
    let views = vec![
        view(a, "Nuke", 0, true),
        view(b, "Nuke", 45, false), // 45 / 15 = 3s, shortest pending in team A
        view(c, "Nuke", 600, false),
        view(d, "Nuke", 0, true),
        view(e, "Nuke", 0, true),
        // Team A also has two Lightning Storm holders, one ready.
        view(a, "LStorm", 0, true),
        view(b, "LStorm", 90, false),
    ];

    let rows = grouped_super_rows(&houses, &links, &views, None);
    assert_eq!(rows.len(), 4, "team A: Nuke+LStorm, team B: Nuke, -: Nuke");

    let nuke_a = rows.iter().find(|r| r.team == TeamKey::Named(0) && r.sw_type == test_intern("Nuke")).unwrap();
    assert_eq!((nuke_a.ready, nuke_a.total), (1, 2));
    assert_eq!(nuke_a.nearest_seconds, 3);
    assert_eq!(nuke_a.representative, a, "stable representative is the first house in the team");

    let storm_a = rows.iter().find(|r| r.team == TeamKey::Named(0) && r.sw_type == test_intern("LStorm")).unwrap();
    assert_eq!((storm_a.ready, storm_a.total), (1, 2));
    assert_eq!(storm_a.nearest_seconds, 6);

    let nuke_b = rows.iter().find(|r| r.team == TeamKey::Named(1)).unwrap();
    assert_eq!((nuke_b.ready, nuke_b.total), (2, 2), "d+e are the later team B");
    assert_eq!(nuke_b.nearest_seconds, 0, "all ready -> 0:00");

    let nuke_c = rows.iter().find(|r| r.team == TeamKey::Unteamed).unwrap();
    assert_eq!((nuke_c.ready, nuke_c.total), (0, 1), "c alone is the - group");
    assert_eq!(nuke_c.nearest_seconds, 40);
}

/// Local is in a named team: its own Supers count into that team's tally.
#[test]
fn local_in_named_team_counts_its_own_supers() {
    let [a, b, c] = [house(0), house(1), house(2)];
    let houses = [a, b, c];
    let links = allies(&[(a, b)], houses.len());
    let views = vec![
        view(a, "Nuke", 0, true),
        view(b, "Nuke", 30, false),
        view(c, "Nuke", 60, false),
    ];

    let rows = grouped_super_rows(&houses, &links, &views, Some(a));
    assert_eq!(rows.len(), 2);
    let team_a = rows.iter().find(|r| r.team == TeamKey::Named(0)).unwrap();
    assert_eq!(team_a.representative, a, "stable representative is the first house");
    assert_eq!((team_a.ready, team_a.total), (1, 2), "local 'a' counts into its team");
    assert_eq!(team_a.nearest_seconds, 2);
    assert!(rows.iter().any(|r| r.team == TeamKey::Unteamed), "c stays as no-team");
}

/// Local has no named team (it is alone): its own Supers are hidden, others
/// still show in the no-team bucket.
#[test]
fn local_without_team_hides_its_own_supers() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = allies(&[], houses.len());
    let views = vec![view(a, "Nuke", 0, true), view(b, "Nuke", 60, false)];

    let rows = grouped_super_rows(&houses, &links, &views, Some(a));
    assert_eq!(rows.len(), 1, "only b's row remains; local a is hidden");
    let unteamed = rows.iter().find(|r| r.team == TeamKey::Unteamed).unwrap();
    assert_eq!((unteamed.ready, unteamed.total), (0, 1));
    assert_eq!(unteamed.representative, b);
}

/// A local house not present in `houses` has no team and hides nothing (its
/// Supers could not be in `views` under a listed owner anyway).
#[test]
fn unknown_local_does_not_suppress_rows() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = allies(&[(a, b)], houses.len());
    let views = vec![view(a, "Nuke", 0, true), view(b, "Nuke", 30, false)];

    let rows = grouped_super_rows(&houses, &links, &views, Some(house(99)));
    let team_a = rows.iter().find(|r| r.team == TeamKey::Named(0)).unwrap();
    assert_eq!((team_a.ready, team_a.total), (1, 2));
}

/// Root changes never renumber teams, and adding a late ally to an existing
/// component does not split it. Anchor is the first house in native order.
#[test]
fn team_order_is_stable_in_house_order() {
    let [a, b, c, d] = [house(0), house(1), house(2), house(3)];
    let houses = [a, b, c, d];
    // Pair (c,d) comes first in insertion order, but (a,b) starts earlier in
    // the house array, so it must be team A.
    let links = allies(&[(c, d), (a, b)], houses.len());
    let views = vec![
        view(a, "Nuke", 15, false),
        view(c, "Nuke", 15, false),
    ];
    let rows = grouped_super_rows(&houses, &links, &views, None);
    assert_eq!(rows[0].team, TeamKey::Named(0));
    assert_eq!(rows[0].representative, a);
    assert_eq!(rows[1].team, TeamKey::Named(1));
    assert_eq!(rows[1].representative, c);
}

#[test]
fn houses_without_super_views_produce_no_rows() {
    let [a, b] = [house(0), house(1)];
    let houses = [a, b];
    let links = allies(&[(a, b)], houses.len());
    let rows = grouped_super_rows(&houses, &links, &[], None);
    assert!(rows.is_empty());
}

#[test]
fn frames_round_down_and_negative_clamp_to_zero() {
    let a = house(0);
    let houses = [a];
    let links = allies(&[], houses.len());
    let views = vec![
        view(a, "Nuke", 14, false),
        view(a, "LStorm", -1, false),
    ];
    let rows = grouped_super_rows(&houses, &links, &views, None);
    let nuke = rows.iter().find(|r| r.sw_type == test_intern("Nuke")).unwrap();
    assert_eq!(nuke.nearest_seconds, 0, "14 frames truncates to 0 seconds");
    let storm = rows.iter().find(|r| r.sw_type == test_intern("LStorm")).unwrap();
    assert_eq!(storm.nearest_seconds, 0, "negative clamps to 0");
}
