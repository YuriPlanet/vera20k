# Superweapon Money.Amount

Requires an explicit `--features ares` build. Default builds do not read or execute
these extensions; see [build capability](ares.md).

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
VERA implements that contract for single-stage powers, including
GenericWarhead and UnitDelivery, and ticketed two-stage powers described below.
It retains the existing House wallet as the
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
Two-stage Super instances additionally retain a selection sequence and optional
one-use ticket; no balance is duplicated. Snapshot schema 451 rejects older
positional records rather than guessing missing fields. Nonzero
resolved money settings participate in configuration hashing; default zero does
not add a new hash input. Source INI identity is still hashed normally.
Presentation refusal events are transient and outside saved/hash state.

## Two-stage powers and other limits

Money-bearing two-stage configurations require a `PreClick=yes` source and a
`PostClick=yes` destination with a valid `PreDependent`. This retains the existing
native mapping: `PreDependent`'s type/kind index selects an entry in
`[SuperWeaponTypes]`. If several destinations match, registry order selects the
first matching pair with money on either definition. Malformed half-pairs and
charge-drain configurations remain refused; clearing a Chronosphere/Warp's
phase flags does not turn its nonzero money setting into a supported single
stage. Both source and destination clicks must be on the map.

Ares's [Chronosphere flags](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Misc/SWTypes/ChronoSphere.cpp#L14)
exclude money at source selection, and its
[launch owner](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Ext/SWType/Body.cpp#L469)
uses remembered source settings at final activation. Accordingly:

- A source click checks current affordability but neither pays nor rewards.
  It records the source/destination IDs and a monotonically increasing sequence
  on the existing Super. Native source readiness may return next House update;
  reselecting only replaces the ticket, never settles money.
  A well-formed explicit source request consumes its sequence ID even when
  admission later refuses it; only this replay watermark changes, not money,
  charge or pair state. A later retry cannot reuse a refused request's ID.
- Final activation requires that exact owner's current ticket, rechecks source
  availability and the current wallet, and settles the **source's** amount once.
  A destination amount is not an additional fee or reward: a zero-amount source
  remains free even if its destination has a nonzero setting.
- An unaffordable or invalid-target final click preserves the selection and
  source/pair state. There is no partial payment; the owner-local funds cue uses
  source settings. Successful final activation consumes the ticket, retires the
  source animation and starts source recharge. A one-time source grant is only
  consumed at this final boundary, not at source selection or cancellation.
- Right-click, changing pointer modes/reselecting, another accepted power,
  reset, provider loss, suspension and defeat retire the pending selection
  without settlement. A stale cancel cannot cancel a newer sequence. Ordinary
  provider checks retain the existing one-time/nonlosable trigger exemptions.
- Begin/complete/cancel are explicit appended VERA command variants, not
  alterations to native SPECIAL_PLACE bytes. Even cancel-before-source-execution
  is ordered through the existing saved command queue. A forged, stale, replayed,
  wrong-owner or unticketed paid destination cannot activate or settle it.
- Save/load retains pending and consumed/cancelled sequence history, source cell,
  one-time flags and queued commands. Loading the same earlier save creates an
  earlier timeline as usual; it is not a cross-save anti-cheat ledger.
  Restore rejects tickets on the wrong source/owner, missing destinations,
  out-of-history sequence IDs or multiple pending selections for one House.
- Scripted paired launches use the same finalizer synchronously. Automatic
  single-click AI does not invent a destination for paired powers.

The stock zero-cost pair keeps its original readiness, selection, command shape
and hash stream. The added nondefault state is hashed only for a paid workflow.
This closes the mechanism described in [#2107](https://github.com/YuriPlanet/vera20k/issues/2107),
subject to the evidence boundary below.

Also excluded: Money.DrainAmount/Delay, charge-drain activation, the reported
donor's legacy SW.Money alias/test-range bypass, general Ares messages and
animations, country restrictions, and automatic granting.

Evidence levels: public Ares source establishes the tag/control-flow references;
focused Rust regressions exercise VERA's production readers, wallet, commands,
AI, client adapters and persistence. There is no Ares executable comparison,
full-engine parity claim or live audio-device acceptance claim.
