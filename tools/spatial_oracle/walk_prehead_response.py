"""Original Walk 75B696 result response, with explicitly supplied CanEnter classes.

Interior caller controls: NOT proof of Infantry51BF90 returning those classes.
Ordinary controls stop before recursive Process; explicit continuation controls
execute it with supplied path and admission answers. See walk_prehead_response.md.
"""
from pathlib import Path
import struct
from unicorn.x86_const import *
from tools.native_oracle import STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle import walk_head_occupation as h
from tools.spatial_oracle.map_queries import dwords, packed

HERE = Path(__file__).resolve().parent
HOUSE, RULES, AUX = SCRATCH + 0x10000, SCRATCH + 0x16000, SCRATCH + 0x18000
OTHER, OTHER_TYPE, OTHER_HOUSE = SCRATCH + 0x21000, SCRATCH + 0x23000, SCRATCH + 0x25000
OVERLAY_TYPES, OVERLAY_TYPE = SCRATCH + 0x31000, SCRATCH + 0x32000
TARGET_CELL = AUX + 0x1400


class Original(h.Original):
    def __init__(self, row):
        self.row = row
        self.active = False
        super().__init__()
        u = self.uc
        u.mem_map(SCRATCH + 0x10000, 0x30000)
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        u.mem_write(h.VTABLE, bytes(u.mem_read(0x7EB058, 0x600)))
        # Preserve the older original-head fixture's documented owner-index and
        # unrelated predicate seams. All decoder callbacks are original bodies.
        u.mem_write(h.VTABLE + 0x38, dwords(h.OWNER_GET))
        for offset in (0x1D4, 0x1D8, 0x37C):
            u.mem_write(h.VTABLE + offset, dwords(h.FALSE_GET))
        u.mem_write(h.VTABLE + 0x54C, dwords(h.STOP_EVENT))
        u.mem_write(h.CELL, dwords(0x7E4EEC))
        u.mem_write(h.CURRENT, dwords(0x7E4EEC))
        u.mem_write(h.OWNER + 0x6C0, dwords(h.TYPE))
        u.mem_write(h.OWNER + 0x21C, dwords(HOUSE))
        u.mem_write(h.OWNER + 0xAC, dwords(row.get('mission', 8)))
        u.mem_write(h.OWNER + 0xB4, dwords(row.get('queued_mission', -1)))
        u.mem_write(h.OWNER + 0x6C4, dwords(row.get('doing', 6)))
        u.mem_write(h.OWNER + 0x90, b'\1')
        u.mem_write(h.OWNER + 0x684, b'\xff')
        u.mem_write(HOUSE + 0x1EC, b'\1')
        u.mem_write(h.TYPE + 0x5B4, dwords(7))
        u.mem_write(h.OWNER + 0x5A4, dwords(h.CELL))
        u.mem_write(h.OWNER + 0x2B4, dwords(0))
        u.mem_write(h.OWNER + 0x5E0, dwords(2, 3, 4, 5, *([-1] * 20)))
        u.mem_write(h.OWNER + 0x558, packed(*row.get('path_reference', [9, 8])))
        u.mem_write(h.OWNER + 0x640, dwords(*row.get('movement_timer', [50, 0, 5])))
        u.mem_write(h.OWNER + 0x668, dwords(*row.get('blockage_timer', [40, 0, 6])))
        u.mem_write(h.OWNER + 0x64C, dwords(row.get('retries', 4)))
        u.mem_write(h.OWNER + 0x68A, bytes([row.get('flag68a', True)]))
        u.mem_write(h.OWNER + 0x418, bytes([row.get('tethered', False)]))
        u.mem_write(h.OWNER + 0x3D5, bytes([row.get('in_playfield', False)]))
        u.mem_write(h.OWNER + 0x6B7, bytes([row.get('blocked', False)]))
        u.mem_write(0xA8ED84, dwords(row.get('frame', 100)))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0x1718, dwords(row.get('close_enough', 128)))
        u.mem_write(RULES + 0x1768, dwords(row.get('blockage_delay', 22)))
        u.mem_write(RULES + 0x1760, struct.pack('<d', row.get('path_delay', 0.01)))
        u.mem_write(RULES + 0x700, dwords(-1))
        u.mem_write(0x89EA40, struct.pack('<90f', *([1.0] * 90)))
        u.mem_write(0x89E7C0, dwords(104))
        u.mem_write(0x89F6D8, dwords(0,-256,256,-256,256,0,256,256,0,256,-256,256,-256,0,-256,-256))
        u.mem_write(h.MAP + 0xF4, dwords(8, 8, 0, 0, 8, 8))
        # Used by original path-failure zone checks, including retry exhaustion.
        u.mem_write(h.MAP + 0x68, dwords(AUX, 289))
        u.mem_write(h.MAP + 0x18 + 7 * 4, dwords(AUX + 0x800))
        u.mem_write(AUX + 0x800, packed(1, 2))
        if row.get('zone_rejected'):
            u.mem_write(AUX + (10 * 17 + 10) * 4 + 2, packed(1, 0))
        if row.get('attack_target'):
            tx, ty = row.get('target_cell', [11, 10])
            u.mem_write(h.OWNER + 0x2B4, dwords(TARGET_CELL))
            u.mem_write(h.TABLE + (ty * 512 + tx) * 4, dwords(TARGET_CELL))
            u.mem_write(TARGET_CELL, dwords(0x7E4EEC))
            u.mem_write(TARGET_CELL + 0x24, packed(tx, ty))
            u.mem_write(TARGET_CELL + 0x44, dwords(-1))
            u.mem_write(TARGET_CELL + 0x11B, b'\2\1')
            if row.get('target_zone_rejected'):
                u.mem_write(AUX + (ty * 17 + tx) * 4 + 2, packed(1, 0))
        if row.get('radio'):
            u.mem_write(h.OWNER + 0xE4, dwords(AUX + 0x1000, 1))
            u.mem_write(AUX + 0x1000, dwords(h.CELL))
        self.call(0x75AA90, h.LOCO, [])
        self.call(0x4C91C0, h.OWNER + 0x388, [])
        u.mem_write(h.LOCO + 0xC, dwords(h.OWNER))
        u.mem_write(h.OWNER + 0x674, dwords(h.LOCO + 4))
        u.mem_write(h.LOCO + 0x14, dwords(1))
        u.mem_write(h.LOCO + 0x34, bytes([1, 0, row.get('motion', 1)]))
        self.current = row.get('current', [9 * 256 + 192, 10 * 256 + 64, 260])
        self.prospective = [self.current[0] + 256, self.current[1], self.current[2]]
        self.destination = row.get('destination', [10 * 256 + 128, 10 * 256 + 128, 260])
        u.mem_write(h.OWNER + 0x9C, dwords(*self.current))
        u.mem_write(h.LOCO + 0x1C, dwords(*self.destination))
        u.mem_write(h.CELL + 0x140, dwords(row.get('candidate_flags', 0)))
        u.mem_write(h.CELL + 0x11B, bytes([row.get('candidate_level', 2) & 255, 1]))
        u.mem_write(h.CURRENT + 0xEC, dwords(row.get('current_land', 0)))
        u.mem_write(h.CELL + 0x124, dwords(row.get('candidate_raw', 0)))
        if row.get('current_missing'):
            cx, cy = (n // 256 if n >= 0 else -(-n // 256) for n in self.current[:2])
            u.mem_write(h.TABLE + (cy * 512 + cx) * 4, dwords(0))
            u.mem_write(h.DUMMY + 0xEC, dwords(row['dummy_land']))
        if 'wall' in row:
            u.mem_write(h.CELL + 0x44, dwords(0))
            u.mem_write(0xA83D84, dwords(OVERLAY_TYPES))
            u.mem_write(OVERLAY_TYPES, dwords(OVERLAY_TYPE))
            u.mem_write(OVERLAY_TYPE + 0x2A8, bytes([row['wall']]))
        if row.get('obstacle') or 'cloak' in row:
            u.mem_write(OTHER, dwords(0x7F5C70))
            u.mem_write(OTHER + 0x14, dwords(7))
            u.mem_write(OTHER + 0x6C4, dwords(OTHER_TYPE))
            u.mem_write(OTHER + 0x21C, dwords(HOUSE if row.get('allied') else OTHER_HOUSE))
            u.mem_write(OTHER_HOUSE + 0x30, dwords(1))
            u.mem_write(OTHER + 0x9C, dwords(*self.prospective))
            u.mem_write(OTHER + 0x90, bytes([row.get('obstacle_alive', True)]))
            u.mem_write(OTHER + 0x6C, dwords(100))
            u.mem_write(h.CELL + (0xE8 if row.get('obstacle_deck') else 0xE4), dwords(OTHER))
            u.mem_write(OTHER + 0x220, dwords(row.get('cloak', 0)))
            u.mem_write(RULES + 0x628, dwords(10))
            u.mem_write(RULES + 0x6A0, dwords(-1))
            u.mem_write(OTHER_TYPE + 0x310, dwords(4))
        self.call(0x65C6D0, h.SCENARIO + 0x218, [row.get('seed', 31)])
        self.events = []
        self.active = True

    def observe(self, u, address, size, data):
        if not self.active:
            return super().observe(u, address, size, data)
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.row.get('current_missing'):
            if address == 0x75B6C7:
                self.events.append(['class6_candidate', u.reg_read(UC_X86_REG_EAX)])
            elif address == 0x75B7D8:
                cell = u.reg_read(UC_X86_REG_EAX)
                self.events.append(['stop_band_cell', cell,
                                    list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4))),
                                    self.read32(cell + 0xEC)])
        if self.row.get('failed_retry_probe'):
            if address == 0x75B040:
                self.events.append(['failed_retry_distance', u.reg_read(UC_X86_REG_EAX)])
            elif address == 0x75B075:
                self.events.append(['failed_retry_counter', u.reg_read(UC_X86_REG_ECX)])
            elif address in (0x75B18F, 0x75B2AB):
                self.events.append(['failed_retry_zone_result', hex(address), u.reg_read(UC_X86_REG_EAX) & 255])
            elif address == 0x56D100:
                args = list(struct.unpack('<IIIIII', u.mem_read(sp + 4, 24)))
                points = [list(struct.unpack('<hh', u.mem_read(p, 4))) for p in args[:2]]
                self.events.append(['zone', *points, *args[2:]])
            elif address == 0x750920:
                sound = u.reg_read(UC_X86_REG_ECX)
                self.events.append(['scold_sound', sound - (1 << 32) if sound & (1 << 31) else sound,
                                    u.reg_read(UC_X86_REG_EDX),
                                    list(struct.unpack('<II', u.mem_read(sp + 4, 8)))])
        if address in (self.read32(0x7E11C8), self.read32(0x7E11CC)):
            p = self.read32(sp + 4)
            v = self.read32(p) + (1 if address == self.read32(0x7E11C8) else -1)
            u.mem_write(p, dwords(v))
            self.ret(4, v)
            return
        if address == 0x4D3920:
            self.events.append(['supplied_find_path', list(struct.unpack('<III', u.mem_read(sp + 4, 12)))])
            if self.row.get('continue_recursive') and self.row.get('path_found'):
                u.mem_write(h.OWNER + 0x5E0, dwords(2, 3, 4, 5, *([-1] * 20)))
            self.ret(12, self.row.get('path_found', False))
            return
        if address == 0x51BF90 and self.row.get('continue_recursive'):
            self.events.append(['supplied_recursive_admission', list(struct.unpack('<IIIII', u.mem_read(sp + 4, 20)))])
            self.ret(20, self.row.get('recursive_code', self.row['code']))
            return
        if address == 0x75AEC0:
            self.events.append(['recursive_process', self.read32(sp + 4)])
        if address == 0x481670:
            args = list(struct.unpack('<IIII', u.mem_read(sp + 4, 16)))
            self.events.append(['scatter', u.reg_read(UC_X86_REG_ECX), args])
        elif address == 0x578AD0:
            self.events.append(['gate', self.read32(sp + 4), list(struct.unpack('<hh', u.mem_read(self.read32(sp + 8), 4)))])
        elif address in (0x521B20, 0x4D3710, 0x75ADA0, 0x51AA40, 0x521DD0, 0x4DC030, 0x521EB0, 0x483480, 0x65AE30, 0x75C240, 0x703850, 0x7036C0):
            self.events.append(hex(address))
        elif address in (0x65C780, 0x65C7E0):
            self.events.append(['rng', hex(address)])
        elif address == 0x7509E0:
            sound = u.reg_read(UC_X86_REG_ECX)
            xyz = list(struct.unpack('<iii', u.mem_read(u.reg_read(UC_X86_REG_EDX), 12)))
            self.events.append(['sound', sound - (1 << 32) if sound & (1 << 31) else sound, xyz, self.read32(sp + 4)])
        elif address == 0x4D8F40:
            self.events.append(['override', list(struct.unpack('<III', u.mem_read(sp + 4, 12)))])
        return super().observe(u, address, size, data)

    def state(self):
        u = self.uc
        def ints(p, n): return list(struct.unpack('<' + 'i' * n, u.mem_read(p, n * 4)))
        state = dict(doing=ints(h.OWNER + 0x6C4, 1)[0], path=ints(h.OWNER + 0x5E0, 4),
                    head=ints(h.LOCO + 0x28, 3), destination=ints(h.LOCO + 0x1C, 3),
                    moving=u.mem_read(h.LOCO + 0x34, 1)[0], motion=u.mem_read(h.LOCO + 0x36, 1)[0],
                    speed=struct.unpack('<d', u.mem_read(h.OWNER + 0x578, 8))[0],
                    blocked=u.mem_read(h.OWNER + 0x6B7, 1)[0], retries=ints(h.OWNER + 0x64C, 1)[0],
                    movement_timer=ints(h.OWNER + 0x640, 3), blockage_timer=ints(h.OWNER + 0x668, 3),
                    nav=self.read32(h.OWNER + 0x5A4), target=self.read32(h.OWNER + 0x2B4),
                    suspended_nav=self.read32(h.OWNER + 0x5A8),
                    suspended_target=self.read32(h.OWNER + 0x2B8),
                    mission=ints(h.OWNER + 0xAC, 1)[0],
                    queued_mission=ints(h.OWNER + 0xB4, 1)[0],
                    suspended_mission=ints(h.OWNER + 0xB0, 1)[0],
                    obstacle_cloak=ints(OTHER + 0x220, 8),
                    flag68a=u.mem_read(h.OWNER + 0x68A, 1)[0],
                    rng=bytes(u.mem_read(h.SCENARIO + 0x218, 0x3F4)).hex())
        if self.row.get('current_missing'):
            state['dummy'] = dict(cell=list(struct.unpack('<hh', u.mem_read(h.DUMMY + 0x24, 4))),
                                  land=self.read32(h.DUMMY + 0xEC))
        return state

    def execute(self):
        u = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(sp, bytes(0x80))
        u.mem_write(sp + 0x10, packed(10, 10))
        u.mem_write(sp + 0x14, dwords(2))
        u.mem_write(sp + 0x3C, dwords(*self.prospective))
        u.mem_write(sp + 0x48, dwords(RET_MAGIC, self.row.get('restart', False)))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_EBP, h.LOCO)
        u.reg_write(UC_X86_REG_EBX, h.LOCO + 0x28)
        u.reg_write(UC_X86_REG_EAX, self.row['code'])
        before = self.state()
        endpoints = RET_MAGIC if self.row.get('continue_recursive') else (RET_MAGIC, 0x75AEC0)
        run_checked(u, 0x75B696, endpoints, count=100000,
                    required_addresses=[0x75B696])
        endpoint = u.reg_read(UC_X86_REG_EIP)
        continuation = None
        if endpoint == 0x75AEC0:
            continuation = dict(this=u.reg_read(UC_X86_REG_ECX),
                                argument=self.read32(u.reg_read(UC_X86_REG_ESP) + 4))
        return dict(input=self.row, before=before, after=self.state(), endpoint=hex(endpoint),
                    recursive_process=continuation, events=self.events)


def inputs():
    for code in range(8):
        for restart in (False, True):
            yield dict(code=code, restart=restart)
    for code in (0, 1, 2, 6, 7):
        for doing in (3, 17, 0, 5):
            yield dict(code=code, doing=doing)
    for options in [dict(movement_timer=[95,0,10]), dict(blocked=True),
                    dict(blocked=True,blockage_timer=[95,0,10]), dict(path_found=True),
                    dict(blocked=True,path_found=True), dict(zone_rejected=True),
                    dict(path_delay=0),dict(path_delay=0.02),dict(path_delay=-0.01)]:
        yield dict(code=2,**options)
    for options in [dict(movement_timer=[95,0,5]),dict(movement_timer=[95,0,6]),
                    dict(movement_timer=[-1,0,1]),dict(movement_timer=[-1,0,0]),
                    dict(blockage_delay=0),dict(blocked=True,movement_timer=[95,0,10]),
                    dict(mission=15),dict(mission=-1,queued_mission=15)]:
        yield dict(code=2,**options)
    for options in [dict(close_enough=203),dict(close_enough=202),dict(close_enough=1000),
                    dict(close_enough=1000,radio=True),dict(close_enough=1000,current_land=10),
                    dict(close_enough=1000,destination=[2688,2688,468]),
                    dict(candidate_flags=256,current=[2496,2624,520]),
                    dict(candidate_flags=256,current=[2496,2624,416]),
                    dict(candidate_flags=256,current=[2496,2624,104]),
                    dict(candidate_flags=256,current=[2496,2624,-104])]:
        yield dict(code=6,**options)
    for code in (4, 5):
        for options in [dict(obstacle=True), dict(obstacle=True,allied=True),
                        dict(obstacle=True,restart=True), dict(obstacle=True,obstacle_deck=True),
                        dict(wall=True),dict(wall=False),dict(wall=True,restart=True)]:
            yield dict(code=code,**options)
    for cloak in (0, 1, 2, 3):
        yield dict(code=1,cloak=cloak)
    yield dict(code=1,cloak=2,obstacle_alive=False)
    yield dict(code=1,cloak=2,obstacle_deck=True)
    for row in [dict(code=0,motion=0),dict(code=0,candidate_raw=0x1C),
                dict(code=0,current=[2432,2688,260])]:
        yield row
    for code in (1, 4, 5, 6, 7):
        for path_found in (False, True):
            yield dict(code=code,restart=True,continue_recursive=True,path_found=path_found)
    for options in [dict(code=7,recursive_code=0),dict(code=6,recursive_code=0),
                    dict(code=4,obstacle=True),dict(code=5,wall=True),
                    dict(code=1,cloak=2)]:
        yield dict(options,restart=True,continue_recursive=True,path_found=True)
    # Required no-queue failure continuation reached by the one-shot retry.
    # All previous 101 rows remain unchanged; only these rows add branch events.
    failure = dict(code=7,restart=True,continue_recursive=True,path_found=False,
                   failed_retry_probe=True)
    for retries in (0, 1, 2, 10, 256, 0xFFFFFFFF):
        yield dict(failure,retries=retries)
    for close_enough in (202, 203):
        for tethered in (False, True):
            yield dict(failure,close_enough=close_enough,tethered=tethered)
    for options in [dict(retries=0,in_playfield=True),
                    dict(retries=0,flag68a=False),
                    dict(retries=0,in_playfield=True,flag68a=False),
                    dict(retries=0,in_playfield=True,mission=15),
                    dict(retries=0,in_playfield=True,attack_target=True),
                    dict(retries=0,in_playfield=True,attack_target=True,target_zone_rejected=True),
                    dict(retries=0,attack_target=True,target_zone_rejected=True),
                    dict(retries=1,in_playfield=True,attack_target=True,target_zone_rejected=True),
                    dict(path_reference=[-17,31]),
                    dict(mission=15),dict(mission=-1,queued_mission=15)]:
        yield dict(failure,**options)
    for dummy_land in (0, 10):
        yield dict(code=6,restart=False,close_enough=1000,current_missing=True,
                   dummy_land=dummy_land)


def generate():
    return [Original(row).execute() for row in inputs()]


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'walk_prehead_response.json', provenance=lambda: provenance(
        scope='Original Walk75B696 decoder through caller return or recursive75AEC0 boundary, plus explicit continued-recursion controls including failed no-queue continuation75AFEE..75B2DC. Supplied admission classes0..7; no claim that51BF90 produces a class or that an AStar/FindPath supplied answer is native.',
        assumptions=['Interior frame models original ready path2,3,4,5 and candidateCell10,10 from currentCell9,10. EAX is supplied class. The input key restart is the original ProcessMovement argument: true is the active virtualProcess75AC80 first pass, false is the internal recursive pass. Stack+40 contains prospectiveY2624 as in the real caller, copied into unused timer pause word.',
                     'Original Infantry vtable, Walk and Facing constructors, ordinary alive ENGINEER-shaped mission8/typeMovementZone7, no aircraft/JumpJet flags, default zero target/Team/radio. NavCom is candidate Cell. Cells are level2/slope1; synthetic one-row paths and map raw-zone data are supplied. Candidate lists are empty except explicit one-Unit ground/deck obstacle or cloak rows.',
                     'Frame100, supplied Rules CloseEnough128, BlockagePathDelay22, PathDelay0.01; explicit boundary contrasts. Random seed31 and complete1012-byte state preserved before/after. Native placement, map queries, height, timers and every reached decoder callback execute.',
                     'Obstacle/cloak rows supply one real Unit vtable receiver with Abstract flags7, health100, owner house index1 versus mover0 (same-house ally contrast), type at+6C4 and exact candidateXYZ. Actual Cell47C5A0/47C3D0 ground picking, House4F9A90, Foot4D8F40/Techno7013A0/Mission5B3650 override and original Infantry setters execute. Explicit wall rows supply overlay0 and OverlayType+2A8 flag; actual Cell-target Override executes. No surrounding combat, weapon firing or restoration claim.',
                     'Cloak rows execute original ground-list483480,703850 and7036C0 with supplied cloak state0..3,Rules+CloakingStages offset628=10,type+CloakingSpeed offset310=4,Rules+CloakSound6A0=-1. Original7509E0 sound entry and no-sound return execute; this does not validate audible output. Dead-object and upper-list contrasts expose absent alive/type filtering and ground-only dispatch.',
                     'Failed-retry probes append21 controls to the original101. They supply full32-bit retries+64C, radio tether+418, in_playfield+3D5, ScoldSound latch+68A, optional Cell attack target11,10 and connected/disconnected raw-zone row. Original distance41C380, retry arithmetic, callbacks, Map56D100 and original750920 quiet return with Rules+700=-1 execute. Zone events retain raw32-bit stack arguments; boolean arguments use only their low byte. Default Type+D94=false, Type+C94=false, mission_only+3D4=false and Team=NULL bound Infantry/allow-fringe callbacks. These rows do not execute Unlimbo or radio flag writers, audible output, arbitrary target classes or FindPath side effects.',
                     'Two current_missing controls append to the first122 unchanged rows. Class6/ProcessMovement(false)/CloseEnough1000 uses physicalXYZ2496,2624,260 and destination2688,2688,260, with sourceMap slot9,10=NULL, retained candidateCell10,10 allocated, and supplied DummyABDC50+EC land0 versus10. Original candidate lookup5657A0 and current-coordinate lookup565730 execute; the latter mutates only Dummy coordinates. Events capture actual candidate and stop-band Cell receivers, and before/after state captures retained Dummy land. This remains an interior supplied-class boundary, not proof that original CanEnter would produce class6 on a missing source cell.',
                     'Head fixture owner-index/false-predicate/Stop-event and absent-gate lookup seams retained and declared. No full object constructor, retail rules loader, map loader, dynamic gate/scatter recipient or nonempty scatter proof. Ordinary rows stop before recursive Process. continue_recursive rows execute the original recursive caller with supplied FindPath and admission answers, octant lepton-vector table89F6D8, full coordinate/map/height/query-argument producers and decoder consequences.'],
        substitutions=['CanEnter return EAX is supplied at interior75B696, not executed; this is a result-decoder comparison.',
                       'FindPath4D3920 return is supplied with original12-byte argument cleanup and no state effects EXCEPT continue_recursive/path_found rows also supply ready path2,3,4,5,-1..-1. Its wrappers/AStar are not part of this comparison. Actual next caller timer and+4F4/+4F8/zone/null-setter callbacks execute. In continue_recursive rows51BF90 records the actual five arguments and returns supplied recursive_code (default same class), with original20-byte cleanup;51BF90 itself is not executed.',
                       'Inherited head fixture +38 owner index41, +1D4/+1D8/+37C false, +54C Stop callback observed no-op,47C4D0 absent gate. OS Interlocked increment/decrement update their supplied counter. No other reached gameplay callback is replaced.'],
        entry_points={'decoder':0x75B696,'constructor':0x75AA90,'infantry_stop_anim':0x521B20,
                      'path_result_seam':0x4D3920,'path_failure':0x521DD0,'foot_failure':0x4DC030,
                      'path_success':0x521EB0,'head':0x75C240,'gate':0x578AD0,
                      'scatter':0x481670,'null_destination':0x51AA40,'stop':0x75ADA0,
                      'failed_retry':0x75AFEE,'distance':0x41C380,'zone':0x56D100,
                      'allow_destination_fringe':0x4DA1D0,'should_be_on_bridge':0x4DDC40,
                      'scold_sound':0x750920,
                      'recursive_endpoint':0x75AEC0}))
