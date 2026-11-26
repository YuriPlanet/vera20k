# Spatial native comparisons

Each top-level `<name>.py` runs retail `gamemd.exe` code under Unicorn and checks
or writes `<name>.json` with a `<name>.meta.json` sidecar. See the
[shared native runner](../native_oracle.md) for setup, `--check`/`--write` and
evidence limits. Subdirectories hold larger packets with their own README
(for example [anytown_damage](anytown_damage/README.md) and
[fv_cell_attack](fv_cell_attack/README.md)).

## Shared fixture owners

Most producers build on a few fixtures. Extend these instead of copying their
setup.

| Module | Provides | Importers |
| --- | --- | ---: |
| `map_queries` | Sparse cell-table state for the original map lookup, normalization and playfield bodies; `dwords`, `packed`, `TABLE`/`EMPTY_TABLE`/`GLOBAL_TABLE` | ~220 |
| `building_body_rules` | BuildingType body readers over supplied INI lookups; `--render-bounds` replays the four consumed ordered image-bound controls; `SP`, `INI`, `RULES`, `TYPE`, `Fixture` | ~170 |
| `unit_source_scatter` | Source-aware Unit Scatter map and actor; `SCENARIO`, `CELLS`, `make_source_fixture` | ~40 |
| `refinery_dock` | Refinery/depot dock fixture; `make_dock_fixture`, `cell`, `HOUSE`, `BLD` | ~35 |
| `building_construction` | `JoinedFixture` and `joined_inputs`, the joined building world reused by building death, sale, repair, capture and factory output | ~30 |
| `anim_bouncer_launch` | Anim constructor through Bouncer; `constructor_state` decodes Anim state for construction, deploy, death, shrapnel, FV and AnyTown snapshots | ~20 |
| `guarded_arena` | `Arena`, guarded supplied malloc/free for fixtures that allocate | 2 |

A change to a shared owner changes every importer's inputs. Run the importers'
`--check` before publishing, and list affected corpora in the PR.

## Finding producers and consumers

```sh
git grep -l 'spatial_oracle.map_queries' -- tools   # Python importers of a fixture
git grep -n 'spatial_oracle/<name>.json' -- src     # Rust tests reading a corpus
python -m tools.spatial_oracle.<name> --check       # replay; the sidecar names the command
```

A harness stays only while a Rust test reads its output (AGENTS.md). Delete a
producer and its corpus together once its last consumer is gone.

## Fatal garrison evacuation

`python -m tools.spatial_oracle.building_death_anims --garrison-fatal --check`
replays `building_garrison_fatal.json`. Set `VERA20K_BUILDING_DEATH_ASSETS` to
the existing joined building fixture's physical input directory and `RA2_DIR`
to the retail install. The input directory contains RULESMD, fixed ARTMD,
optional LANGRULE, MPBattleMD, Hills.map/Hills.mmx and the joined fixture's
requested SHP files. The added civilian building image comes from the retail
temperate archive through the existing MIX reader.

The two controls use stock CAHSE01 with zero or one E1 occupant, seed 31 and
Skirmish mode 5. Whole Building ReceiveDamage and the shared Techno death arm
execute before SellBuilding and Infantry Scatter. The comparison stops at
DestructionEffects entry; prior garrison membership is supplied. It records
the two phases' Scenario words and the released infantry's destination and
lifecycle flags. The production-reader Rust consumer is
`sim::passenger::garrison_eject::tests::retail_fatal_debris_precedes_garrison_scatter`.
This covers evacuation ordering and those outcomes, not a complete native
death lifecycle or match. Use `--write` only to regenerate native goldens.
