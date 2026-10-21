"""Bounded original MCV continuation branches, Drive turning and TryToDeploy's
fallback site table.

Run python -m tools.mcv_deploy_oracle --check (or explicit --write).
Live Ghidra 2026-09-10: event9 4C77EA..4C7812 stops/clears target/queues
Unload. Unit::Mission_Unload 73D630: states0/1/2 reach the blocks below;
misfaced Unit::Deploy 739650 writes runtime +68C then returns1. Drive's
Do_Turn4B0EF0 delegates owner+388 to FacingClass::Set4C9220.

This executes original instruction bytes. It does NOT execute an entire mission,
placement/conversion, event producer, locomotion tick, cadence or RNG. Mission
fixtures deliberately supply callee-return/liveness/flag inputs at interior block
boundaries; paths stop before external calls. Facing fixtures execute the complete
original Drive Do_Turn and FacingClass::Current with retained original state.
"""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import (SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE, call, load_image,
                                RET_MAGIC, run_checked, finish_vectors, provenance)

UNIT = SCRATCH
IFACE = SCRATCH + 0x2000
OUT = SCRATCH + 0x2100
FRAME = 0x00A8ED84

def u32(n):
    return struct.pack('<I', n & 0xffffffff)

def branches():
    cases = []
    # Start AFTER first Deploy returns. Alive/flag are supplied helper results.
    fixtures = [('initial', alive, flag, 0, 0) for alive in (0, 1) for flag in (0, 1)]
    fixtures += [('retry', 1, flag, nav, result) for flag in (0, 1)
                 for nav in (0, 1) for result in (0, 1)]
    fixtures += [('state0', 1, 0, 0, 0)]
    for kind, alive, flag, nav, result in fixtures:
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(SCRATCH, SCRATCH_SIZE)
        uc.mem_write(UNIT + 0x90, bytes([alive]))
        uc.mem_write(UNIT + 0x68c, bytes([flag]))
        uc.mem_write(UNIT + 0x5a4, u32(nav))
        uc.mem_write(UNIT + 0xbc, u32(0 if kind == 'state0' else 1 if kind == 'initial' else 2))
        uc.mem_write(UNIT + 0x5e0, u32(42))
        uc.reg_write(UC_X86_REG_ESI, UNIT)
        uc.reg_write(UC_X86_REG_EBX, 0)
        uc.reg_write(UC_X86_REG_EAX, result)
        entry = {'initial': 0x73DDCB, 'retry': 0x73DD62, 'state0': 0x73DD8E}[kind]
        ends = (0x73DE3A, 0x73DDEB, 0x73DDA2)
        trace = run_checked(uc, entry, ends, count=100)
        cases.append(dict(kind=kind, alive=alive, flag=flag, nav=nav, result=result,
                          stop_address=hex(trace), out_state=int.from_bytes(uc.mem_read(UNIT+0xbc, 4),'little'),
                          out_flag=uc.mem_read(UNIT+0x68c,1)[0],
                          out_path_head=int.from_bytes(uc.mem_read(UNIT+0x5e0,4),'little',signed=True)))
    return cases

def turns():
    cases = []
    fixtures = [(0x4000,0x8000,128),(0x8000,0x8000,128),
                (0xff00,0,256),(0,0x8000,128),
                (0x4000,0x8000,0),(0x7f80,0x8000,256)]
    fixtures += [(start << 8, 0x8000, rate) for rate in (1280,2560)
                 for start in (0,64,128,255)]
    for start, target, rate in fixtures:
        initial = struct.pack('<IIIIII',start,start,0xffffffff,0,0,rate)
        turned = call(0x4B0EF0, stack_args=[IFACE,target],
                      writes={IFACE+8:u32(UNIT), UNIT+0x388:initial, FRAME:u32(100)},
                      dumps={'facing':(UNIT+0x388,24)}, required_addresses=[0x4C9220])
        original_state = bytes.fromhex(turned['dumps']['facing'])
        for duplicate in (False, True):
            state = original_state
            if duplicate:
                repeated = call(0x4B0EF0, stack_args=[IFACE,target],
                                writes={IFACE+8:u32(UNIT), UNIT+0x388:state, FRAME:u32(101)},
                                dumps={'facing':(UNIT+0x388,24)}, required_addresses=[0x4C9220])
                state = bytes.fromhex(repeated['dumps']['facing'])
                if state != original_state:
                    raise RuntimeError('same target restarted native FacingClass turn')
            # +0x0C is native uninitialized timer padding; do not capture it.
            for elapsed in (0,1,2,6,12,13,25,26,31,64,128,256):
                if duplicate and elapsed == 0:
                    continue  # duplicate was issued at frame101; never sample backwards
                res = call(0x4C93D0, ecx=UNIT+0x388, stack_args=[OUT],
                           writes={UNIT+0x388:state,FRAME:u32(100+elapsed)}, dumps={'current':(OUT,2)})
                current=int.from_bytes(bytes.fromhex(res['dumps']['current']),'little')
                cases.append(dict(start=start,target=target,rate=rate,elapsed=elapsed,
                                  duplicate_at_one=duplicate,current=current,
                                  duration=int.from_bytes(state[16:20],'little')))
    return cases

def turn_completion():
    cases = []
    for rotating in (False, True):
        for previous in (False, True):
            uc = Uc(UC_ARCH_X86, UC_MODE_32)
            load_image(uc)
            uc.mem_map(SCRATCH, SCRATCH_SIZE)
            uc.mem_map(STACK_BASE, STACK_SIZE)
            uc.mem_write(IFACE+8, u32(UNIT))
            uc.mem_write(IFACE+0x5e, bytes([previous]))
            uc.mem_write(UNIT, u32(SCRATCH+0x3000))
            uc.mem_write(UNIT+0x388,struct.pack('<IIIIII',0x8000,0,100,0,10,1280))
            uc.mem_write(FRAME,u32(101 if rotating else 110))
            uc.reg_write(UC_X86_REG_ECX,UNIT)
            uc.reg_write(UC_X86_REG_ESI,IFACE)
            uc.reg_write(UC_X86_REG_EBX,0)
            uc.reg_write(UC_X86_REG_ESP,STACK_BASE+STACK_SIZE-0x100)
            # Execute original Is_Rotating call and latch writes. Stop BEFORE
            # movement helper or virtual PerCellProcess; callback itself excluded.
            stop=run_checked(uc,0x4B0775,(0x4B078C,0x4B08A4,0x4B08D1),
                             required_addresses=[0x4C9480])
            cases.append(dict(rotating=rotating,previous=previous,
                              out_previous=bool(uc.mem_read(IFACE+0x5e,1)[0]),
                              callback_zero=stop==0x4B08A4,stop_address=hex(stop)))
    return cases

SITES = 0xB1D010
SITES_END = 0xB1D0A0
SITES_GUARD = 0xB1CF9D

def try_to_deploy_sites():
    """UnitClass::TryToDeploy's function-local static CellStruct[36]: run its
    one-time initializer (0x738F9C..0x7392C2, guard bit clear) and stop before
    the atexit registration call; the loop at 0x7392CA reads it in this order."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_write(SITES_GUARD, bytes([0]))
    uc.reg_write(UC_X86_REG_EBX, 0)
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x100)
    run_checked(uc, 0x738F9C, 0x7392C2, count=1000)
    if uc.mem_read(SITES_GUARD, 1)[0] & 1 != 1:
        raise RuntimeError('initializer did not set its guard bit')
    raw = bytes(uc.mem_read(SITES, SITES_END - SITES))
    return [list(struct.unpack_from('<hh', raw, i)) for i in range(0, len(raw), 4)]


def deploy_admission():
    """Whole original CanDeploy and its Deploy caller's admission boundary.

    Uses the actual Unit vtable and prepared ordinary cell/MCV/yard data. No
    predicate, EMP result, coordinate result or continuation result is supplied.
    """
    rows = []
    typ, yard, table, cell = (SCRATCH + n for n in (0x2000, 0x5000, 0x7000, 0x8000))
    for entry, counter, death_frame in [
            ('can_deploy', 0, -1), ('can_deploy', 1, -1),
            ('can_deploy', -1, -1), ('can_deploy', 0x7fffffff, -1),
            ('can_deploy', 0, 0),
            ('unit_deploy', 0, -1), ('unit_deploy', 1, -1)]:
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(SCRATCH, SCRATCH_SIZE)
        uc.mem_map(STACK_BASE, STACK_SIZE)
        writes = {
            UNIT: u32(0x7F5C70), UNIT + 0x6C4: u32(typ),
            UNIT + 0x6D8: u32(death_frame), UNIT + 0x684: b'\xff',
            UNIT + 0x504: u32(counter), UNIT + 0x68C: b'\1',
            UNIT + 0x9C: struct.pack('<iii', 128, 128, 0),
            typ + 0x404: u32(yard), yard + 0x16B9: b'\1',
            0x87F924: u32(table), table: u32(cell),
            cell + 0x116: struct.pack('<h', -1),
        }
        for at, raw in writes.items():
            uc.mem_write(at, raw)
        before = bytes(uc.mem_read(UNIT, 0x1000))
        original = bytes(uc.mem_read(0x401000, 0x3e1000))
        frame_before = bytes(uc.mem_read(FRAME, 4))
        visits = []
        def observe(_uc, address, _size, _data):
            if address in (0x65C6D0, 0x65C780, 0x65C7E0, 0x7258D0):
                raise RuntimeError(f'unexpected RNG/Detach entry {address:08X}')
            if address in (0x7393C0, 0x700D50, 0x484AE0, 0x700E47,
                           0x746C90, 0x70EFD0):
                visits.append(f'{address:08X}')
        uc.hook_add(UC_HOOK_CODE, observe)
        sp = STACK_BASE + STACK_SIZE - 0x1000
        uc.mem_write(sp, u32(RET_MAGIC))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, UNIT)
        # Allowed Deploy stops before locomotion/placement; the refused path
        # executes its complete return, including resetting +68C.
        stops = (RET_MAGIC, 0x7393DA) if entry == 'unit_deploy' else RET_MAGIC
        stop = run_checked(uc, 0x7393C0 if entry == 'unit_deploy' else 0x700D50,
                           stops, count=20000,
                           required_addresses=(0x700D50, 0x484AE0, 0x700E47, 0x746C90))
        after = bytes(uc.mem_read(UNIT, 0x1000))
        assert original == bytes(uc.mem_read(0x401000, 0x3e1000))
        assert frame_before == bytes(uc.mem_read(FRAME, 4))
        rows.append(dict(entry=entry, emp_counter=counter, death_frame=death_frame,
            result=dict(allowed=stop == 0x7393DA if entry == 'unit_deploy'
                        else bool(uc.reg_read(UC_X86_REG_EAX) & 255),
                        stop_address=f'{stop:08X}',
                        counter_unchanged=before[0x504:0x508] == after[0x504:0x508],
                        changed_unit_offsets=[i for i, (a, b) in enumerate(zip(before, after)) if a != b],
                        deploy_retry=after[0x68C], native_visits=visits)))
    return rows

def generate():
    return {'source':'unicorn/gamemd.exe','mission_branches':branches(),'turns':turns(),'turn_completion':turn_completion(),
            'try_to_deploy_sites':try_to_deploy_sites(),'deploy_admission':deploy_admission()}

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='13 continuation interior branch cases, 322 complete native turn/current samples, four turn-completion latch cases, the 36-entry TryToDeploy site table and seven MCV EMP admission/caller controls',
        assumptions=['Unit and FacingClass layouts from live Ghidra disassembly; fresh emulator per case',
                     'mission fixtures start after substituted Deploy return, or after state0 radio call',
                     'no mission timing, RNG, placement, successful conversion or visual parity claim',
                     'FacingClass timer padding ignored; global frame is explicitly initialized',
                     'same-target duplicate Do_Turn at frame101 must preserve entire FacingClass state',
                     'turn completion executes original rotation predicate/latch branch, stopping before PerCellProcess call',
                     'site table runs the one-time initializer with the guard bit clear, stopping before its atexit call',
                     'MCV admission executes whole700D50 using actual Unit vtable7F5C70, actual cell tunnel check484AE0 and Unit EMP query746C90 to70EFD0; alive DeathFrame=-1 and a separate dying-unit control',
                     'Prepared ordinary cell, no tube, no parasite/mind controller, DeploysInto construction yard, no passengers; no final conversion/placement claim',
                     'Deploy7393C0 caller runs refused path to return; allowed path stops7393DA before locomotion/placement',
                     'Admission/caller controls preserve original executable bytes, the global frame and EMP counter; monitored RNG/Detach entries fail, but valid whole RNG stream initialization and all other timers are not claimed'],
        substitutions=['mission blocks supply Deploy return and resulting unit alive/flag/NavCom inputs; stop before further calls'],
        entry_points={'state0':0x73DD8E,'initial_result':0x73DDCB,'retry_result':0x73DD62,
                      'Drive_Do_Turn':0x4B0EF0,'Facing_Current':0x4C93D0,'Drive_turn_completion':0x4B0775,
                      'TryToDeploy_site_init':0x738F9C,'CanDeploySlashUnload':0x700D50,
                      'Unit_EMP_query':0x746C90,'Techno_EMP_query':0x70EFD0,
                      'Unit_Deploy_admission_caller':0x7393C0}))
