use super::*;
use crate::rules::house_colors::{HouseColorIndex, house_text_rgb};
use crate::sim::house_state::{HouseState, MatchStatistics};

/// Entry: the sidebar gadget 0x006538FE and Request_Diplomacy_Menu 0x006ABC40.
#[test]
fn entry_follows_the_native_gadget_and_tab_rules_per_mode() {
    use DiplomacyRequest::{Hotkey, SidebarButton};
    use RequestedState::{Diplomacy, MissionBriefing, Nothing};
    let coop = Some(MpModeClass::Cooperative);
    let battle = Some(MpModeClass::Battle);
    for (request, mode, class, expected) in [
        (SidebarButton, SessionMode::Campaign, None, MissionBriefing),
        (Hotkey, SessionMode::Campaign, None, Nothing),
        (SidebarButton, SessionMode::Skirmish, battle, Diplomacy),
        (Hotkey, SessionMode::Skirmish, battle, Diplomacy),
        (SidebarButton, SessionMode::Lan, battle, Diplomacy),
        (SidebarButton, SessionMode::Lan, coop, MissionBriefing),
        (SidebarButton, SessionMode::Wol, coop, MissionBriefing),
        // Tab writes 8 even in co-op; the gadget's co-op test is LAN/WOL only.
        (Hotkey, SessionMode::Lan, coop, Diplomacy),
        (SidebarButton, SessionMode::Skirmish, coop, Diplomacy),
    ] {
        assert_eq!(
            requested_state(request, mode, class),
            expected,
            "{request:?} in {mode:?} with {class:?}"
        );
    }
}

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    schemes: Vec<crate::rules::color_scheme::ColorSchemeEntry>,
    colors: crate::map::houses::HouseColorMap,
    player: InternedId,
    first_ai: InternedId,
    second_ai: InternedId,
}

/// The local player sits at House index 2, behind two computer Houses; a
/// Neutral House without Multiplay follows.
fn fixture() -> Fixture {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Countries]\n0=Americans\n1=Alliance\n2=Russians\n3=Neutral\n\
         [Americans]\nMultiplay=yes\n[Alliance]\nMultiplay=yes\n\
         [Russians]\nMultiplay=yes\n[Neutral]\nMultiplayPassive=yes\n",
    ))
    .unwrap();
    let schemes = crate::rules::color_scheme::parse_color_schemes(
        &crate::rules::ini_parser::IniFile::from_str(
            "[Colors]\nDarkRed=0,230,255\nDarkBlue=153,214,212\nGrey=0,0,131\nLightGrey=0,0,200\n",
        ),
    );
    let mut sim = Simulation::with_seed(1);
    let mut house = |name: &str, country: &str, human: bool| {
        let name = sim.interner.intern(name);
        let country = sim.interner.intern(country);
        sim.houses
            .register(HouseState::new(name, 0, Some(country), human, 0, 10));
        name
    };
    let first_ai = house("Computer1", "Americans", false);
    let second_ai = house("Computer2", "Alliance", false);
    let player = house("Player", "Russians", true);
    house("Neutral", "Neutral", false);
    sim.session.set_current_house_for_test(Some(player));
    let colors = [("Computer1", 0), ("Computer2", 0), ("Player", 1)]
        .into_iter()
        .map(|(name, index)| (name.to_string(), HouseColorIndex(index)))
        .collect();
    Fixture {
        sim,
        rules,
        schemes,
        colors,
        player,
        first_ai,
        second_ai,
    }
}

const SKIRMISH: DiplomacyRules = DiplomacyRules {
    allies_allowed: true,
    quick_coop: false,
    local_inactive: false,
    game_mode_nonzero: true,
};

fn rows(fixture: &Fixture, switches: DiplomacyRules) -> ResolvedRows {
    resolve_rows(
        &fixture.sim,
        &fixture.rules,
        switches,
        &HouseUiSources {
            handle: Some("[New Player]"),
            computer: "Computer",
            schemes: &fixture.schemes,
            colors: &fixture.colors,
        },
        |_| None,
    )
}

fn ally(sim: &mut Simulation, asker: &str, other: &str) {
    sim.house_alliances
        .entry(asker.to_ascii_uppercase())
        .or_default()
        .insert(other.to_ascii_uppercase());
}

/// Row 0 is the local player and its own slot shows House 0; Houses beyond
/// the array or without Multiplay get no row.
#[test]
fn rows_swap_the_local_player_into_row_zero_and_skip_non_multiplay_houses() {
    let fixture = fixture();
    let resolved = rows(&fixture, SKIRMISH);
    let names: Vec<Option<&str>> = resolved
        .rows
        .iter()
        .map(|row| row.as_ref().map(|row| row.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            Some("[New Player]"),
            Some("Computer"),
            Some("Computer"),
            None,
            None,
            None,
            None,
            None
        ]
    );
    // Insignia by HouseType index: Russians (2) on row 0, then the swapped
    // Americans (0) on row 2, Alliance (1) on row 1.
    let insignia: Vec<_> = resolved.rows[..3]
        .iter()
        .map(|row| row.as_ref().unwrap().insignia)
        .collect();
    assert_eq!(
        insignia,
        [Some("frai.pcx"), Some("japi.pcx"), Some("usai.pcx")]
    );
    assert!(resolved.rows[0].as_ref().unwrap().allies.is_none());
}

/// Colours: the House's scheme, Grey once defeated; kills are the
/// MatchStatistics total.
#[test]
fn rows_colour_by_scheme_grey_when_defeated_and_count_kills() {
    let mut fixture = fixture();
    let second_ai = fixture.second_ai;
    fixture
        .sim
        .houses
        .get_mut(&second_ai)
        .unwrap()
        .set_is_defeated_for_test(true);
    fixture
        .sim
        .houses
        .get_mut(&fixture.player)
        .unwrap()
        .set_stats_for_test(MatchStatistics::from_totals_for_test(3, 2, 0, 0, 0));
    let resolved = rows(&fixture, SKIRMISH);
    let scheme = |name| {
        let entry =
            crate::rules::color_scheme::scheme_entry_by_name(&fixture.schemes, name).unwrap();
        house_text_rgb(&fixture.schemes, HouseColorIndex(entry as u8))
    };
    let row = |index: usize| resolved.rows[index].as_ref().unwrap();
    assert_eq!(row(0).rgb, scheme("DarkBlue"));
    assert_eq!(row(0).kills, Some(5));
    assert_eq!(row(1).rgb, scheme("Grey"));
    assert_eq!(row(2).rgb, scheme("DarkRed"));
    // A defeated House's Allies box is hidden; the header still counts it.
    assert!(row(1).allies.is_none());
    assert!(resolved.allies_header);
}

/// Checked is the player's one-way alliance, the partner flag the other
/// House's; computer rows are never enabled, and no row is human so the
/// Chat column and header stay hidden.
#[test]
fn allies_boxes_read_both_alliance_directions_and_disable_computer_rows() {
    let mut fixture = fixture();
    ally(&mut fixture.sim, "Player", "Computer1");
    ally(&mut fixture.sim, "Computer2", "Player");
    let resolved = rows(&fixture, SKIRMISH);
    let allies = |index: usize| resolved.rows[index].as_ref().unwrap().allies.unwrap();
    assert_eq!(
        allies(1),
        DiplomacyAlliesBox {
            checked: false,
            partner: true,
            enabled: false
        }
    );
    assert_eq!(
        allies(2),
        DiplomacyAlliesBox {
            checked: true,
            partner: false,
            enabled: false
        }
    );
    assert!(resolved.rows.iter().flatten().all(|row| row.chat.is_none()));
    assert!(!resolved.chat_header);
}

/// AlliesAllowed off (or an observing player, or WOL Quick Co-op) hides the
/// column and its header.
#[test]
fn allies_column_follows_allies_allowed() {
    let fixture = fixture();
    for switches in [
        DiplomacyRules {
            allies_allowed: false,
            ..SKIRMISH
        },
        DiplomacyRules {
            quick_coop: true,
            ..SKIRMISH
        },
    ] {
        let resolved = rows(&fixture, switches);
        assert!(!resolved.allies_header);
        assert!(
            resolved
                .rows
                .iter()
                .flatten()
                .all(|row| row.allies.is_none())
        );
    }
}

/// A human row's Allies box is enabled only while the local player is
/// active, and it shows the Chat box and header.
#[test]
fn human_rows_enable_allies_until_the_local_player_is_inactive() {
    let mut fixture = fixture();
    let first_ai = fixture.first_ai;
    let mut human = fixture.sim.houses.get(&first_ai).unwrap().clone();
    human.set_is_human_for_test(true);
    fixture.sim.houses.replace_for_test(human);
    let resolved = rows(&fixture, SKIRMISH);
    let row = resolved.rows[2].as_ref().unwrap();
    assert!(row.allies.unwrap().enabled);
    assert_eq!(
        row.chat,
        Some(DiplomacyChatBox {
            checked: true,
            enabled: true
        })
    );
    assert!(resolved.chat_header);
    let inactive = rows(
        &fixture,
        DiplomacyRules {
            local_inactive: true,
            ..SKIRMISH
        },
    );
    let row = inactive.rows[2].as_ref().unwrap();
    assert!(!row.allies.unwrap().enabled);
    assert!(!row.chat.unwrap().enabled);
}

/// Retail layering through the production reader: RULESMD says
/// `AlliesAllowed=no`, the Battle mode INI lays `yes` over it and Free For
/// All keeps `no`; the ten playable countries carry Multiplay, the special
/// Houses do not.
#[test]
fn retail_allies_allowed_layers_by_mode_and_rows_take_the_playable_countries() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let layered = |mode: Option<&str>| {
        let mut layers = RulesLayerStack::new(root.clone());
        if let Some(mode) = mode {
            layers.push(
                RulesLayerKind::GameMode,
                crate::rules::retail_ini_fixture::retail_ini(mode).expect(mode),
            );
        }
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap()
    };
    assert!(!layered(None).general.allies_allowed);
    assert!(layered(Some("mpbattlemd.ini")).general.allies_allowed);
    assert!(!layered(Some("mpfreeforallmd.ini")).general.allies_allowed);
    let rules = layered(Some("mpbattlemd.ini"));
    for country in [
        "Americans",
        "Alliance",
        "French",
        "Germans",
        "British",
        "Africans",
        "Arabs",
        "Confederation",
        "Russians",
        "YuriCountry",
    ] {
        assert!(rules.country_multiplay(country), "{country}");
    }
    for country in ["Neutral", "Special"] {
        assert!(!rules.country_multiplay(country), "{country}");
    }
    // The insignia follow the HouseType index: Americans 0 .. YuriCountry 9.
    assert_eq!(
        rules.country_index("Americans").map(|index| index.0),
        Some(0)
    );
    assert_eq!(
        rules.country_index("YuriCountry").map(|index| index.0),
        Some(9)
    );
}

/// State_Machine 0x0048C8C6: outside campaign a House flagged to die (or
/// won, or lost) opens no modal, nor does an undefeated one once the latch
/// is clear; campaign skips the test.
#[test]
fn state_machine_refuses_modals_once_the_local_house_is_flagged() {
    let mut fixture = fixture();
    let player = fixture.player;
    let house = |fixture: &Fixture| fixture.sim.houses.get(&player).cloned();
    let live = house(&fixture);
    assert_eq!(
        App::state_machine_admission(true, true, live.as_ref()),
        Some(true)
    );
    // A confirmed Abort cleared the latch: refused until defeated, and a
    // defeated House relatches it.
    assert_eq!(
        App::state_machine_admission(true, false, live.as_ref()),
        None
    );
    let mut defeated = live.clone().unwrap();
    defeated.set_is_defeated_for_test(true);
    assert_eq!(
        App::state_machine_admission(true, false, Some(&defeated)),
        Some(true)
    );
    assert!(fixture.sim.flag_house_to_die(player));
    let flagged = house(&fixture);
    assert_eq!(
        App::state_machine_admission(true, true, flagged.as_ref()),
        None
    );
    assert_eq!(
        App::state_machine_admission(false, false, flagged.as_ref()),
        Some(false)
    );
}
