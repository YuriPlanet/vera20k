//! Diagnostic only: locate live load-produced inputs for native 0x00586BF0.
//! This does not implement the restamp or assert native final-state parity.

#[test]
#[ignore = "requires active retail assets; writes a local diagnostic receipt"]
fn retail_inactive_high_record_restamp_inventory() {
    use serde_json::json;
    use std::collections::BTreeSet;

    let retail = super::retail_dir().expect("configured active retail install");
    let maps = std::env::var("VERA20K_RESTAMP_MAPS")
        .unwrap_or_else(|_| "BayOPigs.mmx;Hills.mmx;Deadman.mmx".into());
    let mut results = Vec::new();
    for map_name in maps.split(';') {
        let scenario = crate::headless_scenario::load(&retail, map_name, super::SEED)
            .unwrap_or_else(|error| panic!("load {map_name}: {error}"));
        let sim = scenario.sim();
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("live resolved terrain");
        let state = sim.bridge_state.as_ref().expect("live bridge state");
        let records = state.endpoint_records();
        let mut assets = crate::assets::asset_manager::AssetManager::new(&retail).unwrap();
        let theater = crate::map::theater::load_theater(&mut assets, &scenario.map.header.theater)
            .expect("same retail theater as production load");
        let bridge_bases: Vec<_> = [theater.bridge_set, theater.wood_bridge_set]
            .into_iter()
            .map(|set| set.map(|index| theater.lookup.bounds()[usize::from(index)].start))
            .collect();
        let source_cells: BTreeSet<_> = sim
            .production
            .terrain_object_cells
            .keys()
            .copied()
            .collect();
        let objects = crate::sim::tiberium::TiberiumPlacementObjectContext::new(
            sim.entities(),
            sim.occupancy(),
            &scenario.runtime.resources.rules,
            &sim.interner,
            &sim.production.terrain_object_cells,
        );
        let admission =
            crate::sim::tiberium::NewTiberiumAdmission::runtime(terrain, sim.path_grid(), objects);
        let inactive: Vec<_> = records
            .iter()
            .filter(|r| r.is_high() && !r.active)
            .collect();
        let mut gaps = Vec::new();
        let mut affected = BTreeSet::new();
        for record in &inactive {
            let a = (
                i32::from(record.endpoint_a.0 as i16),
                i32::from(record.endpoint_a.1 as i16),
            );
            let b = (
                i32::from(record.endpoint_b.0 as i16),
                i32::from(record.endpoint_b.1 as i16),
            );
            assert!(a.0 == b.0 || a.1 == b.1, "axis-aligned producer record");
            let step = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
            let mut cursor = (a.0 + step.0, a.1 + step.1);
            while cursor != b {
                if terrain
                    .cell(cursor.0 as u16, cursor.1 as u16)
                    .is_some_and(|cell| cell.bridge_flags() & 0x100 == 0)
                {
                    gaps.push(json!({"record_a": a, "record_b": b, "gap": cursor}));
                    for offset in -2..=1 {
                        affected.insert(if step.0 == 0 {
                            (cursor.0 + offset, cursor.1)
                        } else {
                            (cursor.0, cursor.1 + offset)
                        });
                    }
                }
                cursor = (cursor.0 + step.0, cursor.1 + step.1);
            }
        }
        let facts: Vec<_> = affected.into_iter().map(|(x, y)| {
            let coord = (x as u16, y as u16);
            let cell = terrain.cell(coord.0, coord.1);
            json!({"coord": [x, y], "real": cell.is_some(), "cell": cell.map(|c| json!({
                "tile": c.final_tile_index, "subtile": c.final_sub_tile,
                "flags": c.bridge_flags(), "level": c.level, "slope": c.slope_type,
                "land": c.yr_cell_land_type, "zone": c.zone_type,
                "outside_playfield": c.outside_playfield, "allows_tiberium": c.allows_tiberium,
                "base_build_blocked": c.base_build_blocked,
                "resolved_tiberium_admission": crate::sim::tiberium::resolved_cell_accepts_tiberium(c),
                "overlay_id": sim.overlay_grid.as_ref().and_then(|g| g.cell(coord.0, coord.1).overlay_id),
                // All terrain cells conservatively form the source exclusion set;
                // an admitted empty witness is also admitted with the narrower spawner set.
                "live_new_tiberium_admission": sim.overlay_grid.as_ref().is_some_and(|g|
                    crate::sim::tiberium::can_place_new_tiberium(g, &source_cells, admission, coord)),
                "techno_occupants": sim.occupancy().get(coord.0, coord.1).map_or(0, |o| o.occupants.len()),
                "terrain_object": sim.production.terrain_object_cells.contains_key(&coord),
                "ground_walkable": sim.path_grid().and_then(|g| g.cell(coord.0, coord.1)).map(|c| c.ground_walkable),
            }))})
        }).collect();
        if map_name.eq_ignore_ascii_case("Deadman.mmx") {
            let native: serde_json::Value = serde_json::from_str(include_str!(
                "../../../tools/spatial_oracle/bridge_restamp_retail.json"
            ))
            .unwrap();
            for change in native["changes"].as_array().unwrap() {
                let x = change["coord"][0].as_u64().unwrap() as u16;
                let y = change["coord"][1].as_u64().unwrap() as u16;
                assert_eq!(
                    u64::from(terrain.cell(x, y).unwrap().bridge_flags()),
                    change["after"].as_u64().unwrap(),
                    "post-load native flags at{x},{y}"
                );
            }
            let witness = facts
                .iter()
                .find(|row| row["coord"] == serde_json::json!([57, 42]))
                .unwrap();
            assert_eq!(
                witness["cell"]["live_new_tiberium_admission"], false,
                "fresh stock load must publish native placement rejection"
            );
        }
        println!(
            "{map_name}: records={}, inactive_high={}, gaps={}, affected_cells={}",
            records.len(),
            inactive.len(),
            gaps.len(),
            facts.len()
        );
        let native_cells: Vec<_> = if inactive.is_empty() {
            Vec::new()
        } else {
            terrain
                .iter()
                .map(|c| {
                    json!([
                        c.rx,
                        c.ry,
                        c.final_tile_index,
                        c.final_sub_tile,
                        c.bridge_flags(),
                        c.yr_cell_land_type,
                        c.tube_index.map_or(-1, |id| i32::from(id.0)),
                        c.level,
                        c.slope_type
                    ])
                })
                .collect()
        };
        results.push(json!({"map": map_name, "records": records, "gaps": gaps,
            "affected_cells": facts, "native_cells": native_cells,
            "size": [scenario.map.header.width, scenario.map.header.height],
            "bridge_bases": bridge_bases,
            "tubes": terrain.tube_facts(),
            "local_size": [scenario.map.header.local_left, scenario.map.header.local_top,
                scenario.map.header.local_width, scenario.map.header.local_height],
            "theater": scenario.map.header.theater,
            "tile_allows_tiberium": (0..theater.lookup.len()).map(|tile|
                theater.lookup.allows_tiberium(tile as u16)).collect::<Vec<_>>(),
            "land_buildable": (0..=10).map(|land|
                scenario.runtime.resources.rules.terrain_rules.semantics_for_land_type(land)
                    .map(|semantics| semantics.buildable)).collect::<Vec<_>>(),
        }));
    }
    let output = std::env::var("VERA20K_RESTAMP_OUTPUT")
        .unwrap_or_else(|_| "target/restamp-retail-inventory.json".into());
    if let Some(parent) = std::path::Path::new(&output).parent() {
        std::fs::create_dir_all(parent).expect("inventory output directory");
    }
    std::fs::write(output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
}

/// Count the terrain shape A8's D1 fires on, so its frequency stops being a
/// guess.
///
/// D1: native decides uphill from downhill with two `GetGroundHeight` samples -
/// the destination cell's centre against the ground under the mover's exact XY
/// (`0x004B3CEC`, `0x004B3D1A`, compared at `0x004B3D21`) - where
/// `terrain_speed.rs` compares the two cells' integer `level` bytes. The two
/// disagree exactly where a mover leaves a ramp onto a flat cell standing at the
/// ramp's own level: the level bytes are equal, so VERA reads "flat" and applies
/// x1.0, while native's sub-cell sample still sits on the slope and applies
/// `Tracked/WheeledDownhill = 1.2`.
///
/// This is static terrain, so it is countable without playing anything - which
/// is why the ledger row must not sit behind "needs a run". It reports the count
/// rather than asserting a threshold: the number is the evidence, and pinning a
/// map's terrain shape here would only break when the map list changes.
///
/// **Read the share carefully.** A ramp cell at level L rises to L+1, so its
/// flat neighbours divide between L and L+1 more or less evenly, and a result
/// near half is close to a description of what a ramp *is* rather than a
/// property of these maps. The load-bearing claim is the structural one -
/// `slope_factor_for` compares raw level bytes, so **every** exit of this shape
/// diverges - and this census only says the shape is ordinary rather than rare.
///
/// **Two facts decide whether a player sees it, and they point opposite ways.**
///
/// - `TechnoClass::SetSpeedFraction 0x004D3710` clamps the owner's `+0x578` at
///   1.0 (`FCOMP [0x007E1718]`, `TEST AH,1`). `drive+0x50` holds only the
///   *target*; `+0x578` is the accumulator, and both the accelerate and
///   decelerate arms of `Process_Drive_Track` write it through vtable `+0x544`.
///   So for a healthy mover on a 100% row, target 1.2 and target 1.0 both
///   saturate at 1.0 and the divergence is **invisible**.
/// - The damaged-mover factor is applied *after* the slope and reaches no clamp
///   of its own: `0x004B3DD4` calls the health ratio, `FCOMP [Rules+0x1700]`
///   tests ConditionYellow, and `0x004B3DF0 FMUL [0x007E7FC0]` multiplies by
///   0.75 (read from the binary as `0x3FE8000000000000`). On a 100% row native
///   then holds `1.0 x 1.2 x 0.75 = 0.9` where pre-fix VERA held `0.75` - both
///   under the clamp, so **every** qualifying exit is visible to any vehicle at
///   or below ConditionYellow. Review caught this; the first version of this
///   census reported such exits as invisible.
///
/// It counts static adjacency **pairs**, not traversals: how often a mover
/// actually drives one still depends on traffic.
///
/// UNCHECKED: that `ResolvedTerrainCell::level` is the ramp's **base** rather
/// than its top. If it were the top this counts uphill exits instead, which is
/// the opposite of the shape D1 is about.
#[test]
#[ignore = "requires active retail assets; reports a terrain census"]
fn retail_ramp_exit_onto_equal_level_flat_census() {
    let retail = super::retail_dir().expect("configured active retail install");
    let maps = std::env::var("VERA20K_RAMP_CENSUS_MAPS")
        .unwrap_or_else(|_| "BayOPigs.mmx;Hills.mmx;Deadman.mmx".into());
    for map_name in maps.split(';') {
        let scenario = crate::headless_scenario::load(&retail, map_name, super::SEED)
            .unwrap_or_else(|error| panic!("load {map_name}: {error}"));
        let sim = scenario.sim();
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("live resolved terrain");

        let rules = &scenario.runtime.resources.rules;
        // Gate on the mover's own land row, NOT on `ground_walkable`. Review
        // caught the latter: `pathfinding::core` deliberately reports water as
        // ground-walkable ("SpeedType cost=0 blocks ground in A*"), and retail
        // gives Water and Beach `Track=0%`, so the boolean admitted exactly the
        // cells a tracked vehicle can never enter - and, being below full speed,
        // scored them as divergences too. A 0% row also diverges by nothing: the
        // exact-zero substitution at `0x004B3DB0` yields 0.5 either way, because
        // `0 x 1.2` is still 0.
        //
        // **Both stock ground defaults are measured, not just Track.** A second
        // review caught the claim that stock ground vehicles all default to
        // Track. They do not: `UnitTypeClass::ReadINI` leaves `+0x67C` at -1
        // when the INI omits `SpeedType=` and then derives it from `Crusher=` -
        // `0x007476DE MOV DL,[type+0xD28]` / `NEG DL` / `SBB EDX,EDX` /
        // `ADD EDX,2`, which is 1 (Track) when the byte is set and 2 (Wheel)
        // when it is clear. `+0xD28` is `Crusher=`, stored at `0x00714CE3` from
        // key string `0x0081BB58` inside `TechnoTypeClass::ReadINI 0x00712170`.
        // In stock `rulesmd.ini` that makes the majority **Wheel**: of 80
        // `[VehicleTypes]`, 56 omit `SpeedType=` and split 23 Track / 33 Wheel,
        // with 13 explicit Float and 10 explicit Hover.
        for speed_type in [
            crate::rules::locomotor_type::SpeedType::Track,
            crate::rules::locomotor_type::SpeedType::Wheel,
        ] {
            let track_row = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
                cell.speed_costs.speed_multiplier_for(speed_type)
            };
            let passable = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
                !cell.outside_playfield && track_row(cell) > crate::util::fixed_math::SIM_ZERO
            };

            let (mut ramp_cells, mut exits, mut flat_pairs, mut visible) = (0u32, 0u32, 0u32, 0u32);
            // Measured, not asserted: which land rows the qualifying destinations
            // actually carry. An earlier version of this census claimed "Clear,
            // Road and Rough" from the ratio alone, which the ratio cannot show.
            let mut dest_rows = [0u32; 12];
            for ry in 0..terrain.height() {
                for rx in 0..terrain.width() {
                    let Some(cell) = terrain.cell(rx, ry) else {
                        continue;
                    };
                    if cell.slope_type == 0 {
                        continue;
                    }
                    if !passable(cell) {
                        continue;
                    }
                    ramp_cells += 1;
                    // The eight neighbours a Drive mover can leave a ramp through.
                    for (dx, dy) in [
                        (-1i32, -1i32),
                        (0, -1),
                        (1, -1),
                        (-1, 0),
                        (1, 0),
                        (-1, 1),
                        (0, 1),
                        (1, 1),
                    ] {
                        let (nx, ny) = (i32::from(rx) + dx, i32::from(ry) + dy);
                        let (Ok(nx), Ok(ny)) = (u16::try_from(nx), u16::try_from(ny)) else {
                            continue;
                        };
                        let Some(neighbour) = terrain.cell(nx, ny) else {
                            continue;
                        };
                        if neighbour.slope_type != 0 {
                            continue;
                        }
                        if !passable(neighbour) {
                            continue;
                        }
                        flat_pairs += 1;
                        if neighbour.level == cell.level {
                            exits += 1;
                            dest_rows[usize::from(neighbour.land_type.min(11))] += 1;
                            // An UNDAMAGED mover only sees the coefficient when the
                            // destination row leaves room under the clamp. A damaged
                            // one sees every exit - see the print below.
                            if track_row(neighbour) < crate::util::fixed_math::SIM_ONE {
                                visible += 1;
                            }
                        }
                    }
                }
            }
            // Each share against its OWN denominator. Review caught both counts
            // being divided by `flat_pairs` while the sentence said "of those",
            // which understated the second by roughly a factor of two.
            let share = |n: u32, of: u32| {
                if of == 0 {
                    0.0
                } else {
                    f64::from(n) * 100.0 / f64::from(of)
                }
            };
            let rows: Vec<String> = dest_rows
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .map(|(land, n)| {
                    let land = u8::try_from(land).ok();
                    let name = land
                        .and_then(crate::rules::terrain_rules::LandType::from_index)
                        .map_or("?", |land_type| land_type.section_name());
                    // Review: printing the land-type INDEX alone cannot tell a
                    // real 100% row from a row that failed to parse, because
                    // `cost_for_speed_type` answers `None` there and
                    // `speed_multiplier_for` turns `None` into full speed - so a
                    // wholly collapsed table would print an identical histogram
                    // and an identical zero. Printing the resolved percentage,
                    // and "UNPARSED" when the row is absent, closes that.
                    let pct = land
                        .and_then(|land| rules.terrain_rules.semantics_for_land_type(land))
                        .map_or("UNPARSED".to_string(), |semantics| {
                            semantics
                                .cost_for_speed_type(speed_type)
                                .map_or("UNPARSED".to_string(), |cost| format!("{cost}%"))
                        });
                    format!("{name}={n}@{pct}")
                })
                .collect();
            println!(
                "A8 D1 census {map_name} [{speed_type:?}]: {ramp_cells} passable ramp \
             cells; of {flat_pairs} ramp-to-flat adjacency PAIRS (both cells inside \
             the playfield and above {speed_type:?}=0%), {exits} sit at the ramp's own \
             level ({:.1}% of pairs), of which {visible} ({:.1}% of exits) have a \
             destination row below full speed. Destination rows: {}. \
             READ THE SPLIT CAREFULLY: only the {visible} can diverge for an UNDAMAGED \
             mover, the rest being clamped to 1.0 by SetSpeedFraction. A mover at or \
             below ConditionYellow is NOT clamped (0x004B3DF0 applies 0.75 after the \
             slope), so on those the other exits diverge too - but that is arithmetic, \
             not a count this census took, and it holds only while the mover's sampled \
             ground height still exceeds the destination's: at the ramp's low edge the \
             two samples are equal and 0x004B3D21 takes no coefficient at all. Pairs, \
             not traversals; vehicles only.",
                share(exits, flat_pairs),
                share(visible, exits),
                rows.join(" ")
            );
        }
    }
}

/// Count the shape native's cliff override fires on.
///
/// Before the slope coefficient, `Process_Movement` compares two cell **level
/// bytes**: `0x004B3589` fetches the destination cell, `0x004B358E MOVSX
/// ECX,byte [EAX+0x11B]` reads its Level, `0x004B3595..0x004B359C` takes the
/// absolute difference against the mover's cell level (plus 4 when the mover is
/// on a bridge), and `0x004B359E CMP EAX,2` / `0x004B35A1 JGE 0x004B3C84`
/// forces the land-type row to **1** at `0x004B3C88` instead of the cell's own
/// `+0xEC`. VERA's `terrain_speed_factor` has no equivalent, so it reads the
/// destination's real row where gamemd reads full speed.
///
/// That is a larger swing than the 1.2 downhill coefficient on slow terrain, and
/// its frequency was recorded UNCHECKED. This counts it the same way the ramp
/// census counts D1's shape: over static terrain, no play required.
///
/// Three things this counts and one it does not, all found by review:
///
/// - **Only pairs a vehicle can actually take.** Level jumps of two or more sit
///   at cliff faces and shorelines, which `Can_Enter_Cell` refuses, and native
///   reaches `0x004B3589` only for a destination the pathfinder committed to.
///   Both cells must therefore be ground-walkable and inside the playfield. The
///   first version counted every pair and reported a cliff-perimeter statistic.
/// - **Only where forcing the row changes something.** The override's live
///   effect is `MOV ESI,1` - the Road row - and retail Road, Clear and Rough are
///   all 100% for Foot, Track and Wheel. So a qualifying pair whose destination
///   already reads 100% diverges by nothing; the divergence needs a slower row
///   (Ice, Weeds, Tiberium, Railroad). Both are reported.
/// - **Vehicles only.** `0x004B2630` is `DriveLocomotionClass`; Walk's Level-byte
///   sites are a Z sanity check and the bridge transition, with no Road override.
///
/// Not counted, and it is **not** a lower bound: the bridge arm adds 4 to the
/// **mover's** side alone, so a mover on a deck over flat ground reads a
/// difference of 4 on *every* step, not merely when leaving the deck. That
/// pushes the true figure up; the land-row gate pushes it down. Neither bounds
/// the other.
#[test]
#[ignore = "requires active retail assets; reports a terrain census"]
fn retail_cliff_override_level_difference_census() {
    let retail = super::retail_dir().expect("configured active retail install");
    let maps = std::env::var("VERA20K_CLIFF_CENSUS_MAPS")
        .unwrap_or_else(|_| "BayOPigs.mmx;Hills.mmx;Deadman.mmx".into());
    for map_name in maps.split(';') {
        let scenario = crate::headless_scenario::load(&retail, map_name, super::SEED)
            .unwrap_or_else(|error| panic!("load {map_name}: {error}"));
        let sim = scenario.sim();
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("live resolved terrain");

        let rules = &scenario.runtime.resources.rules;
        // Same correction as the D1 census: gate on the mover's own land row,
        // not on `ground_walkable`, which is deliberately true for water and so
        // admitted cells a tracked vehicle can never enter.
        //
        // **Track alone is the wrong measurement here, and review caught it.**
        // The override forces land row 1 = Road and then indexes that row by
        // the mover's SpeedType (`0x004B3C88 MOV ESI,1`, `LEA ECX,[ESI+ESI*8]`
        // into the table at `0x0089EA40`), so what matters is Road[SpeedType]
        // against the destination's own row for that same SpeedType. Retail
        // Railroad is Foot 90 / Track 100 / **Wheel 50**: a wheeled mover
        // crossing a two-level delta onto Railroad diverges 1.0 against 0.5,
        // and a Track-only test scores exactly that pair as unable to diverge.
        // With the diverging count resting on single-digit populations on two
        // of three maps, one such pair moves the answer, so both stock ground
        // defaults are measured.
        // `Float` is here because the override is NOT unique to Drive:
        // `ShipLocomotionClass::Process_Movement 0x006A1C80` carries a
        // byte-identical copy at `0x006A2BDD` / `0x006A2BED` / `0x006A2BF0`.
        // Review pointed out that the water census this needs is not a new
        // harness at all - `passable` is `row > 0`, and with Float that admits
        // exactly Water and deletes all eleven other land rows, because retail
        // sets Float to 0% on every one of them. So the gate is not blind to
        // the Ship site; only the Track/Wheel instantiations were.
        for speed_type in [
            crate::rules::locomotor_type::SpeedType::Track,
            crate::rules::locomotor_type::SpeedType::Wheel,
            crate::rules::locomotor_type::SpeedType::Float,
        ] {
            let track_row = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
                cell.speed_costs.speed_multiplier_for(speed_type)
            };
            let passable = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
                !cell.outside_playfield && track_row(cell) > crate::util::fixed_math::SIM_ZERO
            };
            // **Read the Road row; do not assume it is 100%.** This predicate
            // used to be `dest_row >= 1.0`, which silently hard-codes
            // Road = full speed. Retail Road is 100% for Foot, Track, Wheel and
            // Amphibious but **Hover 75% and Float 0%**, so the old form is
            // right for the two ground defaults and wrong for exactly the Ship
            // run above: it would have reported 0 diverging on every map while
            // every qualifying water pair in fact diverges. What the override
            // does is force land row 1 and index it by the mover's own
            // SpeedType (`0x004B3C88 MOV ESI,1`, `LEA ECX,[ESI+ESI*8]` into the
            // table at `0x0089EA40`; the Ship copy at `0x006A32D7` is the same
            // shape), so the comparison is Road[SpeedType] against the
            // destination's own row for that SpeedType.
            let road_row = rules
                .terrain_rules
                .semantics_by_name("Road")
                .map_or(crate::util::fixed_math::SIM_ONE, |road| {
                    road.speed_costs.speed_multiplier_for(speed_type)
                });
            let diverges = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
                // The exact-zero substitution is applied to the COMBINED value
                // (`0x004B3DB0`, Ship `0x006A3403`), so a 0% forced row and a
                // 0% destination row both land on 0.5 and agree; any other
                // inequality is a real divergence.
                road_row != track_row(cell)
            };

            let (mut pairs, mut forced, mut diverging) = (0u32, 0u32, 0u32);
            // Review flagged a silent-failure mode this closes: a land row that
            // failed to parse comes back `None`, and `speed_multiplier_for` answers
            // `SIM_ONE` for `None` - which reads as both passable and full speed,
            // manufacturing exactly the zero this census reports. Printing the row
            // distribution makes that visible instead of invisible.
            let mut dest_rows = [0u32; 12];
            for ry in 0..terrain.height() {
                for rx in 0..terrain.width() {
                    let Some(cell) = terrain.cell(rx, ry) else {
                        continue;
                    };
                    if !passable(cell) {
                        continue;
                    }
                    for (dx, dy) in [
                        (-1i32, -1i32),
                        (0, -1),
                        (1, -1),
                        (-1, 0),
                        (1, 0),
                        (-1, 1),
                        (0, 1),
                        (1, 1),
                    ] {
                        let (nx, ny) = (i32::from(rx) + dx, i32::from(ry) + dy);
                        let (Ok(nx), Ok(ny)) = (u16::try_from(nx), u16::try_from(ny)) else {
                            continue;
                        };
                        let Some(neighbour) = terrain.cell(nx, ny) else {
                            continue;
                        };
                        if !passable(neighbour) {
                            continue;
                        }
                        pairs += 1;
                        // The same absolute difference the binary takes, on the same
                        // signed Level byte.
                        let delta =
                            (i32::from(cell.level as i8) - i32::from(neighbour.level as i8)).abs();
                        if delta >= 2 {
                            forced += 1;
                            dest_rows[usize::from(neighbour.land_type.min(11))] += 1;
                            if diverges(neighbour) {
                                diverging += 1;
                            }
                        }
                    }
                }
            }
            // Each share against its own denominator: review caught both counts
            // being divided by `pairs` while the sentence read "of those".
            let share = |n: u32, of: u32| {
                if of == 0 {
                    0.0
                } else {
                    f64::from(n) * 100.0 / f64::from(of)
                }
            };
            let rows: Vec<String> = dest_rows
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .map(|(land, n)| {
                    let land = u8::try_from(land).ok();
                    let name = land
                        .and_then(crate::rules::terrain_rules::LandType::from_index)
                        .map_or("?", |land_type| land_type.section_name());
                    // Review: printing the land-type INDEX alone cannot tell a
                    // real 100% row from a row that failed to parse, because
                    // `cost_for_speed_type` answers `None` there and
                    // `speed_multiplier_for` turns `None` into full speed - so a
                    // wholly collapsed table would print an identical histogram
                    // and an identical zero. Printing the resolved percentage,
                    // and "UNPARSED" when the row is absent, closes that.
                    let pct = land
                        .and_then(|land| rules.terrain_rules.semantics_for_land_type(land))
                        .map_or("UNPARSED".to_string(), |semantics| {
                            semantics
                                .cost_for_speed_type(speed_type)
                                .map_or("UNPARSED".to_string(), |cost| format!("{cost}%"))
                        });
                    format!("{name}={n}@{pct}")
                })
                .collect();
            println!(
                "A8 cliff-override census {map_name} [{speed_type:?}]: of {pairs} passable \
             adjacency PAIRS (both cells inside the playfield and above \
             {speed_type:?}=0%), {forced} differ by two or more levels ({:.2}% of \
             pairs), and {diverging} of those have a destination row differing from the \
             forced row ({:.2}% of those). Destination rows of the {forced}: {}. The \
             test is Road[{speed_type:?}] against the destination's own row for the same \
             SpeedType, NOT an assumption that Road is 100 percent - it is not for Hover \
             (75) or Float (0). That count is an upper bound for an UNDAMAGED mover: the \
             slope coefficient multiplies both sides, so a row at or above 1/1.2 still \
             reaches the SetSpeedFraction clamp from both directions and shows nothing. \
             The Float pass MEASURES the Ship copy of this override \
             (ShipLocomotionClass::Process_Movement 0x006A1C80, test at \
             0x006A2BDD/0x006A2BED/0x006A2BF0), because Float is 0 percent on every land \
             row but Water, so this same gate reduces the Float pass to the water cells. \
             STILL NOT MEASURED, and it would raise the figure: the bridge arm, which \
             adds 4 to the mover's side on every step of a deck crossing. Pairs, not \
             traversals.",
                share(forced, pairs),
                share(diverging, forced),
                rows.join(" ")
            );
        }
    }
}
