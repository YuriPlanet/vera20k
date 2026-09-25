# Shell certification tooling

This package validates the one-shot production `main-menu-0xe2-steady` capture
bundle and compares four physical presentation crops with the sealed native
guard. It never writes to the guard and never performs desktop input or native
capture.

The Rust artifact is a tight logical 800x600 BGRA8 frame. Guard hashes are not
logical-crop hashes: they cover half-open `presentation_rect` crops in the
1920x1080 DDrawCompat surface. The comparator reconstructs each presentation
crop from the full logical frame using the verified point mapping
`source=floor((2*destination_offset+1)*5/18)` relative to the content rectangle
`[240,0,1680,1080)`, then hashes the resulting tight BGRA8 bytes. It never
compares a direct `logical_rect` crop with a guard digest.

Compare an existing bundle:

```powershell
python -m tools.shell_certification compare `
  --capture C:\path\to\new-run `
  --guard $env:VERA20K_SHELL_GUARD `
  --output C:\path\to\new-run\comparison.json
```

Run the explicit hidden capture child and compare it:

```powershell
python -m tools.shell_certification capture-and-compare `
  --executable C:\path\to\vera20k.exe `
  --working-directory $env:VERA20K_REPO_ROOT `
  --guard $env:VERA20K_SHELL_GUARD `
  --run-dir C:\path\to\brand-new-run-directory
```

Derive the enrolled RGB565 presentation codebooks from all three source frames
named by the sealed guard:

```powershell
python -m tools.shell_certification derive-presentation-profile `
  --guard $env:VERA20K_SHELL_GUARD `
  --oracle-runs $env:VERA20K_ORACLE_RUNS `
  --output C:\path\to\brand-new-presentation-profile.json
```

Profile derivation validates the guard's sealed SHA-256, accepts exactly three
guarded source frames, and resolves every source only beneath the supplied
Oracle runs root. It rejects traversal, links, non-files, concurrent mutation,
wrong byte length or digest, non-opaque alpha, source-table disagreement,
non-32/64/32 channel cardinality, and different blue/red five-bit tables. The
canonical `vera20k.shell-presentation-profile.v1` JSON is written once and
never replaces an existing path. Neither the guard nor any source run is
modified. Keys and source-derived values are deterministic and sorted; the
documented `generated_at_utc` field is the only intentionally variable value.

The derived tables describe only the enrolled active
retail/AMD/DDrawCompat/DXGI presentation environment sealed by that guard. They
are not a universal `gamemd.exe` RGB565 expansion claim and do not certify
other shell states, resolutions, adapters, display pipelines, or blended
packed-domain behavior. The artifact therefore records
`parity_certification` as `NONE`; it is executable input evidence, not a
completion or parity certificate.

Generate the title-specific RED differential from an existing immutable Rust
capture and all three native source frames named by the guard:

```powershell
python -m tools.shell_certification title-differential `
  --capture C:\path\to\immutable-rust-capture `
  --guard $env:VERA20K_SHELL_GUARD `
  --oracle-runs $env:VERA20K_ORACLE_RUNS `
  --output C:\path\to\brand-new-title-differential.json
```

The title report validates stable source reads and hashes, collapses the sealed
point-scaled crop only after proving every replica of each logical pixel
agrees, searches every in-bounds glyph-mask translation, and derives the
terminal Path-A tint through native-source RGB565 codebooks. It never
overwrites an output or mutates the guard/run tree. A RED report exits with
status 1 because the current capture is still drift; an invalid evidence input
exits with 2.

The additive `main-menu-0xe2-entry-sequence` path is separate from the sealed
steady schema and comparator. It validates exactly 18 final-swapchain BGRA
frames (ticks `0..17`, the slide engine's loop at 800x600) after the RGB565
presenter:

```powershell
python -m tools.shell_certification validate-entry-sequence `
  --capture C:\path\to\immutable-entry-sequence

python -m tools.shell_certification capture-entry-sequence `
  --executable C:\path\to\vera20k.exe `
  --working-directory $env:VERA20K_REPO_ROOT `
  --run-dir C:\path\to\brand-new-entry-sequence
```

The bundle inventory is exactly `capture.json` and `frames.bgra`. The payload
is 34,560,000 bytes: eighteen contiguous 800x600 BGRA frames of 1,920,000
bytes each. Validation requires one generation, ordered ticks and offsets,
completion after tick 17, neutral software cursor identity, bare dialog
`0xE2`, and presenter domain `final-swapchain-after-rgb565`. The validator
reports manifest, payload, and per-frame SHA-256 values as provenance.

A valid Rust sequence is not native pixel parity evidence. Only the settled
tail ticks have a native comparison (retail `to-mm.png`, recorded in
`docs/research/shell/2026-09-24-slide-transitions-evidence.md`); movie phase,
child text, cursor/focus behavior and every other native sequence row remain
`UNVERIFIED`. The capture command must not
be launched when the enrolled Oracle/capture safety status is invalid,
unenrolled, stale, or ambiguous.

The examples use `VERA20K_REPO_ROOT`, `VERA20K_SHELL_GUARD`, and
`VERA20K_ORACLE_RUNS` so paths remain explicit and portable. The optional
sealed-evidence unit test also reads `VERA20K_SHELL_CAPTURE`; it skips unless
all three evidence paths are configured.

The working directory is required because VERA20k loads `config.toml` and
other relative resources from its process working directory. The wrapper
requires an absolute non-link directory and a regular non-link `config.toml`,
passes that directory as the child `cwd`, and records the config path plus
pre/post SHA-256 without serializing its contents. A changed config forces
`INVALID`.

The wrapper uses no shell and, on timeout, kills only the exact child PID it
created. It retains partial diagnostics and refuses to overwrite a run
directory or evidence file.

Exit codes are `0` for `MATCH`, `1` for `DRIFT`, and `2` for `INVALID` or a
tooling error. A region `MATCH` applies only to that named point-scaled
presentation crop. It does not certify the whole frame, movie phase, OS
compositor/display behavior, cursor, route transitions, input, audio, another
dialog, or another resolution.

Focused tests:

```powershell
python -m unittest discover -s tools/shell_certification/tests -p 'test_*.py' -v
```

## Diagnostic skirmish capture

`skirmish-0x102-steady` is a separate, unenrolled diagnostic checkpoint. It
follows ordinary Main Menu → Single Player → Skirmish action handlers, waits
for the slide and three text reveals, observes a steady frame, then records the
next final swapchain frame. It does not synthesize desktop input. The existing
main-menu guard, schemas and comparison commands do not accept this checkpoint.

Run from the intended resource directory containing `config.toml`, using an
absolute executable path and a new absolute output directory:

```powershell
& C:\path\to\vera20k.exe --shell-capture skirmish-0x102-steady `
  --width 800 --height 600 --cursor-x 400 --cursor-y 300 `
  --output C:\path\to\new-skirmish-capture
python -m tools.shell_certification.skirmish C:\path\to\new-skirmish-capture
```

The hidden process has a 60-second capture budget after initialization. As with
other capture modes, startup options use deterministic defaults rather than
loading the physical RA2MD.INI options profile; skirmish selections still follow
their production initialization. Record executable, config, asset and selection
identity for any comparison. The built-in command records selection state but
does not itself enroll or hash all external inputs.

The validator checks the two-file inventory, route ordering, selection shape,
dimensions and frame digest. Exit 0 means a valid diagnostic bundle; exit 2
means invalid input. Every result explicitly says `parity_certification: NONE`.
The parsed-map digest hashes a Rust Debug representation solely to detect a
selection change within this capture. It is not a stable asset fingerprint.
Native input enrollment, reference capture and pixel comparison remain separate
work. See the [bounded capture evidence](../../docs/research/skirmish-ui/2026-09-12-production-capture.md).
