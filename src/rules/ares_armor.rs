//! Installed Ares3.0 ordinary-weapon armor definitions and retained WH records.
//!
//! Identity93ce890f387a6c1b3240a2dd7b674f25fec5a0e96f0b0d0306796b2edf7b5b83.
//! Registry1002B013..1002B123, defaults10006890, WH10051D80/10006460.
//! Native instruction controls and boundaries: tools/rules_oracle/ares_armor.
//! Public master pre-fills every WH entry; this binary instead initializes and
//! overrides each index before the next one, so backward inheritance sees the
//! earlier override while a forward reference takes its own fallback record.

#[cfg(feature = "ares")]
use crate::rules::armor::ArmorInteraction;
use crate::rules::armor::{ArmorId, RETAIL_ARMOR_NAMES};
#[cfg(feature = "ares")]
use crate::rules::ini_parser::{IniFile, IniSection};
#[cfg(feature = "ares")]
use crate::rules::ini_value::truncate_native_bytes;
use crate::util::native_x87::NativeF64Bits;
#[cfg(feature = "ares")]
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Versus {
    multiplier: NativeF64Bits,
    force_fire: bool,
    retaliate: bool,
    passive_acquire: bool,
}

impl Default for Versus {
    fn default() -> Self {
        Self {
            multiplier: NativeF64Bits::ONE,
            force_fire: true,
            retaliate: true,
            passive_acquire: true,
        }
    }
}

#[cfg(feature = "ares")]
impl Versus {
    ///100523E0..100524B6. The numeric read is shared with the original
    /// Verses parser; only Ares's independent flag conventions are added.
    fn parse(token: &str) -> Self {
        let value = crate::rules::warhead_type::verses_value(token);
        let mut out = Self {
            multiplier: NativeF64Bits::from_bits(value.to_bits()),
            ..Self::default()
        };
        if token.contains('%') {
            // Native strlen counts this store's one-char-per-byte text.
            if token.chars().count() == 2 {
                match token.chars().next() {
                    Some('0') => {
                        out.force_fire = false;
                        out.retaliate = false;
                        out.passive_acquire = false;
                    }
                    Some('1') => {
                        out.retaliate = false;
                        out.passive_acquire = false;
                    }
                    Some('2') => out.passive_acquire = false,
                    _ => {}
                }
            }
        } else {
            // Live x87 subtract/compare, rather than rounded host f64
            // subtraction: the native .021 boundary is above .001.
            let greater = |base: f64| {
                X87::compare(
                    X87::sub(
                        X87::load_f64(out.multiplier),
                        X87::load_f64(NativeF64Bits::from_bits(base.to_bits())),
                    ),
                    X87::load_f64(NativeF64Bits::from_bits(0.001_f64.to_bits())),
                ) == MaskedX87Ordering::Greater
            };
            out.passive_acquire = greater(0.02);
            out.retaliate = out.passive_acquire || greater(0.01);
            out.force_fire = out.retaliate || greater(0.0);
        }
        out
    }

    fn interaction(self) -> ArmorInteraction {
        ArmorInteraction::ares(
            self.multiplier,
            self.force_fire,
            self.retaliate,
            self.passive_acquire,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct ArmorDefinition {
    name: String,
    default_index: Option<ArmorId>,
    default_verses: Versus,
}

/// Sole ordered armor catalog, retained by NativeRulesRegistryState's
/// ordered armor state. This wire type exists in both builds so a foreign
/// save is decoded and rejected by the existing capability check.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ArmorCatalog {
    armors: Vec<ArmorDefinition>,
}

impl Default for ArmorCatalog {
    fn default() -> Self {
        Self {
            armors: RETAIL_ARMOR_NAMES
                .into_iter()
                .map(|name| ArmorDefinition {
                    name: name.into(),
                    default_index: None,
                    default_verses: Versus::default(),
                })
                .collect(),
        }
    }
}

impl ArmorCatalog {
    pub(crate) fn len(&self) -> usize {
        self.armors.len()
    }

    pub(crate) fn contains(&self, armor: ArmorId) -> bool {
        armor.index() < self.armors.len()
    }

    pub(crate) fn valid_saved_state(&self) -> bool {
        self.armors.len() >= RETAIL_ARMOR_NAMES.len()
            && self.armors[..RETAIL_ARMOR_NAMES.len()]
                .iter()
                .zip(RETAIL_ARMOR_NAMES)
                .all(|(definition, name)| definition.name == name)
            && self.armors.iter().all(|armor| {
                armor.name.chars().count() < 32
                    && armor.default_index.is_none_or(|index| self.contains(index))
            })
    }

    #[cfg(feature = "ares")]
    fn find(&self, name: &str) -> Option<ArmorId> {
        self.armors
            .iter()
            .position(|armor| armor.name.eq_ignore_ascii_case(name))
            .map(ArmorId::from_index)
    }

    ///679A15's hook allocates all keys before reading any defaults.
    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(&mut self, pass: &IniFile) {
        let section = pass.section_or_empty("ArmorTypes");
        for name in section.keys() {
            if self.find(name).is_none() {
                self.armors.push(ArmorDefinition {
                    name: truncate_native_bytes(name, 31).into(),
                    default_index: None,
                    default_verses: Versus::default(),
                });
            }
        }
        for index in 0..self.armors.len() {
            let name = self.armors[index].name.as_str();
            if let Some(value) = section.read_name(name, 0x40) {
                let default_index = self.find(value);
                let default_verses = Versus::parse(value);
                self.armors[index].default_index = default_index;
                self.armors[index].default_verses = default_verses;
            }
        }
    }

    /// Common ObjectType Armor reader4753F0 -> Ares10006E10. Bind against
    /// this read's catalog: an unknown name becomes None0 immediately.
    #[cfg(feature = "ares")]
    pub(crate) fn read_binding(&self, section: &IniSection, current: ArmorId) -> ArmorId {
        section.read_name("Armor", 32).map_or(current, |name| {
            self.find(name).unwrap_or_else(|| {
                log::warn!("Unknown Ares armor {name:?} in [{}]", section.name);
                ArmorId::NONE
            })
        })
    }
}

/// One Warhead extension's independent vector. It never mirrors into the
/// original WH+A0 array or derives the original WH+149 shortcut flag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresWarheadVerses {
    entries: Vec<Versus>,
}

impl Default for AresWarheadVerses {
    fn default() -> Self {
        Self {
            entries: vec![Versus::default(); RETAIL_ARMOR_NAMES.len()],
        }
    }
}

impl AresWarheadVerses {
    pub(crate) fn valid_saved_state(&self, catalog: &ArmorCatalog) -> bool {
        // A type constructed after its family's body loop can still hold
        // only the11 constructor entries until the next reached read.
        self.entries.len() >= RETAIL_ARMOR_NAMES.len() && self.entries.len() <= catalog.len()
    }

    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(&mut self, section: Option<&IniSection>, catalog: &ArmorCatalog) {
        if let Some(tokens) = section.and_then(|section| section.read_list("Verses", 0x800)) {
            for (index, token) in tokens.into_iter().take(11).enumerate() {
                self.entries[index] = Versus::parse(token);
            }
        }
        for (index, armor) in catalog.armors.iter().enumerate() {
            if index == self.entries.len() {
                let inherited = armor
                    .default_index
                    .and_then(|index| self.entries.get(index.index()))
                    .copied()
                    .unwrap_or(armor.default_verses);
                self.entries.push(inherited);
            }
            let Some(section) = section else {
                continue;
            };
            let key = format!("Versus.{}", armor.name);
            if let Some(value) = section.read_name(&key, 32) {
                self.entries[index] = Versus::parse(value);
            }
            let entry = &mut self.entries[index];
            entry.force_fire = section.read_bool(&format!("{key}.ForceFire"), entry.force_fire);
            entry.retaliate = section.read_bool(&format!("{key}.Retaliate"), entry.retaliate);
            entry.passive_acquire =
                section.read_bool(&format!("{key}.PassiveAcquire"), entry.passive_acquire);
        }
    }

    #[cfg(feature = "ares")]
    pub(crate) fn interaction(&self, armor: ArmorId) -> ArmorInteraction {
        self.entries
            .get(armor.index())
            .expect("armor interaction requires a reached Warhead read")
            .interaction()
    }
}

#[cfg(test)]
#[path = "ares_armor_tests.rs"]
mod tests;
