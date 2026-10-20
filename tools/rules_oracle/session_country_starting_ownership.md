# Starting extra-unit ownership and shared budget

Replay the dedicated mode of the existing physical Rules VM:

```powershell
$env:VERA20K_GAMEMD_EXE = 'C:/your-retail-install/gamemd.exe'
python -m tools.rules_oracle.session_country --starting-ownership --check
```

The [corpus](session_country_starting_ownership.json) and
[sidecar](session_country_starting_ownership.meta.json) retain synthetic inputs,
native outputs, selected binary identity and execution bounds. The default
`session_country` mode and its references are unchanged. No mod assets are needed.

## Native decisions

The ordinary Battle constructor writes vtable `0x7EE184` at `0x5C0DF8`.
Its slots `+0xC8` and `+0xCC`, at `0x7EE24C` and `0x7EE250`, contain
`0x5D7030` (starting MCV) and `0x5D70F0` (extra starting units). Scenario start
`0x686905..0x68693C` reaches `0x5D6D80` when its supplied mode pointer is nonnull
and the preceding byte gate is clear; it first calls the mode's `+0x84` method.
These are original caller/instruction/data observations, not a whole native
session-start execution.

`0x5D6D80` calculates one budget from the union of actual non-passive Countries:

- `0x5D6DAF..0x5D6DD1`: House registry pointer -> House `+0x34` Country;
  Country `+0x1A6` skips passive entries; Country `+0xB4` selects the Owner bit.
- `0x5D6DF5..0x5D6E66`: UnitType registry order; AllowedToStart `+0x6D5`,
  Owner `+0x6CC` intersection, TechLevel `+0x634 <= [0x822CF4]`, and exclusion
  of pointers in Rules BaseUnit `+0xB24/+0xB30`.
- `0x5D6E73..0x5D6EB6`: InfantryType registry order and the same first three
  gates, without BaseUnit exclusion.
- `0x5D6EB8..0x5D6ECF`: native count fallback, half-count rounding, signed
  division and multiplication by `[0xA8B270]` (requested starting-unit count).

The per-House loop repeats the Country-passive check at
`0x5D6EED..0x5D6EF8`, jumping to the next House before either callback.
Observer node `+0x6B == -1` also skips both at `0x5D6F4A..0x5D6F53`.
MCV callback `+0xC8` is invoked at `0x5D6F69`; after success, extra-unit
callback `+0xCC` is invoked at `0x5D6F83`. Observer status does not enter the
earlier budget mask collector.

`0x5D70F0..0x5D72AB` builds the extra-unit candidate lists for one House.
It reads the actual Country `+0xB4` at `0x5D7156`, forms its bit at
`0x5D7161`, and tests Owner at `0x5D7186` (Unit) and `0x5D7246` (Infantry).
The other gates match the budget scans, except that TechLevel comes from that
House `+0x1D4`. The original whole body has no ParentCountry lookup or
RequiredHouses/ForbiddenHouses (`+0xDA0/+0xDA4`) read. Country constructor
`0x5115ED..0x51163A` obtains `+0xB4` from actual registry position.

The MCV path differs: `0x5D7059` calls `FirstOwnable @ 0x505310`, whose
`0x505316..0x50531C` resolves ParentCountry through `0x5117D0`. This evidence
does not authorize changing MCV ownership to the extra-unit identity.

## Executed controls and limits

The fixture uses original physical INI loading, Rules/Country constructors and
live Country readers. Each Country deliberately names the other as its parent.
Original type-array initializers `0x4E7460/0x4E7560` and full Unit/Infantry
constructors enroll native registry order. Original retained reader blocks
produce Owner, both qualifiers, TechLevel, Cost and AllowedToStart. The original
ordinary Cost getter `0x711EB0` executes in eligible budget controls. No parser
output, cost calculation, original code or ownership decision is supplied.

The controls distinguish exact ID, different-case ID, different-case Name alias,
unknown Owner, opposite ParentCountry, deliberately failing Required/Forbidden
qualifiers, BaseUnit exclusion, participant union and Country-passive removal.
Candidate lists come from actual native vector contents. Five native budget
results are preserved; the passive cases follow a real additional physical
Country reader pass using the saved `passive_layer` string.
An additional American HouseTechLevel=0 control, with global TechLevel still10,
executes the same candidate prefix and produces two empty lists. It distinguishes
the per-House candidate threshold from the global budget threshold.

House storage, selected registry membership, House/global TechLevel, requested
unit count and BaseUnit pointer list are declared continuation inputs. Full
House/Scenario construction, Rules Process, native mode selection and whole
world startup are excluded. Candidate execution stops before the spawn loop,
then resumes the original vector cleanup tail. Budget execution stops before
the per-House callback loop. Three complete RNG states are compared unchanged;
the corpus retains their hashes and the empty raw/ranged draw-entry record.
The executed admission and arithmetic paths have no simulation timer write or
detach call.

The unchanged downstream mechanism remains outside this comparison: each spawn
attempt calls Scenario ranged RNG at `0x5D735F` or `0x5D737E`; failed placement
can call the object's deleting virtual method at `0x5D73D7`; successful placement
subtracts cost at `0x5D73EF..0x5D73F3`, then sets initial veterancy and mission.
The outer routine additionally draws Scenario `0..65535` at `0x5D6FFE` after
all Houses, including the zero-budget path. Placement, mission timers, object
destruction/detach and those downstream RNG effects are not certified here.
Special `<random>`/`<none>` Owner-token behavior belongs to the shared mask
reader and is not covered by this ordinary-token corpus.
