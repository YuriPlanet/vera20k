//! Authoritative scenario initialization after map objects are installed.
//!
//! This owns the `ScenarioClass::Post_Map_Init @ 0x00686890` tail that must
//! complete before tick 0: native tiberium queue construction, navigation
//! publication, the AI enemy-search delays and skirmish credit grant, crate RNG, the
//! final HouseClass alliance pass, the Special-house ally pass, Secret Lab
//! rewards, and the post-`Full_Init` setup tail of
//! `FUN_00684C30` (per-load particle-system ID, OreTwinkle Scenario
//! draws). The app submits immutable map/session inputs and consumes only the
//! receipt.

use crate::map::houses::HouseRoster;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::crates::CratePlacement;
use crate::sim::world::{FrameEffects, Simulation};

/// Immutable inputs for the one-shot post-map scenario command.
pub(crate) struct ScenarioPostMapInput<'a> {
    pub(crate) map_width: u16,
    pub(crate) map_height: u16,
    /// Map Basic.FillSilos after the fresh Scenario reset(false).
    pub(crate) fill_silos: bool,
    pub(crate) rules: &'a RuleSet,
    pub(crate) overlay_registry: &'a OverlayTypeRegistry,
    pub(crate) house_roster: &'a HouseRoster,
    pub(crate) skirmish_session: Option<&'a crate::sim::scenario_bootstrap::MatchLaunchDescriptor>,
    /// Every fresh campaign installation, including continuation and retry.
    pub(crate) campaign_map: Option<&'a crate::map::map_file::MapFile>,
}

/// Presentation/logging facts returned after authoritative initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioPostMapOutput {
    pub navigation_published: bool,
    pub crates: Option<CratePlacement>,
    pub(crate) ore_twinkle: crate::sim::ore_twinkle::OreTwinkleReceipt,
    /// Scenario View1..4: a campaign's HomeCell, otherwise the Cell of
    /// [`Self::opening_view_coord`].
    pub opening_view_cell: Option<(u16, u16)>,
    /// A non-campaign scenario's exact opening view coordinate, in leptons.
    /// A campaign opens on the centre of [`Self::opening_view_cell`].
    pub opening_view_coord: Option<[i32; 3]>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) skirmish_order: [Option<ScenarioPostMapStep>; 4],
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioPostMapStep {
    StartupCrates,
    AiOpeningCredits,
    LaunchAlliances,
    SpecialAllies,
}

#[cfg(test)]
thread_local! {
    /// A test's one read of the simulation before the Special-House pass
    /// ([`observe_before_special_allies`]).
    static BEFORE_SPECIAL_ALLIES: std::cell::RefCell<Option<Box<dyn FnOnce(&Simulation)>>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `load` and have `observe` read the simulation once, right before the
/// first Special-House pass inside it
/// ([`Simulation::ally_special_house_with_every_house`]), which walks every
/// starting unit's sight again. `None` when `load` reaches no such pass.
#[cfg(test)]
pub(crate) fn observe_before_special_allies<R: 'static, T>(
    observe: impl FnOnce(&Simulation) -> R + 'static,
    load: impl FnOnce() -> T,
) -> (T, Option<R>) {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
    let sink = std::rc::Rc::clone(&seen);
    BEFORE_SPECIAL_ALLIES.with_borrow_mut(|hook| {
        assert!(hook.is_none(), "nested Special-House observers");
        *hook = Some(Box::new(move |sim| *sink.borrow_mut() = Some(observe(sim))));
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_SPECIAL_ALLIES.with_borrow_mut(|hook| *hook = None);
        }
    }
    let reset = Reset;
    let result = load();
    drop(reset);
    (result, seen.take())
}

impl Simulation {
    /// Commit the post-map authority cone in native-observable order.
    ///
    /// This is a fresh-load-only command and is intentionally not idempotent:
    /// a second call would repeat the AI grant, crate RNG, and ore scheduler reset.
    /// Snapshot restore uses its narrower map-authority reconstruction path.
    pub(crate) fn finalize_scenario_post_map(
        &mut self,
        input: ScenarioPostMapInput<'_>,
    ) -> Result<ScenarioPostMapOutput, crate::sim::crates::RuntimeOverlayError> {
        // Runtime rebuilds use this same sim-owned publication seam. Crate
        // placement below reads the newly published canonical grid.
        let navigation_published = self.rebuild_dynamic_navigation(input.rules);

        #[cfg(any(test, feature = "test-support"))]
        let mut skirmish_order = [None; 4];
        let crates = if input.skirmish_session.is_some() {
            let player_count = crate::sim::crates::human_player_count(self);
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[0] = Some(ScenarioPostMapStep::StartupCrates);
            }
            let placement = crate::sim::crates::place_scenario_start_crates(
                self,
                input.rules,
                input.overlay_registry,
                player_count,
                FrameEffects::default(),
            )?;
            // Startup OverlayClass::Mark completes synchronously before native
            // proceeds to AI credits; the placement refreshed the bridge
            // records and navigation from the now-final CellClass state.
            Some(placement)
        } else {
            None
        };

        if self.session.game_mode_nonzero() {
            // ScenarioClass::Post_Map_Init006869EA..00686A2B starts every
            // nonhuman/nonpassive House's timer, before its credit grant.
            // The timer writes consume no RNG and do not affect credits or
            // alliances. Restore never enters this fresh-load owner.
            let frame = self.session.binary_frame() as i32;
            for house in self.houses.values_mut() {
                house.initialize_enemy_search_timer(frame, &input.rules.general);
            }
        }

        if let Some(descriptor) = input.skirmish_session {
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[1] = Some(ScenarioPostMapStep::AiOpeningCredits);
            }
            crate::sim::scenario_bootstrap::apply_skirmish_ai_opening_credits(
                &mut self.houses,
                input.rules,
            );
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[2] = Some(ScenarioPostMapStep::LaunchAlliances);
            }
            crate::sim::scenario_bootstrap::apply_skirmish_launch_alliances(
                self,
                descriptor.session(),
                input.rules,
            );
        } else {
            // A map House's Allies= is read only in GameMode0 (0x006877B9).
            if self.session.campaign_mission_counter().is_none()
                && !self.session.game_mode_nonzero()
            {
                self.install_house_alliances(
                    crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(
                        input.house_roster.alliance_map(),
                    ),
                    input.rules,
                );
            }
        }

        // Post_Map_Init runs for non-campaign scenarios only. Its camera step
        // (`0x006869A6`) follows the starting units and crates; the AI credits
        // and alliances after it change no owner or Limbo state it reads. The
        // base query is PlayerPtr's, so a network game stamps no shared Dummy.
        let player_view = if self.session.game_mode_nonzero() {
            let network = self.session.game_mode().is_network();
            let cells = self.resolved_terrain.as_ref().map(|terrain| {
                crate::map::resolved_terrain::NativeCellQuery::for_local_input(terrain, network)
            });
            self.opening_view_on_player_technos(cells.as_ref())
        } else {
            None
        };

        // Post_Map_Init's Special-House pass (lookup `0x006869AB..0x006869BC`,
        // alliances `0x00686A9E..0x00686AC1`), in the same GameMode>0 segment
        // as the AI credits and before the Secret Lab call (`0x00686AFE`) below.
        if self.session.game_mode_nonzero() {
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[3] = Some(ScenarioPostMapStep::SpecialAllies);
            }
            #[cfg(test)]
            {
                let observe = BEFORE_SPECIAL_ALLIES.with_borrow_mut(Option::take);
                if let Some(observe) = observe {
                    observe(self);
                }
            }
            self.ally_special_house_with_every_house(&input);
        }

        // Post_Map_Init686AFE calls68C050 after launch alliances. Authored
        // Full_Init687BC8..687BF1 skips that pass in GameMode0 (campaign).
        // Generated/.SED maps have a separate684990 call, but the admitted
        // generated family is noncampaign. Restore never reruns this owner.
        if self.session.game_mode_nonzero() {
            Self::assign_secret_lab_production(
                &mut self.interner,
                &mut self.scenario_rng,
                &mut self.substrate,
                input.rules,
            );
        }

        // Full_Init687C16, after Post_Map_Init and the deferred-finalization
        // drain, in every mode: Ares ValidateThings100677B0's CanMakeStuffUp
        // draws. Restore never enters this fresh-only owner.
        #[cfg(feature = "ares")]
        self.ares_make_stuff_up_rules(input.rules);

        // Full_Init687C2B..687C44 clears the placement scope around
        // FUN_00452D40: every laser fence post raises its sections.
        self.laser_fence_link_scenario_posts(crate::sim::world::UninitContext::new(
            input.rules,
            Some(input.overlay_registry),
        ));

        // ReadScenario684ACE → FillInData684F02..684F7F, before restamp
        // and the particle/OreTwinkle tail. Every nonnull House participates;
        // restore never enters this fresh-only owner or repeats its credits.
        if input.fill_silos {
            for owner in self.houses.order().to_vec() {
                let income = crate::sim::house_state::income_mult_for_owner(
                    &self.houses,
                    &self.interner,
                    input.rules,
                    self.interner.resolve(owner),
                );
                let Some(house) = self.houses.get_mut(&owner) else {
                    continue;
                };
                // Native dereferences Tiberium[0], even if its Value is0.
                // Do not substitute a name/default or silently skip an invalid
                // empty registry on an enabled nonempty-House scenario.
                let value = input
                    .rules
                    .tiberium_types
                    .types()
                    .first()
                    .expect("native FillSilos requires Tiberium registry slot0")
                    .value;
                let capacity = house.tracking().storage_capacity();
                house
                    .economy_mut()
                    .fill_silos_fresh_empty_store(value, capacity, income);
            }
        }

        // Fill_In_Data684F81..684F89: Map4ADCD0(1,0) visits mobile sources
        // before the later first-House full pass. This does not consume +24B.
        self.initialize_scenario_sight(crate::sim::world::UninitContext::new(
            input.rules,
            Some(input.overlay_registry),
        ));

        // Original684C30 performs586BF0 after the zone rebuilds and before its
        // particle/twinkle tail. This command is fresh-load-only: restore writes
        // saved real flags, and ordinary navigation rebuilds never rerun it.
        if let (Some(terrain), Some(bridges)) =
            (self.resolved_terrain.as_mut(), self.bridge_state.as_ref())
        {
            crate::sim::bridge_state::gap_restamp::restamp_inactive_high_records(
                terrain,
                bridges.endpoint_records(),
                |_, index, flags| {
                    if let Some(index) = index {
                        self.real_cell_bridge_flags_0x1180
                            .set_allocated_cell(index, flags);
                    }
                },
            );
        }

        // Native `FUN_00684C30` runs after `Full_Init` (and therefore after the
        // Post_Map_Init credit/crate/alliance work above): the GasCloudSys
        // particle-system ID, then the OreTwinkle Scenario draws.
        let ore_twinkle = self.run_post_load_ore_twinkle_pass(
            input.rules,
            input.overlay_registry,
            input.map_width,
            input.map_height,
        )?;

        // gamemd Fill_In_Data684C9A..684D43 reads Evade+0 (8A38E0), the
        // incoming global0 captured by CampaignCarryOver::Save4C6180.
        // NewCampaign52DF05->Clear4C6140 clears it. A continuation/retry
        // uses that retained record before Restore4C6210 writes the new
        // Scenario's globals; its current live global0 is not the selector.
        // Basic Read68A601..68A656 reads both indexes with current defaults;
        // fresh SetDefaults683622/683628 initializes both to699.
        let opening_view_cell = input.campaign_map.and_then(|map| {
            assert!(
                input.skirmish_session.is_none() && !self.session.game_mode_nonzero(),
                "campaign opening view requires the campaign family"
            );
            let incoming = self
                .session
                .campaign_restart_carry_over()
                .expect("campaign record");
            let waypoint = if incoming.globals()[0] {
                map.basic.alt_home_cell
            } else {
                map.basic.home_cell
            };
            u32::try_from(waypoint.unwrap_or(699)).ok().map(|index| {
                map.waypoints
                    .get(&index)
                    .map_or((0, 0), |waypoint| (waypoint.rx, waypoint.ry))
            })
        });

        // ReadScenario684B45..684B53, after 684C30 and before the initial
        // Techno capture: the WOL clan alliances.
        self.apply_squad_alliances(input.rules, Some(input.overlay_registry));

        // ReadScenario684ACE completes684C30 before684C05..684C1B captures
        // each House's initial Technos. Saved-world restore never enters this
        // fresh-only owner and therefore retains, rather than recaptures, them.
        Self::retain_initial_house_technos(&self.substrate, &mut self.houses);

        Ok(ScenarioPostMapOutput {
            navigation_published,
            crates,
            ore_twinkle,
            opening_view_cell: opening_view_cell.or(player_view.map(|(_, cell)| cell)),
            opening_view_coord: player_view.map(|(coord, _)| coord),
            #[cfg(any(test, feature = "test-support"))]
            skirmish_order,
        })
    }

    /// `Post_Map_Init`'s Special-House pass. Before its per-House loop
    /// (`0x006869D2..0x00686ACE`), native resolves the literal at `0x00817318`
    /// ("Special") as a Country through `HouseTypeClass::FindIndexOfName @
    /// 0x005117D0` and takes that Country's first House through
    /// `HouseClass::Find_By_Country_Index @ 0x00502D30`
    /// (`0x006869AB..0x006869BC`). The loop's tail (`0x00686A9E..0x00686AC1`)
    /// then `HouseClass::Make_Ally @ 0x004F9B70`s it with every other
    /// registered House, house→Special then Special→house, each with announce
    /// 0. The same loop grants the AI opening credits first; neither pass reads
    /// what the other writes, so VERA runs them one after the other.
    ///
    /// Native uses the House unchecked: the fixed CreateHouses tail
    /// (`crate::sim::scenario_bootstrap::populate_special_houses`) supplies it
    /// in every nonzero game mode. The early return only covers rules without
    /// a Special Country.
    ///
    /// The whole pass runs inside the object-placement scope because native
    /// still holds the ScenarioInit counter `[0xA8E7AC]` here: `CanAlly @
    /// 0x00501540` returns before its defeated and remaining-enemy gates
    /// (`0x005015A1..0x005015AE`) and `Make_Ally` skips its live Paranoid,
    /// target and announcement tail (`0x004F9C1F..0x004F9C28`). Outside the
    /// scope a single-live-House Special would be refused by the
    /// leaves-no-enemy gate, which no native start is.
    ///
    /// That arm keeps `Make_Ally`'s sight brackets (`0x004F9B9A`, and
    /// `0x004F9C2A..0x004F9C31` jumps to `0x004F9F46`). The starting units
    /// already stand (`Generate_Starting_Units @ 0x005D6D80`, called at
    /// `0x00686937`), so each admitted call releases (`Map 0x004ADEE0`,
    /// ReReveal `0x0070B1D0`) and re-admits (`Map 0x004ADCD0`, UpdateReveal
    /// `0x0070AF50`) the sight of every discovered Techno of the player on
    /// the player's map. `MapCell 0x004A9CA0` recomputes both edge frames on
    /// every call (`0x004A9D09`, `0x004A9D41`), so a starting unit's cells
    /// leave the pass with the frames its whole footprint gives, not the
    /// ones its Unlimbo's own walk stored.
    fn ally_special_house_with_every_house(&mut self, input: &ScenarioPostMapInput<'_>) {
        let Some(special) = input
            .rules
            .country_index_of_name("Special")
            .and_then(|country| self.house_by_country_index(country, input.rules))
        else {
            return;
        };
        let order = self.houses.order().to_vec();
        self.with_object_placement_scope(|sim| {
            for owner in order {
                if owner == special {
                    continue;
                }
                sim.make_ally(
                    owner,
                    special,
                    input.rules,
                    Some(input.overlay_registry),
                    FrameEffects::default(),
                    false,
                );
                sim.make_ally(
                    special,
                    owner,
                    input.rules,
                    Some(input.overlay_registry),
                    FrameEffects::default(),
                    false,
                );
            }
        });
    }

    /// Rebuild the derived bridge record vector after a crate `OverlayClass::Mark`
    /// batch. Native Mark mutates live CellClass state synchronously; Rust builds
    /// `BridgeRuntimeState` earlier in the load funnel, so both the startup batch
    /// and the per-tick regeneration rung refresh it from the now-final CellClass
    /// state.
    pub(crate) fn refresh_bridge_runtime_after_crate_mark(&mut self) -> bool {
        let Some((destroyable, bridge_strength)) = self
            .bridge_state
            .as_ref()
            .map(|state| (state.is_destroyable(), state.bridge_strength()))
        else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let (Some(bounds), Some(size_height)) =
            (self.playfield_bounds(), self.playfield_size_height())
        else {
            return false;
        };
        let bridges =
            crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
                terrain,
                destroyable,
                bridge_strength,
                (bounds.base, size_height),
            );
        self.install_bridge_runtime(bridges);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};

    use crate::rules::ini_parser::IniFile;

    use crate::sim::overlay_grid::OverlayGrid;

    const MAP_SIZE: u16 = 8;

    fn post_map_rules_and_overlays() -> (RuleSet, OverlayTypeRegistry) {
        let ini = IniFile::from_str(
            "[General]\n\
             TiberiumGrows=yes\n\
             TiberiumSpreads=yes\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Tiberiums]\n\
             0=Riparius\n\
             [Riparius]\n\
             Image=1\n\
             Growth=2200\n\
             GrowthPercentage=.06\n\
             Spread=2200\n\
             SpreadPercentage=.06\n\
             [OverlayTypes]\n\
             0=TIBCELL\n\
             1=WOOD\n\
             2=WATER\n\
             [TIBCELL]\n\
             Tiberium=yes\n\
             [WOOD]\n\
             Crate=yes\n\
             [WATER]\n\
             Crate=yes\n\
             [CrateRules]\n\
             CrateImg=WOOD\n\
             WoodCrateImg=WOOD\n\
             WaterCrateImg=WATER\n\
             CrateMinimum=1\n\
             CrateMaximum=1\n",
        );
        let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
        (
            RuleSet::from_ini(&ini).expect("post-map rules"),
            OverlayTypeRegistry::from_ini(&ini, None),
        )
    }

    fn flat_terrain() -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(MAP_SIZE, MAP_SIZE, |rx, ry| ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
        })
    }

    fn twinkle_rules_and_overlays() -> (RuleSet, OverlayTypeRegistry) {
        let ini = IniFile::from_str(
            "[General]\n\
             OreTwinkle=TWNK1\n\
             TiberiumGrows=no\n\
             TiberiumSpreads=no\n\
             [AudioVisual]\n\
             OreTwinkleChance=2\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Tiberiums]\n\
             0=Riparius\n\
             [Riparius]\n\
             Image=1\n\
             Value=25\n\
             [OverlayTypes]\n\
             0=TIBCELL\n\
             [TIBCELL]\n\
             Tiberium=yes\n",
        );
        let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
        let mut rules = RuleSet::from_ini(&ini).expect("twinkle rules");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[TWNK1]\nLoopCount=-1\nRandomLoopDelay=120,300\nDetailLevel=2\nHideIfNoOre=true\nRate=450\n",
        ));
        art.bind_anim_frame_count_for_test("TWNK1", 8);
        rules.replace_art_registry_for_test(art);
        (rules, OverlayTypeRegistry::from_ini(&ini, None))
    }

    fn generic_post_map_input<'a>(
        rules: &'a RuleSet,
        overlays: &'a OverlayTypeRegistry,
        house_roster: &'a HouseRoster,
    ) -> ScenarioPostMapInput<'a> {
        ScenarioPostMapInput {
            map_width: MAP_SIZE,
            map_height: MAP_SIZE,
            fill_silos: false,
            rules,
            overlay_registry: overlays,
            house_roster,
            skirmish_session: None,
            campaign_map: None,
        }
    }

    #[test]
    fn ai_hate_delay_initialization_respects_house_gates_and_campaigns() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};
        use crate::sim::scenario_session::ScenarioGameMode;
        use crate::sim::timer::CdTimer;
        let ini = IniFile::from_str("[General]\nAIHateDelays=-7,0,70\n");
        let rules = RuleSet::from_ini(&ini).unwrap();
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let roster = HouseRoster::default();
        let mut sim = Simulation::new();
        sim.session.set_game_mode(ScenarioGameMode::Skirmish);
        let hard = sim.interner.intern("Hard");
        let easy = sim.interner.intern("Easy");
        let human = sim.interner.intern("Human");
        let passive = sim.interner.intern("Passive");
        for owner in [hard, easy, human, passive] {
            sim.houses
                .register(HouseState::new(owner, 0, None, owner == human, 0, 10));
        }
        sim.houses
            .get_mut(&hard)
            .unwrap()
            .set_difficulty_for_test(HouseDifficulty::Hard);
        sim.houses
            .get_mut(&easy)
            .unwrap()
            .set_difficulty_for_test(HouseDifficulty::Easy);
        sim.houses
            .get_mut(&easy)
            .unwrap()
            .set_player_control_for_test(true);
        sim.houses
            .get_mut(&passive)
            .unwrap()
            .set_multiplay_passive_for_test(true);
        let mut campaign = Simulation::new();
        let campaign_owner = campaign.interner.intern("Campaign");
        campaign
            .houses
            .register(HouseState::new(campaign_owner, 0, None, false, 0, 10));
        campaign.session.set_game_mode(ScenarioGameMode::Campaign);
        sim.session.set_binary_frame_for_test(17);
        sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
            .unwrap();
        assert_eq!(
            *sim.houses[&hard].enemy_search_timer(),
            CdTimer::started(17, -7)
        );
        assert_eq!(
            *sim.houses[&easy].enemy_search_timer(),
            CdTimer::started(17, 70)
        );
        assert_eq!(
            *sim.houses[&human].enemy_search_timer(),
            CdTimer::started(0, 0)
        );
        assert_eq!(
            *sim.houses[&passive].enemy_search_timer(),
            CdTimer::started(0, 0)
        );
        campaign
            .finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
            .unwrap();
        assert_eq!(
            *campaign.houses[&campaign_owner].enemy_search_timer(),
            CdTimer::started(0, 0)
        );
        let delayed_hash = sim.state_hash();
        sim.houses
            .get_mut(&easy)
            .unwrap()
            .initialize_enemy_search_timer(18, &rules.general);
        assert_ne!(sim.state_hash(), delayed_hash);
    }

    /// The base post-map fixture plus the diplomacy rules fragment the
    /// Special-House pass needs: "Special" sits on the `Mutant` side — not
    /// `Civilian`, whose side CanAlly refuses in a nonzero game mode
    /// (`0x0050157C..0x0050159E`) — and is MultiplayPassive.
    fn special_ally_rules_and_overlays() -> (RuleSet, OverlayTypeRegistry) {
        let ini = IniFile::from_str(
            "[General]\n\
             TiberiumGrows=yes\n\
             TiberiumSpreads=yes\n\
             [MultiplayerDialogSettings]\n\
             AlliesAllowed=yes\n\
             [Sides]\n\
             GDI=Americans\n\
             Nod=Russians\n\
             Civilian=Neutral\n\
             Mutant=Special\n\
             [Countries]\n\
             0=Americans\n\
             1=Russians\n\
             2=Neutral\n\
             3=Special\n\
             [Neutral]\n\
             MultiplayPassive=yes\n\
             [Special]\n\
             MultiplayPassive=yes\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Tiberiums]\n\
             0=Riparius\n\
             [Riparius]\n\
             Image=1\n\
             [OverlayTypes]\n\
             0=TIBCELL\n\
             [TIBCELL]\n\
             Tiberium=yes\n",
        );
        let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
        (
            RuleSet::from_ini(&ini).expect("special-ally rules"),
            OverlayTypeRegistry::from_ini(&ini, None),
        )
    }

    /// Registers `(name, country, human)` houses, resolving each
    /// MultiplayPassive fact from its country rules as the roster
    /// constructor does.
    fn register_special_pass_houses(
        sim: &mut Simulation,
        rules: &RuleSet,
        houses: &[(&str, &str, bool)],
    ) {
        for &(name, country, human) in houses {
            let id = sim.interner.intern(name);
            let side = rules.country_side_index(country).expect("country side").0;
            let country_id = sim.interner.intern(country);
            let mut house =
                crate::sim::house_state::HouseState::new(id, side, Some(country_id), human, 0, 10);
            house.set_multiplay_passive_for_test(
                crate::sim::house_state::resolve_multiplay_passive(rules, Some(country)),
            );
            sim.houses.register(house);
        }
    }

    #[test]
    fn campaign_post_map_skips_random_secret_lab_assignment_on_retail_missions() {
        use crate::map::map_file::MapFile;
        use crate::rules::process_owner::NativeScenarioRulesPrefix;
        use crate::rules::retail_ini_fixture;
        use crate::sim::house_state::HouseState;
        use crate::sim::rng::trace_draws;
        use crate::sim::scenario_session::ScenarioGameMode;

        let Some((_, assets)) = retail_ini_fixture::retail_assets() else {
            return;
        };
        for (filename, reward) in [("SOV05UMD.MAP", "DESO"), ("SOV07TMD.MAP", "GTGCAN")] {
            let map = MapFile::from_bytes(assets.get_ref(filename).expect("retail campaign map"))
                .expect("retail campaign map parses");
            let prefix_name = filename.replace(".MAP", ".INI");
            let prefix = assets.resolve_ref(&prefix_name).map(|_| {
                crate::rules::retail_sources::select_ini(&assets, &prefix_name)
                    .expect("present campaign prefix parses")
                    .ini
            });
            let mut process = retail_ini_fixture::retail_rules_owner(&assets);
            let (rules, processed, art, _) = process
                .load_scenario(
                    NativeScenarioRulesPrefix::Campaign(prefix.as_ref()),
                    &map.ini,
                )
                .expect("production campaign rules reader")
                .into_parts();
            let overlays = OverlayTypeRegistry::from_ini(&processed, Some(&art));
            let lab_type = rules.object("CASLAB").expect("stock Secret Lab");
            assert!(lab_type.secret_lab().enabled());
            assert_eq!(
                lab_type
                    .secret_lab()
                    .production_override()
                    .map(|(_, name)| name),
                Some(reward),
                "{filename}: native map override remains authoritative"
            );
            assert_eq!(
                rules
                    .general
                    .secret_lab_choices()
                    .iter()
                    .map(Vec::len)
                    .sum::<usize>(),
                8
            );
            let labs: Vec<_> = map
                .entities
                .iter()
                .filter(|entity| entity.type_id == "CASLAB")
                .collect();
            assert_eq!(labs.len(), 1, "{filename}: placed stock Secret Lab");
            let mut sim = Simulation::with_seed(1);
            sim.install_session_mode(ScenarioGameMode::Campaign);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let owner = sim.interner.intern(&labs[0].owner);
            sim.houses
                .register(HouseState::new(owner, 0, None, false, 0, 10));
            // Only this registered building is needed to exercise the actual
            // post-map dispatcher. Terrain, combat and prior mission play are
            // outside this bounded startup regression.
            let lab = sim
                .spawn_object("CASLAB", &labs[0].owner, 10, 10, 0, &rules)
                .unwrap();
            let before = sim.rng_state();
            let roster = HouseRoster::default();
            let (_, draws) = trace_draws(|| {
                sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
                    .unwrap()
            });
            assert!(
                draws.is_empty(),
                "{filename}: campaign skips Post_Map_Init's random reward pass"
            );
            assert_eq!(sim.rng_state(), before, "{filename}: all three RNG streams");
            assert_eq!(
                sim.substrate
                    .entities
                    .get(lab)
                    .unwrap()
                    .secret_lab_fallback(),
                None
            );
        }
    }

    #[test]
    fn campaign_opening_view_uses_the_saved_incoming_global_zero() {
        let Some(bytes) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV07TMD.MAP")
        else {
            return;
        };
        let map = crate::map::map_file::MapFile::from_bytes(&bytes).unwrap();
        assert_eq!(
            (map.basic.home_cell, map.basic.alt_home_cell),
            (Some(17), Some(699))
        );
        let (rules, overlays) = post_map_rules_and_overlays();
        let roster = HouseRoster::default();
        // FillInData684C9A reads Evade+0, captured from the previous mission
        // by4C6180. It does not read the new Scenario's live global0.
        for (incoming, current, expected) in [(true, false, (107, 109)), (false, true, (149, 148))]
        {
            let mut previous = Simulation::new();
            Simulation::set_trigger_variable(
                &previous.session,
                &mut previous.scenario_rng,
                &mut previous.trigger_runtime,
                true,
                0,
                incoming,
            );
            let record = previous.save_campaign_carry_over();
            let mut sim = Simulation::from_descriptor(&Default::default());
            sim.session
                .initialize_campaign_startup(1, 7, None, 1, record)
                .unwrap();
            Simulation::set_trigger_variable(
                &sim.session,
                &mut sim.scenario_rng,
                &mut sim.trigger_runtime,
                true,
                0,
                current,
            );
            let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
            let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
                .unwrap()
                .sim;
            let before = loaded.rng_state();
            let mut input = generic_post_map_input(&rules, &overlays, &roster);
            input.campaign_map = Some(&map);
            let output = loaded.finalize_scenario_post_map(input).unwrap();
            assert_eq!(
                output.opening_view_cell,
                Some(expected),
                "incoming={incoming}, current={current}"
            );
            assert_eq!(
                output.opening_view_coord, None,
                "campaigns open on HomeCell"
            );
            assert_eq!(loaded.rng_state(), before);
        }
    }

    #[test]
    fn campaign_opening_view_matches_original_new_game_basic_start_rows() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start.json",
        ))
        .unwrap();
        let (rules, overlays) = post_map_rules_and_overlays();
        let roster = HouseRoster::default();
        let mut compared = 0;
        for row in native["basic_start"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["alternate"].as_i64() == Some(0))
        {
            let mut map = crate::map::map_file::MapFile::from_bytes(
                b"[Map]\nSize=0,0,4,4\n[IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n",
            )
            .unwrap();
            let ini = IniFile::from_sections_for_test(["Basic", "Waypoints"].map(|name| {
                let mut section = crate::rules::ini_parser::IniSection::new(name.to_string());
                for (key, value) in row["sections"][name].as_object().unwrap() {
                    section.set(key, value.as_str().unwrap());
                }
                section
            }));
            map.basic = crate::map::basic::parse_basic_section(&ini);
            map.waypoints = crate::map::waypoints::parse_waypoints(&ini);
            // Ordinary NewCampaign cleared Evade; a different AltHome must
            // not alter the selected HomeCell in this shared post-load owner.
            map.basic.alt_home_cell = Some(-1);
            let mut sim = Simulation::from_descriptor(&Default::default());
            sim.session
                .initialize_campaign_startup(
                    1,
                    1,
                    None,
                    0,
                    crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
                )
                .unwrap();
            let before = sim.rng_state();
            let mut input = generic_post_map_input(&rules, &overlays, &roster);
            input.campaign_map = Some(&map);
            let output = sim.finalize_scenario_post_map(input).unwrap();
            assert_eq!(
                output.opening_view_cell,
                Some((
                    row["cell"][0].as_u64().unwrap() as u16,
                    row["cell"][1].as_u64().unwrap() as u16
                )),
                "{}",
                row["filename"]
            );
            assert_eq!(sim.rng_state(), before);
            compared += 1;
        }
        assert_eq!(
            compared, 2,
            "alternate1 remains continuation-bound evidence"
        );
    }

    #[test]
    fn campaign_post_map_keeps_the_once_installed_house_graph_and_constructor_cursors() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start_houses.json",
        ))
        .unwrap();
        let row = native["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "one_way_allies")
            .unwrap();
        assert_eq!(row["scenario_init"], 2);
        let ini = IniFile::from_sections_for_test(row["sections"].as_object().unwrap().iter().map(
            |(name, values)| {
                let mut section = crate::rules::ini_parser::IniSection::new(name.clone());
                for (key, value) in values.as_object().unwrap() {
                    section.set(key, value.as_str().unwrap());
                }
                section
            },
        ));
        let (rules, overlays) = post_map_rules_and_overlays();
        let roster = crate::map::houses::parse_house_roster(&ini, &[], Some(&rules));
        let mut sim = Simulation::new();
        sim.session
            .initialize_campaign_startup(
                1,
                1,
                None,
                0,
                crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            )
            .unwrap();
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut sim,
            &roster,
            &rules,
            Some(&ini),
        );
        assert!(crate::map::houses::is_allied_with(
            sim.house_alliances.as_map(),
            "A",
            "B"
        ));
        assert!(!crate::map::houses::is_allied_with(
            sim.house_alliances.as_map(),
            "B",
            "A"
        ));
        let alliances = sim.house_alliances.as_map().clone();
        let order = sim.houses.order().to_vec();
        let house_ids: Vec<_> = order
            .iter()
            .map(|owner| sim.houses[owner].native_unique_id())
            .collect();
        let rng = sim.rng_state();
        let ids = sim.native_unique_ids.as_ref().unwrap().current_raw();
        let absent_late_roster = HouseRoster::default();
        let output = sim
            .finalize_scenario_post_map(generic_post_map_input(
                &rules,
                &overlays,
                &absent_late_roster,
            ))
            .unwrap();
        assert_eq!(sim.house_alliances.as_map(), &alliances);
        assert_eq!(sim.houses.order(), order);
        assert_eq!(
            order
                .iter()
                .map(|owner| sim.houses[owner].native_unique_id())
                .collect::<Vec<_>>(),
            house_ids
        );
        assert_eq!(sim.rng_state(), rng);
        // Original68503B constructs GasCloudSys even without OreTwinkle.
        // It spends one ID after the House pass, never another House/Super ID.
        assert_eq!(
            output.ore_twinkle,
            crate::sim::ore_twinkle::OreTwinkleReceipt {
                particle_system_id_consumed: true,
                ..Default::default()
            }
        );
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            ids.wrapping_add(1)
        );
    }

    /// `FUN_00684C30 @ 0x0068504D..0x006850F3`: one `RandomRanged(0, N-1)`
    /// Scenario draw per resource cell in `CellIterator` order, one native ID
    /// for the GasCloudSys particle system first, and one `AnimClass` per zero
    /// roll at the cell centre with draw flags 0x600.
    #[test]
    fn post_load_ore_twinkle_pass_rolls_each_resource_cell_in_native_order() {
        use crate::sim::native_identity::build_noncampaign_fresh_id_prefix;
        use crate::sim::ore_twinkle::OreTwinkleReceipt;
        use crate::sim::rng::SimRng;

        let (rules, overlays) = twinkle_rules_and_overlays();
        let seed = 0x0037_7EA5;
        let mut sim = Simulation::with_seed(seed);
        sim.session.set_map_width_for_test(MAP_SIZE);
        sim.session.set_map_height_for_test(MAP_SIZE);
        sim.resolved_terrain = Some(flat_terrain());
        let mut overlay_grid = OverlayGrid::new(MAP_SIZE, MAP_SIZE);
        // The (8,8) fixed-grid diamond admits only x+y > 8 here; the cells are
        // deliberately listed out of native order.
        let ore_cells: [(u16, u16); 4] = [(7, 7), (3, 7), (7, 2), (5, 4)];
        for (rx, ry) in ore_cells {
            overlay_grid.place_overlay(rx, ry, 0, 3);
        }
        // Outside the native diamond: never visited, never rolled.
        overlay_grid.place_overlay(1, 1, 0, 3);
        sim.overlay_grid = Some(overlay_grid);
        sim.native_unique_ids =
            Some(build_noncampaign_fresh_id_prefix(0, 0, 0, 0, 0, 0, 1, 1).into_cursor());
        let native_before = sim
            .native_unique_ids
            .as_ref()
            .expect("native cursor")
            .current_raw();
        let main_before = sim.main_rng.state();
        let roster = HouseRoster::default();

        let output = sim
            .finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
            .unwrap();

        let native_order: Vec<(u16, u16)> =
            crate::map::authored_overlay::NativeOverlayMapShape::new(8, 8)
                .recalc_cells()
                .into_iter()
                .filter_map(|(x, y)| {
                    let cell = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
                    ore_cells.contains(&cell).then_some(cell)
                })
                .collect();
        assert_eq!(native_order, vec![(5, 4), (7, 2), (3, 7), (7, 7)]);
        let mut expected = SimRng::new(seed);
        let mut expected_spawns = Vec::new();
        for cell in &native_order {
            if expected.next_range_u32_inclusive(0, 1) == 0 {
                expected_spawns.push(*cell);
            }
        }
        assert!(
            !expected_spawns.is_empty() && expected_spawns.len() < native_order.len(),
            "seed must exercise both roll outcomes: {expected_spawns:?}"
        );
        assert_eq!(sim.scenario_rng.state(), expected.state());
        assert_eq!(
            sim.main_rng.state(),
            main_before,
            "Main RNG is absent from this corridor"
        );
        assert_eq!(
            output.ore_twinkle,
            OreTwinkleReceipt {
                resource_cells_rolled: 4,
                spawned: expected_spawns.len() as u32,
                spawn_failures: 0,
                particle_system_id_consumed: true,
            }
        );
        let anims: Vec<((i32, i32, i32), i32, u32)> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| {
                (
                    (
                        anim.world_coord().x,
                        anim.world_coord().y,
                        anim.world_coord().z,
                    ),
                    anim.native_unique_id(),
                    anim.draw_flags(),
                )
            })
            .collect();
        let expected_anims: Vec<((i32, i32, i32), i32, u32)> = expected_spawns
            .iter()
            .enumerate()
            .map(|(index, (rx, ry))| {
                (
                    (i32::from(*rx) * 256 + 128, i32::from(*ry) * 256 + 128, 0),
                    // One ID for the particle system, then one per twinkle.
                    native_before.wrapping_add(2 + index as u32) as i32,
                    0x600,
                )
            })
            .collect();
        assert_eq!(anims, expected_anims);
    }

    /// Post_Map_Init's Special-House loop `0x00686A9E..0x00686AC1`: in a
    /// nonzero game mode the "Special" house leaves the map mutually allied
    /// with every other House in both directions, no other pairing gains an
    /// edge, and the pass records itself after the launch alliances.
    #[test]
    fn post_map_allies_the_special_house_with_every_house_both_ways() {
        let (rules, overlays) = special_ally_rules_and_overlays();
        let mut sim = Simulation::with_seed(7);
        sim.install_session_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        register_special_pass_houses(
            &mut sim,
            &rules,
            &[
                ("A", "Americans", false),
                ("B", "Americans", false),
                ("Special", "Special", false),
            ],
        );
        let roster = HouseRoster::default();
        let output = sim
            .finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
            .unwrap();
        let alliances = sim.house_alliances.as_map();
        for other in ["A", "B"] {
            assert!(
                crate::map::houses::are_houses_mutually_allied(alliances, "Special", other),
                "Special must leave mutually allied with {other}"
            );
        }
        assert!(
            !crate::map::houses::is_allied_with(alliances, "A", "B")
                && !crate::map::houses::is_allied_with(alliances, "B", "A"),
            "the Special loop must not ally the other Houses with each other"
        );
        assert_eq!(
            output.skirmish_order,
            [None, None, None, Some(ScenarioPostMapStep::SpecialAllies)],
            "the Special pass records itself after the launch alliances"
        );
    }

    /// The pass's House is the first one of the Country named "Special"
    /// (`FindIndexOfName @ 0x005117D0`, then `Find_By_Country_Index @
    /// 0x00502D30`), not a House that is merely named "Special".
    #[test]
    fn post_map_finds_the_special_house_by_country() {
        let (rules, overlays) = special_ally_rules_and_overlays();
        let mut sim = Simulation::with_seed(7);
        sim.install_session_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        register_special_pass_houses(
            &mut sim,
            &rules,
            &[
                ("Special", "Americans", false),
                ("Lab", "Special", false),
                ("A", "Americans", false),
            ],
        );
        let roster = HouseRoster::default();
        sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
            .unwrap();
        let alliances = sim.house_alliances.as_map();
        for other in ["Special", "A"] {
            assert!(
                crate::map::houses::are_houses_mutually_allied(alliances, "Lab", other),
                "the Special Country's House must leave allied with {other}"
            );
        }
        assert!(
            !crate::map::houses::is_allied_with(alliances, "Special", "A"),
            "a House only named Special is not the pass's target"
        );
    }

    /// The pass runs inside native's ScenarioInit bracket `[0xA8E7AC]`:
    /// without it `CanAlly @ 0x00501540` refuses both directions at its
    /// remaining-enemy gate (`0x005015C2..0x00501638`) — the single live
    /// House plus the passive Special, which counts as a friend in the
    /// count, already cover every live House — but inside the placement
    /// scope the pair still leaves mutually allied.
    #[test]
    fn post_map_allies_special_despite_a_single_live_house_leaving_no_enemy() {
        let (rules, overlays) = special_ally_rules_and_overlays();
        let mut sim = Simulation::with_seed(7);
        sim.install_session_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        register_special_pass_houses(
            &mut sim,
            &rules,
            &[("H", "Americans", false), ("Special", "Special", false)],
        );
        let roster = HouseRoster::default();
        sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster))
            .unwrap();
        assert!(crate::map::houses::are_houses_mutually_allied(
            sim.house_alliances.as_map(),
            "Special",
            "H"
        ));
    }
}
