//! Command permissions by admitted source and native House slot.
//!
//! A source's primary House comes from the session's source roster. The
//! extra grants are fixed at admission. The lookup table is derived from
//! those grants and the live House array.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::util::lookup_hash::LookupMap;

const MAX_SOURCE_COUNT: usize = i8::MAX as usize + 1;

/// A requested grant for a source and a House other than its primary House.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ControlGrant {
    pub source: i8,
    pub house: u8,
    pub level: ControlGrantLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ControlGrantLevel {
    FullControl,
    UnitsOnly,
}

impl ControlGrantLevel {
    const fn scope(self) -> ControlScope {
        match self {
            Self::FullControl => ControlScope::FullControl,
            Self::UnitsOnly => ControlScope::UnitsOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlGrantError {
    NotLockstep,
    ScenarioAlreadyStarted,
    GrantsAlreadyAdmitted,
    InvalidSource(i8),
    InvalidSourceCount { admitted: usize, available: usize },
    InvalidHouse(u8),
    OwnHouse { source: i8, house: u8 },
    ConflictingLevels { source: i8, house: u8 },
}

impl fmt::Display for ControlGrantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NotLockstep => f.write_str("explicit grants require a lockstep launch"),
            Self::ScenarioAlreadyStarted => {
                f.write_str("control grants must be admitted before the scenario starts")
            }
            Self::GrantsAlreadyAdmitted => {
                f.write_str("the scenario already has admitted control grants")
            }
            Self::InvalidSource(source) => write!(f, "source {source} is not admitted"),
            Self::InvalidSourceCount {
                admitted,
                available,
            } => write!(
                f,
                "saved control authority has {admitted} source rows for a roster of {available}"
            ),
            Self::InvalidHouse(house) => write!(f, "House slot {house} is not registered"),
            Self::OwnHouse { source, house } => {
                write!(
                    f,
                    "House slot {house} is already source {source}'s primary House"
                )
            }
            Self::ConflictingLevels { source, house } => {
                write!(
                    f,
                    "source {source} has conflicting grants for House slot {house}"
                )
            }
        }
    }
}

impl std::error::Error for ControlGrantError {}

/// An admitted source's authority over a House.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlScope {
    None,
    OwnFull,
    FullControl,
    UnitsOnly,
}

/// The authority a synchronized command needs on each of its subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandCapability {
    PrimaryHouse,
    FullControl,
    Tactical,
}

impl ControlScope {
    pub(crate) const fn permits(self, capability: CommandCapability) -> bool {
        use CommandCapability::{FullControl, PrimaryHouse, Tactical};

        match self {
            Self::None => false,
            Self::OwnFull => true,
            Self::FullControl => match capability {
                PrimaryHouse => false,
                FullControl | Tactical => true,
            },
            Self::UnitsOnly => match capability {
                PrimaryHouse | FullControl => false,
                Tactical => true,
            },
        }
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ControlAuthority {
    delegated: Option<Box<ControlGrantTable>>,
}

impl Clone for ControlAuthority {
    fn clone(&self) -> Self {
        Self {
            delegated: self.delegated.as_ref().map(|grants| {
                Box::new(ControlGrantTable {
                    entries: Arc::clone(&grants.entries),
                    source_count: grants.source_count,
                    scopes: RwLock::default(),
                })
            }),
        }
    }
}

impl ControlAuthority {
    /// Validate and normalize grants against the source roster and live registration slots.
    /// The caller's roster lookup resolves admitted sources to their primary House.
    pub(crate) fn admit(
        mut grants: Vec<ControlGrant>,
        house_count: usize,
        source_count: usize,
        source_house: impl Fn(i8) -> Option<usize>,
    ) -> Result<Self, ControlGrantError> {
        if grants.is_empty() {
            return Ok(Self::default());
        }
        let source_count = source_count.min(MAX_SOURCE_COUNT);
        grants.sort_unstable_by_key(|grant| (grant.source, grant.house));
        grants.dedup();
        Self::validate_entries(&grants, house_count, source_count, source_house)?;
        Ok(Self {
            delegated: Some(Box::new(ControlGrantTable {
                entries: grants.into(),
                source_count,
                scopes: RwLock::default(),
            })),
        })
    }

    /// Validate decoded authority against the restored, append-only House
    /// registry. The original source width can be smaller than a growing
    /// House roster. An explicit shared-source roster never changes width.
    pub(crate) fn validate_restored(
        &self,
        house_count: usize,
        live_source_count: usize,
        shared_sources: bool,
        source_house: impl Fn(i8) -> Option<usize>,
    ) -> Result<(), ControlGrantError> {
        let Some(grants) = &self.delegated else {
            return Ok(());
        };
        let available = live_source_count.min(MAX_SOURCE_COUNT);
        if grants.source_count > available || shared_sources && grants.source_count != available {
            return Err(ControlGrantError::InvalidSourceCount {
                admitted: grants.source_count,
                available,
            });
        }
        Self::validate_entries(
            &grants.entries,
            house_count,
            grants.source_count,
            source_house,
        )
    }

    fn validate_entries(
        grants: &[ControlGrant],
        house_count: usize,
        source_count: usize,
        source_house: impl Fn(i8) -> Option<usize>,
    ) -> Result<(), ControlGrantError> {
        let mut previous = None;
        for &ControlGrant { source, house, .. } in grants {
            let Some(primary) = usize::try_from(source)
                .ok()
                .filter(|&row| row < source_count)
                .and_then(|_| source_house(source))
                .filter(|&primary| primary < house_count)
            else {
                return Err(ControlGrantError::InvalidSource(source));
            };
            let target = usize::from(house);
            if target >= house_count {
                return Err(ControlGrantError::InvalidHouse(house));
            }
            if target == primary {
                return Err(ControlGrantError::OwnHouse { source, house });
            }
            if previous.replace((source, house)) == Some((source, house)) {
                return Err(ControlGrantError::ConflictingLevels { source, house });
            }
        }
        Ok(())
    }

    pub(crate) const fn has_any_grants(&self) -> bool {
        self.delegated.is_some()
    }

    pub(crate) fn admitted_source_count(&self) -> Option<usize> {
        self.delegated.as_ref().map(|table| table.source_count)
    }

    pub(crate) fn grants(&self) -> &[ControlGrant] {
        match &self.delegated {
            Some(grants) => &grants.entries,
            None => &[],
        }
    }

    /// Share a width-checked table with a projection for one immutable world read.
    pub(crate) fn scope_table(&self, houses: &Houses) -> Option<Arc<ControlScopeTable>> {
        self.delegated
            .as_ref()?
            .with_scope_table(houses, Arc::clone)
    }

    /// The caller resolves the admitted source to its primary House before
    /// asking about a command's live subject.
    pub(crate) fn scope(
        &self,
        source: i8,
        primary: InternedId,
        subject: InternedId,
        houses: &Houses,
    ) -> ControlScope {
        let Ok(source) = usize::try_from(source) else {
            return ControlScope::None;
        };
        match &self.delegated {
            Some(grants) if source >= grants.source_count => ControlScope::None,
            _ if primary == subject => ControlScope::OwnFull,
            Some(grants) => grants.scope(source, subject, houses),
            None => ControlScope::None,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ControlGrantTable {
    #[serde(serialize_with = "save_grants", deserialize_with = "load_grants")]
    entries: Arc<[ControlGrant]>,
    #[serde(deserialize_with = "load_source_count")]
    source_count: usize,
    // The live House registry owns the width. A restored table builds the
    // corresponding scope cache only when it is first queried.
    #[serde(skip)]
    scopes: RwLock<Option<Arc<ControlScopeTable>>>,
}

fn save_grants<S: serde::Serializer>(
    entries: &Arc<[ControlGrant]>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(entries.as_ref(), serializer)
}

fn load_grants<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<[ControlGrant]>, D::Error> {
    let entries = <Vec<ControlGrant> as serde::Deserialize>::deserialize(deserializer)?;
    let key = |entry: &ControlGrant| (entry.source, entry.house);
    if entries.first().is_none_or(|entry| entry.source < 0)
        || entries
            .windows(2)
            .any(|pair| key(&pair[0]) >= key(&pair[1]))
    {
        return Err(serde::de::Error::custom(
            "invalid canonical control grant table",
        ));
    }
    Ok(entries.into())
}

fn load_source_count<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    let count = <usize as serde::Deserialize>::deserialize(deserializer)?;
    if count > MAX_SOURCE_COUNT || count == 0 && !crate::util::save_codec::layout::describing() {
        return Err(serde::de::Error::custom(
            "control grant source count is out of range",
        ));
    }
    Ok(count)
}

impl ControlGrantTable {
    fn scope(&self, source: usize, subject: InternedId, houses: &Houses) -> ControlScope {
        self.with_scope_table(houses, |table| table.get(source, subject))
            .unwrap_or(ControlScope::None)
    }

    fn with_scope_table<T>(
        &self,
        houses: &Houses,
        read: impl FnOnce(&Arc<ControlScopeTable>) -> T,
    ) -> Option<T> {
        // House slots are append-only in a running match. A different live
        // count requires a new stride and name-to-slot mapping.
        let current_size = |table: &&Arc<ControlScopeTable>| table.house_count == houses.len();
        let scopes = self.scopes.read().unwrap_or_else(PoisonError::into_inner);
        if let Some(table) = scopes.as_ref().filter(current_size) {
            return Some(read(table));
        }
        drop(scopes);

        let mut scopes = self.scopes.write().unwrap_or_else(PoisonError::into_inner);
        // Another reader may have refreshed it while we acquired the write lock.
        if scopes.as_ref().filter(current_size).is_none() {
            *scopes =
                ControlScopeTable::build(&self.entries, self.source_count, houses).map(Arc::new);
        }
        scopes.as_ref().map(read)
    }
}

#[derive(Debug)]
pub(crate) struct ControlScopeTable {
    house_count: usize,
    columns: LookupMap<InternedId, usize>,
    cells: Box<[ControlScope]>,
}

impl ControlScopeTable {
    fn build(grants: &[ControlGrant], source_count: usize, houses: &Houses) -> Option<Self> {
        let house_count = houses.len();
        let mut cells = vec![ControlScope::None; source_count.checked_mul(house_count)?];
        for &ControlGrant {
            source,
            house,
            level,
        } in grants
        {
            let source = usize::try_from(source).ok()?;
            let house = usize::from(house);
            // Both dimensions are checked so an absent House slot cannot
            // address a cell in the next source's row.
            if source < source_count && house < house_count {
                cells[source * house_count + house] = level.scope();
            }
        }
        let columns = houses
            .order()
            .iter()
            .copied()
            .enumerate()
            .map(|(index, name)| (name, index))
            .collect();
        let table = Self {
            house_count,
            columns,
            cells: cells.into_boxed_slice(),
        };
        debug_assert!(grants.iter().all(|grant| {
            houses
                .order()
                .get(usize::from(grant.house))
                .is_none_or(|&subject| {
                    table.get(grant.source as usize, subject) == grant.level.scope()
                })
        }));
        Some(table)
    }

    pub(crate) fn get(&self, source: usize, subject: InternedId) -> ControlScope {
        let Some(&column) = self.columns.get(&subject) else {
            return ControlScope::None;
        };
        self.cells
            .get(source * self.house_count + column)
            .copied()
            .unwrap_or(ControlScope::None)
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;
    use crate::sim::house_state::HouseState;

    fn make_house(name: u32) -> HouseState {
        HouseState::new(InternedId::from_index(name), 0, None, false, 0, 10)
    }

    fn grant(source: i8, house: u8, level: ControlGrantLevel) -> ControlGrant {
        ControlGrant {
            source,
            house,
            level,
        }
    }

    #[test]
    fn shared_sources_keep_their_own_grants_when_a_house_is_registered() {
        use ControlGrantLevel::{FullControl, UnitsOnly};
        let mut houses: Houses = [20, 8, 11].into_iter().map(make_house).collect();
        let primary = InternedId::from_index(20);
        let first = InternedId::from_index(8);
        let second = InternedId::from_index(11);
        let authority = ControlAuthority::admit(
            vec![
                grant(1, 1, UnitsOnly),
                grant(0, 2, FullControl),
                grant(0, 1, FullControl),
                grant(1, 1, UnitsOnly),
            ],
            houses.len(),
            2,
            |_| Some(0),
        )
        .unwrap();
        assert_eq!(
            authority.grants(),
            [
                grant(0, 1, FullControl),
                grant(0, 2, FullControl),
                grant(1, 1, UnitsOnly),
            ]
        );

        assert_eq!(
            authority.scope(0, primary, first, &houses),
            ControlScope::FullControl
        );
        assert_eq!(
            authority.scope(1, primary, first, &houses),
            ControlScope::UnitsOnly
        );
        assert_eq!(
            authority.scope(0, primary, second, &houses),
            ControlScope::FullControl
        );
        assert_eq!(
            authority.scope(1, primary, second, &houses),
            ControlScope::None
        );

        let late = houses.register(make_house(3)).name();
        assert_eq!(
            authority.scope(1, primary, late, &houses),
            ControlScope::None
        );
        assert_eq!(
            authority.scope(1, primary, first, &houses),
            ControlScope::UnitsOnly
        );
        assert_eq!(
            authority.scope(0, primary, second, &houses),
            ControlScope::FullControl
        );
        let scopes = authority.delegated.as_ref().unwrap().scopes.read().unwrap();
        let table = scopes.as_ref().unwrap();
        assert_eq!(table.house_count, 4);
        assert_eq!(table.cells.len(), 2 * table.house_count);
        assert_eq!(table.columns.get(&late), Some(&3));
        assert!(
            authority
                .clone()
                .delegated
                .unwrap()
                .scopes
                .into_inner()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn empty_admission_keeps_stock_authority_without_building_a_table() {
        let houses: Houses = [20, 8].into_iter().map(make_house).collect();
        let primary = InternedId::from_index(20);
        let other = InternedId::from_index(8);
        let authority = ControlAuthority::admit(Vec::new(), houses.len(), 2, |_| {
            panic!("empty admission needs no source roster lookup")
        })
        .unwrap();

        assert!(!authority.has_any_grants());
        assert_eq!(
            authority.scope(0, primary, primary, &houses),
            ControlScope::OwnFull
        );
        assert_eq!(
            authority.scope(0, primary, other, &houses),
            ControlScope::None
        );
    }

    #[test]
    fn admission_rejects_ambiguous_or_unbound_grants() {
        use ControlGrantLevel::{FullControl, UnitsOnly};
        let admit = |entries| {
            ControlAuthority::admit(entries, 2, 3, |source| {
                (source == 0 || source == 2).then_some(0)
            })
        };
        assert_eq!(
            admit(vec![grant(-1, 1, UnitsOnly)]).unwrap_err(),
            ControlGrantError::InvalidSource(-1)
        );
        assert_eq!(
            admit(vec![grant(1, 1, UnitsOnly)]).unwrap_err(),
            ControlGrantError::InvalidSource(1)
        );
        assert_eq!(
            ControlAuthority::admit(vec![grant(3, 1, UnitsOnly)], 2, 3, |_| Some(0)).unwrap_err(),
            ControlGrantError::InvalidSource(3)
        );
        assert_eq!(
            admit(vec![grant(2, 2, FullControl)]).unwrap_err(),
            ControlGrantError::InvalidHouse(2)
        );
        assert_eq!(
            admit(vec![grant(2, 0, UnitsOnly)]).unwrap_err(),
            ControlGrantError::OwnHouse {
                source: 2,
                house: 0
            }
        );
        assert_eq!(
            admit(vec![grant(0, 1, FullControl), grant(0, 1, UnitsOnly)]).unwrap_err(),
            ControlGrantError::ConflictingLevels {
                source: 0,
                house: 1
            }
        );
    }
}
