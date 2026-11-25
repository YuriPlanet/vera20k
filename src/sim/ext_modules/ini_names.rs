//! INI names modules register (RFC 0001 section 6h):
//! keys on sections, registries and module events, and the engine's string
//! table (parameter texts, the start-of-match records' names and values).
//!
//! A module registers from `vera_init` only ([`IniNames::register`]): during
//! the rules stage for a module that exports `on_rules`
//! ([`super::PreparedExtModules`] holds the store until the simulation
//! exists), at install for the others. Both run before the INI triggers are
//! resolved and before the module phase hands out the start records
//! ([`crate::sim::world::Simulation::deliver_ini_start_records`]).
//!
//! The store is simulation state: saved and in the state hash, since a
//! loaded game runs no `vera_init` and resolves module events and strings
//! by it.

use vera20k_wasm_ext::{INI_EVENT, INI_KEY, INI_REGISTRY, Invalid, MAX_INI_REGISTRATIONS, Status};

use super::ModuleSource;

/// One registered name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Registration {
    pub(crate) source: ModuleSource,
    /// `IniKind`.
    pub(crate) kind: i32,
    /// `IniSection` for a key; 0 object, 1 global for an event; 0 for a
    /// registry.
    pub(crate) section: i32,
    pub(crate) name: String,
    /// The handler's dispatch key (keys and registries); the event ID for
    /// an event.
    pub(crate) key: i32,
}

/// What `ini_register` registered and the strings records carry.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct IniNames {
    /// In registration order.
    registrations: Vec<Registration>,
    /// The event registrations' indices in `registrations`; an event's ID
    /// is its position here.
    events: Vec<usize>,
    /// String ID `n` is `strings[n - 1]`; 0 is none. Only grows.
    strings: Vec<String>,
}

impl IniNames {
    pub(crate) fn is_empty(&self) -> bool {
        self.registrations.is_empty() && self.strings.is_empty()
    }

    /// Registers `name` (its form and `section` already checked by the
    /// sandbox). `Refused` when its kind already has the name, from any
    /// source, or it starts with `Vera.`.
    pub(crate) fn register(
        &mut self,
        source: ModuleSource,
        kind: i32,
        section: i32,
        name: &str,
        key: i32,
    ) -> Result<(i32, Status), Invalid> {
        if !matches!(kind, INI_KEY | INI_REGISTRY | INI_EVENT) {
            return Err(Invalid(format!("unknown IniKind {kind}")));
        }
        let mine = self
            .registrations
            .iter()
            .filter(|r| r.source == source)
            .count();
        if mine >= MAX_INI_REGISTRATIONS {
            return Err(Invalid(format!(
                "more than {MAX_INI_REGISTRATIONS} INI registrations"
            )));
        }
        let reserved = name
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Vera."));
        if reserved || self.find(kind, name).is_some() {
            return Ok((0, Status::Refused));
        }
        let key = match kind {
            INI_EVENT => {
                self.events.push(self.registrations.len());
                self.events.len() as i32 - 1
            }
            _ => key,
        };
        self.registrations.push(Registration {
            source,
            kind,
            section,
            name: name.to_owned(),
            key,
        });
        Ok((if kind == INI_EVENT { key } else { 0 }, Status::Ok))
    }

    fn find(&self, kind: i32, name: &str) -> Option<&Registration> {
        self.registrations
            .iter()
            .find(|r| r.kind == kind && r.name.eq_ignore_ascii_case(name))
    }

    /// Registrations of `source` and `kind`, in registration order.
    pub(crate) fn of(
        &self,
        source: ModuleSource,
        kind: i32,
    ) -> impl Iterator<Item = &Registration> + '_ {
        self.registrations
            .iter()
            .filter(move |r| r.source == source && r.kind == kind)
    }

    /// A registered event's ID by name, from any source.
    pub(crate) fn event_id(&self, name: &str) -> Option<i32> {
        self.find(INI_EVENT, name).map(|r| r.key)
    }

    /// A registered event by ID.
    pub(crate) fn event(&self, id: i32) -> Option<&Registration> {
        let index = *self.events.get(usize::try_from(id).ok()?)?;
        self.registrations.get(index)
    }

    /// Whether event `id` was registered as global.
    pub(crate) fn event_is_global(&self, id: i32) -> Option<bool> {
        self.event(id).map(|r| r.section == 1)
    }

    /// The string ID of `text`, added when new; 0 for empty text.
    pub(crate) fn intern(&mut self, text: &str) -> i32 {
        if text.is_empty() {
            return 0;
        }
        match self.string_id(text) {
            Some(id) => id,
            None => {
                self.strings.push(text.to_owned());
                self.strings.len() as i32
            }
        }
    }

    /// An existing string's ID.
    pub(crate) fn string_id(&self, text: &str) -> Option<i32> {
        self.strings
            .iter()
            .position(|s| s == text)
            .map(|i| i as i32 + 1)
    }

    pub(crate) fn string(&self, id: i32) -> Option<&str> {
        let index = usize::try_from(id.checked_sub(1)?).ok()?;
        self.strings.get(index).map(String::as_str)
    }

    /// Bytes for the state hash and the multiplayer checksum.
    pub(crate) fn state_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        let registrations = self.registrations.iter().flat_map(|r| {
            [r.source as u8, r.kind as u8, r.section as u8]
                .into_iter()
                .chain(r.key.to_le_bytes())
                .chain(r.name.bytes())
                .chain([0])
        });
        let strings = self.strings.iter().flat_map(|s| s.bytes().chain([0]));
        registrations.chain([0xff]).chain(strings)
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_per_kind_across_sources() {
        let mut names = IniNames::default();
        assert_eq!(
            names.register(ModuleSource::Mod, INI_KEY, 0, "Kratos.Shield", 7),
            Ok((0, Status::Ok))
        );
        // Another kind may share the name; the same kind may not.
        assert_eq!(
            names.register(ModuleSource::Mod, INI_EVENT, 0, "kratos.shield", -1),
            Ok((0, Status::Ok))
        );
        assert_eq!(
            names.register(ModuleSource::Map, INI_KEY, 1, "KRATOS.SHIELD", 3),
            Ok((0, Status::Refused))
        );
        assert_eq!(
            names.register(ModuleSource::Map, INI_EVENT, 1, "Map.Wave", -1),
            Ok((1, Status::Ok))
        );
        assert_eq!(
            names.register(ModuleSource::Map, INI_REGISTRY, 0, "vera.Things", 1),
            Ok((0, Status::Refused))
        );
        assert_eq!(names.event_id("map.wave"), Some(1));
        assert_eq!(names.event_is_global(1), Some(true));
        assert_eq!(names.event_is_global(0), Some(false));
        assert_eq!(names.of(ModuleSource::Mod, INI_KEY).count(), 1);
    }

    #[test]
    fn strings_intern_from_one() {
        let mut names = IniNames::default();
        assert_eq!(names.intern(""), 0);
        assert_eq!(names.intern("50"), 1);
        assert_eq!(names.intern("HeavyShield"), 2);
        assert_eq!(names.intern("50"), 1);
        assert_eq!(names.string(2), Some("HeavyShield"));
        assert_eq!(names.string(0), None);
        assert_eq!(names.string_id("HeavyShield"), Some(2));
    }
}
