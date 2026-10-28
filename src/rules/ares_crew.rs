//! Ares survivor configuration. The ordered Process owner reads and retains
//! these fields; simulation consumes the immutable ObjectType/Side projection.

use crate::rules::ini_parser::IniSection;

/// TypeExt constructor1003C72E..1003C76E: all survivor counts/chances -1,
/// side-specific pilot pointers empty, Crew.TechnicianChance unset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CrewTypeRules {
    pilots: i32,
    pilot_chances: [i32; 3],
    passenger_chances: [i32; 3],
    side_pilots: Vec<Option<String>>,
    technician_chance: Option<i32>,
    engineer_chance: Option<i32>,
}

impl Default for CrewTypeRules {
    fn default() -> Self {
        Self {
            pilots: -1,
            pilot_chances: [-1; 3],
            passenger_chances: [-1; 3],
            side_pilots: Vec::new(),
            technician_chance: None,
            engineer_chance: None,
        }
    }
}

impl CrewTypeRules {
    ///1003DFDA..1003E016,1003E01B..1003E070,1003EC7C..1003EC8A.
    /// Pointer resolution is Find-only and belongs to the Process registry.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        side_pilots: &[Option<Option<String>>],
    ) {
        self.pilots = section.read_ares_int("Survivor.Pilots", self.pilots);
        read_ranked_chances(section, "Pilot", &mut self.pilot_chances);
        read_ranked_chances(section, "Passenger", &mut self.passenger_chances);
        self.side_pilots.resize(side_pilots.len(), None);
        for (pilot, update) in self.side_pilots.iter_mut().zip(side_pilots) {
            if let Some(update) = update {
                *pilot = update.clone();
            }
        }
        self.technician_chance =
            section.read_ares_nullable_int("Crew.TechnicianChance", self.technician_chance);
        self.engineer_chance =
            section.read_ares_nullable_int("Crew.EngineerChance", self.engineer_chance);
    }

    /// Ares GetCrewCount1004C870; negative leaves the original6F3950 result.
    pub(crate) fn pilot_count(&self, native_default: i32) -> i32 {
        if self.pilots >= 0 {
            self.pilots
        } else {
            native_default
        }
    }

    pub(crate) fn pilot_chances(&self) -> [i32; 3] {
        self.pilot_chances
    }
    pub(crate) fn passenger_chances(&self) -> [i32; 3] {
        self.passenger_chances
    }
    pub(crate) fn pilot_for_side(&self, side: i32) -> Option<&str> {
        self.side_pilots
            .get(usize::try_from(side).ok()?)?
            .as_deref()
    }
    pub(crate) fn technician_chance(&self) -> Option<i32> {
        self.technician_chance
    }
    pub(crate) fn engineer_chance(&self) -> Option<i32> {
        self.engineer_chance
    }
}

/// Installed Ares1003F750..1003F833 reads the unqualified key first,
/// broadcasting a successful value, then Rookie, Veteran and Elite overrides.
fn read_ranked_chances(section: &IniSection, kind: &str, chances: &mut [i32; 3]) {
    if let Some(base) = section.read_ares_nullable_int(&format!("Survivor.{kind}Chance"), None) {
        *chances = [base; 3];
    }
    for (rank, chance) in ["Rookie", "Veteran", "Elite"].into_iter().zip(chances) {
        *chance = section.read_ares_int(&format!("Survivor.{rank}{kind}Chance"), *chance);
    }
}
