# Ghidra working notes

[AGENTS.md](../../AGENTS.md) defines evidence and delivery. These notes cover tool
behavior and recurring interpretation errors; choose the investigation method yourself.

For repeatable static byte reads, disassembly, direct caller candidates and field
scans beside Ghidra, use [the shared native inspection tool](../../tools/native_inspect.md).
It checks the original executable identity and maps file-backed PE ranges through
the oracle owner. Its linear sweeps do not establish instruction boundaries,
reachability or exhaustive aliases; preserve those limits in findings.

Loaded PE images can contain zero-filled virtual tails with no original file bytes.
Do not record an empty string or zero read there as an executable literal. For
example, retail `0x889F64` is not file-backed: the inspection tool rejects a one-byte
read. Keep that address as a pointer lead until its producer or runtime contents
are established. Validate literal ranges with `native_oracle.file_span`; reading
zeroes from a loaded image does not establish the current contents of mutable data.

For read-only before/rehearsal comparisons of decompilation artifacts, known
callers and native-versus-p-code stack frames, use
[`python -m tools.ghidra_compare`](../../tools/ghidra_compare.md). Supply explicit
programs and census paths. Incomplete reads cannot pass; findings still require
instruction-level interpretation rather than automatic acceptance.

## Connect to the intended program

Discover the instance and confirm the program path, binary identity and image base
before relying on addresses. Use explicit program selectors when the tool exposes
them; another session can change the shared current program.

The installed bridge registers analysis tools after connection. `check_tools` checks
that registry, not endpoint health: `not_found` before connection does not establish
that an operation is unsupported. After connecting, inspect the current schema,
load needed groups if using lazy loading, and try a relevant read. Tool names,
arguments and availability come from that schema, not an old command list.

Empty discovery does not prove Ghidra is stopped. Inspect the process and configured
connection before relaunching; use machine-local `ghidra-up` when available. Reuse
the analyzed program. Re-importing or enabling analysis is not routine reconnection.

GhidraMCP 5.14.2 headless can abandon a large p-code reply before sending headers.
At `0x675210`, the full export exhausted its 2 GB Java heap in Gson serialization;
replacement HTTP workers were idle while the client waited. A timeout alone does
not establish that decompilation or the server stopped. `granularity=basic` returned
the complete 248,735-operation graph while exporting each operation once. In this
installed Ghidra 12.1.2 path, both modes obtain a fresh `HighFunction`; its AST
decoder links every decoded operation into a block. These block operations retain
the decompiled SSA view. Reject export errors and incomplete blocks, and preserve
all operations and varnode widths. This equivalence is specific to the unchanged
decoded graph: a generally edited `PcodeSyntaxTree` may also contain dead operations
outside its blocks. The comparison tool uses the block export and still requires
complete reads and native frame checks.

That export omits SSA definition identities and block edges. Repeated
`(space, offset, size, merge_group)` values can describe different definitions;
do not join them globally as one value. Branch direction also needs native
instructions and C: at `0x740E2B`, the exported `INT_NOTEQUAL`/`CBRANCH` target
appears reversed relative to the original `JE` and matching C. A predicate and
target alone cannot establish which branch executes.

Raw tuples can also be reused within one instruction. At `0x6F188A`, an address
input has the same unique tuple as a later `LOAD` output. Resolve only definitions
before each use within that instruction; a later output cannot establish the input.
Keep entry values opaque unless original instructions and connected C establish their role.

An exported operation's `seq.address` does not prove its native operand role.
Ghidra 12.1.2 can insert a phi-edge `COPY` tagged with the predecessor block's
last address. At `0x41028C`, the native instruction reads the receiver at entry
stack `+4`, while a high-p-code `COPY` from stack `+8` seeds the GUID comparison
loop at `0x410290`. Grouping operands by that address reports a false frame
mismatch. Check the original instruction and the operation's block/operand role
before classifying a shift; preserve the raw diagnostic and separately check
the native receiver and call inputs.

The repository's `NativeFrames.code` skips memory operands at function entry.
At `0x465380`, original `MOV EAX,[ESP+8]` (`8B 44 24 08`) reads a four-byte stack
input, while the mapper reports no stack operands for that body. Check the entry
instruction and its input/output in C and high p-code separately; an empty frame
map does not certify their absence or survival.

In installed GhidraMCP 5.14.2 with Ghidra 12.1.2, `clone_data_type` can rename a
stored function definition instead of creating an independent copy. The handler
calls `source.clone(current_manager)` and then `setName`; both
`FunctionDefinitionDB` and `FunctionDefinitionDataType` return the original object
when cloned into their own manager. Do not use this route to fork a callback type.

## Interpret evidence

- Names, signatures and pseudocode are interpretations. Resolve consequential
  ambiguity from bytes/instructions, receiver/argument flow and actual callers.
  A nearby label or attractive decompile is not proof of identity.
- Check enum values against the native reader and dispatcher. YRpp `8468aab5`
  declares `BehavesLike::Smoke = 0` and `Gas = 1`, but ParticleTypeClass's INI
  reader uses `Gas, Smoke, Fire, Spark, Railgun` at `0x8370BC` and stores that
  table index at type `+0x314` (`0x6453D7..0x6453FF`). ParticleClass's dispatcher
  reads the same field and switches on it (`0x62CE43..0x62CE54`), so those two
  header values would misidentify the native particle handlers.
- Check pointer types: `int *p; p[0xac]` addresses byte offset `0x2b0` on this
  32-bit target. Addition to an integer address uses byte offsets.
- For virtual calls, establish the table/subobject owner, read the actual slot,
  follow receiver-adjusting thunks and check callers. Inspect surrounding instructions
  for questionable boundaries; compiler lifecycle plumbing can resemble gameplay.
- HTTP xref kinds reflect Ghidra flow overrides. At `0x70DC6A`, the original
  bytes `E9 11 9C 00 00` jump to `0x717880`, while `/get_xrefs_to` reports
  `UNCONDITIONAL_CALL`. Decode the original instruction before deriving argument
  storage or a caller's stack effect from that label; a tail jump pushes no return address.
- Find state writers and initialization. Zero-filled image data may be populated
  at runtime. Confirm active-YR gates and retail inputs; inherited TS code alone
  does not establish a feature's applicability.
- A scan finding no absolute pointer or direct branch to an address does not
  establish that it never runs. Computed pointers and indirect dispatch are
  outside that scan. Record its coverage; unreachable claims need breakpoint
  or flag-to-leaf evidence.
- A missing field in a register-tracking scan does not prove it is unused. Check
  indexed operands and receiver preservation across compiler helpers. `_chkstk`
  saves ECX at `0x7CA650` and restores it at `0x7CA678`; treating that call as an
  ordinary volatile-register call hides `Find_Path`'s receiver save at `0x4D3936`
  and its indexed path-buffer access at `0x4D3E98` (`this+index*4+0x5E0`).
- Ghidra 12.1.2 defaults to ignoring NaN tests attached to floating comparisons.
  Read the original x87 status test before porting an equality: `0x5B38E5` does
  `FCOMP`, `FNSTSW AX`, then tests C3 with `TEST AH,0x40`; its equal branch also
  admits unordered operands. The decompile shows equality alone on both sides
  of the INI typing pass. A displayed `NAN(...)` is a p-code predicate, not proof
  of a native function call; check the instruction and high p-code.
- Check a decompile's stack offsets against the code when a parameter or local looks
  misplaced (`unaff_retaddr`, `in_stack_`, a parameter where another is pushed). For a
  call whose stack change it does not know, the decompiler assumes the call pops
  nothing, unless paths that meet pin the value. That covers every virtual or COM call,
  and every direct call to a function without a stored purge. A call that pops its
  arguments then leaves the rest of that path off by those bytes. `__alloca_probe`
  (0x7CA650) is MSVC's `_chkstk`, which Ghidra's stack analysis does not model, so the
  decompiles of its 30 callers place their locals above the return address. The
  repository's [`ghidra_compare
  frames`](../../tools/ghidra_compare.md#stack-frames-against-the-native-instructions)
  command compares the decompiler's offsets with ESP computed from the code. Typing the
  function pointer does not change the pop: the decompiler applies a pointer's prototype
  only after its stack analysis (`ActionDeindirect`). A user
  `CALL_OVERRIDE_UNCONDITIONAL` reference on the call does: the decompile then shows a
  direct call with the target's prototype and pop. Add one only where the target is
  proven to be a single function ([Virtual-call references](#virtual-call-references)).
  The override takes effect only as the operand's primary reference. Where analysis
  propagated the vtable, the operand already holds a primary `READ` of the vtable slot;
  an override added beside it is ignored, and removing the `READ` afterwards does not
  promote it. Remove every reference on the operand, then add the override alone
  (`add_memory_reference` then reports `is_primary: true`). The overrides at the second
  Fetch_ID call of four Load functions (`0x41B534`, `0x521A5A`, `0x71CEB6`, `0x74456A`)
  sat beside such `READ`s and changed nothing until they were redone on 2026-10-07.
- In a method typed with an interface view (`<Class>_ILocomotionView`, the object seen
  from its interface pointer at +4), `this[-1].<last view field>`, with or without `&`,
  is `this - 4`, the object itself. An owner typed `FootClass *` that is an AircraftClass
  shows AircraftClass fields as `pLinkedTo[1].<field>`, offsets past FootClass's size.
- A method that gets `this` on the stack (COM interface methods, view methods) may store
  another value in `this`'s slot. From there on the decompile shows that value as
  `this`. Check writes to entry stack +4 before trusting a later `this`. Any stack
  parameter can be reused this way: CounterClass__Load reads its entry count into
  `pStm`'s slot (0x49FC22), so the later `pStm` and the loop bound are that count.
- A derived class's destructor that adds no cleanup compiles to its base's code: its own
  vptr store is dead and dropped, so the body shows only the base vtable.
  `~DynamicVectorClass<T>` and `~TypeList<T>` are `~VectorClass<T>`'s body, and
  CounterClass's destructor (0x49F9D0) is `~VectorClass<int>`'s. Name such a destructor
  from the object its callers destroy.
- `new[]` of an element type with a destructor stores the element count in the 4 bytes
  before the elements, also in a buffer the caller supplies, so the element pointer is
  the block + 4. Such a vector constructor's items pointer is therefore not the
  allocation itself.
- MSVC's `_ftol` (`Math__ftol`, 0x7C5F00) takes its input in ST0, which a prototype set
  over HTTP cannot express. Its callers show `Math__ftol()`, and the decompiler drops the
  x87 expression that computes the input. A parameter used only there looks unused:
  Rocket's FUN_006620f0 reads `pRocketRules` +0x28 (0x662100, `fimul` at 0x66218E).
- A typed stack aggregate can change the displayed base without moving the address
  the code uses. Follow the full constant pointer expression in high p-code, including
  member offsets: the native LEA at `0x425713` addresses entry stack `-24`; after typing
  a coordinate, Ghidra expresses it as base `-28` plus member offset `4`. Comparing only
  the first `PTRSUB` reports a false stack shift. Keep this check within the connected
  expression; a unique varnode reused at another instruction is not an address proof.
- At a native stack write, compare the high p-code destination. A carried source can
  overlap the correct address while the destination is wrong: `0x4B3493` writes entry
  stack `-72`, but its earlier high p-code copied a value from `-72` into `-76`.
  Counting both as candidate addresses falsely certified the write. After typing split
  members removed that source alias, the same wrong destination looked like a new
  regression. Keep unrepresented destinations as failed checks rather than dropping
  them from coverage; a matching source or unchanged call count proves no destination.

Follow production consumers far enough to establish the claimed result. Visual/audio
work includes composition, active flags, selected assets/frames, timing and output;
a loaded asset or working helper does not prove the final result. Keep address,
verified role and reproducible evidence together, naming uncertainty honestly.

## Names and their sources

For recorded name/comment passes, use the
[annotation replay runner](../../tools/ghidra_pass.md). Check both `pending=0`
and `conflict=0` after saving: a non-atomic pass can skip conflicts while reaching
zero pending operations. Existing names and comments remain evidence leads;
transferring them does not establish class layouts or receiver types.
GhidraMCP's `get_plate_comment` and `set_plate_comment` address function headers.
For a plate on switch data, read `audit_global.plate_comment`, append to the
existing text, write only `batch_set_comments.plate_comment`, and read it back.
Do not treat a function-only read error as an empty data comment.

Bulk passes append a dated, tagged paragraph to each plate they touch (data labels
get a plate on the data address). The tag says where the name came from and what
was checked:

- `[2026-09-30 vtable functions]`: a function created at an RTTI vtable target
  that had none; its extent is Ghidra's disassembly from that entry.
- `[2026-09-30 RTTI names]`: a virtual method named from its slot. The class prefix
  is the slot's owner, the first class in the MSVC RTTI hierarchy whose vtable
  holds this body at that slot. The plate names the slot (including the interface
  vtable of a secondary subobject) and the source of the method name: the COM
  interface declaration, the other named overrides of that slot, YRpp's virtual
  declaration order where its length matches the RTTI vtable (a lead), or the
  deleting-destructor body (flag test, `operator delete` 0x7C8B3D on `this`,
  `RET 4`). `vt_entry_<hex>` means the method is unidentified and gives the slot's
  byte offset; `_adjustor<N>` marks a thunk that shifts `this` by N and jumps to the
  implementation. Interface-slot names, wrong class prefixes and constructor names on
  destructor slots were corrected, and the plate records the earlier name. A corrected
  prefix keeps the earlier method name when the slot's method is unidentified; when
  that earlier name's class is unrelated to the slot owner, the name was kept. Where
  an older method name only disagrees with the slot, the name was kept and the plate
  records the slot's method; most of these are synonyms. Seven older names that YRpp
  gives to another slot of the same class were checked against their bodies and
  corrected (for example `UnitClass__DrawExtras` 0x73CEC0, now `UnitClass__DrawIt`).
- `[2026-09-30 YRpp names]`: a non-virtual function or global named from a YRpp
  address binding. The plate states whether the body's `RET` matches YRpp's declared
  arguments. The name stays a lead. A global that already had its own name kept it;
  its plate records YRpp's binding. A later pass also replaced `vt_entry` placeholders
  at YRpp-bound addresses when YRpp declares the method in that slot, and skipped
  bindings whose YRpp name is itself a placeholder (`func_3C`, `sub_53E3C0`). Some YRpp
  addresses are wrong: a few land in the middle of an instruction, and some are a few
  bytes off. The plates of the functions involved say which.

  These passes read an Ares-era YRpp copy (29d74e92). A refresh against the pinned
  Phobos fork (`reference/YRpp` at 8468aab5), whose paragraphs cite it, named 597
  more functions at fork-bound addresses, 43 `vt_entry` slots and 4 owner-draw window
  procedures, and labelled 185 globals. Every name passed the `RET` check;
  constructors and destructors of classes with RTTI vtables also had to store their
  class's vtable. Of a random sample of 40 names and 10 labels judged from their
  bodies, 48 matched, one was contradicted (not applied) and one could not be told.
  Where the fork binds another name at an address named from the older copy, the
  name stayed and the plate records the fork's binding. The fork renames 20 slot
  names taken from the older order (slot +0x114 is `DrawIt`, +0x42C
  `GetAttackCoordinates`, +0x2A8 `TurretFacing`), and they were renamed. Slot +0x104
  is `DrawIfVisible`: it tests visibility and then calls +0x114, so
  `ObjectClass__DrawIt` 0x5F4B10 and two overrides named after the wrong slot were
  corrected.
- Trigger actions: `TActionClass__Execute` 0x6DD8B0 (the fork's
  `TActionClass::Execute`) switches on the action kind. Of the 131 handlers the fork
  binds, it calls 47 from the case its enum names. The other 84 have no call, jump or
  pointer found by the static image scan; that alone does not prove they never
  run. The fork notes that Execute inlines most handlers. Port an action from its
  Execute case and establish the active path there.
- `[2026-09-30 destructor audit]`: a destructor an older pass had named
  `__Constructor`, with the byte evidence.
- `[2026-09-30 duplicate names]`: a name several functions shared, or a
  `__Constructor` name on a function that is not that constructor, corrected from
  the body.
  - The vtable a function stores last names its class. A constructor calls its base
    first and returns `this`; a destructor stores its own vtable first and returns
    nothing. An older pass had named every function that stores X's vtable
    `X__Constructor`.
  - `_NoInit` is the save-game constructor: `X__Load` or a derived NoInit
    constructor calls it, it pops one argument and it sets the vtables.
    `AbstractClass__Constructor_VtablesOnly` is AbstractClass's.
  - `_Default` is the constructor without arguments. Usually only
    `TClassFactory<X>__CreateInstance` calls it.
  - `_Copy`, `_StringObj` and `_FromSurface` mark other overloads; the plate says
    what each takes.
  - A function that is not the constructor its name claimed, and whose identity is
    unproven, went back to its default `FUN_` name. Its plate keeps the evidence and
    the earlier name. Examples are 0x4CD600, which `FlyLocomotionClass__Process`
    0x4CCB40 calls every frame, and 0x718B70, which only
    `TeleportLocomotionClass__Move_To` calls.
  - Twelve names still belong to more than one function. They are thunks that show
    their target's name, three identical `CRect` copies, the two `What_Am_I` slots of
    `CellClass` and `SuperClass`, and the two `VXL_Sort_Rasterize` variants.
- `vtable__<Class>` and `vtable__<Class>__secondary_<offset>` (a decimal offset) label
  every vtable that has an RTTI complete object locator. Where an older label existed,
  the older one stays primary, and listings and decompiles show it: `vtable_BuildingClass`,
  `vtable_MapClass` and the other map and sidebar layers, and the locomotors'
  `<Class>__ILocomotion_vtable`, `__IUnknown_vtable` and `__IPiggyback_vtable`.
- `[2026-10-04 VERA-cited names]`: a function the Rust code cites as a `0x…` or
  `FUN_…` address that still had its default `FUN_` name, named from its
  instructions, callers and receiver; the plate gives the evidence and the citing
  Rust files. 191 functions were named. For 11 of them the only match was a number
  inside the body from the FFmpeg tables in `bink_data.rs`, not a citation; they are
  named from their code alone. Eleven matched functions have no call, jump or stored
  pointer anywhere in the image; they keep `FUN_`, and their plates say what the body
  does. The scan missed citations written without `0x` (such as
  `comparison410A40`), so some cited functions are still `FUN_`. Switch tables the
  Rust code cites
  got a plate listing their cases. Prefixes on functions without a receiver (static
  initializers and helpers such as `Shell__`, `Rmg__` and `Planning__`) are module
  labels, not class claims. The pass is recorded for replay on other copies of the
  database as
  [`2026-10-04-vera-cited-names.json`](../../tools/ghidra_pass/passes/2026-10-04-vera-cited-names.json).

Destructor and COM-interface method names rest on the bytes. For the 2,356 method
names taken from YRpp's declaration order, each body's `ret N` was compared with
YRpp's declared parameters: none showed a shifted slot, and a one-slot shift would
have changed the popped bytes for about 60% of them. The two bodies of slot +0x2F4
(`GetLastFlightMapCoords`, formerly `GetSomeCellStruct`) pop a pointer: the hidden
return buffer of the CellStruct the fork declares they return. A random sample of 40
names from YRpp order, YRpp address bindings and older overrides all matched their
bodies; 10 were trivial bodies judged through other overrides of the slot.

A name without a dated paragraph predates these passes; judge it by its own plate or
re-derive it. The scripts, plans and results of the 2026-09-30 passes are in the
machine-local research folder listed in `LOCAL.md`.

## Function boundaries

Offline passes on 2026-09-30 changed function extents. Their plates name what was
checked:

- `[2026-09-30 DB repair]`: misdecoded bytes that overlapped real instructions were
  cleared. The function was created or its body was extended, and the plate gives the
  old and new instruction counts.
- `[2026-09-30 recovered functions]`: a function created where real code had none.
  The plate names the evidence:
  - an entry of the startup or exit initializer tables (static initializers, which set
    globals' startup values);
  - a call or tail jump;
  - an address stored as a callback, such as the main window procedure 0x7775C0 and
    the dialog procedures;
  - or no reference at all. Such functions are probably dead code: show that one runs
    before porting it.
  
  Three functions were split out of bodies that had absorbed them, including one that
  `AircraftClass__Mission_Move` tail-jumps to.
- `[2026-09-30 boundary repair]`: one of these changes.
  - A body that stopped early was completed.
  - A fragment was merged back into its function.
  - A switch got its table and cases.
  - A jump to another function's entry was marked as a tail call.
  - A reference that pointed into the middle of an instruction was removed.
  - A body that left out bytes of its own instructions got them (11 functions).
  - `FUN_00435b70`, which started inside another instruction, became
    `BuildingLightClass__Destructor` at its real start 0x435B50.
  - Four functions that store a vtable were created where the bytes had never been
    decoded or were in no function, such as `LightConvertClass__Destructor` 0x556510.
  - Three scalar deleting destructors named `__Destructor` were renamed
    `__ScalarDeletingDestructor`, like every other function in the AbstractClass
    destructor slot (+0x20). `X__Destructor` names the plain destructor.

The decompiler follows control flow past a function's body, so a completed body
changes listings, cross-references and call graphs, but rarely the decompile. A switch
the decompiler cannot recover is the exception. Its decompile warns "Could not recover
jumptable" and "Treating indirect jump as call", and leaves out every case.
`BuildingClass__Mission_Attack` had this problem until a jump-table override was added.

The decompiler also shows a plain jump to another function's entry as that function's
code, inline. Every such jump now carries the flow override `CALL_RETURN`, so it shows
as a call. `_adjustor<N>` thunks therefore decompile as a bare call to their target,
because the target has no prototype. The `this` shift is N.

About 800 instructions still belong to no function. They are plausible code that
nothing references, so a reference from one of them (a reader or writer "in no
function") is not evidence until that code is shown to run. Some numbers in data
tables were once typed as pointers into code. A data reference into the middle of a
function is not proof of a code pointer until its source has been checked.

Undecoded bytes that decode as code are not always a missed function. Some functions
begin with a patched `ret` or `mov al,1; ret`, and their original body follows as
undecoded bytes that never run: 0x49F5C0, 0x49F740, 0x49F7A0 and 0x49F8B0, for example.
The code after each stub reaches its `ret` having popped 4 to 16 bytes more than it
pushed, which only the overwritten prologue could have supplied. At 0x4E60EB a
conditional jump was patched to `jmp`, which skips the code after it.

## Virtual-call references

Since 2026-09-30, a `call [reg+disp]` site whose receiver's class is established has
user-defined `COMPUTED_CALL` references to every function the RTTI vtables can put in
that slot. Caller lists, cross-references and call graphs include these virtual calls;
the decompile does not change. 6,296 of the 18,872 such sites have them. The analysis
and the list of added references are in the research folder listed in `LOCAL.md`.

- The receiver's class comes from the bytes: `this` of a virtual method (its class and
  every subclass), `this` of a function whose every caller passes a known object, a
  global object, an object a constructor just built, or a vtable the function stored.
- They are may-call edges: a call through a base class lists every override, including
  overrides that site never reaches.
- Calls through an object loaded from a field, an argument or a container have none. A
  method without callers may still be called virtually.
- `get_bulk_function_hashes` hashes cover references, so the hashes of the functions
  that got one changed that day.

Since 2026-10-06, locomotor calls whose target is proven to be a single function carry
user-defined `CALL_OVERRIDE_UNCONDITIONAL` references instead (`add_memory_reference`,
operand 0), and their decompiles show direct calls with correct stack pops:

- calls through the object's own vtables (no locomotor class derives from another);
- calls through the owner's vtable in slots that UnitClass, InfantryClass, AircraftClass
  and FootClass all fill with the same function;
- calls through a cell that the map's cell getters returned (CellClass has no subclasses).

Each function's plate lists its overrides; the evidence is in the same research folder.
Other virtual calls keep the decompiler's guess that they pop nothing. Ghidra's per-call
signature override would fix them, but it is stored as a label in the function's
`override` namespace, and GhidraMCP's `create_label` writes only global labels.

## Class layouts

Since 2026-10-01 these structs have every field checked in code against the
constructors, ReadINI, the other methods and the reads through their type pointers:

- `BuildingTypeClass` (0x1798 bytes): 195 fields after its base. `BuildingClass` +0x520
  is `BuildingTypeClass *pType`.
- `TechnoTypeClass` (0xDF8 bytes): 318 fields after its base, so BuildingClass
  decompiles show `pType->base_TechnoTypeClass.nTechLevel`. `g_TechnoTypeClass_Array`
  is `TechnoTypeClass **`.
- `UnitTypeClass` (0xE78 bytes): 34 fields after its base. `UnitClass` +0x6C4 is
  `UnitTypeClass *pType` and `g_UnitTypeClass_Array` is `UnitTypeClass **`. The
  TechnoType and BuildingType fields that hold a unit type (`pUndeploysInto`,
  `pPowersUnit`, `pUnloadingClass`, `pFreeUnit`, `pSecretUnit`) are `UnitTypeClass *`.
- `InfantryTypeClass` (0xED0 bytes): 38 fields after its base. `pSequence` points to a
  `SequenceStruct`: 42 `SubSequenceStruct` entries of 0x24 bytes, one per DoType.
  `InfantryClass` +0x6C0 is `InfantryTypeClass *pType` and `g_InfantryTypeClass_Array` is
  `InfantryTypeClass **`. `pEnslaves` (TechnoType) and `pSecretInfantry` (BuildingType)
  are `InfantryTypeClass *`.
- `AircraftTypeClass` (0xE10 bytes): 11 fields after its base. `AircraftClass` +0x6C4 is
  `AircraftTypeClass *pType`, the same offset UnitClass uses, and
  `g_AircraftTypeClass_Array` is `AircraftTypeClass **`. `pAirstrikeTeamType`,
  `pEliteAirstrikeTeamType` and `pSpawns` (TechnoType) are `AircraftTypeClass *`.
  AircraftClass +0x6C0 holds the IFlyControl interface, so IFlyControl methods such as
  `AircraftClass__Is_Fighter` read the type as `[this+4]`.
- The `TechnoClass` chain, +0x0..+0x520: 224 fields in `AbstractClass` (0x24, new),
  `ObjectClass` (0xAC), `MissionClass` (0xD4), `RadioClass` (0xF0, new) and `TechnoClass`
  (0x520), with new member structs such as `FacingClass` (`PrimaryFacing` +0x388),
  `StageClass`, `TransitionTimer` and `RecoilData`. The flat structs keep their older
  fields in their old shape, so the same member can appear twice: `Location_X/_Y/_Z` ints
  in ObjectClass and TechnoClass are a `CoordStruct` in RadioClass, and MissionClass
  `nDispatchStartFrame`/`nDispatchDelayFrames` are the `CDTimerClass DispatchTimer` of
  TechnoClass. Object fields have no INI key: a name is the existing one, YRpp's where
  the code bears it out, one taken from the code, or `Unknown_0xNNN`. TechnoClass
  +0x2AC/+0x2B0 are `pLocomotorTarget`/`pLocomotorSource` (once `DeployedFrom` and
  `pDeployedInto`), and CDTimerClass +4 is `dwClockPad` (once `nAccumTime`; no timer
  reads it).
- `FootClass` (0x6C0 bytes) is flat from +0 like TechnoClass: the TechnoClass rows
  +0..+0x520 are copied in, then 66 own fields. The server's naming policy prefixed 28
  older names in the copy (`Health` is `nHealth` there), so fix a chain field in both
  structs. `UnitClass`, `InfantryClass` and `AircraftClass` embed `FootClass
  base_FootClass` at +0. Corrected names include `NavQueue` +0x5AC (once `EnterQueue`),
  the attack-move order +0x5C4..+0x5D1 (`MegaMission`, `pMegaDestination`,
  `pMegaTarget`, `fHaveAttackMoveTarget`; once `TarCom_*`), the path directions +0x5E0
  (one int[24] kept as `nPathDirections` and `aPathDirections_1`), `cTubeIndex` +0x684,
  `fIsInitiated` +0x689 (once `bConvoyArrived`), `fIsFiring` +0x68D (once
  `bHasReachedDock`) and `fShouldEnterOccupiable` +0x690 (once `bIsDockingToBuilding`).
- `BuildingClass` (0x720 bytes) is flat from +0 the same way: the TechnoClass rows
  +0..+0x520, then 94 own fields, so a chain field is now fixed in TechnoClass, FootClass
  and BuildingClass. The garrison is `Occupants` +0x684 and the overpowering infantry
  `Overpowerers` +0x66C, each a `DynamicVectorClass<InfantryClass *>` spelled out as seven
  members. Where the code contradicts YRpp: +0x6CA/+0x6CB are the `[Structures]` map fields
  8 and 15 (`AIRebuildable`, `AIRepairable`; YRpp BeingProduced and ShouldRebuild), +0x6EB
  holds +1 or -1 for the cloak generator (`CloakGeneratorState`; YRpp HasCloakingData),
  and +0x53C, +0x6C9 and +0x6DE stay `Unknown_0xNNN` (YRpp OwnerCountryIndex,
  ShowRealName and NeedsRepairs; the code shows none of those roles). `PrismTargetCoords`
  +0x708 holds the weapon index in delayed-fire stage 1 and the coordinates only in
  stage 2.

Notes for readers:

- **Field names.** A field read from an INI key carries the exact key. The server's
  strict naming policy puts a Hungarian type prefix in front: `fPowered`, `nX`,
  `aBuildupFile`, `pToOverlay`. A key that names a file is stored in `<Key>File`, and
  the object built from it takes the key: `aCameoFile` and `pCameo`.
- **Still placeholders.** `ObjectTypeClass` (0x294 bytes) has only `pVtable` typed.
  `AircraftClass` (0x6D8 bytes) has its FootClass base and `pType`, none of its own
  fields.
- **Receivers.** Since 2026-10-01 the methods of `AbstractClass`, `ObjectClass`,
  `MissionClass`, `RadioClass`, `TechnoClass`, `FootClass`, `UnitClass`,
  `InfantryClass`, `AircraftClass` and `BuildingClass` have typed receivers. 928 of the
  1,079 functions with those name prefixes are `__thiscall` in their class namespace, so a decompile
  reads `TechnoClass::TechnoClass__IronCurtain(TechnoClass *this, ...)` and
  `this->IronCurtainTimer`. Each prototype declares the stack bytes its RETs pop;
  parameters nobody has checked are `undefined4`. Plates tagged `[receivers 2026-10-01]`
  record twelve corrected prototypes, among them the two `GetCursorForCell` of FootClass
  and InfantryClass, which take no stack parameter. Every direct call to the TechnoClass
  and ObjectClass `ReceiveDamage` now shows its seven arguments, and the 70 calls to
  `BuildingClass__CreateAnimForSlot` show their five. The plates also say why 151
  functions stay untyped:
  - COM methods (primary-vtable slots 0–7) take `this` on the stack (`__stdcall`).
  - Methods of a secondary interface receive the interface pointer, not the object.
    The IUnknown adjustor thunks (`_adjustor<N>`) shift it on the stack and jump.
    `What_Am_I`, `Fetch_ID` and `Create_ID` (+4) and AircraftClass's IFlyControl
    methods (+0x6C0) take it as their first stack argument. The INoticeSink overrides
    (+8) read it from ECX, which is the object + 8. The interface names are YRpp leads.
  - Trampolines tail-jump through a vtable, and some direct functions have no usable
    caller evidence. Some are not methods: `BuildingClass__ReadFromINI` 0x44F820 runs
    with the scenario INI in ECX.

  Six vtable methods without a class prefix were outside both passes and are untyped:
  0x4D9C60, 0x4E0150, 0x6FDD50, 0x709A90, 0x70A990 and 0x70AA60. The type-class methods
  are not typed yet, so their decompiles still show raw offsets.
- **Signatures.** Since 2026-10-01 every locked prototype declares the stack bytes its
  RETs pop. Plates tagged `[signatures 2026-10-01]` record the 27 corrected ones, among
  them `WeaponTypeClass__ReadINI` (it had a stack parameter named `this`, and it returns
  a bool) and `TechnoClass__ImbueLocomotor` (`FootClass * victim, CLSID locomotor`).
  Seventeen functions that cannot return are marked no-return:
  - the CRT exits: `CRT__exit` (the C `exit`), `__amsg_exit`, abort, terminate,
    `_fptrap`, the entry point and two internal ones;
  - the C++ and COM throwers `__CxxThrowException@8`, `_com_raise_error` and 0x7DC72E;
  - `FUN_006bec50`, which exits with the code in ECX, and `FUN_0054a8c0`, which prints
    and then calls it;
  - two endless loops that stand in for element compares, and their two wrappers.

  `_com_issue_error` cannot return either, but it stays unmarked and untyped; its plate
  says why. No switch table has a case the flows lack. 10,088 functions without a
  prototype still have an unknown stack purge.
- **Stack-address inputs.** The 2026-10-01 pass assigned an ECX input to 923 functions:
  735 were `__thiscall`, and 188 were `__fastcall` because they read EDX first too.
  `this` was `void *`, and each prototype declared the stack bytes its RETs pop. Those
  signatures and the `[stack objects 2026-10-01]` label record incoming storage, not
  established object identity; check the pointee in the original body and callers.
  For the stack-object cases, the decompiler previously missed the passed object and
  kept the values last stored there: a COM smart pointer folded to NULL, and the branches
  that test it vanished
  (FootClass__ChronoWarpTo 0x4DF7F0, SuperClass__Launch 0x6CC390). 9,407 of the 10,674
  calls that pass a stack address in ECX now reach a typed function. Plates tagged
  `[stack objects 2026-10-01]` say why each takes ECX: it reads it first, or it passes it
  on unchanged to a function that does. 80 such callees stay untyped; the stack-objects
  research folder (see `LOCAL.md`) gives each one's reason in `apply/left.json`.
  - Where such an object's address reaches a typed function, the decompiler no longer
    folds the object's vtable, so a call through it shows as a slot call:
    `DynamicVectorClass<MSAnim*>__SetCapacity(n + 10, 0)` reads
    `(*(code *)vt[2])(n + growth, 0)`.
  - `__alloca_probe`'s (0x7CA650) plate says it is MSVC's `_chkstk`. The plates of
    FUN_005271c0 and FUN_005271e0 say they are one function split in two.

Plates tagged `[2026-10-01 BuildingTypeClass layout]`,
`[2026-10-01 TechnoTypeClass layout]`, `[2026-10-01 UnitTypeClass layout]`,
`[2026-10-01 InfantryTypeClass layout]`, `[2026-10-01 AircraftTypeClass layout]`,
`[2026-10-01 TechnoClass layout]`, `[2026-10-01 FootClass layout]` and
`[2026-10-01 BuildingClass layout]` record where YRpp is wrong and the native quirks a
port must keep. Examples:

- AddOccupy and RemoveOccupy are swapped in YRpp.
- `TurretControl` is 0x14 bytes, and nothing initialises WeaponCount.
- PitchAngle is read in degrees but stored in radians.
- An unset BurstDelay draws a 3..5 frame delay from the scenario RNG.
- ReadPip does not keep an absent Pip: the default 1 comes back as 2.
- A missing EliteAirstrikeTeamType takes the AirstrikeTeamType read on the same pass.
- A SpawnDelay of 0 faults (it divides the frame counter) on an aircraft with a Trailer.
- The TechnoClass constructor draws once from the scenario RNG (+0x3C8), and
  TechnoClass::Fire draws the first spray offset of each burst (+0x2A0).
- The magnetron release sets BeingManipulatedBy only on a foot target whose vt+0x1C8()
  is above 0.
- Secret labs draw their reward with the drawn number itself as the index, so two labs can
  get the same type (`Assign_Secret_Lab_Production` 0x68C050).

The UnitTypeClass pass also corrected two wrong names: the LandType name converters
0x48DFD0 and 0x48DF80, once named `MovementZone_*`, are `LandType__ToName` and
`LandType__FromName`. The InfantryTypeClass pass found that 0x522910, named
`BuildingClass__AddGarrisonOccupant`, runs with the entering infantry as `this`; its
plate has the evidence. The AircraftTypeClass pass found that
`HouseClass__CheckBuildLimit` reads AirportBound through its type argument, where no
scan of the reads through AircraftClass +0x6C4 can see it; its plate has the rule. The
TechnoClass pass renamed nine functions whose `this` or purpose the code contradicts:
0x4D0EF0 `FoggedObjectClass__Constructor_Building`, 0x6B7D80
`SpawnManagerClass__CountLaunchingSpawns`, 0x4C2BD0 `EBolt__SetOwner`, 0x56DC20
`MapClass__Find_Nearby_Passable_Cell`, 0x720440 `ThemeControl__Constructor`, and the
iron-curtain and airstrike tint functions 0x70E380, 0x70E4B0, 0x70E5A0 and 0x70E920 (once
named after temporal, warp-in and gap effects). The FootClass pass renamed 0x4DFCB0
`FootClass__EnterBattleBunker` (once `Find_Nearest_Dock`) and 0x457CE0
`BuildingClass__CanBeOccupiedBy` (once `CanDock`; it tests CanBeOccupied, Occupier and
Assaulter, not docking). The BuildingClass pass named 0x459840
`BuildingClass__GetSecretProduction` and 0x68C050 `Assign_Secret_Lab_Production`, and
corrected the HasTurret 0x4527D0 plate (its loop walks the upgrades, not the occupants).
Each plate gives the evidence. The
per-field ledgers, the checks and the rehearsals are in the research folder listed in
`LOCAL.md`. Reading established these facts. Nothing was executed, so a port pins the
conversions with the native oracle.

## Preserve findings without polluting shared analysis

During authorized reverse engineering, preserve proven identities and useful evidence
with focused labels, comments and missing references. Read-only requests or
`--no-sync-ghidra-labels` disable these writes; `--sync-ghidra-labels` explicitly requests
them. This policy is the same for serial and delegated work. No separate candidate
ledger is required when a concise finding suffices.

Keep uncertain identities unnamed. References need decoded endpoints, operand and
reference kind; check for duplicates. Type, prototype or boundary repairs belong
within an authorized analysis-repair task, with prior definitions recoverable.
Once that scope is granted, do not ask permission for every edit. Read back structural
repairs immediately, including layout/offsets and affected decompilation. Byte patches,
bulk reanalysis and unrelated database changes need their own task scope.

Several copies of the database exist. Record a pass as a ledger for
[`ApplyGhidraPass.java`](../../tools/ghidra_pass.md) so the other copies can replay it
instead of redoing it.

Use one writer per shared program and coordinate changes affecting other workers'
evidence. Small coherent annotation batches are allowed. Inspect per-item results,
explicitly save the intended program, and read back the changes before unrelated
work or handoff. A committed analysis transaction is not a disk save. After a timeout,
inspect actual state before retrying; report partial or unsaved work accurately.

Quiet-window checks and frozen local files do not keep database evidence current.
Unsaved signature, calling-convention, purge or storage changes can leave project
file hashes unchanged; name/body hashes omit those properties too. After a backup
or any wait, reread the affected function metadata and full C/p-code at the final
prewrite boundary. Invalidate that admission after an intervening change or batch.

Label/type changes affect analysis, not executable bytes. Inspect current analyzer
settings when relevant; do not assume historical settings are still in force or
blame drift on an analyzer without evidence.

Tool behavior checked 2026-09-04 against the installed GhidraMCP 5.14.2 bridge
(`connect_instance`, `check_tools`) and plugin (`CommentService`,
`ProgramScriptService.saveCurrentProgram`). No connected instance was available
for a live persistence test; repeat capability checks when working against one.

Checked 2026-09-30 against the headless GhidraMCP 5.14.2 server:

- `get_plate_comment` and `set_plate_comment` work on functions only. For a data
  address, read the plate with `audit_global` (`plate_comment`) and write it with
  `batch_set_comments`, which rejects a first line shorter than four words.
- `rename_data` rejects a global name without a `g_` prefix; `rename_or_label`
  accepts a per-call `strict_mode` (`enforce`, `warn`, `off`).
- `can_rename_at_address` omits the current name at undefined addresses;
  `audit_global` reports it.
- A thunk shows its target's name until it gets its own, so renaming the target
  renames the thunk's display name. Rename thunks before their targets.
- `rename_function_by_address` with a function's own default name (`FUN_` and its
  address) turns the name back into a default symbol. It rejects an empty name.
- `create_label` at an address that already has a label adds a second one; the first
  stays primary. `audit_global` reports only the primary label; `list_globals` with
  `name_substring` finds the others.
- `set_global` checks the name against `NamingConventions.java`: after `g_` it needs a
  recognized Hungarian prefix (`p`, `n`, `dw`, `sz`, `ab`, ...), and where the server
  maps the prefix to types, the type must fit. No prefix stands for a struct, so it
  rejected `g_MouseCursorTimer` (a CDTimerClass) and `g_aMouseCursors` (an array of
  structs) on 2026-10-07. Only a prefix without a type mapping, such as `ab`, gets
  through, and it misdescribes the type. `apply_data_type` types a global and keeps
  its label; a default `DAT_` label then shows as the type and address
  (`CDTimerClass_00abf2a0`).
- Labels inside a struct-typed global stop showing in decompiles. Once
  g_DisplaySingleton (0x87F7E8) was typed MouseClass, CreditsClass__AI's write to
  0x884B90 read `...base_SidebarClass.fCreditsChanged = true`, not the label
  `g_SidebarNeedsRedraw` at that address (checked 2026-10-07). A name search still
  finds such labels, so check each one against the field it lands in when typing
  the global.
- The `find_code_gaps` records carry the neighbouring function names; compare gap
  positions and sizes, not the text, across renames.
- Longer names can rewrap caller C while basic p-code stays unchanged (checked in
  Ghidra 12.1.2/MCP 5.14.2 on 2026-10-06: 0x4144B0's call to 0x4DB0D0).
  For rename-only checks, compare C tokens with only the admitted identifier
  substitutions. Preserve literal contents and operator boundaries; compare
  comments, warnings and p-code separately.
- The decompiler exports plate text inside a block comment. An inner annotation
  such as `/*ECX*/` or `/*bridge*/` closes that exported comment early and can make
  complete-body readers reject intact function code. Use plain parentheses in
  plate text. Preserve the failing raw export when repairing delimiters; a syntax
  repair does not establish the annotation's claims or a passing before comparison.
- Saved-copy readers with the same Java filename in multiple script directories
  can dispatch an older copy despite the supplied script path. Give a modified
  private reader a unique filename and matching public class name. Check the
  actual `SCRIPT:` path and output markers against the pinned reader source;
  successful earlier post-scripts do not establish that the final reader ran.

Checked 2026-10-01, struct tools:

- Send `create_struct` and `recreate_struct` `fields` as a JSON string. In a JSON array,
  each offset reaches the parser as a Gson double (`3589.0`), which `Integer.parseInt`
  rejects. The field is then appended instead, so the layout comes out packed.
- Strict naming is the default when the project has no `.ghidra-mcp/conventions.json`.
  It puts a Hungarian type prefix on struct field names on create, `add_struct_field`
  and `modify_struct_field`, and a per-call `strict_mode` does not change that. Struct
  types, `sbyte` and the plain `pointer` type keep the name as given. A `uint` field
  gets `dw`. Readbacks spell `unsigned char` as `uchar` and a pointer to pointer as `T *
  *`, so a plan compared with readbacks must use those spellings.
- A retype clears the field name. Pass `new_name` to `modify_struct_field`;
  `modify_struct_field_type` always drops the name.
- No struct tool clears a field in place. `remove_struct_field` deletes the component
  and shifts every later field down (FootClass shrank from 0x6C0 to 0x6BC bytes on a
  staging copy). `add_struct_field` refuses to overlay a defined field ("Not enough
  undefined bytes"), and `modify_struct_field` rejects `undefined` ("New data type not
  found"). Change a live layout only by filling undefined bytes and retyping or renaming
  in place; a retype to a smaller type frees the tail bytes.

Receiver tools, checked on staging copies:

- An incoming ECX value does not establish an object receiver. Neither a class-name
  prefix, a convention-generated auto `this`, nor `lea ecx, [esp + ...]` proves its
  pointee identity. Confirm the input's use in the original body and its producer at
  callers before assigning object `this` or a class namespace; distinguish a complete
  object from a biased interface pointer. `BuildingTypeClass__FindIndexByName`
  (`0x45E7B0`) and `BuildingTypeClass__FindOrAllocate` (`0x4653C0`) consume name bytes
  in ECX, including caller-local text buffers, and compare them with BuildingType IDs.
  Their compatible one-register `__fastcall` view keeps `char *pName` explicit in
  `ECX:4` and uses the global namespace. It models the checked argument storage and
  does not establish the original C++ static/member declaration.
- A body that never reads ECX can still be a `__thiscall` member. When every caller
  loads ECX right before the call and nothing else uses that load, ECX is the receiver:
  both callers of DisplayClass__UpdateCellLighting (0x4AE4C0) load the screen singleton
  0x87F7E8 and load it again after the call, while the body names the singleton
  directly. Its class still needs other evidence (here its position among DisplayClass's
  members).
- `set_function_this_type` needs a `__thiscall` or `__fastcall` convention first. It
  moves the function into a class namespace named after the struct, and the auto
  `this` then takes that struct. It leaves an explicit custom-storage `this` with its
  old type. `set_function_prototype` applies dynamic storage, so run it first. No
  endpoint moves a function back to the global namespace.
- In the installed headless GhidraMCP 5.14.2, `set_variable_storage` reports
  `success=true` after printing the current/requested storage and manual instructions;
  it does not change storage. This was checked against source and installed bytecode
  on 2026-10-03. Read `get_function_variables` after storage-related writes and verify
  each register and stack slot. A prototype uses convention-derived dynamic storage;
  it cannot express every custom ABI. Keep an unrepresentable native contract qualified
  rather than assign its input to an unsupported register or invent an object receiver.
- `get_function_variables` mixes stored metadata with decompiler hints. Local
  `name`, `type` and `storage` come from the database; `is_phantom` depends on whether
  the current decompile exposes that local name, and controls `needs_type`,
  `needs_rename` and suggested types/prefixes. A callee signature change can change
  those hints while every stored local tuple stays identical (checked in 5.14.2 on
  2026-10-04). Preserve the raw replies, compare stored tuples separately, and check
  the changed caller's C and native operand flow before admitting the hint changes.
- `get_function_by_address` renders an effective signature through `getSignature()`;
  the saved rich reader and `SignatureCensus` use `getPrototypeString(true, false)`
  for a formal signature. Auto parameters and convention text can therefore differ.
  For a DEFAULT signature with no parameters, HTTP renders `(void)` while the saved
  readers can render `()`. HTTP names are unqualified; census names include namespaces.
  Checked in Ghidra 12.1.2/MCP 5.14.2 on 2026-10-04. Preserve each literal view and
  compare it with the same producer before/after; verify storage and flags separately.
  These formatting differences do not establish a native ABI or a source declaration.
- A `__thiscall` prototype must declare as many stack bytes as the function's RETs pop.
  `(void)` on a `ret 0xC` function breaks the stack analysis of its callers. A custom
  prototype that declares only `this` makes every direct caller's decompile drop the
  arguments.
- A low-byte read does not establish a byte-sized parameter. Trace whole-word copies
  and later reuse of the same entry-stack slot before narrowing its type. Team's
  `Script_Move_To_Own_Building` reads entry ESP+8 as a byte at `0x6EE5C3`, then uses
  that slot as a signed dword and a packed cell pair (`0x6EE5FB..0x6EE6F3`). A `bool`
  formal made Ghidra invent a widened `_NewStep` alias. Keep `undefined4` storage
  when the source declaration is unproved, and qualify the incoming low-byte flag
  in the plate. Inspect widened aliases as well as `in_stack_` warnings in rehearsals.
- A struct passed by value is copied into the outgoing argument area below ESP. Typing
  the copy constructor that builds it there can break the caller when the receiving call
  is untyped: in MPCooperative__vt_entry_A8 (0x5C42D0) the DynamicVectorClass copy built
  at 0x5C4437..0x5C4441 for an untyped virtual call (0x5C4449, the callee pops 0x28)
  made five earlier untyped calls lose their arguments once 0x5C4F30 was typed. A call
  override and a 40-byte prototype for the callee did not restore them, so that
  constructor stays without a prototype until those calls are typed.
- A register `bool` that the body forwards as a whole register (`mov ebx, ecx`, later
  `push ebx`) shows its upper bytes as `in_register_` at the forward, e.g.
  `CONCAT31(in_register_00000005, fOfficial)` in ScenarioClass__Post_Map_Init
  (0x68692A). The callers set only CL, so `bool` stays; a PRE comment explains the
  forward. `BOOL` would move the artifact into every caller.
- For a printf-style sink whose body is a bare RET (Stub__DebugLog 0x4068E0), `void
  __cdecl (char * pszFormat, ...)` keeps each call's arguments in its callers'
  decompiles.
- A new `Type propagation algorithm not settling` warning needs native producer and
  consumer checks alongside artifact and frame comparisons. Ghidra 12.1.2's
  [seven-round guard](https://github.com/NationalSecurityAgency/ghidra/blob/c0f584bf229fffba61b36431f3ce30c0c3e4e682/Ghidra/Features/Decompiler/src/decompile/cpp/coreaction.cc#L5425-L5467)
  can stop inference before checking whether the next round changes anything. A
  matched private replay of `0x574600` converged with identical final p-code and C
  after removing only the warning; that result covers this candidate alone.
  Other callers repeatedly alternated inferred coordinate homes between 8 and 12
  bytes. Those inferred widths do not establish native write spans: `0x4C93D0`
  and `0x565660` each write one DWORD to their output buffer. Check actual byte
  offsets, result consumers and branch inputs in each warned function, and record
  unresolved virtual targets or lifetimes. Keep established native receiver,
  argument and return contracts; changing them to silence a warning can conceal
  the underlying analysis problem.
  In the screen-class passes (2026-10-07) the warning followed the depth of the
  base chain. `TabClass__RemoveCommandBarButtons`, which only calls through the
  vtable, warned with `this` typed TabClass, whose `pVtable` is six bases down. It
  decompiled clean typed SidebarClass or PowerClass, and flattening a base did not
  help. Typing the screen singleton as MouseClass gave the warning, and nothing
  else, to 9 of its 1,082 readers.
- `set_function_prototype` applies the parameters under the function's old convention
  and sets the new convention afterwards. Ghidra stores a stack purge only for a function
  that has none, and computes it from that first step. So a `__fastcall` prototype on a
  function without a stored purge also counts 4 bytes for each register parameter, and
  callers read the stack that many bytes off after each call (8 with ECX and EDX). Write
  `__stdcall` with only the stack parameters first, then the `__fastcall` prototype. No
  endpoint reads a purge; the signature census of a staging copy does.
  For a native caller-cleanup `__cdecl` helper whose purge is unknown, use a
  zero-argument `__stdcall` prelude before the full `__cdecl` prototype. On 2026-10-03,
  directly typing comparators `0x52B6A0`/`0x52B720` stored 8 despite their bare native
  RETs; the signature and parameter readbacks looked correct. Check the stored purge
  with the census. Further prototype writes preserve an already valid purge, so this
  prelude cannot repair an existing incorrect stored value.
- The server renumbers parameters named `param_N` by position.
- `set_function_prototype` renames a function that still has its default `FUN_` name to
  the name in the prototype text, although the tool description says that name is only
  parsed. Use the intended name in the prototype, or rename afterwards.
- `set_function_prototype` writes `void *` without an error for a pointer to a type name
  it cannot find. Check that each named type exists before the write, and read the
  signature back.
- `create_function_signature` stores no calling convention and no parameter names, and
  nothing reads its parameters back; check the type through a caller's decompile.
- A PRE comment shows in the decompile only above a statement whose instruction keeps its
  p-code. One on a register copy that the decompiler folds away does not appear; put it
  on the call, store or cast of the statement.
- A return-type-only write can pass metadata readback while leaving the decompile
  unusable. In the 2026-10-03 INI rehearsal, `undefined1` left eight inferred
  full-EAX returns unchanged; concrete `byte` then locked their still-unknown
  signatures and removed inferred ECX/stack inputs. Establish the complete native
  input and return storage before locking a signature, then check fresh C and
  high p-code as well as the stored parameters and purge.
- Neutral `undefined` can use Ghidra's `DefaultDataType`; its length of 1 does not
  establish a one-byte native result or `AL:1` storage. On Ghidra 12.1.2,
  `0x5687F0` retained `<UNASSIGNED>` with no return varnodes before and after a
  checked `__thiscall` receiver write. Inspect the saved return datatype and
  storage, and establish any native result from its producers and consumers.
- A neutral `undefined4` return can still force `EAX:4`. Factory AI
  `0x004C9B20` gained a `CONCAT31` expression using `in_EAX` when a native exit
  defined only `AL`, leaving upper EAX bits untouched. Check each exit and result-using
  caller before assigning a width; retain unassigned metadata when no common
  return contract is established. A caller discarding the result does not prove
  a `void` declaration.
- When checking saved storage through Ghidra's Java API,
  `VariableStorage.toString()` includes ` (auto)` for automatic parameters, such
  as `ECX:4 (auto)`. Compare the literal storage and `isAutoParameter()` together.
  Removing the tag only from the expected string rejects a correctly stored
  automatic `this`.
- Stack cleanup totals do not establish each argument's physical storage.
  The research helper `protos.slot` rounds widths to four-byte homes, so
  `Stack[0x4]:1` and `Stack[0x4]:4` have the same `stack_end`; duplicate homes also
  leave that maximum unchanged. Check each saved datatype, storage varnode width,
  ordinal, automatic-parameter flag and distinct home against the native contract.
  A four-byte datatype display and matching `RET` cleanup cannot admit a narrowed
  native four-byte argument.
- A successful type-size lookup or `validate_function_prototype` reply does not
  establish that the signature parser can resolve a datatype. The validator checks
  format and convention without parsing the types. On 2026-10-02, two `GUID` entries
  made a staged `GUID *` prototype fail with `Can't resolve datatype: GUID *`, although
  the size lookup returned 16 bytes and pre-validation returned `valid: true`.
  `search_data_types` exposed both names; `get_struct_layout` checked the uniquely
  named, existing 16-byte `_GUID` view, whose pointer parsed successfully. Check a
  compatible unique type and rehearse the actual write; preserve ambiguous aliases
  rather than deleting or recreating them. `validate_data_type_exists` answered false
  for existing types on 2026-10-08; test existence with `search_data_types` (`pattern=`,
  a large `limit`, exact-name match).
- `set_function_no_return` makes the decompiler end each caller's path at the call
  (`/* WARNING: Subroutine does not return */`) and remove blocks reached only after it.
  It also removes live code where the decompiler wrongly folds an error branch to
  always-taken. That happens when a function passes a stack object's address in ECX to a
  callee without a prototype: the decompile does not show the callee writing the object.
  Marking `_com_issue_error` cost 12 of its callers code this way. A prototype can do the
  same: with only `_com_issue_error` typed, the stack past the call was tracked, and two
  callers lost blocks. On a staging copy, a mark did not change indirect calls the server
  resolved to the function. Compare the callers' decompiles on a copy before writing.
