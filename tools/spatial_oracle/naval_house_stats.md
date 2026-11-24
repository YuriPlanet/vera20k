# House statistics through sinking saves

`naval_house_stats.py` executes the original concrete House save and load,
not merely a raw-memory-copy surrogate.

From the VERA20k repository root, with the native Python environment and retail
executable configured, run:

```sh
VERA20K_SHRAPNEL_INPUTS=target/shrapnel-native-inputs/extract \
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python tools/spatial_oracle/naval_house_stats.py --check
```

Only `--write` replaces references. This packet depends on the adjacent `naval_occupants.py` and the extracted
retail input directory described in [naval_repair.md](naval_repair.md).

## Executed results

- `actual_sinking`: actual original Cell repair-neighbor damage yields
  `UnitsLost=1`. Full House save/load retains 1. The original subsequent Unit
  `RecordKill` yields 2. This last call is an explicit terminal callback boundary;
  complete restored Unit AI and scenario loading are excluded.
- `mixed_counters`: both twenty-entry kill tables, loss counters 123/456,
  signed score -789, and four nonempty capacity-three built Counters round-trip
  exactly. Built totals 6/9/12/15 and all twelve entries survive.
- A separate no-init call on a copy retains the inline statistics but resets
  built Counters. This is why the full concrete load's appended Counter streams
  matter. Executed normal-constructor statistics blocks clear every covered
  table, scalar and Counter total.
- Main, Scenario and MapGen complete states remain unchanged through all these
  measured save/load and continuation operations.

## Native ownership and boundaries

House vtable `7EA8A0`: Load `+14=503040`, Save `+18=504080`,
size `+30=504730` returns `160B8` bytes. Save calls Abstract `410320`, which writes
the saved-this token and the full raw block, then the concrete House writes Base,
twelve Counter and dynamic-vector streams. Load calls Abstract `410380`, which
preserves the reconstructed receiver's live `+1C`, then no-init `4F5190`, then
twelve Counter loads `49FBE0`. All these functions execute to their ordinary
returns. The supplied services have empty object vectors and COM references;
the four mixed built Counter arrays are nonempty.

The only I/O substitution implements IStream Read/Write. Original code chooses
every length and payload. Successful allocation/free use the inherited fixture
allocator. Original Swizzle `6CF180`, `6CF2C0`, `6CF240`, `6CF230/6CF350` execute.
There is one pending HouseType pointer and two mappings (House plus the supplied
retained external HouseType identity); conversion drains both vectors. The actor
owner link is rebound to the restored House as an explicit composition input.

| Native owner | Statistics field |
| --- | --- |
| House `53E4`, twenty dwords | UnitsKilled per victim house |
| House `5434` | UnitsLost |
| House `5438`, twenty dwords | BuildingsKilled per victim house |
| House `5488` | BuildingsLost |
| House `54E8` | Combined native score accumulator |
| House `55A0/55B4/55C8/55DC` | Four built Counter owners, totals at owner `+10` |

The score consumer `5C996D..5C99EB` sums the two kill tables, two losses and four
built totals. That mapping is instruction reading, not a new execution of the
score dialog. Constructor scalar/Counter block `4F5A2A..4F5B39` and kill-table
block `4F629A..4F62B8` execute with declared caller registers. This does not claim
the entire normal House constructor executes.

The packet establishes persistence of native state. It does not establish a new
equivalence proof for Rust's existing aggregate kill/built representation or its
split of the native score into harvested credits and kill points. Those owners
should retain their existing values across save/load instead of resetting them.

## Local display name through House storage

The additional `name_cases` use the same full House stream and swizzle fixture.
Original ordinary scenario/local-player writer `0x688092..0x6880AD` receives the
supplied UTF-16 string in EBX and destination House in EBP. Its bounded copy takes
21 code units, then writes a terminator at `House+0x16052`: the 42-byte field at
`+0x1602A` carries at most 20 displayed code units on this route. ASCII,
non-ASCII and overlong controls record the original written bytes directly.

Full Save `0x504080`/Abstract Save `0x410320` and Load `0x503040`/Abstract Load
`0x410380` preserve those bytes. The no-init constructor `0x4F5190` also retains
the inline name. All three RNG states stay unchanged. The fixture does not
execute every House INI/name writer or claim complete scenario-load coverage.
