"""Machine-derived RNG vectors: seeded state, raw and mixed ranged histories.

Run with ``python -m tools.rmg_oracle.gen_rng_vectors`` (read-only check by default).
Runs the real gamemd.exe routines under Unicorn (see tools.native_oracle):
  Random__Seed 0x0065C6D0  __thiscall(this=ECX, seed=stack) -> fills this+0xC..
  Random__Next 0x0065C780  __thiscall(this=ECX) -> EAX, mutates state in place
  Random__RandomRanged 0x0065C7E0  __thiscall(this=ECX, low, high) -> signed EAX

The original seed/raw cases retain their schema. Added ranged cases execute
mixed calls with full state retained; observe raw words at the actual native
stores (65C79D/65C84B), including RandomRanged's inlined draw and rejection.
No expected-value reduction or returned-function substitution is used.
"""

import struct
from functools import lru_cache
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools import native_oracle as native
from tools.spatial_oracle.native_id_snapshot import ScenarioFixture, SCENARIO
from tools.storage_oracle.shell_relayout import Image

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    call, finish_vectors, load_image, provenance, run_checked,
)

SEED_FN = 0x0065C6D0
NEXT_FN = 0x0065C780
RANGED_FN = 0x0065C7E0
STRUCT = SCRATCH
STRUCT_LEN = 0xC + 250 * 4  # locked/idx_a/idx_b + 250 state dwords = 0x3F4

SEEDS = (0, 1, 1234, 0x7FFF, 0xFFFF)
DRAWS_PER_SEED = 16
RNGS = {'scenario': SCENARIO + 0x218, 'main': 0x886B88, 'mapgen': 0xABE890}
STUB = 0x50000000
SYSTEMTIME = [2026, 10, 6, 10, 19, 27, 43, 123]
SEED_IN = 0x13579BDF


def seeded_struct(seed: int) -> bytes:
    """Run Random__Seed and return the full 0x3F4-byte generator struct."""
    result = call(
        SEED_FN,
        ecx=STRUCT,
        stack_args=[seed],
        dumps={"s": (STRUCT, STRUCT_LEN)},
    )
    return bytes.fromhex(result["dumps"]["s"])


def draws(state: bytes, count: int) -> tuple[list[int], bytes]:
    """Chain `count` calls to Random__Next, carrying the struct forward."""
    current, values = state, []
    for _ in range(count):
        result = call(
            NEXT_FN,
            ecx=STRUCT,
            writes={STRUCT: current},
            dumps={"s": (STRUCT, STRUCT_LEN)},
        )
        values.append(result["eax"])
        current = bytes.fromhex(result["dumps"]["s"])
    return values, current


def constrained_state(seed: int, constraints: dict[int, int]) -> dict:
    """Prepare explicit state-word controls, then verify unchanged original Next.

    Only the early independent XOR lanes are admitted. This is input generation,
    not a substitute generator: seeded_struct and draws execute original code.
    """
    original = seeded_struct(seed)
    state = bytearray(original)
    a, b = struct.unpack_from('<II', state, 4)
    count = max(constraints) + 1
    assert 0 < count <= 32 and min(constraints) >= 0
    left = {(a + n) % 250 for n in range(count)}
    right = {(b + n) % 250 for n in range(count)}
    assert not left & right, 'Only independent early XOR lanes are supported'
    changes = []
    for n, word in sorted(constraints.items()):
        index = (a + n) % 250
        other = struct.unpack_from('<I', state, 12 + 4 * ((b + n) % 250))[0]
        value = other ^ word
        changes.append(dict(raw_index=n, state_word=index,
                            before=struct.unpack_from('<I', state, 12 + 4 * index)[0],
                            after=value, required_raw=word))
        struct.pack_into('<I', state, 12 + 4 * index, value)
    values, after = draws(bytes(state), count)
    assert all(values[n] == word for n, word in constraints.items())
    return dict(source_seed=seed, seeded_state_hex=original.hex(),
                word_changes=changes, before_state_hex=bytes(state).hex(),
                native_draws=values, after_verification_state_hex=after.hex())


def ranged_history(seed: int, advance_raw: int = 0) -> dict:
    """Execute unchanged original routines, observing results and draw stores.

    The fixture supplies the RNG object address and native-produced seed bytes.
    A raw prefix tests either cursor wrapping. Prefix calls execute but are not
    repeated as step receipts; initial_state_hex is their actual continuation.
    """
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.mem_write(STRUCT, seeded_struct(seed))
    original_code = bytes(uc.mem_read(NEXT_FN, 0x65C88D - NEXT_FN))
    raw_words = []

    def observe(machine, address, _size, _user):
        # Both routines store the unmasked XOR result from ESI. Count this
        # instruction rather than assuming one draw or deriving it from EAX.
        if address in (0x65C79D, 0x65C84B):
            raw_words.append(machine.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF)

    uc.hook_add(UC_HOOK_CODE, observe)
    sp = STACK_BASE + STACK_SIZE - 0x1000

    def request(bounds):
        raw_words.clear()
        before = bytes(uc.mem_read(STRUCT, STRUCT_LEN))
        function = NEXT_FN if bounds is None else RANGED_FN
        stack = [RET_MAGIC] + ([] if bounds is None else list(bounds))
        uc.mem_write(sp, struct.pack("<" + "I" * len(stack),
                                     *(value & 0xFFFFFFFF for value in stack)))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, STRUCT)
        required = [function, 0x65C7D0 if bounds is None else 0x65C88A]
        if bounds is not None and bounds[0] != bounds[1]:
            required.append(0x65C84B)
        run_checked(uc, function, RET_MAGIC, count=10000,
                    required_addresses=required,
                    context=dict(seed=seed, advance_raw=advance_raw, bounds=bounds))
        assert uc.reg_read(UC_X86_REG_ESP) == sp + len(stack) * 4
        result = uc.reg_read(UC_X86_REG_EAX) & 0xFFFFFFFF
        row = dict(kind="raw" if bounds is None else "ranged",
                   result=result if bounds is None else struct.unpack("<i", struct.pack("<I", result))[0],
                   raw_draw_count=len(raw_words), raw_draws=raw_words.copy(),
                   before_state_hex=before.hex(),
                   after_state_hex=bytes(uc.mem_read(STRUCT, STRUCT_LEN)).hex())
        if bounds is not None:
            row.update(low=bounds[0], high=bounds[1])
        assert bytes(uc.mem_read(NEXT_FN, len(original_code))) == original_code
        return row

    for _ in range(advance_raw):
        request(None)
    # Seed1's first [0,4] request requires three draws: a useful distinction
    # from modulo reduction. Equal and reversed bounds share the live cursor.
    requests = ((0, 4), None, (0, 99), (-5, 5), None, (5, -5), (7, 7),
                None, (0, 99), (99, 0), (-5, -5), (0, 1), (0, 2), None)
    initial = bytes(uc.mem_read(STRUCT, STRUCT_LEN)).hex()
    return dict(id=f"seed_{seed}_after_{advance_raw}_raw", seed=seed,
                advance_raw=advance_raw, initial_state_hex=initial,
                steps=[request(bounds) for bounds in requests])


def put32(u, at, value):
    u.mem_write(at, struct.pack('<I', value & 0xFFFFFFFF))


def get32(u, at):
    return struct.unpack('<I', u.mem_read(at, 4))[0]


def signed32(u, at):
    return struct.unpack('<i', u.mem_read(at, 4))[0]


def ret(u, cleanup=0, eax=None):
    sp = u.reg_read(UC_X86_REG_ESP)
    if eax is not None:
        u.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    u.reg_write(UC_X86_REG_EIP, get32(u, sp))
    u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)


def invoke(u, function, *, ecx=0, args=(), required=(), sp=0x100DF000):
    u.mem_write(sp, struct.pack('<' + 'I' * (1 + len(args)), native.RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, ecx)
    native.run_checked(u, function, native.RET_MAGIC, count=2_000_000,
                       required_addresses=required, context={'entry': hex(function)})
    assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + len(args) * 4


def code_guard(u):
    return [(native.IMAGE_BASE + rva,
             bytes(u.mem_read(native.IMAGE_BASE + rva, size)))
            for rva, _raw, size, _virtual, flags in native._sections(native.image_bytes())
            if flags & 0x20000000 and size]


def check_code(u, guard):
    assert all(bytes(u.mem_read(at, len(data))) == data for at, data in guard)


def states(u):
    return {name: bytes(u.mem_read(at, STRUCT_LEN)).hex() for name, at in RNGS.items()}


def logical(raw):
    data = bytes.fromhex(raw) if isinstance(raw, str) else raw
    # Byte0 locked, dwords+4/+8 indices, then250 state words. Bytes1..3 are padding.
    return data[:1] + data[4:]


@lru_cache(maxsize=None)
def seeded(seed):
    return seeded_struct(seed)


@lru_cache(maxsize=1)
def supplied_states():
    out = {}
    for name, seed, count in [('scenario',1234,7), ('main',0x10203,3), ('mapgen',0,11)]:
        _values, current = draws(seeded(seed), count)
        out[name] = current
    return out


def scenario_loaded():
    """Execute the shared Scenario stream owner with native-produced RNG inputs.

    This is the common inner load boundary for both UI routes. Only the menu
    caller subsequently enters 52FC20; the in-game loop reachability is retained
    as instruction-level evidence in rng.md, not an invented loop execution.
    """
    fixture = ScenarioFixture()
    u = fixture.u
    for name, state in supplied_states().items():
        u.mem_write(RNGS[name], state)
    put32(u, 0xA8ED94, SEED_IN)
    put32(u, 0xA8B230, SCENARIO)
    before, guard, seed_calls = states(u), code_guard(u), []

    def observe(_u, pc, _size, _data):
        if pc == SEED_FN:
            seed_calls.append({'receiver': u.reg_read(UC_X86_REG_ECX),
                               'seed': get32(u, u.reg_read(UC_X86_REG_ESP) + 4)})

    u.hook_add(UC_HOOK_CODE, observe)
    identity = fixture.round_trip()
    after = states(u)
    assert logical(after['scenario']) == logical(seeded(0))
    assert after['main'] == before['main']
    assert after['mapgen'] == before['mapgen']
    assert get32(u, 0xA8ED94) == SEED_IN
    check_code(u, guard)
    return u, {'id_owner_result': identity, 'before': before, 'after': after,
               'global_seed_before': SEED_IN, 'global_seed_after': get32(u, 0xA8ED94),
               'native_seed_calls': seed_calls.copy(),
               'scenario_equals_native_seed0': True,
               'main_retained_raw': True, 'mapgen_retained_raw': True}


RNG_CASES = [
    dict(id='mode0_clock', mode=0, tick=0x12345678, override=0, gate_a=0, gate_flags=0, gate_secondary=0),
    dict(id='mode5_clock_high_bit', mode=5, tick=0x89ABCDEF, override=0, gate_a=0, gate_flags=0, gate_secondary=0),
    dict(id='mode5_clock_zero', mode=5, tick=0, override=0, gate_a=0, gate_flags=0, gate_secondary=0),
    dict(id='mode5_override', mode=5, tick=0x12345678, override=0xFEDCBA98, gate_a=0, gate_flags=0, gate_secondary=0),
    dict(id='gate_a_preserves_seed', mode=5, tick=0x12345678, override=0xFEDCBA98, gate_a=1, gate_flags=0, gate_secondary=0),
    dict(id='gate_flag2_preserves_seed', mode=5, tick=0x12345678, override=0xFEDCBA98, gate_a=0, gate_flags=2, gate_secondary=0),
    dict(id='gate_secondary1_preserves_seed', mode=5, tick=0x12345678, override=0xFEDCBA98, gate_a=0, gate_flags=0, gate_secondary=1),
    dict(id='gate_secondary2_preserves_seed', mode=5, tick=0x12345678, override=0xFEDCBA98, gate_a=0, gate_flags=0, gate_secondary=2),
    dict(id='mode1_preserves_seed_regular_arm', mode=1, tick=0x12345678, override=0xFEDCBA98, gate_a=0, gate_flags=0, gate_secondary=0),
]


def menu_rng_case(case):
    u, loaded = scenario_loaded()
    guard = code_guard(u)
    # Actual constructor, not a hand-written RandomStraw buffer or patched body.
    invoke(u, 0x6616B0, ecx=0xA8E7B0, required=(0x6616B0,0x65C890,0x6616FF))
    straw_before = bytes(u.mem_read(0xA8E7B0, 0x114)).hex()
    u.mem_map(STUB, 0x1000)
    imports = Image(u).imports()
    assert imports[0x7E10D4] == 'GetSystemTime'
    assert imports[0x7E1138] == 'GetTickCount'
    put32(u, 0x7E10D4, STUB)
    put32(u, 0x7E1138, STUB + 0x10)
    for at, value in [(0xA8B238,case['mode']), (0xA8ED98,case['override']),
                      (0xA8B8B8,case['gate_a']), (0xAA0444,case['gate_secondary'])]:
        put32(u, at, value)
    u.mem_write(0xA8D5F8, bytes([case['gate_flags']]))
    events = []
    writes = {name: [] for name in RNGS}

    def hook(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == STUB:
            pointer = get32(u, sp + 4)
            u.mem_write(pointer, struct.pack('<8H', *SYSTEMTIME))
            events.append({'event':'GetSystemTime', 'supplied': SYSTEMTIME})
            ret(u,4,0)
        elif pc == STUB + 0x10:
            events.append({'event':'GetTickCount', 'supplied':case['tick']})
            ret(u,0,case['tick'])
        elif pc == 0x65C6D0:
            events.append({'event':'native_seed', 'seed':get32(u,sp+4),
                           'receiver':u.reg_read(UC_X86_REG_ECX)})
        elif pc in (0x661850,0x661770,0x661C10,0x65C8B0):
            events.append({'event':'RandomStraw_original', 'entry':hex(pc)})
        elif pc in (0x52FE31,0x52FE51,0x52FE80,0x52FEAB):
            # REP invokes hooks repeatedly; one call-site marker is enough.
            if not events or events[-1] != {'event':'native_copy','entry':hex(pc)}:
                events.append({'event':'native_copy','entry':hex(pc)})

    def write_hook(_u, _access, at, size, _value, _data):
        for name, begin in RNGS.items():
            if at < begin + STRUCT_LEN and begin < at + size:
                writes[name].append({'pc':hex(u.reg_read(UC_X86_REG_EIP)),
                                     'offset':at-begin,'size':size})

    u.hook_add(UC_HOOK_CODE,hook)
    u.hook_add(UC_HOOK_MEM_WRITE,write_hook)
    before = states(u)
    sp = 0x100BF000
    u.reg_write(UC_X86_REG_ESP,sp)
    u.mem_write(sp+0x13,b'\x01')
    alternate = bool(case['gate_a'] or case['gate_flags'] & 2 or case['gate_secondary'])
    copies = (0x52FE80,0x52FEAB) if alternate else (0x52FE31,0x52FE51)
    stop = native.run_checked(u,0x52E619,0x52E61E,count=2_000_000,
                             required_addresses=(0x52E619,0x52FC20,0x65C6D0,*copies),
                             context=case)
    assert u.reg_read(UC_X86_REG_ESP) == sp
    after = states(u)
    actual_seed = get32(u,0xA8ED94)
    expected_source = (SEED_IN if alternate or case['mode'] not in (0,5)
                       else case['override'] or case['tick'])
    assert actual_seed == expected_source
    exact_seed = seeded(actual_seed)
    assert logical(after['scenario']) == logical(exact_seed)
    assert logical(after['main']) == logical(exact_seed)
    assert before['mapgen'] == after['mapgen']
    assert len(writes['scenario']) == 253 and len(writes['main']) == 253
    assert not writes['mapgen']
    assert len([e for e in events if e['event']=='native_seed']) == 2
    # Independent original loaded-flag test; 6370B0 between these slices is excluded.
    branch_stop = native.run_checked(u,0x52E623,(0x52E97D,0x52E62F),count=100,
                                     required_addresses=(0x52E623,0x52E629),context=case)
    assert branch_stop == 0x52E97D
    check_code(u,guard)
    return {'id':case['id'],'input':dict(case,global_seed=SEED_IN,system_time=SYSTEMTIME),
            'after_scenario_save_load':loaded,'menu_before':before,'menu_after':after,
            'resolved_seed':actual_seed,'native_seed_result_hex':exact_seed.hex(),
            'scenario_main_match_native_seed_logical':True,'mapgen_retained_raw':True,
            'random_straw_before':straw_before,'random_straw_after':bytes(u.mem_read(0xA8E7B0,0x114)).hex(),
            'events':events,'rng_writes':writes,'menu_call_stop':hex(stop),
            'separate_loaded_flag_stop':hex(branch_stop),'original_executable_bytes_unchanged':True}



def generate() -> dict:
    """Preserve legacy seed/raw cases and add original-executed mixed calls."""
    vectors = {
        "source": "unicorn/gamemd.exe",
        "seed_fn": hex(SEED_FN),
        "next_fn": hex(NEXT_FN),
        "ranged_fn": hex(RANGED_FN),
        "struct_len": STRUCT_LEN,
        "cases": [],
    }
    for seed in SEEDS:
        blob = seeded_struct(seed)
        locked = blob[0]
        idx_a, idx_b = struct.unpack_from("<II", blob, 4)
        values, _ = draws(blob, DRAWS_PER_SEED)
        vectors["cases"].append(
            {
                "seed": seed,
                "locked": locked,
                "idx_a": idx_a,
                "idx_b": idx_b,
                "state_hex": blob[0xC:].hex(),
                "draws": [f"{v:08x}" for v in values],
            }
        )
    vectors["ranged_cases"] = [ranged_history(seed) for seed in SEEDS]
    vectors["ranged_cases"].extend(ranged_history(1, prefix) for prefix in (146, 249))
    vectors['menu_rng_cases'] = [menu_rng_case(case) for case in RNG_CASES]
    return vectors


def main() -> None:
    finish_vectors(
        generate,
        Path(__file__).parent / "vectors" / "rng.json",
        provenance=lambda: dict(provenance(
            scope=("Random seed and first 16 draws for five selected seeds, plus seven "
                   "mixed raw/ranged histories (98 calls) at ordinary audio bounds. "
                   "Includes equal/reversed bounds, rejection and cursor-wrap continuation; "
                   "nine original Scenario load/menu reseed controls. Not exhaustive signed-range, full world-load or whole-loop RNG parity."),
            assumptions=[
                "A zero-initialized 0x3f4-byte generator is supplied to Random__Seed.",
                "Legacy raw calls use fresh emulators and carry only generator bytes forward.",
                "Each mixed history retains one emulator and native-produced RNG state; optional 146/249 raw prefixes execute before the recorded initial state.",
                "Source RNG initialization and supplied bounds are fixture inputs, not a whole application audio/theme scheduling claim.",
                "Raw draw counts and values are observed at native word stores65C79D/65C84B; RandomRanged inlines the draw instead of calling65C780.",
                "Disabled RNG objects and signed spans at or above 0x7fffffff are outside these controls.",
                "Menu rows reuse ScenarioFixture Save/Load/post-read with supplied stopped timer, empty arrays and false11F8; each incoming RNG state is produced by original Seed/Next.",
                "Original RandomStraw6616B0 constructor executes; mode, gate globals, seed override and two OS clocks are supplied inputs, not captured startup values.",
                "52E619..52E61E executes the actual menu call into52FC20; separate52E623..52E97D observes the loaded flag. Intervening6370B0 and later common continuation are excluded.",
                "Logical RNG comparison excludes only padding bytes1..3; MapGen retention compares all bytes at this bounded boundary, not every full-load consumer.",
                "The in-game caller bypasses52FC20 by original caller/control-flow evidence, not by an emulated complete options/game loop.",
            ],
            substitutions=[
                "Shared Scenario in-memory IStream transport,4068E0 diagnostic return and6C8C40 clock0.",
                "GetSystemTime supplies the declared8-word SYSTEMTIME; GetTickCount supplies each explicit tick. No RNG routine or original executable instruction is substituted.",
            ],
            entry_points={"Random__Seed": SEED_FN, "Random__Next": NEXT_FN,
                          "Random__RandomRanged": RANGED_FN,
                          "ScenarioSave": 0x689310, "ScenarioLoad": 0x689470,
                          "ScenarioPostRead": 0x683560, "MenuCall": 0x52E619,
                          "InitRandom": 0x52FC20, "MenuLoadedFlag": 0x52E623,
                          "RandomStrawConstructor": 0x6616B0},
        ), command="python -m tools.rmg_oracle.gen_rng_vectors --check"),
        source_paths={"generator": Path(__file__),
                      "checked_runner": Path(__file__).parents[1] / "native_oracle.py",
                      "scenario_stream": Path(__file__).parents[1] / "spatial_oracle/native_id_snapshot.py",
                      "pe_imports": Path(__file__).parents[1] / "storage_oracle/shell_relayout.py"},
    )


if __name__ == "__main__":
    main()
