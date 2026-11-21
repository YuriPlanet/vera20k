//! The sound names a set's rules and art name, held once in a table the set
//! builds, so a sound event copies a [`SoundNameId`] instead of a `String`.
//!
//! This is not the simulation's string interner and must never become one:
//! interned ids are saved and hashed, and interning a sound name on first use
//! would move them and the state hash. The table is built with the set from
//! its own fields, never grows while a game runs, and is neither saved nor
//! hashed. A name it does not hold still travels by name
//! ([`SoundName::Unlisted`]), so what plays never depends on the table.
//!
//! Residual: terrain `AmbientSound` is bound after the last rebuild
//! (`bind_terrain_type_sound_references`) and is not walked, so a terrain
//! type's ambient sound travels Unlisted: it plays the same and allocates per
//! emission. No retail terrain type sets one.

use super::RuleSet;
use crate::util::lookup_hash::LookupMap;

/// One name of one [`SoundNames`] table. Copy; it reads only against the
/// table that made it ([`SoundName::resolve`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoundNameId {
    /// The making table's generation: distinct for every table the process
    /// builds, so a rebuilt set's table refuses the ids of the one before.
    table: u64,
    index: u32,
}

impl SoundNameId {
    /// The generation of the table that made this id.
    pub fn generation(self) -> u64 {
        self.table
    }
}

/// The sound a sound event names, spelled exactly as the rules field that
/// named it, case included.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SoundName {
    /// A name the set's table holds: every name the set's fields held when
    /// it was built. Emitting it copies the id.
    Listed(SoundNameId),
    /// A name VERA's own code spells, not one read from rules.
    Static(&'static str),
    /// A name the table does not hold (a field changed after the set was
    /// built, or one its walk does not visit), carried by value as before.
    Unlisted(Box<str>),
}

impl SoundName {
    /// The name, read against `names` for a listed one. A listed name
    /// answers `None` against another table, or none: a rules rebuild ended
    /// the events that named it. The client reports that as a fault
    /// (`GameSoundEvent::sound_id`): no table rebuilds while events run.
    pub fn resolve<'a>(&'a self, names: Option<&'a SoundNames>) -> Option<&'a str> {
        match self {
            Self::Listed(id) => names.and_then(|names| names.name_of(*id)),
            Self::Static(name) => Some(name),
            Self::Unlisted(name) => Some(name),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl SoundName {
    /// The name read against `rules`' table; "" for an id of another table.
    pub fn name_in<'a>(&'a self, rules: &'a RuleSet) -> &'a str {
        self.resolve(Some(rules.sound_names())).unwrap_or_default()
    }
}

/// Every sound name one [`RuleSet`] names, each once, in first-seen order.
/// Derived from the set's fields ([`RuleSet::rebuild_sound_names`]): never
/// hashed or saved.
#[derive(Debug, Default)]
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct SoundNames {
    /// This table's generation; 0, which no built table has, when empty.
    table: u64,
    names: Vec<Box<str>>,
    /// Exact-case name to its index in `names`: a name differing only in
    /// case is another name, as the String it replaces was.
    index: LookupMap<Box<str>, u32>,
}

impl SoundNames {
    fn build<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut table = Self {
            table: super::next_rule_set_serial(),
            ..Self::default()
        };
        for name in names {
            if table.index.contains_key(name) {
                continue;
            }
            // VERA's own bound, unreachable in practice: names past 2^32 stay
            // out of the table and travel Unlisted, so they still play.
            let Ok(index) = u32::try_from(table.names.len()) else {
                break;
            };
            table.names.push(name.into());
            table.index.insert(name.into(), index);
        }
        table
    }

    /// `name` as a sound event carries it: its id when this table holds it.
    pub fn name(&self, name: &str) -> SoundName {
        match self.index.get(name) {
            Some(&index) => SoundName::Listed(SoundNameId {
                table: self.table,
                index,
            }),
            None => SoundName::Unlisted(name.into()),
        }
    }

    fn name_of(&self, id: SoundNameId) -> Option<&str> {
        if id.table != self.table {
            return None;
        }
        self.names.get(id.index as usize).map(|name| &**name)
    }

    /// This table's generation; 0 for the empty table no build made.
    pub fn generation(&self) -> u64 {
        self.table
    }

    /// Whether this table holds `name`, spelled exactly so.
    pub fn holds(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }
}

impl RuleSet {
    /// The set's sound-name table.
    pub fn sound_names(&self) -> &SoundNames {
        &self.sound_names
    }

    /// `name` as a sound event of this set carries it ([`SoundNames::name`]).
    pub fn sound_name(&self, name: &str) -> SoundName {
        self.sound_names.name(name)
    }

    /// Build the table again from the set's fields, after the set is built
    /// and after each binder that rewrites its sound fields. The new table
    /// takes a new generation, so ids of the old one no longer read.
    pub(crate) fn rebuild_sound_names(&mut self) {
        let general = self.general.sound_names();
        let objects = self
            .object_list
            .iter()
            .flat_map(|object| object.sound_names());
        let weapons = self
            .weapon_types
            .iter()
            .flat_map(|weapon| weapon.report.iter().chain(&weapon.down_report))
            .map(String::as_str);
        let anims = self.art_registry.iter_entries().flat_map(|(name, _)| {
            self.art_registry
                .anim_runtime_config(name)
                .into_iter()
                .flat_map(|config| [&config.start_sound, &config.stop_sound])
                .flatten()
                .map(String::as_str)
        });
        let voxel_anims = self
            .voxel_anim_types
            .iter()
            .flat_map(|anim| [&anim.start_sound, &anim.stop_sound])
            .flatten()
            .map(String::as_str);
        let super_weapons = self.super_weapon_order.iter().flat_map(|id| {
            self.super_weapons
                .get(id)
                .into_iter()
                .flat_map(|sw| {
                    [&sw.special_sound, &sw.start_sound]
                        .into_iter()
                        .chain(&sw.presentation.activation_sound)
                })
                .flatten()
                .map(String::as_str)
        });
        let mind_control = [
            &self.mind_control.mind_control_sound,
            &self.mind_control.mind_cleared_sound,
            &self.mind_control.overload_sound,
        ];
        let others = mind_control
            .into_iter()
            .chain([
                &self.bridge_rules.repair_sound,
                &self.crate_rules.heal_crate_sound,
            ])
            .flatten()
            .map(String::as_str);
        let overlays = (0..=u8::MAX)
            .filter_map(|id| self.overlay_registry.flags(id))
            .filter_map(|overlay| overlay.crush_sound.as_deref());
        self.sound_names = SoundNames::build(
            general
                .chain(objects)
                .chain(weapons)
                .chain(anims)
                .chain(voxel_anims)
                .chain(super_weapons)
                .chain(others)
                .chain(overlays),
        );
    }
}

impl super::GeneralRules {
    /// Every sound name the global sections hold.
    fn sound_names(&self) -> impl Iterator<Item = &str> {
        let scalars = [
            &self.cloak_sound,
            &self.leave_bio_reactor_sound,
            &self.enter_bio_reactor_sound,
            &self.enter_grinder_sound,
            &self.upgrade_veteran_sound,
            &self.upgrade_elite_sound,
            &self.slaves_free_sound,
            &self.crate_money_sound,
            &self.crate_reveal_sound,
            &self.crate_fire_sound,
            &self.crate_armour_sound,
            &self.crate_speed_sound,
            &self.crate_unit_sound,
            &self.crate_promote_sound,
            &self.airstrike_abort_sound,
            &self.airstrike_attack_voice,
            &self.building_garrisoned_sound,
            &self.building_abandoned_sound,
            &self.sell_sound,
            &self.base_under_attack_sound,
            &self.building_die_sound,
            &self.building_damage_sound,
            &self.create_unit_sound,
            &self.create_infantry_sound,
            &self.create_aircraft_sound,
            &self.disk_laser_charge_up,
            &self.storm_sound,
            &self.dig_sound,
            &self.psychic_dominator_activate_sound,
            &self.genetic_mutator_activate_sound,
            &self.psychic_reveal_activate_sound,
            &self.spy_plane_camera,
            &self.chute_sound,
            &self.gui_main_button_sound,
            &self.gui_move_in_sound,
            &self.gui_move_out_sound,
            &self.shell_button_slide_sound,
            &self.generic_click_sound,
            &self.guard_sound,
            &self.stop_sound,
            &self.radar_on,
            &self.radar_off,
            &self.spy_sat_activation_sound,
            &self.spy_sat_deactivation_sound,
            &self.voice_ifv_repair,
            &self.start_planning_mode_sound,
            &self.end_planning_mode_sound,
            &self.add_planning_mode_command_sound,
            &self.place_beacon_sound,
            &self.scatter_sound,
            &self.scold_sound,
            &self.generic_beep_sound,
            &self.gui_checkbox_sound,
            &self.gui_build_sound,
            &self.building_slam,
            &self.gui_tab_sound,
            &self.incoming_message_sound,
            &self.message_char_typed_sound,
            &self.system_error_sound,
            &self.options_changed_sound,
            &self.player_left_sound,
            &self.player_joined_sound,
            &self.game_forming_sound,
            &self.game_closed_sound,
            &self.gui_combo_open_sound,
            &self.gui_combo_close_sound,
            &self.gui_open_sound,
            &self.gui_close_sound,
            &self.bunker_walls_down_sound,
            &self.bunker_walls_up_sound,
            &self.chrono_in_sound,
            &self.chrono_out_sound,
            &self.time_warp_out_sound,
            &self.time_warp_in_sound,
            &self.cheer_sound,
            &self.impact_water_sound,
            &self.impact_land_sound,
            &self.sinking_sound,
            &self.score_anim_sound,
            &self.construction_sound,
            &self.building_repaired_sound,
            &self.bomb_ticking_sound,
            &self.bomb_attach_sound,
        ];
        scalars
            .into_iter()
            .chain(&self.gate_sounds)
            .chain(self.ares_decloak_sound.iter())
            .filter_map(|name| name.as_deref())
            .chain(
                self.lightning_sounds
                    .iter()
                    .chain(&self.credit_ticks)
                    .map(String::as_str),
            )
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "sound_names_tests.rs"]
mod tests;
