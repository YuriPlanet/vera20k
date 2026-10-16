# INI token readers: atoi, strtok, ReadString, ReadInt and ReadBool

`ini_token_readers.py` executes original gamemd.exe SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Its JSON results are native outputs, not values copied from Rust. The
`.meta.json` file pins the payload, entry points and substitutions.

## Native behavior established

- CRT `atoi` (`0x007C9BFD` -> `atol` `0x007C9B72`) skips C `isspace` bytes
  (0x09-0x0D, 0x20) through the static `_pctype` table, takes one sign and
  accumulates decimal digits in 32 bits: `2147483648` reads -2147483648,
  `4294967297` reads 1. `0x10`, `$10` and `1e3` stop at the first non-digit.
  Latin-1 0xA0 is not a space.
- CRT `strtok` (`0x007C9CC2`) with `","` skips empty fields and keeps each
  token's spaces.
- `strtrim` (`0x00727CF0`), reached from ReadString and the INI loader
  (`0x00525A60`), drops leading bytes <= 0x20 and shifts the text down. Its
  trailing loop then stops once the byte at the old start offset has been
  cleared, so a trailing run reaches back no further than that offset:
  `"  x  "` reads `"x "`, `"  x "` reads `"x "`, `"    ab  "` stays
  `"ab  "`, and `" ab  "` reads `"ab"`. A value without leading bytes, which
  is all the loader stores, trims fully.
- `CCINIClass::ReadString` (`0x00528A10`) copies the value, or the default for
  an absent key, into the buffer cut at capacity - 1 and strtrims it.
- `ReadInt` (`0x005276D0`): `$` prefix or `h`/`H` suffix selects hexadecimal,
  where a missing digit keeps the default; otherwise `atoi`.
- `ReadBool` (`0x005295F0`) decides on the untrimmed first byte: `1`/`T`/`Y`
  true, `0`/`F`/`N` false, anything else (a leading space included) keeps
  the default.

None of these draws RNG, writes a timer or detaches anything (instruction
reading).

## Rows

- `atoi`: 32 strings (signs, spaces, control and Latin-1 bytes, overflow,
  prefixes and suffixes).
- `strtok`: 13 strings, including the constructor-list fixture and Latin-1.
- `read_string`: capacities 0x14, 0x18, 0x19, 0x20, 0x40 and 0x80 over short,
  exact, over-long, cut-before-space, padded, control and Latin-1 values, and
  absent keys with four defaults; 16 more trim patterns at 0x80; and 8
  values or defaults whose copy is empty. Each row records the copy and
  ReadString's return value (its length).
- `read_int`: 28 values under defaults 9 and -1.
- `read_bool`: 17 values under both defaults.

## Production owner and comparison

`rules::ini_value` owns every reader: `crt_atoi`, `strtok`, `strtrim_ascii`,
`IniSection::read_string`/`read_name`, `read_int` and `read_bool`.
`rules/ini_token_readers_tests.rs` replays every row through them, and checks
that `read_name` is `None` exactly where an empty-default copy returns 0.

## Not covered

- ReadString, ReadInt and ReadBool run on the building_body_rules cached
  section, index and entry objects, not on a loaded file; the loader's own
  line handling is not executed here.
- `CString::Tokenize` (`0x007B5F10`, `read_trimmed_list`), ReadRect,
  ReadMinMax and `sscanf` readers are not executed by this oracle; other
  oracles cover `sscanf` fields (building_body_rules, infantry_sequence_rules).

## Reproduce

Set `VERA20K_GAMEMD_EXE` or `RA2_DIR`, use Unicorn 2.1.4 and run from the
repository root:

```sh
PYTHONPATH=. python -m tools.rules_oracle.ini_token_readers --check
cargo test -p vera20k-core --lib -- rules::ini_token_readers_tests::
```
