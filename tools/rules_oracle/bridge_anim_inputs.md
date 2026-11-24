# Bridge debris animation input evidence

`bridge_anim_inputs.py` independently runs original `AnimTypeClass` constructor
`427530`, `ObjectTypeClass::ReadINI` `5F92E0`, full animation ART reader `427D00`,
and image loading/metadata `427B50`. It supplies exact lexical strings from
physical retail `ARTMD.INI` and full physical SHP bytes at the archive I/O boundary.
It does **not** initialize type fields from VERA's interpreted values. Only after
native execution, it asserts the native results against the production exports
used by `bridge_debris_producer.py` and `bridge_debris_flight.py`.

The saved 24 retail rows cover the 15-entry native MetallicDebris vector (including
literal `D`), four BridgeExplosions, WAKE1, three H2O_EXP types and SMOKEY2. The list
and literal Image25 contracts have separate original-reader evidence in
[bridge_anim_lists.py](bridge_anim_lists.py) and [anim_image.py](anim_image.py).
This corpus adds two asymmetric Scorch/Crater controls because all selected retail
entries set those two flags identically.

## Input chain

| Injected input | Independent native or physical source |
| --- | --- |
| Start, LoopStart, LoopEnd, End, LoopCount, Rate | Original constructor and full ART reader; Rate's authored positive value becomes `900 / value`, while an absent key retains the constructor rate. |
| Damage, Elasticity, MinZVel, MaxXYVel | Original `ReadDouble5283D0` and store instructions, after original CRT floating scanner initialization `7C8F5E`; outputs retain exact binary64 bits. |
| MaxZVel | Original constructor value, still 3.5 for all 24 rows; the producer leaves this field unchanged. |
| DamageRadius, TrailerSeperation | Original integer reader `5276D0` with the actual per-field default arguments. |
| Bouncer, Normalized, Scorch, Crater, Shadow | Original bool reads `5295F0`; distinct Scorch `+36B` and Crater `+36D` confirmed by asymmetric controls. |
| RandomRate | Original `ReadMinMax529880` and complete `428777..4287DC` post-read conversion; retail `220,600` stores `[1,1]`. |
| ExpireAnim, BounceAnim, TrailerAnim, Warhead | Original string reads, native registry lookups/allocation and pointer stores; the output dereferences native target IDs. HE's numeric Warhead rules are established separately. |
| SHP frame count, Middle, image-derived End/LoopEnd | Original image filename formation and header reads consume the unchanged physical SHP bytes. The output records each file's length, SHA-256 and eight-byte header. No frame count is manufactured. |
| `D` absent ART/body/image | Original constructor followed by failed original ART section admission: no image lookup, frames/end/loop-count zero, rate one, no Bouncer or references. The historical exports mark this missing runtime config; the current Rust correction is checked separately. |

Every actual supplied ART field in both production exports is checked against the
native outputs on each generator run. The fixtures' `Normalized` value was only
present in the runtime debug string; the comparison extracts that token explicitly.
Saved payloads identify the compared fields and names. This is independent native
input evidence, not a comparison of native execution against values copied back
from that same execution.

The retail image metadata established here is: all 14 DBRIS types 15 frames;
TWLT026/036/050 17; TWLT070 26; WAKE1 15; H2O_EXP1/2/3 16; SMOKEY2 20.
Middle is respectively 7, 8, 13, 7, 8 and 10 as read/computed by native instructions.

## Reproduction and validation

Use a built release `asset` binary and the configured retail installation. Extract
`ARTMD.INI` and the 23 SHPs named by `NAMES` in the generator, excluding `D`, with
`asset extract <filename> --out target/asset/bridge-anim-inputs`. The extraction tool
puts them under that directory's `extract/` child. Override that directory with
`VERA20K_BRIDGE_ANIM_ASSETS` if needed. The default also honors `CARGO_TARGET_DIR`
instead of assuming the current worktree owns the build directory. Do not build another Cargo process while
another owner is compiling.

```sh
PYTHONPATH=. VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
VERA20K_BRIDGE_ANIM_ASSETS=target/asset/bridge-anim-inputs/extract \
python tools/rules_oracle/bridge_anim_inputs.py --check
```

The executed local check used
`VERA20K_BRIDGE_ANIM_ASSETS=/tmp/bridge-debris-input-assets/extract`; the unchanged
files and extraction provenance remain there for reuse, without another extraction.

The checked ART file is 336535 bytes, SHA-256
`e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31`, resolved from
`ra2md.mix -> localmd.mix`. All 23 SHPs resolve from `ra2.mix -> conquer.mix`.
The saved output pins their individual identities. Native `--write` followed by
`--check` passed for all 26 rows. No Cargo or production load was run by this
harness task; the owning chain records those validations separately.
The Rust production-reader comparison is
`sim::world::bridge_debris_tests::retail_bridge_anim_inputs_match_original_full_art_reader`: it loads physical retail RULESMD/ARTMD through RuleSet and ArtRegistry,
compares all represented input fields against these independent native rows, and
asserts the corrected registered-D constructor behavior. Frame counts use this
corpus at the existing test binding boundary; the release load checks actual assets.

## Bounds

- The original physical INI loader and MIX precedence walk are not executed here.
  The selected unique sections/keys are extracted from physical ART bytes to
  prepare original signed-CRC caches. Numeric/bool/reference conversion and
  image filename construction are original code. ART is not scenario-layered.
- Original allocators return bump storage, deletion is inert, and the CRT TLS
  accessor returns prepared per-thread storage. Asset I/O returns the exact file
  bytes for the filename requested by original code.
- The sound registry is empty. Original sound lookup therefore retains `-1`, as
  explicitly bounded by the producer/flight harnesses; retail sound IDs, playback,
  and their possible observers are not certified.
- The corpus establishes input prerequisites for the bounded producer/primary
  flight comparisons. It does not establish whole-frame scheduling, child AI,
  audio, pixels or bridge parity as a whole.
