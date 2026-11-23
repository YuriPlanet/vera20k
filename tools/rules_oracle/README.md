# Rules and type-reader native comparisons

Each `<name>.py` runs original reader code (gamemd, or the Ares DLL for
`ares_*`) under Unicorn and checks or writes its `<name>.json` with a
`<name>.meta.json` sidecar. Most have a `<name>.md` with their inputs and
coverage. See the [shared native runner](../native_oracle.md) for setup,
`--check`/`--write` and evidence limits. Many need extracted retail INIs or SHPs
in an environment variable named in their docstring.

## Shared fixture owners

| Module | Provides | Importers |
| --- | --- | ---: |
| `bridge_anim_inputs` | `Reader`, a machine that runs the original type readers over supplied ART/INI sections and physical SHPs | ~40 |
| `bridge_child_sound` | `sections` (lexical INI sections for the native readers) and `Sound`, the selected retail sound registry | ~35 |
| `weapon_speed_order` | `retail_history`/`RetailRulesInputs`: original Rules constructor, Process and type post-passes over the retail layers | ~15 |
| `ares_prerequisites` | `Probe` and `identified_bytes`/`ARES_SHA256`, the Ares DLL loader the other `ares_*` controls share | ~13 |
| `bridge_landing_inputs`, `bridge_anim_lists` | Non-ART bridge debris readers and Anim list inputs, mostly reused by spatial bridge packets | ~11 each |

Spatial and projectile producers import these too. Run their `--check` after
changing one.

## Finding producers and consumers

```sh
git grep -l 'rules_oracle.bridge_anim_inputs' -- tools   # Python importers
git grep -n 'rules_oracle/<name>.json' -- src            # Rust tests reading a corpus
python -m tools.rules_oracle.<name> --check              # replay
```

A harness stays only while a Rust test reads its output (AGENTS.md).
