# Fresh-file SED description reader

This increment corrects the shared RMG description reader before saved-seed
catalog and dialog work. It does not establish saved-browser or shell parity.

## Native behavior established

Binary: retail `gamemd.exe`, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Read-only Ghidra inspection and independent criticism covered
`INIClass__ReadCommaHexUTF16` (`0x00528F00`), MapSeed Load (`0x00597A30`),
metadata (`0x00597D60`), INI load/parser (`0x00525A10`/`0x00525A60`), and
the original CRC/scanner instructions.

Both MapSeed callers construct and load a fresh INI before reading
`RandomMap.Description`. Its section-pointer cache is empty. On that cache
miss, `0x00529023` stores the section CRC in the scratch later passed to
`sscanf("%x")` at `0x0052911E`. Native CRC of literal `RandomMap` is
`0x1597B573`. The scanner's result is ignored: a failed first conversion
therefore emits UTF-16 `0xB573`; a later failure repeats the previous value.
An already matching section-pointer cache bypasses that initialization, so
this rule must not be generalized to arbitrary INI reader calls.

The reader copies at most `0x5000` encoded bytes and forces byte `0x4FFF`
to NUL, trims bytes at most `0x20`, and tokenizes on comma only. Empty comma
tokens disappear. Missing or trimmed-empty values use the caller's default;
comma-only values produce an empty result. Hex conversion accepts leading
C whitespace, an optional sign and `0x` prefix, consumes a leading hex run,
and wraps at 32 bits. A bare/invalid `0x` prefix fails conversion. Each token
writes the low 16 bits. A decoded NUL affects final `wcslen`, although the
native loop still processes later tokens.

The supplied count 128 permits 128 units **plus** a terminator. In native
metadata that final terminator overwrites the return address's low word;
in Load it reaches `this+0x178`, the following storage pointer. This is
established corruption, not a demonstrated particular crash outcome.
Browser-produced descriptions are at most 79 UTF-16 units. Rust bounds its
allocation safely; it does not reproduce adjacent-memory corruption.

## Implementation and comparison

[`description.rs`](../../../src/map/rmg/description.rs) owns this fresh-file
conversion, separate from numeric RMG options. `RmgOptions::apply_sed`
delegates to it. The saved-file loader supplies its localized default before
applying the INI, retaining the existing numeric overlay/normalization path.
The direct `.SED` launch path also consumes `apply_sed`; its existing choice
of default description is not changed by this increment.

[`sed_description.py`](../../../tools/storage_oracle/sed_description.py)
executes original reader, CRC, copy, trim, tokenizer and scanner instructions.
It supplies one valid INI section/entry and substitutes only the CRT
thread-storage accessor. File parsing and MapSeed caller execution are outside
the emulation. The output fixture has spare allocation to observe the reader's
128-unit boundary safely. The cached-section diagnostic supplies a different
prior stack pattern and is excluded from the Rust comparison.

The committed JSON and metadata sidecar cover 28 cold-cache fixtures (including
missing section/key and the encoded-source cutoff) and one cached diagnostic.
The production decoder test compares raw visible UTF-16
units with those original outputs. At the reader-only increment, options still
used a lossy `String` boundary. The subsequent [browser increment](2026-09-12-saved-seed-browser.md)
replaces it with raw `SeedDescription` UTF-16 storage and a persistence regression;
display conversion remains explicitly separate.
The disk-load regression covers missing/blank, comma-only, failed tokens,
NUL and prefixed values alongside numeric overlay and unchanged input state.
It also exercises parse → constructor-default options → `apply_sed`, the
sequence used by direct seed scenario loading; it does not run terrain generation.

The independently confirmed reader comment at Ghidra `0x00528F00` was saved and
read back. No function names, boundaries, types, or executable bytes changed.

Reproduce from the repository root with the verified retail executable selected:

```powershell
$env:VERA20K_GAMEMD_EXE = '<retail install>/gamemd.exe'
python -m tools.storage_oracle.sed_description --check
```

Before any Cargo command, follow the shared compiler preflight in `AGENTS.md`.
Focused checks are `cargo test -p vera20k --lib map::rmg::description::` and
`cargo test -p vera20k --lib map::rmg::saved_seeds::`.

Final candidate validation (2026-09-12): full `cargo test -p vera20k --lib`
exited 0 with **8,690 passed, 0 failed, 121 ignored**. Clippy exited 0 with
1,147 repository warnings. The original-reader `--check` passed for all 29
fixtures without writing files. Changed-file whitespace and document links passed.
Independent read-only review reproduced the native comparison and confirmed
the source, fixture, caller coverage and Ghidra comment; no unresolved findings.

## Still required

The [saved-browser follow-up](2026-09-12-saved-seed-browser.md) owns metadata,
sorting, generated filenames, transactions, editing, columns, input routing and
ordinary Load regeneration. Its report records current validation and bounded
limits. Native visual comparison and the
[whole-shell acceptance inventory](../../plans/2026-09-12-retail-shells-acceptance.md)
remain broader than this reader increment.
