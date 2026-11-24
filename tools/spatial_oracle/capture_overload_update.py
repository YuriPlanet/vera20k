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
# Additive Techno AI bracket controls reuse Fixture's image, constructor and
# manager owner. These scratch callback addresses never replace original code.
TECHNO_AI_ORDER = (0x6FA646, 0x6FA95B)
TECHNO_AI_DEAD = 0x6FAFFD
ORDER_TYPE, ORDER_BOMB, ORDER_SLAVE, ORDER_SPAWN = [
    SCRATCH + n * 0x4000 for n in range(8, 12)]
ORDER_CALLBACKS = {name: CALLBACKS + 0x100 * (i + 1) for i, name in enumerate(
    ('type', 'passive_gate', 'passive_acquire', 'slave', 'selfheal',
     'kind', 'height', 'cloak', 'spawn'))}
TECHNO_ORDER_MILESTONES = {
    0x6FA646: 'mission_control', 0x6FA65A: 'passive_admission',
    0x6FA6F5: 'bomb_admission', 0x6FA717: 'slave_admission',
    0x6FA726: 'capture_admission', UPDATE: 'capture_update',
    0x6FA735: 'alive_gate', 0x6FA743: 'selfheal_admission',
    0x6FA793: 'house_selfheal_admission',
    0x6FA941: 'cloak_admission', 0x6FA94C: 'spawn_admission',
}


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
        self.order_case = None
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
        if self.order_case is not None and self.observe_techno_order(pc):
            return
        if pc == RECEIVE_DAMAGE:
            args = [self.word(sp + 4 * n) for n in range(1, 8)]
            self.events.append(dict(damage=self.signed(args[0]), args=args[1:]))
            if self.kill:
                u.mem_write(OWNER + 0x90, b"\x00")
            if self.order_case is not None:
                u.mem_write(OWNER + 0x6C, dwords(max(0, self.word(OWNER + 0x6C) - self.signed(args[0]))))
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

    def order_snapshot(self):
        u = self.u
        return dict(health=self.signed(OWNER + 0x6C), alive=u.mem_read(OWNER + 0x90, 1)[0],
                    manager_present=self.word(OWNER + 0x2BC) != 0,
                    manager_count=self.signed(MGR + 0x34),
                    countdown=self.signed(MGR + 0x4C), flash=self.signed(MGR + 0x44))

    def observe_techno_order(self, pc):
        """Declared callback boundaries around the original AI instructions.

        CaptureManager Update/RNG execute normally. Mission, acquisition,
        bomb, slave, self-heal admission, house heal gate and presentation
        callbacks are isolated here; they do not establish those mechanisms.
        """
        u, case = self.u, self.order_case
        if pc in TECHNO_ORDER_MILESTONES:
            self.order_trace.append(dict(event=TECHNO_ORDER_MILESTONES[pc], **self.order_snapshot()))
        callbacks = {value: name for name, value in ORDER_CALLBACKS.items()}
        if pc in callbacks:
            name = callbacks[pc]
            if name not in ('type', 'kind', 'height'):
                self.order_trace.append(dict(event=name, **self.order_snapshot()))
            values = dict(type=ORDER_TYPE, passive_gate=1, passive_acquire=0,
                          slave=0, selfheal=int(case['selfheal']), kind=1,
                          height=0, cloak=0, spawn=0)
            self.ret(values[name], 4 if name == 'cloak' else 0)
            return True
        if pc == 0x5B3060:
            self.order_trace.append(dict(event='mission_dispatch', **self.order_snapshot()))
            self.ret(0, 0)
            return True
        if pc == 0x438A70:
            self.order_trace.append(dict(event='bomb_ready', **self.order_snapshot()))
            self.ret(1, 0)
            return True
        if pc == 0x438720:
            self.order_trace.append(dict(event='bomb_update', **self.order_snapshot()))
            if case.get('bomb_kills'):
                # Explicit destructive callback, not an emulated Bomb owner.
                # Production regressions own actual detonation and detach.
                u.mem_write(OWNER + 0x6C, dwords(0))
                u.mem_write(OWNER + 0x90, b'\x00')
                if case.get('bomb_frees_captures'):
                    u.mem_write(MGR + 0x34, dwords(0))
                else:
                    u.mem_write(OWNER + 0x2BC, dwords(0))
            self.ret(0, 0)
            return True
        if pc == 0x50D9D0:
            # The separate House repair bonus is excluded from these controls.
            self.ret(0, 0)
            return True
        return False

    def execute_techno_order(self, case):
        # Reuse the default owner's complete initializers and original manager
        # constructor, without a preceding manager visit or a second VM.
        self.execute(dict(name=case['name'], seed=1, captives=4, calls=0,
                          kill=case.get('capture_kills', False)))
        u = self.u
        u.mem_write(ORDER_TYPE + 0xA0, dwords(500))
        for offset, name in ((0x84, 'type'), (0x88, 'type'), (0x4C4, 'passive_gate'),
                             (0x4CC, 'passive_acquire'), (0x294, 'selfheal'),
                             (0x2C, 'kind'), (0x1C8, 'height'), (0x410, 'cloak')):
            u.mem_write(OVT + offset, dwords(ORDER_CALLBACKS[name]))
        for pointer, name in ((ORDER_SLAVE, 'slave'), (ORDER_SPAWN, 'spawn')):
            vtable = pointer + 0x100
            u.mem_write(pointer, dwords(vtable))
            u.mem_write(vtable + 0x5C, dwords(ORDER_CALLBACKS[name]))
        u.mem_write(OWNER + 0x6C, dwords(case['health']))
        u.mem_write(OWNER + 0xAC, dwords(5))
        u.mem_write(OWNER + 0x180, dwords(-1, 0, 0))
        u.mem_write(OWNER + 0x38, dwords(ORDER_BOMB if case.get('bomb') else 0))
        u.mem_write(OWNER + 0x2D8, dwords(ORDER_SLAVE if case.get('slave') else 0))
        u.mem_write(OWNER + 0x2D0, dwords(ORDER_SPAWN if case.get('spawn') else 0))
        u.mem_write(OWNER + 0x2BC, dwords(MGR))
        u.mem_write(MGR + 0x4C, dwords(case.get('countdown', 0)))
        u.mem_write(0xA8ED84, dwords(30))
        u.mem_write(RULES + 0x38, dwords(1))
        self.call(SEED, SCENARIO + 0x218, [1])
        self.events, self.order_trace = [], []
        self.order_case = case
        before = self.order_snapshot()
        # The actual AI prologue creates a 0x78-byte frame, EBP=0 and ESI=this.
        # Earlier AI and later continuation are explicit component boundaries.
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP - 0x78)
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBP
        u.reg_write(UC_X86_REG_ESI, OWNER)
        u.reg_write(UC_X86_REG_EBP, 0)
        end = run_checked(u, TECHNO_AI_ORDER[0], (TECHNO_AI_ORDER[1], TECHNO_AI_DEAD),
                          count=400000, context=dict(case=case))
        self.order_case = None
        return dict(input=case, before=before, trace=self.order_trace,
                    after=self.order_snapshot(), stopped_at=f'0x{end:08X}',
                    manager_events=self.events)


def techno_order_generate():
    """Small original-instruction bracket with explicitly isolated callees."""
    cases = [
        dict(name='surviving_overload_then_heal', health=500, selfheal=True,
             bomb=True, slave=True, spawn=True),
        dict(name='killing_overload_stops_before_heal', health=50, selfheal=True,
             capture_kills=True, bomb=True, slave=True, spawn=True),
        dict(name='killing_bomb_detaches_capture_before_visit', health=500,
             selfheal=True, bomb=True, bomb_kills=True, slave=False, spawn=True),
        dict(name='killing_bomb_frees_captures_before_visit', health=500,
             selfheal=True, bomb=True, bomb_kills=True, bomb_frees_captures=True,
             slave=False, spawn=True),
        dict(name='countdown_return_still_reaches_heal', health=499,
             selfheal=True, countdown=1, bomb=False, slave=False, spawn=False),
    ]
    rows = [Fixture().execute_techno_order(case) for case in cases]
    import os
    inputs = joined_inputs(Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS']))
    joined = joined_case(inputs, dict(name='stock_mind_original_ai_bracket', seed=1,
        captives=4, visits=1, techno_order=True, frame=30, countdown=0))
    return dict(schema=1, rows=rows, joined_inputs=inputs.overload_input, joined=joined)


def techno_order_metadata():
    return provenance(
        scope='Original TechnoClass AI 6FA646..6FA95B mission/passive/bomb/slave/capture/alive/selfheal/cloak/spawn ordering component',
        assumptions=[
            'Reuses the historical Fixture VM, image loader, original CaptureManager constructor4717D0, static target initializer4716F0, stock overload tables and original Scenario seed/RNG. There is no new runtime, duplicate joined bootstrap or altered original instruction.',
            'Entry6FA646 supplies the actual AI prologue frame shape ESP=entryESP-78, ESI=owner and EBP=0. Earlier Techno AI and continuation after6FA95B are excluded. Dead controls stop before the original epilogue6FAFFD. Guard mission5, expired passive timer, MIND-strength500/type-kind1 and due count4 are supplied control priors, not native scenario admission.',
            'Whole original CaptureManager Update471A50 executes if the owner still has its manager. Surviving controls retain original damage/voice/spark coordinate/RNG/countdown order and actual Techno health increment6FA756. The countdown1 control returns from Update without a hit and reaches subsequent heal in the same original bracket.',
            'Bomb, slave and spawn presence are synthetic independent control inputs. Ordinary retail MIND has neither slave nor spawn manager; those callback controls establish the shared AI slot order only. The separate joined overload corpus and production regression own stock MIND inputs and connected gameplay.',
            'One additional joined control reuses joined_inputs/joined_case and their original physical retail readers, whole MIND constructor and successful Unlimbo. It supplies count4/countdown0 at frame30, then runs the same full original6FA646..6FA95B bracket with no additional gameplay substitutions: real mission, passive timer gates, CaptureManager471A50, Unit737C90/Techno701900 receiver, selfheal gates and cloak execute. Constructor passive delay45 is not expired at frame30. No native captive nodes, captures, whole death, Unit outer AI or world tick is claimed.',
        ],
        substitutions=[
            'Inherited ReceiveDamage callback subtracts the actual original requested amount from supplied health and clears IsAlive for the killing-capture control; fatal native damage/FreeAll/detach are excluded. Existing PlayAt, allocation and ParticleSystem callbacks remain recorded and returned at entry.',
            'MissionClass5B3060, passive admission/acquisition owner slots4C4/4CC, Bomb438A70/438720, Slave and Spawn manager slot5C, selfheal admission294, type84/88, runtime kind2C, height1C8, house repair eligibility50D9D0 and cloak410 are declared returned callbacks. House repair cadence is supplied1 with eligibility false. These mechanisms and passive acquisition RNG are not reproduced by this component.',
            'Killing-bomb callbacks explicitly write health0/IsAlive0. The pointer-reload control also sets CaptureManager=NULL; the retained-manager control instead sets its captive count0 and lets original471A50 execute. Native fatal cleanup calls FreeAll702112/4DE5DD and defers ObjectUnInit5F65F0, so the latter is the representative count-zero/no-overload boundary. Neither callback implements native Bomb, FreeAll or detach; the NULL-pointer variant does not establish that retail fatal Bomb skips Update.',
        ], entry_points=dict(begin=TECHNO_AI_ORDER[0], continuation=TECHNO_AI_ORDER[1],
            dead_epilogue=TECHNO_AI_DEAD, capture_update=UPDATE,
            mission=0x5B3060, bomb_ready=0x438A70, bomb_update=0x438720,
            selfheal_increment=0x6FA756, cloak_call=0x6FA946, spawn_call=0x6FA958))


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


def joined_inputs(root):
    """Selected physical MIND/overload readers on the existing shared VM."""
    from collections import deque
    from types import SimpleNamespace
    from tools import native_oracle as native
    from tools.spatial_oracle import building_death_anims as host
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.building_body_rules import INI, SP
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.procedural_drawing_oracle.electric_bolt import read_spark_types, prepare_spark_capacity
    import hashlib

    m = host.joined_inputs(root)
    u, read = m.u, m.read32
    proxy = SimpleNamespace(m=m, trace=deque(maxlen=40), events=[], pending={}, frame=0, phase='setup')

    def transport(u, pc, size, data):
        if pc in host.JOINED_TRANSPORT - {0x527AF9, 0x527B0C}:
            Mission.observe(proxy, u, pc, size, data)
        elif pc == 0x7C978A:
            m.ret(0)  # Existing CRT exit-registration platform transport.

    hook = u.hook_add(UC_HOOK_CODE, transport)
    art = sections((root / 'ARTMD.INI').read_bytes())
    selected_art = {k: v for k, v in m.death_art_input_sections.items() if v}
    selected_art['MIND'] = art['MIND']
    m.make_ini(selected_art)
    art_cache = bytes(u.mem_read(INI, 0x40))
    kind = m.alloc(0xF00)
    m.invoke(0x7470D0, kind, (m.cstring('MIND'),))
    m.types['MIND'] = kind
    weapon = m.invoke(0x772FA0, m.cstring('MultipleMindControlTank'))
    sound = sections((root / 'SOUNDMD.INI').read_bytes())
    sound_name = sections((root / 'RULESMD.INI').read_bytes())['AudioVisual']['MasterMindOverloadDeathSound']
    selected_sound = {k: v for k, v in sound.items() if k in {sound_name, 'Defaults'}}
    selected_sound['SoundList'] = {k: v for k, v in sound['SoundList'].items() if v == sound_name}
    m.make_ini(selected_sound)
    m.invoke(0x7510D0, INI)
    layers, physical = [], []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            physical.append(dict(file=filename, absent=True))
            layers.append((filename, None, None))
            continue
        raw = path.read_bytes()
        all_sections = sections(raw)
        selected = {k: v for k, v in all_sections.items() if k in {
            'MIND', 'MultipleMindControlTank', 'PsychicControl', 'Controller', 'Super',
            'CombatDamage', 'AudioVisual'}}
        m.make_ini(selected)
        ini = m.alloc(0x40)
        u.mem_write(ini, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        m.invoke(0x747620, kind, (ini,))
        m.invoke(0x772080, weapon, (ini,))
        m.invoke(0x46BEE0, read(weapon + 0xA0), (ini,))
        m.invoke(0x75D3A0, read(weapon + 0xAC), (ini,))
        m.invoke(0x7729F0, weapon)
        c4 = m.invoke(0x75E3B0, m.cstring('Super'))
        m.invoke(0x75D3A0, c4, (ini,))
        if 'CombatDamage' in selected:
            # Existing bridge_landing_inputs owns this exact C4 reader seam.
            # The original ReadString/Warhead factory binds the pointer; never
            # supply it from the independently allocated Super above.
            for begin, end, delta in ((0x66C304, 0x66C34C, -4),
                                      (0x66C7A1, 0x66C914, 0)):
                u.mem_write(SP - 4, bytes(0x404))
                u.reg_write(UC_X86_REG_ESP, SP + delta)
                u.reg_write(UC_X86_REG_ESI, m.rules)
                u.reg_write(UC_X86_REG_EDI, ini)
                native.run_checked(u, begin, end, count=2_000_000)
        if 'AudioVisual' in selected:
            m.invoke(0x6691E0, m.rules, (ini,))
        layers.append((filename, None, raw))
        physical.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest()))
    prepare_spark_capacity(m, (0xA83C98, 0xA83D98, 0xA83D68, 0x8874C0))
    read_spark_types(m, layers, cache=m.make_ini, ini_pointer=INI)
    u.hook_del(hook)

    def vector(offset):
        return [struct.unpack('<i', u.mem_read(read(m.rules + offset + 4) + i * 4, 4))[0]
                for i in range(read(m.rules + offset + 16))]

    m.overload_input = dict(layers=physical,
        fixed_sources={filename: hashlib.sha256((root / filename).read_bytes()).hexdigest()
                       for filename in ('ARTMD.INI', 'SOUNDMD.INI')},
        type=dict(name='MIND', strength=read(kind + 0xA0),
                  weight_bits=f'{struct.unpack("<Q", u.mem_read(kind + 0x370, 8))[0]:016x}',
                  primary=m.string(read(kind + 0x898) + 0x24), primary_damage=read(weapon + 0xA4)),
        rules=dict(count=vector(0xEE8), damage=vector(0xF04), frames=vector(0xF20),
                   c4_warhead=m.string(read(m.rules + 0xFA8) + 0x24),
                   overload_sound_index=struct.unpack('<i', u.mem_read(m.rules + 0x258, 4))[0],
                   default_spark=m.string(read(m.rules + 0x1020) + 0x24)),
        spark_types=m.effective_types)
    return m


def joined_case(inputs, case):
    """Full nonlethal Update on an actual MIND, with explicit count-only prior."""
    from types import SimpleNamespace
    from tools.spatial_oracle import building_death_anims as host, building_construction as bc
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace, SparkBirth, prepare_spark_capacity
    from unicorn.x86_const import UC_X86_REG_EDI

    f = host.joined_fixture(inputs, 38, seed=case['seed'], unit_startup=True)
    u, read = f.u, f.read32
    for address in (0xA83C98, 0xA83D98):
        u.mem_write(address, bytes(inputs.u.mem_read(address, 24)))
    prepare_spark_capacity(SimpleNamespace(u=u, alloc=f.allocate), (0xA83D68, 0x8874C0))
    for entry in (STATIC_TARGET_INIT, 0x471750):
        bc.invoke(u, entry, 0)
    u.mem_write(bc.FRAME, dwords(0))
    actor = f.allocate(0x1000)
    bc.invoke(u, 0x7353C0, actor, f.types['MIND'], f.owner_houses[0])
    coord = f.allocate(12)
    u.mem_write(coord, dwords(2176, 2176, 0))
    answer = bc.invoke(u, 0x737BA0, actor, coord, 128) & 255
    assert answer == 1
    manager = read(actor + 0x2BC)
    assert manager and read(manager + 0x3C) == 3 and u.mem_read(manager + 0x40, 1)[0] == 1
    # Numeric/control seam only: no native capture, node traversal or fatal
    # FreeAll is claimed. Counts3/4/7 keep stock MIND alive through one check.
    u.mem_write(manager + 0x34, dwords(case['captives']))
    scenario = read(0xA8B230)
    rng = RngTrace.__new__(RngTrace)
    rng.u, rng.streams = u, {'main': 0x886B88, 'scenario': scenario + 0x218, 'mapgen': 0xABE890}
    rng.requests, rng.pending = [], []
    u.hook_add(UC_HOOK_CODE, rng.observe)
    systems, constructed, pending, events, audio_path, ai_order = [], [], [], [], [], []

    def xyz(pointer):
        return list(struct.unpack('<3i', u.mem_read(pointer, 12)))

    def snapshot():
        return dict(health=read(actor + 0x6C), alive=u.mem_read(actor + 0x90, 1)[0],
            limbo=u.mem_read(actor + 0x81, 1)[0], location=xyz(actor + 0x9C),
            rocking_bits=list(struct.unpack('<4I', u.mem_read(actor + 0x328, 16))),
            count=read(manager + 0x34), countdown=read(manager + 0x4C),
            flash=read(manager + 0x44), latch=u.mem_read(manager + 0x41, 1)[0])

    def observe(u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if case.get('techno_order') and pc in TECHNO_ORDER_MILESTONES:
            ai_order.append(dict(event=TECHNO_ORDER_MILESTONES[pc], **snapshot()))
        if pc in (PLAY_AT, 0x750A38, 0x750A53, 0x750A75, 0x750A7E, 0x750AA5):
            row = dict(pc=f'0x{pc:08X}', rng_request=len(rng.requests))
            if pc == PLAY_AT:
                row['sound_index'] = u.reg_read(UC_X86_REG_ECX)
            elif pc == 0x750A38:
                row['sound_object_present'] = u.reg_read(UC_X86_REG_EDI) != 0
            audio_path.append(row)
        if pending and (pc, sp) == pending[-1][:2]:
            _, _, pointer, row = pending.pop()
            view = SimpleNamespace(u=u, read32=read, scenario=scenario,
                                   systems=systems, particles=[], lights=[])
            row.update(state=SparkBirth.runtime_state(view)['systems'][-1],
                rng_request_after=len(rng.requests), native_cursor_after=read(scenario + 0x214))
            constructed.append(row)
            events.append(dict(kind='spark_return', native_id=row['state']['native_id'],
                               rng_request=len(rng.requests)))
        if pc == PARTICLE_SYSTEM_CTOR:
            pointer = u.reg_read(UC_X86_REG_ECX)
            systems.append(pointer)
            row = dict(type=f.string(read(sp + 4) + 0x24), position=xyz(read(sp + 8)),
                args=dict(target_object=read(sp + 12), owner=read(sp + 16),
                          target=xyz(read(sp + 20)), house=read(sp + 24)),
                rng_request_before=len(rng.requests), native_cursor_before=read(scenario + 0x214))
            pending.append((read(sp), sp + 28, pointer, row))
        elif pc == 0x737C90 and u.reg_read(UC_X86_REG_ECX) == actor:
            events.append(dict(kind='unit_receiver', damage=read(read(sp + 4)), distance=read(sp + 8),
                warhead=f.string(read(sp + 12) + 0x24), attacker=read(sp + 16),
                ignore_defenses=read(sp + 20), arg6=read(sp + 24), source_house=read(sp + 28),
                rng_request=len(rng.requests)))
        elif pc == 0x471C67:
            events.append(dict(kind='side_choice', value=u.reg_read(UC_X86_REG_EAX),
                               rng_request=len(rng.requests)))

    u.hook_add(UC_HOOK_CODE, observe)
    before, prior, cursor = rng.states(), snapshot(), read(scenario + 0x214)
    context = dict(actor_native_id=read(actor + 0x10),
        manager_native_id=struct.unpack('<i', u.mem_read(manager + 0x10, 4))[0],
        manager_max=read(manager + 0x3C), manager_infinite=u.mem_read(manager + 0x40, 1)[0],
        owner_native_id=read(read(actor + 0x21C) + 0x10),
        audio_global_8464ac=u.mem_read(0x8464AC, 1)[0], unlimbo_al=answer)
    if case.get('techno_order'):
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBP
        context['passive_timer'] = list(struct.unpack('<3i', u.mem_read(actor + 0x180, 12)))
        context['mission'] = struct.unpack('<i', u.mem_read(actor + 0xAC, 4))[0]
        u.mem_write(manager + 0x4C, dwords(case['countdown']))
        u.mem_write(bc.FRAME, dwords(case['frame']))
        prior = snapshot()
        u.mem_write(bc.SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, bc.SP - 0x78)
        u.reg_write(UC_X86_REG_ESI, actor)
        u.reg_write(UC_X86_REG_EBP, 0)
        run_checked(u, TECHNO_AI_ORDER[0], TECHNO_AI_ORDER[1], count=2_000_000)
    else:
        for frame in range(case['visits']):
            u.mem_write(bc.FRAME, dwords(frame))
            bc.invoke(u, UPDATE, manager)
    assert not pending and not rng.pending and f.code_unchanged()
    after = snapshot()
    expected_damage = 0 if case['captives'] <= 3 else (50 if case['captives'] <= 6 else 100)
    assert prior['health'] == 500 and after['health'] == 500 - expected_damage
    assert after['alive'] == 1 and after['limbo'] == 0
    assert len(constructed) == (0 if expected_damage == 0 else 5)
    result = dict(input=case, context=context, before=prior, after=after,
        rng_before=before, rng_after=rng.states(), rng_requests=rng.requests,
        raw_draw_count=sum(len(request['raw_draws']) for request in rng.requests),
        native_cursor_before=cursor, native_cursor_after=read(scenario + 0x214),
        events=events, particle_systems=constructed, audio_path=audio_path)
    if case.get('techno_order'):
        result['techno_ai_order'] = ai_order
    return result


def joined_generate():
    import os
    root = Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    inputs = joined_inputs(root)
    cases = [dict(name='three_no_overload', seed=1, captives=3, visits=31)]
    cases += [dict(name=f'count_{count}_seed_{seed}', seed=seed, captives=count, visits=31)
              for count in (4, 7) for seed in (1, 2)]
    rows = [joined_case(inputs, case) for case in cases]
    for count in (4, 7):
        assert {row['after']['rocking_bits'][2] >> 31 for row in rows
                if row['input']['captives'] == count} == {0, 1}
    return dict(schema=1, initial=inputs.overload_input, rows=rows)


def joined_metadata():
    return provenance(
        scope='Whole original CaptureManager471A50 on actual stock MIND with nonlethal receiver and five real Spark constructors',
        assumptions=[
            'Existing building_death_anims joined physical reader and Unit runtime own the VM, original factories, allocation, House/map and registry transport. Full MIND747620/MultipleMindControlTank772080/PsychicControl46BEE0/Controller75D3A0/Super75D3A0 and AudioVisual6691E0 readers execute in RULESMD, optional LANGRULE, Battle and Hills.map order; physical INI loading/whole Rules process and native MIND VXL loading are excluded.',
            'Original66C304..66C34C binds C4Warhead through actual ReadString128/Warhead factory. Original66C7A1..66C914 executes the three complete overload-vector reads. Existing electric_bolt.read_spark_types owns full physical SparkSys/Spark constructors/readers on this same VM; selected sound SoundList/7510D0 binds the real overload voice.',
            'Full native MIND7353C0/manager4717D0 and737BA0 execute at frame0 on supplied admitted flat Clear cells in the existing joined fixture. Extra inherited Building/MTNK and House priors remain component context. Original471750 initializes the CaptureManager registry,4716F0 the zero Spark target. Actual IDs, manager fields and Unlimbo result are read back.',
            'Only manager captive count3/4/7 is supplied after construction. Nodes and native captures are excluded; stock500-HP MIND survives the one selected check. No fatal FreeAll is run over invented nodes. Thirty-one monotonic Update calls include real737C90/701900 damage,7509E0 with observed raw8464AC input and branch PCs, and all five62DC50 constructors/CreateID/Unlimbo/Logic. No actor/particle AI, rocking integration, whole Logic or scene pixels are claimed. The Rust consumer compares native flash+44 at the retained before/after boundaries; this component excludes pip drawing and whole-scene equivalence.',
            'Full Main/Scenario/Mapgen before/after states, ranged rejection words/callers, constructor entry/return RNG checkpoints and unique-ID cursor preserve order. Four native f32 words328/32C/330/334 are observed; only330 is assigned. Numeric inputs and seeded world priors are not inferred from Rust output.',
        ],
        substitutions=[
            'Inherited bounded successful allocation/free, signed-CRC physical lexical INI and requested-SHP transport, existing Windows GUID/COM activation and CRT exit registration. No gameplay receiver, RNG, coordinate, particle constructor, identity, admission or return value is supplied. Count-only component entry and inherited audio/camera/device priors are explicit bounds, not native full-match or audible-output proof.',
        ], entry_points=dict(update=UPDATE, manager_ctor=CTOR, unit_ctor=0x7353C0,
            unit_unlimbo=0x737BA0, unit_receiver=0x737C90, techno_receiver=0x701900,
            spark_ctor=PARTICLE_SYSTEM_CTOR, ranged=RANDOM_RANGED, next=NEXT_RANDOM,
            rate_store=0x471C78, c4_reader=0x66C304, overload_readers=0x66C7A1))


if __name__ == "__main__":
    import sys
    if '--techno-order' in sys.argv:
        if '--joined' in sys.argv:
            raise SystemExit('--techno-order and --joined are separate corpus modes')
        finish_vectors(techno_order_generate, Path(__file__).with_name('capture_overload_techno_order.json'),
            provenance=techno_order_metadata, argv=[arg for arg in sys.argv[1:] if arg != '--techno-order'])
    elif '--joined' in sys.argv:
        finish_vectors(joined_generate, Path(__file__).with_name('capture_overload_joined.json'),
            provenance=joined_metadata, argv=[arg for arg in sys.argv[1:] if arg != '--joined'])
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
