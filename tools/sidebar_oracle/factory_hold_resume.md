# Paid E2 user hold, rate refresh and same-head resume

`factory_loss.py --hold-resume` adds three original-executable controls to the
existing paid Russian/ReadyBuildingFixture owner. It reuses physical inputs,
the VM/allocator, SelectAction, command ring, local event receiver, native call
trace and RNG observers. It preserves the default, sale-queue and sale-sidebar
profiles. The corpus is consumed by shared Factory/House production regressions.

```sh
VERA20K_GAMEMD_EXE=/path/to/original/gamemd.exe \
VERA20K_FACTORY_SIDEBAR_ASSETS=/path/to/physical-sale-sidebar-cache \
python -B -m tools.sidebar_oracle.factory_loss --hold-resume --check
```

`--write` generates `factory_hold_resume.json` and its sidecar. The sidecar pins
the command and physical inputs, without executable or repository-source
hashes.
Use the physical cache described by [sale-sidebar](factory_sale_sidebar.md).
Its original layout/tab setup is reused even though these controls do not Draw.
The input service and admitted House/map priors are bounded; this is not a full
scenario load or Rules::Process/VFS winner comparison.

`affordable` and `poor_wallet` start a real paid E2 with another E2 queued.
Whole Select6AAD00 Right on Infantry kind16/tab2 emits SUSPEND15. Existing local
receive6474C7..6474BF executes HouseSuspend4FA910 and FactorySuspend1 at4C9E60.
Later original FactoryAI visits and House rate rewrite4CA6E0 retain both raw
bytes+70/+71. Whole Left emits PRODUCE14; the receiver enters HouseBegin4FA350,
keeps the same held Type/identity/FIFO and calls Resume0 at4C9EA0. It records a
same-frame visit and the first later due visit. The rate rewrite uses the
inherited House power99/0 input; power-dirty production remains its own owner.

The poor-wallet control supplies an explicit spending request through original
SpendMoney4F9790 before Left, exhausting the current wallet. No pause, wallet,
timer, stage, event or affordability result is assigned by the fixture. Native
Resume can return false after clearing+70 and rearming the timer; HouseBegin
continues linking independently of that bool. The next due AI may then stall.
This declared spending request does not model another purchase's lifecycle.

`completed_same_type` reaches stage54 through original FactoryAI, then directly
requests HouseBegin(force0) for that same type. It distinguishes House admission
from Resume's complete-product refusal. It is not a completed portrait Left
click, whose separate route emits PLACE. Preparation constructors/charging and
the loop reaching completion precede the measured request boundary.

Each phase records frame and the existing cadence state: stage/rate, meaningful
timer start/duration, balance, cash-stall+5C, suspended+70, latch+71, credits/spent,
changed+5D, held type/identity and FIFO. Timer-write observations omit opaque+30.
Calls record original callers, raw argument words and raw EAX receipts; only
Suspend/Resume low-byte results are interpreted as bools. Their argument slots
may carry unrelated high bytes, so `argument_low_byte` is authoritative.
House naval/force byte inputs are separately projected. Void-function EAX and
the local runner's stop address are not semantic return values.
Actual Time_To_Build6F47A0 entry/return is observed for the held product during
rate refresh and resume. Its `return_i32` supplies the true rate/resume kernel
input; the fixture never reconstructs it from the resulting rate. The
`held_object` field binds that call's ECX to the existing held identity.

Complete Main/Scenario/MapGen buffers and existing request/raw-advance hooks are
compared for each measured phase, along with native identity cursor, constructor,
detach and destructor entries. Gameplay code bytes remain unchanged. The
inherited observed EVA752700 RET4 sink records the request but does not establish
EVA queue/sample/device behavior or whole-click device RNG. Rendered HOLD,
native saves, Suspend0/system holds, Resume1/forced held-start, special+68
products and GoOnline/Offline scheduling are outside this corpus. Raw+71 is
observed here; it is not equivalent to a semantic user-hold flag.
