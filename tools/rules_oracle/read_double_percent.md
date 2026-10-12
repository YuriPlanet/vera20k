# ReadDouble percent product, the prone damage head and the C4 fuse

`read_double_percent.py` executes original gamemd.exe SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Its JSON results are native outputs, not calculations copied from Rust.
The `.meta.json` file pins the payload and lists the supplied boundaries.

## Native behavior established

`CCINIClass::ReadDouble` (`0x005283D0`) scans the value with `sscanf "%f"`
into a float and widens it exactly. If the value holds a `%` anywhere, it
multiplies by the double 0.01 at `0x007E3808`
(`fld qword; fmul qword; fstp qword`, `0x0052857A..0x00528584`).

The game runs that multiply under control word 0x0E7F: 53-bit precision,
chop. WinMain selects chop at `0x006BBFC1` and caches the word at
`0x006BBFC9`. `Math__ftol` (`0x007C5F00`) loads the cached word before its
`fistp` and never restores the old one. The startup captures in
`tube_startup_capture.json` and `bounce_startup_capture.json` read 0x0E7F at
every point after the PE entry.

So the product is chopped. For 11 of the 34 distinct `%` tokens in retail
`rulesmd.ini`, the result is one ulp below the nearest double: 5, 10, 20,
28, 35, 40, 45, 65, 70, 80 and 90%. The reader rows also run under
round-to-nearest (0x027F) as a control; those give the nearest double, so
only the rounding mode makes the difference. Every row checks that:
- one value was scanned;
- the percent arm ran exactly when the string holds `%`;
- no control-word load ran inside the call;
- the control word is unchanged afterwards.

`WarheadTypeClass::ReadINI` stores `ProneDamage=` as a double at `+0xF8`
(`0x0075D999`, `0x0075D9A4`). The constructor stores 1.0 there.

`InfantryClass::ReceiveDamage` (`0x00517FA0`, vtable `0x007EB058` slot
`+0x16C`) changes the damage first when all three hold:
- the infantryman is prone (`+0x6DB`);
- the raw damage is positive;
- defenses are not ignored.

It then computes `ftol(fild damage * fmul qword [warhead+0xF8])`
(`0x00517FD9..0x00517FE3`) and keeps EAX, the low dword of ftol's qword. A
result below 1 becomes 1. After that come the InfDeath 9 test (`0x00517FF9`)
and FootClass::ReceiveDamage (`0x00518042`).

With the chopped multiplier, a `70%` warhead deals one less whenever the
damage is a multiple of 10, and an `80%` warhead whenever it is a multiple of
5. For example, a deployed GI's Para (SSA, 25) hits a prone target for 19,
not 20.

A product beyond 32 bits keeps its low dword. `600%` on 1,000,000,000 is
1,705,032,704, and on 357,913,942 it wraps negative, so the clamp makes it 1.
An infinite product converts to the indefinite qword, whose low dword is 0,
so it also clamps to 1.

Neither ReadDouble nor the prone head draws RNG, writes a timer or detaches
anything (instruction reading).

`RulesClass` reads `[CombatDamage] C4Delay=` through ReadDouble into the
double at `Rules+0x1750` (`0x0066CDA2`); its constructor stores 0.03 there
(`0x006675F0`). A C4 plant (`InfantryClass::Per_Cell_Process`) flashes the
building for `ftol(C4Delay * 900.0 * 0.5)` (`0x0051A5B6..0x0051A5C8`) and
starts its C4Timer for `ftol(C4Delay * 900.0)` (`0x0051A5E2..0x0051A608`).
Under the chop word, retail `.03` (0.029999999329447746) gives a fuse of 26
and a flash of 13. The constructor's 0.03 gives the same, because its chopped
product is 26.999999999999996. Rounding to nearest would give 27 for both.

## Coverage

**Reader rows:**
- the 34 retail tokens and 20 other strings: fractional, signed, spaced,
  doubled `%`, trailing junk, exponent, out-of-range and plain values;
- integer percents 0..1000.

**Prone rows:**
- the nine retail values (30, 50, 70, 80, 100, 150, 300, 350 and 600%) and
  six modded ones (1%, 0%, 35%, 90%, plain 0.7 and -50%), each over damage
  1..200 plus 250, 500, 1000, 11000, 65535 and 1,000,000;
- nine out-of-range products, one of them infinite (`1e39%`);
- five gate controls.

**C4 rows:** the absent key, retail `.03` and `0.03`, and modded `.02`,
`.06`, `0.1`, `.5`, `1`, `2.5`, `0` and `-.03`.

A failed scan leaves stale stack bits in native, which this oracle does not
cover. `IniSection::read_double` documents VERA's choice for that case.

## Production owner and comparison

- `rules::ini_value::parse_read_double` owns the percent product. It chops
  with `MaskedX87Chop53`, so an infinite `1e39%` stays infinite. The Verses
  reader already chopped its product and now shares the same constant
  (`ini_value::PERCENT_SCALE`). `rules/read_double_percent_tests.rs` replays
  every reader row, and replays the retail tokens through
  `IniSection::read_double` as well.
- `sim::combat::receiver_call::infantry_prone_raw_damage` owns the prone head.
  `sim/combat/prone_damage_tests.rs` replays every prone row through
  `WarheadType::from_ini_section`. With the retail `rulesmd.ini`, it also
  checks that each of the 58 allocated warheads that set ProneDamage reads
  its native double (nothing allocates `SANoBuilding`, and `RPG`'s only
  weapon, `[RPGTower]`, is referenced by nothing). That
  check found `[KTSTLEXP]`, the elite Kirov bomb's warhead, read as all
  defaults: the name is also an Animation, and the Rules projection let the
  AnimType's empty body replace the warhead's.
- `RuleSet::c4_delay_frames` and `RuleSet::c4_flash_frames` own the fuse and
  flash, on `MaskedX87Chop53`. `rules/read_double_percent_tests.rs` replays
  every C4 row through `RuleSet::from_ini`.

## Not covered

- No live capture at `0x0052857E` during a scenario load. A foreign DLL that
  changes the control word on the main thread between two ftol calls would
  change the reads; a hardware breakpoint there would close this gap.
- Nothing after FootClass::ReceiveDamage is executed. That includes Verses,
  distance falloff and the Techno receiver.
- The C4 rows execute only `0x0051A5A7..0x0051A5CD` and
  `0x0051A5D6..0x0051A60B`: not the Flash call between them, the arm's gates
  before them, or the planter's rearm and Scatter after them.

## Reproduce

Set `VERA20K_GAMEMD_EXE` or `RA2_DIR`, use Unicorn 2.1.4 and run from the
repository root:

```sh
PYTHONPATH=. python -m tools.rules_oracle.read_double_percent --check
VERA20K_REQUIRE_RETAIL_INI=1 cargo test -p vera20k --lib -- rules::read_double_percent_tests:: sim::combat::prone_damage_tests::
```
