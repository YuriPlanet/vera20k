# Retail fixture contracts

[Validation evidence](retail_fixture_contracts.validation.json) records the
strict test runs, native replay, release build and exact production comparison.

Library tests check the narrow tracked INI fixtures against the user's retail
installation. The fixtures support hermetic behavior tests; they are authored
test inputs, not native execution goldens. Matching retail fields establishes
their data provenance, not gameplay equivalence.

Run from the checkout root with the normal build owner:

```sh
export RA2_DIR='/path/to/Red Alert 2'
export VERA20K_REQUIRE_RETAIL_INI=1
export VERA20K_REQUIRE_RETAIL_ASSETS=1
python -m tools.cargo_run -- test -p vera20k --lib
python -m tools.cargo_run -- clippy -p vera20k --lib
```

`VERA20K_REQUIRE_RETAIL_INI` continues to require the extracted files in `ini/`.
`VERA20K_REQUIRE_RETAIL_ASSETS` requires the archive-backed checks' explicit
`RA2_DIR`. With neither an install nor an archive requirement, those checks
print `SKIPPED`; a configured but unusable install fails. Tests never mutate
process environment variables or write retail inputs.

Production source selection owns archive precedence, standalone `RULESMD.INI`,
optional `LANGRULE.INI`, fixed `ARTMD.INI`, and mode/map processing. The checks
reuse that owner and the existing fixture constructors. They do not layer
RA2's `RULES.INI` or `ART.INI` beneath the Yuri's Revenge files.

The former ignored integration test is now covered by these registered library
tests. Each reuses its consumer's fixture constructor or production reader:

| Coverage | Library test |
| --- | --- |
| Authored miner sections, HARV/CMIN fields, ore/gem values, capacities, unload cadence, refinery and terrain fields | `sim::miner::outbound_drive_tests::outbound_contract_matches_selected_retail_hills_battle` |
| Full mode roster and override fields | `skirmish_modes::tests::stock_contract_modes_match_selected_retail_roster_and_overrides` |
| Every neutral-tech name and footprint cell | `map::rmg::tech_catalog::tests::stock_contract_catalog_matches_selected_retail_startup` |
| Stock buildings, factories, rocking fields, warhead flags and C4 fields | `rules::retail_sources::tests::stock_hills_battle_fields_match_retail_contracts` |
| Production temperate loading and RMG tile-ID handoff | `map::theater::theater_tests::selected_retail_temperate_rmg_roles_reach_tile_ids` |

The miner and stock Rules checks use Hills/Battle through the retained startup
owner and the app's theater-before-mode source order. The random-map shell
catalog uses the retained startup projection, before scenario overlays. Authored
root comparisons remain separate from processed scenario-field comparisons.
The synthetic cumulative-tileset fixture remains synthetic; it is not compared
to retail tile numbers or presented as native output.

The theater ordinal resolver already has an executed native comparison:
[theater General reader](rules_oracle/theater_general_reader.md). Its library
replay protects the reader and ordinal decisions; retail loading checks protect
the connection from loaded theater data to the random-map tile IDs.

These checks do not replace the [full decoder corpus](retail_corpus.md),
[headless and production captures](map_observation.md), or native gameplay
comparisons. Other integration diagnostics and ignored tests still require
individual evaluation; this does not close issue #752 as a whole.
