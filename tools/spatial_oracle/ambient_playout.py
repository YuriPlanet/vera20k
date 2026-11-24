"""Original Voc playout sequencing for one SoundEvent from state 1.

Executes SoundEvent::LoadSamples 0x004048B0 (with SelectNextSample
0x00404BB0), PreparePlayout 0x00404700 (with AdvancePlaylist 0x004047B0),
OnChannelSampleEnd 0x00405AC0 and OnChannelDone 0x00405A00 in the order
SoundSystem::UpdateTick's start pass (0x004045C8..0x00404686) and the channel
callbacks give them: LoadSamples and PreparePlayout, both again after each
declared eviction, PreparePlayout once more, a StartPlayback stand-in,
OnChannelSampleEnd until the channel holds no next buffer, OnChannelDone, and
the pre-delay expiry of UpdateState (0x00405626). Main RNG 0x00886B88 is
seeded by Random::Seed 0x0065C6D0 and RandomRanged 0x0065C7E0 runs natively.

The OnChannelDone rows run that callback alone on declared event fields.

Rust consumers: src/audio/sfx/ambient_playout_tests.rs (lifecycles) and
src/audio/arbiter.rs (channel_done).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                               UC_X86_REG_EIP, UC_X86_REG_ESP)

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, finish_vectors, provenance)
from tools.rmg_oracle.gen_rng_vectors import seeded_struct, STRUCT_LEN
from tools.spatial_oracle.map_queries import dwords

LOAD_SAMPLES, PREPARE_PLAYOUT = 0x4048B0, 0x404700
SAMPLE_END, CHANNEL_DONE = 0x405AC0, 0x405A00
RANDOM_RANGED, MAIN_RNG, SAMPLE_TRACKER = 0x65C7E0, 0x886B88, 0x87E298
LOAD_SAMPLE, ADD_REF, RELEASE = 0x401C00, 0x401F80, 0x401F90
GET_BUFFER, SET_NEXT, TIMESTAMP = 0x402030, 0x402790, 0x4093B0

REGION = 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
TYPE, EVENT, CHANNEL, TRACKER = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x3000
SAMPLE_BASE, BUFFER_BASE = 0x5000000, 0x6000000  # stub CachedSample / buffer handles
SAMPLE_ID_BASE = 100  # Sounds= name i is audio index sample 100 + i
NOW = 0x1_0000_2000  # 0x004093B0's timestamp

LOOP, RANDOM, ALL, PREDELAY, INTERRUPT, ATTACK, DECAY, AMBIENT = 1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80
MAX_SAMPLE_ENDS = 64


def entry(section, control, delay, count, attack=0, decay=0, loop=0):
    """AudioEventClass fields as SetControlFlags 0x00406570 leaves them."""
    return dict(section=section, control=control, delay=list(delay), count=count,
                attack=attack, decay=decay, loop=loop)


# Retail soundmd.ini sections (the Rust consumer re-reads them through the
# production reader) and synthetic variants for the attack, decay, failed-load
# and below-floor gap branches.
LIFECYCLES = [
    dict(name="wind_plains", entry=entry("_Amb_WindPlains", LOOP | RANDOM | ALL | AMBIENT, (5000, 10000), 4),
         seed=41, passes=6),
    dict(name="chimp", entry=entry("ChimpAmbient", RANDOM | ALL | AMBIENT, (5000, 10000), 3),
         seed=42, passes=6),
    dict(name="cow", entry=entry("CowAmbient", RANDOM | INTERRUPT | AMBIENT, (5000, 10000), 3),
         seed=43, passes=4),
    dict(name="cruise_ship", entry=entry("CruiseShipAmbience", LOOP | AMBIENT, (10000, 20000), 1),
         seed=44, passes=4),
    dict(name="upgrade_veteran", entry=entry("UpgradeVeteran", PREDELAY | INTERRUPT, (400, 400), 1),
         seed=45, passes=4),
    dict(name="spy_plane", entry=entry("SpyPlaneSnapShot", 0, (500, 500), 1),
         seed=46, passes=4, victims=2),
    dict(name="mig_move", entry=entry("MIGMove", RANDOM | PREDELAY, (1500, 1500), 2),
         seed=47, passes=4),
    dict(name="test_random_loop", entry=entry("TestRandomLoopDelayAll", LOOP | RANDOM | ALL, (500, 3000), 10),
         seed=48, passes=13),
    dict(name="v3_move_start", entry=entry("V3MoveStart", RANDOM | PREDELAY, (0, 400), 3),
         seed=49, passes=3),
    dict(name="attack_decay_loop",
         entry=entry(None, LOOP | RANDOM | ALL | ATTACK | DECAY | AMBIENT, (100, 200), 6, 2, 2),
         seed=50, passes=7),
    dict(name="sequential_attack_decay", entry=entry(None, ALL | ATTACK | DECAY, (100, 200), 5, 1, 1),
         seed=51, passes=7),
    dict(name="decay_alone", entry=entry(None, RANDOM | DECAY, (100, 200), 3, 0, 1),
         seed=52, passes=4),
    dict(name="counted_loop", entry=entry(None, LOOP | RANDOM, (100, 200), 3, loop=2),
         seed=53, passes=5),
    dict(name="failed_load", entry=entry(None, LOOP | RANDOM | ALL | AMBIENT, (100, 200), 3),
         seed=54, passes=4, failed=[1], victims=1),
    dict(name="max_below_min", entry=entry(None, LOOP | RANDOM | ALL | AMBIENT, (100, 20), 3),
         seed=55, passes=4),
    dict(name="below_floor_gap", entry=entry(None, ALL, (0, 400), 3), seed=56, passes=4),
]

# OnChannelDone 0x00405A00 alone: +0x148 next, +0x268 queue, +0x13C gap,
# +0x158 suspension and +0x1C state on entry.
CHANNEL_DONE_ROWS = [
    dict(state=3, next=0, queue=0, gap=0, paused=0),
    dict(state=3, next=0, queue=2, gap=0, paused=0),
    dict(state=3, next=1, queue=0, gap=0, paused=0),
    dict(state=3, next=0, queue=0, gap=5000, paused=0),
    dict(state=3, next=0, queue=1, gap=0x21, paused=0),
    dict(state=3, next=0, queue=0, gap=7000, paused=1),
    dict(state=4, next=0, queue=1, gap=5000, paused=0),
]


def sample_index(pointer, base):
    return None if pointer == 0 else (pointer - base) // 0x10 - SAMPLE_ID_BASE


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.hook_add(UC_HOOK_CODE, self.observe)
        self.trace, self.draw, self.failed = [], None, set()

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def signed(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == RANDOM_RANGED:
            low, high = struct.unpack("<ii", u.mem_read(sp + 4, 8))
            self.draw = (low, high, self.word(sp), sp + 12)
            return
        if self.draw and pc == self.draw[2] and sp == self.draw[3]:
            eax = struct.unpack("<i", struct.pack("<I", u.reg_read(UC_X86_REG_EAX)))[0]
            self.trace.append(["draw", self.draw[0], self.draw[1], eax])
            self.draw = None
        if pc not in (LOAD_SAMPLE, ADD_REF, RELEASE, GET_BUFFER, SET_NEXT, TIMESTAMP):
            return
        ecx, edx = u.reg_read(UC_X86_REG_ECX), u.reg_read(UC_X86_REG_EDX)
        if pc == LOAD_SAMPLE:
            assert ecx == TRACKER, "LoadSample's tracker"
            index = edx - SAMPLE_ID_BASE
            ok = index not in self.failed
            self.trace.append(["load", index, ok])
            u.reg_write(UC_X86_REG_EAX, SAMPLE_BASE + edx * 0x10 if ok else 0)
        elif pc == GET_BUFFER:
            u.reg_write(UC_X86_REG_EAX, BUFFER_BASE + (ecx - SAMPLE_BASE))
        elif pc == SET_NEXT:
            assert ecx == CHANNEL, "SetNext's channel"
            self.trace.append(["set_next", sample_index(edx, BUFFER_BASE)])
        elif pc == TIMESTAMP:
            u.reg_write(UC_X86_REG_EAX, NOW & 0xFFFFFFFF)
            u.reg_write(UC_X86_REG_EDX, NOW >> 32)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4)

    def run(self, function, this):
        u = self.u
        self.trace = []
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, this)
        run_checked(u, function, RET_MAGIC, count=200_000)
        assert self.draw is None, "unfinished RandomRanged"
        return self.trace, u.reg_read(UC_X86_REG_EAX)

    def install(self, fields, seed):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(MAIN_RNG, seeded_struct(seed))
        u.mem_write(SAMPLE_TRACKER, dwords(TRACKER))
        # AudioEventClass: +0x0C fSamplesOK, +0x10 Control, +0x4C Loop,
        # +0x58/+0x5C Delay, +0xB4 sample ids, +0x134 count, +0x138/+0x13C
        # Attack/Decay.
        u.mem_write(TYPE + 0x0C, dwords(1, fields["control"]))
        u.mem_write(TYPE + 0x4C, dwords(fields["loop"]))
        u.mem_write(TYPE + 0x58, dwords(*fields["delay"]))
        count = fields["count"]
        u.mem_write(TYPE + 0xB4, dwords(*(SAMPLE_ID_BASE + i for i in range(count))) if count else b"")
        u.mem_write(TYPE + 0x134, dwords(count, fields["attack"], fields["decay"]))
        # SoundEvent from AllocateFromPool 0x00405190 and UpdateState state
        # 0: zeroed, its type +0x24, its channel +0xB0, state 1.
        u.mem_write(EVENT + 0x1C, dwords(1))
        u.mem_write(EVENT + 0x24, dwords(TYPE))
        u.mem_write(EVENT + 0xB0, dwords(CHANNEL))
        u.mem_write(CHANNEL + 0xC0, dwords(EVENT))

    def call(self, name, function, this):
        """One native call: its draws, loads and SetNext calls, and the result
        the Rust consumer compares (loaded table, buffer or state)."""
        trace, eax = self.run(function, this)
        step = dict(call=name)
        if trace:
            step["trace"] = trace
        if function == LOAD_SAMPLES:
            step["loaded"] = [sample_index(self.word(EVENT + 0x28 + 4 * i), SAMPLE_BASE)
                              for i in range(self.signed(EVENT + 0xA8))]
        elif function == PREPARE_PLAYOUT:
            step["buffer"] = sample_index(eax, BUFFER_BASE)
        elif function == CHANNEL_DONE:
            step["state"] = self.word(EVENT + 0x1C)
        return step

    def lifecycle(self, case):
        self.install(case["entry"], case["seed"])
        self.failed = set(case.get("failed", []))
        steps = []
        for _ in range(case["passes"]):
            steps.append(self.call("LoadSamples", LOAD_SAMPLES, EVENT))
            steps.append(self.call("PreparePlayout", PREPARE_PLAYOUT, EVENT))
            for _ in range(case.get("victims", 0)):
                if steps[-1]["buffer"] is not None:
                    break
                steps.append(self.call("LoadSamples", LOAD_SAMPLES, EVENT))
                steps.append(self.call("PreparePlayout", PREPARE_PLAYOUT, EVENT))
            steps.append(self.call("PreparePlayout", PREPARE_PLAYOUT, EVENT))
            buffer = steps[-1]["buffer"]
            if buffer is None:
                break
            # StartPlayback 0x004054A0: flags |= 8 | 2, state 3, +0x148 = 0.
            flags = self.word(EVENT + 0x18)
            self.u.mem_write(EVENT + 0x18, dwords(flags | 0xA, 3))
            self.u.mem_write(EVENT + 0x148, dwords(0))
            for _ in range(MAX_SAMPLE_ENDS):
                steps.append(self.call("OnChannelSampleEnd", SAMPLE_END, CHANNEL))
                set_next = [entry[1] for entry in steps[-1]["trace"] if entry[0] == "set_next"]
                if set_next[-1] is None:
                    break
            else:
                raise AssertionError("sample chain did not end")
            steps.append(self.call("OnChannelDone", CHANNEL_DONE, CHANNEL))
            state = self.word(EVENT + 0x1C)
            if state == 4:
                break
            if state == 2:
                # UpdateState 0x00405626 once the timestamp passes +0x140.
                self.u.mem_write(EVENT + 0x1C, dwords(self.word(EVENT + 0x20)))
                steps.append(dict(call="Expire"))
            assert self.word(EVENT + 0x1C) == 1, "event resumes in state 1"
        return dict(name=case["name"], entry=case["entry"], seed=case["seed"],
                    failed=sorted(self.failed), victims=case.get("victims", 0), steps=steps)

    def channel_done(self, row):
        self.install(entry(None, 0, (0, 0), 1), 1)
        self.u.mem_write(EVENT + 0x18, dwords(0xA, row["state"], 0))
        self.u.mem_write(EVENT + 0x13C, dwords(row["gap"]))
        self.u.mem_write(EVENT + 0x148, dwords(BUFFER_BASE + SAMPLE_ID_BASE * 0x10 if row["next"] else 0))
        self.u.mem_write(EVENT + 0x158, dwords(row["paused"]))
        self.u.mem_write(EVENT + 0x268, dwords(row["queue"]))
        trace, _ = self.run(CHANNEL_DONE, CHANNEL)
        assert trace == [], "OnChannelDone draws, loads and sets no buffer"
        return dict(input=row, flags=self.word(EVENT + 0x18), state=self.word(EVENT + 0x1C),
                    resume=self.word(EVENT + 0x20),
                    qwtime=self.word(EVENT + 0x140) | self.word(EVENT + 0x144) << 32)


def generate():
    fixture = Fixture()
    return dict(now=NOW, lifecycles=[fixture.lifecycle(case) for case in LIFECYCLES],
                channel_done=[fixture.channel_done(row) for row in CHANNEL_DONE_ROWS])


def metadata():
    return provenance(
        scope="Original SoundEvent LoadSamples/SelectNextSample, PreparePlayout/AdvancePlaylist, "
              "OnChannelSampleEnd and OnChannelDone over one event's start passes and playouts",
        assumptions=[
            "AudioEventClass fields as SetControlFlags 0x00406570 leaves them; sample i is audio index id 100 + i",
            "The event enters state 1 zeroed with a channel; +0x158 suspension is clear in lifecycles",
            "Main RNG 0x00886B88 holds Random::Seed 0x0065C6D0's state for the case seed",
            "A declared eviction count stands in for FindLowestRanked 0x00404E20 finding victims",
        ],
        substitutions=[
            "SampleTracker::LoadSample 0x00401C00 returns a stub CachedSample, NULL for a declared failed load",
            "CachedSample AddRef 0x00401F80 / Release 0x00401F90 return without running",
            "CachedSample::GetBuffer 0x00402030 returns a stub buffer per sample",
            "DSoundChannel::SetNext 0x00402790 is recorded and returns without running",
            "0x004093B0 returns a fixed timestamp",
            "StartPlayback 0x004054A0 is replaced by its event writes: flags |= 0xA, state 3, +0x148 = 0",
            "The pre-delay expiry writes state = resume as UpdateState 0x00405626 does once the timestamp passes",
        ],
        entry_points={"load_samples": LOAD_SAMPLES, "prepare_playout": PREPARE_PLAYOUT,
                      "sample_end": SAMPLE_END, "channel_done": CHANNEL_DONE})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
