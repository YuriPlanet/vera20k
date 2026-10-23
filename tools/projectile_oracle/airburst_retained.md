# Retained Airburst reader flag

`python -m tools.projectile_oracle.airburst_retained --check` runs the original
BulletType constructor and whole rules reader through eight ordered histories.
It reads no game assets. The supplied INI indexes cover explicit yes/no, invalid
tokens, missing keys/sections and an empty token; the same native type survives
each read. Full text and all three RNG storages must remain unchanged.

The native byte at +294 retains its current value when a later token cannot be
read. Production processing must retain that byte in its existing per-type
registry, including after registry serialization. Rebuilding it from flattened
INI text loses an earlier true value. This affects both the existing parent
cluster gate and the already implemented child fan. Effective compatibility
hashing must distinguish those retained states despite identical final text.

The fixture supplies type/INI/vector storage, an empty ART and unrelated default
color registry, plus existing allocator/TLS/file transports. It does not execute
original save/load, a firing unit, child flight or damage. The separate
[`airburst`](airburst.md) corpus owns native fan execution and its boundaries.
