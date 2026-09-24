---
title: LocomotionClass as an engine substrate service — design study
date: 2026-07-29
scope: gamemd.exe LocomotionClass / ILocomotion / IPiggyback, and the VERA20k Rust replacement boundary
kind: design study (docs/plans) — analysis only, no Rust landed in this session
supersedes: nothing. Corrects [docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md) (§11 below)
updated: 2026-07-29 — open-question closeout pass, then an OQ4/OQ5 re-run (see Changelog); §11b holds self-corrections
---

# LocomotionClass as an engine substrate service

## Status legend — used throughout, never upgraded by prose

| Tag | Meaning |
|---|---|
| **[V]** VERIFIED | Re-derived from the binary this study, with the tool call named inline. For a *parity* claim, additionally requires a gamemd-derived executable check or exhaustive proof over the input space — where a parity claim lacks one it is written **[UNCHECKED]** even if the binary fact behind it is **[V]**. |
| **[I]** INFERRED | Reasoned from verified evidence but not itself proved. |
| **[U]** UNCHECKED | Not established this study. Includes UNCHECKED-REACHABILITY (body verified, caller not) and UNCHECKED-EXHAUSTIVENESS (sample, not census). |
| **[R]** REFUTED | A lane report claimed it; an adversarial pass produced counter-evidence. The counter-evidence wins. |

Three independent adversarial refutation passes ran against the eight original lane
reports; a 2026-07-29 closeout wave added four more lanes and two further adversarial
passes, and a same-day OQ4/OQ5 re-run added two more lanes and one more adversarial pass
(Changelog). **Where a lane report and an adversarial verdict disagree, the
adversarial verdict is what this document carries.** Every such override is listed in §11;
places where *this document itself* was wrong are in **§11b**.

Authority order inside this document, for any residual conflict:
`factory-clsid.md` (CLSID/factory/census) > `motion-slots.md` (vtable matrix) >
the adversarial passes > the remaining lanes.

---

## 1. What LocomotionClass is FOR in a running YR skirmish

**Verdict first: the base `LocomotionClass` is not a behavior provider. It is a
lifetime-and-identity shell with four live jobs and one small set of inherited default
bodies.** Almost every slot a reader would expect to carry movement logic is, in the
base, a compile-time constant that no live locomotor ever executes — all eleven concrete
classes override it. The parts of the base that actually run in a stock match are narrow,
and three of them are *mechanisms* (not values) that a Rust port must model explicitly.

The five verified active responsibilities, in mechanism terms:

### 1.1 It is the object whose reference count *is* the piggyback teardown trigger **[V]**

`AddRef` `0x0055A950` / `Release` `0x0055A970` maintain an `InterlockedIncrement` /
`InterlockedDecrement` counter at object `+0x14`, initialised to **0** by the constructor
`0x0055A6C0` (`disassemble_function 0x0055A6C0`; `decompile_function 0x0055A950`,
`0x0055A970`). At zero, `Release` invokes primary-vtable slot 8 with flag 1 — the MSVC
scalar deleting destructor `0x005172F0` — which runs the destructor `0x0055A6F0` and frees
the block (`batch_decompile 0x005172F0`; destructor bytes re-read via
`read_memory 0x0055A6E3 len 48` → 13 NOPs then `c701 c0ae7e00 / c74104 f4ad7e00 / c7410c
00000000 / c3`).

This is not bookkeeping. In `FootClass::AI` the call at `0x004DAEB9` that ends a piggyback
**is** `Release`, and the object must stay alive across the following `End_Piggyback` call
purely because a second reference (taken by the `QueryInterface` at `0x004DAE78`) is still
outstanding. The refcount reaching zero is a sim-visible state transition: the unit stops
teleporting and resumes driving.

The *atomicity* is not load-bearing **[V]** — the adjacent process-global counter
`DAT_00abcd3c` is incremented and decremented with plain non-atomic arithmetic on the same
lines (`decompile_function 0x0055A950`/`0x0055A970`), so the object is only ever touched
from one thread.

### 1.2 It holds the host back-link, and 8 of 11 classes use the base implementation unchanged **[V]**

`Link_To_Object`, ILocomotion slot 3, base `0x0055A710` (`disassemble_function 0x0055A710`):

```
0055a710  MOV ECX,[ESP+0x4]   ; this = object+0x04 (no adjustor thunk on slot 3)
0055a714  MOV EAX,[ESP+0x8]   ; the object being linked
0055a718  MOV [ECX+0x8],EAX   ; object+0x0C = owner
0055a71b  MOV [ECX+0x4],EAX   ; object+0x08 = owner
0055a71e  XOR EAX,EAX
0055a720  RET 0x8             ; S_OK, unconditionally
```

It stores the same pointer twice, does **not** AddRef the owner (so the graph is acyclic
by construction), and cannot fail. Only Hover `0x00513CB0`, Fly `0x004CCA20` and Jumpjet
`0x0054AD30` override it, each calling the base first (`read_memory 0x007EACFC len 16`,
`read_memory 0x007E89F4 len 16`, `read_memory 0x007ECD68 len 16`;
`get_xrefs_to 0x0055A710` → 9 DATA refs + 3 CALL refs).

**Linking is a separate step from construction.** Every creation site calls
`CoCreateInstance` and *then* `Link_To_Object` (`disassemble_bytes 0x00742680 len 110`;
`0x0065F0C2`; `0x0044E060`). A Rust design that establishes the link implicitly at
construction loses a distinction the piggyback protocol depends on — the new locomotor is
fully wired to the host *before* it takes custody of the old one.

### 1.3 It owns the powered flag — but the observable effect is Hover-only **[V] fact, [V] narrowing**

Object `+0x10`, byte, constructor default **1** (`disassemble_function 0x0055A6C0`
`MOV byte ptr [EAX + 0x10],DL` with `DL=1`). Three accessors, all base, nobody overrides
`Power_On` (`disassemble_function 0x0055A8F0` / `0x0055A910` / `0x0055A930`;
`get_xrefs_to 0x0055A8F0` → 12 DATA refs, i.e. all 12 vtables on the base).

Only **Fly** (`0x004CFD20`/`0x004CFD90`/`0x004CFDA0`) and **Hover**
(`0x00516BF0`/`0x00516C70`/`0x00516CA0`) override `Power_Off` / `Is_Powered` /
`Is_Ion_Sensitive` (`read_memory 0x007E8A4C len 16`, `read_memory 0x007EAD54 len 16`).

The adversarial TS-legacy pass narrowed this hard, and the narrowing is what ships:

- **[R]** `power-ion.md` listed *EMP* as the live consumer and put it first in its "Write:"
  list. Refuted: `get_xrefs_to 0x004c52b0` (the `EMPulseClass` constructor) returns **no
  references**, and `search_instructions CALL "0x004c52b0"` returns **0 hits** over
  1,152,158 instructions. `EMPulseClass::Apply` `0x004C54E0` is called only from that dead
  constructor. `ini/rulesmd.ini` carries `[EMPuls]` annotated `;gs disabled in code` and
  `[EMPulseWeapon]` has zero consumers. **EMP does not power a locomotor down in stock YR.**
- **[R]** `power-ion.md`'s "Fly `Power_Off` consumes two RNG draws — port them or desync".
  Refuted: with EMP dead, the surviving `Power_Off` receivers are
  `UnitClass::PerCellProcess` `0x0073A52B` and `TechnoClass::OnDeployBegin` `0x0070FD0B`,
  both re-verified as `[reg+0x674]` (`get_assembly_context`). Aircraft are `AircraftClass`
  and do not deploy, so Fly's override is unreachable. **Porting its two draws would add RNG
  consumption gamemd never performs** — the exact lockstep hazard, inverted.
- **[V]** Hover's overrides *are* reachable: hover units are `UnitClass`, and
  `HoverLocomotionClass::Move` `0x00514ADE` and `__SpeedUpdate` `0x00515F99` read slot 24.
- **[V]** Drive and Ship never consult it: scoped `search_instructions CALL "+0x60]"` inside
  `DriveLocomotionClass::Process` → 0 hits / 485 instructions; Ship `Process` → 0 / 452.
  Walk is **[U]** (the scope hit only 12 instructions).

Live `Power_On` edges, all with the receiver verified as `[reg+0x674]` via
`get_assembly_context`: `BuildingClass::UndockUnit` `0x004593E4`,
`BuildingClass::ReleaseDockedHarvester` `0x00459709`,
`TechnoClass::OnUndeployComplete` `0x0070FC15`, `FootClass::Unlimbo` `0x004D721E`,
`AircraftClass::Set_Destination` `0x0041AD83`, and — the player-facing one —
`TechnoClass::Set_Destination` `0x0074314C`: **ordering a powered-off unit to move
re-powers its locomotor** (`get_assembly_context 0x00742f76`).

### 1.4 It is the runtime type identity that gameplay branches on **[V]**

`IPersist::GetClassID` is **primary-vtable slot 3 (+0x0C)**, not an ILocomotion slot.
`UnitClass::Mission_Harvest` fetches it and 16-byte-compares against the Teleport CLSID
(`disassemble_bytes 0x0073E7F0 len 64` — `CALL dword ptr [EDX + 0xc]`,
`MOV EDI,0x7e9a90`, `CMPSD.REPE`). The same shape appears in `UnitClass::Scatter`
(`0x00743AC3`) and `TechnoClass::Set_Destination` (`0x007424E9`, `0x00741F28`).

The load-bearing subtlety is that the *effective* class is queried through
`IPiggyback::Piggybacker_CLSID` (Drive `0x004AF610`, `disassemble_function 0x004AF610`),
which returns the **stashed** locomotor's class when piggybacking and its own otherwise.
A Chrono Miner with a Drive bolted on for the factory exit still reports **Teleport** to
the harvest loop. Mission logic never sees the temporary locomotor.

### 1.5 It supplies exactly eight default bodies that a live locomotor actually executes **[V]**

Of the 40 slots, the base body is the *installed implementation* for at least one
ACTIVE-YR class in only these cases (full matrix in §2.4, byte-accurate across all 440
cells per the adversarial re-decode of all 12 vtables):

| Slot | Base body | Live classes inheriting it | What it is |
|---|---|---|---|
| **7** Can_Enter_Cell | `0x0055ABF0` `XOR EAX,EAX; RET 8` | **all 8 live** | constant 0 — no live locomotor has a real one |
| **30** Force_Immediate_Destination | `0x0055AC00` `RET 0x10` | 7 of 8 (Walk overrides) | no-op |
| **20** Unlimbo | `0x0055AC20` `RET 4` | 6 of 8 (Drive, Ship override) | no-op |
| **28** Force_Track | `0x0055AC10` `RET 0x14` | 6 of 8 (Drive, Ship override) | no-op |
| **31** Force_New_Slope | `0x0055ACE0` `RET 8` | 6 of 8 (Drive, Ship override) | no-op |
| **6** Head_To_Coord | `0x0055ACA0` | Fly, Teleport, Rocket | **real logic** — returns the linked object's own coordinate |
| **39** Mark_All_Occupation_Bits | `0x004B6620` `RET 8` | Fly, Rocket | no-op |
| **32** Is_Moving_Now | `0x004B6610` | Teleport | **virtual forward** to the object's *own* slot 4 |
| **33** Apparent_Speed | `0x0055AD10` | 10 of 11 (Fly overrides) | tail-call to linked object's `vt[+0x538]` |

(bodies from `read_memory 0x0055AB70 len 464`, `read_memory 0x004B6600 len 96`;
slot 32 decode `disassemble_bytes 0x004B6610 len 48`; slot 6 decode
`disassemble_bytes 0x0055ACA0 len 48`; independently cross-checked by
`get_xrefs_to 0x0055AC10` (10 refs, Drive+Ship absent) and `get_xrefs_to 0x0055AC00`
(10 refs, Walk+Mech absent), which match rows 28 and 30 exactly.)

Two of these are behaviorally interesting rather than trivial:

- **Slot 6** is the identity answer "the coordinate I am heading to is the one I am
  standing on" — correct for a locomotor with no independent target, and it is what Fly,
  Teleport and Rocket ship with. **[V]**
- **Slot 32** dispatches through `[this]`, so Teleport (which inherits 32 but overrides 4)
  gets **Teleport's** `Is_Moving` — a one-byte state test `CMP byte [EAX+0x30],1`
  (`read_memory 0x00718080 len 48`) — **not** the base `false`. A Rust port that
  constant-folds base slot 32 to `false` breaks Teleport. **[V]**

**Never reached by any of the 11** (all override): slots 4, 5, 16, 17, 18, 29. Do not port
their base bodies. Slot 29's base `0x004C9150` is additionally a program-wide shared
`XOR EAX,EAX; RET` stub with **no stack cleanup** in a vtable where every other slot is
`RET 4`/`RET 8` (`read_memory 0x004C9150 len 16` → `33 c0 c3`) — calling it through this
vtable would leak 4 bytes per call. It is unreachable and must not be documented as
"default layer 0".

### 1.6 It is the serialization shell, and the load side is stock OLE structured storage **[V]**

Primary vtable at `0x007EAEC0` is **IPersistStream, 10 slots**
(`read_memory 0x007EAEC0 len 64` → `0x0055A9B0, 0x0055A950, 0x0055A970, 0x004C9150,
0x004B4C30, 0x004C9150, 0x0055AA60, 0x0055AB40, 0x005172F0, 0x004C9150`). `Save`
`0x0055AA60` writes a 4-byte header containing the object's own `this` pointer, then
`Size_Of()` raw bytes, then clears the dirty flag at `+0x11`; `GetSizeMax` `0x0055AB40`
returns `size + 4`, and both call the same class virtual at primary slot 9
(`decompile_function 0x0055AA60`, `0x0055AB40`). `IsDirty` `0x004B4C30` reads `+0x11` with
the correct IPersistStream HRESULT polarity (`read_memory 0x004B4C30 len 32`).

**The load side is not a hole — this study's earlier reading of it was wrong.** The host
does clear `FootClass+0x674` before the base load runs (`FootClass::Load` `0x004DB3C0`,
`004db3df: MOV [ESI+0x674],EBX`), but the *same function* refills it 0x189 bytes later:

```
004db550  LEA EDI,[ESI + 0x674]     ; ppvObj = &this->locomotor
004db560  PUSH 0x7ed358             ; riid = IID_ILocomotion
004db565  PUSH EBP                  ; IStream*
004db568  CALL dword ptr [0x007e15f8]   ; OleLoadFromStream
```

(`disassemble_function 0x004DB3C0`; `get_xrefs_from 0x004db568` →
`PTR_OleLoadFromStream_007e15f8`, `EXTERNAL:0000012f to function OleLoadFromStream`;
`read_memory 0x007ed358 len 16` = `{070F3290-9841-11D1-B709-00A024DDAFD1}` = the
`IID_ILocomotion` this document's own §2.2 table already carried.) Derived twice
independently — once by the OQ1 lane, once by an adversarial pass that reached the identical
mechanism before reading the lane. Full contract in §5.7; the prior claim is corrected in
§11 C14.

---

## 2. Full inventory

### 2.1 Object layout **[V]**

Two vptrs, multiple inheritance. Sub-object at `+0x00` carries **IPersistStream**;
sub-object at `+0x04` carries **ILocomotion**. Methods reached through the ILocomotion
vtable receive `this = object+0x04`, so a body writing `[this+N]` touches object offset
`N+4`. This is the single largest source of wrong offsets in this area.

| Off | Size | Meaning | Proof |
|---|---|---|---|
| +0x00 | ptr | IPersistStream vptr (`0x007EAEC0` for base) | ctor `MOV [EAX],0x7eaec0` |
| +0x04 | ptr | ILocomotion vptr (`0x007EADF4` for base) | ctor `MOV [EAX+4],0x7eadf4` |
| +0x08 | ptr | owner back-pointer, copy A | `Link_To_Object`; destructor leaves it stale |
| +0x0C | ptr | owner back-pointer, copy B | `Link_To_Object`; destructor nulls it |
| +0x10 | u8 | **powered**, default 1 | `Power_On`/`Power_Off`/`Is_Powered` bodies |
| +0x11 | u8 | IPersistStream dirty, default 1 | `Save` clears; `IsDirty` reads with correct polarity |
| +0x14 | i32 | **COM refcount**, default 0 | `InterlockedIncrement/Decrement(this+0x14)` |
| +0x18 | ptr | **IPiggyback vptr** — only on the 6 classes that implement it | Drive QI returns `this + 6` (`decompile_function 0x004AF720`) |

Base object size **0x18 bytes** (`disassemble_function 0x0055A6C0`).
Why two copies of the owner pointer is **[U]** — no reader of `+0x08` was found by any lane.

### 2.2 The four accepted IIDs **[V]**

`QueryInterface` `0x0055A9B0` (`decompile_function 0x0055A9B0`) tests all four every call
and only then checks `ppv`:

| GUID address | GUID | Interface | `*ppv` |
|---|---|---|---|
| `0x007F7C90` | `{00000000-0000-0000-C000-000000000046}` | IID_IUnknown | **object+0x04** |
| `0x007F7C80` | `{00000109-0000-0000-C000-000000000046}` | IID_IPersistStream | object+0x00 |
| `0x007ED358` | `{070F3290-9841-11D1-B709-00A024DDAFD1}` | IID_ILocomotion | object+0x04 |
| `0x007F7C70` | `{0000010C-0000-0000-C000-000000000046}` | IID_IPersist | object+0x00 |

(`read_memory 0x007F7C70 len 48`, `read_memory 0x007ED358 len 16`.) Failure is
`E_NOINTERFACE 0x80004002`; null `ppv` is `E_POINTER 0x80004003` before anything is touched.
The canonical IUnknown identity is the **ILocomotion** sub-object — unusual but
self-consistent, because slots 0/1/2 of the ILocomotion vtable are `-4` adjustor thunks.

`IID_IPiggyback` = `{92FEA800-A184-11D1-B70A-00A024DDAFD1}`, two copies:
`0x007E9B10` (what providers compare against) and `0x00819088` (what consumers pass)
(`read_memory 0x007E9B10 len 16`, `read_memory 0x00819088 len 16`).

### 2.3 The three adjustor thunks, and the shared stubs **[V]**

```
004d0510  SUB dword ptr [ESP+0x4],0x4 ; JMP 0x0055a9b0   (QueryInterface)
004d0520  SUB dword ptr [ESP+0x4],0x4 ; JMP 0x0055a950   (AddRef)
004d0530  SUB dword ptr [ESP+0x4],0x4 ; JMP 0x0055a970   (Release)
```
(`read_memory 0x004D0510 len 48`, rel32 targets resolved.) Jumpjet replaces all three with
its own identically-shaped thunks `0x0054DFF0/E000/E010` (`read_memory 0x007ECD68 len 16`;
`disassemble_bytes 0x0054DFF0 len 48`) — the only class examined that does. Why is **[U]**.

[ILOCOMOTION_COM_PROTOCOL_SPEC.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md)'s addresses and the thunk addresses are **both correct
and describe different things** — the doc lists the real implementations; the vtable slots
hold the thunks. See §11 C1.

The other non-`0x55Axxx` base slots are **shared stubs, not thunks**
(`disassemble_bytes 0x004B4C30 len 96`, `0x004B6610 len 32`, `0x004C9150 len 16`):
`0x004C9150` (`XOR EAX,EAX; RET` — plain RET), `0x004B4C60` (`XOR EAX,EAX; RET 4`),
`0x004B4C70` (`RET 4`), `0x004B4C80` (`XOR AL,AL; RET 4`), `0x004B6610` (forwarder to
slot 4), `0x004B6620` (`RET 8`).

Their addresses landing in the Drive/DropPod/Fly code ranges is **link order, not
ownership** **[V]**: `get_xrefs_to 0x004B6610` returns exactly three DATA refs
(base+0x80, DropPod+0x80, Teleport+0x80) and Drive's vtable is absent;
`get_xrefs_to 0x004B6620` returns five (base, DropPod, Fly, Rocket, Tunnel).
Identical-COMDAT folding is ruled out — byte-identical bodies exist at distinct addresses
(`C2 08 00` separately at `0x0055AC30`, `0x0055ACE0`, `0x004B6620`).

### 2.4 The base 40-slot vtable and the 40 × 11 override matrix **[V]**

Base ILocomotion vtable `0x007EADF4` (`read_memory 0x007EADF4 len 160`), independently
re-decoded by four separate passes including the adversarial one; all agree byte for byte.

`·` = inherits the base body. An address = that class overrides the slot.
**All 440 cells were re-decoded from scratch by the adversarial pass with zero
discrepancies** (`read_memory <vtable> len 160` on all 12 vtables).

| # | Slot name (see naming caveat below) | BASE | Drive | Ship | Walk | Hover | Fly | Jumpjet | Teleport | Rocket | Mech† | DropPod† | Tunnel† |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | QueryInterface | `004D0510` | `004B4D90` | `006A4300` | `0075CC30` | · | · | `0054DFF0` | `0071A160` | · | · | `004B6740` | · |
| 1 | AddRef | `004D0520` | `004B4DA0` | `006A4310` | `0075CC40` | · | · | `0054E000` | `0071A170` | · | · | `004B6750` | · |
| 2 | Release | `004D0530` | `004B4DB0` | `006A4320` | `0075CC50` | · | · | `0054E010` | `0071A180` | · | · | `004B6760` | · |
| 3 | Link_To_Object | `0055A710` | · | · | · | `00513CB0` | `004CCA20` | `0054AD30` | · | · | · | · | · |
| **4** | **Is_Moving** | `0055ACD0` | `004AFB80` | `0069F290` | `0075AB30` | `00514C30` | `004CCA90` | `0054AE50` | `00718080` | `00661F50` | `005AFF70` | `004B5B30` | `00728A50` |
| **5** | **Destination** | `0055AC70` | `004AFC90` | `0069F3A0` | `0075ABA0` | `00514CB0` | `004CCAE0` | `0054AE60` | `007180A0` | `00661FB0` | `005AFF80` | `004B5B40` | `00728A90` |
| **6** | **Head_To_Coord** | `0055ACA0` | `004AFCC0` | `0069F3D0` | `0075AC00` | `00514D10` | **·** | `0054D9B0` | **·** | **·** | `005AFFE0` | · | · |
| **7** | **Can_Enter_Cell** | `0055ABF0` | **·** | **·** | **·** | **·** | **·** | **·** | **·** | **·** | · | · | `0072A090` |
| 8 | Is_To_Have_Shadow | `0055ABE0` | · | · | · | · | · | · | · | · | · | · | `0072A060` |
| 9 | Draw_Matrix | `0055A730` | `004AFF60` | `0069F670` | · | `00513F40` | `004CF610` | `0054DCC0` | · | `00663470` | · | · | `00729B40` |
| 10 | Shadow_Matrix | `0055A7D0` | `004B0410` | `0069FB20` | · | `005142A0` | `004CFB00` | · | · | · | · | · | · |
| 11 | Shadow_Point | `0055ABD0` | · | · | · | · | `004CF830` | · | · | · | · | · | · |
| 12 | Draw_Point | `0055A8C0` | · | · | · | · | `004CF940` | · | · | · | · | · | · |
| 13 | Visual_Character | `0055ABC0` | · | · | · | · | · | · | · | · | · | · | `007291D0` |
| 14 | Z_Adjust | `0055ABA0` | `004B4870` | `006A3EA0` | · | · | · | · | · | · | · | · | `00729E50` |
| 15 | Z_Gradient | `0055ABB0` | `004B4880` | `006A3EB0` | · | · | · | · | · | · | · | · | `0072A020` |
| **16** | **Process** | `0055AC60` | `004B0500` | `0069FC10` | `0075AC80` | `00514310` | `004CCB40` | `0054AEC0` | `007192F0` | `006622C0` | `005B0060` | `004B5B70` | `00728E30` |
| **17** | **Move_To** | `0055AC50` | `004AFD40` | `0069F450` | `0075ACB0` | `00514D90` | `004CCC80` | `0054B1C0` | `00718100` | `006632E0` | `005B0080` | `004B6040` | `00728AF0` |
| **18** | **Stop_Moving** | `0055AC40` | `004AFE00` | `0069F510` | `0075ADA0` | `00516320` | `004CCFD0` | `0054B4D0` | `00718230` | `006633C0` | `005B0120` | `004B63A0` | `00728C00` |
| **19** | **Do_Turn** | `0055AC30` | `004B0EF0` | `006A05C0` | `0075AE00` | `00516370` | `004CFC10` | `0054B6E0` | `007192C0` | **·** | `005B0170` | · | `0072A0E0` |
| **20** | **Unlimbo** | `0055AC20` | `004B04D0` | `0069FBE0` | **·** | **·** | **·** | **·** | **·** | **·** | · | · | · |
| 21 | Tilt_Pitch_AI | `0055AB90` | · | · | · | · | · | · | · | · | · | · | · |
| 22 | Power_On | `0055A8F0` | · | · | · | · | · | · | · | · | · | · | · |
| 23 | Power_Off | `0055A910` | · | · | · | `00516BF0` | `004CFD20` | · | · | · | · | · | · |
| 24 | Is_Powered | `0055A930` | · | · | · | `00516C70` | `004CFD90` | · | · | · | · | · | · |
| 25 | Is_Ion_Sensitive | `0055A940` | · | · | · | `00516CA0` | `004CFDA0` | · | · | · | · | · | · |
| 26 | Push | `0055AB70` | · | · | · | `00516E10` | · | · | · | · | · | · | · |
| 27 | Shove | `0055AB80` | · | · | · | `00516FC0` | · | · | · | · | · | · | · |
| **28** | **Force_Track** | `0055AC10` | `004B0C40` | `006A0310` | **·** | **·** | **·** | **·** | **·** | **·** | · | · | · |
| 29 | In_Which_Layer | `004C9150` | `004B4820` | `006A3E50` | `0075C7E0` | `00517100` | `004CFCF0` | `0054B8D0` | `00719E20` | `00663460` | `005B19D0` | `004B64D0` | `0072A1A0` |
| **30** | **Force_Immediate_Destination** | `0055AC00` | **·** | **·** | `0075AE30` | **·** | **·** | **·** | **·** | **·** | `005B01A0` | · | · |
| **31** | **Force_New_Slope** | `0055ACE0` | `004AFB40` | `0069F250` | **·** | **·** | **·** | **·** | **·** | **·** | · | · | · |
| **32** | **Is_Moving_Now** | `004B6610` | `004AFC20` | `0069F330` | `0075AB40` | `00514C80` | `004CCAC0` | `0054D0D0` | **·** | `00661F90` | `005B19E0` | · | `00728A60` |
| 33 | Apparent_Speed | `0055AD10` | · | · | · | · | `004CFE20` | · | · | · | · | · | · |
| 34 | Drawing_Code | `0055ACF0` | · | · | · | · | · | · | · | · | · | `004B65F0` | · |
| 35 | Can_Fire | `0055AD00` | · | · | · | · | · | · | · | · | · | · | `0072A1C0` |
| 36 | Get_Status | `004B4C60` | · | · | · | · | `004CFE50` | · | · | · | · | · | · |
| 37 | Acquire_Hunter_Seeker_Target | `004B4C70` | · | · | · | · | `004CFE80` | · | · | · | · | · | · |
| 38 | Is_Surfacing | `004B4C80` | · | · | · | · | · | · | · | · | · | · | `0072A1E0` |
| **39** | **Mark_All_Occupation_Bits** | `004B6620` | `004B48D0` | `006A3F00` | `0075CA30` | `005171C0` | **·** | `0054D930` | `0071A090` | **·** | `005B1A50` | · | · |

† Mech / DropPod / Tunnel are **DORMANT-TS** — see §3.

**Naming caveat [U]:** the 40 slot *names* come from
[docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md) and remain **navigation hints**. Names
confirmed against a body or a caller this study: 3 (stores an object pointer), 5 vs 6
(constant-null coord vs live object coord — the reverse assignment is nonsensical), 13
(`FootClass::GetVisualState` first-refusal protocol, `decompile_function 0x004DA4E8`), 16,
17, 18 (arg widths from the base stubs, `disassemble_bytes 0x0055AC40 len 80`), 32
(forwards to slot 4), 33 (forwards to a speed getter), 28 (`Force_Track(0x42, Coord3D)`
callsite `0x0044E160`). **All other names are UNVERIFIED**, and two are actively
suspicious: 36 (`Get_Status` vs Ghidra's `Is_On_Floor` — Fly's body returns a 0..3 enum,
which fits `Get_Status` better) and 35 (`Can_Fire` returning constant `false` in a base
class is an odd contract).

### 2.5 Global helpers, singletons, registries, static tables

| Item | Address | Status | Evidence |
|---|---|---|---|
| CLSID GUID array (contiguous, 16-byte stride) | `0x007E9A20`–`0x007E9AD0` | **[V]** | `read_memory 0x007e99c0 len 256`, `read_memory 0x007e9ac0 len 128` |
| `IID_ILocomotion` (the `_com_ptr_t` template arg) | `0x00817BB0` | **[V]** | `read_memory 0x00817bb0 len 32` |
| `IID_IUnknown` (the CoCreateInstance arg) | `0x00817BC0` | **[V]** | same read |
| `IID_IPersist` (the GetClassID QI arg) | `0x00818858` | **[V]** | `read_memory 0x00818858 len 16` |
| `IID_IPiggyback` consumer copy | `0x00819088` | **[V]** | `read_memory 0x00819088 len 16` |
| `IID_IPiggyback` provider copy | `0x007E9B10` | **[V]** | `read_memory 0x007E9B10 len 16` |
| CoCreateInstance wrapper (`_com_ptr_t::CreateInstance`) | `0x0041C250` | **[V]** | `decompile_function 0x0041C250`; 13 call sites via `get_xrefs_to` |
| IPersist QI helper (Ghidra label is **wrong** — see §11 C4) | `0x0045AEA0` | **[V]** | `decompile_function 0x0045AEA0` |
| IPiggyback QI helper (unlabelled in Ghidra) | `0x0045AF20` | **[V]** | `decompile_function 0x0045AF20`; 18 call sites |
| Outstanding-locomotor global counter, non-atomic | `DAT_00abcd3c` | **[V]** fact, **[U]** purpose | `decompile_function 0x0055A950`/`0x0055A970`; no reader located |
| Base `Destination()` source globals — permanently zero | `0x00ABCC78/7C/80` | **[V] exhaustive** | `get_bulk_xrefs` → exactly 1 READ + 1 WRITE each; the sole writer is the static init at `0x0055A680` (`XOR EAX,EAX` then three stores) |
| "Invalid coordinate" sentinel triple used by Drive slots 4/32 | `0x008A0790/94/98` | **[I]** | inferred from both bodies comparing against it; value not read |
| Ion-storm gate — hardcoded `XOR AL,AL; RET` | `0x0053A130` | **[V]** | `disassemble_function 0x0053A130`; 15 call sites, all dead branches (`get_function_xrefs`) |
| Class-factory registration list | `0x00B0BC88` | **[V]** fact, **[V]** never queried | `get_xrefs_to 00b0bc88` → all 40 refs are `in WinMain [DATA]` |
| `CoRegisterClassObject` import slot | `[0x007E15D8]` | **[V]** | `get_xrefs_from 007e15d8` |
| `CoCreateInstance` / `OleRun` import slots | `[0x007E15FC]` / `[0x007E1600]` | **[V]** | `get_xrefs_from` on each |
| Push direction table (8-entry adjacent-cell offsets) | `0x0089F688` | **[I]** | indexing arithmetic read; contents not dumped. **Dead path anyway** |
| The `.data` pointer array at `0x00812004` | — | **[R]** not a locomotor dispatch table | see §11 C7 |

### 2.6 CLSID → class factory → constructor **[V] — this is the authoritative table**

`factory-clsid.md` §4, re-confirmed independently by the adversarial pass against the
WinMain registration block (`disassemble_bytes 0x006BD140 len 200`,
`disassemble_bytes 0x006BD206 len 130`). Method: GUID bytes located by
`search_byte_patterns`; registration site `R` from `get_xrefs_to`; factory ctor is the
`CALL` at `R-0x15`; factory vtable slot 3 is `CreateInstance`, which names both
`operator new(size)` and the locomotor constructor.

| CLSID addr | GUID | Class | WinMain reg. | Factory ctor | Factory vtable | CreateInstance | `new` size | Locomotor ctor |
|---|---|---|---|---|---|---|---|---|
| `0x007E9A30` | `{4A582741-9839-11d1-B709-00A024DDAFD1}` | **Drive** | `0x006BD15B` | `0x006C3F40` | `0x007F3C78` | `0x006C4010` | 0x70 | `0x004AF540` |
| `0x007E9A40` | `{4A582742-…}` | **Hover** | `0x006BD1D5` | `0x006C4240` | **`0x007F3CA8`** | `0x006C4310` | 0x78 | `0x00513C20` |
| `0x007E9A50` | `{4A582743-…}` | **Tunnel** | `0x006BD24F` | `0x006C4540` | `0x007F3CD8` | `0x006C4610` | 0x3c | `0x00728A00` |
| `0x007E9A60` | `{4A582744-…}` | **Walk** | `0x006BD28C` | `0x006C46C0` | `0x007F3CF0` | `0x006C4790` | 0x3c | `0x0075AA90` |
| `0x007E9A70` | `{4A582745-…}` | **DropPod** | `0x006BD2C9` | `0x006C4840` | `0x007F3D08` | `0x006C4910` | 0x30 | `0x004B5AB0` |
| `0x007E9A80` | `{4A582746-…}` | **Fly** | `0x006BD306` | `0x006C49C0` | `0x007F3D20` | `0x006C4A90` | 0x60 | `0x004CC9A0` |
| `0x007E9A90` | `{4A582747-…}` | **Teleport** | `0x006BD343` | `0x006C4B40` | `0x007F3D38` | `0x006C4C10` | 0x4c | `0x00718000` |
| `0x007E9AA0` | `{55D141B8-DB94-11d1-AC98-006008055BB5}` | **Mech** | `0x006BD380` | `0x006C4CC0` | `0x007F3D50` | `0x006C4D90` | 0x34 | `0x005AFEF0` |
| `0x007E9AB0` | `{2BEA74E1-7CCA-11d3-BE14-00104B62A16C}` | **Ship** | `0x006BD3BD` | `0x006C4E40` | `0x007F3D68` | `0x006C4F10` | 0x70 | `0x0069EC50` |
| `0x007E9AC0` | **`{92612C46-F71F-11d1-AC9F-006008055BB5}`** | **Jumpjet** | `0x006BD198` | `0x006C40C0` | `0x007F3C90` | `0x006C4190` | 0x98 | `0x0054AC40` |
| `0x007E9AD0` | `{B7B49766-E576-11d3-9BD9-00104B972FE8}` | **Rocket** | `0x006BD212` | `0x006C43C0` | `0x007F3CC0` | `0x006C4490` | 0x60 | `0x00661EC0` |

Two footnotes that cost other lanes real errors:

- `0x007E9A20` = `{4A582740-…}` has **zero cross-references program-wide**
  (`get_xrefs_to 007e9a20` → "No references found"). Anyone pattern-matching the
  `…40`–`…47` run will expect a 12th locomotor. There isn't one. **[V]**
- **[R]** `com-lifecycle.md` transcribed the Jumpjet GUID with a byte-swapped `Data1`
  (`{61922C46-…}`). Bytes at `0x007E9AC0` are `46 2c 61 92 | 1f f7 | d1 11 | …`
  (`read_memory 0x007E9A20 len 224`); little-endian `Data1` = `92612C46`. That is also
  what appears in `ini/rulesmd.ini`. A GUID copied from that table would never match.

### 2.7 IPiggyback — 8 slots **[V]**

Drive's IPiggyback vtable `0x007E7E8C`, bracketed by MSVC RTTI Complete Object Locators on
both sides at `0x007E7E88` and `0x007E7EAC` (`read_memory 0x007E7E7C len 64`), so the
8-slot extent is proved by layout, not by counting.

| Slot | +off | Drive | Role (verified from body) |
|---|---|---|---|
| 0/1/2 | 0x00/04/08 | `0x004B4DC0`/`DD0`/`DE0` | IUnknown, tail-calling the shared implementations |
| 3 | 0x0C | `0x004AF8E0` | `Begin_Piggyback(ILocomotion* victim)` |
| 4 | 0x10 | `0x004AF930` | `End_Piggyback(ILocomotion** out)` |
| 5 | 0x14 | `0x004AF970` | `Is_Ok_To_End()` |
| 6 | 0x18 | `0x004AF610` | `Piggybacker_CLSID(CLSID* out)` |
| 7 | 0x1C | `0x004B4CD0` | `Is_Piggybacking()` |

(`batch_decompile` of all eight.) Slots 3/4/5/7 are additionally confirmed from callsites.
Teleport's vtable is `0x007F4FDC` (`read_memory 0x007F4FDC len 32`), DropPod's is
`0x007E8254` (`read_memory 0x007E8254 len 32`).

**Six of eleven implement it**, proved two independent ways **[V]**: (A) the constructor
writes a vtable pointer at object `+0x18` — Drive `0x004AF540`, Ship `0x0069EC50`, Walk
`0x0075AA90`, Jumpjet `0x0054AC40`, Teleport `0x00718000`, DropPod `0x004B5AB0` do; Hover,
Fly, Rocket, Mech, Tunnel reuse `+0x18` as ordinary data (a coordinate or a flag); and
(B) `get_xrefs_to 0x007E9B10` returns exactly six `QueryInterface` bodies testing the IID
(`0x004AF744`, `0x004B6494`, `0x0054DC84`, `0x0069EE54`, `0x00719E54`, `0x0075C814`).

Stash offsets from the object base, all six now read out of a method body rather than
guessed from a constructor **[V]**: Drive **+0x68**, Ship **+0x68**, Teleport **+0x48**,
DropPod **+0x2C**, **Walk +0x38**, **Jumpjet +0x94**. Walk and Jumpjet were closed this
pass via three independent steps each — constructor `MOV [ESI+0x18],<vtbl>`, then the
vtable read, then the `Is_Ok_To_End` body: Walk `disassemble_function 0x0075AA90` →
`0x007F69D4`, `read_memory 0x007F69D4 len 32` → slot 5 `0x0075C8E0`, whose body reads
`param_1 + 0x20` (`0x20 + 0x18 = 0x38`); Jumpjet `disassemble_function 0x0054AC40` →
`0x007ECD44`, `read_memory 0x007ECD44 len 32` → slot 5 `0x0054DB00`, whose body reads
`param_1 + 0x7c` (`0x7C + 0x18 = 0x94`, the last field the constructor zeroes at
`0x0054ACB5`). Jumpjet additionally carries a guard byte at **+0x91** (ctor `0x0054ACAB`).
The interface-offset arithmetic behind all four conversions was recomputed independently by
an adversarial pass and is correct.

**Drive's ILocomotion vtable is 50 slots, not 40** **[V]** — it runs from `0x007E7EB0` to
the RTTI Complete Object Locator pointer at `0x007E7F78`, `(0x7E7F78 − 0x7E7EB0)/4 = 50`
(`read_memory 0x007E7EB0 len 160`, `read_memory 0x007E7F40 len 64`). Slots 40–49 are
class-specific virtuals appended after the 40 interface slots in ordinary MSVC layout; two
of them (`0x004B4BE0` at slot 45, `0x004B4BF0` at slot 46) are the setters for Drive's
`Is_Ok_To_End` guard byte (§5.3). This corrects §2.4's implicit "40 slots" framing for the
*inventory*; it changes no Rust decision, because the Rust design has no vtable (§6.1).

- **[R]** `com-lifecycle.md` called `0x004B6470` "a sixth **shared** implementation".
  It sits inside DropPod's own code cluster (`0x004B5B30`–`0x004B65F0`, with DropPod's
  IUnknown at `0x004B6740/50/60`, `read_memory 0x007E8278 len 160`) and nothing shows any
  other class using it. It is **DropPod's QueryInterface**.

### 2.8 Legacy / dormant TS paths carried by this class

Enumerated here; classified with reachability arguments in §3.

`Push` (26) / `Shove` (27); `Is_Ion_Sensitive` (25) and the whole ion-storm gate;
`Acquire_Hunter_Seeker_Target` (37); `Is_Surfacing` (38); `Can_Fire` (35);
`Visual_Character` (13); the Mech, DropPod and Tunnel locomotor classes in full;
the DropPod IPiggyback installer `0x004DB8A0`; the `WW.TiberianSun` ProgID strings
at `0x00840B80`/`0x00840B98`; base slots 4/5/16/17/18/29 (unreachable, all overridden).

### 2.9 Type fields and host vtable slots that gate locomotor behaviour **[V]**

Added by the 2026-07-29 closeout pass. Every row was proved the same way — locate the INI
key string, `get_xrefs_to` it into a `ReadINI` body, read the store instruction with `this`
established from the prologue. **No `param_1[n]` index division is involved in any row**;
these are raw disassembly byte offsets.

| Struct + offset | INI key | Store site | String proof |
|---|---|---|---|
| `TechnoTypeClass + 0x34C` (16 bytes) | `Locomotor=` | `0x007123FA` → GUID reader `0x00527920`, copied to `[EDI..EDI+0xC]` | `search_strings "Locomotor"` → `0x0084444C` |
| `TechnoTypeClass + 0x380` (double) | `Size=` | `0x0071252C FSTP double [EBP+0x380]` | `read_memory 0x00820168 len 32` → `"Size"` at `0x00820178` |
| `TechnoTypeClass + 0x390` (byte) | **`HoverAttack=`** | `0x00712567 MOV [EBP+0x390],AL` | `read_memory 0x008443B0 len 32` → `"HoverAttack\0SizeLimit…"` |
| `TechnoTypeClass + 0x5EC` (byte) | `ResourceGatherer=` | `0x007143E4` | `read_memory 0x00843CB0 len 32` → `"ResourceGatherer"` at `0x00843CB8` |
| `TechnoTypeClass + 0xCD4` (byte) | **`Teleporter=`** | `0x00713FF6 MOV [EBP+0xCD4],AL` | `read_memory 0x00843E58 len 32` → `"Teleporter"` at `0x00843E60` |
| `TechnoTypeClass + 0xD94` (byte) | `JumpJet=` | `0x00715200 MOV [EBP+0xD94],AL` | `read_memory 0x00843630 len 48` → `"JumpJet"` at `0x00843640` |
| `TechnoTypeClass + 0xD97` (byte) | `Organic=` | `0x0071503F` | `read_memory 0x00843704 len 48` → `"Organic"` at `0x00843714` |
| `TechnoTypeClass + 0x6AD` / `+0x6AE` | `DeployToLand=` / `MobileFire=` | `0x007147C0` region | `read_memory 0x00843A74 len 72`. **A genuine offset collision — the instance fields at TechnoClass `+0x6AD`/`+0x6AE` are different state (§5.3)** |
| `WarheadTypeClass + 0x15B` (byte) | `IsLocomotor=` | `0x0075D87C MOV [ESI+0x15B],AL` | `search_strings "IsLocomotor"` → `0x00847D3C`, one DATA ref `0x0075D86B` |
| `WarheadTypeClass + 0x15C` (16 bytes) | `Locomotor=` | `0x0075D88F` → `0x00527920` | second DATA ref of `0x0084444C` |
| `BuildingTypeClass + 0x16B3` (byte) | `DockUnload=` | `0x004609F0` | `read_memory 0x0081AA88 len 32` → `"DockUnload"` at `0x0081AA94`. Stock: `[GAREFN]` + `[NAREFN]` only |
| `SuperWeaponTypeClass + 0xB4` (int) | `Type=` enum | `0x006CEC7E MOV [EBP+0xB4],EAX` | 12-entry name table `0x008425C0`–`0x008425F0` (`read_memory 0x008425C0 len 48`) |

`EBP = this` for every `TechnoTypeClass::ReadINI` row is established at
`0x00712180 MOV EBP,ECX` (`disassemble_bytes 0x00712170 len 32`).

Host (FootClass-family) vtable slots identified this pass, each by vtable-base arithmetic
from the constructor's `MOV [ESI],imm32`:

| Slot | Identity | Proof |
|---|---|---|
| `+0x160` | **`TechnoClass::IsIronCurtainActive`** `0x0041BF40` | `0x007E8C94 + 0x160 = 0x007E8DF4`; `read_memory 0x007E8DF4 len 4` → `0x0041BF40`; `get_function_by_address` names it. The shared gate in **all three** warp/lift chains (ChronoWarp `0x006CC8D5`, `Teleport_To` `0x004DF80A`, Magnetron `0x00469664`) — an iron-curtained unit can be neither warped nor lifted |
| `+0x480` | **`Set_Destination`** | `0x007EB4D8 − 0x007EB058 = 0x480` (Infantry), `0x007F60F0 − 0x007F5C70 = 0x480` (Unit). **Not a position-commit candidate — see §5.6** |
| `+0x4F8` | InfantryClass override `0x00521EB0`, the Jumpjet→Walk landing installer | `0x007EB550 − 0x007EB058 = 0x4F8`; FootClass's is the shared stub `0x0041C080` |
| `+0x508` | `Teleport_To(CoordStruct) -> bool` **[I] name, [V] role** — FootClass `0x004DF7F0`, InfantryClass `0x00522FE0` | `0x007E919C − 0x007E8C94`, `0x007E27AC − 0x007E22A4`, `0x007F6178 − 0x007F5C70`, all `= 0x508`. Dispatched from **two sites program-wide**, both in `FUN_0050D6D0` ← `FUN_006E1A40` ← `TriggerAction__Execute` (`search_instructions CALL "+ 0x508]"`, 1,152,158 instructions, untruncated) — **map-trigger only, zero in skirmish** |

Class-identity constants used by the gates: `What_Am_I()` returns **1** = UnitClass, **2** =
AircraftClass, **6** = BuildingClass, **0xB** = **CellClass**, **0xF** = InfantryClass.
`0xB` was closed by consumer argument, not by a label: the object that satisfies it is
immediately used as `this` for `0x0047EBA0` = `CellClass::FindFirstUnit`, which walks the
cell occupier list at `+0xE4` with next-link `+0x30` (`decompile_function 0x0047eba0`) —
the same walk `TeleportLocomotionClass::PostWarpValidation` performs on a
`Get_CellClass_At_Coord` result (`decompile_function 0x007187A0`). **[V]**

---

## 3. ACTIVE vs INACTIVE — the ship/no-ship table

**Nothing from a DORMANT-TS row ships to Rust.** Rows marked UNCHECKED do not ship either;
they need a reachability answer first.

| Behavior | Class | Evidence | YR reachability argument |
|---|---|---|---|
| COM object layout, two vptrs, adjustor thunks | **ACTIVE-YR** | `FootClass::AI` calls slot 0 and slot 2 *through the thunks* every tick for every foot unit (`disassemble_bytes 0x004DAE50 len 120`) | universal |
| Refcount at +0x14 / Release-at-zero teardown | **ACTIVE-YR** | `0x004DAEB9` Release is how a piggyback ends; the CoCreateInstance wrapper Releases on reassignment | every piggyback end |
| `Link_To_Object` (slot 3) | **ACTIVE-YR** | called immediately after every `CoCreateInstance` — `0x0065F0C2`, `0x0044E060`, `0x007426C9` | every locomotor creation |
| `QueryInterface(IID_IPiggyback)` as a capability test | **ACTIVE-YR** | `FootClass::AI` `0x004DAE78`; E_NOINTERFACE explicitly tolerated at `0x004DAE94`, `0x0065F06F`, `0x007426A2` | **every mobile unit, every tick** |
| `IPersist::GetClassID` type test (primary slot 3) | **ACTIVE-YR** | `UnitClass::Mission_Harvest` `0x0073E815`, `UnitClass::Scatter` `0x00743AC3`, `TechnoClass::Set_Destination` `0x007424E9` | chrono miner harvest loop, continuously |
| `Piggybacker_CLSID` see-through identity | **ACTIVE-YR** | `disassemble_function 0x004AF610` + the harvest comparison above | every chrono-miner harvest tick |
| `Begin_Piggyback` / `End_Piggyback` / `Is_Ok_To_End` / `Is_Piggybacking` | **ACTIVE-YR** | Chronosphere `0x0065F174`; war-factory exit `0x0044E10E`; `TechnoClass::Set_Destination` `0x00742688` | see §5.4 frequency table |
| `Process` (16), `Move_To` (17), `Stop_Moving` (18), `Is_Moving` (4), `Is_Moving_Now` (32) | **ACTIVE-YR** | `FootClass::AI` `0x004DA877`, `0x004DA692/8BB/96D/A24`; `Set_Destination_Internal` `0x004D965D`, `0x004D96B9` | every unit, every tick / every move order |
| Base slot 6 `Head_To_Coord` returning the object's own coord | **ACTIVE-YR** | installed in Fly, Teleport, Rocket (matrix §2.4) | aircraft + chrono + missiles |
| Base slot 32 forwarding to own slot 4 | **ACTIVE-YR** | installed in Teleport; dispatches through `[this]` | every chrono unit |
| Base slot 7 `Can_Enter_Cell` = constant 0 | **ACTIVE-YR, degenerate** | installed in **all 8 live classes**; Tunnel is the only overrider and Tunnel is dormant | the locomotor-side hook is answered identically always |
| Base slots 20/28/30/31/39 no-ops | **ACTIVE-YR, degenerate** | matrix §2.4 | Force_Track/slope/unlimbo work is Drive+Ship only |
| `Power_On` / `Power_Off` / `Is_Powered` | **ACTIVE-YR, narrowed to Hover** | 7 verified `[reg+0x674]` Power_On sites; 2 surviving Power_Off sites; Drive/Ship `Process` never read slot 24 (0 hits in scoped sweeps) | deploy/undeploy/undock/move-order edges fire in ordinary play; the *observable* effect exists only where Hover overrides |
| `Get_Status` (36) | **ACTIVE-YR** | callers now verified: `AircraftClass::Mission_Enter` `0x00419E41/EC7/F26`, `Mission_Move_Carryall` `0x00417040`, all `[ESI+0x674]`, `CMP EAX,1` | any match with aircraft |
| `[LocomotorBeam]` warhead locomotor swap (Magnetron) | **ACTIVE-YR** | the only `IsLocomotor=yes` section in `rulesmd.ini` (line 27299; **zero** hits in `rules.ini`), installing the Jumpjet CLSID; `[TELE]` Magnetron `TechLevel=2`, `Primary=MagneticBeam`, `Warhead=LocomotorBeam` | **[R]** previously called here "the best-sourced live example of **raw** replacement". It is a **piggyback BEGIN** (`0x007102D8`) — §11 C15. It is still the best-sourced live example, just of the wrong category |
| `IPersistStream` Save / GetSizeMax / dirty flag | **ACTIVE-YR, save-load only** | bodies verified; not on any per-tick path | no skirmish-sim effect while playing |
| `OleRun` branch of the creation wrapper | **ACTIVE-YR, no observable effect** | `dwClsContext = 7`, `7 & 0x14 = 4`, so the branch is taken; it is the **only** spawn path (`disassemble_bytes 0x00517b6b`: `PUSH 0x817bc0`, `CALL [0x007e1600]`, `PUSH 0x817bb0`) | runs on every unit spawn; a no-op for in-proc objects |
| `Is_Ion_Sensitive` (25), ion-storm gate, `IonStorms` | **DORMANT-TS** | `0x0053A130` is `XOR AL,AL; RET` (`disassemble_function`); all 15 call sites are dead branches | cannot be revived by data — the gate reads no global |
| `Push` (26) / `Shove` (27) | **DORMANT-TS / unreachable** | base = always-false stubs; Hover's real `Push` `0x00516E10` is called only by Hover's own `Shove` `0x00516FC0`, and `get_xrefs_to 0x00516FC0` returns only its own vtable slot; `JMP [reg+0x6c]` = 1 hit (a thiscall thunk, unrelated), `JMP [reg+0x68]` = 0 hits | **zero occurrences per match** — not rare, unreachable |
| `Acquire_Hunter_Seeker_Target` (37) | **DORMANT-TS** | Fly is the sole overrider; the gate is `HunterSeeker=` at TechnoTypeClass+0xD27, and `rulesmd.ini` comments out `;GDIHunterSeeker=`, `;NodHunterSeeker=`, `;HSBuilding=` — no `[GHUNTER]`/`[NHUNTER]` section exists | no stock type can set the flag |
| `Is_Surfacing` (38) | **DORMANT-TS (live call, constant result)** | Tunnel is the sole overrider and Tunnel is dormant. **[R]** `render-query-slots.md`'s "no ILocomotion dispatch in +0x98" sweep is wrong — `0x00742314` in `TechnoClass::Set_Destination` *is* a genuine dispatch (`MOV EAX,[EBP+0x674]; PUSH; CALL [ECX+0x98]`) | the call runs; the answer is always `false`. Port a constant, not a mechanism |
| `Can_Fire` (35) | **DORMANT-TS (live call, constant result)** | live dispatches at `0x0074131D` and `0x0051CB7E`, base returns 0, **Tunnel is the sole overrider** | constant `false` in stock YR. **[R]** `render-query-slots.md` classified it SIM-FACING as a trait method |
| `Visual_Character` (13) | **DORMANT-TS (live call, constant result)** | `FootClass::GetVisualState` `0x004DA4E8` gives the locomotor first refusal, but only Tunnel overrides `0x0055ABC0` | always falls through to the TechnoClass default in stock YR. **[R]** `render-query-slots.md` called it a MIXED boundary cut |
| **Mech** locomotor (whole class) | **DORMANT-TS** | `{55D141B8-…}` appears in `rulesmd.ini` only inside `;` comments — Westwood's own conversion notes, e.g. `Locomotor={4A582741-…};<-drive   mech->{55D141B8-…}` | units were re-pointed at Drive for RA2 |
| **DropPod** locomotor + its IPiggyback | **DORMANT-TS** | zero occurrences of `{4A582745-…}` in either INI, not even in a comment; its only installer `0x004DB8A0` has **no references of any kind** (`get_xrefs_to 0x004DB8A0`) | unreachable |
| **Tunnel** locomotor (whole class) | **DORMANT-TS** | zero occurrences of `{4A582743-…}` in either INI; `grep -rn "4A582743" ini/` → no matches | subterranean is on AGENTS.md's known-dormant list |
| Carryall raw locomotor swap `AircraftClass::Carryall_Pickup` | **DORMANT in stock skirmish** | the mechanism is real (`disassemble_bytes 0x00416b40`), but the sole `Carryall=yes` unit is `[HIND]`, `TechLevel=-1`, absent from `[AircraftTypes]`. **[R]** `host-contract.md` called it "common in games with carryall play" | zero frequency in stock skirmish. **[R]** the earlier note "the raw-swap idiom still ships — via Magnetron" is wrong: the Magnetron piggybacks (§11 C15). **These two stores are now the only RAW swap left in the binary**, and `[HIND] TechLevel=-1` was not re-verified this pass (§9 item 16e) |
| Tunnel branch of the war-factory exit (`CLSID_Tunnel` compare at `0x0044DEDD`) | **DORMANT-TS** | no `Locomotor=` names Tunnel; Tunnel does not implement IPiggyback, so the branch would fault if reached | dead comparison |
| Locomotor-spawned dust anim gated on `RulesClass+0x94` | **UNCHECKED** | neither `RulesClass+0x94` nor `TypeClass+0xEC` was traced to a `ReadINI` site by any lane | a RulesClass boolean gating a visual is exactly a possibly-off default |
| `SuperClass::Launch` piggyback branch | **ACTIVE-YR** | case 4 `Type=ChronoWarp` at `0x006CC4B2`; gate chain fully traced (§5.4 row 5). Two `+0x674` hits in the whole 1,907-instruction dispatcher, untruncated — **no other stock superweapon touches a locomotor** | ships; 0–7 per match, bursty |
| `0x00710000` piggyback branch (was labelled `TechnoClass::PerformDeploy`) | **ACTIVE-YR** | label drift — it is the `IsLocomotor=` warhead installer, sole caller `WarheadTypeClass::Detonate`; gates fully traced (§5.4 rows 6/6b) | ships; Yuri-only, `[TELE]` `TechLevel=2` |
| Virtual method `0x004DF7F0` / `0x00522FE0` = host slot `+0x508`, creating a Teleport locomotor | **ACTIVE-YR, map-scripted** | dispatched from **exactly two sites program-wide** (untruncated over 1,152,158 instructions), both in `FUN_0050D6D0` ← `FUN_006E1A40` ← `TriggerAction__Execute`. Gate 3 (`+0x27C`) is the function's own re-entrancy latch | **zero in skirmish** — record as a residual, do not ship |
| Infantry Jumpjet→Walk landing, `0x00521EB0` (InfantryClass `+0x4F8`) | **UNRESOLVED-REACHABILITY** | the "dead because `HoverAttack=yes`" argument is refuted — `+0x390` is read nowhere in this function; the replacement dispatcher argument (Walk-only dispatch vs Jumpjet-only gate) was not discharged (§9 item 11) | **do not ship, and do not record as dead** |
| `Tilt_Pitch_AI` (21) | **UNCHECKED-REACHABILITY** | base is a no-op; overrides not enumerated by any lane | the *visible* body tilt on slopes is real, but this slot was not tied to it |
| Rust `sim/movement/tunnel_movement.rs` (Underground layer, burrow, `TunnelSpeed`) | **DORMANT-TS in the Rust tree** | module doc claims "Terror Drone underground"; `[DRON] Locomotor={4A582741…};<-drive` = **Drive**. `grep -rn "4A582743" ini/` → nothing | no stock unit can select it |
| Rust `sim/movement/droppod_movement.rs` + its spine pass | **DORMANT-TS in the Rust tree** | gamemd installer has zero refs; YR paradrop is `[PDPLANE]` + parachute | no stock path reaches it |

---

## 4. Comparison to the current Rust architecture

Survey base: `dev` @ `ce096b3f`. Nothing in this section is a parity claim — it is a
structural census of Rust code with `file:line`.

### 4.1 Headline

**There is no locomotor abstraction.** No trait, no enum dispatch, no `Box<dyn …>`, no
single call site meaning "ask this unit's locomotor to process a tick". Selection is
decentralised across **132 `LocomotorKind::` comparisons in 30 non-test files** and
**twelve independent per-tick movement passes**.

### 4.2 Concept-by-concept

| gamemd concept | Status | Where, and how bad |
|---|---|---|
| `ILocomotion` host pointer (`FootClass+0x674`) | **ABSENT** | No locomotion object exists. The "host" is `GameEntity` passed as split `&mut` field references — `movement_step::advance_lepton_position` takes **12 parameters**, seven of them `&mut` sub-fields (`movement_step.rs:739–752`). Nearest analogue: the hand-rolled read-only `MoverSnapshot` (`movement/mod.rs:148–164`), which exists purely to survive the borrow split. |
| `Link_To_Object` | **ABSENT from locomotion** | Radio links exist as their own mechanism (`sim/radio/contacts.rs:15`), but nothing links a locomotor to an object. Where a link is needed — a moving nav target — Rust **polls**: `movement_tick.rs:936–948` → `navcom::resolve_entity_nav_target_drive_coord` (`navcom.rs:41`) re-resolves the target to a coordinate every tick. |
| `Process` (slot 16) | **PARTIALLY MODELLED — split 12 ways** | The only named artifact is a stub that does nothing: `drive_locomotion.rs:26 process_drive_locomotion_shell` returns `Processed` unconditionally, is `#[cfg(any(test, debug_assertions))]`-gated at re-export (`movement/mod.rs:85–86`), and its call site is `let _ = …` (`movement_tick.rs:962`). Real work is in `movement_tick.rs:866`, `air_movement.rs:191`, `teleport_movement.rs:258`, `tunnel_movement.rs:174`, `rocket_movement.rs:133`, `homing_movement.rs:379`, `droppod_movement.rs:98`, `parachute_descent.rs:81`, `tube_movement.rs:237`, `movement_tick.rs:61`, `movement_tick.rs:1857+`, `turret.rs:128`. |
| `Move_To` (17) / `Set_Destination` | **PARTIALLY — 6 parallel entries** | `issue_move_command_with_layered` (`movement_commands.rs:286`), `issue_direct_move` (`:231`), `set_destination_for_teleporter_entity` (`:129`), `issue_teleport_command` (`teleport_movement.rs:153`), `issue_tunnel_move_command` (`tunnel_movement.rs:81`), `issue_air_move_command` (`air_movement.rs:140`). Drive-specific work is inlined into the *generic* entry (`movement_commands.rs:545–631`) and skipped by a `uses_drive_locomotor` bool. |
| `Stop_Moving` (18) | **PARTIALLY — Foot + Drive only** | `navcom::foot_stop_moving` (`navcom.rs:91`) clears the owner destination pair; `navcom::drive_stop_moving` (`:216`) clamps `current_speed_fraction` to 0.3 — **already labelled VERA-internal with the native equivalent UNCHECKED** (`navcom.rs:160–162`). Hover, Walk, Ship, Jumpjet, Fly, Tunnel, Teleport have none. |
| `Is_Moving` (4) | **PARTIALLY — open-coded** | `entity.movement_target.is_some()`; the piggyback restore uses `movement_target.is_some() \|\| forced_drive_track.is_some()` (`movement/mod.rs:226`). |
| `Is_Moving_Now` (32) | **PARTIALLY — exact evaluator, NO PRODUCER** | `locomotor_ready.rs:37–100` holds six deliberately-distinct per-family truth tables with full-input-space tests (`:148–302`). **Nothing writes it**: `LocomotorState.mission_ready_state` is `None` in every constructor (`locomotor.rs:273`, `:347`) and is otherwise set only by `set_mission_ready_state_for_test` (`:381`). Production therefore runs `DEGRADED_NOT_MOVING` (`mission/authority.rs:201–207`), so **the "is it moving?" mission gate always answers "no" in ordinary play**. Consumers: `readiness.rs:167`, `:227`. |
| `In_Which_Layer` (29) | **PARTIALLY — three disagreeing sources** | `game_entity.rs:833 movement_layer_or_ground()`, `game_entity.rs:846 occupancy_list_layer()` (documented as intentionally different, `:842–845`), and `MovementTarget::layer_at` (`components.rs:458`). |
| `Mark_All_Occupation_Bits` (39) | **PARTIALLY — not owned by movement** | `OccupancyGrid` (`sim/occupancy.rs`) is mutated from **17 production sites across 12 modules**, only 6 in `sim/movement/`. There is no "mark my cells" locomotor call. |
| `Push` / `Shove` (26/27) | **PARTIALLY MODELLED — and it should not be** | `bump_crush::scatter_blocker` (`bump_crush.rs:722`) is live; `scatter::tick_idle_scatter` (`scatter.rs:71`) is **dead**, commented out at `world/mod.rs:2695–2703`. Since the native slots are unreachable (§3), whatever VERA has here is **VERA-internal invention**, not gamemd parity, and must be labelled as such. |
| `Power_On` / `Power_Off` / `Is_Powered` (22/23/24) | **ABSENT** | Grep over `sim/` finds one unrelated hit (`superweapon/mod.rs:218`). The only surfacing is a hard-coded `true` with an honest comment at `movement_tick.rs:1888–1890`; `hover::hover_vertical_tick` (`hover.rs:188`) has the parameter and an unpowered-sink branch that **nothing in production can reach**. |
| `IPiggyback` | **PARTIALLY — two rival mechanisms** | (a) `LocomotorState.piggyback: Option<PiggybackLocomotor>` (`locomotor.rs:134`, type `:568`) stores only `{kind, layer}` and is hard-coded to exactly one pairing — Teleport primary ← Drive active — returning `false` for any other primary (`:437`). (b) `override_state: Option<OverrideLocomotor>` (`locomotor.rs:194`) stores a `Box<LocomotorState>` clone of the whole component. They are not composable and do not know about each other; `end_override` restores 15 fields by hand (`:518–535`) and **silently drops** `piggyback`, `primary_kind`, `subcell_dest`, `hover_throttle`, `hover_bob_offset`, `speed_fraction`, `fly_current_speed`, `air_progress`, `movement_zone`, `rot`. `OverrideKind::Parachute` (`:494`) is dead — `begin_parachute_descent` (`parachute_descent.rs:45`) never calls `begin_override`. |
| `GetClassID` / see-through identity | **ABSENT** | `primary_kind` (`locomotor.rs:127`) is the nearest thing, but it is `Option` for back-compat, is consulted ad hoc (`world/techno_ai.rs:1206–7`), and is **not in `world_hash`**. |
| Base-default inheritance (which classes share which body) | **ABSENT** | Walk / Mech / Ship have **no branch of their own** in the speed model — they reach `movement_tick.rs:1322`'s generic ramp because no branch claims them. `LocomotorKind::Ship` appears **zero** times outside `locomotor.rs`'s constructors and `is_ground_mover` (`:386`), despite `locomotor_ready.rs:34–35` deliberately keeping a distinct `Ship` readiness variant. |

### 4.3 State that decides which locomotor runs, and is not hashed

`world_hash` covers 7 of ~30 `LocomotorState` fields (`world/world_hash.rs:765–787`:
`kind`, `layer`, `phase`, `hover_throttle`, `hover_bob_offset`, `altitude`,
`mission_ready_state`). Not hashed yet authoritative: `primary_kind`, **`piggyback`**,
**`override_state`**, `air_phase`, `subcell_dest`, `speed_fraction`, `fly_current_speed`,
`jumpjet_current_speed`, `air_progress`, `speed_type`, `movement_zone`, `rot`.

`piggyback` and `override_state` **select which locomotor runs**. A lockstep divergence in
either is invisible to the state hash. This is the single most consequential structural
gap found in the Rust survey.

### 4.4 Ordering inconsistency in the spine

Eight of the nine Phase-2 entries receive `live_order` (the LogicVector snapshot,
`world/mod.rs:2109`/`2167`). `tick_parachute_descent` (`parachute_descent.rs:81`) takes no
order parameter and iterates `entities.keys_sorted()` (`:94`); `tick_low_bridge_tube_movement`
(`tube_movement.rs:237`) likewise (`:241`). Both are deterministic but on a **different
order** from the rest.

### 4.5 Boundary state

The #1 invariant **holds**: grep over `src/sim/` for `crate::render`, `crate::ui`,
`crate::sidebar`, `crate::audio`, `crate::net`, `crate::app` → **zero hits**.

Real issues, descending:

1. **`sim/movement` → `sim/world` upward dependency** — `movement_tick.rs:34`,
   `movement_step.rs:30`, `bump_crush.rs:556`, `turret.rs:149` import from the module that
   calls them. Movement cannot be lifted or tested without `world`.
2. **Render data written by sim.** `Position.screen_x/screen_y` updated with `f32` inside
   sim ticks: `air_movement.rs:410–418`, `:509–517`, `parachute_descent.rs:116–124`,
   `droppod_movement.rs:149`, `rocket_movement.rs:190–192`, `:222–224`. Each is commented
   "render-only f32" and none feeds back, so not a determinism violation — but sim is
   computing screen coordinates.
3. **`f32` mutated inside sim components.** `LocomotorState.infantry_wobble_phase`
   (`locomotor.rs:205`, advanced at `movement_step.rs:992–1002`) and `jumpjet_wobbles`
   (`locomotor.rs:166` → `jumpjet_movement.rs:83–95`). Both `#[serde(skip, default)]` and
   absent from `world_hash`, so lockstep is unaffected — but nothing enforces that beyond
   two attributes.
4. **Outside code writing locomotor execution state**: `docking/bunker_install.rs:341–346`
   and `miner/miner_dock_sequence.rs:598–608` build and install `forced_drive_track`;
   `miner/miner_system.rs:1572` calls `begin_drive_piggyback_for_teleporter` directly;
   `world/bridge_orchestrator.rs` mutates `GroundMovePhase`; `world_commands.rs:314–315`
   calls `loco.end_override()`.

### 4.6 The precedent to follow — `src/sim/substrate/`

303 lines total; every file small and majority-test. Five properties, from
`substrate/mod.rs:1–3` and `direction_tables/mod.rs:1–10`:

1. **Pure and read-only** — free functions and `const` tables, no `&mut`, no entity access.
2. **Deterministic, integer/fixed only** — `lepton_to_cell` (`lepton.rs:33`) reproduces
   `(v + (v>>31 & 0xFF)) >> 8` exactly; no floats.
3. **Minimal dependency floor** — `cell.rs:8` imports exactly one thing.
4. **gamemd-exact, with the evidence in the test, not the prose** — `cell.rs:36–54`
   asserts against a cited binary dump; `quantize.rs:40–51` covers the full `0..=255`
   input space with boundaries asserted explicitly.
5. **Derivation over duplication** — `LEPTON_DELTAS` is `const`-derived from `CELL_DELTAS`
   (`lepton.rs:14–25`) "so it cannot drift", with a second test proving the relation.

Plus two conventions: a safe accessor *and* a faithful one where gamemd is unsafe
(`cell_delta` returns `Option` and rejects the tube sentinel 8; `cell_delta_unchecked`
mirrors the unmasked indexing behind a `debug_assert`), and honest slice labelling in the
module doc with no completion claims.

**Name collision to avoid deepening:** `ObjectSubstrate` (`world/substrate.rs:49`) is a
*different* concept — mutable state owning the entity store, occupancy grid and
LogicVector.

---

## 5. The gamemd-native behavior contract

This is the specification a Rust substrate must honour. It is ordered because the binary
is ordered; x86 gives no reordering freedom at any of these points.

### 5.1 Construction and installation

```
C1.  type CLSID = TechnoTypeClass + 0x34C, 16 raw GUID bytes.
     Default written by TechnoTypeClass::Constructor is the TELEPORT CLSID
     (0x00710C21: MOV ECX,[0x007E9A90]), not Drive.                          [V]
C2.  ReadINI overwrites it only when CCINIClass::ReadCLSID (0x00527920) parses.
     On CLSIDFromString failure / empty value / missing section or entry it
     writes the passed-in DEFAULT and returns — no error, no log, no sentinel.
     A typo'd Locomotor= silently inherits the default.                       [V]
C3.  CoCreateInstance(rclsid, pUnkOuter=NULL, dwClsContext=7, IID_IUnknown)
     -> OleRun(punk) -> punk->QueryInterface(IID_ILocomotion, &slot)
     -> punk->Release().  Identical in UnitClass (via the 0x0041C250 wrapper),
     InfantryClass and AircraftClass (inlined).                               [V]
C4.  The wrapper RELEASES whatever pointer already occupies the destination
     slot BEFORE creating.                                                    [V]
C5.  A NULL result after QueryInterface is FATAL: PUSH 0x80004003 (E_POINTER),
     CALL 0x007DC720.  Contrast C2: bad GUID -> silent default; valid but
     unregistered GUID -> crash.                                              [V]
C6.  new.Link_To_Object(host)   -- ILocomotion slot 3, ALWAYS a separate call
     immediately after creation.  Stores the host twice; cannot fail; does NOT
     AddRef the host.                                                         [V]
C7.  Reassignment idiom, universal in this binary and byte-identical at every
     site:   store -> AddRef(new) -> Release(old),  guarded by `new != old`.  [V]
C8.  Refcount starts at 0.  The creator takes the first reference.            [V]
```

### 5.2 The host tick — `FootClass::AI` `0x004DA530`

Address-ordered, from `disassemble_function 0x004DA530`. **Only six call sites in the
whole function touch the locomotor.**

```
T1.  TechnoClass::AI (0x006F9E50) runs TO COMPLETION first (0x004DA539).
     Everything TechnoClass does this tick sees the PRE-Process position and a
     PRE-Process Is_Moving_Now.                                               [V]
T2.  Liveness gate on byte +0x90; re-tested after every re-entrant call below. [V]
T3.  Clear per-tick latch +0x6B3.                                             [V]
T4.  Self-repair-on-terrain block (rate RulesClass+0x1808).  No locomotor.     [V]
T5.  Is_Moving_Now()  [slot 32, call A @ 0x004DA692]
        -> gates SHROUD/SIGHT REVEAL, rate-limited to once per 15 frames
           (MOV ECX,0xF @ 0x004DA799).
     The reveal therefore uses LAST tick's coordinate and a PRE-Process
     movement reading.                                                        [V]
T6.  Every 16th frame: cell-action / tile-trigger dispatch (0x006E53A0).      [V]
T7.  SNAPSHOT the movement counter +0x538 into EBP (0x004DA806) -- BEFORE
     Process.  This snapshot is the entire basis of "did we move this tick".  [V]
T8.  Process gate chain -- ALL FIVE must hold, else skip Process AND the
     counter increment (jump to 0x004DAA01):
        +0x674 != 0        (a locomotor exists)
        byte +0x3CD == 0
        byte +0x8D  == 0
        +0x2A8 == 0  OR  TypeClass byte +0x692 != 0
        byte +0x81  == 0                                                      [V]
T9.  Process()  [slot 16 @ 0x004DA877]  -- EXACTLY ONCE PER TICK.  There is no
     second +0x40 call anywhere in the function.  Liveness re-tested at
     0x004DA87A; the unit may have died inside Process.                       [V]
T10. Movement-counter predicate; Is_Moving_Now calls C @ 0x004DA8BB,
     D @ 0x004DA96D, E @ 0x004DAA24.  Single increment site 0x004DA9FB.
     TypeClass+0x294 and +0x298 are used as DIVISORS of the global frame
     counter (IDIV @ 0x004DA90E, 0x004DA989) -- two independent step rates
     read from the TYPE, not from the locomotor.                              [V]
T11. CMP snapshot vs +0x538 (0x004DAA01) -> the moving/idle SOUND state
     machine (+0x53C latch, +0x540 three-tick hysteresis, handle at +0x544).  [V]
T12. Three byte-pair sound edge detectors: +0x8D/+0x8E, +0x3CD/+0x3CE,
     +0x425/+0x426.                                                           [V]
T13. Looping-sound position follow-up (0x00750D40) reads the POST-Process
     coordinate.                                                              [V]
T14. Idle scatter, every 64th frame, requires +0x5A4 == 0 and +0x8C == 0.     [V]
T15. PIGGYBACK END BLOCK -- see 5.3.                                          [V]
T16. Clear +0x6B4 (AFTER the swap).                                           [V]
T17. vt[+0x1D8]; if false -> FootClass::TryEnterTransport (0x0070D7E0).       [V]
T18. Team dispatch: ESI = [ESI+0x694]; if nonzero, [[ESI+0x69C]]+0x5C.        [V]
T19. Release(piggy) -- LAST, in the epilogue.                                 [V]

RNG ORDER, per unit per tick:
  locomotor-internal RNG (CrateClass::PickupDispatch 0x00481A00 and
  CellClass::Scatter_Objects 0x00481670, both called from inside Process)
  THEN the host rank-up RNG (0x0065C780 @ 0x004DAACB, which is after Process). [V]

The locomotor SWAP therefore takes effect on the NEXT tick, never mid-tick.
`In_Which_Layer` (slot 29) is never called by FootClass::AI -- there is no
mid-tick render-layer re-sort.                                                [V]
```

### 5.3 Piggyback — BEGIN and END

**BEGIN.** Byte-identical at `ChronoSphere::WarpUnitsAtCell` (`0x0065F174/F182/F190/F19A`)
and the war-factory exit (`0x0044E10E/E11D/E124/E12E`) — re-derived
instruction-by-instruction by the adversarial pass. It is one reusable idiom, not
per-callsite logic. **[V]** Two further instances of the same idiom were recovered by the
closeout pass and follow it step for step: the **ChronoWarp superweapon**
(`SuperClass::Launch` case 4, BEGIN at `0x006CC98C`–`0x006CCB57`) and the **Magnetron
warhead installer** (`0x00710213`–`0x007102E1`). Note the address correction: `0x0065EC30`
is **not** the superweapon — its sole caller is `TriggerAction__Execute`
(`get_function_callers 0x0065EC30`). See §5.4 and §11 C16.

```
B1.  new = CoCreateInstance(CLSID)                 (releases the ComPtr's prior occupant)
B2.  new->Link_To_Object(host)                     -- ILocomotion slot 3
B3.  QI(new, IID_IPiggyback) -> newPiggy
B4.  newPiggy->Begin_Piggyback(host->locomotor)    -- IPiggyback slot 3
       READS the OLD pointer straight out of the host field. Therefore B4
       STRICTLY PRECEDES B5.
       Body: if (victim == NULL) return E_POINTER 0x80004003;
             if (stash  != NULL) return E_FAIL    0x80004005;   // NEVER NESTS
             stash = victim; victim->AddRef(); return S_OK;
B5.  host->locomotor = new
B6.  new->AddRef()
B7.  old->Release()        // survives at refcount 1, held by the stash
B8.  Follow-up command, AFTER the field is assigned, never before:
       Chronosphere    -> Move_To(destination)        [slot 17]
       War-factory exit-> Force_Track(0x42, Coord3D)  [slot 28, @0x0044E160]
```

A failed `Begin_Piggyback` (E_FAIL because the target already holds a stash) means the
caller **abandons its swap** — the new locomotor is never installed. This is a mechanism,
not an error path.

**END.** Two forms.

*(a) Per-tick, inside `FootClass::AI` at T15* — re-derived instruction-by-instruction
across `0x004DAE5F`–`0x004DAF07`. **[V]**

```
E1.  loco = host->locomotor;   if NULL -> skip to T16
E2.  hr = loco->QueryInterface(IID_IPiggyback, &piggy)    [ILocomotion slot 0 -> thunk]
       QI has AddRef'd -> refcount is now 2.
       E_NOINTERFACE (0x80004002) silently tolerated; any other failure asserts.
E3.  if (piggy == NULL) -> skip to T16
E4.  if (!piggy->Is_Ok_To_End())  -> skip to T16                [IPiggyback slot 5, +0x14]
E5.  loco->Release()                                            [ILocomotion slot 2, +0x08]
       refcount 2 -> 1.  The object is alive ONLY because `piggy` holds a ref.
E6.  host->locomotor = NULL
       *** The field is observably NULL between E6 and E7. This window is real. ***
E7.  piggy->End_Piggyback(&host->locomotor)                     [IPiggyback slot 4, +0x10]
       Body: if (out == NULL) return E_POINTER;
             if (stash == 0)  return S_FALSE (1);
             *out = stash; stash = 0; return S_OK;
       OWNERSHIP TRANSFERS with NO AddRef and NO Release.
       Teleport's override additionally clears linked_object[+0x428] and
       [+0x42C] BEFORE the stash test.  Drive does not.  Jumpjet's override
       clears the same pair AND host[+0x6AD]/[+0x6AE] (0x0054DAC4/CD,
       0x0054DA83/8C).  The pair is ALSO cleared at warp completion by the
       Teleport state machine -- three clear paths, not one (see below).
E8.  host[+0x6B4] = 0
E9.  TryEnterTransport
E10. team dispatch
E11. piggy->Release()   -> refcount 0 -> deleting destructor -> free
```

**Consequence to preserve:** the warp locomotor is destroyed at the end of the same tick
in which `Is_Ok_To_End` returned true, but **after** `TryEnterTransport` has already run —
so a unit can begin entering a transport on the same tick its warp ends, with the field
already restored.

*(b) Out-of-band, at order time* — the same E4→E7 core, guarded by an explicit
`Is_Piggybacking()` (slot 7) test first, at `InfantryClass::Set_Destination`
(`0x0051B002`/`0x0051B022`/`0x0051B032`/`0x0051B039`/`0x0051B051`),
`UnitClass::Set_Destination` (`0x00742500`, `0x00742608`), `FootClass::Mission_Enter`
(`0x004D9325`), `FootClass::OnArrival` (`0x004D831F`), the war-factory exit
(`0x0044DFE0`), and the `IsLocomotor=` warhead installer (`0x007100D0`–`0x00710154`,
formerly mislabelled `TechnoClass::PerformDeploy` — §11 C15). **[V]** for the existence
and shape; the gating of the last is now traced (§5.4 row 6b).

**Two BEGIN shapes exist, and a port must model both.** The doc previously described only
"a failed `Begin_Piggyback` means the caller abandons its swap". That is one shape. The
other is **end-first**: `UnitClass::Set_Destination` (`0x00742608`) and the war-factory
exit (`0x0044DFE0`) run a complete END sequence *before* creating the new locomotor, so
they never hit the E_FAIL path at all. **[V]**

**`Is_Ok_To_End` predicates.** The dominant clause in both is `!Is_Moving()` — the swap
back cannot happen while the piggybacking locomotor still reports motion. That is what
makes a chrono warp last as long as it does instead of unwinding on the next tick. **[V]**

All **four** implementing live classes, with the interface-offset conversion applied
(IPiggyback `this` = object `+0x18`) and independently recomputed by an adversarial pass:

| Class | Body | Predicate, in object-base offsets |
|---|---|---|
| Drive | `0x004AF970` | `!Is_Moving()` ∧ `obj[+0x68] != 0` (stash) ∧ `obj[+0x65] != 0` ∧ `host[+0x6AD] == 0` |
| Teleport | `0x00719F30` | `!Is_Moving()` ∧ `obj[+0x48] != 0` (stash) ∧ `obj[+0x35] == 0` ∧ `host[+0x27C] == 0` ∧ **`obj[+0x38] == 0`** ∧ `host[+0x6AD] == 0` |
| Walk | `0x0075C8E0` | `!Is_Moving()` ∧ `obj[+0x38] != 0` (stash) ∧ `obj[+0x35] == 0` ∧ `host[+0x6AD] == 0` |
| Jumpjet | `0x0054DB00` | `!Is_Moving()` ∧ `obj[+0x94] != 0` (stash) ∧ `obj[+0x91] == 0` ∧ (`host[+0x6AD] == 0` **OR** `host[+0x6AE] != 0`) |

(`decompile_function 0x004AF970`, `0x00719F30`; `batch_decompile 0x0054DB00,0x0075C8E0`.)
**Jumpjet's host clause is an OR, not the conjunct the other three use.** A Rust port
written from the Drive/Teleport rows alone encodes the wrong predicate for lifted units.

**The gate meanings, all closed this pass except one:**

| Field | Meaning | Evidence |
|---|---|---|
| Drive `obj[+0x65]` | Non-reentrancy guard, **default 1**, cleared across a critical section. Ctor `0x004AF5BB` sets 1; setters `0x004B4BE0` (=0) / `0x004B4BF0` (=1) are Drive ILocomotion slots 45/46, bracketed around a team-member removal inside `FootClass::Find_Path` (`0x004D4101` / `0x004D4134`, receiver = `+0x674`). **Ship mirrors it exactly** (`0x0069ECCB`, setters `0x006A4214`/`0x006A4224`) | **[V]** shape; **[I]** motive — that the bracketed call can re-enter the END path was not proved |
| Walk `obj[+0x35]` | The **same** guard with the opposite encoding, **default 0**, raised across `Walk::Process` (`decompile_function 0x0075ac8a`: `+0x31 = 1` → `ProcessMovement` → `+0x31 = 0`, `this` = obj+0x04) | **[V]** |
| Teleport `obj[+0x35]` | Inherits Walk's layout (byte-identical constructor prologue) but **Teleport never writes it** — untruncated writer sweeps in all three `this` encodings (`0x35],` 43 hits, `0x1d],` 35, `0x31],` 22) find only the two constructors and Walk's bracket. The conjunct is satisfied at construction and stays satisfied | **[V]** negative; **[I]** that it means the same concept |
| Teleport `obj[+0x38]` | **The warp state-machine phase index, 0–7. `0` = idle.** `decompile_function 0x007192F0` (`TeleportLocomotionClass::StateMachineTick`) is an eight-arm switch on it: seeded from `host[+0x280]`, `INC dword [ESI+0x34]` @ `0x00719970`, reset to 0 @ `0x00719BDF`. `this` is the ILocomotion sub-object — proved by `LEA EAX,[ESI-0x4]` + a primary-vtable call at `0x0071931B` — so `[ESI+0x34]` **is** object `+0x38` | **[V]** |
| host `[+0x27C]` | "A chrono warp has been requested and has not completed." Default 0 (`0x006F2DD2`); set by `SuperClass::Launch` `0x006CCC3D` and both `Teleport_To` bodies (`0x004DF9EA`, `0x005231C1`); cleared by the Teleport state machine `0x007198E4` | **[V]** |
| host `[+0x6AD]` / `[+0x6AE]` | Instance state (**not** the `DeployToLand=`/`MobileFire=` type bytes at the same offsets — §2.9). `+0x6AD` = "held/suspended, normal movement refused"; `+0x6AE` = "a move/release was requested while held". `Set_Destination_Internal` `0x004D94B0` returns early on `+0x6AD` and sets `+0x6AE` on the clear-only teardown | **[V]** |
| Drive `obj[+0x68]`, Teleport `+0x48`, Walk `+0x38`, Jumpjet `+0x94` | the stashes (§2.7) | **[V]** |

**The "sense flip" was a category error, not a finding.** This document previously recorded
"the sense flip between Drive and Teleport is real and unexplained". Drive `+0x65` and
Teleport `+0x35` are not one field with two polarities — they are unrelated members at
different offsets in two incompatible layouts. Once Walk is placed beside Drive the picture
is one mechanism with two encodings: Drive stores *"ok to end"* (default 1, cleared inside
the guarded section), Walk/Teleport store *"busy"* (default 0, set inside it). Each
contributes the identical proposition to its predicate — *"we are not inside the guarded
section."* **[V]** for Drive and Walk from the bracket bodies; **[I]** for Teleport. See
§11 C18.

**`obj[+0x38]` is NOT redundant with `host[+0x27C]`, and this is the one gate a port is
most likely to drop.** Phase **2** of the state machine clears `host[+0x27C]` while phases
3–7 keep running (`decompile_function 0x007192F0`). There is therefore a live window —
reappear-and-validate — in which the warp latch is already down and only `obj[+0x38]` holds
the piggyback shut. Modelling `+0x27C` and omitting `+0x38` lets a unit resume driving
mid-warp. **[V]**

**`host[+0x428]` / `[+0x42C]` are a live `(BuildingClass*, HouseClass*)` pair, not a
write-only vestige.** `BuildingClass::DeployUnit_ChronoWarp` writes
`unit[+0x428] = building` and `unit[+0x42C] = building[+0x21C]` (the crediting house)
(`decompile_function 0x0070FF3F`); `SuperClass::Launch 0x006CCC67` writes `+0x42C` from
`super[+0x2C]`. Readers: `TeleportLocomotionClass::PostWarpValidation` (which passes both
together into `FUN_006B0AE0` on the validation-failure branch), Jumpjet `FUN_0054CA90`,
`TechnoClass::PointerExpired` (which nulls `+0x428` when the building dies — the engine's
own proof it holds a live pointer), and `TechnoClass::Load` (both handed to the save-game
remap machinery). **Cleared on three paths**: Teleport `End_Piggyback` `0x00719EFF`/`F08`,
the Teleport state machine `0x00719AEE`/`AF7`, and Jumpjet `End_Piggyback`
`0x0054DAC4`/`ACD`. `+0x42C` is **not** in the `PointerExpired` sweep, so clearing in only
one place leaves a stale house credit that a later failed warp will use. **[V]**; the
reading of `FUN_006B0AE0` as an occupant-ejection/credit sweep is **[I]**.

**Identity while piggybacking.** `Piggybacker_CLSID` returns the **stashed** class if a
stash exists, else its own (`disassemble_function 0x004AF610`, via
`QI(IID_IPersist)->GetClassID`). All mission / harvest / scatter logic queries through
this, not through the concrete active locomotor. **[V]**

### 5.4 Which situations swap a locomotor, and how often

Rewritten by the 2026-07-29 closeout pass; five of the ten original rows were wrong or
misattributed (§11 C15–C17). Mechanism codes: **SPAWN** = built in the class constructor
from the type CLSID; **PIGGY** = `Begin_Piggyback` stash; **RAW** = pointer replaced with
no stash; **END** = unwind only.

**Status of the table as a whole: [V] per row, [U] as an enumeration.** Each row's gate and
mechanism is verified at its cited address. The *set of rows* is not proved complete — see
the exhaustiveness note below. Do not describe this table as definitive.

| # | Trigger | Verified gate | Mech | CLSID installed | Frequency in a 30–60 min stock skirmish |
|---|---|---|---|---|---|
| 1 | Unit spawn (`InfantryClass`/`UnitClass`/`AircraftClass::Constructor`) | none — always | **SPAWN** | `TechnoTypeClass+0x34C` = `Locomotor=` | Once per unit ever created. Universal |
| 2 | `FootClass::AI` T15 per-tick probe | `+0x674 != 0` ∧ QI succeeds ∧ `Is_Ok_To_End()` | **END** (probe) | — | **Every tick, every mobile unit.** The probe is universal; the unwind fires only when a stash exists |
| 3 | `Teleporter=yes` **vehicle** move order, `UnitClass::Set_Destination` `0x00741970` | `type[+0xCD4] != 0` ∧ `+0x27C == 0` ∧ `+0x2B0 == 0` ∧ `+0x6AD == 0` ∧ active CLSID ≠ Drive (`0x007425F8`) | **PIGGY** (BEGIN `0x00742688`) | **Drive** `0x7E9A30` | **Very high, every Allied game.** `[CMIN]` is the only Allied harvester (`TechLevel=1`); `UnitClass::Mission_Harvest` has **six** `+0x480` dispatches (`0x0073E741/E83E/E8DF/EDB5/EE7F/EF62`, 771 instructions scanned, untruncated). Drive pops every time the miner stops, so the cycle re-installs per move leg × 4–10 miners. **Zero for Soviet/Yuri** — `[HARV]` is Drive |
| 3b | Same call, **refinery** leg | radio contact is `What_Am_I()==6` with `DockUnload=yes` (`+0x16B3`) ∧ destination is `What_Am_I()==0xB` (CellClass) ∧ `CellClass::FindFirstUnit(dest,0) == 0` ∧ active CLSID ≠ Teleport | **END** | restores Teleport | Once per completed ore run per miner. **This is the visible inbound warp** — and the reason the miner warps in but drives out |
| 4 | War-factory exit, `FUN_0044DCB9` | exiting unit's CLSID ∈ {Teleport `0x7E9A90`, `{4A582743}` `0x7E9A50`}; ends any existing piggyback first (`0x0044DFE0`) | **PIGGY** (BEGIN `0x0044E01B`) | **Drive** `0x7E9A30` | Once per Chrono Miner produced — a handful per Allied match plus rebuilds |
| 5 | **ChronoWarp superweapon** — `SuperClass::Launch` case 4 (`0x006CC4B2`) | ¬(`Organic=` ∧ ¬`Teleporter=` ∧ ¬`vt[+0x54]`) ∧ `vt[+0x160]`(IronCurtain)`== 0` ∧ `+0x27C == 0` ∧ `vt[+0x1D4] == 0` ∧ `vt[+0x1D8] == 0` | **PIGGY** (BEGIN `0x006CC98C`) | **Teleport** `0x7E9A90` | **0–7 per match**, × every object in the source footprint. `[GACSPH]` is `TechLevel=10`, `RechargeTime=7`; zero with superweapons off. Bursty — one use can swap a dozen locomotors in one tick |
| 5b | Same, `Organic=yes` ∧ ¬`Teleporter=` targets | as above | **none** — damaged via `vt[+0x16C]` | — | Same frequency. This is why infantry die instead of warping |
| 6 | **Magnetron** `IsLocomotor=yes` warhead (`WarheadTypeClass::Detonate 0x004696FB` → `0x00710000`) | `warhead[+0x15B]` ∧ target `What_Am_I() ∈ {1,2}` ∧ `target[+0x6AD] == 0` ∧ `vt[+0x160] == 0` ∧ `int(ctx[+0x6C]) > type->Size=` | **PIGGY** (BEGIN `0x007102D8`) | the **warhead's** `Locomotor=` — stock: **Jumpjet** `0x7E9AC0` | Bounded, not measured: **zero unless a Yuri player fields `[TELE]`** (`TechLevel=2`, `Cost=1000`); then one install per beam hit on a fresh vehicle/aircraft, `ROF=20`, `Range=12`. **Never on infantry or buildings** — the `What_Am_I()` gate, not the `Verses=0%` line, is the mechanism |
| 6b | Same call, victim is `ResourceGatherer=yes` and already piggybacking | `type[+0x5EC]` ∧ piggy ∧ `+0x2B0 == 0` ∧ `Is_Piggybacking()` | **END** | restores the stash | Only when a Magnetron beams a Chrono Miner mid-move. Rare |
| 7 | Magnetron release | `Set_Destination(NULL)` on a held unit sets `+0x6AE = 1`, satisfying Jumpjet's OR clause | **END** | restores the original | Once per lifted unit |
| 8 | Map-trigger teleport, host virtual `+0x508` | five gates; dispatched only from `FUN_0050D6D0` ← `FUN_006E1A40` ← `TriggerAction__Execute` | **PIGGY** | Teleport `0x7E9A90` | **Zero in skirmish.** Campaign/map-scripted only |
| 8b | `ChronoSphere::WarpUnitsAtCell` `0x0065EC30` | sole caller `TriggerAction__Execute` | **PIGGY** | Teleport | **Zero in skirmish.** Same category as 8 |
| 9 | `InfantryClass::Set_Destination` un-piggyback block `0x0051AFC8` | active CLSID == Jumpjet ∧ `type->HoverAttack == 0` (`+0x390`) | **END** | — | **Zero.** Every stock `JumpJet=yes` type also sets `HoverAttack=yes` (6 pairs, lines 3921/3966, 4715/4758, 10519/10564, 10817/10864, 10881/10924, 11151/11192). The two CLSID-Jumpjet types with no `HoverAttack=` at all — `[DISK]` and `[ZEP]` — are a VehicleType and an AircraftType, so **the exclusion rests on host class, not on `HoverAttack=`** |
| 10 | Jumpjet→Walk landing, `0x00521EB0` (InfantryClass `+0x4F8`) | ≤3 path steps remaining ∧ `type[+0xD94]` (`JumpJet=`) set ∧ active CLSID == Jumpjet | **PIGGY** | Walk `0x7E9A60` | **UNRESOLVED-REACHABILITY — [U], not zero.** See the residual below |
| 11 | Carryall pickup `0x00416BA1` / `0x00416BE8` | — | **RAW** | — | **Zero in stock skirmish** — sole `Carryall=yes` unit `[HIND]` is `TechLevel=-1`, absent from `[AircraftTypes]`. That INI fact was inherited, **not re-verified** this pass. The two raw stores are confirmed to exist |
| 12 | DropPod install `0x004DB8A0` | — | PIGGY | — | **Never** — zero references of any kind |
| 13 | Deploy into a building (`UnitClass::Deploy`) | — | **none** | — | The FootClass is destroyed and a BuildingClass created; 11 `CMP`-only sites on `+0x674`, zero `MOV`. Deploy does not swap the locomotor |

**Frequency ordering for prioritisation** (highest first): row 2 → row 3 → row 3b → row 6 →
row 4 → row 5. Rows 8–13 are zero or unresolved and are residuals, not implementation work.

**The headline stands and is now argued rather than asserted.** Piggybacking is not an
exotic Chronosphere-only path; it is on the critical path of the ordinary Allied economy.
An engine with no piggyback equivalent gets Chrono Miner movement wrong continuously.

**Exhaustiveness — read this before treating the table as a census.** Three untruncated
program-wide sweeps over 1,152,197 instructions found **51** sites touching `+0x674` across
**three distinct addressing idioms**: `MOV …[…+0x674], ` (17), `LEA …[…+0x674]` (27), and
`ADD reg,0x674` (7). Every lane ran a single-pattern sweep and was therefore blind to two
thirds of them — the war-factory exit itself uses the third idiom (`0x0044DE8A ADD
EBX,0x674`) and appears in neither of the first two. Eleven functions that touch the slot
appear in no lane report, and a **live store at `0x004DB95C`** sits inside the address
range row 12 describes as "zero references". Row 12's claim is about `0x004DB8A0`
specifically and survives; the surrounding range does not. **[U] as an enumeration.**

**Residual — row 10, the infantry Jumpjet→Walk landing.** An earlier draft of this closeout
called it dead on the grounds that `0x00521EB0` requires `HoverAttack == 0`. That is false:
`disassemble_bytes 0x00521EB0 len 210` (67 instructions, untruncated, covering the whole
gate chain to `0x00521F81`) **reads `+0x390` nowhere**. The `HoverAttack` gate belongs to
`0x0051AFC8`, a different function. `[JUMPJET]` Rocketeer — `TechLevel=3`,
`Prerequisite=GAPILE,RADAR`, an ordinary buildable Allied unit — satisfies every gate
`0x00521EB0` actually has. A separate deadness argument exists and is stronger but was not
discharged: the only two dispatchers of host `+0x4F8` that can reach a FootClass are both
inside `WalkLocomotionClass::ProcessMovement`, which `get_xrefs_to 0x0075aec0` shows has
**zero vtable references** — so the sole dispatcher needs Walk active while the gate needs
Jumpjet active. The piggyback delegation direction (whether an outer locomotor ever runs a
stashed Walk's `Process` while `+0x674` still reads Jumpjet) was not traced. **Neither
"live" nor "dead" is proven. Record it; do not scope S4 on it.**

**[R]** `piggyback.md` §12's infantry story — "this is why a Chrono Legionnaire walks
around normally" — is refuted, and this document's own follow-on ("what a Chrono Legionnaire
does is UNKNOWN") is now answered: **it teleports, and nothing swaps.** `[CLEG]` line 4155
is `Locomotor={4A582747-…}` = **Teleport**, with the Walk line `{4A582744-…}` **commented
out** on 4156 by Westwood's own author. The same pattern holds for `[CCOMAND]` (4208/4209),
`[CIVAN]` (4691/4692) and the three Teleport vehicles `[CMIN]`/`[CMON]`/`[SMON]`. A Chrono
Legionnaire is *born* with the Teleport locomotor in its constructor
(`disassemble_bytes 0x00517B40 len 130`: `CoCreateInstance(type+0x34C)` → QI into `+0x674`
→ `Link_To_Object`) and an ordinary right-click runs
`InfantryClass::Set_Destination` → `FootClass::Set_Destination_Internal` →
`Move_To` on that same Teleport locomotor. **There was never a Walk locomotor to swap out.**
**[V]** from retail INI bytes plus the constructor disassembly.

**Teleport's `Move_To` is ILocomotion slot 17 = `0x00718100`**, already named
`TeleportLocomotionClass__HeadToCoord` in Ghidra (`read_memory 0x007F5000 len 80`; the
vptr immediates are at `0x0071805E`, 34 bytes past where an earlier 60-byte window
stopped). Its body gates on four host virtuals, scatters infantry occupants of the target
cell, compares the destination against a null-coordinate triple, and on success sets object
`+0x34 = 1` and copies the destination into `+0x1C..+0x24`. **The warp itself is run by the
`+0x38` phase machine, not by `Move_To`.** **[V]**

**[R]** `piggyback.md` §12's vehicle chain: `0x007424FA JZ 0x007425DB` fires when the CLSID
**equals** Teleport and leads *into* the `CoCreateInstance(Drive)` at `0x00742688`; the
`Is_Piggybacking` chain the report cited (`0x00742534…`) is the **not-equal** fall-through.
The conclusion (a chrono vehicle gets a Drive piggybacked for ordinary movement) survives;
the cited instruction chain does not.

### 5.5 Destination — `FootClass::Set_Destination_Internal` `0x004D94B0`

The decompile of this function is unusable (it fabricates `iRam00000000` and a bogus
CLSID name); everything below is from `disassemble_function 0x004D94B0`. **[V]**

```
D1.  +0x5A0 = 0     -- UNCONDITIONAL, BEFORE all guards (0x004D94C7)
D2.  Three early-out guards, each "if FLAG and dest != 0 -> return":
        byte +0x6AD   (0x004D94BE)
        byte +0x82    (0x004D94D7)
        dword +0x2E4  (0x004D94E9)   <-- 0x2E4, NOT 0xB9. The decompile's
                                         param_1[0xB9] is an int* index.
     CLEARING to a null destination is NEVER blocked by any guard.
D3.  If +0x2AC != 0 and dest != 0: this->0x0070FEE0(1).
D4.  +0x5A4 = dest.
D5.  Clear-only link teardown (dest == 0 && +0x6AD != 0 && +0x2B0 != 0):
        partner->+0x2AC = 0 ;  this->+0x2B0 = 0 ;  this->+0x6AE = 1
     A symmetric two-object link, broken from one side, in this order.
D6.  MOVE path (+0x5A4 != 0):
       a. if +0x304 != 0: call its vt[+0xF8], then +0x304 = 0
       b. QI(locomotor, IID_IPersist) via helper 0x0045AEA0, then
          GetClassID (0x004D95B0, CALL [ECX+0xC])
       c. REPE CMPSD against the HOVER CLSID at 0x007E9A40. A match enables a
          rate-limited 12-byte timer update at +0x640/+0x644/+0x648 ONLY --
          it does NOT change the dispatch.
          (4 stock units: LCRF, ROBO, SAPC, YHVR.)
       d. if byte +0x6AC != 0: clear it and SKIP the locomotor call entirely
          (a one-shot "destination already applied" suppressor)
       e. else: coord = dest->vt[+0x4C](out, this);  locomotor->Move_To(coord)
          [slot 17, +0x44, one CoordStruct BY VALUE -- base stub is RET 0x10]
       f. Release the IPersist reference
D7.  STOP path (+0x5A4 == 0):
       if (What_Am_I() == 2 /*Aircraft*/ && (+0xAC == 1 || +0xB4 == 1)
           && +0x2B4 != 0)   -> SKIP Stop_Moving entirely
       else  locomotor->Stop_Moving()   [slot 18, +0x48, no args]
D8.  Tail, both paths: +0x6B7 = 0; two 12-byte countdown timers restarted from
     g_CurrentFrameCounter and RulesClass+0x1768.
```

### 5.6 What `Process` is allowed to do — the reverse-direction contract

**`Process` is not a leaf and not a pure position function.** From
`decompile_function 0x004B0500` and `get_function_callees` on
`0x004B0500`/`0x004B2630`/`0x004B0F20`, Drive's `Process` calls **[V]**:

- `FootClass::Stop_Moving` `0x004DF0D0` — *the locomotor stops the host*, not the reverse
- `FootClass::Find_Path` `0x004D3920` — **pathfinding runs inside Process**
- `FacingClass::UpdateFacing` `0x004C9300` on `FootClass+0x3A0` — facing is committed by
  the locomotor
- `CrateClass::PickupDispatch` `0x00481A00` — **crate pickup fires from inside the
  locomotor**, so its RNG is consumed at Process time
- `CellClass::Scatter_Objects` `0x00481670` — scatters *other* units
- `CellClass::Mark_Objects_Redraw` `0x00483480`, `MapClass::Check_Crushable_Obstacle`
  `0x00578AD0`, `TechnoClass::CanCrushCheck` `0x005F6CD0`,
  `TechnoClass::Clear_Convoy_Chain` `0x006EC3A0`, `RadioClass::Tether_Count` `0x006B7D80`
- `VocClass::PlayAtPos` `0x00750920` — audio
- `AnimClass::Constructor` `0x00421EA0` — **the locomotor allocates a world object**

and dispatches back through host virtuals `+0x2C`, `+0x184`, `+0x18C`, `+0x1B8`, `+0x1BC`,
`+0x2CC`, `+0x480` (**= `Set_Destination`, §2.9**), `+0x484` (**= `OnArrival`**, §5.6.3),
`+0x544`, and the `+0xF0`/`+0xF4` pair (**= occupancy MARK / UNMARK**, §5.6.2 — the earlier
downgrade to [U] is withdrawn).

**Occupancy is updated from inside Process**, not before or after; one `Process` call can
unmark and re-mark within itself. **[V]** — and §5.6.1 shows this happens by *two* routes at
once: the locomotor calls `+0xF4`/`+0xF0` directly around its own arithmetic, and the
position commit runs its own occupancy bracket internally.

#### 5.6.1 The position-commit contract — ordered specification

**Closed 2026-07-29 by a re-run.** The `oq4-position-commit` lane the closeout wave
commissioned died mid-run and produced nothing; this is the replacement pass — two
independent lanes (writer-sweep, vtable-identity) plus an adversarial cross-check. **Where
the lanes and the adversarial pass disagreed, the adversarial verdict is what is written
here**, and the places it overturned a lane are marked **[R]**.

**The binary facts below are [V]. The whole section is [UNCHECKED] as a *parity* claim** —
see "Parity status" at the end. The status legend is not upgraded by any of this prose.

**Headline: gamemd commits the position triple through TWO mechanisms, not one.** X/Y (and
the nominal full triple) go through host virtual `+0x1B4`; **Z has at least four additional
writers that bypass `+0x1B4` entirely, one of them called directly by a live locomotor.**
A Rust host surface with a single `set_coords` is therefore the wrong shape (§8 S3).

##### Mechanism 1 — the full-coordinate commit, host virtual `+0x1B4` = `0x004DB810` **[V]**

**P1. Slot identity.** `+0x1B4` resolves to `0x004DB810` on FootClass, InfantryClass and
UnitClass, byte-identical on all three (`read_memory 0x007E8E48 / 0x007EB20C / 0x007F5E24
len 16` → `10b84d00 a0be4100 60695f00 c0695f00`), and is not overridden below FootClass.
Derived a second, independent way from slot-delta arithmetic: `ObjectClass::GetCoords
0x005F65A0` sits at `+0x48` in both `0x007EB058` and `0x007F5C70`, and in fifteen distinct
vtables the `Set_Raw_Coords` DATA ref sits exactly `0x16C` above that vtable's `GetCoords`
ref (`get_xrefs_to 0x005F6940` → 17 DATA refs; `get_xrefs_to 0x005F65A0`); `0x48 + 0x16C =
0x1B4`. `get_xrefs_to 0x004DB810` returns exactly four DATA refs and **no CALL refs at all**
— it is only ever reached virtually.

**P2. Changed-test first, and it gates all follow-up work.** `decompile_function 0x004DB810`
compares the incoming coord against `this[0x27]/[0x28]/[0x29]` → bytes `+0x9C`/`+0xA0`/
`+0xA4`. Re-committing the same coordinate is cheap and side-effect-free. A Rust
`set_position` that always fires downstream effects is DRIFT.

**P3. The store is bracketed, gated on the placed byte `+0x74`** (`this[0x1d]`, byte read).
`+0x74 == 0` → bare store; otherwise `vt[0x124](0)` → store → `vt[0x124](1)`.

**P4. The store itself is a bare 3-dword move.** `ObjectClass::Set_Raw_Coords 0x005F6940`,
nine instructions — `ADD ECX,0x9c`, three `MOV`, `RET 0x4` (`disassemble_function
0x005F6940`; independently `disassemble_bytes 0x005F6940 len 40`). No redraw, no dirty flag,
no cell relink of its own. **Synchronous and immediate**: there is no deferred queue and no
end-of-tick apply, so every reader of `obj+0x9C` sees the new value the instant the store
retires. A single `Process` call commits more than once — six sites in Drive's
`Process_Drive_Track` alone — and the intermediate positions are observable to everything
called between them.

**P5. The bracket IS the occupancy update — chain fully traced, no inferred hop. [R]**
`vt[0x124]` = `0x004D3780` (`read_memory 0x007E8DB8 len 8`, i.e. `0x007E8C94 + 0x124`; same
value at `0x007EB17C` and `0x007F5D94`) → mode 2 returns early; otherwise `CALL 0x006F4A70`
and, if that succeeds and `vt[0x78]() == 2`, `TechnoClass::ExitCell_RemoveFromMultiCells
0x005687F0` for mode 0 / `EnterCell_AddToMultiCells 0x005683C0` for modes 1 and 3, both with
`ECX = 0x87f7e8` (the global map object) (`disassemble_function 0x004D3780`) →
`CellClass::AddContent 0x0047E8A0` / `RemoveContent 0x0047EA90`, each of which has **exactly
one caller** (`get_xrefs_to 0x0047E8A0` → `0x005684BB`; `get_xrefs_to 0x0047EA90` →
`0x005688EB`) → the object's own `vt[+0xF0]` / `vt[+0xF4]`, fed the object's `[0x27]/[0x28]/
[0x29]` triple (`decompile_function 0x0047E8A0`) → `cell[+0x124]` bit set/clear.

**This refutes the writer-sweep lane's central negative — "nothing in the commit path
touches occupancy". Occupancy is *inside* the commit bracket.** That lane left `+0x124`
UNCHECKED, guessed "cloak bracket" from the Ghidra label `Set_Coords_With_Cloak`, and then
wrote a design rule on the gap. **The rule "keep commit and occupancy separate" must not
reach S3.** Cloak is one branch reached through `0x006F4A70`, not the purpose.

**P6. The bracket's gate is LOCOMOTOR-dependent — found by neither lane. [V]**
The `vt[0x78]() == 2` gate is not a host property. `read_memory 0x007F5CE8` (Unit) and
`0x007EB0D0` (Infantry) both give `0x004DB7E0`, and `decompile_function 0x004DB7E0` shows it
forwards to **the locomotor's** vtable slot `+0x74` (`In_Which_Layer`) through the locomotor
slot `+0x674`. Drive's `+0x74` is `0x004B4820` (`read_memory 0x007E7EB0 len 128`;
`decompile_function 0x004B4820` = `return 2`) — exactly the value the gate wants, so
multi-cell occupancy is live on the ordinary ground-vehicle movement path. **Which locomotor
is installed changes whether the commit does occupancy work.** (Drive's vtable base
`0x007E7EB0` is fixed by the RTTI Complete Object Locator at `0x007E7EAC` and by
`get_xrefs_to 0x004B0500` → the single DATA ref `0x007E7EF0 = base + 0x40`, the `Process`
slot.)

**P7. The bracket also toggles the placed flag and fires a redraw.** `0x004D3780` →
`0x006F4A70` → `ObjectClass::Mark 0x005F5850` (`decompile_function` on each): gated on the
limbo byte `+0x81`, sets `+0x74` on put and clears it on remove, and fires **`vt[0x134]`
(redraw)** on put and on mode 2. **[V] by the vtable-identity lane; not re-read by the
adversarial pass.** Consequence for the port: this is a `sim/`-side call with a render
consequence in the original, and under the architecture invariant it must be expressed as a
dirty-flag write the render layer polls, never a call outward.

**P8. Intra-cell steps deliberately suppress the bracket.** `WalkLocomotionClass::
ProcessMovement`'s same-cell branch saves `host+0x74`, writes `0` ("not placed"), calls
`+0x1B4` then `+0x1CC`, and restores the byte; the cell-crossing branch instead runs the
explicit `vt[0x124](0)` → `vt[0x1B4]` → `vt[0x1CC](0)` → `vt[0x124](1)` sequence
(`decompile_function 0x0075AEC0`; commit sites `0x0075C12E` and `0x0075C20F`).
**[V] by the vtable-identity lane; UNCHECKED by the adversarial pass**, which did not re-read
the forging site. A port that unmarks and re-marks on every sub-cell step does strictly more
cell-list churn than gamemd and changes same-tick occupancy visibility for anything reading a
cell mid-tick.

**P9. On change only, the follower cascade.** `vt[0x84]()` yields a type object; if
`type+0x5E4` is non-zero, `FUN_007104f0` runs (`decompile_function 0x007104F0`): it reads the
object's own triple, walks a list obtained from `FUN_00473450()` — next-link at `piVar1[0xc]`
(byte `0x30`), continue while `*(byte*)(elem + 0x14) & 4` — and re-enters **`+0x1B4`** on each
follower with the parent's new coordinate. It runs **before** the setter returns, so attached
objects are never observable at a stale position later in the same tick. One commit can
cascade into N further commits.

##### Mechanism 2 — the height-only commit **[V]**

Z has at least four writers that never pass through `+0x1B4`. Established by a program-wide
writer sweep (`run_script_inline`: every instruction whose operand 0 is a written memory
operand with a non-`ESP`/`EBP` base and displacement exactly `0x9c`/`0xa0`/`0xa4`), which the
adversarial pass **reproduced exactly** — `TOTAL_INSTRUCTIONS=1152197`, `HITS=213` — and then
read differently:

| Writer | Sites | Route | Evidence |
|---|---|---|---|
| **`ObjectClass::AI`** | `0x005F3F52`, `0x005F3F60` | direct `MOV [ESI+0xa4],EDI`, per-tick | `disassemble_bytes 0x005F3F30 len 64` — `[ESI]` is dereferenced as the vtable throughout, so `ESI` is unambiguously `this` |
| **`FootClass::Set_Height_On_Bridge 0x005F5FA0`** | `0x005F5FFF`, `0x005F6047` | host virtual **`+0x1CC`** | `disassemble_function 0x005F5FA0` (`MOV ESI,ECX` at entry); slot from `0x007E8C94 + 0x1CC = 0x007E8E60` |
| **`FUN_005F6060`** | `0x005F607A`, `0x005F6092` | **direct non-virtual call** — five `UNCONDITIONAL_CALL` refs, no DATA refs, so not a vtable slot | `get_xrefs_to 0x005F6060`; `decompile_function 0x005F6060` (`param_1[0x29]`, `0x29 × 4 = 0xA4`) |
| `ObjectClass::Constructor` | `0x005F399F` | init | sweep hit |

**Each of the first three carries its own `vt[0x124](0)/(1)` occupancy bracket under the same
`+0x74` gate.** Height correction is a *separate commit with its own occupancy consequence*,
not a component of the XY commit.

**[R] The writer-sweep lane's writer set is wrong, from its own sweep output.** It concluded
"the genuine `ObjectClass`-coordinate writers are exactly `ObjectClass__Constructor` and
`ObjectClass__Set_Raw_Coords`". The specific error: it cleared `ObjectClass::AI` by checking
site `0x005F409C` (which writes `[ESI+0x10]` where `ESI = this+0x9C`, byte `0xAC` — a genuinely
different field) and treated that as clearing the whole function, when `0x005F3F52` and
`0x005F3F60` are different sites in the same function and are real `+0xA4` writes.

##### A live locomotor uses mechanism 2 directly **[V], ACTIVE-YR**

`HoverLocomotionClass::Move` commits XY through `+0x1B4` and then immediately calls
`0x005F6060` **non-virtually**, with the host in `ECX` (`disassemble_bytes 0x005148E0 len 48`):

```
005148e9  MOV ECX,dword ptr [ESI + 0x8]   ; ECX = locomotor->host  (loco+0x08 back-link)
005148ec  LEA EAX,[ESP + 0x4c]
005148f0  PUSH EAX                        ; the coord triple
005148f1  MOV EDX,dword ptr [ECX]         ; host vtable
005148f3  CALL dword ptr [EDX + 0x1b4]    ; XY commit through the host virtual
005148f9  MOV ECX,dword ptr [ESP + 0x24]
005148fd  PUSH ECX                        ; the Z value
005148fe  MOV ECX,dword ptr [ESI + 0x8]   ; host again
00514901  CALL 0x005f6060                 ; DIRECT non-virtual host Z-setter -> [host+0xA4]
```

The writer-sweep lane's handoff pass structurally could not see this: it hunted for sites
where an `obj+0x9C` *pointer* is handed to a callee, and here the callee receives the **host
object pointer** in `ECX` and does its own `+0xA4` arithmetic internally.

**Reachability is ordinary skirmish, not an edge case.** Hover is
`{4A582742-9839-11d1-B709-00A024DDAFD1}` (§2.6), and in `ini/rulesmd.ini` that CLSID is set on
`[LCRF]`, `[ROBO]`, `[SAPC]` and `[YHVR]`. Robot Tank is a stock buildable Allied unit and the
amphibious transports appear on every water map, so this path fires whenever one of them
moves.

##### What the locomotor does *not* do

No locomotor function body contains a store instruction targeting `+0x9C`/`+0xA0`/`+0xA4` —
that narrow negative survives three program-wide sweeps. **But the claim that matters for the
port — "the locomotor only ever moves the host through `+0x1B4`" — is false.** §11 C8.

##### Parity status — **[UNCHECKED]**

Everything above is binary-read semantic understanding: a well-provenanced basis for a
ratchet, **not parity evidence**. The one executable check attempted was
`emulate_function 0x005F6940` (`ECX=0x01000000`, `ESP=0x02000000`), which executed 16 steps
and returned `ECX = 0x100009c`, confirming the store base is exactly `this + 0x9C` — but the
tool **returns registers only**, and two different memory-seeding formats both left the
seeded coordinate registers at `0`, so the three stores are unobservable. **For a store-only
function `emulate_function` cannot witness the result.** There is therefore no gamemd-derived
executable check on the committer and no exhaustive proof over the input space. Any
`VERIFIED` label on a future Rust `set_position` must name its own check; nothing here can
supply one.

##### Residual that nobody closed, including the adversarial pass

Every `+0x1B4` / `+0xF0` / `+0xF4` call enumeration in all three reports covers only the
`CALL dword ptr [reg + disp32]` idiom (`search_instructions CALL "+ 0x1b4]"` → 94 hits,
1,152,197 scanned, untruncated). A **register-computed indirect call**
(`MOV EAX,[reg+0x1b4]` … `CALL EAX`) is outside every sweep run in all three reports. Residual
risk is low for a commit-path question — 94 sites already cover every live locomotor — but it
is not zero. Recorded in §10.

#### 5.6.2 `+0xF0` = occupancy MARK, `+0xF4` = occupancy UNMARK **[V]**

This is design-doc open question 5, and the answer **restores** the original occupancy
reading rather than replacing it. The "coordinate getter/setter" hypothesis this document
floated is **[R] REFUTED**; where it came from is §11b C21.

Slot reads (`read_memory 0x007E8D84 len 16` Foot, `0x007EB148 len 16` Infantry,
`0x007F5D60 len 16` Unit):

| Class | `+0xF0` (MARK) | `+0xF4` (UNMARK) |
|---|---|---|
| FootClass | `0x005F60A0` — `cell[+0x124] \|= 0x40` | `0x005F6120` — `cell[+0x124] &= ~0x40` |
| UnitClass | `0x007441B0` — `\|= 0x20` | `0x00744210` — `&= ~0x20` |
| InfantryClass | `0x005217C0` — `\|= (1 << subcell)`, plus occupier index `cell[+0x54] = vt[0x38]()` | `0x00521850` — `&= ~(1 << subcell)`, and `cell[+0x54] = -1` when `(bits & 0x1C) == 0` |

(`decompile_function` on all six.) On a bridge cell — `coord.Z >= GetGroundHeight(coord) +
DAT_00ac13bc` **and** `cell[+0x140] & 0x100` — every body targets `cell[+0x128]` instead. The
bridge test selects *which field*, it is not the purpose. **Both take a `CoordStruct*`, not
`this`-relative state; neither returns a value; neither writes any object field.** They are
pure cell-side effects.

Caller-semantics witness, independent of any Ghidra name: `CellClass::AddContent` calls
**`+0xF0`** (`0x0047EA43`, `0x0047EA7A`); `CellClass::RemoveContent` calls **`+0xF4`**
(`0x0047EB54`, `0x0047EB89`) — `search_instructions CALL "+ 0xf0]"` → 27 sites,
`CALL "+ 0xf4]"` → 50 sites, both 1,152,197 instructions scanned, untruncated. Add marks,
remove unmarks.

Teleport's every-exit-path `+0xF0` — the observation that produced the getter/setter lead — is
the **closing half of an unmark-at-entry / mark-at-exit bracket**, including the failure exit,
where it re-marks the host's *unchanged* coords. The same shape is in Walk (`+0xF4` at
`0x0075B391`) and Drive (`Apply_Track_Delta`: `+0xF4` at `0x004B0BAF`/`0x004B0BBF`/`0x004B0C0E`,
`+0xF0` at `0x004B0BFA`/`0x004B0C2E`), so it is a locomotor-wide convention, not
Teleport-specific.

**[I], not load-bearing:** the bit values differ by class (`0x40` Techno default, `0x20`
UnitClass, per-sub-cell for InfantryClass) because they encode different occupancy kinds. The
bits are verified; *why* they differ is not, and no consumer of `cell[+0x124] & 0x20` vs
`& 0x40` was enumerated.

#### 5.6.3 `+0x484` is `OnArrival`, not a commit **[V]**

The doc's surviving position-commit candidate is eliminated. Slots: `0x007E8C94 + 0x484` →
`0x004D82B0` (Foot), `0x007EB058 + 0x484` → `0x0051CBA0` (Infantry), `0x007F5C70 + 0x484` →
`0x00738970` (Unit) — three distinct overrides, a genuine polymorphic slot
(`read_memory 0x007E9114 / 0x007EB4D8 / 0x007F60F0 len 16`; the second dword of each is
`+0x484`). Bodies (`decompile_function 0x004D82B0`, `0x0051CBA0`, `0x00738970`), signature
`(bool, bool) -> bool`: FootClass opens with a **once-per-tick latch on host byte `+0x6B3`**,
reset at the top of `FootClass::AI` (`decompile_function 0x004DA530`), so it runs at most once
per object per tick no matter how many times the locomotor calls it; if the queued-destination
count is positive it re-issues the queue head through `+0x480` and shifts the queue down,
otherwise it does arrival bookkeeping and calls `+0x544`. Queue fields: `param_1[0x163]` →
byte `0x58C` (array), `param_1[0x166]` → byte `0x598` (count). Infantry and Unit call the
FootClass body first and then add post-arrival mission selection. **It reads and re-issues
destinations; it never writes `+0x9C`.**

Corollary the same pass established: **`Process` itself does not commit, for Drive or
Teleport.** `DriveLocomotionClass::Process 0x004B0500` and `TeleportLocomotionClass::Process
0x00719090` only *read* the host triple; the commit lives one level down in
`Process_Drive_Track` / `Process_Movement` / `Move` / `StateMachineTick`
(`decompile_function` on both, plus the 94-site `+0x1B4` enumeration). That is why the earlier
search for a commit "inside `Process`" came up empty.

### 5.7 Save / load

**Rewritten 2026-07-29. The prior version of this section was wrong in its conclusion; the
correction is C14.** The mechanism is stock OLE structured storage. It was derived twice
independently — once by the OQ1 lane, once by an adversarial pass that reached the identical
addresses before reading the lane. **[V]**

```
S1.  FootClass::Save 0x004DB690 writes the locomotor LAST:
       TechnoClass::Save
       two dword-vector dumps (+0x598/+0x58C, +0x5BC/+0x5B0)
       loco->QueryInterface(IID_IPersistStream @0x00820270)   0x004DB770
         E_NOINTERFACE tolerated; any other HRESULT asserts (0x007DC720)
       OleSaveToStream(pPersist, pStm)                        0x004DB7BE
         -> loco->GetClassID(&clsid)          primary slot 3
         -> WriteClassStm(pStm, clsid)        *** THE CLSID GOES ON THE WIRE ***
         -> loco->Save(pStm, TRUE)            primary slot 6, clears the dirty flag
       Release the QI'd interface                             0x004DB7D1     [V]

S2.  FootClass::Load 0x004DB3C0 mirrors it:
       Release + NULL the old locomotor       0x004DB3D8 / 0x004DB3DF
       TechnoClass::Load                      0x004DB3E7
       two dword-vector reloads
       OleLoadFromStream(pStm, IID_ILocomotion @0x007ED358, &this->+0x674)
                                              0x004DB568
         -> ReadClassStm(pStm, &clsid)        *** THE CLSID COMES BACK OFF ***
         -> CoCreateInstance(clsid, ..., IID_IPersistStream)  a FRESH instance
         -> newobj->Load(pStm)                primary slot 5, restores runtime state
         -> newobj->QueryInterface(IID_ILocomotion, ppv)  -> +0x674 = object+0x04   [V]
     The null-write at S2 line 1 and the refill are 0x189 bytes apart IN THE SAME
     FUNCTION.  Reading only the first is what produced the earlier wrong answer.

S3.  UnitClass::Load 0x00744470 then RE-INSTALLS THE POINTER across an in-place
     constructor re-run -- the MSVC _com_ptr_t::operator= idiom:
       0x0074450B  EDI = [ESI+0x674]          the RESTORED locomotor, not NULL
       0x0074451C  FootClass::Constructor     0x004D355E zeroes +0x674
       0x00744521..34  UnitClass's own four vptrs rewritten
       0x0074453C  EBP = [ESI+0x674]  (= NULL)
       0x00744542  CMP EBP,EDI -> NOT equal
       0x00744548  [ESI+0x674] = EDI          *** PUTS THE LOCOMOTOR BACK ***
       0x00744553  AddRef;  0x00744564 Release  -- the ledger balances exactly    [V]
     The identical shape is in InfantryClass::Load (0x00521A38) and
     AircraftClass::Load (0x0041B512).  It is the universal FootClass-host
     pattern, not a UnitClass quirk.                                             [V]

S4.  A unit saved MID-PIGGYBACK round-trips COMPLETELY, stash included.
     Drive::Save 0x004AF800 writes a 1-byte presence flag then recursively
     OleSaveToStream's the stash; Drive::Load 0x004AF780 reads the flag
     (0x004AF7C3/C6) and recursively OleLoadFromStream's it into [ESI+0x68]
     (raw asm: 0x004AF79C clears it, 0x004AF7D1 ADD ESI,0x68, 0x004AF7EB the
     call).  Teleport is the same shape at +0x48 (Save 0x00719D40, Load
     0x00719CA0).  So a Chrono Miner saved mid-warp restores BOTH the active
     Drive and the stashed Teleport, host-then-stash on both sides.              [V]
     NOTE: the FORMAT supports arbitrary piggyback depth.  Depth is bounded to
     <= 1 only by Begin_Piggyback's runtime refusal to nest (5.3), never by
     the stream.

S5.  Three categories are DELIBERATELY NOT restored from the blob:
       - the refcount at +0x14 (saved to EBX at 0x0055AAFE, written back at
         0x0055AB1B -- the saved process's refcount is discarded by design)
       - every vtable pointer (rewritten from link-time constants)
       - the host back-links at +0x08/+0x0C, which come off the blob STALE,
         are zeroed by the swizzle registrar, and are re-resolved by the
         post-load fixup pass.                                                   [V]

S6.  The post-load fixup pass is SwizzleManagerClass (singleton DAT_00B0C110,
     ctor 0x006CF1D0).  LocomotionClass::Load 0x0055AAC0 registers its own
     identity (0x0055AAF7 -> FUN_006CF2C0) and submits BOTH host back-links
     for remapping (0x0055AB10 for +0x0C, 0x0055AB2C for +0x08).  The resolve
     pass FUN_006CF350 runs twice from Load_Game 0x0067E440.                     [V]
```

**This closes open question 17 by half.** A reader of `+0x08` now exists —
`LocomotionClass::Load 0x0055AB2C`, which submits it for pointer swizzling on every load.
*Why* the owner is stored twice is still **[U]**.

**What a Rust snapshot must carry, and what it must not.** Now evidence-backed rather than
asserted:

- **Must:** the installed class discriminant (the CLSID's direct analogue — the one thing
  gamemd unambiguously writes to the stream); per-locomotor runtime state (gamemd dumps the
  whole object image — locomotor state is *not* reconstructed from rules); the piggyback
  stash **recursively, with its own class identity**, nested under the host in that order.
- **Must not:** any pointer-identity or swizzle token (the 4-byte `this` header is an
  artifact of C++ raw-pointer persistence with no semantic content); a refcount — gamemd
  explicitly discards the saved one, and since §1.1 makes the refcount the piggyback
  teardown trigger, **the teardown trigger is not persisted state and must not enter
  `world_hash`**; vtable/dispatch identity beyond the discriminant; a duplicate of the host
  link (gamemd stores the owner twice and swizzles both — one piece of state, not two).

**The parity claim this permits — and the one it does not.** The gamemd mechanism is now
CHECKED, and it *justifies* the handle shape for a concrete reason: gamemd persists a raw
`this` pointer purely so a post-load pass can rewrite stale pointers, and a handle is that
indirection paid at design time instead of at load time. But a **byte-level comparison
against a gamemd save is impossible** — the stream carries raw process pointers and a
swizzle table. S3's save/load acceptance can therefore assert only **semantic**
correspondence: the set of persisted facts and the host-then-stash ordering. Per AGENTS.md
that is not an executable gamemd-derived check, so **the save/load parity status stays
UNVERIFIED**, and a Rust-vs-prior-Rust snapshot hash remains a ratchet. The OLE contract
step itself (what `OleSaveToStream`/`OleLoadFromStream` do internally) is **[I]** — ole32
was not decompiled; it is corroborated from the gamemd side by every class overriding
`GetClassID` to return its own CLSID (Drive's at `0x004B4830` copies 16 bytes from
`0x007E9A30`) and by the `CoRegisterClassObject` block §2.6 already documents, which exists
precisely so `CoCreateInstance` can find the class on load.

**[R]** `host-contract.md` §5.6's "the save format stores a **surrogate token** in `+0x674`
and load rewrites the object's vtable pointers before reinstalling" is still unsupported —
the rewritten vtables at `0x00744521`–`0x00744534` are `UnitClass`'s own. That much of the
earlier correction survives.

---

## 6. The Rust-native replacement boundary

**Rust-native structure, gamemd-native semantics.** Nothing below reproduces COM: no
vtables, no refcounting, no `IUnknown`, no `CoCreateInstance`, no inheritance tree, no raw
pointer vectors. What is reproduced is the ordered behavior contract of §5.

### 6.1 What is pure plumbing and gets dropped

The two vptrs, the `-4` adjustor thunks, the `this`-pointer translation, IUnknown identity,
the `_com_ptr_t` template, `CoCreateInstance`, `OleRun`, `CoRegisterClassObject`, the
class-factory objects, and the CLSID registry are all mechanism for "call the right
function for this locomotor kind". A Rust `enum` + `match` reproduces every observable
outcome exactly. No RNG, no ordering effect, no state. **Drop all of it.**

The atomicity of the refcount is likewise not load-bearing (§1.1).

### 6.2 What is load-bearing and must be modelled explicitly

1. **Piggyback capability is a per-class predicate that changes control flow.** Six of
   eleven answer `QI(IID_IPiggyback)`; five return `E_NOINTERFACE`, which is silently
   swallowed and skips the entire teardown block. Ship it as an explicit capability, not
   as "everything can piggyback".
2. **The teardown *ordering*** — release-current, null-the-slot, restore-stashed, and
   destroy-popped **after** transport entry (§5.3 E5–E11). The null window is real.
3. **See-through identity** — `Piggybacker_CLSID`. Mission code must ask "what kind of
   locomotor is this *really*" and get the stashed kind while piggybacking.
4. **Non-nesting, with TWO caller responses.** `begin` on an already-stashed locomotor
   fails (`E_FAIL`). Some callers abandon the swap; two of the four live BEGIN sites
   (`UnitClass::Set_Destination 0x00742608`, war-factory exit `0x0044DFE0`) instead run a
   complete END *first* and therefore never hit the failure path. Model both (§5.3).
5. **The `Process` gate chain and once-per-tick guarantee**, plus the movement-counter
   snapshot before / compare after.
6. **The RNG order** — locomotor-internal draws before host draws, per unit per tick.
7. **The eight live base defaults** (§1.5), especially slot 6 (identity coord) and slot 32
   (virtual forward to the object's *own* slot 4, so Teleport does not get `false`).

### 6.3 Where the Rust boundary must cut DIFFERENTLY from the C++

| Slot(s) | C++ shape | Rust cut |
|---|---|---|
| 8, 9, 10, 11, 12, 14, 15, 34 | On the same 40-slot interface the sim holds | **Not on the sim locomotor at all.** A separate `render::locomotor_visual` reads sim state read-only. **The justification is what they WRITE — a 3×4 matrix plus a render memoisation key, and no sim state — not what they read.** The "these read a wall clock so they'd break lockstep" argument is **[R]**: `disassemble_bytes 0x0055A730` shows the receiver is `linked_object + 0x388`, per-unit FootClass state, and `motion-slots.md` §5.1 shows that same field gating Drive's `Is_Moving_Now`, a sim predicate. Whether `+0x388` stores ticks or wall-clock is **[U]**. |
| 21 Tilt_Pitch_AI | One virtual that both advances state and feeds the draw matrix | **Split.** The tilt *state* is deterministic sim state advanced each tick and must be snapshotted; the *matrix* built from it is render. Putting the whole thing in `render/` breaks save/load and replay; putting the matrix build in `sim/` breaks the invariant. Base is a no-op, so the base class carries no cost. |
| 29 In_Which_Layer | A locomotor query that also drives draw order | **Layer membership is sim state owned by the entity/scheduler; the renderer reads it.** Do not model it as a render query the sim calls. Note `FootClass::AI` never calls slot 29 — there is no mid-tick layer re-sort. |
| 33 Apparent_Speed | A locomotor method that tail-calls the object | **Not a locomotor method.** Read the unit's speed directly; give only Fly an override hook (`0x004CFE20`). |
| 7 Can_Enter_Cell | An ILocomotion slot | **Not on the Rust locomotor.** It is constant 0 for all 8 live classes. The real contention model is the host-side 0–7 return-code machine inside `Process_Movement` (`0x004B2630`, four dispatch sites), with `IsTrain` (+0xC94) downgrading every code below 7 to OK and `Crusher` (+0xD28) downgrading only codes 4/5. Note: the claim that the *host* slot `+0x1AC` is named `Can_Enter_Cell` is **[U] UNPROVEN** (§11 C9); the 0–7 state machine itself is **[V]**. |
| 26, 27 Push/Shove | Two interface slots | **Do not exist.** Unreachable in gamemd. |
| 35, 38, 13 | Trait methods | **Constants.** `false`, `false`, "not handled" — Tunnel is the sole overrider of each and Tunnel is dormant. |
| 25 Is_Ion_Sensitive | A trait method behind an ion gate | **Does not exist.** |
| 5 Destination | A getter returning `{0,0,0}` in the base | Every live class overrides it; the base body and its three globals are **not ported**. |

### 6.4 Proposed module and type layout

Two homes, because locomotion is not one kind of thing. The pure half follows the
`src/sim/substrate/` precedent exactly; the mechanism half cannot, because `Process`
mutates the world (§5.6).

```
src/sim/substrate/locomotion/
    mod.rs              // doc + flat re-exports; depends only on util/ + rules/
    class.rs            // LocomotorClass enum (8 LIVE classes) + CLSID <-> class table
    defaults.rs         // the 8 live base defaults, as const/pure fns, one per slot
    capability.rs       // piggyback_capable(), overrides_link(), power_observable()
    ready.rs            // MOVED from movement/locomotor_ready.rs, unchanged

src/sim/movement/locomotion/
    mod.rs              // doc: this is the mechanism half; it owns &mut world access
    slot.rs             // LocomotorSlot -- the host-side field (was FootClass+0x674)
    instance.rs         // Locomotor enum + per-class state structs
    install.rs          // create / link / raw-replace, in the verified order (5.1)
    piggyback.rs        // begin / end / is_ok_to_end / is_piggybacking / effective_class
    process.rs          // the SINGLE per-tick dispatch and its gate chain
```

The pure half:

```rust
// substrate/locomotion/class.rs
/// The locomotor classes a stock YR unit can actually select.
/// Mech, DropPod and Tunnel are DORMANT-TS and deliberately absent.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum LocomotorClass {
    Drive, Hover, Walk, Fly, Teleport, Ship, Jumpjet, Rocket,
}

/// Retail GUID text -> class. The eight values that appear in rulesmd.ini.
/// Parse failure returns None; the CALLER applies the type default, which is
/// Teleport, matching TechnoTypeClass::Constructor. Never hard-error: gamemd
/// silently falls back.
pub fn class_from_clsid(text: &str) -> Option<LocomotorClass>;

// substrate/locomotion/capability.rs
/// Six of eleven native classes answer QI(IID_IPiggyback); DropPod is one of
/// them but is dormant, so five ship. Five return "no capability" and the
/// caller skips the entire teardown block.
pub const fn piggyback_capable(c: LocomotorClass) -> bool {
    matches!(c, Drive | Ship | Walk | Jumpjet | Teleport)
}
```

The mechanism half:

```rust
// movement/locomotion/instance.rs
pub enum Locomotor {
    Drive(DriveLoco), Hover(HoverLoco), Walk(WalkLoco),   Fly(FlyLoco),
    Teleport(TeleportLoco), Ship(ShipLoco), Jumpjet(JumpjetLoco), Rocket(RocketLoco),
}

// movement/locomotion/slot.rs
/// One field on GameEntity, replacing LocomotorState.kind + the twelve sibling
/// Option<XState> fields' SELECTION role. There is no back-pointer: the host id
/// is an argument to every call, which is the Rust expression of Link_To_Object.
pub struct LocomotorSlot {
    active: Locomotor,
    /// Set at install; what Piggybacker_CLSID reports when nothing is stashed.
    installed_class: LocomotorClass,
    /// Base +0x10. Default TRUE. Observable only where Hover overrides.
    powered: bool,
}

// movement/locomotion/piggyback.rs
/// The stash lives INSIDE the piggyback-capable variant, exactly as the native
/// class stores it at its own object offset. Nesting is impossible by type.
pub struct Stash(Option<Box<Locomotor>>);

pub enum BeginError { NullVictim, AlreadyStashed }   // E_POINTER / E_FAIL
pub enum EndOutcome { Restored(Locomotor), NothingStashed }  // S_OK / S_FALSE
```

Dispatch is a `match`, not a trait object. Three reasons, all contract-driven: the
discriminant must be a first-class query (`Piggybacker_CLSID` see-through, §5.3), the set
is closed at eight, and `BTreeMap`-ordered determinism plus snapshot round-tripping is
simpler with a plain enum than with `dyn`.

`Process` takes world access, because gamemd's does:

```rust
// movement/locomotion/process.rs
/// Called AT MOST ONCE per entity per tick, behind the five-term host gate.
/// Returns the native bool (base returns TRUE).
pub fn process(slot: &mut LocomotorSlot, ctx: &mut LocomotorCtx<'_>) -> bool;

/// Everything Process is verified to reach: pathfinding, occupancy, facing,
/// animation spawn, sound events, crate pickup, neighbour scatter, and the
/// host's own stop_moving. A pure `-> Position` signature CANNOT reproduce it.
pub struct LocomotorCtx<'a> { /* entity id, entities, occupancy, path grid,
                                 terrain, rng, sound-event sink, anim sink */ }
```

### 6.5 Invariants the design keeps

- `sim/` never references `render/`, `ui/`, `sidebar/`, `audio/`, `net/`. The render slots
  live above `sim/` and borrow sim state read-only.
- All locomotion math is `fixed`-point. The two existing `f32` fields on `LocomotorState`
  (`infantry_wobble_phase`, `jumpjet_wobbles`) move to the render half or are re-derived
  in fixed point; `ALTITUDE_VISUAL_SCALE` and the `screen_x/screen_y` writes leave `sim/`.
- `EntityStore` stays `BTreeMap<u64, GameEntity>`; the locomotor is one field on the entity,
  not a side table.
- **`LocomotorSlot` in full — including the stash and the installed class — goes into
  `world_hash` and into the snapshot.** It selects which locomotor runs; leaving it out of
  the hash (as `piggyback` and `override_state` are today) means a lockstep divergence the
  hash cannot see.
- The `sim/movement` → `sim/world` upward dependency is not resolved by this design and is
  called out as a separate debt.

---

## 7. Ad hoc Rust logic to retire

Ranked by player impact = visibility × frequency. Each row carries a frequency clause,
because severity without one is a guess.

### R1 — Sharp-turn fallback silently drops a path node
`movement_step.rs:133–153`, specifically `target.next_index += 1; // drop the impossible path step`.
**Frequency: every ≥135° turn by every Drive-locomotor unit** — a unit exiting a war
factory, taking a reverse order, or navigating a base. Highest raw frequency on this list.
**Symptom:** the vehicle ends one cell off its planned route and repaths; visible as
overshooting corners and doubling back. It is also the direct producer of the non-adjacent
`next_index` that feeds R3.
**Replacement:** the verified native contract — build a straight track in the current
direction (`cur_dir × 9`), rotate in place, then continue on the **unmodified** path queue.
Node count is preserved. Substrate owner: `movement/locomotion/process.rs` + the existing
`drive_track` tables.

### R2 — `Is_Moving_Now` has an exact evaluator and no producer
`locomotor.rs:121`/`:273`/`:347` (never written), `mission/authority.rs:201–207`
(`DEGRADED_NOT_MOVING`), consumers `readiness.rs:167`/`:227`.
**Frequency: every mission ReadyToCommence evaluation, for every unit, every tick.**
gamemd's host reads slot 32 four times per tick per unit (§5.2 T5/T10).
**Symptom:** the mission gate always answers "not moving", so queued missions can commence
while a unit is still in motion.
**Replacement:** the substrate's `ready.rs` (already exact, already exhaustively tested)
fed by real per-class fields from `LocomotorSlot`. Delete the degraded constant.

### R3 — Tube-step kill gate destroys move orders
`tube_movement.rs:177–235` producer, `movement_tick.rs:1144–1147` the kill
(`TubePathStepResult::Blocked => { finished_entities.push(entity_id); continue; }`).
**Frequency: rare per unit, but it fires on the harvest loop, which runs continuously all
match, and the failure is total — the unit never resumes.** Two miner-stranding bugs have
already shipped from it (`6f6ec58e`, `4324d33b`), and both only *narrowed* the trigger.
**gamemd has no equivalent:** native path nodes are direction octants, so non-adjacency is
unrepresentable; tube entry happens via path sentinel node 8 plus the current-cell tube
index. The 2026-07-28 trace swarm states verbatim "No native movement kill gate exists".
**Replacement:** the octant + sentinel-8 path-node contract in the substrate; no abort path
at all.

### R4 — Locomotor power is absent, and hover's powered flag is hard-coded `true`
`movement_tick.rs:1888–1890` (`true,   // <- the powered flag`), `hover.rs:188`.
**Frequency: every hover unit, every tick** — the unpowered-sink branch exists and is
unreachable from production.
**Replacement:** `LocomotorSlot.powered`, default `true`, with the verified edges of §1.3.
**Explicitly do NOT add:** EMP-drives-power (the `EMPulseClass` constructor is unreachable),
Fly's two `Power_Off` RNG draws (unreachable — porting them would consume RNG gamemd never
consumes), or any ion-storm path.

### R5 — Two rival piggyback mechanisms, one dead kind, ten silently dropped fields
`locomotor.rs:134` (`piggyback`, hard-coded to Teleport←Drive, `:437` returns `false` for
any other primary) and `locomotor.rs:194` (`override_state`, a `Box<LocomotorState>` clone
whose `end_override` at `:518–535` drops `piggyback`, `primary_kind`, `subcell_dest`,
`hover_throttle`, `hover_bob_offset`, `speed_fraction`, `fly_current_speed`,
`air_progress`, `movement_zone`, `rot`). `OverrideKind::Parachute` (`:494`) is never set.
**Frequency: every Chrono Miner factory exit and every chrono-unit move order** — the
Allied economy's critical path.
**Replacement:** one mechanism, the ordered protocol of §5.3, with the stash living inside
the capable variant so nesting is impossible by construction. Add the whole slot to
`world_hash`.

### R6 — Scatter destination ordering is synthesized, not the native table
`scatter.rs:401–442` (`generate_spiral_offsets`, concentric square shells) replacing a
350-entry `(dx,dy)` table; consumed at `:213` and `:312–356`. Plus
`IDLE_SCATTER_INTERVAL: u64 = 150` at `:48–51`, whose own comment says the value is
rules-driven (`frame_counter % rules+0x1808`).
**Frequency: many times per minute during any base-building phase** — every building
placement, every MCV deploy, plus the idle-crowding timer.
**Symptom:** units scatter to visibly different cells than gamemd, because ordering *is*
the algorithm — the first passable offset in table order becomes the destination.
**Replacement:** the real diamond-ring tables in the substrate
(`FootClass::Find_Nearby_Passable_Cell` `0x56DC20`, 24 candidates then
closest-to-target or frame-modulo random), and read the interval from rules.

### R7 — CloseEnough uses the wrong distance metric
`movement_blocked.rs:87–105`, duplicated verbatim in `miner/miner_system.rs:1657–1662`.
The *mechanism* is native and must NOT be removed (`ini/rulesmd.ini:58 CloseEnough=2.25`,
bound to `RulesClass+0x1718` for code 6). The **metric** is Manhattan `(dx+dy)*256`; gamemd
uses a Euclidean `Sqrt_Approx`.
**Frequency: whenever a unit is blocked near its destination — routine in any base with
traffic.** At the stock 576-lepton threshold a unit blocked on a pure diagonal aborts at
~1.6 cells where gamemd aborts at 2.25.
**Replacement:** one shared `Sqrt_Approx`-equivalent in the substrate, consumed by both
copies. Two research traces already record this drift.

### R8 — Air fine-approach: a deadlock workaround plus a snap arrival
`air_movement.rs:70–78` (`FINE_APPROACH_THRESHOLD = 86`, `RAPID_DECEL_FACTOR = 0.5`,
`MIN_CREEP_SPEED = 0.05`), `:329–341`, `:385–398` (arrival `dist < 128 && speed < 0.05`,
then a hard snap of `position.rx/ry` to the goal and `sub_x/sub_y` to cell centre), and
`:374–375` (`.min(511)` hard map bound).
**Frequency: every aircraft move order** — Kirov, Rocketeer, Harrier/Black Eagle
return-to-pad, every helipad landing.
**Symptom:** aircraft visibly jump the last fraction of a cell instead of gliding in.
`MIN_CREEP_SPEED`'s own comment names it as a workaround for a deadlock that
`RAPID_DECEL_FACTOR` two lines above creates. `approach_target_speed` (`:100–110`) *is*
documented as matching native `Horizontal_Step` zones; this fourth zone is bolted on top.
**Replacement:** the native `FlyLocomotionClass::Process` flight model, including its real
map-boundary handling (deflect ±0x80 leptons for FlyBy types, else scatter) instead of
`.min(511)`.
**Status (2026-09-24):** the distance tiers (`approach_target_speed`) and the fine-approach
halving are deleted; Process's own target-speed writer (`0x004CE145..0x004CE2DB`,
`air_movement::write_fly_target_speed`) replaces them. The snap arrival and the map bound
remain; see the Fly target speed residuals in `2026-09-21-combat-parity.md`.

### R9 — Hardcoded 25 lep/s speed floor and `Speed=4` default, five copies
`movement_tick.rs:553–557`, `scatter.rs:368–377`, `world/world_commands.rs:86–89`,
`world/world_orders.rs:129–148`, `production/production_queue.rs:551–553`.
**Frequency: honestly UNKNOWN.** At 25 lep/s against a stock `Speed=4` base of roughly
100+ lep/s the floor likely binds only under heavy terrain penalties or a large
`speed_multiplier` reduction. **Ranked here on maintenance risk, not measured player
impact** — five copies guarantee divergence, and the `Speed=4` fallback masks a rules-lookup
miss as a moving unit instead of a visible failure.
**Replacement:** one speed resolver in the substrate, no floor unless one is found in the
binary and cited.

### R10 — Dormant-TS modules in the Rust tree
`sim/movement/tunnel_movement.rs` (21 KB: Underground layer, burrow FSM, `TunnelSpeed`) and
`sim/movement/droppod_movement.rs` (14 KB) plus their spine passes at `world/mod.rs:2201`
and `:2223`.
**Frequency: zero.** `[DRON]` carries `Locomotor={4A582741…};<-drive` = **Drive**, and
`grep -rn "4A582743" ini/` returns nothing anywhere; the native DropPod installer has zero
references and YR paradrop is `[PDPLANE]` + parachute. Both passes iterate every entity
every tick and always find nothing.
**Replacement:** deletion. Also review `rocket_state` and `homing_state`, whose writers
(`attach_rocket_state`, `attach_homing_state`) have no production caller either.

### R11 — `EntityCategory::Infantry` used as a proxy for "uses the Walk locomotor"
~15 sites, sharpest at `movement_step.rs:164` and `movement_tick.rs:375`
(`if category == EntityCategory::Infantry || mover_rot <= 0 { *facing = new_face; }` —
"infantry turn instantly, ignore ROT", which is a *Walk-locomotor* property asserted from
category). Wrong for `[JUMPJET]` (Rocketeer, an InfantryType with the Jumpjet locomotor)
and `[CLEG]`.
**Frequency: low today** — the affected units mostly route through the air and teleport
paths before reaching these branches. Structural, not a live behaviour bug.
**Replacement:** `snap.locomotor.kind` is already in scope at every site.

### R12 — Uncited constants (group; the fix is the same: cite or label VERA-internal)
`mod.rs:353` (4-lepton sub-cell arrival epsilon), `mod.rs:107–109`
(`CLIFF_HEIGHT_THRESHOLD`, doc says "Rust's **defensive** cliff detection"),
`mod.rs:110–117` (`INFANTRY_WOBBLE_*`, f32 in `sim/`, "just enough to feel alive"),
`air_movement.rs:37–58` (`checked_mul_log` saturating fallback in deterministic math),
`pathfinding/core.rs:384` (`TUBE_DIR_TIEBREAK = 9`, self-declared extrapolation past an
8-entry recovered table), `core.rs:118` (`CLIFF_COST_MULTIPLIER = 4`),
`terrain_speed.rs:32–36`, `path_smooth.rs:245/250`, `terrain_cost.rs:23`,
`locomotor.rs:107` (`FLY_CLIMB_RATE = 300`), `locomotor.rs:269` (`jj_turn_rate` default 4),
`drive_locomotion.rs:15` (`DRIVE_DESTINATION_BRAKE_FLOOR = 0.3`, no doc comment at all,
same number as `mod.rs:120 MIN_BRAKE_FRACTION`), `bump_crush.rs:61` (sub-cell centre radius
60), and the `if len > SIM_HALF { len } else { SIM_ONE }` guard duplicated at
`movement_tick.rs:427` and `movement_step.rs:199`.
**Frequency: individually low, collectively continuous.**

### What is already right — do not disturb

`movement_tick.rs:486–546` (three `pending_arrival_clear` re-arms, each labelled
VERA-internal with "the gamemd fallback is UNCHECKED" — the exact AGENTS.md form);
`movement_step.rs:204–217` (`hover_steer` hold-position, disclosed as an approximation with
the native behaviour stated, the residual bounded, and a plan doc named);
`pathfinding/terrain_speed.rs:11–14` (records a *removed* invented crowd-jam factor and
why — the template for retiring everything above); `pathfinding/core.rs:99–147` (multipliers
carrying real addresses); `locomotor_ready.rs`; `navcom.rs`'s NavCom/MovementTarget split;
and the absence of any unit-name string matching in `src/sim/movement/`.

---

## 8. Migration slices and acceptance tests

Each slice is independently landable and independently revertible. Per AGENTS.md, **a
shadow-mode slice flips to authoritative within two sessions or gets reverted** — the
shadow windows below are stated in sessions, not "eventually".

A note on evidence that applies to every slice: **a Rust-vs-prior-Rust hash or a replay
fixture is a regression ratchet, not parity evidence.** Where a slice's acceptance test is
a ratchet, it says so, and the parity claim stays `UNCHECKED`.

---

### S1 — Substrate skeleton — **LANDED `17841a46` (another session), reviewed 2026-07-29**

> **Delivered as specified.** `src/sim/substrate/locomotion/{mod,class,capability,defaults}.rs`,
> purely additive with no consumers, 4 tests green. Eight live variants with Mech/DropPod/
> Tunnel absent; CLSID table; `piggyback_capable` (Drive/Walk/Teleport/Ship/Jumpjet — five,
> correctly excluding the dormant sixth native provider DropPod); the nine base-default
> slots and the 8 × 9 inherit/override matrix.
>
> **`clsid_table_matches_retail_ini` is a genuine parity check.** It reads retail
> `ini/rulesmd.ini` bytes, strips `;` comments (correctly — two Drive rows carry the Mech
> GUID in trailing comments), and asserts the exact 155-key histogram plus
> `dormant_total == 0`.
>
> **[R] `base_default_map_matches_vtables` is mislabelled — it is a tautology, not a parity
> check.** Its `expected` array is a **verbatim duplicate** of the `INHERITS_BASE_DEFAULT`
> constant under test, so it can only catch someone editing one copy and not the other. The
> `PARITY:` comment claims the cells are byte-decoded from `read_memory`, but nothing in the
> test verifies that; per AGENTS.md prose never upgrades a status. **The underlying data is
> sound** — this review independently re-decoded three rows from live vtable reads and all
> matched exactly, including the two most distinctive claims:
> Drive (`read_memory 0x007E7EB0 len 160`) → `[f,t,f,f,f,t,f,f,f]`;
> Rocket (`read_memory 0x007F0B1C len 160`) → `[t,t,t,t,t,t,t,f,t]`, the only class
> inheriting slot 19 `Do_Turn`; Teleport (`read_memory 0x007F5000 len 160`) →
> `[t,t,f,t,t,t,t,t,f]`, the only class inheriting slot 32 `Is_Moving_Now`.
> **RESOLVED `fbf29094`.** The test was restructured to state the expectation in a
> different shape — per-slot inheritor sets, Drive/Ship row equality, the
> inherits/overrides complement over all 72 cells, and a whole-matrix total — so any
> single-cell change now fails at least one assertion. Proven by mutation: flipping Hover
> slot 31 fails with a message naming the slot and the missing class, and the previous
> version passed that same mutation. Relabelled **RATCHET**, since nothing re-reads the
> binary at test time.
>
> **All eight rows are now verified, 72/72 cells.** This review decoded every live class
> vtable and compared each slot pointer against the base entry at
> `read_memory 0x007EADF4`: Drive `0x007E7EB0`, Hover `0x007EACFC`, Walk `0x007F69F8`,
> Fly `0x007E89F4`, Teleport `0x007F5000`, Ship `0x007F2D8C`, Jumpjet `0x007ECD68`,
> Rocket `0x007F0B1C`. Distinctive facts confirmed: Rocket alone inherits slot 19
> `Do_Turn`; Teleport alone inherits slot 32 `Is_Moving_Now`; Walk alone overrides slot 30;
> Ship's row is identical to Drive's; 40 of 72 cells inherit.
>
> **Two enums now coexist** — `rules::LocomotorKind` (13 variants, hashed by discriminant,
> includes the two inert TS variants S7 retained) and `substrate::LocomotorClass` (8). That
> is expected mid-migration; **S3 owns resolving it**, and the S7 note about the hashed
> discriminant applies at exactly that moment.
>
> **Bookkeeping:** S7's `dormant_clsids_absent_from_retail_inis` was placed in
> `rules::locomotor_type` on the assumption S1 had not landed. It had. The two tests are
> complementary rather than duplicated — S1's checks the *parse path* in `rulesmd.ini`
> only; S7's checks raw GUID absence in **both** `rulesmd.ini` and `rules.ini`. No action
> needed, but a future tidy could co-locate them.

**Scope.** `src/sim/substrate/locomotion/{mod,class,capability,defaults}.rs`. No consumer
changes. Follows the `direction_tables` precedent exactly: `const` tables, free functions,
majority-test files, dependency floor of `util/` + `rules/`.

**What lands.** `LocomotorClass` (8 live variants — Mech/DropPod/Tunnel deliberately
absent); the CLSID ↔ class table; `piggyback_capable()`; the eight live base-default bodies
of §1.5 as pure functions; the per-class override map for those eight slots.

**Acceptance test.** `substrate::locomotion::class::tests::clsid_table_matches_retail_ini`
— parses `ini/rulesmd.ini`, comment-stripped, and asserts the exact histogram
**155 keys: Walk 60, Drive 52, Ship 13, Jumpjet 9, Fly 8, Teleport 6, Hover 4, Rocket 3**,
and that the three dormant GUIDs appear **zero** times outside `;` comments.
Plus `substrate::locomotion::defaults::tests::base_default_map_matches_vtables`, asserting
the inherit/override pattern for slots 6, 7, 19, 20, 28, 30, 31, 32, 39.

**Why this is a real parity check.** The first golden is **retail INI bytes** — machine-
derived, not hand-computed. The second is the 440-cell vtable matrix, which was decoded
byte-by-byte from `read_memory` by four independent passes with zero discrepancies; encode
the expected pattern as data in the test, with the `read_memory` addresses cited in the
test comment.

**Rollback.** Delete the directory. Nothing consumes it.

---

### S2 — `Is_Moving_Now` producers (retires R2)

**Scope.** Move `movement/locomotor_ready.rs` to `substrate/locomotion/ready.rs` unchanged.
Wire real per-class inputs from the existing `LocomotorState` fields. Delete
`DEGRADED_NOT_MOVING` and the `degraded_moving_gate` parameter.

> **[R] Corrected 2026-07-29 while writing the slice prompts.** The two symbols are **not** in
> `locomotor_ready.rs`, which this scope line implied. They are in `sim/mission/authority.rs`
> — `const DEGRADED_NOT_MOVING` at `:201`, the `degraded_moving_gate: bool` parameter at `:214`,
> and the `.or(if degraded_moving_gate { Some(DEGRADED_NOT_MOVING) … })` fallback at `:226`–`:227`
> that is what actually forces the gate. Also: the truth tables are at `:103`–`:303` (the file is
> 303 lines, `mod tests` opens at `:103`), not `:148`–`:302`. Verified by `grep -rn` over `src/`.

**What lands.** A production producer for `mission_ready_state`, so the mission gate stops
answering "no" unconditionally.

**Acceptance test.** The existing full-input-space truth-table tests
(`locomotor_ready.rs:148–302`) move with the file and must stay green — those *are* a real
proof for the predicate, being exhaustive over the input space. **New:**
`mission::readiness::tests::moving_unit_is_not_ready_to_commence`, a production-path test
asserting that a unit with a live `MovementTarget` reports moving, and a live-observe smoke
(`RA2_QUICKPLAY=minerloop.map`) confirming the miner still docks.

**Why it is a real parity check for the predicate, and not for the producer.** The truth
tables are gamemd-derived and exhaustive → the predicate claim may say VERIFIED. The
*producer* mapping (which Rust field feeds which native input) is **UNCHECKED** until each
input is traced to its native field; state that in the module doc.

**Risk.** This changes a gate that currently always answers one way, so it will move
behaviour. Expect a golden shift; if the tree carries another session's unmerged shifts,
record a line in `docs/scans/PENDING_REBASELINES.md` and leave the test red rather than
baking their work in.

**Rollback.** Restore the constant. Single-commit revert.

---

#### S2 — LANDED, and the verified native contract behind it

**Status.** Landed across `9b1385d7` (Drive/Ship/Teleport/Jumpjet), `6f187158` and `6fe38f7a`
(Walk/Hover + re-baselines) in `src/sim/movement/ready_producer.rs`. All six live families
produce `mission_ready_state`; `DEGRADED_NOT_MOVING` still exists in `sim/mission/authority.rs`
because Fly/Rocket/etc. return `None`.

**A parallel implementation exists — do not discard it.** Another agent ("Codex") implemented
S2 independently, **uncommitted**, in the `ra2-rust-game-locomotion-s2` worktree. It is better
structured: derives readiness on demand at the gate (no per-tick full-order scan), completed
the `locomotor_ready.rs` → `substrate/locomotion/ready.rs` move with all consumers, deletes
`DEGRADED_NOT_MOVING` and the `degraded_moving_gate` parameter, and is probably hash-neutral.
Its per-family mappings are weaker. The intended endpoint is **its structure with the verified
mappings below**.

##### The five readiness slots — VERIFIED 2026-07-29 from the binary

Decoded live this session; these supersede the lane summaries.

| Family | Slot-32 body | Verified predicate |
|---|---|---|
| Drive | `0x004AFC20` | `turning \|\| (own_Is_Moving() && head_to != null && owner_speed > 0)` (`disassemble_function 0x004AFC20`) |
| Walk | `0x0075AB40` | `own_Is_Moving() && *(double*)(owner+0x578) > 0.0 && head_to(iface +0x24/+0x28/+0x2C) != null` (`decompile_function 0x0075AB40`) |
| Hover | `0x00514C80` | `own_Is_Moving() && *(double*)(iface+0x44) != 0.0` (`decompile_function 0x00514C80`) |
| Jumpjet | `0x0054D0D0` | `state(iface +0x4C, int) != 0 && != 2` (`disassemble_bytes 0x0054D0D0 len 48`; Ghidra has no function defined there) |
| Teleport | inherits base slot 32 → own slot 4 `0x00718080` | `*(char*)(iface+0x30) == 1` (`decompile_function 0x00718080`) |

**`0x004c9480` — the stale-label trap.** Named `CDTimerClass__Remaining`; it returns a
**boolean 0/1**, not a count: `rate@+0x14 > 0 && (g_CurrentFrameCounter − start@+0x08) <
duration@+0x10`. Drive calls it on linked-owner `+0x388`. It is a **time** test — a port that
asks "is a facing target set" diverges, staying true after the rotation expires, and because
this term short-circuits the predicate to TRUE that yields a false "moving" (the stall
direction). Use the `FacingClass` port (`entity.body_facing.is_rotating(frame)`), which is
frame-count based and already matches.

**Corrections.**
1. **[R] My own earlier correction here was WRONG and is retracted.** I wrote that Walk's
   `moving_byte` is "not a raw byte at `+0x30` — the body calls its own slot 4". Both are
   true: the slot-32 body calls slot 4, and **Walk's slot 4 IS the byte read** —
   `decompile_function 0x0075AB30` is literally `return *(byte*)(this + 0x30);`. The lane
   report was right; I was wrong. Lesson: "it dispatches a virtual" and "it reads a byte" are
   not alternatives — follow the virtual before contradicting the source.
2. Jumpjet's body proves only that **0 and 2 are the false values**. The "0..6 enum,
   0=grounded / 2=hover-hold" labelling remains **[I] inference**.

##### The three `Is_Moving` (slot 4) shapes — VERIFIED 2026-07-29

The readiness slot's first term. These differ per family and are NOT interchangeable:

| Family | Slot 4 | Verified body |
|---|---|---|
| Drive | `0x004AFB80` | `dest(+0x30/34/38) != null → 1`; else `head(+0x3C/40/44) == null → 0`; else `head.xy == owner(+0x9C/+0xA0) → 0`; else `1`. **Z deliberately not compared** in the third test |
| Walk | `0x0075AB30` | `return *(byte*)(this + 0x30)` — a plain flag, no coord comparison at all |
| Hover | `0x00514C30` | `dest(iface +0x14/18/1C) != null \|\| head(iface +0x20/24/28) != null → 1`; else `0`. **No owner-position comparison** (unlike Drive) |

**Ghidra state written back and saved:** plate comments on `0x004c9480` and `0x00718080`;
`0x0075AB40` renamed from the wrong `WalkLocomotionClass__Is_To_Have_Shadow` (that is slot 8;
this is slot 32) to `WalkLocomotionClass__Is_Moving_Now`; `0x00514C80` from `FUN_00514c80` to
`HoverLocomotionClass__Is_Moving_Now`.

##### What is NOT native yet — the honest gap

**The predicates are native; the inputs are proxies.** S2 feeds gamemd's exact decision
functions with approximated inputs. Better proxies cannot close this — the gaps need state we
do not model.

**Gap A — `slot_moving` / `moving_byte`. [R] RE-FRAMED TWICE. Read the third version only.**

This section was rewritten once already, on the basis of the **slot-4** bodies. That was the
wrong predicate: the Mission gate reads **slot 32 (`Is_Moving_Now`)**, never slot 4. Slot 4
matters only as an inner term, because Drive/Ship/Walk/Hover's slot 32 opens with `slot4()`.
Both earlier framings of this gap are therefore superseded. Settled contract and citations:
[docs/research/ILOCOMOTION_IS_MOVING_NOW_SLOT32_AND_MISSION_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_IS_MOVING_NOW_SLOT32_AND_MISSION_GATE_GHIDRA_REPORT.md).

What survived re-grounding on the right slot: **all six Rust predicates were already
slot-32 shapes** — term set, conjunction structure, short-circuit order and signedness. The
worry that motivated the re-check did not materialise. Corrected in `9f153be3`:

- **Walk DOES have a coord term** — its slot 32 third conjunct tests the head-to triple at
  `iface+0x24`. The previous revision's "Walk needs no coord comparison at all" was true of
  slot 4 and false of the predicate the gate uses. Our `destination_nonnull` input already
  supplies that term; the defect is its *lifetime*, below.
- **Hover's two-source OR is real**, and is exactly the `slot4()` inner term of its slot 32.
  That part of the previous revision stands.
- **Jumpjet's arms were fabricated** and one of them (`Descending`) mapped to a value the
  predicate reads as moving, so a landing jumpjet deferred its mission for the whole descent.
  Fixed to fail toward not-moving; the state enum is UNDECODED and now says so.
- **Two doc claims were simply false** and are deleted: that Walk's native speed fraction is
  only ever 1.0 or 0.0 and never written by the walk locomotor (it is written from a range of
  values at nine sites), and that Hover's `!= 0.0` speed term is "the strict one" (it is
  weaker than `> 0.0` — it accepts negatives).

**What actually remains of Gap A** is one input's lifetime. Walk's third conjunct natively
reads a **sub-cell** point, so it is null in two states our `path[next_index]` is not: on the
tick a move order is issued (the byte is set synchronously by `Head_To_Coord`, but the coord
is only filled by the next movement step), and when the next cell has no free infantry
sub-cell to reserve. Both make us answer moving where native answers not-moving — the stall
direction. `LocomotorState::subcell_dest` is the structural match but is not cleared on a
failed reservation, so switching to it today would trade an over-inclusive input for a stale
one; closing this means giving that field the native coord's lifetime.

Exposure is bounded by the other two conjuncts, which both key off `live_move`: a blocked
walker, or one with no movement target, already reports not-moving regardless. What is left
is a one-tick disagreement on infantry order issue, plus the crowded-cell contention case.

**Gap B — `owner_speed`. Structurally missing, behaviourally near-neutral. Lower priority.**
Native keeps an owner-side applied speed fraction at `owner+0x578` (double), written by
`TechnoClass::SetSpeedFraction 0x004d3710`, which clamps `>= 1.0 → 1.0`, `<= 0.0 → 0.0`, else
the raw value. Drive's `Process` drives it to **exactly zero at rest** — gate verified at
`disassemble_bytes 0x004B0850 len 80`:

```
head_to.X/.Y/.Z all == NullCoord        (loco +0x3C/+0x40/+0x44)
  && owner path_queue[0] (+0x5E0) == -1
  && *(double*)(owner+0x578) > 0.0
  -> CALL owner_vtable[+0x544]  SetSpeedFraction(0.0)     @ 0x004b0880
```

Rust conflates this owner-side fraction with the locomotor-side ramp
(`DriveLocomotionRuntime::current_speed_fraction`, which carries the native `Stop_Moving` 0.3
clamp on `loco+0x4C` — a **different field**). **However:** that gate fires exactly when head-to
is null and the path queue is empty, which in our tree is when `movement_target` has already
been cleared — so the current proxy already returns 0 in the same cases. Implementing it is
structural fidelity, not a behaviour fix, and it adds hashed state and costs a re-baseline.
**[U]** whether any stock situation separates the two.

**Gap C — residual inferences.** That `TeleportPhase::Relocate` corresponds to the native
`+0x30` flag's window, and Jumpjet's 1/3/4/5/6 meanings, are both **[I]**.

##### Handoff — recommended order for the next session

1. ~~**Merge Codex's structure** — on-demand derivation at the gate.~~ **DONE, `9f153be3`.**
   Landed with the native justification rather than as a refactor preference: the readiness
   virtual makes a fresh locomotor call at each of its ~two dozen call sites, with no cached
   per-frame flag anywhere on that path, and the same object is gated on both sides of its own
   movement step in one tick. The stored field also left the lockstep hash and the snapshot
   wire format (version 30 → 31), where a derived predicate never belonged. Both ratchets
   re-baselined; composition-only, proved four ways.

2. **The live-observe smoke (`RA2_QUICKPLAY=minerloop.map`) is still unrun, and is now the
   highest-value outstanding check on the slice.** Both fixtures came out bit-identical under
   the on-demand change, which means neither exercises a mid-tick locomotor state change
   before a gate call — so the fixtures cannot speak to the paths the change exists for. The
   exposed shape is a same-tick stop followed by a mid-tick queue-and-commence: dock, unlink,
   unload, deploy. Needs ~200s machine idle (see `reference_vera20k_smoke_foreground_lock`).

3. **Pin `owner+0x388`** — the countdown that is Drive and Ship's first slot-32 term. We feed
   the facing-rotation timer, which fits, but the field is UNCHECKED. This is the only term
   whose misreading yields a false "moving", so if it is some longer-running timer, affected
   vehicles defer missions for its whole duration. Cheapest remaining correctness win.

4. **Decode Jumpjet's state enum** at `iface+0x4c`, then replace the placeholder arms. The
   route is known and cheap: Rocket's phases (3=boost, 4=cruise, 5=terminal) were recovered
   from its slot-16 `Process` switch, and Jumpjet has the same switch.

5. **Walk's sub-cell lifetime** — give `subcell_dest` the native coord's lifetime (cleared on
   a failed reservation), then switch the third conjunct to it. All that survives of Gap A.

6. **Gap B** only if a stock case is found that separates the owner fraction from the ramp.
   Note the Walk half of the old Gap B rationale was refuted (see Gap A); the Drive rest-gate
   decode below still stands.

7. **Ghidra label debt** from this investigation, worth clearing while the evidence is fresh:
   `UnitClass__ShouldIdle @ 0x00744270` is really the UnitClass readiness override,
   `FUN_00521b60` is the Infantry one, `AircraftClass__Override_Mission @ 0x0041b870` sits in
   the `Commence` slot, and Ship/Jumpjet/Rocket/Fly slot 32 are all unlabeled. Full list in
   the research report's "Corrections" section.

---

### S3 — `LocomotorSlot` on the entity, plus install and raw-replace

**Scope.** `movement/locomotion/{slot,instance,install}.rs`. Introduce `LocomotorSlot` as
the single authority for "which locomotor is installed", superseding the selection role of
`LocomotorState.kind` and the twelve sibling `Option<XState>` fields. Add the slot to
`world_hash` and to the snapshot.

**What lands.** The verified construction order of §5.1: parse-or-default (**default is
Teleport, not Drive**), silent fallback on an unparseable CLSID, create → link → store →
AddRef-equivalent → drop-old, guarded by `new != old`.

**Install-at-spawn is now the whole install story, and it is simpler than this document
first assumed.** One CLSID per type at `TechnoTypeClass+0x34C`, straight from `Locomotor=`;
constructed **once in the class constructor**; `QueryInterface` straight into `+0x674`; then
`Link_To_Object(host)`. Byte-for-byte the same shape in `InfantryClass`, `UnitClass` and
`AircraftClass` (`disassemble_bytes 0x00517B40 len 130`; `0x007354DC`; `0x00413DC1`).
**No stock YR unit is constructed with one locomotor and then permanently swapped to
another.** The Rust equivalent is: resolve the kind from the type at spawn, store it, done.

**Acceptance test — corrected; the version this document previously specified encoded the
wrong contract.** `movement::locomotion::install::tests::locomotor_beam_stashes_and_installs_jumpjet`
— from `ini/rulesmd.ini`: `[LocomotorBeam] IsLocomotor=yes
Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}`, reached from `[TELE]` via
`Primary=MagneticBeam` → `Warhead=LocomotorBeam`. Assert the victim's **effective** class
becomes Jumpjet **and the previous locomotor is STASHED, not dropped** — `0x00710000` runs
a full `Begin_Piggyback` at `0x007102D8` on the IPiggyback QI of the freshly created
locomotor, with the host's old pointer as the argument, in the canonical B4-before-B5
order (§5.4 row 6). Also assert the victim is a `What_Am_I() ∈ {1,2}` host — infantry and
buildings can never be lifted, by code.
Plus `install::tests::missing_locomotor_key_defaults_to_teleport`,
`install::tests::unparseable_clsid_falls_back_silently`, and
`install::tests::six_stock_sections_resolve_to_teleport` (`[CLEG]`, `[CCOMAND]`, `[CIVAN]`,
`[CMIN]`, `[CMON]`, `[SMON]` — the direct disproof of R11).

**S3 has lost its raw-replace example.** After this pass no stock-YR trigger in §5.4 is a
RAW swap except Carryall pickup, whose two stores (`0x00416BA1`, `0x00416BE8`) provably
exist but whose reachability rests on `[HIND] TechLevel=-1` — an INI fact inherited and
**not re-verified**. Either drop raw-replace from S3's scope, or land it labelled
VERA-internal with the stock trigger recorded as **UNCHECKED**, not as "none".

**Save/load: implement the §5.7 mechanism; the parity status stays UNVERIFIED.** The
blocker on *understanding* is gone — the re-acquisition chain is `OleSaveToStream` /
`OleLoadFromStream` with the CLSID on the wire, verified at `0x004DB7BE` / `0x004DB568`,
and the piggyback stash round-trips recursively. That validates the snapshot *shape*
(discriminant + full runtime state + nested stash, host-then-stash order) and rules three
things out of the snapshot (§5.7). It does **not** produce an executable gamemd-derived
check — a byte comparison against a gamemd save is impossible, and semantic correspondence
argued in prose does not upgrade a status. **Write `UNVERIFIED` in the module doc.**

**Why the rest is a real parity check.** The install goldens are retail INI bytes plus the
verified constructor default at `0x00710C21`, and the Magnetron test's expected outcome is
the verified BEGIN sequence at `0x007102D8`. None is a Rust-vs-Rust comparison. The
"installed class + stash present" assertion is still a *semantic* assertion, not exhaustive
proof — accurate labelling is "parity-grade golden, non-exhaustive assertion".

**The host-side position surface S3 defines — corrected by the OQ4 re-run.** S3 is the slice
that fixes the `sim`-side locomotor surface, so it owns this boundary even though the commit
itself is host state. Four constraints, all from §5.6.1:

1. **Two commit entry points, not one.** Both lanes recommended a single host `set_coords`;
   the adversarial pass showed that shape is wrong. gamemd has a **full-coordinate commit**
   (`+0x1B4`) *and* a **height-only commit** (`+0x1CC` plus a directly-called setter), and a
   live locomotor — Hover, on `[LCRF]`/`[ROBO]`/`[SAPC]`/`[YHVR]` — uses each within a single
   `Move`. A single XY-style setter has nowhere to put the three Z-only committers, and
   forcing Hover's height through it changes which occupancy brackets fire.
2. **The occupancy bracket belongs INSIDE the commit**, conditioned on the placed flag, on
   both entry points. The writer-sweep lane's guidance to keep commit and occupancy separate
   was written on an UNCHECKED gap and is **the one recommendation that must not be carried
   forward** (§5.6.1 P5).
3. **The commit is host-owned and not a public field.** It must compare against the current
   triple, bracket conditionally, store all three components, and cascade to attached
   followers **only when the coordinate changed**. If any locomotor module writes the entity
   coordinate directly, the bracket, the intra-cell suppression and the follower cascade
   become impossible to keep consistent.
4. **The position field sits at the generic-entity level**, not on a foot/unit struct — it is
   an `ObjectClass` field, and anims, bullets, particles and buildings use the same setter and
   the same slot.

Two derived requirements for whichever slice owns movement, recorded here so they are not
lost: intra-cell versus cell-crossing must be distinguished **at commit time** (§5.6.1 P8),
and the redraw fired from inside the bracket must become a dirty flag the render layer polls,
never a call outward from `sim/` (§5.6.1 P7).

**Parity status of all four: [UNCHECKED].** These are binary-read semantics with no
gamemd-derived executable check behind them; `emulate_function` provably cannot witness a
store-only function's result (§5.6.1). A Rust `set_position` test ships as a well-provenanced
ratchet citing `0x004DB810`, `0x005F6940`, `0x004D3780` and `0x005F6060` — not as parity.

**Shadow window.** `LocomotorSlot` may coexist with `LocomotorState.kind` for **at most one
session** while consumers migrate. It flips authoritative in the next session or the slice
is reverted.

**Rollback.** The slot is additive until the flip; the flip is one commit.

---

### S4 — One piggyback mechanism (retires R5)

**Scope.** `movement/locomotion/piggyback.rs`. Delete `PiggybackLocomotor` and
`OverrideLocomotor` and their two rival APIs; delete the dead `OverrideKind::Parachute`.
Rewire the five current callers (`movement_commands.rs:184`/`:208`,
`miner/miner_system.rs:1572`, `teleport_movement.rs:172`/`:365`,
`droppod_movement.rs:78`/`:198`, `world_commands.rs:314–315`).

**Scope grew by two live mechanisms, and did NOT shrink by one.** In addition to the chrono
vehicle path and the factory exit, S4 must carry (a) the **ChronoWarp superweapon** BEGIN
(§5.4 row 5) and (b) the **Magnetron warhead** BEGIN/END (rows 6/6b) — the latter is what
writes the `+0x2AC`/`+0x2B0` mutual link and `host[+0x6AD]`, which the whole `Is_Ok_To_End`
gate table hangs on. **Do not scope S4 on "infantry contribute zero piggyback traffic"** —
that claim rested on a gate that is not in `0x00521EB0`, and row 10 is
UNRESOLVED-REACHABILITY, not dead (§5.4 residual).

**What lands.** The ordered BEGIN and END protocols of §5.3, with: non-nesting enforced by
type; `Is_Ok_To_End` dominated by `!is_moving()`; the observable null window; the popped
locomotor destroyed **after** transport-entry processing; ownership transferred on `end`
with no AddRef/Release analogue; and `effective_class()` implementing the see-through
identity so harvest/scatter/set-destination ask through it.

Plus four gate-model requirements the closeout pass made non-optional:

1. **Per-kind gate state, not a shared `in_critical_section: bool`.** Teleport's gate is two
   fields, and `obj[+0x38]` is an **eight-valued phase index** (§5.3). Teleport never writes
   `+0x35`, so a single-bool model makes Teleport's gate constant-true and lets the
   piggyback unwind mid-warp — the exact failure the gate exists to prevent. Teleport
   carries a phase enum.
2. **Model the phases-3-to-7 window.** `host[+0x27C]` alone is not the warp gate; phase 2
   clears it while phases 3–7 still run.
3. **Jumpjet's host clause is an OR.** `(host[+0x6AD] == 0 || host[+0x6AE] != 0)`. Omitting
   it means a lifted unit can never be released.
4. **Clear `+0x428`/`+0x42C` on all three paths** — `End_Piggyback` *and* warp completion.
   Clearing in one place leaves a stale `HouseClass` credit that a later failed warp uses.

**Both BEGIN shapes must exist:** abandon-on-E_FAIL, and **end-first** (`0x00742608`,
`0x0044DFE0`), which two of the four live BEGIN sites use. `begin_refuses_to_nest` is
load-bearing here, not decorative.

**Acceptance test.** `movement::locomotion::piggyback::tests::end_order_matches_native` —
asserts the exact E5→E11 sequence via an ordered event log: current dropped **before** the
slot is cleared, slot observably empty before restore, restore before the `+0x6B4`
equivalent, destroy **after** transport entry.
Plus `piggyback::tests::begin_refuses_to_nest`, `piggyback::tests::begin_ends_first_on_the
_two_end_first_sites`, `piggyback::tests::teleport_gate_holds_through_phase_7`, and
`piggyback::tests::effective_class_sees_through_stash` (a Chrono Miner with Drive
piggybacked still reports Teleport — now independently motivated, because
`UnitClass::Set_Destination` branches on the **active** locomotor's `GetClassID` at
`0x007424E9`/`0x007425EB` while mission logic branches on the see-through identity, so both
must be distinguishable).
Plus a live-observe run: a Chrono Miner exits a war factory, drives out, and pops the Drive.

**S4 CANNOT claim parity on the END ordering, and the two named blockers being closed does
not change that.** oq6's `obj[+0x38]` and oq9's Chrono-Miner-inbound precondition are both
closed (§5.3, §5.4 row 3b) — S4 can now be written against the *correct* predicate, which
is real progress. But `end_order_matches_native` as specified is a **hand-transcribed
golden with a prose citation**, precisely the category AGENTS.md records as having produced
wrong references here before. The evidence that the risk is live is inside this very wave:
the lane whose entire brief was offset conversion, and which opens with a correct statement
of the conversion rule, mis-converted one instruction and published a wrong field semantic
and a wrong "next step" on the strength of it. **Ship `end_order_matches_native` as a
well-provenanced ratchet** — cite `0x004DAE5F`–`0x004DAF07`, `0x0044E10E`, `0x0065F174`,
`0x006CC98C` and `0x007102D8` in the test comment — and label it `UNCHECKED`, not
`VERIFIED`. **The route to a VERIFIED ordering claim is `emulate_function` over
`0x007192F0` / `0x004DAE5F` with a recorded machine-derived call trace**, not a larger pile
of correct prose. A world-hash comparison against the previous Rust is a ratchet and must
not be described as parity.

**The OQ4/OQ5 re-run does not change S4's parity status, and does not add scope.** Position
commit and occupancy are **not piggyback-sensitive**: `+0x1B4`, `+0x124`, `+0x1CC`, `+0xF0`
and `+0xF4` are all **host** virtuals dispatched on the host vtable, so an inner stashed
locomotor running under an outer one commits through exactly the same host slots with no
handoff, and swapping the stash cannot move the host or leave a half-written position. The
Hover finding does not disturb this — `0x005F6060` is host-side too, reached with the host as
`this` (`disassemble_bytes 0x005148E0 len 48`). Two consequences worth stating: a Rust
`PiggybackLocomotor` that owned or cached a position would be DRIFT, and S4's observable null
window is safe with respect to position, so nothing in the BEGIN/END protocol needs to
save or restore the host coordinate.

**One caveat the re-run does add.** The commit's occupancy gate reads the **installed**
locomotor's `In_Which_Layer` (§5.6.1 P6), so a piggyback swap changes which locomotor answers
that gate mid-commit. That is a read, not a handoff, and it does not change the END ordering
`end_order_matches_native` asserts — but a Rust model that resolves the layer from a cached
value rather than from the currently-installed locomotor would diverge exactly during a
piggyback. **[UNCHECKED]** — no pass has traced a stock trigger where the two locomotors
return different layers.

**Rollback.** Single revert; the two old mechanisms are deleted in the same commit that
adds the new one, so there is no half-migrated state to unwind.

---

### S5 — Power (retires R4)

**Scope.** `LocomotorSlot.powered`, default `true`; `power_on()` / `power_off()` /
`is_powered()`; the Hover-only observable effect; wiring the verified edges.

**What lands.** deploy-begin → off; undeploy-complete → on; undock and
release-docked-harvester → on; per-cell-process → off; **set-destination on an unpowered
unit → on** (the player-facing recovery edge). `hover_vertical_tick`'s `powered` parameter
stops being a hard-coded `true`.

**Explicitly does NOT land.** EMP-drives-power; Fly's `Power_Off` RNG draws; any
`Is_Ion_Sensitive`, `IonStorms` or `[SpecialFlags]` ion path; any Lightning-Storm →
locomotor-power coupling; and the `Power_On`/`Power_Off` → `Is_Powered` re-dispatch (a
structural artefact with no verified effect).

**Acceptance test.** `movement::locomotion::power::tests::move_order_repowers_locomotor`
and `::hover_unpowered_sinks`.

**Honest labelling.** The *edges* are VERIFIED from callsites with receivers confirmed as
`[reg+0x674]`. **How often a stock skirmish actually reaches the powered-off state is
UNCHECKED** — with EMP dead, the surviving producers are deploy-begin and per-cell-process,
and neither was traced to a frequency. Put that sentence in the module doc.

**Rollback.** Default the flag to `true` and stop writing it — behaviourally identical to
today.

---

### S6 — Retire the two invented gates (retires R1 and R3) — **LANDED `a093e9ee` 2026-07-29**

> **[R] What actually landed differs from this slice's description below.** Two corrections:
>
> 1. **R1's replacement was one deletion, not a rebuild.** This slice specified rebuilding
>    the `cur_dir × 9` straight-track behaviour "with the path queue **unmodified**" and
>    "node count preserved". Both phrasings are wrong. `build_sharp_turn_fallback` already
>    built the correct `cur_dir × 9` track; and gamemd **does** consume one node here — the
>    ordinary non-cell-crossing shift. Verified via `disassemble_bytes 0x004b4016 len 64`:
>    the substitute store at `0x004b4031` falls through to `0x004b4034`, converging with the
>    normal path (`JNZ` at `0x004b402c`) **before** the `flags & 8` test at `0x004b403a`, so
>    the `REP MOVSD` at `0x004b4607` is shared-tail behaviour. The bug was a *second*
>    increment on top of the ordinary advance. Fix: delete one line.
>    See §11 C20 and the correction banner added to
>    [docs/research/DRIVE_SHARP_TURN_FALLBACK_RE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_SHARP_TURN_FALLBACK_RE.md).
> 2. **The full octant path-node re-representation did NOT land** and is not required to
>    retire either gate. `TubePathStepResult::Blocked` was deleted outright, which makes the
>    abort unrepresentable by type — the stated goal — without changing how `MovementTarget`
>    stores nodes. Re-representing path nodes as octants remains open and would be a bulk
>    refactor needing its own approval.
>
> **Tests landed:** `movement::movement_tests::sharp_turn_preserves_path_node_count`,
> `tube_movement::tests::path_tube_step_on_portal_declines_foreign_node_without_aborting`,
> and `..._declines_zero_step_shell_state_without_aborting` (renamed from `..._blocks_...`).
> All **UNCHECKED** as parity. Full `--lib`: 5083 passed, 0 failed.
> **No golden shifted** — so the suite had no prior coverage of sharp-turn node accounting,
> which is how the bug survived two narrowing fixes (`6f6ec58e`, `4324d33b`).

**Scope.** `movement_step.rs:133–153` and `tube_movement.rs:177–235` +
`movement_tick.rs:1144–1147`. Replace both with the substrate's path-node contract:
direction octants plus the sentinel-8 tube entry, so non-adjacency is unrepresentable and
no abort path exists.

**What lands.** The `cur_dir × 9` straight-track-then-rotate-in-place behaviour with the
path queue **unmodified**; deletion of the `Blocked → finished_entities` kill.

**Acceptance test.**
`movement::locomotion::process::tests::sharp_turn_preserves_path_node_count` — a Drive unit
takes a ≥135° turn and the queue length before and after differs by exactly the nodes
consumed, never one extra. Plus a low-bridge scenario asserting the unit never lands in
`finished_entities` without reaching its goal.

**Why this is a real parity check.** The contract is the 2026-07-28 trace swarm's
instruction-level finding ("no native movement kill gate exists"; nodes are octants) —
cite the scan directory in the test comment. **The node-count assertion is derived from the
native contract, not from prior Rust output.** The low-bridge scenario is a ratchet.

**Rollback.** Both gates are small and localized; restoring them is a two-hunk revert. If
a stall reappears, that is a *finding* about the substrate contract, not a reason to
re-add the gate.

---

### S7 — Delete the dormant-TS Rust modules (retires R10) — **LANDED `a1554e3f` 2026-07-29**

> **[R] One deviation, and it constrains S1 and S3.** The slice said to delete
> `LocomotorKind::{Tunnel, DropPod}` "if nothing else references them". They are
> **retained as inert variants** instead. `world_hash` hashes the locomotor kind by raw
> discriminant (`(loco.kind as u8)`, `src/sim/world/world_hash.rs:767`), so deleting a
> variant renumbers every later one and shifts the replay baseline for **zero runtime
> benefit**. Confirmed empirically both ways: with the variants removed,
> `replay_hash_stable_through_slice6` failed its schema probe
> (left `3955274016651650540`, right `14099801084960151601`); restoring **only** the two
> variants, with both movement systems still deleted, returned it to green. That is also
> the proof the shift was pure enumeration churn and not behaviour.
>
> **Consequence for S1/S3 — plan for it rather than rediscovering it.** S1 specifies a
> `LocomotorClass` with 8 live variants, and S3 supersedes `LocomotorState.kind`. Whichever
> of those changes the hashed discriminant **will** shift the replay baseline, so it must
> own a coordinated re-baseline. Note `docs/scans/PENDING_REBASELINES.md` already lists
> this baseline as red from another session's in-flight deltas including an UNATTRIBUTED
> entry, so per AGENTS.md no re-baseline may be taken until the tree is clean of those.
>
> **Also corrected:** this slice claimed `world_hash` and snapshot "arms" for the two
> states. There were none — `tunnel_state` and `droppod_state` were never hashed. Deleting
> them was hash-neutral; only the enum discriminants were not.
>
> **Test landed:** `rules::locomotor_type::tests::dormant_clsids_absent_from_retail_inis`
> (not `substrate::locomotion::…`, since S1 has not landed). Golden is retail INI bytes —
> a real parity check on the dormancy claim. Widened beyond the slice: **zero occurrences
> across every file in `ini/`**, not just `rulesmd.ini`/`rules.ini`, though the test itself
> asserts only those two. Campaign and map INIs remain UNCHECKED.
> Full `--lib`: 5070 passed, 0 failed.

**Scope.** Remove `sim/movement/tunnel_movement.rs`, `sim/movement/droppod_movement.rs`,
their spine passes (`world/mod.rs:2201`, `:2223`), their `Option<XState>` fields
(`game_entity.rs:354`, `:367`), their `world_hash` and snapshot arms, and
`LocomotorKind::{Tunnel, DropPod}` if nothing else references them. Do **not** touch
`tube_movement.rs` — low-bridge `TubeClass` movement is active YR behaviour and must not
be conflated with subterranean.

**What lands.** Two fewer full-order scans per tick, and two fewer TS systems in the tree.

**Acceptance test.** `cargo test -p vera20k --lib` green (report the literal
`test result:` line), plus `substrate::locomotion::tests::dormant_clsids_absent_from_ini`
asserting `{4A582743-…}` and `{4A582745-…}` appear zero times in `ini/rulesmd.ini` and
`ini/rules.ini`.

**Why this is a real parity check.** The golden is retail INI bytes. If a future mod or
campaign INI reintroduces those GUIDs the test goes red, which is the correct signal —
note that only `rulesmd.ini`/`rules.ini` were checked; campaign and map INIs are
**UNCHECKED**.

**Rollback.** Git revert. The modules are self-contained.

---

### S8 — The render split — **PARTIAL: `9ecf1fbc` 2026-07-29; the main body is NOT started**

> **[R] S8 is far larger than this slice text implies — measured 2026-07-29.**
> `screen_x`/`screen_y` are cached `f32` fields on `Position` itself
> (`src/sim/components.rs:43-48`, `#[serde(skip)]`), not on a locomotor type. Live
> reference counts: **app ~183, sim 92, map 60, render 30, util 5 — roughly 370 sites
> across every layer.** Extracting them is a cross-layer bulk refactor, which AGENTS.md
> requires explicit approval for, and it delivers **no player-visible change**. It should
> be planned as its own multi-slice piece of work, not carried as one bullet here.
>
> **Landed (`9ecf1fbc`):** the dead `jumpjet_wobbles` `f32` — written at spawn, restored
> across override, read by nothing. Hash-neutral, suite green.
>
> **NOT landed, and why:**
> - **`screen_x`/`screen_y` extraction** — the ~370-site refactor above.
> - **`ALTITUDE_VISUAL_SCALE`** — defined identically three times
>   (`air_movement.rs:63`, `parachute_descent.rs:24`, `rocket_movement.rs:41`) and used
>   only to write `screen_y`. It cannot move before the `screen_y` writes move.
> - **`infantry_wobble_phase`** — the second `f32`, and it is **not** dead. It accumulates
>   in `movement_step.rs:1013` and is consumed at `movement_tick.rs:1693`
>   (`phase.cos() * INFANTRY_WOBBLE_AMPLITUDE` → `position.screen_y`). Moving it needs
>   `render::locomotor_visual` to own per-entity visual state and tick the phase itself,
>   which is coherent only once the other `screen_y` writes have moved. **Verified benign
>   in the meantime:** it writes `screen_y` only and touches no deterministic state, so it
>   is not a fixed-point violation despite being `f32` in a sim struct.
> - **The compile-time guard test** — would fail immediately against the 44 remaining
>   `screen_` write sites in `sim/`. It is the *exit* criterion for the real slice.
>
> **Also note:** `render::locomotor_visual` does not exist yet; `src/render/` has no
> locomotor module at all. Slots 8/9/10/11/12/14/15/34 are gamemd vtable slots that this
> engine never modelled as sim-held types, so "move them out of any sim-held type" has no
> direct referent — the actionable content of S8 is the four bullets above, not the slot
> list.

**Scope.** Move slots 8/9/10/11/12/14/15/34 out of any sim-held type into
`render::locomotor_visual`, reading sim state read-only. Split slot 21: tilt *state* stays
in `sim/` and is snapshotted; the matrix build moves to `render/`. Move `screen_x/screen_y`
writes and `ALTITUDE_VISUAL_SCALE` out of `sim/`, and remove the two `f32` fields from
`LocomotorState`.

**Acceptance test.** A compile-time guard: `sim/` contains no `screen_` writes and no
`f32`/`f64` in locomotion state (a `#![deny]`-style test or a grep test in CI). Plus the
existing render goldens.

**Honest labelling.** **This slice's boundary rests on what these slots *write* (a matrix
plus a render cache key, no sim state), not on the refuted wall-clock argument.** Whether
`linked_object + 0x388` holds ticks or wall-clock is **UNCHECKED**; decode it before
claiming anything stronger.

**Rollback.** Mechanical revert.

**Ordering note.** S8 is independent of S1–S7 and can be deferred; S1 → S2 → S3 → S4 is the
dependency chain that matters, and **S1 is the recommended first slice** because it is
purely additive, has a machine-derived golden, and unblocks everything else.

---

## 9. Open questions — what remains unknown

Generous by design. False completeness is worse than an admitted gap.

**Honest count after the 2026-07-29 closeout plus the OQ4/OQ5 re-run: 11 of the original 26
binary-side questions closed, 1 half-closed, 14 still open, 6 new ones opened, and one new
residual recorded in §10.** The list below is *longer* than the version it replaces, not
shorter — because every surviving question now carries what was searched and ruled out,
which is the point. Fewer questions, more words.

**Closed by the 2026-07-29 closeout pass** (answers moved into the body; listed here only
so nobody re-opens them by accident, and so the count is honest):

| Was | Now | Lives in |
|---|---|---|
| 1. Save/load re-acquisition | **CLOSED** — `OleSaveToStream`/`OleLoadFromStream`, CLSID on the wire, stash recurses | §1.6, §5.7 |
| 2. What a Chrono Legionnaire does on a move order | **CLOSED** — nothing swaps; `[CLEG]` is *born* with Teleport | §5.4 |
| 3. `TechnoTypeClass+0x390` | **CLOSED** — it is `HoverAttack=`; `Teleporter=` is `+0xCD4` | §2.9 |
| 4. Which host virtual commits the position | **CLOSED by a 2026-07-29 re-run** — `+0x1B4` = `0x004DB810` for the full triple, **plus a second height-only mechanism** that bypasses it; `+0x484` eliminated (it is `OnArrival`) | §5.6.1, §5.6.3 |
| 5. Which of `+0xF0`/`+0xF4` is mark and which is unmark | **CLOSED by the same re-run** — `+0xF0` = MARK, `+0xF4` = UNMARK; the occupancy reading is **restored**, the getter/setter hypothesis refuted | §5.6.2, §11b C21 |
| 6. `Is_Ok_To_End`'s per-class gates | **CLOSED** — all four classes, all six fields, including `obj[+0x38]` = the 0–7 warp phase | §5.3 |
| 7. `FootClass[+0x428]`/`[+0x42C]` | **CLOSED** — `(BuildingClass*, HouseClass*)`, three clear paths, four readers | §5.3 |
| 9. `SuperClass::Launch` / "`PerformDeploy`" gating | **CLOSED** — case 4 `Type=ChronoWarp`; `0x00710000` is the Magnetron warhead installer | §5.4 rows 5, 6 |
| 10. The virtual at `0x004DF7F0` | **CLOSED** — host slot `+0x508`, map-trigger only, zero in skirmish | §2.9 |
| 11. Sites `0x00521F82`/`0x0052200A`/`0x00523107` — *identity* | **CLOSED** — two undefined functions: InfantryClass `+0x508` override and InfantryClass `+0x4F8` Jumpjet→Walk installer. **Their reachability is NOT closed — see (11) below** | §5.4 rows 8, 10 |
| 15. Walk / Jumpjet stash offsets | **CLOSED** — Walk `+0x38`, Jumpjet `+0x94`, both from method bodies | §2.7 |

**OQ4 and OQ5 were closed by a re-run, not by the original wave.** The `oq4-position-commit`
lane the closeout wave commissioned **died mid-run and produced nothing**; a replacement pass
on 2026-07-29 ran two independent lanes plus an adversarial cross-check and closed both. The
adversarial pass overturned material in the writer-sweep lane (§11b C21, §11 C8), so the
answers in §5.6 are the adversarial verdict, not the lane consensus. **Neither closure is a
parity claim** — see "Parity status" in §5.6.1.

What the re-run searched and ruled out, so nobody repeats it: `+0x480` (it is
`Set_Destination`, §2.9); `+0x484` (an arrival hook with a once-per-tick `+0x6B3` latch —
three overrides decompiled in full, none writes `+0x9C`); the commit living inside any
`Process` body (Drive's and Teleport's read the triple and never write it); `+0xF0`/`+0xF4` as
a coordinate getter/setter (six bodies decompiled — none returns a value, none writes an
object field); `+0xF0`/`+0xF4` as two unrelated neighbours (overridden in lockstep, exact
set/clear mirrors, matched `AddContent`/`RemoveContent` callers); `cell[+0x124]` as a redraw
mask (its only callers are the content add/remove pair); and the multi-cell branch of
`0x004D3780` as building-only (the gate is the locomotor's `In_Which_Layer`, and Drive returns
the `2` the gate wants). **What is still not ruled out:** register-computed indirect calls —
§10.

**One of the eight contract-critical questions remains open, and it is the narrowed one.**

**Contract-critical (block a slice until answered):**

8. **`linked_object + 0x388` — ticks or wall-clock? NARROWED, still open.** Ruled in:
   `0x004C93D0` and `0x004C9480` are `RateTimer__Current` and `CDTimerClass__Remaining`
   (`batch_decompile 0x004C93D0,0x004C9480`), both computing `g_CurrentFrameCounter − start`
   with `start == -1` meaning "not running"; `g_CurrentFrameCounter` is `0x00A8ED84`, written
   from `Main_Tick 0x0055DE81` / `Main_Game 0x0052DA08` / `Read_Scenario 0x0068466B` and read
   from ~95 sim sites. **So anything routed through either helper is TICKS, not wall-clock.**
   What is still missing is the one link that matters: **`+0x388` was never tied to either
   helper.** Until it is, the render/sim cut on slots 9/10 stays deferred (§10).

**Reachability gaps (do not ship until answered):**

11. **Is the infantry Jumpjet→Walk landing path (`0x00521EB0`) reachable in stock YR?
    STILL OPEN, both directions.** Ruled out this pass: the argument that it is dead because
    it requires `HoverAttack == 0`. `disassemble_bytes 0x00521EB0 len 210` (67 instructions,
    untruncated, whole gate chain) **reads `+0x390` nowhere** — that gate belongs to
    `0x0051AFC8`, a different function. `[JUMPJET]` Rocketeer passes every gate the function
    actually has. A stronger but undischarged deadness argument exists:
    `search_instructions CALL "+ 0x4f8]"` returns **4 sites program-wide** (1,152,197
    scanned, untruncated) and the only two that can reach a FootClass are both inside
    `WalkLocomotionClass::ProcessMovement`, which `get_xrefs_to 0x0075aec0` shows has **zero
    vtable references** — so the sole dispatcher needs Walk active while the gate needs
    Jumpjet active. **Not yet traced:** the piggyback delegation direction — whether an outer
    locomotor ever runs a stashed Walk's `Process` while `+0x674` still reads Jumpjet. That
    single question decides it.
12. `Tilt_Pitch_AI` (slot 21) — base is a no-op; no pass has enumerated the overrides or tied
    the slot to the visible slope tilt. Untouched by the closeout.
13. Whether any campaign or map INI reintroduces the Tunnel / DropPod / Mech CLSIDs. Only
    `rulesmd.ini` and `rules.ini` were checked. Untouched.
14. Which stock YR weapon, if any, constructs an `EMPulseClass`. With the constructor
    unreferenced the answer is probably "none", but the negative is a search result, not a
    proof. Untouched.
16. Whether `TechnoClass::AI` consults `Is_Moving_Now` for cloak decay. `0x006F9E50` has
    still never been decompiled. If it does, it necessarily reads a **pre-Process** value,
    because `TechnoClass::AI` runs first (§5.2 T1). `ILOCOMOTION_COM_PROTOCOL_SPEC.md`
    asserts the cloak link in two places with no evidence — see §11 C3. Untouched.
16b. **Is the §5.4 trigger table complete? NO — and nothing establishes how far off it is.**
    Three untruncated sweeps found 51 `+0x674` sites across three addressing idioms, with
    **eleven touched functions absent from every lane report**:
    `BuildingClass::MissionRepairAndProduce 0x0044C714`,
    `FootClass::Greatest_Threat_Scan 0x004D586A`, `FootClass::OnArrival 0x004D8308`,
    `FootClass::Mission_Enter 0x004D9310`, `InfantryClass::Fire_At_Target 0x005207A9`,
    `UnitClass::Scatter 0x00743A70`, `UnitClass::Mission_Harvest 0x0073E799`,
    `FUN_00518D80 0x00518E3F`, `0x005190F1`, `0x0051CA0D`, `0x00521F00` — plus a live store
    at **`0x004DB95C`** inside the address range §5.4 row 12 describes as zero-reference, and
    four `TechnoClass::Set_Destination` sites (`0x00741EB5`, `0x00742414`, `0x007427D5`,
    `0x007429DA`) of which one was analysed. The three sweeps are complete *for
    MOV-with-displacement, LEA, and ADD-immediate*; they do not prove no fourth idiom exists.
16c. **Which trigger-action opcode reaches `FUN_006E1A40`** (and thence the `+0x508`
    map-trigger teleport). Not attempted — assessed low value, since the whole path is zero
    in skirmish. Recorded so the next reader knows it was skipped deliberately.
16d. **Which code consumes `SuperWeaponTypeClass`'s parsed `PreDependent=` to grant or launch
    `ChronoWarpSpecial` alongside `ChronoSphereSpecial`.** The *pairing* is closed from data
    plus parser — `[ChronoWarpSpecial] PostClick=yes / PreDependent=ChronoSphere`, and
    `0x006CEC90` parses `PreDependent=` into the same 12-entry `Type=` name table
    `0x008425C0` that §5.4 row 5 uses. The **dispatch hop is [I]**, not traced.
16e. **`[HIND] TechLevel=-1`**, on which §5.4 row 11's zero-frequency verdict rests, was
    inherited from an earlier pass and re-verified by nobody in this wave. Cheap to close.

**Field and slot meanings still UNKNOWN:**

17. Why `Link_To_Object` stores the owner twice (`+0x08` and `+0x0C`). **Half-closed:** a
    reader of `+0x08` now exists — `LocomotionClass::Load 0x0055AB2C` submits it to the
    swizzle registrar on every load, exactly as it does `+0x0C` at `0x0055AB10` (§5.7 S6).
    *Why* two copies is still UNKNOWN.
18. `DAT_00abcd3c` — incremented in AddRef, decremented in Release; no reader located.
    "Diagnostic counter" is a guess.
19. Object `+0x11`'s role beyond the IPersistStream dirty flag (the polarity proof is
    solid; whether anything else reads it is not).
20. Host vtable slots `+0x54`, `+0x1C8`, `+0x1D0`, `+0x1D4`, `+0x1D8`, `+0x1E8`, `+0x1F0`,
    `+0x280`, `+0x3D0`, `+0x488`, `+0x48C`, `+0x538`, `+0x544`, and object-vtable `+0x1AC`.
    Observed call sites and argument counts only. **Three were closed by the closeout wave and
    have moved to §2.9:** `+0x160` = `TechnoClass::IsIronCurtainActive`
    (`read_memory 0x007E8DF4 len 4` → `0x0041BF40`) — the shared gate in all three
    warp/lift chains; `+0x480` = `Set_Destination`; `+0x508` = the `Teleport_To` map-trigger
    virtual. **Six more were closed by the OQ4/OQ5 re-run and live in §5.6:** `+0x1B4` = the
    full-coordinate commit, `+0x124` = `Mark(MarkType)`, `+0x1CC` =
    `FootClass::Set_Height_On_Bridge` (the height-only commit), `+0xF0`/`+0xF4` = occupancy
    mark/unmark, `+0x484` = `OnArrival`, and `+0x78` = a **forwarder to the installed
    locomotor's** `In_Which_Layer` (`read_memory 0x007F5CE8`, `0x007EB0D0` → `0x004DB7E0`;
    `decompile_function 0x004DB7E0`). `+0x54`, `+0x1D4` and `+0x1D8` appear together as gate
    1/4/5 in **two** independent copies of the same warp gate chain (`0x004DF7F0` and
    `SuperClass::Launch` case 4), which makes them a high-value, low-cost next target.
    `vt[0x134]` (the redraw fired from inside the `+0x124` bracket, §5.6.1 P7) and
    `vt[0x84]`/`vt[0x38]` are **newly observed and unidentified** — added to this list.
20b. **`RulesClass + 0xBF4` / `+0xBF8` / `+0xBFC` / `+0xC00`** — the quartet feeding the
    Teleport warp-delay timer at object `+0x3C` (a three-dword `CDTimerClass` block inlined
    into `0x007192F0`). None of the four was traced to a `RulesClass::ReadINI` string. Their
    shape suggests a distance factor, an enable, a minimum and a threshold. **Do not name INI
    keys on this.**
20c. **Teleport `object[+0x35]`.** Its *effect* is closed (the conjunct is satisfied at
    construction and Teleport never writes it — three untruncated writer sweeps), so nothing
    depends on it. Its *semantic* is inferred from the constructor layout Teleport shares
    with Walk. Low value; recorded so it is not re-investigated as if it mattered.
21. `TechnoTypeClass+0xD27` (read by Fly's `Is_Ion_Sensitive`) and `BuildingTypeClass+0x16BD`
    (read by Hover's).
22. `RulesClass+0x94` and `TypeClass+0xEC`, which gate the locomotor-spawned dust anim.
    Neither was traced to a `ReadINI` site, so the anim's active-by-default status is
    UNKNOWN.
23. Why Jumpjet needs its own AddRef/Release. Its QI body at `0x0054DC60` was never
    decompiled (no function defined there).
24. The `0x00812004` `.data` function-pointer array's producer and consumer, and its exact
    end address.
25. The 65 non-locomotor COM classes registered in the same WinMain block.
26. Slot names for 8, 11, 12, 14, 15, 21, 34, 35, 36, 37, 38, 39 — still doc-derived, with
    36 carrying an unresolved conflict between `Get_Status` and Ghidra's `Is_On_Floor`.

**Rust-side gaps this study did not close:**

27. Per-locomotor test coverage — `movement_tests.rs` (108 KB) and `drive_track_tests.rs`
    (35 KB) were not read.
28. `drive_track.rs` (3,937 lines), `homing_movement.rs`, `teleport_movement.rs`,
    `parachute_descent.rs`, `rocket_movement.rs`, `group_destination.rs`,
    `movement_bridge.rs`, `movement_path.rs`, `movement_commands.rs` and most of
    `pathfinding/` were keyword-swept, not read in full. A full read would likely add more
    R12-class items. **This study certifies nothing about them.**
29. No `cargo` command was run in this session; nothing here is a build claim.

---

## 10. Where the evidence was insufficient to make a recommendation

Stated plainly, so no one mistakes silence for a decision.

- **The render/sim cut for slots 9 and 10 — still deferred, but the gap is now one hop
  wide.** The lane's original justification was refuted and the replacement justification
  (what they *write*) is weaker than a boundary decision deserves. The closeout pass
  established that the two timer helpers behind this question, `RateTimer__Current`
  `0x004C93D0` and `CDTimerClass__Remaining` `0x004C9480`, are both
  `g_CurrentFrameCounter − start` — i.e. **ticks, not wall-clock** — so the original fear is
  almost certainly unfounded. What it did **not** do is tie `linked_object + 0x388` to either
  helper. **Recommendation stays deferred on that one link.** Keep the slots out of `sim/`;
  do not write "verified render-only" anywhere.
- **The delivery priority of the whole power path.** With EMP dead, the surviving
  `Power_Off` producers are deploy-begin and per-cell-process, and no lane established how
  often a stock skirmish reaches the powered-off state. The *parity fact* (the flag exists,
  defaults true, and is consumed) is solid; the *priority* is not. S5 is sequenced late for
  that reason, not because the mechanism is doubtful.
- **RESOLVED — what replaces the infantry chrono-move behaviour.** This entry existed
  because §9 finding (2) removed a story without supplying a replacement. The replacement
  is now in §5.4: *nothing swaps*, the Chrono Legionnaire is constructed with the Teleport
  locomotor and moves on it. Evidence is retail INI bytes plus the constructor disassembly.
  Port that.
- **Whether the infantry Jumpjet→Walk landing path is live — genuinely undecidable on
  current evidence.** Two arguments point opposite ways and neither is discharged (§9 item
  11). This is not a priority call being deferred; it is an honest "insufficient evidence".
  **Record it as UNRESOLVED-REACHABILITY, implement nothing, and do not let S4's scope rest
  on either answer.**
- **Whether S3 should keep a raw-replace path at all.** After this pass the only RAW swap
  left in stock YR is Carryall pickup, whose two stores exist (`0x00416BA1`, `0x00416BE8`)
  but whose reachability rests on an unre-verified INI fact. Keeping it costs a code path
  with no live trigger; dropping it costs the ability to model a mechanism gamemd has. **No
  recommendation** — decide when `[HIND]`'s `TechLevel` is re-checked (§9 item 16e), which
  is a one-minute job.
- **Whether `Force_Track`'s `0x42` argument is a track index, a facing, or something else.**
  The callsite and signature are verified (`Force_Track(0x42, Coord3D)` at `0x0044E160`,
  slot 28); the argument's meaning is not. The existing deferred note
  (`project_force_track_bib_step`) already flags the related sub-cell bib step as needing a
  Ghidra verify before wiring.
- **NEW — how to obtain a parity check on the position committer at all.** The OQ4 re-run
  closed the *semantics* of the commit (§5.6.1) and then hit a tooling wall: the committer is
  a store-only function, and `emulate_function 0x005F6940` returns **registers only**. It
  confirmed `ECX = this + 0x9C` from a seeded `ECX = 0x01000000`, but two different
  memory-seeding formats both failed to land, so the three stores are unobservable and the
  result cannot be witnessed. AGENTS.md's route to `VERIFIED` — a gamemd-derived executable
  check — **has no known instrument here**. Live capture or a debugger watchpoint on
  `+0x9C` would supply one; neither was attempted. Until then any Rust `set_position` test is
  a well-provenanced ratchet, and no pass should label it otherwise. **No recommendation on
  which instrument to use.**
- **NEW — register-computed indirect calls are outside every sweep ever run here.** All
  `+0x1B4` / `+0xF0` / `+0xF4` / `+0x508` / `+0x4F8` / `+0x68` / `+0x6C` call enumerations in
  this document cover only the `CALL dword ptr [reg + disp32]` idiom. A two-step
  `MOV EAX,[reg+0x1b4]` … `CALL EAX` would evade all of them, in all three OQ4 reports
  including the adversarial one, and in the §11 C11 Push/Shove sweeps. Residual risk is low
  for the commit question — 94 sites already cover every live locomotor — and **not** low for
  any negative claim that rests on a small site count. **Recommendation: stop writing
  "program-wide, untruncated" without naming the idiom the sweep covers.** Whether it is worth
  building a sweep for the register-computed form is undecided; nobody has estimated how
  common the idiom is in this binary.
- **The `sim/movement` → `sim/world` cycle.** Real and worth fixing, but this study did not
  determine whether the fix is moving `SimSoundEvent`/`EnterOrderCounter` down or moving the
  callers up. No recommendation.
- **Whether any Rust `Push`/`Shove`-shaped displacement is a bug.** `bump_crush::scatter_blocker`
  is live and does something reasonable; gamemd's Push/Shove is unreachable, so the Rust
  path is VERA-internal invention. Whether it should be *removed* or merely *relabelled*
  depends on what gamemd does instead in the same situation (the blocked-code state machine
  plus `CellClass::Scatter_Objects`), which was mapped but not compared against the Rust.
  **Relabel now; decide removal later.**

---

## 11. Corrections to existing docs

Every place this study contradicts a doc in `docs/research/`. The main target is
[ILOCOMOTION_COM_PROTOCOL_SPEC.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md).

### C1 — Not a contradiction; two different things ([ILOCOMOTION_COM_PROTOCOL_SPEC.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md) §3, slots 0/1/2)

The doc lists base fallbacks `0x0055A9B0`/`0x0055A950`/`0x0055A970` for QI/AddRef/Release.
Those are the **real implementations**. The base ILocomotion vtable *slots* hold `-4`
adjustor thunks at `0x004D0510`/`0x0520`/`0x0530` (`read_memory 0x004D0510 len 48`). Both
address sets are correct. The doc should say so explicitly, because reading either alone
makes the other look wrong. The doc's §1 already describes the thunk pattern — the §3 table
should cross-reference it.

### C2 — §2's "IUnknown vtable" column is mislabelled; the vtable at `0x007EAEC0` is **IPersistStream, 10 slots**

`read_memory 0x007EAEC0 len 64` gives `0x0055A9B0, 0x0055A950, 0x0055A970, 0x004C9150,
0x004B4C30, 0x004C9150, 0x0055AA60, 0x0055AB40, 0x005172F0, 0x004C9150` — the
IPersistStream layout (QI/AddRef/Release/GetClassID/IsDirty/Load/Save/GetSizeMax) plus two
class virtuals (the deleting destructor and a `Size_Of`).

This is load-bearing, not cosmetic: **slot 3 (+0x0C) is `GetClassID`**, which is the slot
`FootClass::Set_Destination_Internal` calls at `0x004D95B0`, the slot
`Piggybacker_CLSID` calls, and the slot `UnitClass::Mission_Harvest` calls at `0x0073E815`.
The doc's §1 layout diagram (`+0x00 IUnknown vtable`) and its per-class "IUnknown vtable"
column should read **IPersistStream**. Confirmed from the other direction: `QueryInterface`
`0x0055A9B0` accepts `IID_IPersistStream` and `IID_IPersist` and returns object+0x00 for
both, while returning object+**0x04** for `IID_IUnknown`.

### C3 — §5 and §5.2's cloak claims are unsupported

The doc states in three places that `Is_Moving_Now` gates cloak-break / cloak-reapply /
`cloak_decay` in `FootClass::AI`. **All four `Is_Moving_Now` call sites in `FootClass::AI`
are accounted for and none touches a cloak field** (`disassemble_function 0x004DA530`):
`0x004DA692` gates the shroud/sight reveal block (which ends in a map call with a ±3 cell
window and caches the sight value); `0x004DA8BB`, `0x004DA96D` and `0x004DAA24` gate the
movement counter and the idle-sound state machine. The doc's own §5 row already hedges
("which one gates cloak-reapply specifically is UNVERIFIED"); the §5.2 diagram line
`cloak_decay -= 1 IF NOT locomotor->Is_Moving_Now()` should be removed or marked
UNVERIFIED. If cloak does consult it, it does so from `TechnoClass::AI`, which runs
**before** any locomotor call and therefore reads a pre-Process value.

### C4 — §5's `LocomotionClass__QueryInterface_IPiggyback` at `0x0045AEA0` queries **IID_IPersist**, not IID_IPiggyback

`decompile_function 0x0045AEA0` shows the IID argument is `0x00818858` =
`{0000010C-0000-0000-C000-000000000046}` (`read_memory 0x00818858 len 16`). The real
IPiggyback getter is the **unlabelled** `FUN_0045af20` at `0x0045AF20`, whose IID is
`0x00819088` = `{92FEA800-A184-11D1-B70A-00A024DDAFD1}`. The Ghidra label is wrong and it
**inverts the meaning of every callsite that uses it** — including the CLSID lookups in the
war-factory exit and inside `Drive::Piggybacker_CLSID`. The doc's §5 row
("Checks whether the current locomotor supports IPiggyback") describes the wrong function:
that preamble call is the **CLSID lookup**.

### C5 — §7's destructor data-reference is an address inside the function, not its entry

The doc cites "From `0x0055A6F6` in `LocomotionClass__Destructor` [DATA]". The destructor
**starts at `0x0055A6F0`**; `0x0055A6F6` is its *second* instruction, the one restoring the
ILocomotion vptr at object+0x04. Verified by disassembling the gap between the
constructor's end (`0x0055A6E2`) and `Link_To_Object` (`0x0055A710`):
`read_memory 0x0055A6E3 len 48` → 13 NOPs then
`c701 c0ae7e00 / c74104 f4ad7e00 / c7410c 00000000 / c3`. The destructor releases nothing,
frees nothing, and does not clear object+0x08 — it restores both vptrs and nulls +0x0C.

### C6 — §5's save/load implications: `UnitClass::Load` does **not** reinstall the locomotor

Not a claim in the spec doc itself, but a correction to `host-contract.md` that any future
doc must not repeat. `FootClass::Load` (`0x004DB3C0`) runs at `0x007444FE`, **before**
`MOV EDI,[ESI+0x674]` at `0x0074450B`, and nulls `+0x674` at `0x004DB3DF`. So `EDI` reads
NULL, `CMP EBP,EDI` is equal, and the guarded store at `0x00744548` is a no-op. Also
`0x004D3540` is **`FootClass::Constructor`** (it writes `param_1[0x19D] = 0`), not "a
helper with an out-buffer". The "surrogate token in +0x674" reading is unsupported — the
vtables rewritten at `0x00744521`–`0x00744534` are `UnitClass`'s own. **`+0x674` is NULL
after a load and the re-acquisition path is UNKNOWN.**

### C7 — [INDEX_PATHFINDING_LOCOMOTION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/INDEX_PATHFINDING_LOCOMOTION.md) §6: `0x812D50` / `0x814A58` are not init-dispatch tables

The doc lists them as unresearched per-class dispatch tables holding
`Compute_*HeightStep` / `Compute_*BridgeZOffset` / `InitNullCoords`. **All of that is
wrong.** They are interior offsets into one large `.data` function-pointer array
(`list_segments`: `.rdata 007e1000–00811fff`, `.data 00812000–00b79be3`; every real vtable
in this binary is in `.rdata`). `get_xrefs_to` returns "No references found" at six sampled
points (`0x00812004`, `0x00812D30`, `0x00812D50`, `0x00812D70`, `0x00814A58`,
`0x00815000`). The array is address-ascending program-wide, so the "Drive" and "Ship"
attributions are positional coincidence in a sorted array. It demonstrably **excludes**
functions that *are* in the Drive vtable (`0x004AFB80`, `0x004AFC90` fall in a gap the
array skips) and includes ones that are not. **Recommended action: delete the two entries
rather than relabel them.** There is no cross-class init-dispatch mechanism —
initialisation happens entirely in the constructors reached via
`IClassFactory::CreateInstance`, which set their vtables inline.

### C8 — `host-contract.md` §3.6's "the locomotor never writes `FootClass+0x9C`" — RESOLVED 2026-07-29: **[R] REFUTED in effect**, true only in a narrow literal form

**Original correction (2026-07-29 closeout), which stands as far as it goes.** The claim was
marked VERIFIED on the strength of
`search_instructions mnemonic=mov operand_pattern="0x9c],"`, a pattern that only matches a
direct displacement store. **The binary's own coordinate-copy idiom in this very class family
is a computed-pointer store** — base slot 6 at `0x0055ACA0` does
`ADD ECX,0x9C; MOV EAX,ECX; MOV [EDI],…`, which the search cannot see. The scan also reported
837,300 instructions where every program-scope scan since has reported 1,152,197.

**Resolution by the OQ4 re-run.** The exhaustiveness gap was closed, and the claim splits:

- **The narrow negative survives. [V]** No locomotor function body contains a store
  instruction targeting `+0x9C`/`+0xA0`/`+0xA4`. Three program-wide sweeps agree: tool-side
  displacement scans (147 + 181 hits), a script-side displacement sweep
  (`run_script_inline`, `directDispWrites=213, inLocomotors=0`, 1,152,197 instructions), and
  an alias-tracked computed-pointer sweep (`ptrSites=661, locoSites=114, writes=39,
  locoWrites=3`, all three locomotor hits being `LEA`-clobber false positives).
- **The `0x0055ACA0` idiom C8 cited is a *reader*, not a writer. [V]** `read_memory
  0x0055ACA0` decodes to: load the linked object, `ADD ECX,0x9C`, read the three dwords, copy
  them **out** to the caller's buffer, `RET 8`. Drive carries a byte-identical private copy at
  `0x004AFD0E` (`read_memory 0x004AFD0E`) — which is also why Ghidra's
  `DriveLocomotionClass__Head_To_Coord` label there is drift (§11 C13). So C8's specific
  mechanism worry was unfounded; a different one replaced it.
- **[R] The claim that matters for the port is false.** *"The locomotor only ever moves the
  host through `+0x1B4`"* is refuted: `HoverLocomotionClass::Move` calls
  **`0x005F6060` directly and non-virtually** with the host in `ECX`, writing `host+0xA4`
  (`get_xrefs_to 0x005F6060` → five `UNCONDITIONAL_CALL` refs including `0x00514901` and
  `0x00514A16`; `disassemble_bytes 0x005148E0 len 48`). This is ACTIVE-YR — Hover is on
  `[LCRF]`, `[ROBO]`, `[SAPC]`, `[YHVR]`. Full detail in §5.6.1.

**Correct form of the claim, and the one to carry into `host-contract.md`:** *the locomotor
never writes the host coordinate through a store in its own body; it commits X/Y through host
virtual `+0x1B4`, and at least one live locomotor (Hover) additionally commits Z through a
direct non-virtual call to a second host-side setter.* The status is **[V] as a binary fact,
[UNCHECKED] as parity** — no executable gamemd-derived check exists on either committer
(§5.6.1). The residual that remains genuinely open is register-computed indirect calls, which
no sweep in any report covers (§10).

### C9 — `push-shove.md`'s `+0x1AC = Can_Enter_Cell` is unproven, and conflates two slots

Two callsites of the same slot prove it is the same slot, not what it is named; no body was
decompiled and no INI or string anchor was tied to it. Separately, `+0x1AC` is a slot on the
**linked object's** vtable, whereas the anchor table's `Can_Enter_Cell` is **ILocomotion
slot 7 at +0x1C**, whose base body `0x0055ABF0` returns 0 and is inherited by all 8 live
locomotors. The report uses one name for both without saying so. **The 0–7 return-code state
machine at `0x004B34C0` is real and useful and should be kept; the *name* for `+0x1AC` is
UNPROVEN.**

### C10 — `push-shove.md`'s INI census and Hover factory vtable are wrong

Census: the report claims 156 `Locomotor=` assignments with Drive at 62, and its own rows
sum to 165. Actual: `grep -c '^Locomotor=' ini/rulesmd.ini` = **155**, Drive = **52**;
`factory-clsid.md` §8 reproduces exactly. The overcount is consistent with counting the
commented-out alternate CLSIDs on lines like
`Locomotor={4A582741-…};<-drive   mech->{55D141B8-…}`. **Do not quote push-shove's census.**

Factory vtable: Hover's is **`0x007F3CA8`** (`read_memory 0x007F3CA8 len 48` → slot 3 =
`0x006C4310`, exactly the `CreateInstance` push-shove names), not `0x007F3CC0`, which is
Rocket's.

### C11 — [LOCOMOTION_PUSH_SHOVE_CALLER_PROVENANCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOCOMOTION_PUSH_SHOVE_CALLER_PROVENANCE_GHIDRA_REPORT.md) (2026-05-14): counts wrong, hedge should retire

The doc's core findings all re-verify — override matrix, base-stub disassembly, the
`Shove → Push` callsite at `0x00516FCD`, the data-only xrefs, the false-positive families.
Three corrections: (a) raw call counts of 180 / 64 for `CALL [reg+0x68]` / `[reg+0x6C]` are
wrong; instruction-aligned live counts are **47** and **62** (the 180 came from a raw
`.text` byte scan counting `ff 5x 68` sequences off instruction boundaries, a ~4×
overcount); (b) the "medium confidence, a runtime breakpoint may yet find a caller" hedge
should retire — `Shove` has **zero** references of any kind beyond its own vtable slot, and
`JMP [reg+0x68]` = 0 hits, `JMP [reg+0x6c]` = 1 hit (an unrelated thiscall thunk on global
`0x00A8B238`); the correct status is **unreachable**; (c) its "Rust Parity Guidance"
justification for preserving `Can_Enter_Cell(target, -1, -1, 0, 1)` on account of the Hover
Push branch should be dropped — that branch cannot execute. The Jumpjet landing/abort path
cited in the same section is the live justification and should carry it alone.

Note the asymmetry the doc should record: **the `Shove` half of the verdict is stronger
than the `Push` half.** The sweep covers the direct `CALL [reg+off]` form; a two-step
`MOV reg,[vtbl+0x68]; CALL reg` would evade it. That residual cannot rescue `Shove`.

### C12 — [MAGNETRON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAGNETRON_SYSTEM_GHIDRA_REPORT.md): the IPiggyback anchor is not corroborated

The complete IPiggyback enumeration (5 direct QI sites via `get_xrefs_to 0x00819088`, 18
helper sites via `search_instructions CALL 0x0045af20`, 13 creation sites via
`get_xrefs_to 0x0041C250`) contains **no magnetron site**. Any claim in that doc that the
magnetron lift goes through IPiggyback should be re-checked before use. What *is* verified
is the data path: `[TELE]` → `Primary=MagneticBeam` → `Warhead=LocomotorBeam` →
`IsLocomotor=yes` + the Jumpjet CLSID — i.e. a **raw** locomotor replacement, not a
piggyback. This is the best-sourced live example of raw replacement in stock YR.

### C13 — Ghidra label drift recorded (read-only passes; nothing was written back)

Per AGENTS.md these should be written into Ghidra by whoever next holds the write lock,
with the evidence citation in the plate comment. Inferred items stay unlabelled.

| Address | Current label | Reality | Evidence |
|---|---|---|---|
| `0x0045AEA0` | `LocomotionClass__QueryInterface_IPiggyback` | queries **IID_IPersist** | `decompile_function 0x0045AEA0` |
| `0x0045AF20` | *(unlabelled)* | the real IPiggyback QI helper | `decompile_function 0x0045AF20` |
| `0x007E9A50` | `CLSID_HoverLocomotion` | `{4A582743-…}` = **Tunnel**; Hover is `{4A582742-…}` at `0x007E9A40` | `read_memory 0x007E9A50 len 16` |
| `0x004B4C60/70/80` | `DriveLocomotionClass__Get_Status` / `__Acquire_Hunter_Seeker_Target` / `__Is_Surfacing` | **base/shared** stubs in 11–12 vtables; Fly is the sole overrider of 36 and 37, Tunnel of 38 | `get_bulk_xrefs 0x004B4C60,0x004B4C70,0x004B4C80` |
| `0x004CFD90` | `FlyLocomotionClass__Move_To` | occupies Fly's `+0x60` (**Is_Powered**); body is a tail call to the base getter. Move_To is slot 17 (`+0x44`) | `read_memory 0x007E8A4C len 16` |
| `0x004CF830` | `FlyLocomotionClass__Stop_Moving` | installed at Fly **slot 11**; writes a 2-dword out-param `(0, sin-based bob)` — a Shadow_Point | `read_memory 0x007E89F4 len 160` |
| `0x004E1570` | `LocomotionClass__ForEach_SetSlopeIndex` | a `thiscall` linked-list walk calling `[vtbl+0x6c]` then `[vtbl+0x4]`; nothing to do with LocomotionClass. True owner UNKNOWN | `disassemble_function 0x004E1570` |
| `0x0070FEE0` | `BuildingClass__DeployUnit_ChronoWarp` | called at `0x004D9505` with `ECX` = the **FootClass** | `disassemble_function 0x004D94B0` |
| `0x004C9150` | `Stub__ReturnZero` | accurate — **keep** | `read_memory 0x004C9150 len 16` |
| plate on `0x0055A8F0` | "calls slot +0x60 (Mark_Track_Followed or similar)" | `+0x60` is `Is_Powered` `0x0055A930`, a pure getter. The rest of the plate checks out | `disassemble_function 0x0055A930` |
| plate on `DriveLocomotionClass__Constructor` | "0x6C bytes total" | the factory allocates **0x70** | `decompile_function 0x006C4010` |
| `0x00710000` | `TechnoClass__PerformDeploy` | the **`IsLocomotor=` warhead locomotor installer**. `this` is the *firing* object; args are `(victim, CLSID by value)`. Not a deploy of any kind | `get_xrefs_to 0x00710000` → exactly one caller, `WarheadTypeClass__Detonate 0x004690B0` |
| `0x0065EC30` | `ChronoSphere__WarpUnitsAtCell` | accurate as a *name*, misleading as a *role* — reachable **only** from `TriggerAction__Execute`. It is not the superweapon path | `get_function_callers 0x0065EC30` |
| `0x00741970` | `TechnoClass__Set_Destination` | **`UnitClass::Set_Destination`** — one DATA ref `0x007F60F0`, and `0x007F60F0 − 0x007F5C70 = 0x480` | `get_xrefs_to 0x00741970` |
| `0x0065AD30` | *(cited by a lane as "the vehicle chrono-warp entry")* | **`RadioClass::Contact_With_Whom`**, a 16-byte function | `get_function_by_address 0x0065AD30` |
| `0x0054DA50` / `0x0054DB00` / `0x0075C8E0` | `FUN_*` | Jumpjet `End_Piggyback` / Jumpjet `Is_Ok_To_End` / Walk `Is_Ok_To_End` — all three provable from vtable slot bytes | `read_memory 0x007ECD44 len 32`, `read_memory 0x007F69D4 len 32` |
| `0x0054DA00`, `0x0075C850`, `0x0075C8A0` | *(no function defined)* | Jumpjet/Walk `Begin_Piggyback` and Walk `End_Piggyback` | same vtable reads |
| `0x00521EB0`, `0x00522FE0`, `0x004DF7F0` | *(no function defined)* | InfantryClass `+0x4F8`, InfantryClass `+0x508`, FootClass `+0x508` | vtable-base arithmetic, §2.9 |
| `0x00718100` | already `TeleportLocomotionClass__HeadToCoord`, with a plate comment | **accurate — keep.** Recorded because an earlier pass declared it unrecoverable without ever asking Ghidra for it | `read_memory 0x007F5000 len 80` (slot 17) |
| `0x004C93D0` / `0x004C9480` / `0x00A8ED84` | already `RateTimer__Current` / `CDTimerClass__Remaining` / `g_CurrentFrameCounter` | **accurate — keep.** §9 OQ8 called all three UNKNOWN for a full study | `batch_decompile 0x004C93D0,0x004C9480` |

Also note: the decompiler output for `FootClass::Set_Destination_Internal` `0x004D94B0` is
**unusable** — it fabricates `iRam00000000`, mislabels the IPersist QueryInterface as a
direct vtable call, and names the compared CLSID `CLSID_WalkLocomotion` when the bytes at
`0x007E9A40` are the **Hover** CLSID. Anyone reading that function must use the assembly.

---

## 11b. Self-corrections — where THIS document was wrong

Added 2026-07-29. These matter more than the corrections to other docs, because a reader
who trusted the earlier text would have built the wrong thing. Each states the old claim
verbatim, then the evidence that overturns it.

### C14 — §1.6, §5.7 S2/S4 and §11 C6: "`+0x674` is NULL after a load" is **REFUTED**

**Old claim.** *"`UnitClass::Load` NEVER REINSTALLS A LOCOMOTOR … `+0x674` is NULL after a
load. The re-acquisition path is UNKNOWN."*

**Why it was wrong.** Truncation, not misreading — every instruction the old text cited is
real. It read the pre-load teardown at `0x004DB3DF` and stopped, 0x189 bytes before
`FootClass::Load`'s own tail refills the slot via `OleLoadFromStream` at `0x004DB568`
(`disassemble_function 0x004DB3C0`; `get_xrefs_from 0x004db568` →
`PTR_OleLoadFromStream_007e15f8`). Every downstream inference inverted with it:

| Old | New |
|---|---|
| "`EDI` reads the NULL that S1 wrote" | `EDI` holds the **restored** locomotor |
| "`CMP EBP,EDI` is equal" | `EBP` is NULL (zeroed by `FootClass::Constructor` at `0x004D355E`), `EDI` is not — they differ |
| "the guarded store at `0x00744548` is a no-op" | it executes on **every** load and is the only thing that survives the locomotor across the in-place constructor re-run |

Three independent corroborations the correction rests on, beyond the call itself: the
refcount ledger balances exactly (`OleLoadFromStream` → 1, `AddRef 0x00744553` → 2,
unconditional `Release 0x00744564` → 1); `0x00744561 MOV EAX,[EDI]` dereferences `EDI`
**unguarded**, so gamemd itself assumes the slot is always refilled; and the identical
three-instruction shape appears in `InfantryClass::Load 0x00521A38` and
`AircraftClass::Load 0x0041B512`. The only part of the old C6 that survives is the
incidental observation that the vtables at `0x00744521`–`0x00744534` are `UnitClass`'s own.
**Corrected text: §5.7.**

### C15 — §5.4: "Magnetron `[LocomotorBeam]` warhead — **RAW swap**, no piggyback" is **REFUTED**

`0x00710000` performs a full `Begin_Piggyback` at `0x007102D8` — a direct `CALL [EDX+0xC]`
on the IPiggyback QI of the freshly created locomotor, with the host's old pointer as the
argument, in canonical B4-before-B5 order (`disassemble_function 0x00710000`). Its sole
caller is `WarheadTypeClass::Detonate` (`get_xrefs_to 0x00710000`), and the CLSID is
supplied **by the caller** from `WarheadTypeClass+0x15C`, not hard-coded.
**Consequence: the acceptance test §8 S3 previously named
(`locomotor_beam_installs_jumpjet`, asserting "the previous locomotor is dropped, not
stashed") would have locked in behaviour gamemd does not have, and would have left a lifted
unit unable ever to return to its original locomotor.** That test is rewritten in S3. This
is the single most valuable output of the closeout wave.

### C16 — §5.3/§5.4: the Chronosphere superweapon address is misattributed

**Old claim.** *"Chronosphere superweapon warp (`0x0065EC30`)."* `0x0065EC30` has exactly
one caller and it is `TriggerAction__Execute` (`get_function_callers 0x0065EC30`) — a
map-scripted path, zero in skirmish. The superweapon is `SuperClass::Launch 0x006CC390`,
jump-table case **4 = `Type=ChronoWarp`** at `0x006CC4B2`, installing **Teleport**
`0x7E9A90` with BEGIN at `0x006CC98C`. `search_instructions function=SuperClass__Launch
operand=0x674` returns **exactly two** hits over 1,907 instructions, untruncated — so no
other stock superweapon touches a locomotor at all. *(Labelling note: the step that places
`0x006CC95A` in case 4 by address-betweenness in a jump table is **[I]**, not [V] —
betweenness is not control-flow proof. The conclusion is corroborated by case 3 being
self-contained, ending `RET 0x8`, containing no `CoCreateInstance`, and setting the
pending-action global to 4; and by `[ChronoWarpSpecial] PostClick=yes /
PreDependent=ChronoSphere`, which `0x006CEC90` parses into the same `Type=` name table.)*

### C17 — §9 OQ3 / §5.4: "`TechnoTypeClass+0x390` = `Teleporter=`" is **REFUTED**

`+0x390` is **`HoverAttack=`**: `0x00712553 MOV CL,[EBP+0x390]` → `PUSH 0x8443B0` →
`CALL 0x005295F0` → `0x00712567 MOV [EBP+0x390],AL`, and `read_memory 0x008443B0 len 32`
= `"HoverAttack\0SizeLimit…"`, with `EBP = this` proved at `0x00712180 MOV EBP,ECX`.
`Teleporter=` is **`+0xCD4`** (`0x00713FF6`, string at `0x00843E60`). Every other reader of
`+0x390` — `Mission_Attack`, `Mission_Guard`, `FootClass::AI` — is consistent with
`HoverAttack=` and none with `Teleporter=`. Derived by one lane and re-derived
independently by an adversarial pass.

### C18 — §5.3: "the sense flip between Drive and Teleport is real and unexplained" is a **category error**

They are unrelated members at different offsets in two incompatible object layouts. Placed
beside Walk — which no earlier pass did — the picture is one mechanism with two encodings
(§5.3). There is no inversion to explain. **A Rust model that collapses them into a single
`in_critical_section: bool` is worse than the confusion it fixes**, because Teleport's gate
is two fields and one of them is an eight-valued phase index; under that model Teleport's
gate becomes constant-true and the piggyback can unwind mid-warp.

### C19 — §9 OQ7: "`+0x428`/`+0x42C` … cleared by Teleport's `End_Piggyback` and by nothing else" is **REFUTED**

Both halves. They are cleared on **three** paths (Teleport `End_Piggyback` `0x00719EFF`,
the Teleport state machine `0x00719AEE`, Jumpjet `End_Piggyback` `0x0054DAC4`) and read on
**four** (`PostWarpValidation`, Jumpjet `FUN_0054CA90`, `TechnoClass::PointerExpired`,
`TechnoClass::Load`). They are a live `(BuildingClass*, HouseClass*)` pair. `PointerExpired`
registering `+0x428` is the engine's own proof it holds a live object pointer. Full detail
in §5.3.

### C20 — §2.4: the 40-slot framing understates Drive's vtable

Drive's ILocomotion vtable is **50 slots**, not 40 — the extent is bounded by the RTTI
Complete Object Locator pointer at `0x007E7F78`, giving `(0x7E7F78 − 0x7E7EB0)/4 = 50`
(`read_memory 0x007E7EB0 len 160`, `read_memory 0x007E7F40 len 64`). Slots 40–49 are
class-specific virtuals appended after the interface, and two of them are the setters for
the `Is_Ok_To_End` guard byte this document previously listed as UNKNOWN. §2.4's 40×11
matrix is **correct for the interface**; the inventory framing "of the 40 slots" is what is
incomplete. **Null impact on the Rust design** — AGENTS.md forbids porting the C++ dispatch
architecture and `LocomotorSlot` has no slot table to size (§6.1). Recorded in §2.7.

### C21 — §5.6 / §9 OQ5: downgrading "`+0xF0`/`+0xF4` = occupancy mark/unmark" to [U] and floating a getter/setter reading was **WRONG**

Added by the 2026-07-29 OQ4/OQ5 re-run.

**Old claim.** *"Consequently the `+0xF0`/`+0xF4` pair may not be occupancy mark/unmark at
all. The producer/consumer shape above fits a coordinate getter/setter better than an
occupancy bracket. Both readings are now [U]; the earlier [I] 'occupancy mark/unmark' label is
downgraded, not replaced."*

**Why it was wrong — a decompiler artifact, and this document acted on it.** The observation
behind the downgrade was real: in `TeleportLocomotionClass::Process` a `vt[+0xF4]` call is
immediately followed by `iStack_34 = *unaff_retaddr; iStack_30 = unaff_retaddr[1]; …`, which
reads as an out-parameter being filled. **`unaff_retaddr` is a Ghidra artifact from an
unmodelled `__thiscall` stack argument, not a real out-parameter.** The same idiom appears in
`WalkLocomotionClass::ProcessMovement`, where the decompiler *does* resolve it and it reads
unambiguously as a *producer*: the code copies the host's own `+0x9C/+0xA0/+0xA4` into a local
triple and then calls `vt[0xF4]` to **unmark at the current position**. Six decompiled bodies
settle it — none returns a value, none writes an object field, all six take a `CoordStruct*`
and touch only `cell[+0x124]`/`cell[+0x128]` — and the caller-semantics witness is decisive:
`CellClass::AddContent` calls `+0xF0`, `CellClass::RemoveContent` calls `+0xF4`.

The artifact is not a one-off: the adversarial pass hit the identical `unaff_retaddr` shape
independently in `decompile_function 0x005F6060` and `0x005F5FA0`, where the decompile
contradicts the assembly and **the assembly wins**. Treat a bare `unaff_retaddr` in this
codebase as a signal to drop to disassembly, not as evidence.

**Two downstream statements inverted with it.** §5.6's dispatch list carried the `[U]`
downgrade forward, and **§11 C8's parenthetical rationale** — *"the conclusion may well be
right (the `+0xF0`/`+0xF4` host-virtual route is documented)"* — was wrong twice over:
`+0xF0`/`+0xF4` is not a position route at all, and the conclusion it was defending is
refuted in effect (C8, as now rewritten). **Corrected text: §5.6.2, §5.6.1, §11 C8.**

**The general lesson this document should carry.** The `[I]` that caused this was a role
guess from a call-shape observation, made without decompiling the callee. The writer-sweep
lane repeated the same mistake on `+0x124` in the same wave — guessing "cloak bracket" from a
Ghidra label, leaving the slot UNCHECKED, and then writing a **design rule for S3** on top of
the gap. Both times the cost was a recommendation that would have shaped Rust. **A slot whose
identity is UNCHECKED must not carry a design consequence.**

### C22 — §5.6 / §11 C8: treating the position commit as ONE mechanism was never stated but was assumed by both lanes, and is **WRONG**

Recorded here rather than as a correction to another doc, because the assumption is what the
document would have carried into S3 had the re-run not run. Both OQ4 lanes independently
recommended "expose a single host `set_coords`". gamemd has a **full-coordinate commit**
(`+0x1B4`) *and* a **height-only commit** — `FootClass::Set_Height_On_Bridge` at `+0x1CC`,
`ObjectClass::AI`'s per-tick `+0xA4` writes, and the directly-called `0x005F6060` — each with
its own occupancy bracket, and `HoverLocomotionClass::Move` uses one of each in a single call
(§5.6.1). The document itself never asserted the single-mechanism shape, but it also never
questioned it; a reader taking the lane recommendations at face value would have built it.
**Corrected text: §5.6.1 and §8 S3.**

---

## Appendix — provenance

Eight parallel read-only Ghidra lanes (`com-lifecycle`, `motion-slots`,
`render-query-slots`, `push-shove`, `power-ion`, `piggyback`, `factory-clsid`,
`host-contract`) plus two Rust survey lanes (`rust-architecture`, `rust-adhoc`), then three
independent adversarial refutation passes whose only goal was to break the lane reports.
The adversarial passes produced 11 + 8 + 18 findings; every one that contradicted a lane is
carried in this document as the winning claim, and each is marked **[R]** at its point of
use or listed in §11.

**No Ghidra state was modified in any pass** — no renames, no comments, no `save_program`.
All `disassemble_bytes` calls used `dry_run: true`.

**No Rust code was written this session.** This is a design study; the code lands in the
slices of §8.

---

## Changelog

### 2026-07-29 — OQ4 / OQ5 re-run (position commit and occupancy mark/unmark)

**Why this pass exists.** The closeout wave commissioned an `oq4-position-commit` lane; **it
died mid-run and produced nothing**, so OQ4 and OQ5 stayed open with a wrong `[I]` lead in the
document. This is the replacement: two independent lanes (a data-first `+0x9C` writer sweep,
a vtable-first slot sweep) plus an adversarial cross-check whose only job was to break them.
**Where the lanes and the adversarial pass disagree, this document carries the adversarial
verdict.**

**Closed — OQ4 and OQ5, both.** OQ4: the full-coordinate commit is host virtual **`+0x1B4` =
`0x004DB810`**, confirmed on all three host vtables by raw slot reads and independently by
slot-delta arithmetic over fifteen vtables; its store is the nine-instruction
`ObjectClass::Set_Raw_Coords 0x005F6940`. **But the commit is two mechanisms, not one** —
Z has at least four writers that bypass `+0x1B4`, and `HoverLocomotionClass::Move` calls one of
them (`0x005F6060`) directly, non-virtually, with the host in `ECX`, in ordinary skirmish
(Hover is on `[LCRF]`/`[ROBO]`/`[SAPC]`/`[YHVR]`). OQ5: **`+0xF0` = MARK, `+0xF4` = UNMARK**,
proved from six decompiled bodies and from `CellClass::AddContent`/`RemoveContent` as the
matched callers. Both answers live in §5.6.1–§5.6.3. Three slots closed in passing —
`+0x484` = `OnArrival`, `+0x124` = `Mark(MarkType)`, `+0x1CC` = the height-only commit — plus
`+0x78`, which turns out to forward to **the installed locomotor's** `In_Which_Layer`, so the
occupancy gate inside the commit is locomotor-dependent (§9 item 20).

**Overturned — three claims, one of them this document's own.**
(a) The writer-sweep lane's *"nothing in the commit path touches occupancy"* is **REFUTED**:
the `+0x124` bracket **is** the occupancy update, chain traced end to end with no inferred
hop. Its design consequence — "keep commit and occupancy separate" — was written on an
UNCHECKED slot and **must not reach S3**.
(b) The same lane's writer set is wrong **from its own sweep output**, which the adversarial
pass reproduced instruction-for-instruction (1,152,197 instructions, 213 hits): three `+0xA4`
writers were missed, and `ObjectClass::AI` was cleared by checking a different site inside it.
(c) **C21** — this document's downgrade of the `+0xF0`/`+0xF4` occupancy reading to `[U]`, and
the getter/setter hypothesis it floated, were caused by a Ghidra `unaff_retaddr` artifact. The
occupancy reading is **restored**, not replaced. **C22** records that both lanes assumed a
single-mechanism commit and that the document would have carried it into S3.

**Still open, and stated plainly.** **Nothing in this pass meets the VERIFIED parity bar.** The
one executable check attempted — `emulate_function 0x005F6940` — confirmed the store base is
`this + 0x9C` but **returns registers only**, and two memory-seeding formats both failed to
land, so a store-only function's result is unobservable to that tool. All of the above is
semantic understanding: a basis for a ratchet, not a parity check (§5.6.1 "Parity status",
§10). Separately, **register-computed indirect calls** (`MOV EAX,[reg+0x1b4]; CALL EAX`) are
outside every sweep in all three reports and in this document's earlier Push/Shove sweeps
(§10). Not attempted at all: the `vt[0x134]` redraw body, `vt[0x84]`/`vt[0x38]`,
`FUN_00473450`'s list identity, and whether any `+0x124` path touches radar. The Walk
intra-cell suppression site is **[V] by the vtable lane, UNCHECKED by the adversarial pass**.

**Slice status.** **S4's parity status is unchanged and the re-run adds no scope** — every
slot involved (`+0x1B4`, `+0x124`, `+0x1CC`, `+0xF0`, `+0xF4`) is a **host** virtual, so a
stashed inner locomotor commits through the same host slots with no handoff, and the Hover
finding is host-side too. `end_order_matches_native` still ships as a well-provenanced
ratchet; the route to VERIFIED is still `emulate_function` over `0x007192F0` / `0x004DAE5F`.
One new caveat: the commit's occupancy gate reads the *installed* locomotor's layer, so a
cached layer would diverge during a piggyback (**[UNCHECKED]** — no stock trigger traced).
**S3 gains four host-surface constraints and loses one recommendation**: two commit entry
points rather than one, occupancy bracketed *inside* both, host-owned commit with the
changed-test and follower cascade, and the position field at the generic-entity level. All
four are **[UNCHECKED]** as parity.

**No Ghidra state was written. No Rust source was edited.** Label drift observed and not
written back: `0x004D3780` (`TechnoClass__DoCloak` → `Mark`), `0x004DB7E0`
(`FootClass__GetThreatLevel` → forwarder to the locomotor's `In_Which_Layer`), `0x004AFD0E`
(`Head_To_Coord` → a coordinate *getter*) — §11 C13 is the place for whoever next holds the
write lock.

### 2026-07-29 — open-question closeout pass

Four investigation lanes (`oq1-saveload`, `oq2-infantry-chrono`, `oq6-isoktoend-gates`,
`oq9-frequency`) plus two adversarial verification passes whose only job was to break them.
**A fifth lane, `oq4-position-commit`, was commissioned and never written** — OQ4 is
therefore untouched by this wave and must not be counted as addressed. Where a lane and an
adversarial verdict disagree, this document carries the verdict.

> **Superseded in part.** Everything below about OQ4 and OQ5 is the state as of this wave
> only; both were closed later the same day by the re-run entry above. The `+0xF0` `[I]` lead
> recorded here was wrong (§11b C21).

**Closed — 9 of the 26 binary-side open questions, plus one half-closure.**
OQ1 (save/load re-acquisition), OQ2 (Chrono Legionnaire), OQ3 (`+0x390`), OQ6
(`Is_Ok_To_End` gates, all four classes), OQ7 (`+0x428`/`+0x42C`), OQ9
(`SuperClass::Launch` / the "PerformDeploy" branch), OQ10 (`0x004DF7F0`), OQ11 *identity
only*, OQ15 (Walk/Jumpjet stashes). OQ17 half-closed — a reader of `+0x08` now exists; the
"why twice" does not.

**Still open — and one is contract-critical with no lane behind it.**
OQ4 (position commit): `+0x480` withdrawn as a candidate because it is `Set_Destination`;
`+0x484` examined by nobody; `+0xF0` offered as an **[I]** lead. OQ5 (`+0xF0`/`+0xF4`
mark/unmark) — the assignment *and* the "occupancy" reading are now both [U]. OQ8
(`+0x388`) narrowed to one hop: the timer helpers are ticks, but `+0x388` was never tied to
them. OQ11 *reachability* — the earlier "dead" verdict was refuted and the replacement
argument was not discharged; UNRESOLVED-REACHABILITY both ways. OQ12–14, 16, 18–29
untouched. Six new items recorded (16b–16e, 20b, 20c), the largest being that the §5.4
trigger table is **not** an enumeration: 51 `+0x674` sites across three addressing idioms,
eleven touched functions in no lane report, and a live store inside a range previously
called zero-reference.

**Prior claims overturned — six, five of them this document's own.**
C14 "`+0x674` is NULL after a load" (inverted — it is refilled in the same function).
C15 "Magnetron = RAW swap" (it is a `Begin_Piggyback`; the S3 acceptance test as previously
written would have encoded the wrong contract).
C16 the Chronosphere superweapon address (`0x0065EC30` is trigger-only).
C17 "`+0x390` = `Teleporter=`" (it is `HoverAttack=`).
C18 "the sense flip is real and unexplained" (category error).
C19 "`+0x428`/`+0x42C` cleared by nothing else" (three paths, four readers).
C20 records a 40-vs-50 slot inventory gap with null design impact.

**Slice status — neither S3 nor S4 gains a VERIFIED parity claim from this wave.**
S3's install-at-spawn model is strengthened and its Magnetron test is corrected to assert a
stash; its save/load half is now *understood* but stays **UNVERIFIED**, because semantic
correspondence argued in prose is not a gamemd-derived executable check and a byte
comparison against a gamemd save is impossible. S3 also loses its only live raw-replace
example. S4 can now be written against the *correct* END predicate — both blockers the
lanes named are closed — but `end_order_matches_native` remains a hand-transcribed golden
and ships as a **well-provenanced ratchet**, not parity; the route to VERIFIED is
`emulate_function` over `0x007192F0` / `0x004DAE5F`. S4's scope grew by the ChronoWarp
superweapon and the Magnetron, and must **not** rest on "infantry contribute zero piggyback
traffic".

**No Ghidra state was written in any pass. No Rust source was edited.**
