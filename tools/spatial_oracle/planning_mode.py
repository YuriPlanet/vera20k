"""Small original planning-state corpus, consumed by the planning Rust owner.

This executes original instructions with supplied containers; it is not a
whole-game replay. Select VERA20K_GAMEMD_EXE, then run with --write or --check.
"""
import struct
from pathlib import Path

from tools import native_oracle as native

PATH = native.SCRATCH
NODES = PATH + 0x1000
ACTOR = PATH + 0x2000
EVENT = PATH + 0x3000
OUTPUT = PATH + 0x4000


def words(*values):
    return struct.pack("<" + "I" * len(values), *(v & 0xFFFFFFFF for v in values))


def signed(value):
    return struct.unpack("<i", words(value))[0]


def prestate(count, pending, loop_start=-1, loop_length=-1):
    writes = {PATH: words(ACTOR, 0x7EFE44, NODES, 8),
              PATH + 0x14: words(count), PATH + 0x8C: words(pending, loop_length, loop_start),
              NODES: words(*(PATH + 0x5000 + i * 0x100 for i in range(count)))}
    for i in range(count):
        node = PATH + 0x5000 + i * 0x100
        writes[node + 0x24] = words(PATH + 0x7000 + i * 0x100)
        writes[node + 0x30] = words(2)
        writes[node + 0xAC] = words(100 + i, 200 + i, 300 + i)
        writes[PATH + 0x7000 + i * 0x100] = words(PATH + 0x8000 + i * 0x100, PATH + 0x9000 + i * 0x100)
        writes[PATH + 0x8000 + i * 0x100 + 0x74] = words(400 + i)
        writes[PATH + 0x9000 + i * 0x100 + 0x74] = words(500 + i)
    return writes


def commit(count, pending, loop_start, loop_changed):
    writes = prestate(count, pending, loop_start, 3)
    writes[PATH + 0x98] = bytes([loop_changed])
    dumps = {"path": (PATH + 0x8C, 14)}
    for i in range(count):
        dumps[f"node{i}"] = (PATH + 0x5000 + i * 0x100 + 0xAC, 12)
        dumps[f"barrier{i}a"] = (PATH + 0x8000 + i * 0x100 + 0x74, 4)
        dumps[f"barrier{i}b"] = (PATH + 0x9000 + i * 0x100 + 0x74, 4)
    result = native.call(0x636CE0, ecx=PATH, writes=writes, dumps=dumps)
    path = bytes.fromhex(result["dumps"]["path"])
    rows = []
    for i in range(count):
        fields = struct.unpack("<3i", bytes.fromhex(result["dumps"][f"node{i}"]))
        rows.append(dict(index=i, active=fields[0], preserved=fields[1], cache=fields[2],
                         barrier_colors=[signed(int.from_bytes(bytes.fromhex(result["dumps"][f"barrier{i}{part}"]), "little")) for part in ("a", "b")]))
    return dict(count=count, pending=pending, loop_start=loop_start, loop_changed=loop_changed,
                after_pending=struct.unpack_from("<i", path)[0], after_loop_changed=path[12], nodes=rows)


def lookup(count, loop_start, loop_length, index):
    result = native.call(0x636E60, ecx=PATH, stack_args=[index & 0xFFFFFFFF],
                         writes=prestate(count, -1, loop_start, loop_length))
    pointer = result["eax"]
    return dict(count=count, loop_start=loop_start, loop_length=loop_length, index=index,
                node=None if pointer == 0 else (pointer - PATH - 0x5000) // 0x100)


def consume_loop(pending):
    writes = prestate(3, pending, 0, 3)
    for i in range(3):
        node = PATH + 0x5000 + i * 0x100
        record = PATH + 0xA000 + i * 0x100
        writes[node + 4] = words(PATH + 0xB000 + i * 0x100)
        writes[node + 0x10] = words(1)
        writes[node + 0x1C] = b"\x01"  # Already queued for native cleanup; no enqueue needed.
        writes[PATH + 0xB000 + i * 0x100] = words(record)
        writes[record] = words(ACTOR, EVENT + i * 0x100, -1, 0)
        writes[EVENT + i * 0x100] = bytes([0x40 + i]) * 0x6F
    result = native.call(0x636590, ecx=PATH, stack_args=[OUTPUT], writes=writes,
                         dumps={"nodes": (NODES, 12), "path": (PATH + 0x8C, 14),
                                "event": (OUTPUT, 0x6F), "retained_event": (PATH + 0x1D, 0x6F)})
    path = bytes.fromhex(result["dumps"]["path"])
    pointers = struct.unpack("<3I", bytes.fromhex(result["dumps"]["nodes"]))
    return dict(pending=pending, after_pending=struct.unpack_from("<i", path)[0],
                loop_start=struct.unpack_from("<i", path, 8)[0], rotated=bool(path[13]),
                nodes=[(pointer - PATH - 0x5000) // 0x100 for pointer in pointers],
                event=result["dumps"]["event"], retained_event=result["dumps"]["retained_event"])


def dispatch_latch(flag):
    writes = prestate(3, 1, 0, 3)
    writes[ACTOR + 0x514] = words(PATH)
    writes[PATH + 0x1C] = b"\x01"
    writes[EVENT + 0x1D] = bytes([flag])
    result = native.call(0x6386E0, ecx=ACTOR, edx=EVENT, writes=writes,
                         dumps={"path": (PATH + 0x8C, 14), "latch": (PATH + 0x1C, 1),
                                "count": (PATH + 0x14, 4)})
    path = bytes.fromhex(result["dumps"]["path"])
    return dict(flag=flag, count=int.from_bytes(bytes.fromhex(result["dumps"]["count"]), "little"),
                pending=struct.unpack_from("<i", path)[0], loop_start=struct.unpack_from("<i", path, 8)[0],
                executing=bool(int(result["dumps"]["latch"], 16)))


def patrol_conversion(loop_start, capabilities, missions):
    """639CB0 mutates all event memberships, gated by first-node actors."""
    writes = prestate(len(missions), -1, loop_start, len(missions) - loop_start)
    writes[ACTOR + 0x514] = words(PATH)
    dumps = {}
    for member, capable in enumerate(capabilities):
        actor = ACTOR + member * 0x800
        vtable = PATH + 0xC000 + member * 0x800
        stub = PATH + 0xF000 + member * 0x10
        writes[actor] = words(vtable)
        writes[vtable + 0x4C0] = words(stub)
        writes[stub] = b"\xb8" + words(int(capable)) + b"\xc3"
    for i, row in enumerate(missions):
        node = PATH + 0x5000 + i * 0x100
        members = PATH + 0xB000 + i * 0x100
        writes[node + 4] = words(members)
        writes[node + 0x10] = words(len(row))
        for member, mission in enumerate(row):
            record = PATH + 0x6000 + i * 0x400 + member * 0x100
            event = record + 0x40
            writes[members + member * 4] = words(record)
            writes[record] = words(ACTOR + member * 0x800, event)
            writes[event] = bytes([0xA5]) * 0x6F
            writes[event + 0xC] = bytes([mission])
            dumps[f"event{i}_{member}"] = (event, 0x6F)
    result = native.call(0x639CB0, ecx=ACTOR, writes=writes, dumps=dumps,
                         required_addresses=[0x639D13, 0x639D97] if any(capabilities) else [0x639D13])
    after = []
    for i, row in enumerate(missions):
        converted = []
        for member, mission in enumerate(row):
            event = bytes.fromhex(result["dumps"][f"event{i}_{member}"])
            assert event[:0xC] + event[0xD:] == bytes([0xA5]) * 0x6E
            converted.append(event[0xC])
        after.append(converted)
    return dict(loop_start=loop_start, capabilities=capabilities, missions=missions, after=after)


def commit_admission(mission, target_kind):
    vtable, target, target_vtable = PATH + 0xC000, PATH + 0xD000, PATH + 0xD400
    mission_stub, kind_stub, id_stub = PATH + 0xF000, PATH + 0xF010, PATH + 0xF020
    writes = {ACTOR: words(vtable), vtable + 0x184: words(mission_stub),
              mission_stub: b"\xb8" + words(mission) + b"\xc3",
              ACTOR + 0x2B4: words(0 if target_kind == "none" else target),
              target: words(target_vtable, target_vtable + 0x100),
              target_vtable + 0x2C: words(kind_stub),
              target_vtable + 0x110: words(id_stub),
              kind_stub: b"\xb8" + words(11 if target_kind == "cell" else 3) + b"\xc3",
              id_stub: b"\xb8" + words(17) + b"\xc2\x04\x00",
              target + 0x24: struct.pack("<2h", 10, 12)}
    result = native.call(0x638C70, ecx=ACTOR, writes=writes)
    return dict(mission=mission, target_kind=target_kind, admit=bool(result["eax"] & 255))


def interpolate_rgb(source, target, amount):
    result = native.call(0x6612C0, ecx=PATH, stack_args=[amount, PATH + 16],
                         writes={PATH: bytes(source), PATH + 16: bytes(target)},
                         dumps={"rgb": (PATH, 3)},
                         required_addresses=[0x6612C0, 0x6612ED, 0x66133A])
    return dict(source=source, target=target, amount=amount,
                rgb=list(bytes.fromhex(result["dumps"]["rgb"])))


def stroke_controls():
    """Execute both native width wrappers on the existing memory surface."""
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EDX, UC_X86_REG_ESP
    from tools.procedural_drawing_oracle import rally

    rows = []
    for start, end in [([20, 30], [120, 30]), ([40, 20], [40, 100]),
                       ([20, 20], [100, 100]), ([120, 30], [25, 85])]:
        for width, ms, selected in [(4, 0, True), (4, 32, True), (3, 198, True), (4, 242, False)]:
            f = rally.Rally({})
            u = f.u
            u.mem_write(0x88731C, words(rally.SURFACE))
            #72AD20 subtracts the168px sidebar and32px command bar even
            # with A8EB7C set. Supply the shell extent so marker/badge
            # clipping has the same160x120 tactical extent as the strokes.
            u.mem_write(0x886FB0, words(0, 0, rally.SIZE[0] + 168, rally.SIZE[1] + 32))
            u.mem_write(0xA8EB7C, b"\x01")
            clock = rally.MEM + 0x1FF000
            u.mem_write(0x7E1530, words(clock))

            def observe(vm, pc, _size, _data):
                if pc == clock:
                    sp = vm.reg_read(UC_X86_REG_ESP)
                    vm.reg_write(UC_X86_REG_EAX, ms)
                    vm.reg_write(UC_X86_REG_ESP, sp + 4)
                    from unicorn.x86_const import UC_X86_REG_EIP
                    vm.reg_write(UC_X86_REG_EIP, int.from_bytes(vm.mem_read(sp, 4), 'little'))

            u.hook_add(UC_HOOK_CODE, observe)
            outer = 0xFFFF if selected else 0
            u.reg_write(UC_X86_REG_EDX, outer)
            rally.call(u, 0x63D910, width, (*start, *end))
            u.reg_write(UC_X86_REG_EDX, 0x9F73)
            rally.call(u, 0x63D960, width, (*start, *end))
            # Original actor marker outer6x6, inset4x4. Node outer8x8
            # and inset6x6 are separately established by635350/635490.
            rally.call(u, 0x63D400, outer, end)
            rally.call(u, 0x63D480, 0x9F73, end)
            rows.append(dict(start=start, end=end, width=width, ms=ms,
                             selected=selected, inner_rgb565=0x9F73, pixels=f.pixels()))
    return rows


def generate():
    return dict(schema=1, commits=[commit(3, p, l, changed) for p, l, changed in
                                  [(-1, -1, False), (0, -1, False), (1, -1, False), (2, 0, True), (-1, 1, True)]],
                lookups=[lookup(3, l, length, i) for l, length in [(-1, -1), (0, 3), (1, 2)]
                         for i in [-1, 0, 1, 2, 3, 4, 6]],
                loop_consumes=[consume_loop(p) for p in [-1, 0, 2]],
                dispatch_latches=[dispatch_latch(flag) for flag in [1, 2]],
                patrol_conversions=[patrol_conversion(start, capabilities, missions) for start, capabilities, missions in [
                    (0, [True], [[2], [2], [2]]),
                    (1, [True], [[2], [2], [11], [29]]),
                    (1, [False, True], [[2, 2], [2, 11], [29, 2], [2, 2]]),
                    (0, [False, False], [[2, 2], [2, 11], [29, 2]]),
                ]],
                commit_admissions=[commit_admission(mission, target) for mission in [1, 2, 5, 11, 28]
                                   for target in ["none", "cell", "object"]],
                render=dict(stroke_controls=stroke_controls(), rgb_controls=[interpolate_rgb(source, target, amount)
                    for source, target, amount in [
                        ([1, 127, 255], [0, 0, 0], 80),
                        ([1, 127, 255], [255, 255, 255], 100),
                        ([255, 128, 1], [0, 0, 0], 80),
                        ([255, 128, 1], [255, 255, 255], 100),
                        ([17, 63, 249], [201, 3, 89], 80),
                        ([17, 63, 249], [201, 3, 89], 100),
                        ([17, 63, 249], [201, 3, 89], 356),
                    ]]))


def local_defeat_cases():
    """Input histories; all activation, exit and queue decisions run natively."""
    return [
        dict(name="active_local_defeat_then_blocked_edges", active=True,
             session300=0, house_index=3, listener=True,
             operations=["house_defeat_tail", "keyboard_press", "keyboard_release",
                         "direct_press", "direct_release"]),
        dict(name="inactive_local_defeat_preserves_references", active=False,
             session300=0, house_index=0, listener=False,
             operations=["house_defeat_tail", "direct_release"]),
        dict(name="ordinary_registered_keyboard_edges", active=False,
             session300=0, house_index=7, listener=True,
             operations=["keyboard_press", "keyboard_press", "keyboard_release",
                         "keyboard_release"]),
        dict(name="supplied_defeated_active_release", active=True,
             session300=1, house_index=1, listener=False,
             supplied_control="An already-defeated active prior exercises the release entry; no normal producer is claimed.",
             operations=["direct_press", "direct_release", "direct_release"]),
    ]


def local_defeat_inputs():
    """Physical lexical evidence only; resolved protocol sound IDs are inputs."""
    import hashlib
    import os
    from tools.rules_oracle.bridge_child_sound import sections

    root = Path(os.environ["VERA20K_PLANNING_DEFEAT_ASSETS"])
    keys = ("StartPlanningModeSound", "EndPlanningModeSound", "SavourDelay")
    layers = []
    for filename in ("RULESMD.INI", "LANGRULE.INI", "MPBattleMD.ini", "Hills.map"):
        path = root / filename
        if not path.is_file():
            assert filename == "LANGRULE.INI", path
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        audio = sections(raw).get("AudioVisual", {})
        values = {key: audio[key] for key in keys if key in audio}
        layers.append(dict(file=filename, absent=False, sha256=hashlib.sha256(raw).hexdigest(),
                           audio_visual=values))
    # This profile selects the existing physical Battle/Hills installation;
    # it does not introduce another layered INI or sound-reference reader.
    root_values = layers[0]["audio_visual"]
    assert all(not row.get("audio_visual") for row in layers[1:])
    sound_raw = (root / "SOUNDMD.INI").read_bytes()
    sound = sections(sound_raw)
    bindings = {}
    for role, key, identity in (("start", keys[0], 21), ("end", keys[1], 22)):
        name = root_values[key]
        assert name in sound["SoundList"].values() and name in sound
        bindings[role] = dict(key=key, name=name, fixture_id=identity,
                              physical_definition=sound[name])
    return dict(layers=layers, sound_bindings=bindings,
                sound_file=dict(file="SOUNDMD.INI", sha256=hashlib.sha256(sound_raw).hexdigest()))


def local_defeat_case(case, inputs):
    """Compose the existing Selection/Keyboard VM and original planning code."""
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
                                   UC_X86_REG_ESI, UC_X86_REG_EBP)
    from tools.input_oracle.selection_navigation import SelectionFixture
    from tools.input_oracle.fast_scroll import DISPLAY, return_from_sink

    m = SelectionFixture([], [[], [], [], [], []], seed=31)
    u = m.uc
    m.put(0xA8B238, 5)
    house = m.houses["local"]
    m.put(house + 0x30, case["house_index"])
    u.mem_write(house + 0x1F5, bytes([case["session300"] != 0 or
                                    "house_defeat_tail" in case["operations"]]))
    m.put(0xA8B538, case["session300"])
    u.mem_write(0xAC4CF4, bytes([case["active"]]))
    m.put(0xAC4BF4, 1)
    m.put(0xAC4BF8, 1)
    m.put(0xAC4BFC, 1)  # settled native first-use hint counters.
    m.put(0xA802C8, 0)
    m.put(0xA802CC, 0)
    m.put(0xA802D0, 0)
    m.put(0x7E1530, m.alloc(16))
    clock = m.get(0x7E1530)
    rules = m.alloc(0x2000)
    m.put(0x8871E0, rules)
    # Actual constructor stores, with its -1 constant in EBP. The runtime
    # sound catalog bindings below are explicit fixture transport inputs.
    u.reg_write(UC_X86_REG_ESI, rules)
    u.reg_write(UC_X86_REG_EBP, 0xFFFFFFFF)
    native.run_checked(u, 0x6658F2, 0x66590A,
                       required_addresses=(0x6658F2, 0x665904))
    constructor_sounds = {role: signed(m.get(rules + offset))
                          for role, offset in (("start", 0x1D4), ("end", 0x1E0))}
    for role, offset in (("start", 0x1D4), ("end", 0x1E0)):
        m.put(rules + offset, inputs["sound_bindings"][role]["fixture_id"])
    for pointer in m.rngs.values():
        m.invoke(0x65C6D0, pointer, (31,))
    refs = {name: m.alloc(16) for name in ("hover", "connect", "highlight")}
    ref_fields = dict(hover=0xAC4CCC, connect=0xAC4C38, highlight=0xAC4BF0)
    ref_ids = {pointer: index + 1 for index, pointer in enumerate(refs.values())}
    for name, address in ref_fields.items():
        m.put(address, refs[name])
    u.mem_write(house + 0x298, words(-1, 0, 0))
    u.mem_write(0xB0EA80, words(95, 0, 25))
    m.put(0xA8ED84, 100)
    # An optional UI observer is a transport, not a fabricated native gadget.
    listener, listener_vtable, listener_sink = m.alloc(16), m.alloc(16), m.alloc(16)
    m.put(listener, listener_vtable)
    m.put(listener_vtable, listener_sink)
    m.put(0xAC4B80, listener if case["listener"] else 0)
    command = m.objects["PlanningMode"]
    # Original PlanningMode vtable7EB9B4 has Execute at+20/7EB9D4;
    # +14 is the separate536710 metadata getter, not execution.
    keyboard_execute = m.get(m.get(command) + 0x20)
    assert keyboard_execute == 0x536750
    trace = native.NativeCallTrace(u, m.get)
    effects, imports, rng_requests, raw_advances, detach = [], [], [], [], []
    specs = {
        0x637A10: ("planning_exit", 0, 0), 0x6379C0: ("planning_enter", 0, 0),
        0x731A50: ("planning_press", 0, 0), 0x731A70: ("planning_release", 0, 0),
        0x536750: ("registered_keyboard", 1, 4),
        0x63A120: ("first_use_hint", 0, 0), 0x4AC960: ("beacon_mode", 1, 4),
        0x4C66C0: ("empty_event_constructor", 2, 8),
        0x6521C0: ("out_list_enqueue", 0, 0x70),
        0x750920: ("centered_sound_request", 2, 8),
        listener_sink: ("optional_ui_notification", 2, 8),
    }
    sound_ids = {row["fixture_id"]: role for role, row in inputs["sound_bindings"].items()}

    def hook(vm, pc, _size, _data):
        sp = vm.reg_read(UC_X86_REG_ESP)
        trace.returned(pc, sp)
        if pc in specs:
            trace.entered(pc, sp, specs[pc])
        if pc == 0x6521C0:
            effects.append(dict(event="commit_enqueue_attempt", opcode=vm.mem_read(sp + 4, 1)[0],
                                issuer=struct.unpack("<b", vm.mem_read(sp + 6, 1))[0],
                                frame=signed(m.get(sp + 7))))
        elif pc == 0x750920:
            identity = signed(vm.reg_read(UC_X86_REG_ECX))
            effects.append(dict(event="sound", fixture_id=identity, role=sound_ids.get(identity),
                                pan=vm.reg_read(native.UC_X86_REG_EDX),
                                volume_f32_hex=bytes(vm.mem_read(sp + 4, 4)).hex(),
                                handle=m.get(sp + 8)))
            return_from_sink(vm, 8)
        elif pc == listener_sink:
            effects.append(dict(event="optional_ui_notification", message=m.get(sp + 4),
                                data=m.get(sp + 8)))
            return_from_sink(vm, 8)
        elif pc == clock:
            imports.append(dict(event="OS_timeGetTime", result=100000))
            return_from_sink(vm, 0, 100000)
        elif pc == 0x4068E0:
            imports.append(dict(event="diagnostic_log", entry=f"{pc:08X}"))
            return_from_sink(vm, 0)
        elif pc in (0x65C780, 0x65C7E0):
            rng_requests.append(dict(entry=f"{pc:08X}", receiver=vm.reg_read(UC_X86_REG_ECX)))
        elif pc in (0x65C79D, 0x65C84B):
            raw_advances.append(dict(entry=f"{pc:08X}"))
        elif pc in (0x70D4A0, 0x7258D0):
            detach.append(dict(entry=f"{pc:08X}"))

    observed_fields = {0xAC4CF4: "active", 0xA8B538: "session300"} | {
        address: name for name, address in ref_fields.items()}

    def write(vm, _access, address, size, value, _data):
        if address in observed_fields:
            effects.append(dict(event="state_write", field=observed_fields[address],
                                value=value, bytes=size, entry=f"{vm.reg_read(UC_X86_REG_EIP):08X}"))

    u.hook_add(UC_HOOK_CODE, hook)
    u.hook_add(UC_HOOK_MEM_WRITE, write, begin=0xA8B538, end=0xA8B53B)
    u.hook_add(UC_HOOK_MEM_WRITE, write, begin=0xAC4BF0, end=0xAC4CF4)

    def rng():
        return {name: raw.hex() for name, raw in m.rng_bytes().items()}

    def state():
        count, head = m.get(0xA802C8), m.get(0xA802CC)
        events = []
        for index in range(count):
            pointer = 0xA802D4 + ((head + index) & 127) * 0x6F
            events.append(dict(opcode=u.mem_read(pointer, 1)[0],
                               issuer=struct.unpack("<b", u.mem_read(pointer + 2, 1))[0],
                               frame=signed(m.get(pointer + 3))))
        return dict(active=bool(u.mem_read(0xAC4CF4, 1)[0]), session300=m.get(0xA8B538),
                    **{name: ref_ids.get(m.get(address)) for name, address in ref_fields.items()},
                    queue_count=count, events=events,
                    house_timer=list(struct.unpack("<3i", u.mem_read(house + 0x298, 12))),
                    action_line_timer=list(struct.unpack("<3i", u.mem_read(0xB0EA80, 12))))

    initial, before, phases = state(), rng(), []
    for index, operation in enumerate(case["operations"]):
        frame = 100 + index
        m.put(0xA8ED84, frame)
        effect_start, import_start, call_start = len(effects), len(imports), len(trace.calls)
        if operation == "house_defeat_tail":
            # Exact local branch entered after the preceding House admissions;
            # stop before MapReveal577F30, with its one argument still pushed.
            u.mem_write(m.stack - 0x1000, bytes(0x1800))
            u.reg_write(UC_X86_REG_ESP, m.stack)
            u.reg_write(UC_X86_REG_ESI, house)
            native.run_checked(u, 0x4FC1EE, 0x4FC20F,
                               required_addresses=(0x637A10, 0x4AC960, 0x4FC205))
            assert u.reg_read(UC_X86_REG_ESP) == m.stack - 4
        elif operation.startswith("keyboard_"):
            m.invoke(keyboard_execute, command, (0x800 if operation == "keyboard_release" else 0,))
        elif operation.startswith("direct_"):
            m.invoke(0x731A70 if operation == "direct_release" else 0x731A50, 1)
        else:
            raise ValueError(operation)
        trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        assert not trace.pending and not m.trace.pending
        assert m.text_hash() == m.code_hash
        phases.append(dict(input=operation, frame=frame, state=state(),
                           effects=effects[effect_start:], imports=imports[import_start:],
                           calls=trace.calls[call_start:], rng=rng()))
    return dict(input=case, initial=initial, constructor_sounds=constructor_sounds,
                reference_storage=refs, registered_command=command, rng_before=before,
                phases=phases, rng_after=rng(), rng_requests=rng_requests,
                raw_advances=raw_advances, detach=detach, original_text_unchanged=True)


def local_defeat_generate():
    assert native.image_sha256() == native.NATIVE_SHA256, "This profile requires the current sealed original image"
    inputs = local_defeat_inputs()
    return dict(schema_version=1, source="unicorn/gamemd.exe", native_sha256=native.image_sha256(),
                seed=31, inputs=inputs,
                cases=[local_defeat_case(case, inputs) for case in local_defeat_cases()])


def local_defeat_metadata():
    return native.provenance(
        scope="Local House defeat tail4FC1EE..4FC20F, whole planning exit637A10 and registered keyboard/direct731A50/731A70 admission with actual event header and OutList enqueue",
        assumptions=[
            "Reuses SelectionFixture/KeyboardFixture, their original532150 command registration and runtime containers, NativeCallTrace, native RNG seeding and call runner; no additional VM, INI parser, planning decision or RNG implementation.",
            "The House and preceding local/nonpassive/noncampaign/count admissions are supplied scene state. Defeat controls start after the native Defeated flag store, execute637A10 and4AC960 plus actual Session+300 store, and stop BEFORE577F30 map reveal. Beacon is inactive and placement absent. Map/radar/result flags, SavourDelay countdown, full HouseUpdate and whole frame scheduler are not emulated.",
            "Three original65C6D0 seeds31 initialize the retained Main/Scenario/MapGen buffers before each history. All1012 bytes remain continuous across phases; frame labels advance only the supplied global. Constructor draws before these priors and downstream audio draws are outside scope.",
            "Original Rules constructor sound-default stores6658F2..66590A execute with its-1 EBP constant. Resolved runtime sound IDs21/22 are explicit fixture transport bindings to physical StartPlanningModeSound/EndPlanningModeSound names, NOT native global catalog indices or an execution of the Rules sound reader. Existing native reader identities6696C0/66970A and production retail-reader tests establish that separate owner. Physical RULESMD, absent LANGRULE, MPBattleMD, Hills.map and unlayered SOUNDMD lexical inputs/hashes are retained using the existing sections owner.",
            "Null actor/layer/selection containers, empty capacity128 OutList and nonzero display reference priors are supplied. No other paths execute; these records certify event issuance/current House index and cleanup, not post-Logic dispatch timing. MainTick55DC9E/55DE40 and QueueAI6474DC timing remains instruction-level evidence plus the connected production regression.",
            "The first-use planning hint counters are supplied settled1 so actual63A120 declines tutorials. The already-defeated active prior is an explicit receiver control; no ordinary producer is claimed. Original .text remains unchanged."],
        substitutions=[
            "Diagnostic4068E0 returns0 with cdecl caller cleanup; original event constructor4C66C0 writes its actual header. Original6521C0 performs the real111-byte OutList copy, capacity check, counters and imported timeGetTime timestamp; the Windows clock import returns100000.",
            "Centered Voc750920 is observed then returns0/RET8 before playback/device/pool admission and downstream audio RNG. Optional AC4B80 listener dispatch is observed at a supplied callback address and returns0/RET8; no native gadget paint/behavior is supplied or certified. Existing Mouse virtual cursor transports remain installed but unreached by inactive beacon controls.",
            "No planning active/Session gate, event/opcode/issuer, state clear, queue or timer return is supplied. Event payload beyond its constructed header is caller scratch, not a gameplay claim. No scheduler decision is answered."],
        entry_points=dict(house_local_tail=0x4FC1EE, stop_before_reveal=0x4FC20F,
                          planning_exit=0x637A10, planning_enter=0x6379C0,
                          planning_press=0x731A50, planning_release=0x731A70,
                          keyboard_execute=0x536750, first_use_hint=0x63A120,
                          event_constructor=0x4C66C0, enqueue=0x6521C0,
                          beacon_mode=0x4AC960, random_seed=0x65C6D0,
                          centered_sound=0x750920, sound_default_stores=0x6658F2))


if __name__ == "__main__":
    import sys
    if "--local-defeat" in sys.argv[1:]:
        args = list(sys.argv[1:])
        args.remove("--local-defeat")
        root = Path(__file__).resolve().parents[2]
        native.finish_vectors(local_defeat_generate,
            Path(__file__).with_name("planning_local_defeat.json"),
            provenance=local_defeat_metadata, argv=args,
            source_paths={name: root / name for name in (
                "tools/spatial_oracle/planning_mode.py", "tools/native_oracle.py",
                "tools/input_oracle/selection_navigation.py", "tools/input_oracle/fast_scroll.py",
                "tools/storage_oracle/keyboard_bindings.py", "tools/sidebar_oracle/stock.py",
                "tools/rmg_oracle/gen_rng_vectors.py", "tools/rules_oracle/bridge_child_sound.py")})
        raise SystemExit(0)
    native.finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: native.provenance(
            scope="Five commits, 21 path lookups, three loop-head consumptions, two event-dispatch latch controls, four patrol conversions, 15 commit mission/target admission controls, seven RGB controls and16 composed planning stroke/marker controls.",
            assumptions=["Supplied actor, path, node, display predecessor and event records; original constructors do not execute.",
                         "Loop consumption uses three nodes with existing cleanup latches, enough vector capacity and loop start zero; nonloop destruction and whole-game scheduling are not covered."],
            substitutions=["Commit admission uses supplied actor mission, target class and target identity virtual getters; original 638C70 and target-token construction execute unchanged.",
                           "Patrol conversion executes original639CB0 with supplied path/memberships and Can_Attack_Move virtual return values. All event bytes other than the mission are checked unchanged; local feedback latch and whole-game scheduling are outside coverage.",
                           "Stroke controls reuse Rally's BSurface locking/storage with original7BA8C0 patterned and7BA610 solid pixels. Imported clock returns the supplied ms;63D910/63D960 and63D400/63D480 execute unchanged. Width/selection/endpoints are supplied, not whole-manager admission."],
            entry_points={"commit": 0x636CE0, "lookup": 0x636E60, "consume": 0x636590, "dispatch_latch": 0x6386E0, "patrol_conversion": 0x639CB0, "commit_admission": 0x638C70, "interpolate_rgb": 0x6612C0}),
        source_paths={"planning_mode": Path(__file__), "native_oracle": Path(native.__file__)})
