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

Nothing is stubbed: both blocks are straight-line reads and writes of the Techno and
the frame counter.

Rust consumers: src/sim/combat/world_receiver.rs (fireat_rearm_frames, the rearm write
in emit_admitted_fire) and src/sim/timer.rs (CdTimer::remaining, read by
src/sim/world/techno_ai/mission_handlers.rs evaluate_foot_guard_cadence).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_ESP)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, RET_MAGIC, finish_vectors, provenance)

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


def generate():
    return dict(
        fire=[fire_rearm(case) for case in fire_cases()],
        guard=[guard_row(case) for case in guard_cases()],
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("FireAt's rearm write after GetROF over the returned value's sign and "
               "magnitude (incl. int32 extremes), the berserk byte and the frame counter; "
               "Mission_Guard's rearm read over the timer's start (incl. the -1 paused "
               "sentinel and wrapping differences), duration (incl. zero and negative) and "
               "the frame counter, recording the value returned or the fall-through."),
        assumptions=[
            "The FireAt block starts with GetROF's return in EAX and EBX holding the "
            "WeaponType (read only past the recorded end).",
            "The Mission_Guard block is entered at 0x004D52A9 with ESI the Techno; its "
            "exits are observed at the instruction that sets EAX for the return.",
        ],
        substitutions=[],
        entry_points={"fire_begin": FIRE_BEGIN, "fire_end": FIRE_END,
                      "guard_begin": GUARD_BEGIN, "guard_running": GUARD_RUNNING,
                      "guard_paused": GUARD_PAUSED, "guard_spent": GUARD_SPENT,
                      "guard_zero": GUARD_ZERO},
    ))
