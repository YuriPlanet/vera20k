//! Ares `[General]CanMakeStuffUp` (April-fools #1000, RulesExt+0x28).
//!
//! - INIClass_ReadScenario_ValidateThings (Ares.dll 100677B0, hook
//!   0x687C16, in `ScenarioClass::Full_Init @ 0x00686B20` after
//!   Post_Map_Init and DrainDeferredFinalizationQueue) draws 25 to 28
//!   Scenario RandomRanged values (one per crew type that exists), in the
//!   order of
//!   [`Simulation::ares_make_stuff_up_rules`], and writes them into the
//!   shared RulesClass. VERA's Rules stay immutable: the written values are
//!   [`AresMadeUpRules`], saved with the Simulation and in its state hash.
//!   After the writes the DLL reopens the log and shuffles a 53-entry
//!   "Gremlins" message table, ten more draws (`RandomRanged(0, 52)` down to
//!   `(0, 43)`); only the log sees the shuffle, the draws are kept here.
//! - LogicClass_Update_1000 (Ares.dll 100680F0, hook 0x55AFB3, the head of
//!   `LogicClass::AI @ 0x0055AFB0`): every 90th frame each InfantryClass row
//!   ([`Simulation::ares_make_stuff_up_logic`]).
//!
//! RESIDUAL: the readers of the written Rules values. The accessors below
//! answer each value, but no Rust port of their gamemd readers asks them
//! yet (VeteranRatio,
//! BuildSpeed, BuildupTime, RefundPercent, GrowthRate, Stray, FlightLevel,
//! the two fall rates, the three crews, HoverHeight, WindDirection,
//! MaximumQueuedObjects, MaxWaypointPathLength, CruiseHeight, the four
//! colours, PoseDir, DeployDir, Gravity, IvanTimedDelay, PlayerAutoCrush,
//! PlayerReturnFire, PlayerScatter: about 110 Rust read sites). Trigger:
//! `CanMakeStuffUp=yes`, which no retail INI sets. Effect: the game keeps
//! the authored values; the draws and the infantry pass still match.
//!
//! Ledger: Scenario RNG only, the draws named above. Timers, detaches: none.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// The values 100677B0 wrote into RulesClass, as their raw draws.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresMadeUpRules {
    /// `RandomRanged(1, 500)`: VeteranRatio = draw / 100.0.
    veteran_ratio: i32,
    /// `RandomRanged(1, 350)`: BuildSpeed = draw / 100.0.
    build_speed: i32,
    /// `RandomRanged(1, 50)`: BuildupTime = draw / 100.0.
    buildup_time: i32,
    /// `RandomRanged(1, 900)`: RefundPercent = draw / 100.0.
    refund_percent: i32,
    /// `RandomRanged(1, 5)`: GrowthRate /= draw.
    growth_rate_divisor: i32,
    /// `RandomRanged(1, 5)`: Stray = draw << 8, whole cells in leptons.
    stray_cells: i32,
    /// `RandomRanged(900, 2500)`: FlightLevel.
    flight_level: i32,
    /// `RandomRanged(1, 10) == 3`: NoParachuteMaxFallRate -= 5, then
    /// ParachuteMaxFallRate negated.
    fall_rates_flipped: bool,
    /// JOSH, CAML and COW when they exist and their `RandomRanged(0, 4)`
    /// is 0: AlliedCrew, SovietCrew and ThirdCrew.
    crews: [Option<String>; 3],
    /// `RandomRanged(-30, 30)` added to HoverHeight.
    hover_height: i32,
    /// `RandomRanged(0, 7)`: WindDirection.
    wind_direction: i32,
    /// `RandomRanged(-5, 5)` added to MaximumQueuedObjects.
    maximum_queued_objects: i32,
    /// `RandomRanged(-5, 5)` added to MaxWaypointPathLength.
    max_waypoint_path_length: i32,
    /// `RandomRanged(-200, 200)` added to CruiseHeight.
    cruise_height: i32,
    /// `RandomRanged(-1, 1)` added to LaserTargetColor, IronCurtainColor,
    /// BerserkColor and ForceShieldColor, each clamped to 0..=13.
    colors: [i32; 4],
    /// `RandomRanged(0, 255)`: PoseDir.
    pose_dir: i32,
    /// `RandomRanged(0, 255)`: DeployDir.
    deploy_dir: i32,
    /// `RandomRanged(-2, 2)` added to Gravity.
    gravity: i32,
    /// `RandomRanged(-150, 150)` added to IvanTimedDelay.
    ivan_timed_delay: i32,
    /// `RandomRanged(0, 2) == 0`: PlayerAutoCrush, PlayerReturnFire,
    /// PlayerScatter.
    player_flags: [bool; 3],
}

/// Which of the four colours 100677B0 rerolls, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MadeUpColor {
    LaserTarget,
    IronCurtain,
    Berserk,
    ForceShield,
}

/// The three crews 100677B0 may replace, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MadeUpCrew {
    Allied,
    Soviet,
    Third,
}

/// The three player flags 100677B0 rerolls, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MadeUpPlayerFlag {
    AutoCrush,
    ReturnFire,
    Scatter,
}

/// The InfantryType IDs 100677B0 looks up (`DAT_100a7b30`, `DAT_100a7b38`,
/// `DAT_1009cf30`), all three before the first roll.
const CREW_IDS: [&str; 3] = ["JOSH", "CAML", "COW"];

/// `(double)draw / 100.0`, exact in binary64 as on the x87 at PC53.
fn hundredths(draw: i32) -> f64 {
    f64::from(draw) / 100.0
}

#[allow(dead_code)]
impl AresMadeUpRules {
    pub(crate) fn veteran_ratio(&self) -> f64 {
        hundredths(self.veteran_ratio)
    }
    pub(crate) fn build_speed(&self) -> f64 {
        hundredths(self.build_speed)
    }
    pub(crate) fn buildup_time(&self) -> f64 {
        hundredths(self.buildup_time)
    }
    pub(crate) fn refund_percent(&self) -> f64 {
        hundredths(self.refund_percent)
    }
    /// GrowthRate after its one division by the draw.
    pub(crate) fn growth_rate(&self, authored: f64) -> f64 {
        authored / f64::from(self.growth_rate_divisor)
    }
    pub(crate) fn stray(&self) -> i32 {
        self.stray_cells << 8
    }
    pub(crate) fn flight_level(&self) -> i32 {
        self.flight_level
    }
    pub(crate) fn parachute_max_fall_rate(&self, authored: i32) -> i32 {
        if self.fall_rates_flipped {
            authored.wrapping_neg()
        } else {
            authored
        }
    }
    pub(crate) fn no_parachute_max_fall_rate(&self, authored: i32) -> i32 {
        if self.fall_rates_flipped {
            authored.wrapping_sub(5)
        } else {
            authored
        }
    }
    /// The replacing InfantryType ID, or `authored`.
    pub(crate) fn crew<'a>(
        &'a self,
        crew: MadeUpCrew,
        authored: Option<&'a str>,
    ) -> Option<&'a str> {
        self.crews[crew as usize].as_deref().or(authored)
    }
    pub(crate) fn hover_height(&self, authored: i32) -> i32 {
        authored.wrapping_add(self.hover_height)
    }
    pub(crate) fn wind_direction(&self) -> i32 {
        self.wind_direction
    }
    pub(crate) fn maximum_queued_objects(&self, authored: i32) -> i32 {
        authored.wrapping_add(self.maximum_queued_objects)
    }
    pub(crate) fn max_waypoint_path_length(&self, authored: i32) -> i32 {
        authored.wrapping_add(self.max_waypoint_path_length)
    }
    pub(crate) fn cruise_height(&self, authored: i32) -> i32 {
        authored.wrapping_add(self.cruise_height)
    }
    pub(crate) fn color(&self, color: MadeUpColor, authored: i32) -> i32 {
        authored
            .wrapping_add(self.colors[color as usize])
            .clamp(0, 13)
    }
    pub(crate) fn pose_dir(&self) -> i32 {
        self.pose_dir
    }
    pub(crate) fn deploy_dir(&self) -> i32 {
        self.deploy_dir
    }
    pub(crate) fn gravity(&self, authored: i32) -> i32 {
        authored.wrapping_add(self.gravity)
    }
    pub(crate) fn ivan_timed_delay(&self, authored: i32) -> i32 {
        authored.wrapping_add(self.ivan_timed_delay)
    }
    pub(crate) fn player_flag(&self, flag: MadeUpPlayerFlag) -> bool {
        self.player_flags[flag as usize]
    }
}

impl Simulation {
    /// The Rules values CanMakeStuffUp wrote at scenario start, if it did.
    #[allow(dead_code)]
    pub(crate) fn ares_made_up_rules(&self) -> Option<&AresMadeUpRules> {
        self.ares_made_up_rules.as_ref()
    }

    /// INIClass_ReadScenario_ValidateThings 100677B0's CanMakeStuffUp block
    /// (hook 0x687C16), after its type checks: every draw from Scenario RNG
    /// in the DLL's order. Fresh scenario load only; a restore keeps the saved
    /// values.
    pub(crate) fn ares_make_stuff_up_rules(&mut self, rules: &RuleSet) {
        if !rules.ares_general().can_make_stuff_up() {
            return;
        }
        let rng = &mut self.scenario_rng;
        let mut draw = |low: i32, high: i32| rng.next_range_i32_inclusive(low, high);
        let veteran_ratio = draw(1, 500);
        let build_speed = draw(1, 350);
        let buildup_time = draw(1, 50);
        let refund_percent = draw(1, 900);
        let growth_rate_divisor = draw(1, 5);
        let stray_cells = draw(1, 5);
        let flight_level = draw(900, 2500);
        let fall_rates_flipped = draw(1, 10) == 3;
        // InfantryTypeClass::Find for all three, then one roll per found type.
        let found = CREW_IDS.map(|id| {
            rules
                .infantry_ids
                .iter()
                .find(|name| name.eq_ignore_ascii_case(id))
                .cloned()
        });
        let crews = found.map(|id| id.filter(|_| draw(0, 4) == 0));
        let hover_height = draw(-30, 30);
        let wind_direction = draw(0, 7);
        let maximum_queued_objects = draw(-5, 5);
        let max_waypoint_path_length = draw(-5, 5);
        let cruise_height = draw(-200, 200);
        let colors = [(); 4].map(|()| draw(-1, 1));
        let pose_dir = draw(0, 255);
        let deploy_dir = draw(0, 255);
        let gravity = draw(-2, 2);
        let ivan_timed_delay = draw(-150, 150);
        let player_flags = [(); 3].map(|()| draw(0, 2) == 0);
        // The log's "Gremlins" table shuffle: RandomRanged(0, n) for n = 52
        // down to 43 (`0x2a < n` ends the loop).
        for high in (43..=52).rev() {
            draw(0, high);
        }
        self.ares_made_up_rules = Some(AresMadeUpRules {
            veteran_ratio,
            build_speed,
            buildup_time,
            refund_percent,
            growth_rate_divisor,
            stray_cells,
            flight_level,
            fall_rates_flipped,
            crews,
            hover_height,
            wind_direction,
            maximum_queued_objects,
            max_waypoint_path_length,
            cruise_height,
            colors,
            pose_dir,
            deploy_dir,
            gravity,
            ivan_timed_delay,
            player_flags,
        });
    }

    /// LogicClass_Update_1000 100680F0 (hook 0x55AFB3): on a frame divisible
    /// by 90, each InfantryClass array row in order. One not falling
    /// (`+0x8D`) draws `RandomRanged(0, 99)`: below 8 it Panics (vt+0x518,
    /// Infantry521C10), below 12 it UnPanics (vt+0x51C, Infantry521C40). A
    /// falling one with a Parachute (`+0x88`) above 415 leptons (GetHeight,
    /// vt+0x1C8) loses it: the canopy's remaining loops (`+0x195`) and its
    /// FallRate (`+0x2C`) become 0, and it falls as a bomb (`+0x8F`).
    pub(crate) fn ares_make_stuff_up_logic(&mut self, rules: &RuleSet) {
        if !rules.ares_general().can_make_stuff_up() || self.session.binary_frame() % 90 != 0 {
            return;
        }
        for index in 0..self.substrate.entities.infantry_registry_len() {
            let Some(id) = self.substrate.entities.infantry_registry_at(index) else {
                continue;
            };
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            if entity.category() != EntityCategory::Infantry {
                continue;
            }
            if !entity.is_falling_down() {
                let roll = self.scenario_rng.next_range_i32_inclusive(0, 99);
                let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                    continue;
                };
                let entity = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .expect("same infantry row");
                if roll < 8 {
                    crate::sim::infantry::apply_panic_force(object, entity);
                } else if roll < 12 {
                    crate::sim::infantry::apply_unpanic_force(object, entity);
                }
            } else if let Some(canopy) = entity.parachute_animation() {
                if crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    self.resolved_terrain.as_ref(),
                ) > 415
                {
                    if let Some(anim) = self.substrate.anims.get_mut(canopy) {
                        anim.end_loops();
                    }
                    if let Some(entity) = self.substrate.entities.get_mut(id) {
                        entity.drop_parachute_as_bomb();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn rules(general: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\n{general}\n[InfantryTypes]\n0=E1\n1=JOSH\n\
             [E1]\nStrength=125\nArmor=none\nSpeed=4\n\
             Locomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\nMovementZone=Infantry\n\
             [JOSH]\nStrength=50\nArmor=none\nSpeed=4\n"
        ));
        let layers = crate::rules::native_processing::RulesLayerStack::new(ini);
        let processed = layers
            .process_with_fixed_art(&IniFile::from_str(""))
            .unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    }

    /// 100677B0 draws in its exact order and count: JOSH exists, so it alone
    /// of the three crews rolls, and the log shuffle draws ten more. The
    /// written values survive a save and change the state hash.
    #[test]
    fn ares_make_stuff_up_draws_in_dll_order_and_saves_the_values() {
        let rules = rules("CanMakeStuffUp=yes");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        let mut replay = sim.scenario_rng.clone();
        sim.ares_make_stuff_up_rules(&rules);
        let mut ranges = vec![
            (1, 500),
            (1, 350),
            (1, 50),
            (1, 900),
            (1, 5),
            (1, 5),
            (900, 2500),
            (1, 10),
            (0, 4),
            (-30, 30),
            (0, 7),
            (-5, 5),
            (-5, 5),
            (-200, 200),
            (-1, 1),
            (-1, 1),
            (-1, 1),
            (-1, 1),
            (0, 255),
            (0, 255),
            (-2, 2),
            (-150, 150),
            (0, 2),
            (0, 2),
            (0, 2),
        ];
        ranges.extend((43..=52).rev().map(|high| (0, high)));
        let draws: Vec<i32> = ranges
            .iter()
            .map(|&(low, high)| replay.next_range_i32_inclusive(low, high))
            .collect();
        assert_eq!(sim.scenario_rng.logical_state(), replay.logical_state());
        let made_up = sim.ares_made_up_rules().unwrap().clone();
        assert_eq!(made_up.veteran_ratio(), f64::from(draws[0]) / 100.0);
        assert_eq!(made_up.stray(), draws[5] << 8);
        assert_eq!(made_up.flight_level(), draws[6]);
        assert_eq!(
            made_up.crew(MadeUpCrew::Allied, None),
            (draws[8] == 0).then_some("JOSH")
        );
        assert_eq!(made_up.crew(MadeUpCrew::Soviet, Some("E1")), Some("E1"));
        assert_eq!(made_up.deploy_dir(), draws[19]);
        assert_eq!(made_up.gravity(6), 6 + draws[20]);
        assert_eq!(
            made_up.player_flag(MadeUpPlayerFlag::Scatter),
            draws[24] == 0
        );
        let hashed = sim.state_hash();
        let saved = sim.ares_made_up_rules.take();
        assert_ne!(sim.state_hash(), hashed, "the written values are hashed");
        sim.ares_made_up_rules = saved;
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "STUFF", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(restored.ares_made_up_rules(), Some(&made_up));

        // Without the key nothing is drawn.
        let mut off = Simulation::new();
        let before = off.scenario_rng.logical_state();
        off.ares_make_stuff_up_rules(&self::rules(""));
        assert_eq!(off.scenario_rng.logical_state(), before);
        assert!(off.ares_made_up_rules().is_none());
    }

    /// 100680F0 runs only on frames divisible by 90 and draws one
    /// RandomRanged(0, 99) per standing infantryman; below 8 it panics.
    #[test]
    fn ares_make_stuff_up_rolls_each_infantryman_every_ninety_frames() {
        let rules = rules("CanMakeStuffUp=yes");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let soldiers: Vec<u64> = (0..3)
            .map(|i| {
                sim.spawn_object("E1", "Americans", 10 + i, 10, 0, &rules)
                    .unwrap()
            })
            .collect();
        sim.session.set_binary_frame_for_test(91);
        let before = sim.scenario_rng.logical_state();
        sim.ares_make_stuff_up_logic(&rules);
        assert_eq!(sim.scenario_rng.logical_state(), before);

        sim.session.set_binary_frame_for_test(180);
        let mut replay = sim.scenario_rng.clone();
        // One draw per standing InfantryClass row, in array order.
        let rolls: Vec<(u64, i32)> = (0..sim.substrate.entities.infantry_registry_len())
            .filter_map(|index| sim.substrate.entities.infantry_registry_at(index))
            .map(|id| (id, replay.next_range_i32_inclusive(0, 99)))
            .collect();
        sim.ares_make_stuff_up_logic(&rules);
        for (id, roll) in rolls {
            if !soldiers.contains(&id) {
                continue;
            }
            let fear = sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .infantry
                .as_ref()
                .unwrap()
                .fear_level();
            assert_eq!(fear == 300, roll < 8, "roll {roll}");
        }
        assert_eq!(sim.scenario_rng.logical_state(), replay.logical_state());
    }
}
