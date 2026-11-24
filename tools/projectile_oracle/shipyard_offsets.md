# Shipyard target coordinate

`python -m tools.projectile_oracle.impact_ladder --shipyard-offsets --check`
replays `shipyard_offsets.json`. It needs the supported `gamemd.exe` and the
physical `ini/rulesmd.ini` and `ini/artmd.ini` in the checkout. `--write`
deliberately replaces the corpus and its binary-identity/command sidecar.

The existing `building_prism.laser_type_inputs` fixture executes each original
BuildingType constructor, then the original focused readers over physical retail
strings. The coordinate reader is `460F50..460F9C`, calling `ReadCoord529CA0`
with the retained three dwords as default. The controls use GAYARD, NAYARD,
YAYARD and the zero-offset GAPOWR. Source hashes and reader inputs are in the
corpus. These focused reads are not a replay of the complete Scenario loader.

The existing impact-ladder harness then executes Bullet `468D80` until its first
`DetonateAtCoord4690B0`. These rows install the actual Building virtual+A4
(`4500A0`), which calls the supplied +48 centre and adds the original reader's
offset. Virtual+58 executes Object410540 and forwards to the same +48 centre.
The +48 centre `(5000,6000,416)`, foundation dimensions,
IsInAir, layer and RTTI are fixture inputs. Width/height and the other getters
remain supplied boundaries; no claim is made about executing their owners.
The rows cover near-centre admission, Inaccurate bypass and distance rejection.

The Rust consumer in `world/projectile_collision.rs` loads the retail rules
through the production mode/map source stack, spawns each building, and translates
the native coordinates to that building's actual centre. It checks the production
collision facts and impact resolver against the native detonation, as well as the
unchanged +58 coordinate. The physical reader offsets and complete impact
instructions execute; area damage, terrain destruction, rendering and a full
native battle are outside this corpus.
