//! Campaign victory continuation: the GameMode 0 tail of `GameExit::Victory`
//! `0x00685670` after StandaloneScore dialog `0x103` returns, or after
//! `[Basic] SkipScore=yes` bypasses that dialog. Generic score dialog `0x108`
//! is a non-campaign path.
//!
//! Native order (`0x0068594B..0x00685D7F`): the PostScore movie
//! (`Scenario+0x1448`), then PreMapSelect (`+0x144C`), each through
//! Play_Movie `0x005BF260` when set. OneTimeOnly (`+0x34AF`) then returns to
//! the shell. EndOfGame (`+0x34AC`) plays the campaign's FinalMovie
//! (`Campaign+0x29C`) and Show_Credits `0x004C3E30`, then returns. Otherwise
//! MapSelect writes the next map into Scenario+0x125C, the carry-over record
//! `0x004C6180` saves the 50 globals, the mission counter Scenario+0x1254
//! increments (`0x00685AAF`) and Start_Scenario `0x00683AB0` runs with the
//! retained campaign index (`0x00685C47`). The record (globals, the
//! player House's money, difficulty row and remaining countdown) is written back by `0x004C6210` once
//! Start_Scenario has read the next map; see `sim::campaign_carry_over`.
//!
//! Residuals, all inert on the retail campaign maps:
//! - `[Basic] SkipScore` (`+0x34AE`) is read by
//!   the outcome router: yes skips only `0x103`, then follows this continuation;
//!   retail campaign maps use `SkipScore=no`.
//! - the calls after the restore (`0x00685D7F..0x00685DAE`: `0x007241A0`,
//!   `0x004F4480`, `0x004A3C30`) are not ported.
//! - Session+0x3C0 bit 2 (`0x00685934`) skips the score, movies and
//!   MapSelect and restarts Scenario+0x125C; VERA has no writer of that bit.
//! - Show_Credits returns to the main menu with INTRO queued, as the shell's
//!   credits do; the theme native plays when Main_Game returns is not traced.
//! - the carry-over record is written after installation and is not part of
//!   the startup the diagnostic replay records.
//! SkipMapSelect5ADE20 matches the selected NextScenario against the saved
//! stage's choices; both paths publish Scenario+35A0 before EvadeSave4C6180.
//! Saved campaigns retain their Scenario index and incoming carry-over;
//! their wins enter this same continuation after saved-world admission.

use std::collections::VecDeque;

use super::{App, AppState};
use crate::app::match_bootstrap::PreparedCampaignStartup;
use crate::rules::ini_parser::IniFile;

/// `CCINIClass::ReadString` into a CString (`0x00528C00`) reads through a
/// 0x400-byte buffer.
const MAP_SELECT_VALUE_CAPACITY: usize = 0x400;

/// gamemd GameExit::Victory @0x00685670,685915..685960: Play_Movie_ByIndex
/// 5BF260 reads Scenario+143C before SkipScore is tested. ReadMovie4757D0
/// resolves against the fixed ART registry; SetDefaults6837F4 supplies -1.
/// The existing movie owner returns to the retained outcome on every exit.
pub(crate) fn win_movie<'a>(
    destination: &super::match_runtime::scenario_exit::ScenarioExitDestination,
    basic: &crate::map::basic::BasicSection,
    movies: &'a crate::rules::movies::MovieRegistry,
) -> Option<&'a str> {
    use super::match_runtime::scenario_exit::ScenarioExitDestination;
    if !matches!(
        destination,
        ScenarioExitDestination::CampaignScore { .. }
            | ScenarioExitDestination::CampaignVictoryContinue
    ) {
        return None;
    }
    movies.name(movies.find_index(basic.win.as_deref()?))
}

/// What a won campaign mission does once its score dialog returns.
#[derive(Debug)]
pub(crate) struct CampaignVictory {
    /// Play_Movie calls still to run, in native order.
    movies: VecDeque<String>,
    then: VictoryEnd,
}

#[derive(Debug)]
enum VictoryEnd {
    /// OneTimeOnly, or a map VERA cannot continue: the shell resumes.
    Shell,
    /// EndOfGame: Show_Credits, then the shell.
    Credits,
    /// Start_Scenario with the next map.
    NextMission {
        startup: Box<PreparedCampaignStartup>,
        rearm_initial_save: bool,
    },
    /// FUN_005ADD10's failed Init: acknowledge before retrying this map.
    MapSelectFailed(Box<PreparedCampaignStartup>),
}

/// MapSelect's result for Scenario+0x125C.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MapSelectPick {
    /// The chosen stage's `Scenario=` replaces the map.
    Next { scenario: String, stage: u16 },
    /// Scenario+0x125C keeps the current map, which Start_Scenario replays.
    KeepCurrent,
    /// Init5AE220 failed. Wrapper5ADD10 shows MSG:UnableMapSelect; victory
    /// ignores its return and reloads the current map without rearming ABCE08.
    InitFailed,
    /// SkipMapSelect found the saved stage but no matching destination after
    /// clearing Scenario+125C (5AE013). Show the notice, then attempt that name.
    DestinationMissing,
    /// SetNextScenario dereferences a missing choice after successful Init.
    Fault(&'static str),
}

/// The stage sections MapSel init (`0x005CEEF0`) lists for one side: the
/// side section's keys `1`, `2`, ... up to the first absent one
/// (`0x005CEF98..0x005CF073`).
fn map_select_stages(mapsel: &IniFile, side: &str) -> Vec<String> {
    let Some(section) = mapsel.section(side) else {
        return Vec::new();
    };
    (1..=100)
        .map(|stage: u32| section.read_string(&stage.to_string(), "", MAP_SELECT_VALUE_CAPACITY))
        .take_while(|name| !name.is_empty())
        .collect()
}

/// The index of `side`'s MAPSELMD stage whose `Scenario=` is `scenario`, as
/// `0x005CF760` finds the current stage.
pub(crate) fn map_select_stage_of(mapsel: &IniFile, side: &str, scenario: &str) -> Option<u16> {
    map_select_stages(mapsel, side)
        .iter()
        .position(|stage| {
            map_select_value(mapsel, stage, "Scenario").eq_ignore_ascii_case(scenario)
        })
        .and_then(|index| u16::try_from(index).ok())
}

fn map_select_value(mapsel: &IniFile, section: &str, key: &str) -> String {
    mapsel
        .section(section)
        .map(|section| section.read_string(key, "", MAP_SELECT_VALUE_CAPACITY))
        .unwrap_or_default()
}

/// MapSelect without SkipMapSelect (`0x005ADD10` -> `0x005AE100`); it shows
/// no selection dialog. Failed Init shows the wrapper's message box. `side`
/// is the ID of the player House type's side
/// (`Scenario+0x1C74` -> `HouseType+0xBC`, `0x005ADF40..0x005ADF5B`).
pub(crate) fn map_select(
    mapsel: Option<&IniFile>,
    side: Option<&str>,
    current: &str,
    skip: Option<(u16, &str)>,
) -> MapSelectPick {
    // FUN_005ADF20 rejects an empty NextScenario before opening MAPSELMD.
    if skip.is_some_and(|(_, next)| next.is_empty()) {
        return MapSelectPick::InitFailed;
    }
    let stages = match (mapsel, side) {
        (Some(mapsel), Some(side)) => map_select_stages(mapsel, side),
        _ => Vec::new(),
    };
    // Init `0x005AE220` fails without the file, a side or any stage.
    let Some(mapsel) = mapsel.filter(|_| !stages.is_empty()) else {
        return MapSelectPick::InitFailed;
    };
    // `0x005CF760`: the stage whose Scenario equals the current map, both
    // uppercased (`0x007B5B30`).
    let stage = match skip {
        // 5CF810 masks the saved word and indexes the loaded vector.
        Some((index, _)) => stages.get(usize::from(index)),
        None => stages.iter().find(|stage| {
            map_select_value(mapsel, stage, "Scenario").eq_ignore_ascii_case(current)
        }),
    };
    let Some(stage) = stage else {
        // gamemd FUN_005AE220 @0x005AE220,5AE31C..5AE369 rejects the
        // current stage before SetNextScenario5AE126 rearms the save latch.
        return MapSelectPick::InitFailed;
    };
    // `0x005AE15C`: only a stage with a MapVQ chooses.
    if skip.is_none() && map_select_value(mapsel, stage, "MapVQ").is_empty() {
        return MapSelectPick::KeepCurrent;
    }
    // Choices are keys `0..=255`, lowest first (`0x005CFF81..0x005D0040`);
    // `0x005AE188` takes the first.
    let choices = (0..=255u32)
        .map(|key| map_select_value(mapsel, stage, &key.to_string()))
        .filter(|choice| !choice.is_empty());
    for choice in choices {
        // 5CF6B0 finds the stage; 5CF830 -> vtable5D11D0 returns its index.
        let Some((index, next)) = stages
            .iter()
            .enumerate()
            .find(|(_, stage)| stage.eq_ignore_ascii_case(&choice))
        else {
            return MapSelectPick::Fault("the MAPSELMD choice is no stage");
        };
        let scenario = map_select_value(mapsel, next, "Scenario");
        if let Some((_, desired)) = skip {
            // 5AE025..5AE096 compares lowercased scenario strings, visiting
            // every authored choice, independently of MapVQ.
            if !scenario.eq_ignore_ascii_case(desired) {
                continue;
            }
        } else if scenario.is_empty() {
            return MapSelectPick::KeepCurrent;
        }
        return MapSelectPick::Next {
            scenario,
            stage: index as u16,
        };
    }
    if skip.is_some() {
        MapSelectPick::DestinationMissing
    } else {
        MapSelectPick::Fault("the current MAPSELMD stage has no choice")
    }
}

/// GameExit::Victory's branch once the score has returned.
#[derive(Debug, PartialEq, Eq)]
enum VictoryBranch {
    /// OneTimeOnly (`0x006859B9`).
    Shell,
    /// EndOfGame (`0x006859C7`).
    Credits,
    /// MapSelect, then Start_Scenario.
    MapSelect,
}

/// The movies and branch the finished map's `[Basic]` selects
/// (`0x00685965..0x00685A63`). `movie` resolves a catalog name; `final_movie`
/// is the campaign's FinalMovie when it is not -1 (`0x006859C9`).
fn victory_plan(
    basic: &crate::map::basic::BasicSection,
    movie: impl Fn(&str) -> Option<String>,
    final_movie: Option<String>,
) -> (VecDeque<String>, VictoryBranch) {
    let mut movies: VecDeque<String> = [&basic.post_score, &basic.pre_map_select]
        .into_iter()
        .filter_map(|name| movie(name.as_deref()?))
        .collect();
    let branch = if basic.one_time_only.unwrap_or(false) {
        VictoryBranch::Shell
    } else if basic.end_of_game.unwrap_or(false) {
        movies.extend(final_movie);
        VictoryBranch::Credits
    } else {
        VictoryBranch::MapSelect
    };
    (movies, branch)
}

impl App {
    /// Read everything the victory tail needs while the won Simulation is
    /// still resident. Only a won mission of an installed campaign qualifies.
    pub(super) fn take_campaign_victory(state: &mut AppState) -> Option<CampaignVictory> {
        let campaign = state.match_state.startup.installed_campaign()?;
        let runtime = state.match_state.sim_runtime.as_mut()?;
        let sim = &mut runtime.simulation;
        let owner = sim.session().current_house()?;
        let won = sim.ready_outcome_for_owner(owner).is_some_and(|outcome| {
            outcome.kind == crate::sim::house_state::HouseOutcomeKind::Victory
        });
        if !won {
            return None;
        }
        let movies = state
            .process_assets
            .native_rules()
            .map(|rules| rules.movies());
        let (queue, branch) = victory_plan(
            &state.match_state.map_basic,
            |name| {
                let movies = movies?;
                movies.name(movies.find_index(name)).map(str::to_owned)
            },
            movies.and_then(|movies| {
                movies
                    .name(campaign.campaign().final_movie())
                    .map(str::to_owned)
            }),
        );
        let then = match branch {
            VictoryBranch::Shell => VictoryEnd::Shell,
            VictoryBranch::Credits => VictoryEnd::Credits,
            VictoryBranch::MapSelect => {
                let mapsel = state.process_assets.manager().and_then(|assets| {
                    crate::rules::retail_sources::select_ini(assets, "MAPSELMD.INI")
                        .map_err(|error| log::warn!("MapSelect: {error}"))
                        .ok()
                });
                let side = crate::sim::house_state::house_state_for_owner_id(sim.houses(), owner)
                    .and_then(|house| {
                        runtime
                            .resources
                            .rules
                            .side_name(crate::rules::ruleset::SideIdx(
                                u8::try_from(house.side_index()).ok()?,
                            ))
                    });
                let current = sim.session().map_name();
                // Victory685A38..685A5C selects +108 with Global0, otherwise +4.
                let skip = state.match_state.map_basic.skip_map_select.unwrap_or(false);
                let next = if sim.global_variables()[0] {
                    state.match_state.map_basic.alt_next_scenario.as_deref()
                } else {
                    state.match_state.map_basic.next_scenario.as_deref()
                }
                .unwrap_or("");
                let pick = map_select(
                    mapsel.as_ref().map(|selected| &selected.ini),
                    side,
                    current,
                    skip.then_some((sim.session().campaign_map_stage(), next)),
                );
                let init_failed = matches!(
                    pick,
                    MapSelectPick::InitFailed | MapSelectPick::DestinationMissing
                );
                let scenario = match pick {
                    MapSelectPick::Next { scenario, stage } => {
                        sim.set_campaign_map_stage(stage);
                        Some(scenario)
                    }
                    MapSelectPick::DestinationMissing => Some(String::new()),
                    MapSelectPick::KeepCurrent | MapSelectPick::InitFailed => {
                        log::warn!("MapSelect chose no new map; {current} is replayed");
                        Some(current.to_owned())
                    }
                    MapSelectPick::Fault(reason) => {
                        log::error!("MapSelect cannot continue after {current}: {reason}");
                        None
                    }
                };
                match (scenario, sim.session().campaign_mission_counter()) {
                    (Some(scenario), Some(counter)) => {
                        let startup = Box::new(campaign.next_mission(
                            scenario,
                            counter + 1,
                            state.persistence.options_profile.difficulty,
                            sim.session().game_options().game_speed,
                            sim.save_campaign_carry_over(),
                            &mut crate::app::match_bootstrap::OrdinaryMatchSeedClock,
                        ));
                        if init_failed {
                            VictoryEnd::MapSelectFailed(startup)
                        } else {
                            VictoryEnd::NextMission {
                                startup,
                                rearm_initial_save: !skip,
                            }
                        }
                    }
                    _ => VictoryEnd::Shell,
                }
            }
        };
        Some(CampaignVictory {
            movies: queue,
            then,
        })
    }

    /// Run the next step of the victory tail; each movie returns here.
    pub(super) fn advance_campaign_victory(state: &mut AppState, mut victory: CampaignVictory) {
        if let Some(movie) = victory.movies.pop_front() {
            Self::start_fullscreen_movie(
                state,
                &movie,
                crate::app::frontend::fullscreen_movie::MovieReturn::CampaignVictory(Box::new(
                    victory,
                )),
            );
            return;
        }
        match victory.then {
            VictoryEnd::Shell => {
                Self::destroy_current_shell_dialog(state);
                state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
                Self::resume_shell_after_match(state);
            }
            VictoryEnd::Credits => Self::start_credits_roll(
                state,
                crate::app::frontend::credits_roll::CreditsReturn::MainMenu,
            ),
            VictoryEnd::MapSelectFailed(startup) => {
                // gamemd FUN_005ADD10 @0x005ADD10,5ADD8B..5ADDCF calls
                // ShellMessageBox5D3490 with MSG:UnableMapSelect/TXT_OK.
                // Victory685A56 ignores its result and proceeds after any
                // dismissal. Failed Init never reaches5AE126's ABCE08 write.
                let labels =
                    crate::ui::main_menu_dialogs::ShellMessageBoxLabels::map_select_failure(
                        &|key, fallback| Self::csf_label(state, key, fallback),
                    );
                Self::destroy_current_shell_dialog(state);
                state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
                Self::open_shell_message_box(
                    state,
                    crate::app::frontend::state::ShellMessageBox {
                        labels,
                        purpose:
                            crate::app::frontend::state::ShellMessageBoxPurpose::CampaignVictory(
                                Box::new(CampaignVictory {
                                    movies: VecDeque::new(),
                                    then: VictoryEnd::NextMission {
                                        startup,
                                        rearm_initial_save: false,
                                    },
                                }),
                            ),
                    },
                );
            }
            VictoryEnd::NextMission {
                startup,
                rearm_initial_save,
            } => {
                // Only successful MapSelect Init rearms ABCE08 at5AE126.
                // The failed-Init acknowledgement preserves that owner latch.
                if rearm_initial_save {
                    state.persistence.arm_initial_campaign_save();
                }
                Self::start_campaign_scenario(
                    state,
                    crate::app::loading::pump::LoadingRequest::campaign(*startup),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPSEL: &str = "\
[GDI]
1=ALL01
2=ALL02
3=ALL03
[ALL01]
Scenario=ALL01umd.MAP
MapVQ=GDIMAP01.VQA
2=ALL02
[ALL02]
Scenario=ALL02umd.MAP
MapVQ=GDIMAP01.VQA
5=ALL01
3=ALL03
[ALL03]
Scenario=ALL03umd.MAP
";

    fn pick(text: &str, side: &str, current: &str) -> MapSelectPick {
        map_select(Some(&IniFile::from_str(text)), Some(side), current, None)
    }

    #[test]
    fn the_stage_after_the_current_map_names_the_next_scenario() {
        assert_eq!(
            pick(MAPSEL, "GDI", "all01UMD.map"),
            MapSelectPick::Next {
                scenario: "ALL02umd.MAP".into(),
                stage: 1
            }
        );
        // The lowest-numbered choice wins, not the first written.
        assert_eq!(
            pick(MAPSEL, "GDI", "ALL02umd.MAP"),
            MapSelectPick::Next {
                scenario: "ALL03umd.MAP".into(),
                stage: 2
            }
        );
    }

    #[test]
    fn skip_map_select_uses_saved_stage_and_retained_destinations() {
        let previous = basic("NextScenario=all02UMD.map\nAltNextScenario=all03UMD.map");
        let mut basic = basic("SkipMapSelect=yes");
        basic.resolve_retained_destinations(&previous);
        let mapsel = IniFile::from_str(MAPSEL);
        // The saved stage wins over the current filename. Neither MapVQ nor
        // the normal first-choice rule gates the explicit destination search.
        assert_eq!(
            map_select(
                Some(&mapsel),
                Some("GDI"),
                "ALL03umd.MAP",
                Some((0, basic.next_scenario.as_deref().unwrap()))
            ),
            MapSelectPick::Next {
                scenario: "ALL02umd.MAP".into(),
                stage: 1
            }
        );
        assert_eq!(
            map_select(
                Some(&mapsel),
                Some("GDI"),
                "ALL01umd.MAP",
                Some((1, basic.alt_next_scenario.as_deref().unwrap()))
            ),
            MapSelectPick::Next {
                scenario: "ALL03umd.MAP".into(),
                stage: 2
            }
        );
        assert_eq!(
            map_select(
                Some(&mapsel),
                Some("GDI"),
                "ALL01umd.MAP",
                Some((u16::MAX, "ALL02umd.MAP"))
            ),
            MapSelectPick::InitFailed
        );
        assert_eq!(
            map_select(Some(&mapsel), Some("GDI"), "ALL01umd.MAP", Some((0, ""))),
            MapSelectPick::InitFailed
        );
        assert_eq!(
            map_select(
                Some(&mapsel),
                Some("GDI"),
                "ALL01umd.MAP",
                Some((0, "ALL03umd.MAP"))
            ),
            MapSelectPick::DestinationMissing
        );
    }

    #[test]
    fn next_scenario_readers_keep_native_capacity_and_fresh_defaults() {
        let parsed = basic(&format!(
            "NextScenario={}\nAltNextScenario=alt.MAP",
            "x".repeat(300)
        ));
        assert_eq!(parsed.next_scenario.as_ref().unwrap().len(), 259);
        assert_eq!(parsed.alt_next_scenario.as_deref(), Some("alt.MAP"));
        let defaults = basic("");
        assert_eq!(defaults.next_scenario, None);
        assert_eq!(defaults.alt_next_scenario, None);
    }

    #[test]
    fn failed_initialization_requires_a_notice_but_a_stage_without_vq_succeeds() {
        assert_eq!(
            pick(MAPSEL, "Nod", "ALL01umd.MAP"),
            MapSelectPick::InitFailed
        );
        assert_eq!(
            map_select(None, Some("GDI"), "ALL01umd.MAP", None),
            MapSelectPick::InitFailed
        );
        assert_eq!(pick(MAPSEL, "GDI", "OTHER.MAP"), MapSelectPick::InitFailed);
        let no_vq = MAPSEL.replace("MapVQ=GDIMAP01.VQA\n2=ALL02", "2=ALL02");
        assert_eq!(
            pick(&no_vq, "GDI", "ALL01umd.MAP"),
            MapSelectPick::KeepCurrent
        );
    }

    #[test]
    fn missing_choices_after_successful_init_are_native_faults() {
        let last = MAPSEL.replace(
            "Scenario=ALL03umd.MAP",
            "Scenario=ALL03umd.MAP\nMapVQ=X.VQA",
        );
        assert!(matches!(
            pick(&last, "GDI", "ALL03umd.MAP"),
            MapSelectPick::Fault(_)
        ));
    }

    fn basic(text: &str) -> crate::map::basic::BasicSection {
        crate::map::basic::parse_basic_section(&IniFile::from_str(&format!("[Basic]\n{text}")))
    }

    fn catalog(name: &str) -> Option<String> {
        (!name.eq_ignore_ascii_case("<none>")).then(|| name.to_owned())
    }

    #[test]
    fn win_movie_uses_the_fixed_catalog_and_saved_basic_reader() {
        let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
            return;
        };
        let rules = crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::empty(),
            None,
            art,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let movie = rules.movies().name(0).expect("retail movie catalog");
        let basic = basic(&format!(
            "Win={}\nSkipScore=yes",
            movie.to_ascii_lowercase()
        ));
        // SavedMapMetadata serializes this same BasicSection with save_codec;
        // its layout digest automatically changes when the field is added.
        let bytes = crate::util::save_codec::serialize(&basic).unwrap();
        let restored = crate::util::save_codec::deserialize(&bytes).unwrap();
        use crate::app::match_runtime::scenario_exit::ScenarioExitDestination;
        assert_eq!(
            win_movie(
                &ScenarioExitDestination::CampaignVictoryContinue,
                &restored,
                rules.movies()
            ),
            Some(movie),
            "SkipScore bypasses the dialog, not Win"
        );
        assert_eq!(
            win_movie(
                &ScenarioExitDestination::CampaignDefeat,
                &restored,
                rules.movies()
            ),
            None
        );
        assert_eq!(
            win_movie(
                &ScenarioExitDestination::CampaignVictoryContinue,
                &crate::map::basic::BasicSection::default(),
                rules.movies()
            ),
            None
        );
    }

    #[test]
    fn the_basic_flags_pick_the_victory_branch_after_the_movies() {
        let plan = |text: &str| victory_plan(&basic(text), catalog, Some("FINALE".into()));
        let (movies, branch) =
            plan("PostScore=A08_F00e\nPreMapSelect=PRE\nEndOfGame=yes\nOneTimeOnly=no");
        assert_eq!(Vec::from(movies), ["A08_F00e", "PRE", "FINALE"]);
        assert_eq!(branch, VictoryBranch::Credits);
        // OneTimeOnly is tested before EndOfGame (`0x006859B1`).
        let (movies, branch) = plan("PostScore=<none>\nEndOfGame=yes\nOneTimeOnly=yes");
        assert!(movies.is_empty());
        assert_eq!(branch, VictoryBranch::Shell);
        assert_eq!(plan("SkipMapSelect=yes").1, VictoryBranch::MapSelect);
        assert_eq!(
            plan("SkipMapSelect=no\nEndOfGame=no").1,
            VictoryBranch::MapSelect
        );
    }

    const ALLIED: [&str; 7] = [
        "ALL01umd", "all02umd", "all03umd", "all04dmd", "all05umd", "all06umd", "all07smd",
    ];
    const SOVIET: [&str; 7] = [
        "SOV01umd", "sov02smd", "sov03umd", "sov04dmd", "sov05umd", "sov06lmd", "sov07tmd",
    ];

    /// The retail campaign maps continue through MapSelect until each
    /// campaign's seventh mission, which ends it after its PostScore movie.
    #[test]
    fn retail_campaign_maps_continue_until_their_last_mission() {
        let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        for maps in [ALLIED, SOVIET] {
            for (index, name) in maps.iter().enumerate() {
                let loaded = crate::map::source::load_map_by_name_or_path_with_assets(
                    &root,
                    &format!("{name}.MAP"),
                    &assets,
                )
                .expect("retail campaign map");
                if index == 0 {
                    assert_eq!(loaded.map.basic.next_scenario.as_deref(), Some("GDI2A.map"));
                    assert_eq!(
                        loaded.map.basic.alt_next_scenario.as_deref(),
                        Some("GDI9C.MAP")
                    );
                    assert_eq!(loaded.map.basic.skip_map_select, Some(false));
                }
                assert_eq!(loaded.map.basic.win.as_deref(), Some("<none>"), "{name}");
                let (movies, branch) = victory_plan(&loaded.map.basic, catalog, None);
                if index + 1 < maps.len() {
                    assert!(movies.is_empty(), "{name}: {movies:?}");
                    assert_eq!(branch, VictoryBranch::MapSelect, "{name}");
                } else {
                    assert_eq!(movies.len(), 1, "{name}: {movies:?}");
                    assert_eq!(branch, VictoryBranch::Credits, "{name}");
                }
            }
        }
    }

    /// Continue must retain the next map's MISSIONMD loading art and copy,
    /// including MAPSELMD's mixed-case Scenario values.
    #[test]
    fn retail_mapselmd_chains_both_campaigns() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mapsel = crate::rules::retail_sources::select_ini(&assets, "MAPSELMD.INI")
            .expect("retail MAPSELMD.INI");
        let mission = crate::rules::retail_sources::select_ini(&assets, "MISSIONMD.INI")
            .expect("retail MISSIONMD.INI");
        let battle = crate::rules::retail_sources::select_ini(&assets, "BATTLEMD.INI")
            .expect("retail BATTLEMD.INI");
        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(&battle.ini, &crate::rules::movies::MovieRegistry::default());
        let mut clock = crate::app::match_bootstrap::OrdinaryMatchSeedClock;
        assert_eq!(
            map_select(Some(&mapsel.ini), Some("GDI"), "UNLISTED.MAP", None),
            MapSelectPick::InitFailed
        );
        for (side, name, maps) in [("GDI", "ALL1", ALLIED), ("Nod", "SOV1", SOVIET)] {
            let mut startup = crate::app::match_bootstrap::prepare_campaign_startup(
                &campaigns, name, 1, None, None, &mut clock,
            )
            .expect("retail campaign");
            for pair in maps.windows(2) {
                let current = startup.scenario().to_owned();
                match map_select(Some(&mapsel.ini), Some(side), &current, None) {
                    MapSelectPick::Next { scenario: next, .. } => {
                        assert!(
                            next.eq_ignore_ascii_case(&format!("{}.MAP", pair[1])),
                            "{current} -> {next}"
                        );
                        startup = startup.next_mission(
                            next,
                            startup.mission_counter() + 1,
                            1,
                            3,
                            crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
                            &mut clock,
                        );
                        let mut metadata =
                            crate::rules::campaign_loading::CampaignLoadingMetadata::default();
                        metadata.apply_ini(&mission.ini, startup.scenario());
                        for value in [
                            metadata.background_name_640(),
                            metadata.background_name_800(),
                            metadata.background_palette_name(),
                            metadata.load_message_key(),
                            metadata.load_briefing_key(),
                        ] {
                            assert!(
                                !value.is_empty(),
                                "{} lost its loading presentation after {current}",
                                startup.scenario()
                            );
                        }
                    }
                    other => panic!("{current}: {other:?}"),
                }
            }
        }
    }
}
