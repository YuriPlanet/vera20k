# Tactical certification tooling

This isolated package drives and validates the hidden
`radar-online-v2` VERA20k production checkpoint. It accepts only one sealed
Soviet or Yuri profile, uses an explicit fixed Battle launch, and never focuses
the desktop or injects operating-system input.

The v3 profile requires the current native 1x sidebar and the production GAME.FNT bitmap atlas. Geometry is compiled
by `sidebar::layout_spec::SidebarChromeLayoutSpec` and covered by the pinned
executable identity; no external RON layout is loaded. The original v1/v2 profiles remain unchanged as historical inputs. Current admission
rejects them clearly: v2 manifests recorded an egui pass and a pinned Windows
Verdana file, while v3 records the font bytes consumed by the shared BitFont
owner. The unchanged contract.v2.json continues to seal the environment policy;
its version is independent of the capture/profile evidence schemas.

Construction milestones remain exact simulation ticks. The production radar
uses real wall time, so v3 observes Online within a 4,096-tick / 20-second opening
budget and an 8,192-tick overall cap. A second complete readiness observation
must follow one tick later; capture follows 16 stable frames after that.
Soviet/Yuri source identities and the current 800x600 sidebar geometry are checked.

The output is Rust production-route regression evidence. It has no native
comparator and makes no parity certification. Tool results are only `VALID` or
`INVALID`.

Validate either tracked profile:

```powershell
python -m tools.tactical_certification validate-profile `
  --profile C:\path\to\tools\tactical_certification\profiles\soviet-radar-online-v3.json
```

Run one capture:

```powershell
python -m tools.tactical_certification capture `
  --profile C:\path\to\tools\tactical_certification\profiles\soviet-radar-online-v3.json `
  --contract C:\path\to\src\app\diagnostics\tactical_capture\contract.v2.json `
  --executable C:\path\to\vera20k.exe `
  --working-directory C:\path\to\ra2-rust-game `
  --run-dir C:\path\to\brand-new-run
```

The wrapper creates the outer run directory exclusively. The child publishes a
separate `capture` directory atomically; a valid child directory contains
exactly `capture.json` and `frame.bgra`. Wrapper-owned retained profile,
stdout, stderr, validation, and run-report files are created exclusively and
fsynced. `run.json` is written last.

Before launch, the wrapper rejects links, junctions, reparse points, loose
`Fight.MAP` and `GAME.FNT` shadows, state-affecting environment variables, a
noncanonical retail root, and any identity mismatch in the executable, config,
profile, contract, `multimd.mix`, or the `ra2.mix` archive containing the sealed
bitmap font. Every identity is rechecked after the child exits.

`VALID` additionally requires the complete v3 production evidence: accepted
controlled startup, exact tick/command/placement ledgers, stock map payload
identity, hidden and unfocused lifecycle, powered online radar authority, the
real sidebar/minimap/radar draw observations, a stable final fingerprint, and
a nonuniform BGRA readback. Font admission requires the actual production
GAME.FNT archive winner, its payload digest, the same source identity retained
by the installed bitmap atlas, and emitted sidebar text. Fallback fonts are
rejected. Window scale comes directly from winit; no egui scale or system font
identity is fabricated. Missing, extra, contradictory, or truncated
load-bearing evidence fails closed. The manifest, frame, and exact two-file
inventory are rechecked before validation returns.

The v3 child timeout is exactly 720 seconds. On timeout, the wrapper kills only
the still-live `Popen` child it created, waits a bounded five seconds, and reads
stdout/stderr from temporary regular files. It never uses a shell, pipes,
`taskkill`, a process group, descendant traversal, or process-name termination.

Validate an existing child capture or compare two same-profile runs:

```powershell
python -m tools.tactical_certification validate `
  --capture C:\path\to\run\capture `
  --profile C:\path\to\profile.json `
  --executable C:\path\to\vera20k.exe `
  --working-directory C:\path\to\ra2-rust-game `
  --output C:\path\to\brand-new-validation.json

python -m tools.tactical_certification validate-repeat `
  --first C:\path\to\run-a\capture `
  --second C:\path\to\run-b\capture `
  --profile C:\path\to\profile.json `
  --executable C:\path\to\vera20k.exe `
  --working-directory C:\path\to\ra2-rust-game `
  --output C:\path\to\brand-new-repeat.json
```

Different wall-clock scheduling can produce different Online/capture ticks.
Those observations remain in stable evidence: repeat validation can correctly
report `INVALID` even when each individual capture is valid. It does not hide
these differences or promise repeatability for a real-time animation.

Repeat validation compares exact BGRA bytes, the entire declared stable
evidence object, and typed profile/contract/frame identities. It excludes only
the separately declared run object plus host timestamps, paths, process IDs,
and durations. The two capture arguments must resolve to distinct directories;
one capture compared to itself is `INVALID`.

Focused tests:

```powershell
python -m unittest discover -s tools/tactical_certification/tests -v
```
