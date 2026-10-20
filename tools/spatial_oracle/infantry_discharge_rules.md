# Infantry ART discharge-frame reads

`infantry_sequence_rules.py --discharge` runs the original InfantryType ART
prefix `5246BE..52473A`, including all four original `ReadInt5276D0` calls.
It saves a separate [150-row receipt](infantry_discharge_rules.json) and
[provenance](infantry_discharge_rules.meta.json). The ordinary invocation keeps
the historical 76 sequence cases and their 42-action records unchanged.

The four signed DWORDs at `E40/E44/E48/E4C` are respectively `FireUp`,
`FireProne`, `SecondaryFire` and `SecondaryProne`. Each read passes its own
retained field as the default. There is no clamp, byte conversion or fallback
to another key. The executed constructor receipt uses original EBX zeroing
at `5236A7` and original stores `5236D7..5236F9`; base construction and vector
allocation are outside that receipt.

The existing fixture supplies INI cache backing at reader entry. Both modes
reuse one cache owner; original CRC lookup, ReadInt and CRT numeric parsing
execute unchanged. CRCs for supplied indexes are prepared by original
`4A1DE0` before emulation, avoiding nested Unicorn VM collection in a live
callback. No native function's result or original instruction is substituted.
At the prefix boundary the fixture supplies ESI/EDI, the section name and AL
from the preceding Crawls read. The receipt records the incidental Crawls
store but does not establish that earlier read.

Rows cover every field independently from zero and supplied retained state,
omission, present-empty and malformed values, signed limits, values above255,
decimal overflow, hex, key case and reload order. Entry traces record every
key/default and the actual returned value in native read order. Physical
`ARTMD.INI[GI]` is extracted through the existing lexical-input owner and its
full file SHA256 and section lines are saved. Native outputs are `[2,0,0,0]`:
only `FireUp=2` exists in that section. Physical file/archive loading is a
supplied premise. This receipt does not establish firing timing or rendered
sequence behavior.

```sh
export VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/physical/extract
PYTHONPATH=. python -m tools.spatial_oracle.infantry_sequence_rules --discharge --check
PYTHONPATH=. python -m tools.spatial_oracle.infantry_sequence_rules --check
```

`--write` deliberately replaces the selected receipt and its provenance.
Selected provenance pins both this generator and the shared retail lexical
owner. Original instruction slices are saved and checked unchanged. The
default payload's canonical SHA256 remains
`03d8276eaee8ac553333412005bdc0602387ddbe6ded9e4fd06295c66871055f`.

Rust comparison entry points are
`rules::art_data::tests::native_discharge_art_reader_matches_original_signed_independent_keys`
(all150 cached-input rows through the production reader, plus75
constructor-state production registry rows),
`rules::ruleset::tests::native_discharge_retail_gi_binding_retains_independent_zero_defaults`
(physical retail rules/art binding), and
`rules::ruleset::tests::native_discharge_signed_art_fields_reach_infantry_type`
(signed limits survive the ART-to-type projection). Cached-input tests use
the existing INI storage operation to retain present-empty/raw values supplied
by the native fixture. Physical loader rows add an unknown `FixtureOnly=1`
marker so omission/empty controls retain a section; it is not a discharge
input. Retained-state controls establish the shared reader's current-field
defaults, not a claim that the registry reloads live InfantryType objects.
