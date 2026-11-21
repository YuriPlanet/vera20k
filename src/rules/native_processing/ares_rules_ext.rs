//! Ares RulesExt state: the global controls phase and the `[General]` rules
//! Ares reads beside RulesClass.

use crate::rules::ini_parser::IniFile;
use std::hash::Hash;

/// RulesExt ctor1002A360 clears+14A/+14B/+14C. Process entry668BF0's installed
/// hook1002C970 reaches1002B490/1002B370 before Colors and type allocation.
/// This independent RulesExt phase belongs to the existing retained Rules owner.
/// Native constructor/phase2 reads: rules_oracle/ares_factory_owners.json
/// global_controls; Process caller/order rests on installed instructions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresGlobalControls {
    pub(super) phase: i32,
    /// RulesExt+70: constructor empty; LoadAfterTypeData reads HSBuilding.
    #[serde(default)]
    pub(super) hunter_seeker_buildings: Vec<String>,
    pub(super) allow_bypass_build_limit: [bool; 3],
    // RulesExt+160/+164: constructor zero/unset; retained by the global
    // Rules owner, independently of per-Type reset.
    pub(super) emp_ai_recover_mission: Option<crate::rules::mission_data::MissionType>,
    /// RulesExt+4B: constructor1002A3DF sets true; reader1002AC23..35.
    #[serde(default = "AresGlobalControls::default_type_select_use_deploy")]
    pub(super) type_select_use_deploy: bool,
    /// RulesExt+14F `[General] ChronoInfantryCrush`: constructor1002A360
    /// stores the dword 0x01010100 at +14C (+14F true); 1002AB60 reads it
    /// with Valueable<bool>. The Teleport make-room hook (718275,
    /// 1004B980) reads it.
    #[serde(default = "AresGlobalControls::default_chrono_infantry_crush")]
    pub(super) chrono_infantry_crush: bool,
    /// RulesExt+144 `[CrateRules] RandomCrateMoney` (Valueable<int>,
    /// constructor 900): the Money crate's random top-up
    /// (CellClass_CrateBeingCollected_MoneyRandom 1002C8A0, hook 0x48248D).
    #[serde(default = "AresGlobalControls::default_random_crate_money")]
    pub(super) random_crate_money: i32,
    /// RulesExt+14E `[General] RepairStopOnInsufficientFunds` (Valueable<bool>,
    /// constructor true, the dword 0x01010100 at +14C): BuildingClass_
    /// UpdateRepair_Funds 10014630 (hook 0x4509B4).
    #[serde(default = "AresGlobalControls::default_repair_stop_on_insufficient_funds")]
    pub(super) repair_stop_on_insufficient_funds: bool,
    #[serde(default)]
    pub(super) bounty: crate::rules::ares_bounty::BountyGlobalRules,
    #[serde(default)]
    pub(super) cloak_power: crate::rules::ares_cloak_power::CloakPowerGlobalRules,
    #[serde(default)]
    pub(super) warhead: crate::rules::ares_warhead::WarheadGlobalRules,
    #[serde(default)]
    pub(super) veterancy: crate::rules::ares_veterancy::VeterancyGlobalRules,
    #[serde(default)]
    pub(super) techno_combat: crate::rules::ares_techno_type::TechnoCombatGlobalRules,
    #[serde(default)]
    pub(super) building: crate::rules::ares_building_type::BuildingGlobalRules,
    #[serde(default)]
    pub(super) tiberium: crate::rules::ares_tiberium::TiberiumGlobalRules,
    #[serde(default)]
    pub(super) drop_pod: crate::rules::superweapon_type::AresDropPodGlobalRules,
    #[serde(default)]
    pub(super) firestorm: crate::rules::ares_firestorm::FirestormGlobalRules,
    #[serde(default)]
    pub(super) general: AresGeneralRules,
    #[serde(default)]
    pub(super) forest_fire: crate::rules::terrain_object_type::ForestFireRules,
}

/// RulesExt keys of LoadBeforeTypeData (1002AB60) and LoadAfterTypeData
/// (1002A800) whose consumers read them here; defaults are the constructor
/// 1002A360's. The ones marked residual have no consumer in VERA
/// (docs/ares-techno-hooks.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresGeneralRules {
    /// +0x8 `[AudioVisual] InfantryElectrocuted` (Valueable<AnimTypeClass*>,
    /// Find only, null), read first by LoadAfterTypeData.
    pub(super) infantry_electrocuted: Option<String>,
    /// +0x28 `CanMakeStuffUp` (false): `sim::ares_make_stuff_up`.
    pub(super) can_make_stuff_up: bool,
    /// +0x48 `EnemyInsignia` (true; constructor 0x1002A3DF).
    enemy_insignia: bool,
    /// +0x49 `EnemyWrench` (true).
    pub(super) enemy_wrench: bool,
    /// +0x4C `TeamRetaliate` (false): a computer team takes its attacker
    /// as its focus (TeamClass_AttackedBy_Retaliate 1003A180, hook
    /// 0x6EB432).
    pub(super) team_retaliate: bool,
    /// +0x7C..+0x8C `HunterSeekerDetonateProximity`, `DescendProximity`,
    /// `AscentSpeed`, `DescentSpeed`, `EmergeSpeed` (Valueable<int>, 0).
    /// The speeds feed the HunterSeeker Fly height step; the proximities
    /// are residual.
    pub(super) hunter_seeker: [i32; 5],
    /// +0xB0 `Message.SilosNeeded` (Valueable<CSFText>, empty). Residual.
    pub(super) message_silos_needed: String,
    /// +0x112 `[AudioVisual] DiskLaserAnimEnabled` (false).
    pub(super) disk_laser_anim_enabled: bool,
    /// +0x13D `UnitsUnsellable` (false): the default of TechnoType
    /// `Unsellable` for a unit's sale on a depot.
    pub(super) units_unsellable: bool,
    /// +0x150 `StartInMultiplayerUnitCost` (Nullable<int>, unset).
    pub(super) start_in_multiplayer_unit_cost: Option<i32>,
    /// +0x158 `AIFriendlyDistance` (Nullable<int>, unset).
    pub(super) ai_friendly_distance: Option<i32>,
}

impl Default for AresGeneralRules {
    fn default() -> Self {
        Self {
            infantry_electrocuted: None,
            can_make_stuff_up: false,
            enemy_insignia: true,
            enemy_wrench: true,
            team_retaliate: false,
            hunter_seeker: [0; 5],
            message_silos_needed: String::new(),
            disk_laser_anim_enabled: false,
            units_unsellable: false,
            start_in_multiplayer_unit_cost: None,
            ai_friendly_distance: None,
        }
    }
}

impl AresGeneralRules {
    /// 1002AB60's reads of these keys, in its order.
    pub(super) fn read_before_type_data(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(general) = pass.section("General") {
            self.can_make_stuff_up =
                general.read_ares_bool("CanMakeStuffUp", self.can_make_stuff_up);
            // 0x1002ABF5.
            self.enemy_insignia = general.read_ares_bool("EnemyInsignia", self.enemy_insignia);
            self.team_retaliate = general.read_ares_bool("TeamRetaliate", self.team_retaliate);
            self.message_silos_needed =
                general.read_ares_csf_label("Message.SilosNeeded", &self.message_silos_needed);
        }
        if let Some(audio_visual) = pass.section("AudioVisual") {
            self.disk_laser_anim_enabled =
                audio_visual.read_ares_bool("DiskLaserAnimEnabled", self.disk_laser_anim_enabled);
        }
        if let Some(general) = pass.section("General") {
            self.units_unsellable =
                general.read_ares_bool("UnitsUnsellable", self.units_unsellable);
            self.start_in_multiplayer_unit_cost = general.read_ares_nullable_int(
                "StartInMultiplayerUnitCost",
                self.start_in_multiplayer_unit_cost,
            );
            self.ai_friendly_distance =
                general.read_ares_nullable_int("AIFriendlyDistance", self.ai_friendly_distance);
        }
    }

    /// 1002A800's reads of these keys: InfantryElectrocuted first (its
    /// Find-only lookup `electrocuted`), then after EngineerAlwaysCaptureTech
    /// EnemyWrench, and after HSBuilding the hunter-seeker values.
    pub(super) fn read_after_type_data(
        &mut self,
        pass: &IniFile,
        electrocuted: Option<Option<String>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(anim) = electrocuted {
            self.infantry_electrocuted = anim;
        }
        let Some(general) = pass.section("General") else {
            return;
        };
        self.enemy_wrench = general.read_ares_bool("EnemyWrench", self.enemy_wrench);
        for (slot, key) in [
            "HunterSeekerDetonateProximity",
            "HunterSeekerDescendProximity",
            "HunterSeekerAscentSpeed",
            "HunterSeekerDescentSpeed",
            "HunterSeekerEmergeSpeed",
        ]
        .into_iter()
        .enumerate()
        {
            // Stock RulesClass keys, re-parsed.
            self.hunter_seeker[slot] = crate::rules::ares_switch::not_ares_read(|| {
                general.read_ares_int(key, self.hunter_seeker[slot])
            });
        }
    }

    /// `InfantryElectrocuted`; InfantryClass_ReceiveDamage_ElectricDeath
    /// (Ares.dll 1002C930, hook 0x518744) falls back to `ELECTRO`, then to
    /// the second AnimType.
    pub(crate) fn infantry_electrocuted(&self) -> Option<&str> {
        self.infantry_electrocuted.as_deref()
    }

    /// `CanMakeStuffUp`: the scenario-start Rules draws (Ares.dll 100677B0,
    /// hook 0x687C16) and the 90-frame infantry pass (100680F0, hook
    /// 0x55AFB3), `sim::ares_make_stuff_up`.
    pub(crate) fn can_make_stuff_up(&self) -> bool {
        self.can_make_stuff_up
    }

    /// `TeamRetaliate`: a formed team takes its attacker as its focus
    /// (TeamClass_AttackedBy_Retaliate, Ares.dll 1003A180, hook 0x6EB432).
    pub(crate) fn team_retaliate(&self) -> bool {
        self.team_retaliate
    }

    /// `EnemyWrench`: a player not allied with a repairing building's owner
    /// sees its wrench while it is uncloaked (TechnoClass_DrawExtras_PowerOff,
    /// Ares.dll 10049FE0, hook 0x6F526C).
    pub(crate) fn enemy_wrench(&self) -> bool {
        self.enemy_wrench
    }

    /// `EnemyInsignia`: a player not allied with an object's owner sees its
    /// rank insignia unless the type's `Insignia.ShowEnemy` says otherwise
    /// (TechnoClass_DrawVeterancy, Ares.dll 1004E0C0, hook 0x70A990).
    pub(crate) fn enemy_insignia(&self) -> bool {
        self.enemy_insignia
    }

    /// `DiskLaserAnimEnabled`: a disk laser's hit plays the warhead's damage
    /// animation (DiskLaserClass_Update_Anim, Ares.dll 1004CFF0, hook
    /// 0x4A76ED).
    pub(crate) fn disk_laser_anim_enabled(&self) -> bool {
        self.disk_laser_anim_enabled
    }

    /// `UnitsUnsellable`: TechnoType `Unsellable`'s default when a unit on a
    /// depot asks whether it can be sold (FootClass_CanBeSold_SellUnit,
    /// Ares.dll 10048FD0, hook 0x4D9EBD).
    pub(crate) fn units_unsellable(&self) -> bool {
        self.units_unsellable
    }

    /// `HunterSeekerDetonateProximity`, `DescendProximity`, `AscentSpeed`,
    /// `DescentSpeed` and `EmergeSpeed`, in that order.
    pub(crate) fn hunter_seeker_speeds(&self) -> [i32; 5] {
        self.hunter_seeker
    }

    /// `StartInMultiplayerUnitCost`: the price of one starting unit
    /// (MPGameModeClass_CreateStartingUnits_UnitCost, Ares.dll 1001EE10,
    /// hook 0x5D6D9A).
    pub(crate) fn start_in_multiplayer_unit_cost(&self) -> Option<i32> {
        self.start_in_multiplayer_unit_cost
    }

    /// `AIFriendlyDistance`, else `AISafeDistance`: the cells a team regroups
    /// from its base (TeamClass_GatherAtFriendlyBase_Distance, Ares.dll
    /// 1003A2A0, hook 0x6EFB69).
    pub(crate) fn ai_friendly_distance(&self, ai_safe_distance: i32) -> i32 {
        self.ai_friendly_distance.unwrap_or(ai_safe_distance)
    }
}

impl Default for AresGlobalControls {
    fn default() -> Self {
        Self {
            phase: 0,
            hunter_seeker_buildings: Vec::new(),
            allow_bypass_build_limit: [false; 3],
            emp_ai_recover_mission: None,
            type_select_use_deploy: Self::default_type_select_use_deploy(),
            chrono_infantry_crush: Self::default_chrono_infantry_crush(),
            random_crate_money: Self::default_random_crate_money(),
            repair_stop_on_insufficient_funds: Self::default_repair_stop_on_insufficient_funds(),
            bounty: Default::default(),
            cloak_power: Default::default(),
            warhead: Default::default(),
            veterancy: Default::default(),
            techno_combat: Default::default(),
            building: Default::default(),
            tiberium: Default::default(),
            drop_pod: Default::default(),
            firestorm: Default::default(),
            general: Default::default(),
            forest_fire: Default::default(),
        }
    }
}

impl AresGlobalControls {
    pub(crate) fn warhead(&self) -> &crate::rules::ares_warhead::WarheadGlobalRules {
        &self.warhead
    }

    pub(crate) fn building(&self) -> &crate::rules::ares_building_type::BuildingGlobalRules {
        &self.building
    }

    pub(crate) fn tiberium(&self) -> &crate::rules::ares_tiberium::TiberiumGlobalRules {
        &self.tiberium
    }

    pub(crate) fn firestorm(&self) -> &crate::rules::ares_firestorm::FirestormGlobalRules {
        &self.firestorm
    }

    pub(crate) fn general(&self) -> &AresGeneralRules {
        &self.general
    }

    /// The stock `TreeFire=` and `TreeFlammability=` reads, for the Ares
    /// terrain fire (`sim::terrain_fire`).
    pub(crate) fn forest_fire(&self) -> &crate::rules::terrain_object_type::ForestFireRules {
        &self.forest_fire
    }

    /// Read by SW_DropPod::Activate (10070E40) and the DropPod locomotor
    /// (`sim::superweapon::drop_pod`).
    pub(crate) fn drop_pod(&self) -> &crate::rules::superweapon_type::AresDropPodGlobalRules {
        &self.drop_pod
    }

    const fn default_type_select_use_deploy() -> bool {
        true
    }

    pub(crate) fn type_select_use_deploy(&self) -> bool {
        self.type_select_use_deploy
    }

    const fn default_chrono_infantry_crush() -> bool {
        true
    }

    pub(crate) fn chrono_infantry_crush(&self) -> bool {
        self.chrono_infantry_crush
    }

    const fn default_random_crate_money() -> i32 {
        900
    }

    pub(crate) fn random_crate_money(&self) -> i32 {
        self.random_crate_money
    }

    const fn default_repair_stop_on_insufficient_funds() -> bool {
        true
    }

    pub(crate) fn repair_stop_on_insufficient_funds(&self) -> bool {
        self.repair_stop_on_insufficient_funds
    }

    pub(crate) fn bounty(&self) -> &crate::rules::ares_bounty::BountyGlobalRules {
        &self.bounty
    }

    pub(crate) fn hunter_seeker_buildings(&self) -> &[String] {
        &self.hunter_seeker_buildings
    }

    pub(crate) fn cloak_power(&self) -> &crate::rules::ares_cloak_power::CloakPowerGlobalRules {
        &self.cloak_power
    }

    pub(crate) fn veterancy(&self) -> &crate::rules::ares_veterancy::VeterancyGlobalRules {
        &self.veterancy
    }

    pub(crate) fn techno_combat(&self) -> &crate::rules::ares_techno_type::TechnoCombatGlobalRules {
        &self.techno_combat
    }

    pub(crate) fn ai_bypasses_build_limit(&self, difficulty_index: usize) -> bool {
        self.allow_bypass_build_limit
            .get(difficulty_index)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn emp_ai_recover_mission(&self) -> Option<crate::rules::mission_data::MissionType> {
        self.emp_ai_recover_mission
    }

    /// Installed679A15 ->1002C9C0/1002B356 ->1002AB60/1002AECF.
    /// Unlike the Process-entry controls above, this unconditional reader
    /// runs immediately before Type data. The native named lookup is5B3910.
    pub(super) fn read_before_type_data(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        // Installed Ares1002AC23..1002AC35: General, retained bool reader.
        if let Some(section) = pass.section("General") {
            self.type_select_use_deploy =
                section.read_ares_bool("TypeSelectUseDeploy", self.type_select_use_deploy);
            self.chrono_infantry_crush =
                section.read_ares_bool("ChronoInfantryCrush", self.chrono_infantry_crush);
            // 1002AB60 reads it between IronCurtainFlash and
            // ChronoInfantryCrush; neither neighbour is a pointer.
            self.repair_stop_on_insufficient_funds = section.read_ares_bool(
                "RepairStopOnInsufficientFunds",
                self.repair_stop_on_insufficient_funds,
            );
        }
        // 1002AB60, after VeteranFlashTimer.
        if let Some(section) = pass.section("CrateRules") {
            self.random_crate_money =
                section.read_ares_int("RandomCrateMoney", self.random_crate_money);
        }
        self.bounty.read_before_type_data(pass);
        //1002AB60 reads DeactivateDim* and the speak delays in AudioVisual.
        self.cloak_power.read_before_type_data(pass);
        //1002AB60 also reads DamageAirConsiderBridges and IronCurtainFlash.
        self.warhead.read_before_type_data(pass);
        //1002AB60 also reads VeteranFlashTimer in AudioVisual.
        self.veterancy.read_before_type_data(pass);
        //1002AB60 also reads BerserkROFMultiplier, AutoRepel, PlayerAutoRepel.
        self.techno_combat.read_before_type_data(pass);
        //1002AB60 also reads ReturnStructures and Degrade.* in General.
        self.building.read_before_type_data(pass);
        //1002AB60 also reads CanMakeStuffUp .. AIFriendlyDistance.
        self.general.read_before_type_data(pass);
        if let Some(mission) = pass
            .section("CombatDamage")
            .and_then(|section| section.read_name("EMPAIRecoverMission", 2048))
            .and_then(crate::rules::mission_data::MissionType::from_map_name)
        {
            self.emp_ai_recover_mission = Some(mission);
        }
    }

    pub(super) fn read_pass(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if !(0..=2).contains(&self.phase) {
            return;
        }
        if self.phase == 0 {
            self.phase = 1;
        }
        //1002B3D3/10005800 reads2048, then1002A180 consumes at most3 bools.
        // A successful prefix writes+14C,+14B,+14A in reverse difficulty order;
        // failure stops, retaining the unparsed suffix of the current array.
        if let Some(tokens) = pass
            .section("GlobalControls")
            .and_then(|section| section.read_list("AllowBypassBuildLimit", 2048))
        {
            for (slot, token) in (0..3).rev().zip(tokens) {
                let Some(value) = crate::rules::ini_value::read_bool_decision(token) else {
                    break;
                };
                self.allow_bypass_build_limit[slot] = value;
            }
        }
        self.phase = 2;
    }
}
