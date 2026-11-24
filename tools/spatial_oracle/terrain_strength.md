# Terrain Strength initialization

`terrain_strength.py` executes the original TerrainType constructor Strength
store at `0x0071DBAC`, ObjectType's exact `Strength` integer read/store at
`0x005F94D3..0x005F94F3` (including `CCINIClass::ReadInt` at `0x005276D0`),
and Terrain's successful-base-reader gate and fallback at
`0x0071DEC0..0x0071DEE2`. The constructor stores `-1`; a successful body read
replaces a stored `-1` with the current `RulesClass+0x1144` TreeStrength.
An explicit `Strength=-1` therefore has the same fallback as a fresh type
whose Strength key is missing.

```sh
python -m tools.spatial_oracle.terrain_strength --check
```

Run from the repository root with the executable and Unicorn configured as in
[the native comparison workflow](../native_oracle.md). The sidecar pins the
input boundary and substitutions. There are 69 fresh-type
rows (68 successful reads and one supplied failed-base-reader gate) plus three
two-pass retention characterizations. The complete ObjectType reader and
scenario rules-pass driver do not execute. Resolved TreeStrength is supplied;
this does not establish the Rules constructor default or its General reader.

The input `raw` is authored text. `cached_raw` is the supplied lookup value
after ASCII bytes <= `0x20` are trimmed and an empty result is omitted, following
the existing `INIClass::LoadFromStraw` (`0x00525A60`) loader contract. The section
has another nonempty key. This avoids treating an artificial cached empty entry
as physical INI input; the harness does not execute lexical loading itself.

The production regression
`rules::terrain_object_type::tests::bridge_tree_strength_reader_matches_original_constructor_and_fallback`
uses the physical `IniFile` parser and `RuleSet` reader for the 68 successful
fresh rows, then reads retail `RULESMD.INI` and selects the matching native
TREE01 input. This is fresh-type initialization coverage, not full reader
equivalence.

## Selected retail inputs and retained-state residual

The production `AssetManager`/`IniFile` exporter
[`bridge_retail_terrain_inputs.rs`](bridge_retail_terrain_inputs.rs) and its
[`saved input rows`](bridge-retail-terrain-inputs.json) inspect the selected
Hills Battle inputs. Only present keys are emitted; an unavailable file emits
`absent:true`.

| Layer | General TreeStrength | TREE01 / TIBTRE01 Strength |
| --- | --- | --- |
| RULESMD.INI | `200` | both absent |
| LANGRULE.INI | file absent | file absent |
| MPBattleMD.ini | absent | both absent |
| Hills.mmx | absent | both absent |

Thus the selected Hills plain-tree receiver starts with Strength 200 and does
not trigger the retained-state discrepancy below. This is not an audit of all
retail modes, maps or Terrain types.

The original integer reader takes the object's current Strength as its default.
The three saved two-pass outputs establish these cases:

| First pass: TreeStrength 200 | Second pass: TreeStrength 375 | Final native Strength |
| --- | --- | --- |
| Strength missing | Strength missing | 200 |
| Strength `-1` | Strength missing | 200 |
| Strength missing | Strength `-1` | 375 |

The current Rust `native_processing::process_plain_family(Terrain)` retains
merged raw type sections, while `RuleSet` creates the Terrain object once from
that projection and the final General TreeStrength. Consequently a later
mode/map General override can recompute an earlier fallback even when the
later type Strength key is omitted. A later explicit `-1` should recompute;
omission should retain the earlier resolved field. Fixing this requires
per-pass resolved Terrain scalar ownership, including allocation timing and
the successful-body-read gate, rather than another final-parser fallback.

This remains a required separate rules-owner chain for existing Terrain types
under such later overrides. Its frequency is conditional on the later inputs;
its downstream risk is different health, destruction timing and bridge-debris
damage outcomes for affected map objects. The fresh-type correction and the
selected retail Hills comparison do not close that residual or the whole
bridge goal.
