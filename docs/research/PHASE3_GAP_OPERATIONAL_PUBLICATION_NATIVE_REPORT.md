# Phase 3: Building gap operational publication

The original updates a GAGAP's deposited gap at its Building turn when its
operational state changes. Rust at baseline `417dfe4d` instead reclassifies all
generators after the object pass and again during House reconciliation. These
orders can produce different shroud knowledge at the next 120-frame sweep.
The implementation moves publication to that Building owner and retains admission
across House reconciliation and restore. Bounded production validation and fresh
independent review passed; this is not row50 closure.

## Evidence and active caller

The original executable is `gamemd.exe`, SHA256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
[The retained comparison](../../tools/spatial_oracle/gap_admission.py) executes
18 complete `4555D0` predicate cases with original Mission `5B3040` and House
power-ratio `4FCE30` callees, and two ordered shroud sequences using the original
leaves and complete periodic sweep. [Outputs](../../tools/spatial_oracle/gap_admission.json)
and [provenance](../../tools/spatial_oracle/gap_admission.meta.json) are retained.
No instructions, return values, or callees are replaced.

Fresh retail `rulesmd.ini` SHA256
`3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef`
declares GAGAP `GapGenerator=yes`, `Powered=true`, `Power=-100`, radius10,
SuperGapRadius10, Strength600, Sight5, Capturable=false and AIBuildThis=yes.
This binds ordinary power-dependent incidence; optional predicate cases do not
establish retail producers for every raw field.

Building constructor `43B9FA` installs vtable `7E3EBC`. Slot+5C at `7E3F18`
is `43FB20`, reached by the live Logic object call at `55B610`. Its initial
operational result is compared with retained byte+6C8 at `43FB59/5F`.
Only a changed result calls `4549B0` at `43FBEA`, then stores the result at
`43FBEF`. Unchanged visits emit no gap write.

`4549B0` rechecks operational+350. Type+CD1 and inactive+269 call gap-add+414
at `454A59`; rejection and active+269 call gap-remove+418 at `454BA9`.
The other direct caller, `6E0B60` via `6E0C7A`, processes an ownership action;
the older report's generic power-plant destruction interpretation is unsupported.
House `4F8440` calls aggregate power assessment `508C30`; its Building power
notification `454CE0` is an empty return. This does not publish a gap removal
before the next Building visit.

## Operational predicate

`4555D0` reads Type at Building+520 and House at+21C. It rejects when:

- byte+660 is clear and signed+67C is below2;
- signed+504 is positive, or health+6C is exactly zero;
- Type+1573 is set, signed Type+EE4 is positive, House power ratio is below1,
  and signed+67C is below2;
- the Type+1574 House timer/+577B gate is not satisfied;
- Type+1552 is set without Building+6CC;
- effective Mission is Construction12 or Selling13.

The actual Mission virtual+184 is `5B3040`: current+AC, falling back to queued+B4
only when current is -1. A queued Selling mission does not reject an active
Guard current mission. `4FCE30` reads signed output+53A4/drain+53A8: output at
least drain or zero drain returns1; otherwise it returns the original ratio.
The comparison supplies normal stock power inputs and executes this callee.

## Observable ordering witness

The original sequence `reveal, gap, leave, gap, reveal, remove, frame120`
reaches one ordinary sight receipt, one hostile gap, counter-1, closed knowledge,
and no pending conceal. All state comes from actual original writes.
The source is parked so no intervening moving high-flight timer event refreshes
it. A due release/admit is admitted at frame239. Then:

| Event order before frame240 | Final counter/gap | Original IsShrouded AL |
|---|---|---|
| Source release/admit, then gap removal | -1 / 0 | 1 |
| Gap removal, then source release/admit | -1 / 0 | 0 |

The native witness composes supplied object order, not a full Logic/power-system
execution. The production regression damages the generator owner's power
plant before House assessment at frame238. The baseline House collector removes
the gap then; the original retains it until the Building turn at239. Source-first
registration consequently chooses different rows. Both registration orders and
snapshot continuation are exercised by the regression, including the actual
high-flight refresh producer and the CPU shroud-fill consumer.

## Retained lifecycle and integration

Techno constructor `6F2B40` initializes admission+269 and cached radius+26C to
zero; Building constructor initializes previous operational sample+6C8 to zero
and HasPower+660 to one. Add `6FB170` checks admission, rechecks `4555D0`, caches
the signed Type radius when needed, and sets admission before the cell loop.
Remove `6FB470` clears admission before its loop, retains radius and does not
check current power. Admission exists even for a friendly viewer with no hostile
cell receipts. The Rust entity therefore retains a shared operational sample and
per-viewer admission/radius, independently of those cell receipts.

Building save `454190` reaches AbstractSave `410320`; virtual size leaf
`459E70` returns `0x720`. Load `453E20` reaches `410380` for the same body.
Restore constructors `43B680 → 6F4300 → 65A7E0 → 5F3B50 → 4101C0` neither
reset these fields nor replay the gap. Snapshot schema144 preserves and hashes
the retained Rust state. House and cache reconciliation only project it.

Techno Limbo `6F6AC0` releases ordinary sight at `6F6B16`, removes the gap at
`6F6B6A`, then reaches Object Limbo. Ownership `448260` instead removes the
gap before releasing sight and changing owner; ordinary new-owner reveal
`701875` precedes the new gap admission. These existing Rust lifecycle
chokepoints now publish in that order. SpySat's selected viewer bracket uses
live gap candidates and rechecks current operational state, including candidates
with no old receipt; other viewers and the Building operational sample remain
unchanged. Repeated House materialization is not an admission event.

Complete add/remove leaves also clear local House+240 (`6FB43F` / `6FB71C`),
including friendly admissions with no hostile receipt. This is distinct from
House+577A's SpySat latch. The selected event owner invalidates the mapping
latch; successful post-bulk re-admissions clear it again after `577D90` sets it.
Rejected re-admissions leave the new mapping latch set. The production regression
uses the launch shroud option, a hostile gap, power loss and a new SpySat to
exercise this existing consumer, plus friendly re-admission.

Fresh Unlimbo is not an unconditional gap add: the discovery path reaches
`445F80 → 446AA3` behind its caller gates. Construction `449A50` invokes that
path before queuing Guard, so the operational recheck still rejects it. Current
Rust placement represents construction with `BuildingUp` without necessarily
publishing Mission12; the selected gate respects that existing owner until the
build completes, then the next Building visit admits the gap.

The ordinary death path is synchronous: raw Building+4EC resolves `4415F0`,
and ReceiveDamage's postlude `44266B..4426A7` calls UnInit when its ordinary
non-Selling, `Explodes=no` timer has positive remaining time. Stock GAGAP uses
that path. No eight-frame delayed-death behavior was introduced. Selling or
`Explodes=yes` deferred destruction remains outside this comparison.

## Delivery boundary and limits

The existing shroud counter and per-viewer gap receipt owner remain applicable;
see [the current-sight report](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_SHROUD_CURRENT_SIGHT_NATIVE_REPORT.md).
The selected delivery covers operational edges, ownership and Limbo ordering,
SpySat rechecks, and persistence. House/cache refresh must not substitute a new
power classification for a Building event.

General EMP/NeedsEngineer/PoweredSpecial/+67C producers, mobile gap geometry,
campaign/discovery gates and optional fog records remain separate unproved
domains. The stock static geometry comparison does not prove current-coordinate
removal for moving/warped gaps. Friendly fog's existing boolean projection does
not establish native Cell+13C counter equivalence. No entire Building policy or
power system equivalence is claimed.

## Validation receipts

- Original-byte comparison: `python -m tools.spatial_oracle.gap_admission --check`
  passed all18 operational predicate cases and2 ordered shroud sequences, exit0.
  A fresh independent critic repeated it on2026-09-11 with the same result.
  The named Guard fixture uses original raw5; optional input cases do not prove
  their production writers or retail reachability.
- Focused delivery checks passed7/7; affected GSI checks17/17, vision69/69,
  and owner-change4/4. The rendering boundary separately consumes the same
  simulation/native-order fixture and checks the final shroud fill.
- After integrating main `ed8f4837`, the full `cargo test -p vera20k --lib`
  passed8650 tests, zero failures,120 ignored, exit0 (27.36s test execution).
  Receipt: `.local/gap-admission-full-v2.log`.
- The first full run caught a test layering violation and three replay hash
  differences. The consumer assertion now lives in the rendering layer. Passive
  projection retains the previous empty per-viewer receipt containers; all
  original current and historical hash/RNG assertions pass without rebaselining.
- Fresh independent reviews passed native semantics, production integration,
  main/MCV overlap and the corrective delta.
- `cargo clippy -p vera20k --lib` passed, exit0 (1m17s,1144 warnings).
  Receipt: `.local/gap-admission-clippy-v1.log`.

Snapshot144 deliberately rejects older positional layouts, including main's
MCV143 payload. The MCV fields, hash contribution and deployment implementation
remain intact. This delivery does not close row50 or any other Phase3 row.
