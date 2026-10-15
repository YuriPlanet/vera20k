//! Retained StripClass entry order: CompareItems 6A8420, InsertEntry 6A8710,
//! AddCameo 6A6300 and Recalculate 6AA600. Native comparisons live in
//! tools/sidebar_oracle/cameo_order.py.

use std::collections::HashMap;

pub(crate) use crate::sim::snapshot::SavedCameoId as CameoId;

use super::SidebarTab;

/// Current comparison inputs, projected from the rules, local House Cost_Of
/// and the process string table. They are never retained as gameplay state.
#[derive(Debug, Clone)]
pub(crate) struct Cameo {
    pub id: CameoId,
    pub tab: SidebarTab,
    pub available: bool,
    pub key: CameoKey,
}

#[derive(Debug, Clone)]
pub(crate) enum CameoKey {
    SuperWeapon {
        recharge_frames: i32,
        name: String,
    },
    Techno {
        own_side: bool,
        /// These flags apply only to UnitType/AircraftType (RTTI 40/3).
        considered_aircraft: bool,
        naval: bool,
        tech_level: i32,
        cost: i32,
        name: String,
    },
}

impl CameoKey {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::SuperWeapon { name, .. } | Self::Techno { name, .. } => name,
        }
    }

    /// Native returns true on an equal name, so this is an insertion
    /// predicate, not a Rust sorting comparator. Both vehicle flags being
    /// set also makes the relation non-strict before the numeric keys.
    pub(crate) fn inserts_before(&self, other: &Self) -> bool {
        let (left_name, right_name) = match (self, other) {
            (
                Self::SuperWeapon {
                    recharge_frames: a,
                    name: an,
                },
                Self::SuperWeapon {
                    recharge_frames: b,
                    name: bn,
                },
            ) => {
                if a != b {
                    return a < b;
                }
                (an, bn)
            }
            (Self::SuperWeapon { .. }, Self::Techno { .. }) => return true,
            (Self::Techno { .. }, Self::SuperWeapon { .. }) => return false,
            (
                Self::Techno {
                    own_side: a_side,
                    considered_aircraft: a_air,
                    naval: a_naval,
                    tech_level: a_level,
                    cost: a_cost,
                    name: an,
                },
                Self::Techno {
                    own_side: b_side,
                    considered_aircraft: b_air,
                    naval: b_naval,
                    tech_level: b_level,
                    cost: b_cost,
                    name: bn,
                },
            ) => {
                if a_side != b_side {
                    return *a_side;
                }
                let a_ground = !a_air && !a_naval;
                let b_ground = !b_air && !b_naval;
                if a_ground {
                    if *b_air {
                        return true;
                    }
                } else if *a_air {
                    if b_ground {
                        return false;
                    }
                } else if *a_naval && (b_ground || *b_air) {
                    return false;
                }
                if (a_ground || *a_air) && *b_naval {
                    return true;
                }
                if a_level != b_level {
                    return a_level < b_level;
                }
                if a_cost != b_cost {
                    return a_cost < b_cost;
                }
                (an, bn)
            }
        };
        // CRT wcscmp 7CA5D3 compares Windows UTF-16 code units, stopping at
        // the first NUL. Unicode scalar-value / UTF-8 order differs for
        // supplementary characters versus BMP characters above D7FF.
        left_name
            .encode_utf16()
            .take_while(|unit| *unit != 0)
            .cmp(right_name.encode_utf16().take_while(|unit| *unit != 0))
            != std::cmp::Ordering::Greater
    }
}

/// The single retained order of the four sidebar strips. Rules/admission
/// projections supply the current members and comparison inputs; surviving
/// entries keep their positions, even if costs or other keys change. Match
/// replacement and local-house replacement discard this derived projection.
#[derive(Debug, Clone, Default)]
pub(crate) struct CameoStrips {
    strips: [Vec<Cameo>; 4],
    scroll_rows: [usize; 4],
    /// Strip entry +88 survives key refresh and Mouse save/no-init. Entries
    /// removed by Recalculate discard their deadline with their identity.
    flash_deadlines: HashMap<CameoId, i32>,
    redraw_until: i32,
    frame: i32,
}

impl CameoStrips {
    /// Restore validated identities in their saved order; never reinsert or sort.
    /// MouseClass Load 5BDF70 and NoInit 6A4F20 preserve these records.
    pub(crate) fn from_retained(strips: [Vec<Cameo>; 4], scroll_rows: [usize; 4]) -> Self {
        Self {
            strips,
            scroll_rows,
            ..Self::default()
        }
    }

    pub(crate) fn set_frame(&mut self, frame: i32) {
        self.frame = frame;
    }

    /// Strip AI6A8CF3..6A8D02 requests drawing while Sidebar+53A0 is
    /// strictly after the signed native frame. The extra deadline frame
    /// written by6AC728 includes the entry's final clearing draw.
    pub(crate) fn redraw_requested(&self, frame: i32) -> bool {
        self.redraw_until > frame
    }

    /// TAction114 6E4100 -> Strip6ABAC0 admits only a nonempty strip.
    pub(crate) fn selectable_tab(&self, index: i32) -> Option<SidebarTab> {
        let tab = *SidebarTab::all().get(usize::try_from(index).ok()?)?;
        (!self.items(tab).is_empty()).then_some(tab)
    }

    /// TAction115 -> Sidebar6AC5F0 matches the first retained numeric type
    /// index, deliberately without comparing RTTI (Unit/Aircraft can collide).
    /// Original-executed controls: tools/sidebar_oracle/trigger_cameos.json.
    pub(crate) fn flash_type_index(
        &mut self,
        tab: SidebarTab,
        type_index: i32,
        duration: i32,
        frame: i32,
        mut index_of: impl FnMut(CameoId) -> Option<i32>,
    ) {
        let deadline = frame.wrapping_add(duration);
        if let Some(entry) = self.strips[tab.tab_index()]
            .iter()
            .take(75)
            .find(|entry| index_of(entry.id) == Some(type_index))
        {
            self.flash_deadlines.insert(entry.id, deadline);
        }
        self.redraw_until = deadline.wrapping_add(1);
    }

    /// StripDraw6A9B59..6A9B76 uses signed deadline/frame and signed IDIV,
    /// not an unsigned clock or an elapsed-time approximation.
    pub(crate) fn flash_darkened(&self, id: CameoId) -> bool {
        self.flash_deadlines.get(&id).copied().unwrap_or(0) > self.frame && self.frame % 16 > 8
    }

    pub(crate) fn saved_flash(&self) -> ([Vec<i32>; 4], i32) {
        (
            std::array::from_fn(|index| {
                self.strips[index]
                    .iter()
                    .map(|entry| self.flash_deadlines.get(&entry.id).copied().unwrap_or(0))
                    .collect()
            }),
            self.redraw_until,
        )
    }

    /// Called after the saved-order owner validates counts and identities.
    pub(crate) fn restore_flash(&mut self, deadlines: [Vec<i32>; 4], redraw_until: i32) {
        self.flash_deadlines.clear();
        for (strip, deadlines) in self.strips.iter().zip(deadlines) {
            for (entry, deadline) in strip.iter().zip(deadlines) {
                self.flash_deadlines.insert(entry.id, deadline);
            }
        }
        self.redraw_until = redraw_until;
    }

    pub(crate) fn scroll_rows(&self) -> [usize; 4] {
        self.scroll_rows
    }

    pub(crate) fn scroll_row(&self, tab: SidebarTab) -> usize {
        self.scroll_rows[tab.tab_index()]
    }

    pub(crate) fn set_scroll_row(&mut self, tab: SidebarTab, row: usize) {
        self.scroll_rows[tab.tab_index()] = row;
    }

    pub(crate) fn saved_identities(&self) -> [Vec<CameoId>; 4] {
        std::array::from_fn(|index| self.strips[index].iter().map(|entry| entry.id).collect())
    }

    pub(crate) fn items(&self, tab: SidebarTab) -> &[Cameo] {
        &self.strips[tab.tab_index()]
    }

    /// Refresh the pointed-to type/House values without moving entries.
    pub(crate) fn refresh_keys(&mut self, candidates: &[Cameo]) {
        let current: HashMap<_, _> = candidates.iter().map(|item| (item.id, item)).collect();
        for entry in self.strips.iter_mut().flatten() {
            if let Some(item) = current.get(&entry.id) {
                entry.clone_from(item);
            }
        }
    }

    /// AddCameo rejects a duplicate identity and count above 75. Equal keys
    /// enter before the existing item. Returns whether insertion succeeded.
    /// Native allocates only 75 records, so its accepted 76th entry can
    /// overwrite adjacent storage. Vec preserves that admission boundary
    /// safely; it does not reproduce the overwrite.
    pub(crate) fn insert(&mut self, candidate: &Cameo) -> bool {
        let strip = &mut self.strips[candidate.tab.tab_index()];
        if strip.len() > 75 || strip.iter().any(|entry| entry.id == candidate.id) {
            return false;
        }
        let at = strip
            .iter()
            .position(|entry| candidate.key.inserts_before(&entry.key))
            .unwrap_or(strip.len());
        strip.insert(at, candidate.clone());
        true
    }

    /// Recalculate6AA600 keeps surviving order and adjusts TopRow from its
    /// old visible snapshot (6AABC9..6AAC76). The signed divisions truncate
    /// toward zero, including the odd-count upper bound. This deliberately
    /// preserves the native adjustment even when only a later item disappears.
    /// Original-executed controls: tools/sidebar_oracle/cameo_order.json.
    pub(crate) fn retain(&mut self, visible_rows: usize, mut keep: impl FnMut(CameoId) -> bool) {
        let visible_slots = visible_rows * 2;
        for (strip, row) in self.strips.iter_mut().zip(&mut self.scroll_rows) {
            let old_count = strip.len();
            let start = *row * 2;
            let mut old_index = 0;
            let mut new_index = 0;
            let mut first_visible = None;
            strip.retain(|entry| {
                let retained = keep(entry.id);
                if retained {
                    if first_visible.is_none()
                        && old_index >= start
                        && old_index - start < visible_slots
                    {
                        first_visible = Some((old_index - start, new_index));
                    }
                    new_index += 1;
                }
                old_index += 1;
                retained
            });
            if strip.len() != old_count {
                *row = first_visible.map_or(0, |(snapshot_index, retained_index)| {
                    let shifted = *row as i64 - (snapshot_index as i64 - retained_index as i64) / 2;
                    let last = (strip.len() as i64 - visible_slots as i64) / 2;
                    shifted.min(last).max(0) as usize
                });
            }
        }
        self.flash_deadlines
            .retain(|id, _| self.strips.iter().flatten().any(|entry| entry.id == *id));
    }

    /// House4F927C..4F92FD adds ordinary cameos before recalculating the
    /// strips, then grants Supers. Visibility still comes from the existing
    /// production admission owner; this projection does not invent CanBuild.
    pub(crate) fn reconcile(&mut self, candidates: &[Cameo], visible_rows: usize) -> bool {
        self.refresh_keys(candidates);
        let mut inserted_build = false;
        for candidate in candidates
            .iter()
            .filter(|item| item.available && matches!(item.id, CameoId::Object(_)))
        {
            inserted_build |= self.insert(candidate);
        }
        let available: std::collections::HashSet<_> = candidates
            .iter()
            .filter(|item| item.available)
            .map(|item| item.id)
            .collect();
        self.retain(visible_rows, |id| available.contains(&id));
        for candidate in candidates
            .iter()
            .filter(|item| item.available && matches!(item.id, CameoId::SuperWeapon(_)))
        {
            self.insert(candidate);
        }
        inserted_build
    }

    /// Layout fixtures explicitly supply order; they do not have a rules
    /// database or claim to exercise admission/comparison.
    #[cfg(test)]
    pub(crate) fn layout_fixture(
        options: &[crate::sim::production::BuildOption],
        ready: &[crate::sim::production::ReadyBuildingView],
        supers: &[crate::sim::superweapon::SuperWeaponView],
    ) -> Self {
        let mut result = Self::default();
        result.strips[SidebarTab::Defense.tab_index()].extend(supers.iter().map(|item| Cameo {
            id: CameoId::SuperWeapon(item.type_id),
            tab: SidebarTab::Defense,
            available: true,
            key: CameoKey::SuperWeapon {
                recharge_frames: 0,
                name: item.display_name.clone(),
            },
        }));
        for (id, category, name, cost) in options
            .iter()
            .filter(|item| item.visible_in_sidebar())
            .map(|item| {
                (
                    item.type_id,
                    item.queue_category,
                    &item.display_name,
                    item.cost,
                )
            })
            .chain(
                ready
                    .iter()
                    .map(|item| (item.type_id, item.queue_category, &item.display_name, 0)),
            )
        {
            let strip = &mut result.strips[SidebarTab::for_category(category).tab_index()];
            if !strip.iter().any(|entry| entry.id == CameoId::Object(id)) {
                strip.push(Cameo {
                    id: CameoId::Object(id),
                    tab: SidebarTab::for_category(category),
                    available: true,
                    key: CameoKey::Techno {
                        own_side: true,
                        considered_aircraft: false,
                        naval: false,
                        tech_level: 0,
                        cost,
                        name: name.clone(),
                    },
                });
            }
        }
        result
    }
}
