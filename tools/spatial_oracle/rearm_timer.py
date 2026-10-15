"""Original rearm timer (TechnoClass+0x2EC CDTimer, duration +0x2F4) write and read.

Executes, case by case:
- TechnoClass::FireAt 0x006FF28F..0x006FF2BE, from GetROF's return (EAX) to the burst
  remainder: a berserk firer (+0x298) halves the value signed and toward zero (CDQ, SUB,
  SAR), the value is copied to +0x2F8, and the timer is started at the current frame
  ([0x00A8ED84]) with it as the duration. The burst remainder after it is pinned by
  techno_burst_index.py.
- FootClass::Mission_Guard 0x004D52A9 to its return: while the timer runs (a paused
  start of -1 reads the duration; else frame - start against the duration, signed) the
  handler returns the remaining frames, 0x004D52ED (running) or 0x004D5338 (paused);
  a spent timer falls through to 0x004D5301 (DistributedFire, then the jitter draw).

The original fire/guard rows retain those bounds. Additive guard_distributed_rng
rows continue the Guard tail through its real return, actual Unit type getter
and Scenario RandomRanged. They supply the earlier EBP cadence and original RNG
owner's retained before-state; no RNG result or request gate is substituted.

Rust consumers: src/sim/combat/world_fire.rs (fireat_rearm_frames, the rearm write
in emit_admitted_fire) and src/sim/timer.rs (CdTimer::remaining, read by
src/sim/world/techno_ai/mission_handlers.rs evaluate_foot_guard_cadence).
"""
import json
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_ECX)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, RET_MAGIC, finish_vectors, provenance, OracleError)

FIRE_BEGIN, FIRE_END = 0x6FF28F, 0x6FF2BE
GUARD_BEGIN = 0x4D52A9
GUARD_RUNNING, GUARD_PAUSED, GUARD_SPENT = 0x4D52ED, 0x4D5338, 0x4D5301
GUARD_ZERO = 0x4D52F5
FRAME = 0xA8ED84
TECHNO, WEAPON = SCRATCH + 0x1000, SCRATCH + 0x3000
SP = STACK_BASE + STACK_SIZE - 0x4000


def u32(value):
    return struct.pack("<I", value & 0xFFFFFFFF)


def i32(data):
    return struct.unpack("<i", bytes(data))[0]


def machine():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ESI, TECHNO)
    return uc


def fire_rearm(case):
    uc = machine()
    uc.mem_write(FRAME, u32(case["frame"]))
    uc.mem_write(TECHNO + 0x298, bytes([case["berserk"]]))
    uc.mem_write(TECHNO + 0x2EC, u32(0x7777))
    uc.mem_write(TECHNO + 0x2F4, u32(0x7777))
    uc.mem_write(TECHNO + 0x2F8, u32(0x7777))
    uc.reg_write(UC_X86_REG_EBX, WEAPON)
    uc.reg_write(UC_X86_REG_EAX, case["rof"])
    run_checked(uc, FIRE_BEGIN, FIRE_END, count=1_000)
    return dict(input=case,
                start=i32(uc.mem_read(TECHNO + 0x2EC, 4)),
                duration=i32(uc.mem_read(TECHNO + 0x2F4, 4)),
                rof_copy=i32(uc.mem_read(TECHNO + 0x2F8, 4)))


def guard_rearm(case):
    uc = machine()
    uc.mem_write(FRAME, u32(case["frame"]))
    uc.mem_write(TECHNO + 0x2EC, u32(case["start"]))
    uc.mem_write(TECHNO + 0x2F4, u32(case["duration"]))
    end = run_checked(uc, GUARD_BEGIN, (GUARD_RUNNING, GUARD_PAUSED, GUARD_SPENT, GUARD_ZERO),
                      count=1_000)
    at = uc.reg_read(UC_X86_REG_EIP)
    assert at == end
    if at == GUARD_RUNNING:
        returned = i32(u32(uc.reg_read(UC_X86_REG_EAX)))
    elif at == GUARD_PAUSED:
        # 0x004D5338: MOV EAX, EDI, and EDI holds the duration read at 0x004D52D0.
        returned = i32(u32(uc.reg_read(UC_X86_REG_EDI)))
    elif at == GUARD_ZERO:
        returned = 0
    else:
        returned = None
    return at, returned


def guard_row(case):
    at, returned = guard_rearm(case)
    return dict(input=case, returns=returned,
                exit={GUARD_RUNNING: "running", GUARD_PAUSED: "paused", GUARD_ZERO: "zero",
                      GUARD_SPENT: "spent"}[at])


FIRE_ROFS = (0, 1, 2, 3, 4, 5, 7, 20, 49, 50, 51, 99, 100, 250, 1000, 2147483647, -1, -2, -3,
             -51, -2147483648)
FRAMES = (0, 1, 1234, 2147483647, -1)


def fire_cases():
    for rof in FIRE_ROFS:
        for berserk in (0, 1):
            for frame in (0, 1234):
                yield dict(rof=rof, berserk=berserk, frame=frame)
    for frame in FRAMES:
        yield dict(rof=50, berserk=1, frame=frame)


def guard_cases():
    for duration in (0, 1, 2, 30, 50, 2147483647, -1, -30):
        for start, frame in ((100, 100), (100, 101), (100, 129), (100, 130), (100, 131),
                             (100, 99), (100, 0), (-1, 100), (-1, 0), (-2, 5),
                             (2147483647, -2147483648), (0, 2147483647)):
            yield dict(start=start, duration=duration, frame=frame)


def guard_rng_input():
    """An original-produced continuation, including a rejecting (0,2) request.

    Reuse the RNG owner's retained before-state, never a Rust/host RNG answer.
    This reads only when explicitly generating the native controls.
    """
    path = Path(__file__).parents[1] / 'rmg_oracle/vectors/rng.json'
    corpus = json.loads(path.read_text())
    history = next(h for h in corpus['ranged_cases'] if h['id'] == 'seed_0_after_0_raw')
    step = next(s for s in history['steps'] if s.get('low') == 0 and s.get('high') == 2)
    state = bytes.fromhex(step['before_state_hex'])
    if len(state) != 0x3F4:
        raise OracleError('RNG owner supplied an incomplete Guard input state')
    return state


def guard_distributed_rng(case, rng_before):
    """Original timer/gate/RNG/return, after the earlier mission-side effects.

    EBP is the already-computed mission cadence; object/type/count/timer are
    explicit inputs. The Unit type getter and RandomRanged execute unchanged.
    """
    uc = machine()
    kind, scenario = SCRATCH + 0x5000, SCRATCH + 0x8000
    rng = scenario + 0x218
    spans = ((GUARD_BEGIN, 0x4D5342), (0x741490, 0x741497), (0x65C7E0, 0x65C88D))
    original = [bytes(uc.mem_read(a, b-a)) for a, b in spans]
    uc.mem_write(TECHNO, u32(0x7F5C70))
    uc.mem_write(TECHNO + 0x6C4, u32(kind))
    uc.mem_write(kind + 0x6B0, bytes((case['distributed'],)))
    uc.mem_write(TECHNO + 0x468, u32(case['candidate_count']))
    uc.mem_write(TECHNO + 0x2EC, u32(case['rearm_start']))
    uc.mem_write(TECHNO + 0x2F4, u32(case['rearm_duration']))
    uc.mem_write(FRAME, u32(case['frame']))
    uc.mem_write(0xA8B230, u32(scenario))
    uc.mem_write(rng, rng_before)
    # Four saved registers and twelve local bytes from the real enclosing
    # prologue. Each native return unwinds this frame, then RETs to the sentinel.
    saved = (0x13579BDF, 0x2468ACE0, 0x31415926, 0x27182818)
    uc.mem_write(SP, b''.join(u32(v) for v in (*saved, 0, 0, 0, RET_MAGIC)))
    uc.reg_write(UC_X86_REG_EBP, case['base_cadence'])
    uc.reg_write(UC_X86_REG_ECX, TECHNO)
    calls, visits = [], []

    def observe(u, pc, _size, _data):
        if pc in (GUARD_BEGIN, GUARD_SPENT, 0x4D531D, 0x4D532F, 0x4D5338, 0x741490):
            visits.append(f'{pc:08X}')
        if pc == 0x65C7E0:
            sp = u.reg_read(UC_X86_REG_ESP)
            if u.reg_read(UC_X86_REG_ECX) != rng:
                raise OracleError('Guard used a different RNG stream')
            calls.append(dict(low=i32(u.mem_read(sp+4, 4)), high=i32(u.mem_read(sp+8, 4)),
                              stream='scenario', raw_words=[]))
        elif pc == 0x65C84B:
            if not calls:
                raise OracleError('Guard raw store without a ranged request')
            calls[-1]['raw_words'].append(u.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF)
        elif pc == 0x4D5334:
            calls[-1]['result'] = i32(u32(u.reg_read(UC_X86_REG_EAX)))

    uc.hook_add(UC_HOOK_CODE, observe)
    run_checked(uc, GUARD_BEGIN, RET_MAGIC, count=10000, required_addresses=(GUARD_BEGIN,))
    if uc.reg_read(UC_X86_REG_ESP) != SP + 32:
        raise OracleError('Guard failed to unwind its original caller frame')
    for register, value in zip((UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ESI), saved):
        if uc.reg_read(register) != value:
            raise OracleError('Guard failed to restore a saved register')
    if original != [bytes(uc.mem_read(a, b-a)) for a, b in spans]:
        raise OracleError('Guard changed original executable bytes')
    return dict(name=case['name'], input={k:v for k,v in case.items() if k != 'name'},
                rng_before_hex=rng_before.hex(), rng_after_hex=bytes(uc.mem_read(rng, 0x3F4)).hex(),
                ranged_calls=calls, returns=i32(u32(uc.reg_read(UC_X86_REG_EAX))),
                visited_boundaries=visits)


def guard_distributed_rng_controls():
    base = dict(distributed=True, candidate_count=1, frame=173, rearm_start=173,
                rearm_duration=0, base_cadence=27)
    cases = [('spent_positive', {}), ('spent_two', dict(candidate_count=2)),
             ('spent_empty', dict(candidate_count=0)), ('spent_flag_false', dict(distributed=False)),
             ('running', dict(rearm_duration=15, rearm_start=170)),
             ('paused', dict(rearm_duration=15, rearm_start=-1))]
    before = guard_rng_input()
    return [guard_distributed_rng(dict(base, name=name, **changes), before)
            for name, changes in cases]


def generate():
    return dict(
        fire=[fire_rearm(case) for case in fire_cases()],
        guard=[guard_row(case) for case in guard_cases()],
        guard_distributed_rng=guard_distributed_rng_controls(),
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("FireAt's rearm write after GetROF over the returned value's sign and "
               "magnitude (incl. int32 extremes), the berserk byte and the frame counter; "
               "Mission_Guard's rearm read over the timer's start (incl. the -1 paused "
               "sentinel and wrapping differences), duration (incl. zero and negative) and "
               "the frame counter, recording the value returned or the fall-through; "
               "six additive complete Guard tails through DistributedFire's request gate and original Scenario RNG."),
        assumptions=[
            "The FireAt block starts with GetROF's return in EAX and EBX holding the "
            "WeaponType (read only past the recorded end).",
            "The Mission_Guard block is entered at 0x004D52A9 with ESI the Techno; its "
            "exits are observed at the instruction that sets EAX for the return.",
            "Additive guard_distributed_rng controls execute4D52A9 through RET, actual Unit type getter741490 and Scenario RandomRanged65C7E0. Supplied original caller frame and already-computed EBP cadence27 exclude earlier mission/Rate-conversion effects.",
            "Guard RNG before-state is the original-produced seed_0_after_0_raw (0,2) step from tools/rmg_oracle/vectors/rng.json; raw words are observed at65C84B, not inferred from requests. Positive/empty/flagfalse and running/paused controls retain the same full input state.",
        ],
        substitutions=[],
        entry_points={"fire_begin": FIRE_BEGIN, "fire_end": FIRE_END,
                      "guard_begin": GUARD_BEGIN, "guard_running": GUARD_RUNNING,
                      "guard_paused": GUARD_PAUSED, "guard_spent": GUARD_SPENT,
                      "guard_zero": GUARD_ZERO, "guard_return": 0x4D5341,
                      "guard_random": 0x65C7E0, "unit_type_getter": 0x741490},
    ))
