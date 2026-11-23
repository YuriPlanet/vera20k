# Optional Phobos prone movement speed

Build with `--features phobos` to replace the two hardcoded prone speed
multipliers in `InfantryClass::GetMovementSpeed` with configurable ones.

```ini
[General]                     ; rulesmd.ini
ProneSpeed.Crawls=            ; double, default 0.67
ProneSpeed.NoCrawls=          ; double, default 1.5

[INFANTRYTYPE]                ; an [InfantryTypes] entry
ProneSpeed=                   ; double, default: the [General] value Crawls= selects
```

The global keys pick the multiplier by the type's `Crawls=` art flag; the
InfantryType key, when present, overrides both.

## Native site

`InfantryClass::GetMovementSpeed @ 0x00521D80` reads the prone byte
(`InfantryClass+0x6DB`, `0x00521D8A`) and then branches on the type's `Crawls=`
byte (`Type+0xEBD`, `0x00521D9A`):

| Branch | Retail arithmetic | Native range |
| --- | --- | --- |
| `Crawls=yes` | `speed - speed / 3` | `0x00521DA4..0x00521DBB` |
| `Crawls=no` | `speed + speed / 2` | `0x00521DBC..0x00521DC3` |

Phobos's `InfantryClass_CurrentSpeed_ProneSpeed` hook (`0x00521D94`, six bytes,
`src/Ext/Techno/Hooks.cpp`) replaces the type read with

```cpp
pTypeExt->ProneSpeed.Get(pType->Crawls ? RulesExt::Global()->ProneSpeed_Crawls
                                       : RulesExt::Global()->ProneSpeed_NoCrawls)
```

and returns `static_cast<int>(static_cast<double>(currentSpeed) * multiplier)`
through the shared epilogue at `0x00521DC5`. The product is a plain IEEE double
truncated toward zero. The default 1.5 matches the retail `speed + speed / 2` at
every speed, but 0.67 is **not** `speed - speed / 3`: at ten leptons a crawling
man moves 6 here and 7 there, and at one lepton 0 instead of 1.

## Where VERA20K owns it

- `rules/ruleset.rs` — `GeneralRules::prone_speed_crawls` /
  `prone_speed_no_crawls`, and `RuleSet::prone_speed_for`, which resolves the
  type value first and then the flag-selected global.
- `rules/object_type.rs` — `ObjectType::prone_speed`, read only for
  `[InfantryTypes]` sections, as `InfantryTypeExt` is.
- `sim/infantry.rs` — `apply_prone_speed` takes the resolved multiplier;
  `None` keeps the retail integer arms byte for byte.
- `sim/movement/walk_step.rs` — applies it where the foot step is paid, and
  `sim/movement/movement_tick.rs` resolves it once per prone infantryman from
  the same `is_prone_for_damage` gate the crawls byte uses.

## Residuals

- The multiplier is applied where VERA20K pays a foot step, not inside a single
  native `GetMovementSpeed` port; every consumer that asks for a prone
  infantryman's step speed sees the configured value, but a caller that reads
  the type's speed without walking does not.
- Non-positive speeds return unchanged, keeping the existing retail-arm guard;
  the hook itself would multiply them.
- Speeds are whole leptons, as the native `int` current speed is.
