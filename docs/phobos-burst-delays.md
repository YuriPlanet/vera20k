# Per-shot burst delays

Phobos is in every build; turn it on from a game's data to give each shot of a weapon's burst its own
rearm delay. Stock gamemd has only the Unit's four `BurstDelay0..3=` entries
and a fixed `RandomRanged(3, 5)` for every other class; this key puts the
delays on the weapon itself and applies to every class.

```ini
[SomeWeapon]                    ; rulesmd.ini
Burst.Delays=                   ; comma-separated integers, default empty
```

`Burst.Delays=10,20,30` arms the burst's second shot with a 10-frame rearm,
the third with 20 and the fourth with 30; any later shot reuses the last
entry. Each value is the delay that shot arms, not a cumulative offset, so it
lands in the same `RearmTimer` as a `BurstDelay{i}=` entry. A `-1` entry falls
back to the stock path for that index: the Unit's `BurstDelay{i}=` table when
it applies, otherwise `RandomRanged(3, 5)`. An empty list therefore leaves
every index but 0 stock. Index 0 always answers 0 - even a weapon without the
key arms a zero rearm for the burst's first shot instead of drawing 3..=5, so
the installed hook changes the stock cadence of any multi-shot weapon.

Values read once per rules pass (`ValueableVector<int>`): an absent key keeps
the constructor's empty list, a later pass that authors the key replaces the
whole list, a token that does not convert is skipped, and a present value with
no usable token replaces with empty. A negative index only matches the
last-entry arm, because native casts the index to `unsigned` before the length
compare.

## Native site

`WeaponTypeExt::GetBurstDelay` (`src/Ext/WeaponType/Body.cpp:62-74`) answers
0 for index 0, the `index - 1` entry of a longer list, the last entry of a
shorter non-empty one, and -1 otherwise. Phobos hooks `TechnoClass::GetROF @
0x006FCFA0` at `0x6FD05E` (`src/Ext/Techno/Hooks.Firing.cpp:1149-1166`),
replacing the 7 bytes of the mid-burst arm (`CurrentBurstIndex < Burst`,
signed, `0x6FD05A`). A non-negative answer returns it at the shared return
`0x6FD099`, bypassing the stock index 0 / index 5+ guard and the unit
`BurstDelay{i}` table; -1 restores the stock instruction, `0x6FD067` for
index 1..=4 when it applies and `0x6FD084` (`RandomRanged(3, 5)`) otherwise.
The hook is class-independent.

## Where VERA20K owns it

- `rules/weapon_type.rs` - the `Burst.Delays=` read through
  `IniSection::read_ares_int_list`, and `WeaponType::phobos_burst_delay`, the
  `GetBurstDelay` port returning `Some` only for the hook's `>= 0` gate.
- `sim/combat/rof.rs` - `get_rof` consults the weapon at the top of the
  mid-burst arm, before the Unit `BurstDelay` table and the 3..=5 draw. Every
  GetROF caller (FireAt's rearm, the C4 plant, the gunner hand-over,
  `UnitClass::PerCellProcess`) sees the same answer.

## Tests
`rules::weapon_type::tests::burst_delays_reads_a_sparse_int_list` reads the key
through the production rules layers: absent, parsed, invalid-token and
tokenless values, plus a later pass replacing the list.
`sim::combat::rof::tests::burst_delays_answer_each_burst_index_before_the_stock_arm`
drives `get_rof`: each entry answers without a draw, an index beyond the list
uses its last entry, 0 is accepted, and a -1 entry falls back to the unit table
or to the 3..=5 draw. The
stock-kernel corpus `get_rof_matches_the_original` runs without this feature
(a Phobos build correctly diverges at burst index 0).

## Residuals

- The two `Burst.FireWithinSequence` hooks (`src/Ext/Unit/Hooks.Firing.cpp:72`,
  `src/Ext/Infantry/Hooks.Firing.cpp:101`) also call `GetBurstDelay`, for the
  cumulative firing-sequence delay; they are ported on the
  [burst sequence](phobos-burst-sequence.md) page. The unit half there still
  waits for the `UnitTypeExt::FireUp` art key.
