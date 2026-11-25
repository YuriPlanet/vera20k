//! Forking the world: every field shared, forked, asked whether it was
//! written and put back, for the object pass that is to run an object's turn
//! on a private fork of the world (`util::forked`).
//!
//! [`fork_helpers!`] writes a struct's helpers from the one list of its
//! fields. Every body names each field without `..`, so a field added to
//! `Simulation` or `ObjectSubstrate` and missing from its list does not
//! compile.

use super::Simulation;

/// Writes `$ty`'s fork helpers from the list of its fields, in declaration
/// order. A field in brackets is a struct of fields with helpers of its own;
/// a field in braces is not forked (VERA: the Wasm modules, whose live
/// instances cannot be copied): a fork gets its `fork_empty()`, and the other
/// helpers leave it alone. Every other field is a `Forked`. `$prefix` goes before each field's name
/// in [`Simulation::written_fields`].
macro_rules! fork_helpers {
    ($ty:ident, $prefix:literal: $($(#[$cfg:meta])* $([$part:ident])? $({$own:ident})? $($field:ident)?,)+) => {
        #[cfg_attr(
            not(test),
            expect(dead_code, reason = "the forked object pass is the first caller")
        )]
        impl $ty {
            /// Share every field, for a world about to be forked.
            pub(crate) fn share_fields(&mut self) {
                let $ty { $($(#[$cfg])* $($part)? $($own)? $($field)?,)+ } = self;
                $($(#[$cfg])* {
                    $($part.share_fields();)?
                    $(let _ = $own;)?
                    $($crate::util::forked::Forked::share($field);)?
                })+
            }

            /// Take every field back inline once the forks are gone.
            ///
            /// # Panics
            /// When a fork still holds a field.
            pub(crate) fn unshare_fields(&mut self) {
                let $ty { $($(#[$cfg])* $($part)? $($own)? $($field)?,)+ } = self;
                $($(#[$cfg])* {
                    $($part.unshare_fields();)?
                    $(let _ = $own;)?
                    $($crate::util::forked::Forked::unshare($field);)?
                })+
            }

            /// A fork: another handle on every shared field, and a full copy
            /// of every other.
            pub(crate) fn fork(&self) -> Self {
                let $ty { $($(#[$cfg])* $($part)? $($own)? $($field)?,)+ } = self;
                $ty {
                    $($(#[$cfg])* $($part: $part.fork(),)? $($own: $own.fork_empty(),)? $($field: $crate::util::forked::Forked::fork($field),)?)+
                }
            }

            /// Add the names of the written fields to `names`, in
            /// declaration order.
            pub(super) fn push_written_fields(&self, names: &mut Vec<&'static str>) {
                let $ty { $($(#[$cfg])* $($part)? $($own)? $($field)?,)+ } = self;
                $($(#[$cfg])* {
                    $($part.push_written_fields(names);)?
                    $(let _ = $own;)?
                    $(if $crate::util::forked::Forked::written($field) {
                        names.push(concat!($prefix, stringify!($field)));
                    })?
                })+
            }

            /// Forget which fields were written.
            pub(crate) fn clear_written_fields(&mut self) {
                let $ty { $($(#[$cfg])* $($part)? $($own)? $($field)?,)+ } = self;
                $($(#[$cfg])* {
                    $($part.clear_written_fields();)?
                    $(let _ = $own;)?
                    $($crate::util::forked::Forked::clear_written($field);)?
                })+
            }

            /// Make every written field `base`'s fork again, dropping what
            /// this fork wrote.
            pub(crate) fn restore_written_fields_from(&mut self, base: &Self) {
                let $ty { $($(#[$cfg])* $($part)? $($own)? $($field)?,)+ } = self;
                $($(#[$cfg])* {
                    $($part.restore_written_fields_from(&base.$part);)?
                    $(let _ = $own;)?
                    $(if $crate::util::forked::Forked::written($field) {
                        $crate::util::forked::Forked::restore_from($field, &base.$field);
                    })?
                })+
            }
        }
    };
}
pub(super) use fork_helpers;

fork_helpers! {
    Simulation, "":
    interner,
    type_tech_levels,
    building_upgrade_art_names,
    voxel_type_damage,
    rule_handles,
    production,
    terrain_objects,
    ore_growth_config,
    ore_growth_state,
    session,
    #[cfg(feature = "debug-tools")]
    debug_tools,
    frame_profiling,
    frame_profile,
    object_placement_scope_depth,
    buildup_bypass,
    #[cfg(feature = "phobos")]
    reflected_warhead,
    ares_made_up_rules,
    scenario_rng,
    main_rng,
    mapgen_rng,
    native_unique_ids,
    authored_tiberium_value_total,
    post_load_particle_system_constructed,
    native_map_tubes,
    load_objects,
    fog,
    house_alliances,
    [substrate],
    lifecycle_outputs,
    unit_voice_visit_interest,
    mission_spawned_entities,
    ice_zones_stale,
    ownership_changed_since_output,
    frame_overlay_updates,
    frame_overlay_removals,
    terminal_score_snapshot,
    #[cfg(any(test, feature = "test-support"))]
    lifecycle_test_events,
    #[cfg(test)]
    receiver_fixture,
    trigger_effects,
    revision,
    #[cfg(any(test, feature = "test-support"))]
    frame_step_trace,
    #[cfg(any(test, feature = "test-support"))]
    house_ai_activation_order_test_trace,
    sound_events,
    lighting_sources,
    fire_events,
    combat_light_requests,
    screen_shake_updates,
    projectiles,
    #[cfg(feature = "phobos")]
    warhead_anim_runtime,
    waves,
    active_wave_links,
    object_custom_sounds,
    #[cfg(any(test, feature = "test-support"))]
    bale_events,
    #[cfg(any(test, feature = "test-support"))]
    bunker_wall_events,
    team_script_vm,
    houses,
    movement_pass_cache,
    reveal_viewers,
    civilian_house_memo,
    expiry_listeners,
    path_batch,
    idle_sleep,
    scan_batch,
    effect_anims,
    turn_streams,
    zone_grid,
    path_grid,
    structure_navigation_cells,
    resolved_terrain,
    shared_cell_dummy,
    native_local_cell_cursor,
    native_allocated_cell_cursor,
    real_cell_bridge_flags_0x1180,
    dynamic_terrain_cells,
    ice_cracks,
    terrain_dent,
    bridge_state,
    pending_bridge_collapses,
    overlay_grid,
    crate_authority,
    smudge_grid,
    radiation,
    bombs,
    kamikaze,
    disk_lasers,
    playfield_bounds,
    playfield_size_height,
    playfield_revision,
    bridge_explosions,
    metallic_debris,
    dirty_cells,
    power_states,
    super_weapons,
    lightning_storm,
    psychic_dominator,
    unit_deliveries,
    ares_chrono_warps,
    ares_firestorm,
    ares_tunnels,
    ion_blasts,
    alpha_shapes,
    terrain_speed_config,
    input_delay_ticks,
    quit_requested,
    executed_exit_owner,
    departed_senders,
    connection_lost,
    frame_sync_checksum,
    state_hash_interval,
    pending_commands,
    local_commands,
    dispatch_reserved_slots,
    frame_command_admissions,
    frame_continuation,
    planning,
    replaying_recorded_commands,
    trigger_runtime,
    {ext_modules},
}

impl Simulation {
    /// The names of the fields written since their flags were last cleared,
    /// in declaration order; the object substrate's as `substrate.<name>`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the forked object pass is the first caller")
    )]
    pub(crate) fn written_fields(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        self.push_written_fields(&mut names);
        names
    }
}

#[cfg(all(test, tests_sim_world_c_o))]
mod tests {
    use super::Simulation;
    use crate::map::entities::{EntityCategory, MapEntity};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::pathfinding::PathGrid;

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n\n\
             [GAPOWR]\nStrength=750\nArmor=wood\nFoundation=2x2\nSight=5\nPower=100\n",
        ))
        .expect("fork test rules parse")
    }

    /// A flat arena with one power plant: a frame runs its turn.
    fn world(rules: &RuleSet) -> (Simulation, PathGrid) {
        let mut sim = Simulation::with_seed(0xF0_4B00_0001);
        let grid = crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        sim.spawn_from_map(
            &[MapEntity {
                owner: "Americans".to_string(),
                type_id: "GAPOWR".to_string(),
                health: 256,
                cell_x: 10,
                cell_y: 10,
                facing: 64,
                category: EntityCategory::Structure,
                sub_cell: 0,
                veterancy: Some(0),
                high: false,
                mission: None,
                trigger_tag: None,
                recruitable_a: true,
                recruitable_b: true,
                group: -1,
                follows: None,
                structure_columns: Default::default(),
                structure_ai_sellable: false,
                structure_ai_repairable: false,
                structure_online: true,
            }],
            Some(rules),
        );
        (sim, grid)
    }

    fn tick(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
        let _ = sim.advance_tick(&[], Some(rules), Some(grid), None, 67);
    }

    #[test]
    fn a_fork_advances_like_a_copy_and_leaves_its_base_alone() {
        let rules = rules();
        let (mut base, grid) = world(&rules);
        let before = base.state_hash();
        let mut copy = base.clone();
        tick(&mut copy, &rules, &grid);
        assert_ne!(copy.state_hash(), before);

        base.share_fields();
        let mut fork = base.fork();
        tick(&mut fork, &rules, &grid);
        assert_eq!(base.state_hash(), before);
        assert_eq!(fork.state_hash(), copy.state_hash());

        drop(fork);
        base.unshare_fields();
        tick(&mut base, &rules, &grid);
        assert_eq!(base.state_hash(), copy.state_hash());
    }

    #[test]
    fn written_fields_names_exactly_the_stores_a_call_wrote() {
        let rules = rules();
        let (mut sim, _) = world(&rules);
        sim.clear_written_fields();
        assert_eq!(sim.written_fields(), [""; 0]);
        // Reads write nothing.
        let _ = (sim.state_hash(), sim.scenario_rng.state());
        assert_eq!(sim.written_fields(), [""; 0]);
        let _ = sim.scenario_rng.next_u32();
        let _ = sim.substrate.allocate_stable_id();
        assert_eq!(
            sim.written_fields(),
            ["scenario_rng", "substrate.next_stable_object_id"]
        );
    }

    #[test]
    fn restoring_the_written_fields_returns_a_fork_to_its_base() {
        let rules = rules();
        let (mut base, grid) = world(&rules);
        base.share_fields();
        let mut fork = base.fork();
        tick(&mut fork, &rules, &grid);
        assert_ne!(fork.state_hash(), base.state_hash());
        assert!(fork.written_fields().contains(&"substrate.entities"));
        fork.restore_written_fields_from(&base);
        assert_eq!(fork.state_hash(), base.state_hash());
        assert_eq!(fork.written_fields(), [""; 0]);
    }
}
