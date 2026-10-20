# Optional Ares custom armor for ordinary weapons

Build with `--features ares` to register `[ArmorTypes]`, bind object `Armor`
names, and read warhead `Versus.<name>` plus its `.ForceFire`, `.Retaliate`
and `.PassiveAcquire` switches. Default builds retain the original eleven
armor names and raw native Verses reader.

The retained Rules registry owns the ordered armor catalog. Each processed
type owns its armor index; each warhead owns its independent Ares verses
vector. RuleSet exposes immutable projections, and ordinary damage, estimates,
weapon selection, fire admission, retaliation, threat scoring and Terrain
receivers read the same effective warhead/armor record. The original raw
warhead array is not populated from Ares's independent vector.

All armor names are allocated before defaults are read. Newly appended
entries inherit an earlier entry's already applied overrides; a forward
reference takes its own fallback when its target has not been initialized.
Omitted reads retain prior records. Definition history and bound IDs enter
the simulation configuration hash. Snapshot schema 462 stores the changed
retained Rules records and rejects older positional layouts; the Ares mode
metadata header continues to start at schema 459.

The [57 original-instruction controls](../tools/rules_oracle/ares_armor.md)
bound parser flags, inheritance, weapon admission and numeric damage results.
Rust regressions consume those goldens and exercise production rules and
ordinary consumers. Supplied CRT conversion and runtime services, full DLL
startup, complete effect lifecycles and whole-mod parity remain outside that
evidence. This feature does not implement EMP or AttachEffect. No mod rules,
assets, resource extraction or installation adapters are included.
