"""Original CaptureManagerClass::Update 0x00471A50 (the Mastermind overload).

Each case builds its manager with the original constructor 0x004717D0 (run up
to its registration in the global vector at 0x00471835), sets the node count
and calls the original Update once per frame. The row walk, the countdown
cadence (the constructor's first delay included), the ReceiveDamage arguments,
the voice latch, the five spark systems with their Scenario draws and
coordinates, and the lean's draw and sign execute. The file-static target
coordinate 0x0089E138 comes from its original initializer 0x004716F0.

ReceiveDamage (owner vt+0x16C), VocClass::PlayAt 0x007509E0, operator new
0x007C8E17 and the ParticleSystemClass constructor 0x0062DC50 are recorded and
return at their entry without running; a killing case clears the owner's
IsAlive byte (+0x90) inside ReceiveDamage, as the real death arm does.

Rust consumer: src/sim/capture_manager_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x40000
(MGR, OWNER, OVT, RULES, SCENARIO, TABLES, NEWBUF, CALLBACKS) = [
    SCRATCH + n * 0x4000 for n in range(8)]
SP = STACK_BASE + STACK_SIZE - 0x1000
DVC_INT_VTABLE = 0x7E4DD8
CTOR, CTOR_REGISTER, UPDATE = 0x4717D0, 0x471835, 0x471A50
STATIC_TARGET_INIT = 0x4716F0
RANDOM_RANGED, NEXT_RANDOM, SEED = 0x65C7E0, 0x65C780, 0x65C6D0
PLAY_AT, OPERATOR_NEW, PARTICLE_SYSTEM_CTOR = 0x7509E0, 0x7C8E17, 0x62DC50
RECEIVE_DAMAGE = CALLBACKS
# Update's three RandomRanged return sites: the spark pair, then the lean.
RANDOM_RETURNS = (0x471B95, 0x471BCE, 0x471C67)
# Markers passed through as arguments.
C4_WARHEAD, OVERLOAD_SOUND, SPARK_TYPE = 0x0C4C4C40, 0x123, 0x5A5A0000
STOCK = dict(count=[3, 6, 10, 50], damage=[0, 50, 100, 500], frames=[30, 60, 60, 60])
LOCATION = (12928, 25728, 480)


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.events = []
        self.kill = False
        self.allocated = 0
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def signed(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def coord(self, address):
        return list(struct.unpack("<iii", self.u.mem_read(address, 12)))

    def ret(self, value, cleaned):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleaned)

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == RECEIVE_DAMAGE:
            args = [self.word(sp + 4 * n) for n in range(1, 8)]
            self.events.append(dict(damage=self.signed(args[0]), args=args[1:]))
            if self.kill:
                u.mem_write(OWNER + 0x90, b"\x00")
            self.ret(0, 0x1C)
        elif pc == PLAY_AT:
            self.events.append(dict(sound=u.reg_read(UC_X86_REG_ECX),
                                    at=self.coord(u.reg_read(UC_X86_REG_EDX)),
                                    arg=self.word(sp + 4)))
            self.ret(0, 4)
        elif pc == OPERATOR_NEW:
            self.events.append(dict(new=self.word(sp + 4)))
            self.allocated += 1
            self.ret(NEWBUF + self.allocated * 0x100, 0)
        elif pc == PARTICLE_SYSTEM_CTOR:
            this = u.reg_read(UC_X86_REG_ECX)
            self.events.append(dict(spark=self.word(sp + 4), at=self.coord(self.word(sp + 8)),
                                    target=self.word(sp + 12), owner=self.word(sp + 16),
                                    target_at=self.coord(self.word(sp + 20)),
                                    house=self.word(sp + 24)))
            self.ret(this, 0x18)
        elif pc == RANDOM_RANGED:
            self.events.append(dict(random=[self.signed(sp + 4), self.signed(sp + 8)]))
        elif pc in RANDOM_RETURNS:
            value = struct.unpack("<i", dwords(u.reg_read(UC_X86_REG_EAX)))[0]
            self.events[-1]["value"] = value

    def call(self, address, ecx, args=(), end=RET_MAGIC):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, end, count=400000)
        return u.reg_read(UC_X86_REG_EAX)

    def execute(self, case):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        tables = case.get("tables", STOCK)
        # The file-static zero coordinate the spark systems take as target.
        u.mem_write(0x89E138, bytes([0x5A] * 12))
        self.call(STATIC_TARGET_INIT, 0)
        # The owner: its vtable (ReceiveDamage at +16C), Location (+9C),
        # IsAlive (+90) and the lean float (+330).
        u.mem_write(OWNER, dwords(OVT))
        u.mem_write(OVT + 0x16C, dwords(RECEIVE_DAMAGE))
        u.mem_write(OWNER + 0x9C, struct.pack("<iii", *case.get("location", LOCATION)))
        u.mem_write(OWNER + 0x90, b"\x01")
        u.mem_write(0x8871E0, dwords(RULES))
        items = TABLES
        for offset, key in ((0xEE8, "count"), (0xF04, "damage"), (0xF20, "frames")):
            values = tables[key]
            u.mem_write(RULES + offset, dwords(DVC_INT_VTABLE, items, len(values), 0, len(values), 10))
            u.mem_write(items, dwords(*values))
            items += 0x100
        u.mem_write(RULES + 0xFA8, dwords(C4_WARHEAD))
        u.mem_write(RULES + 0x258, dwords(OVERLOAD_SOUND))
        u.mem_write(RULES + 0x1020, dwords(SPARK_TYPE))
        u.mem_write(0xA8B230, dwords(SCENARIO))
        self.call(SEED, SCENARIO + 0x218, [case["seed"]])
        # The original constructor, stopped before its global registration.
        self.call(CTOR, MGR, [OWNER, 3, int(case.get("infinite", True))], end=CTOR_REGISTER)
        constructed = self.state()
        u.mem_write(MGR + 0x34, dwords(case["captives"]))
        self.kill = case.get("kill", False)
        frames = []
        for frame in range(1, case["calls"] + 1):
            if str(frame) in case.get("captives_at", {}):
                u.mem_write(MGR + 0x34, dwords(case["captives_at"][str(frame)]))
            self.events = []
            self.call(UPDATE, MGR)
            if self.events:
                frames.append(dict(frame=frame, events=self.events, after=self.state()))
        final = self.state()
        final["alive"] = u.mem_read(OWNER + 0x90, 1)[0]
        final["lean"] = struct.unpack("<f", u.mem_read(OWNER + 0x330, 4))[0]
        next_random = self.call(NEXT_RANDOM, SCENARIO + 0x218) & 0xFFFFFFFF
        return dict(input=case, constructed=constructed, frames=frames, final=final,
                    next_random=next_random)

    def state(self):
        return dict(countdown=self.signed(MGR + 0x4C), flash=self.signed(MGR + 0x44),
                    latch=self.u.mem_read(MGR + 0x41, 1)[0], max=self.signed(MGR + 0x3C),
                    infinite=self.u.mem_read(MGR + 0x40, 1)[0])


def inputs():
    cases = [
        dict(name="idle_at_the_first_row", seed=1, captives=3, calls=70),
        dict(name="row_one", seed=1, captives=4, calls=130),
        dict(name="row_two", seed=2, captives=7, calls=35),
        dict(name="row_three", seed=3, captives=11, calls=35),
        dict(name="row_three_capped", seed=4, captives=51, calls=35),
        dict(name="boundary_six", seed=5, captives=6, calls=31),
        dict(name="boundary_ten", seed=5, captives=10, calls=31),
        dict(name="boundary_fifty", seed=5, captives=50, calls=31),
        dict(name="no_captives", seed=5, captives=0, calls=31),
        dict(name="dies_from_the_overload", seed=6, captives=4, calls=31, kill=True),
        dict(name="voice_rearms", seed=7, captives=4, calls=130, captives_at={"62": 3, "93": 4}),
        dict(name="finite_manager", seed=8, captives=5, calls=40, infinite=False),
        dict(name="single_row_damages_without_lean", seed=9, captives=5, calls=31,
             tables=dict(count=[2], damage=[10], frames=[5])),
        dict(name="zero_frames_checks_every_frame", seed=10, captives=3, calls=34,
             tables=dict(count=[0, 5], damage=[0, 20], frames=[0, 0])),
        dict(name="negative_damage_rearms", seed=10, captives=4, calls=31,
             tables=dict(count=[3, 6], damage=[-5, -5], frames=[30, 30])),
        dict(name="located_elsewhere", seed=11, captives=7, calls=31, location=(-300, 70000, 1234)),
    ]
    cases += [dict(name=f"spark_seed_{seed}", seed=seed, captives=4, calls=31)
              for seed in (12, 13, 14, 15, 16, 17)]
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original 0x004717D0 constructor (to its registration) and 0x00471A50 CaptureManagerClass::Update, frame by frame",
        assumptions=[
            "Owner vtable +16C is ReceiveDamage (7 stdcall arguments); owner +9C Location, +90 IsAlive, +330 the lean float",
            "Rules +EE8/+F04/+F20 are the OverloadCount/Damage/Frames DynamicVectorClass<int>; +FA8 C4Warhead, +258 the overload sound, +1020 DefaultSparkSystem",
            "Scenario RNG seeded through the original 0x0065C6D0; next_random from the original 0x0065C780",
        ],
        substitutions=[
            "Recorded at entry and returned without running: owner ReceiveDamage (a killing case clears +90), VocClass::PlayAt 0x007509E0, operator new 0x007C8E17 (scratch storage), ParticleSystemClass::ParticleSystemClass 0x0062DC50",
            "Warhead, sound and spark type are marker values passed through",
        ],
        entry_points={"constructor": CTOR, "update": UPDATE, "random_ranged": RANDOM_RANGED,
                      "static_target": STATIC_TARGET_INIT})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
