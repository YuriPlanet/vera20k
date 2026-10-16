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

/// The tab-state consequences of one reconcile pass.
///
/// Native flips a tab button and moves ActiveTab only on these transitions:
/// `AddCameo` 6A6300 enables the added strip's button and, when no strip held
/// any entry before the insertion, switches ActiveTab to the first populated
/// strip; `Recalculate` 6AA600 disables the button of the strip it just
/// emptied and then switches ActiveTab to the first populated strip when that
/// is not already the active one. Nothing re-derives the buttons or the active
/// tab from the current strip contents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CameoTransitions {
    build_entered: [bool; 4],
    emptied: [bool; 4],
    super_entered: [bool; 4],
    active_tab_destination: Option<SidebarTab>,
}

impl CameoTransitions {
    /// The tab-button state this pass left for `tab`, or `None` when the pass
    /// did not touch that button. One strip can see several native events, so
    /// they apply in their native order: AddCameo enables, Recalculate
    /// disables, and a super granted afterwards enables again.
    pub(crate) fn tab_button_disabled(&self, tab: SidebarTab) -> Option<bool> {
        let index = tab.tab_index();
        if self.super_entered[index] {
            Some(false)
        } else if self.emptied[index] {
            Some(true)
        } else if self.build_entered[index] {
            Some(false)
        } else {
            None
        }
    }

    /// The last tab selected by an AddCameo/Recalculate transition in this
    /// pass, preserving native event order across ordinary and super entries.
    pub(crate) fn active_tab_destination(&self) -> Option<SidebarTab> {
        self.active_tab_destination
    }
}

/// One `AddCameo`/`Recalculate` pass: the construction-options line's insertion
/// flag plus the tab transitions the pass produced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CameoReconcile {
    build_inserted: bool,
    transitions: CameoTransitions,
}

impl CameoReconcile {
    pub(crate) fn build_inserted(&self) -> bool {
        self.build_inserted
    }

    pub(crate) fn transitions(&self) -> CameoTransitions {
        self.transitions
    }

    /// The scenario's first projection is not an insertion for the
    /// construction-options line, but its tab transitions still happen.
    pub(crate) fn without_build_insertion(mut self) -> Self {
        self.build_inserted = false;
        self
    }
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
    /// strips, then grants Supers. Ordinary membership changes only on that
    /// House pass (or initial seed). StripAI6A8B30 and Draw6A9540 refresh
    /// decoration without pruning when FindFactory starts rejecting Selling.
    /// Visibility still comes from the existing production admission owner.
    ///
    /// The reported transitions follow the same pass: the ordinary additions
    /// run first, `retain` prunes (Recalculate 6AA600), and only then do newly
    /// granted supers enter — so a super granted into a sidebar the prune had
    /// just emptied still counts as an insertion into an empty sidebar.
    pub(crate) fn reconcile(
        &mut self,
        candidates: &[Cameo],
        visible_rows: usize,
        recalculate_builds: bool,
    ) -> CameoReconcile {
        self.refresh_keys(candidates);
        let mut build_entered = [false; 4];
        let mut build_inserted = false;
        let empty_before_builds = self.sidebar_empty();
        let mut active_tab_destination = None;
        for candidate in candidates.iter().filter(|item| {
            recalculate_builds && item.available && matches!(item.id, CameoId::Object(_))
        }) {
            let accepted = self.insert(candidate);
            build_entered[candidate.tab.tab_index()] |= accepted;
            build_inserted |= accepted;
            if empty_before_builds && accepted && active_tab_destination.is_none() {
                // AddCameo scans after each insertion. The first accepted
                // ordinary cameo is the only one that sees an empty sidebar;
                // later candidates must not replace its selection.
                active_tab_destination = Some(candidate.tab);
            }
        }
        let counted: [usize; 4] = std::array::from_fn(|index| self.strips[index].len());
        let available: std::collections::HashSet<_> = candidates
            .iter()
            .filter(|item| item.available)
            .map(|item| item.id)
            .collect();
        // Super grant/revoke already has its own current projection. Preserve
        // that route; an ordinary presentation refresh may not prune builds.
        self.retain(visible_rows, |id| {
            (!recalculate_builds && matches!(id, CameoId::Object(_))) || available.contains(&id)
        });
        let emptied =
            std::array::from_fn(|index| counted[index] > 0 && self.strips[index].is_empty());
        if emptied.iter().any(|was_emptied| *was_emptied)
            && let Some(first) = self.first_populated_tab()
        {
            // Recalculate runs after all ordinary additions and scans the
            // resulting strips, even if the emptied strip was not active.
            active_tab_destination = Some(first);
        }
        let empty_before_supers = self.sidebar_empty();
        let mut super_entered = [false; 4];
        let mut super_inserted = false;
        for candidate in candidates
            .iter()
            .filter(|item| item.available && matches!(item.id, CameoId::SuperWeapon(_)))
        {
            let accepted = self.insert(candidate);
            super_entered[candidate.tab.tab_index()] |= accepted;
            if empty_before_supers && accepted && !super_inserted {
                // Supers are added after Recalculate. If that pass left no
                // entries, only the first accepted Super AddCameo relocates.
                active_tab_destination = Some(candidate.tab);
            }
            super_inserted |= accepted;
        }
        CameoReconcile {
            build_inserted,
            transitions: CameoTransitions {
                build_entered,
                emptied,
                super_entered,
                active_tab_destination,
            },
        }
    }

    fn sidebar_empty(&self) -> bool {
        self.strips.iter().all(|strip| strip.is_empty())
    }

    /// The first populated strip in native scan order (0..3), the destination
    /// both AddCameo's and Recalculate's relocation blocks pick.
    pub(crate) fn first_populated_tab(&self) -> Option<SidebarTab> {
        SidebarTab::all()
            .into_iter()
            .find(|tab| !self.items(*tab).is_empty())
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
