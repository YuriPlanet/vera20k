"""Whole zero-health DoAction -> failed-path -> WalkStop callback ordering.

The current-cell CanEnter virtual result is supplied at its body boundary; the
original action/failed-path/Stop code and original vtables remain unchanged.
"""
from pathlib import Path
import hashlib
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, SCRATCH
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.infantry_deploy_action import Fixture as Base, ACTOR

CELL = SCRATCH+0x1A000
TABLE = 0xC00000


class Fixture(Base):
    def __init__(self):
        super().__init__()
        self.can_enter = self.read(0x7EB058+0x1AC)
        self.answer = 0
        self.map_ready = False

    def observe(self, u, address, size, data):
        if address == 0x51D6F0 and not self.map_ready:
            table = bytearray(0x100000)
            struct.pack_into('<I', table, (10*512+10)*4, CELL)
            u.mem_write(TABLE, bytes(table))
            u.mem_write(0x87F7E8+0x13C, dwords(TABLE,0x40000))
            u.mem_write(0x87F924, dwords(TABLE))
            u.mem_write(CELL, dwords(0x7E4EEC))
            u.mem_write(CELL+0x24, struct.pack('<hh',10,10))
            self.map_ready = True
        if address == self.can_enter:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.events.append(dict(address=f'{address:08X}',
                                    supplied_can_enter=self.answer, state=self.state()))
            u.reg_write(UC_X86_REG_EAX, self.answer & 0xffffffff)
            u.reg_write(UC_X86_REG_EIP, self.read(sp)&0xffffffff)
            u.reg_write(UC_X86_REG_ESP, sp+24)
            return
        if address in (0x51DAF0,0x51DB46,0x51DBAC,0x51DBBE,0x4D55C0):
            self.events.append(dict(address=f'{address:08X}',state=self.state()))
        super().observe(u,address,size,data)

    def execute(self, row):
        self.answer = row.get('can_enter',0)
        self.map_ready = False
        original = bytes(self.u.mem_read(0x51DAF0,0xDF))
        result = super().execute(row)
        result['cell_entry_blocked'] = self.u.mem_read(ACTOR+0x6DC,1)[0]
        assert original == bytes(self.u.mem_read(0x51DAF0,0xDF))
        return result


def generate():
    f=Fixture()
    rows=[dict(kind='action',health=hp,doing=-1,request=request,prone=prone,
               pending=pending,can_enter=answer,moving=1,motion=1)
          for hp in (0,-1,1) for request in (0,2,27,31)
          for prone,pending,answer in ((0,0,0),(1,0,1),(1,1,0))]
    rows += [dict(kind='callback',health=0,doing=doing,counts=[[28,count]],
                  can_enter=answer) for doing in (0,31) for count in (0,6)
             for answer in (0,2)]
    return [f.execute(row) for row in rows]


def metadata():
    f=Fixture()
    out = provenance(
        scope='Whole DoAction zero-health synchronous failed-path action and Stop callback; bounded supplied current-cell admission result.',
        assumptions=[
            'Original action51D6F0, failed-path51DAF0, WalkStop75ADA0 and pending521B40 code/vtables remain unchanged. Original Map565730 resolves supplied Cell10,10 and original Facing/current-bridge query execute.',
            'Current-cell Infantry CanEnter virtual body result is supplied0/1/2; collision/occupation/type producers of that result are outside this fixture.',
            'Type/House/sequence/locomotor inputs and OS-interlocked support inherit infantry_deploy_action. Original Scenario RNG executes; full state captured.',
            'Health0 contrasts with signed health-1/+1. Ordinary movement zone, no carry/airborne/falling remaps and no sequence sound entries.'
        ], substitutions=[f'Infantry virtual CanEnter body{f.can_enter:08X}: return supplied EAX and original five-argument stack cleanup; original callable bytes/vtable unchanged'],
        entry_points={'do_action':0x51D6F0,'failed_path':0x51DAF0,'walk_stop':0x75ADA0})
    out['original_failed_path'] = dict(start='0051DAF0',end_exclusive='0051DBCF',
        hex=(code:=bytes(f.u.mem_read(0x51DAF0,0xDF))).hex(),sha256=hashlib.sha256(code).hexdigest())
    return out


class CheerFixture(Base):
    """Reuse the original Infantry/type/sequence transport for virtual388."""
    def __init__(self):
        super().__init__()
        self.bypass = False
        self.cheer_code = bytes(self.u.mem_read(0x522BC0, 0x68))

    def prepare_execution(self, row):
        self.bypass = row.get('bypass_admission', False)
        self.u.mem_write(ACTOR+0x90, bytes([row.get('alive', 1)]))
        self.u.mem_write(ACTOR+0x81, bytes([row.get('limbo', 0)]))
        #5B3040 uses the queued selector only when current Mission is-1.
        self.u.mem_write(ACTOR+0xB4, dwords(row.get('queued', -1)))

    def call(self, address, owner, args=()):
        if address == 0x51D6F0:
            address, args = 0x522C00, [int(self.bypass)]
        return super().call(address, owner, args)

    def execute(self, row):
        output = super().execute(row)
        assert self.cheer_code == bytes(self.u.mem_read(0x522BC0, 0x68))
        return output


def cheer_generate():
    base = dict(kind='action', health=100, pending=0, prone=0, moving=0,
                motion=0, doing=0, mission=5)
    rows = [dict(base, doing=doing, mission=mission, bypass_admission=bypass)
            for bypass in (False, True) for mission in (5, 11, 2)
            for doing in (-1, 0, 9, 10, 11, 32)]
    rows += [dict(base, mission=-1, queued=mission) for mission in (5, 11, 2)]
    rows += [dict(base, alive=0, limbo=1),
             dict(base, counts=[[32, 0]]),
             dict(base, doing=11, bypass_admission=True)]
    fixture = CheerFixture()
    return [fixture.execute(row) for row in rows]


def cheer_metadata():
    return provenance(
        scope='Infantry virtual388522C00 and CanCheer522BC0 through original DoAction/Stage, contrasting House argument0 with SlaveFree argument1',
        assumptions=[
            'Infantry/type/sequence/Mission/Walk prior state uses the existing infantry_deploy_action transport; no House50C8C0 array scan or TAction113 dispatcher executes in this corpus.',
            'Original Mission5B3040, CanCheer522BC0, Cheer522C00, DoAction51D6F0 and native Stage/action record bodies and vtables execute unchanged.',
            'Health100, ordinary land and no falling/loaded/wet remap. Refused unchanged, absent-sequence and uninterruptible controls retain Doing/Stage. Current-1 controls expose original queued-Mission fallback.',
            'No sequence sound entries. Existing disabled audio gate and OS interlocked transport are inherited; full rendered animation/audio/campaign progression remain outside coverage.'],
        substitutions=[
            'Existing infantry_deploy_action OS interlocked import support and explicit actor/type/sequence priors only; no Cheer, CanCheer, Mission or DoAction receiver replacement.'],
        entry_points=dict(can_cheer=0x522BC0, cheer=0x522C00,
                          get_mission=0x5B3040, do_action=0x51D6F0,
                          house_caller=0x50C8C0, slave_caller=0x6B0BD1))


if __name__ == '__main__':
    import argparse
    selector = argparse.ArgumentParser(add_help=False)
    selector.add_argument('--cheer', action='store_true')
    selected, remaining = selector.parse_known_args()
    if selected.cheer:
        finish_vectors(cheer_generate, Path(__file__).with_name('infantry_cheer.json'),
            provenance=cheer_metadata, argv=remaining,
            source_paths={'producer': Path(__file__),
                          'infantry_transport': Path(__file__).with_name('infantry_deploy_action.py')})
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'),
                       provenance=metadata, argv=remaining)
