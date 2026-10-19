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

An existing building provider or ordinary trigger grant still supplies the
retained House/Super instance; merely registering a section does not grant it.
The example uses an existing stock dispatch kind and does not require the
separate UnitDelivery or GenericWarhead contributions.

## Origin and contract

This contribution adapts the author's earlier ra2-mono work and is submitted
under this repository's GPLv3, without a separate license notice. Its sources
are `GameLogicManager.Cmd.SuperweaponResolvers.cs::TryDeductSuperweaponCost`,
the target/effect-preflight ordering in `GameLogicManager.Cmd.Superweapon.cs`,
`SuperWeaponTypeIniLoader.cs`, and the paid-superweapon/refusal scenarios in
`GameLogicManagerTriggerSuperweaponTests.cs` and
`SuperweaponTargetPreflightTests.cs`.

The existing C# cost helper chiefly supplies the negative-cost/funds gate.
Positive one-shot grants, checked minimum-integer handling, the VERA wallet
adapter and client projection are completed here, not claimed as already
complete in that source helper. This is not imported Ares C++ code.

The tag contract is documented by [Ares's Cost page](https://ares-developers.github.io/Ares-docs/new/superweapons/cost.html):
`Money.Amount` defaults to zero, a negative value is a cost, and a positive
value grants credits. A paid power exposes its cost and is darkened while
the owner cannot afford it. An attempted unaffordable launch is refused.

## Ownership and behavior

- The ordinary ordered INI scalar readers read `Money.Amount` and
  `EVA.InsufficientFunds`, including omitted-layer retention and explicit zero
  overrides. Their existing saved processed-rules record already retains the
  source values; there is no separate extension-rule save record. Numeric
  parsing follows VERA's existing integer reader, not a claim of Ares behavior
  for malformed or overflowing text.
- A single House wallet owns the cash, spending statistic and score. Reads
  use the same available-money predicate as launch settlement. Negative
  amounts use the existing `Spend_Money` adapter, positive ones use
  `Add_Credits`; the latter deliberately retains its wrapping i32 semantics
  and does not add spending or score. No independent balance is introduced.
- Affordability is checked before paired-Super writes, then checked again at
  settlement after readiness and busy-storm/dominator admission. Off-map targeted paid
  targets and the unimplemented charge-drain dispatch refuse without payment.
  The signed-minimum cost represents 2147483648 credits and is unaffordable
  by any signed House wallet; it cannot wrap into a reward or panic.
- Settlement happens once immediately before an accepted `Launch` call.
  Replaying a command against its consumed charge or one-time grant does not
  settle again. A payload's later failure does not refund an otherwise
  accepted launch: this retains the host's called-Launch transaction boundary,
  not a promise that every dispatch kind always produces an object/effect.
- AI skips an unaffordable power before target search/RNG, and its eventual
  command still checks the current wallet. If funds disappear after selection
  or during input delay, the command refuses without consuming its charge.
- `SuperWeaponView.can_afford` is a derived read, independent of readiness.
  Paid cameo overlays refresh from that read between membership rebuilds;
  a cash change does not grant/revoke/reorder a power or clear its charge.
  Local input rechecks the same view before selecting a target or requesting
  an immediate launch. `Action=None` retains the ordinary direct-launch event
  with cell (0, 0), which is not mistaken for an invalid targeted click.
- An owner-scoped refusal fact is presented only on that owner's client.
  The default cue is `EVA_InsufficientFunds`; `EVA.InsufficientFunds` can
  replace it and `none` silences it. These facts are transient presentation,
  not a second authoritative gameplay state or saved event queue.
- Paid tooltips use the existing localized CSF money format, including an
  i64 display conversion for the minimum integer. Zero-cost and reward
  powers retain their existing name-only tooltip.

## Two-stage powers, persistence and limits

Put a Chronosphere's one-shot fee on its `PreClick` source definition; the
normal zero-cost `PostClick` warp keeps its existing native behavior and does
not charge that source fee again. The source fee is paid at accepted source
selection, not postponed until the destination or refunded on cancellation.

An explicitly nonzero `Money.Amount` on a `PostClick` definition is refused
before changing the pair. The current host's post-click arm intentionally
bypasses normal readiness and has no authoritative one-use financial token;
allowing a reward there would allow repeated grants. Supporting that custom
configuration requires a separate saved selection/settlement lifecycle and is
not silently approximated by an app-local flag here.

The existing wallet, Super flags and processed rule source own persistence.
There is no new serialized Simulation/instance/processed-type field, so this
contribution does not change the snapshot version or positional record layout.
Nonzero resolved money settings participate in configuration hashing; zero
settings add no new hash input. This does not erase ordinary source-INI hash
differences when an author adds an explicit key.

Not included: `Money.DrainAmount`/`Money.DrainDelay`, charge-drain activation,
the source helper's legacy `SW.Money` alias/test-range bypass, general Ares
messages, animations, automatic granting, country restrictions or other
superweapon dispatch kinds. No retail files are included or modified.

The synthetic Rust tests exercise actual rules processing, command dispatch,
wallet primitives, busy/readiness refusals, AI, client adapters and binary state
round trips. They are not a full workspace suite, live game/device acceptance,
retail loader verification or an Ares executable/RNG parity certification.
