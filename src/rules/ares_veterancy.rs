//! Retained optional Ares promotion, experience-sharing and academy
//! configuration, projected by the rules registry.
//! Source: installed Ares TypeExt reader1003DFB0 (the Experience.* block,
//! then the Promote.* block), its constructor1003C710, BuildingTypeExt
//! reader1000DD40 (the Academy.* block and the computed+138 flag) and RulesExt
//! LoadBeforeTypeData1002AB60 (VeteranFlashTimer). Promote.*Sound and the
//! EVA.*Promoted cues are fixed-catalog references bound with the other
//! sounds; the Promote.*Type pointers are Find-only (reader1003FA40 ->
//! 1003C650), resolved by the registry before each pass reaches this reader.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::superweapon_type::TechnoTypeReference;
use crate::util::native_x87::NativeF64Bits;

/// TypeExt+17B..+1B0 (Experience.*) and +503, +514..+520, +52C..+540
/// (Promote.*).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct VeterancyTypeRules {
    passengers_gain_experience: bool,
    experience_from_passengers: bool,
    passenger_modifier: NativeF64Bits,
    mind_control_self_modifier: NativeF64Bits,
    mind_control_victim_modifier: NativeF64Bits,
    spawn_owner_modifier: NativeF64Bits,
    spawn_modifier: NativeF64Bits,
    experience_from_airstrike: bool,
    airstrike_modifier: NativeF64Bits,
    include_passengers: bool,
    /// Nullable<int> Promote.VeteranFlash / Promote.EliteFlash.
    flash: [Option<i32>; 2],
    /// Promote.VeteranType / Promote.EliteType.
    promote_types: [Option<TechnoTypeReference>; 2],
    /// Promote.VeteranExperience / Promote.EliteExperience.
    promote_experience: [NativeF64Bits; 2],
}

impl Default for VeterancyTypeRules {
    fn default() -> Self {
        // Constructor1003C710: PromotePassengers false, FromPassengers true,
        // the passenger/victim/spawn/airstrike modifiers 1.0, the mind
        // controller's own and the spawn owner's 0.0; no promotion type,
        // flash or experience.
        let one = NativeF64Bits::ONE;
        let zero = NativeF64Bits::POSITIVE_ZERO;
        Self {
            passengers_gain_experience: false,
            experience_from_passengers: true,
            passenger_modifier: one,
            mind_control_self_modifier: zero,
            mind_control_victim_modifier: one,
            spawn_owner_modifier: zero,
            spawn_modifier: one,
            experience_from_airstrike: false,
            airstrike_modifier: one,
            include_passengers: false,
            flash: [None; 2],
            promote_types: [None, None],
            promote_experience: [zero; 2],
        }
    }
}

/// Which rank a promotion reached; indexes the per-rank Promote.* slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromotedRank {
    Veteran = 0,
    Elite = 1,
}

impl VeterancyTypeRules {
    /// One reached TypeExt LoadFromINI pass. `promote_types` are the
    /// registry's Find-only reads of Promote.VeteranType/EliteType: absent,
    /// empty and unknown names retain (None), `none` clears (Some(None)).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        promote_types: [Option<Option<TechnoTypeReference>>; 2],
    ) {
        // 1003DFB0: Valueable<bool>1000EBF0 and Valueable<double>1000EB00.
        self.passengers_gain_experience = section.read_ares_bool(
            "Experience.PromotePassengers",
            self.passengers_gain_experience,
        );
        self.experience_from_passengers =
            section.read_ares_bool("Experience.FromPassengers", self.experience_from_passengers);
        self.passenger_modifier =
            section.read_ares_double_bits("Experience.PassengerModifier", self.passenger_modifier);
        self.mind_control_self_modifier = section.read_ares_double_bits(
            "Experience.MindControlSelfModifier",
            self.mind_control_self_modifier,
        );
        self.mind_control_victim_modifier = section.read_ares_double_bits(
            "Experience.MindControlVictimModifier",
            self.mind_control_victim_modifier,
        );
        self.spawn_owner_modifier = section
            .read_ares_double_bits("Experience.SpawnOwnerModifier", self.spawn_owner_modifier);
        self.spawn_modifier =
            section.read_ares_double_bits("Experience.SpawnModifier", self.spawn_modifier);
        self.experience_from_airstrike =
            section.read_ares_bool("Experience.FromAirstrike", self.experience_from_airstrike);
        self.airstrike_modifier =
            section.read_ares_double_bits("Experience.AirstrikeModifier", self.airstrike_modifier);
        // The promotion block: Nullable<int>10008CC0 flashes, the two
        // pointers, then the two Valueable<double> experiences.
        self.include_passengers =
            section.read_ares_bool("Promote.IncludePassengers", self.include_passengers);
        for (slot, key) in ["Promote.VeteranFlash", "Promote.EliteFlash"]
            .into_iter()
            .enumerate()
        {
            self.flash[slot] = section.read_ares_nullable_int(key, self.flash[slot]);
        }
        for (slot, target) in promote_types.into_iter().enumerate() {
            if let Some(target) = target {
                self.promote_types[slot] = target;
            }
        }
        for (slot, key) in ["Promote.VeteranExperience", "Promote.EliteExperience"]
            .into_iter()
            .enumerate()
        {
            self.promote_experience[slot] =
                section.read_ares_double_bits(key, self.promote_experience[slot]);
        }
    }

    pub(crate) fn passengers_gain_experience(&self) -> bool {
        self.passengers_gain_experience
    }

    pub(crate) fn experience_from_passengers(&self) -> bool {
        self.experience_from_passengers
    }

    pub(crate) fn passenger_modifier(&self) -> NativeF64Bits {
        self.passenger_modifier
    }

    pub(crate) fn mind_control_self_modifier(&self) -> NativeF64Bits {
        self.mind_control_self_modifier
    }

    pub(crate) fn mind_control_victim_modifier(&self) -> NativeF64Bits {
        self.mind_control_victim_modifier
    }

    pub(crate) fn spawn_owner_modifier(&self) -> NativeF64Bits {
        self.spawn_owner_modifier
    }

    pub(crate) fn spawn_modifier(&self) -> NativeF64Bits {
        self.spawn_modifier
    }

    pub(crate) fn experience_from_airstrike(&self) -> bool {
        self.experience_from_airstrike
    }

    pub(crate) fn airstrike_modifier(&self) -> NativeF64Bits {
        self.airstrike_modifier
    }

    pub(crate) fn include_passengers(&self) -> bool {
        self.include_passengers
    }

    pub(crate) fn flash(&self, rank: PromotedRank) -> Option<i32> {
        self.flash[rank as usize]
    }

    pub(crate) fn promote_type(&self, rank: PromotedRank) -> Option<&TechnoTypeReference> {
        self.promote_types[rank as usize].as_ref()
    }

    pub(crate) fn promote_experience(&self, rank: PromotedRank) -> NativeF64Bits {
        self.promote_experience[rank as usize]
    }
}

/// BuildingTypeExt+138..+170: the computed Academy flag, Academy.Types,
/// Academy.Ignore and the four per-class veterancy bonuses.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcademyTypeRules {
    academy: bool,
    types: Vec<TechnoTypeReference>,
    ignore: Vec<TechnoTypeReference>,
    /// Infantry, aircraft, vehicle and building bonuses (+158..+170).
    bonus: [NativeF64Bits; 4],
}

impl Default for AcademyTypeRules {
    fn default() -> Self {
        // BuildingTypeExt constructor: no lists, every bonus 0.0.
        Self {
            academy: false,
            types: Vec::new(),
            ignore: Vec::new(),
            bonus: [NativeF64Bits::POSITIVE_ZERO; 4],
        }
    }
}

/// The class an academy bonus is chosen by (ApplyAcademy10020750's
/// `considerAs`): organic Units count as infantry, ConsideredAircraft Units as
/// aircraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcademyClass {
    Infantry = 0,
    Aircraft = 1,
    Vehicle = 2,
    Building = 3,
}

impl AcademyTypeRules {
    /// One reached BuildingTypeExt pass (1000DD40). The two lists come from
    /// the registry's Find-only ValueableVector<TechnoTypeClass*>1000EDD0
    /// reads; a reached read replaces the list. The four doubles are
    /// Valueable<double>1000EB00, and the Academy flag is recomputed from
    /// the retained bonuses after every pass.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        types: Option<Vec<TechnoTypeReference>>,
        ignore: Option<Vec<TechnoTypeReference>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(types) = types {
            self.types = types;
        }
        if let Some(ignore) = ignore {
            self.ignore = ignore;
        }
        for (slot, key) in [
            "Academy.InfantryVeterancy",
            "Academy.AircraftVeterancy",
            "Academy.VehicleVeterancy",
            "Academy.BuildingVeterancy",
        ]
        .into_iter()
        .enumerate()
        {
            self.bonus[slot] = section.read_ares_double_bits(key, self.bonus[slot]);
        }
        self.academy = self
            .bonus
            .iter()
            .any(|bonus| f64::from_bits(bonus.bits()) > 0.0);
    }

    /// BuildingTypeExt::IsAcademy: some class bonus is positive.
    pub(crate) fn is_academy(&self) -> bool {
        self.academy
    }

    /// The bonus this academy grants `techno` of `class`, if its lists admit
    /// it: Academy.Types empty or holding it, and Academy.Ignore not.
    pub(crate) fn bonus_for(
        &self,
        techno: &TechnoTypeReference,
        class: AcademyClass,
    ) -> Option<NativeF64Bits> {
        let listed = self.types.is_empty() || self.types.contains(techno);
        (listed && !self.ignore.contains(techno)).then_some(self.bonus[class as usize])
    }
}

/// RulesExt+140 `[AudioVisual] VeteranFlashTimer` (constructor 0).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct VeterancyGlobalRules {
    veteran_flash_timer: i32,
}

impl VeterancyGlobalRules {
    /// LoadBeforeTypeData1002AB60: Valueable<int>1000A570 in AudioVisual.
    pub(crate) fn read_before_type_data(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(section) = pass.section("AudioVisual") {
            self.veteran_flash_timer =
                section.read_ares_int("VeteranFlashTimer", self.veteran_flash_timer);
        }
    }

    pub(crate) fn veteran_flash_timer(&self) -> i32 {
        self.veteran_flash_timer
    }
}
