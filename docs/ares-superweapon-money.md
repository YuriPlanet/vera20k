# Superweapon Money.Amount

```ini
[SuperWeaponTypes]
0=PaidScan

[PaidScan]
Type=PsychicReveal
Action=PsychicReveal
IsPowered=no
RechargeTime=1
Money.Amount=-500
; EVA.InsufficientFunds=EVA_InsufficientFunds
```

An existing provider or trigger grant still supplies the House/Super instance.
Registering a section alone does not grant it.

## Origin and supported behavior

Original contribution by **baixiong443**, submitted under VERA's GPLv3. The
contributor identifies earlier ra2-mono `TryDeductSuperweaponCost`, command
preflight, loader and test code as its source. That donor repository/revision
was unavailable for independent comparison, so those provenance and behavioral
claims remain unverified. This is not imported Ares C++ code.

The independently checked [Ares cost contract](https://ares-developers.github.io/Ares-docs/new/superweapons/cost.html)
defines zero as free, negative amounts as fees, and positive amounts as rewards.
VERA implements that contract for **single-stage powers**, including
GenericWarhead and UnitDelivery. It retains the existing House wallet as the
sole owner of balances, spending and score:

- Negative amounts use the existing Spend_Money owner. The full fee must be
  available; no partial payment is accepted. The minimum signed i32 amount is
  safely unaffordable by a signed wallet, never negated into an overflow.
- Positive amounts use Add_Credits and its existing wrapping i32 semantics.
  Rewards do not increment spending or score.
- Readiness, supported configuration, required buildings and target/payload
  admission precede settlement. AI checks affordability before target search,
  and the final command rechecks the current wallet after any input delay.
- Settlement occurs once immediately before the admitted Launch call. A later
  effect failure does not refund it. For UnitDelivery, acceptance means enqueue:
  later provider loss, blocked landing or save/reload cannot repeat settlement.
- The owner-local refusal cue defaults to the fixed EVA catalog's
  `EVA_InsufficientFunds`, can be replaced by a registered cue, and can be
  silenced. Unknown later names retain the previously resolved cue. The
  processed INI history supplies the same result after restoration.

The source readers preserve omitted/empty/unparseable layer values and explicit
zero overrides. Ares integer syntax is read through the shared INI value owner;
malformed-input overflow and Ares executable arithmetic equivalence are not
claimed.

## Client behavior and state

`SuperWeaponView.can_afford` derives from the same wallet predicate independently
of native readiness, power and charge. Paid cameo availability refreshes between
membership rebuilds without granting, revoking or reordering a power. Local
input rechecks affordability before target selection or direct launch;
right-click cancellation remains available. `Action=None` keeps the ordinary
no-target (0, 0) event convention.

Paid tooltips use VERA's localized money formatter with an i64 display conversion
for the minimum integer. Free/reward tooltips retain their name-only behavior.
VERA's line wrapping and darkening of an unready, unaffordable cameo differ from
Ares presentation; no pixel-level equivalence is claimed.

The existing wallet, Super flags and processed rule source own persistence.
There are no new serialized Simulation/instance/processed-type fields. Nonzero
resolved money settings participate in configuration hashing; default zero does
not add a new hash input. Source INI identity is still hashed normally.
Presentation refusal events are transient and outside saved/hash state.

## Two-stage powers and other limits

Money-bearing `PreClick`, `PostClick`, Chronosphere and ChronoWarp configurations
are rejected before payment, source/pair writes, effects or charge consumption.
A free destination also cannot bypass a money-bearing predecessor. The ordinary
zero-money source/destination path is unchanged.

Ares's [Chronosphere flags](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Misc/SWTypes/ChronoSphere.cpp#L14)
exclude money at source selection, and its
[launch owner](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Ext/SWType/Body.cpp#L469)
uses remembered source settings at final activation. VERA's existing native
source readiness can return on the next House update; it is not a one-use
financial token. Paid two-stage support requires a saved, authoritative selection
transaction and is tracked in [#2107](https://github.com/YuriPlanet/vera20k/issues/2107);
it is not approximated by paying on repeatable source clicks.

Also excluded: Money.DrainAmount/Delay, charge-drain activation, the reported
donor's legacy SW.Money alias/test-range bypass, general Ares messages and
animations, country restrictions, and automatic granting.

Evidence levels: public Ares source establishes the tag/control-flow references;
focused Rust regressions exercise VERA's production readers, wallet, commands,
AI, client adapters and persistence. There is no Ares executable comparison,
full-engine parity claim or live audio-device acceptance claim.
