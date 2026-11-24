"""Execute retail Bullet timer production, save/load, and proximity checks.

Run: python -m tools.projectile_oracle.load_timers --check (configured RA2_DIR).
Fire's upstream coordinates/type/target are supplied. The actual late Fire
body, concrete WhatAmI receivers, detector, Save/Load bodies, size receiver,
global frame reader, and math execute. Hooks implement only IStream bytes and
pointer registration/fixup; there are no substituted timer/math results.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ECX, UC_X86_REG_EIP,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    NATIVE_FPCW, OracleError, finish_vectors, load_image, provenance, run_checked,
)

BASE = 0x20000000
BULLET, PTYPE, TARGET, STREAM, VTABLE, COORD, HOOK = [BASE+i*0x2000 for i in range(7)]
SP, STOP = BASE+0x40000, BASE+0x48000


def w32(u, a, n):
    u.mem_write(a, struct.pack('<I', n & 0xffffffff))


def r32(u, a):
    return struct.unpack('<I', u.mem_read(a, 4))[0]


def signed(n):
    return struct.unpack('<i', struct.pack('<I', n & 0xffffffff))[0]


def run(arm, launch_frame, elapsed, target_kind, distance, failed_load=False,
        origin=(3200,3200,500), reference=(3700,3200,500), candidate=None):
    candidate = candidate or (3700-distance,3200,500)
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(BASE, 0x50000)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    output, input_bytes, cursor = bytearray(), b'', 0
    bullet_load = False

    def hook(u, address, size, _):
        nonlocal cursor
        pop = None
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in (HOOK, HOOK+16):
            dest, length = r32(u, sp+8), r32(u, sp+12)
            if address == HOOK:
                output.extend(u.mem_read(dest, length))
            elif failed_load and bullet_load:
                u.reg_write(UC_X86_REG_EAX, 0x80004005)
                pop = 20
            else:
                if cursor+length > len(input_bytes):
                    raise OracleError("IStream read exceeds supplied fixture bytes")
                u.mem_write(dest, input_bytes[cursor:cursor+length])
                cursor += length
            if pop is None:
                u.reg_write(UC_X86_REG_EAX, 0)
                pop = 20
        elif address in (0x6CF240, 0x6CF2C0):
            # Supplied pointer registry accepts this isolated object's refs.
            u.reg_write(UC_X86_REG_EAX, 0)
            pop = 12 if address == 0x6CF240 else 16
        if pop is not None:
            u.reg_write(UC_X86_REG_EIP, r32(u, sp))
            u.reg_write(UC_X86_REG_ESP, sp+pop)

    u.hook_add(UC_HOOK_CODE, hook)

    def invoke(address, args=(), this=0, stop=STOP):
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, this)
        for i, word in enumerate((STOP, *args)):
            w32(u, SP+4*i, word)
        run_checked(u, address, stop, count=100000)
        return signed(u.reg_read(UC_X86_REG_EAX))

    for address, word in ((BULLET, 0x7E46E4), (BULLET+0xAC, PTYPE),
                          (BULLET+0x10C, TARGET if target_kind != 'none' else 0),
                          (TARGET, 0x7E22A4 if target_kind == 'aircraft' else 0x7F5C70),
                          (PTYPE+0x2F0, arm), (STREAM, VTABLE),
                          (VTABLE+0x10, HOOK), (VTABLE+0xC, HOOK+16),
                          (0xA8ED84, launch_frame)):
        w32(u, address, word)
    u.mem_write(BULLET+0x9C, struct.pack('<iii', *origin))
    u.mem_write(COORD, struct.pack('<iii', *candidate))
    invoke(0x4E1100, this=BULLET+0xB8)
    u.reg_write(UC_X86_REG_EBX, BULLET)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.mem_write(SP+0x44, struct.pack('<iii', *reference))
    run_checked(u, 0x468A3F, 0x468A98, count=10000)
    produced = list(struct.unpack('<10i', u.mem_read(BULLET+0xB8, 40)))
    saved_frame = (launch_frame+elapsed) & 0xffffffff
    w32(u, 0xA8ED84, saved_frame)
    before_mode = invoke(0x4E11F0, (COORD,), BULLET+0xB8)
    before = list(struct.unpack('<10i', u.mem_read(BULLET+0xB8, 40)))
    save_result = invoke(0x46AFB0, (BULLET, STREAM, 0))
    if save_result != 0 or len(output) != 4+0x160:
        raise OracleError(f"Bullet Save failed: HRESULT={save_result}, bytes={len(output)}")
    # Start in a different live frame. Execute the original global stream
    # reader through its third Read, which restores the saved frame before
    # the separately dispatched Bullet Load (outer ordering: 67E8B5 < 67F138).
    w32(u, 0xA8ED84, saved_frame+12345)
    input_bytes, cursor = struct.pack('<III', 0, 0, saved_frame), 0
    invoke(0x67F9C0, this=STREAM, stop=0x67FA22)
    if r32(u, 0xA8ED84) != saved_frame or cursor != len(input_bytes):
        raise OracleError("Global frame reader did not consume and restore the supplied saved frame")
    input_bytes, cursor = bytes(output), 0
    bullet_load = True
    load_result = invoke(0x46AE70, (BULLET, STREAM))
    if load_result != (-2147467259 if failed_load else 0):
        raise OracleError(f"Unexpected Bullet Load HRESULT: {load_result}")
    if cursor != (0 if failed_load else len(input_bytes)):
        raise OracleError("Bullet Load consumed an unexpected byte count")
    loaded = list(struct.unpack('<10i', u.mem_read(BULLET+0xB8, 40)))
    after_mode = invoke(0x4E11F0, (COORD,), BULLET+0xB8)
    after = list(struct.unpack('<10i', u.mem_read(BULLET+0xB8, 40)))
    admission = []
    for dropping, impact, rot, ranged in [(False,False,0,True), (True,False,0,True),
                                        (True,True,0,True), (True,False,8,False),
                                        (True,False,0,False)]:
        u.mem_write(BULLET+0xB8, struct.pack('<10i', *loaded))
        u.mem_write(PTYPE+0x29C, bytes([dropping]))
        u.mem_write(PTYPE+0x2A0, bytes([ranged]))
        w32(u, PTYPE+0x2DC, rot)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, BULLET)
        u.mem_write(SP+0x24, bytes(u.mem_read(COORD, 12)))
        u.mem_write(SP+0x18, bytes([impact]))
        at = run_checked(u, 0x467C0C, (0x467CA9, 0x467FBA), count=10000)
        admission.append(dict(dropping=dropping, impact=impact, rot=rot, ranged=ranged,
                              mode=signed(u.reg_read(UC_X86_REG_ESI)),
                              watermark=signed(r32(u, BULLET+0xDC)),
                              detonate=at == 0x467CA9))
    # Timer padding (+4/+10) is stack residue and not semantic input/output.
    def fields(words):
        return dict(first=[words[0], words[2]], arm=[words[3], words[5]],
                    reference=words[6:9], watermark=words[9])
    return dict(arm=arm, launch_frame=launch_frame, elapsed=elapsed,
                origin=list(origin), reference=list(reference), candidate=list(candidate),
                target_kind=target_kind, distance=distance, failed_load=failed_load,
                produced=fields(produced), before=fields(before), before_mode=before_mode,
                saved_frame=saved_frame, load_result=load_result, loaded=fields(loaded),
                after=fields(after), after_mode=after_mode, admission=admission)


def generate():
    rows = [run(arm, frame, elapsed, target, distance)
            for arm in [0, 1, 2, 10, 9999999, -1, 2147483647, -2147483648]
            for frame in [100, 0xfffffffe, 0x7ffffffe]
            for elapsed in [0, 1, 10]
            for target in ['unit', 'aircraft', 'none']
            for distance in [20, 80, 94, 600]]
    rows += [run(arm, frame, elapsed, 'unit', 20)
             for arm in [-1, 2]
             for frame, elapsed in [(0xffffffff, 10), (100, 0x80000000)]]
    rows += [run(10, 100, 1, 'unit', 0, origin=point, reference=(0,0,0), candidate=point)
             for point in [(63,1,1), (64,-1,2), (120,140,200), (65535,4096,-20000),
                           (2147483647,2147483647,2147483647), (-2147483648,1,-2147483648),
                           (1000000000,32767,1000000001), (123456789,-1987654321,987654321)]]
    rows += [run(10, 100, 1, 'unit', 20, failed_load=True)]
    return rows


def metadata():
    return provenance(
        scope='877 supplied Bullet late-Fire, proximity, Save/global-frame-read/Load and five AI-admission cases per row. Full firing, world scheduling, disk persistence, pointer fixup effects and downstream detonation are excluded.',
        assumptions=[
            'Each row starts with a fresh zero-filled mapped image and fixture objects; x87 control word is NATIVE_FPCW (0x0E7F). Upstream Bullet/ProjectileType/target identities, coordinates, Arm, frame and admission flags are supplied, not retail INI reads.',
            'Firer at Bullet+0xB0, ProjectileType+0x2A6 and unrelated fixture fields remain zero. Supplied native vtables select original Aircraft WhatAmI 0x41C180 or Unit WhatAmI 0x746E20; the null target is a separate case.',
            'Original proximity constructor 0x4E1100 runs before late Fire 0x468A3F..0x468A98. Original target WhatAmI and distance/timer arithmetic execute. Padding words are excluded from semantic observations.',
            'Original Bullet Save 0x46AFB0 runs through Abstract Save and its size receiver; the fixture requires HRESULT zero and 4+0x160 stream bytes. It then changes the live frame, executes global reader 0x67F9C0 through its third successful read to 0x67FA22, and separately invokes Bullet Load 0x46AE70. Outer load dispatch order is supplied, not executed.',
            'The one failed-load case supplies E_FAIL on Bullet stream reads after successful global-frame restoration. Successful loads must consume the full saved stream; failed loads consume none. The same supplied object address is retained; actual pointer relocation is excluded.',
            'Original detector 0x4E11F0 runs before and after load. Each of five admission probes restores the loaded detector bytes and supplies Dropping, prior impact, ROT and Ranged; 0x467C0C executes only to 0x467CA9 or 0x467FBA. The detonation body is not executed.',
            'No RNG or detach effects are established by these selected regions. Timer writes are observed in produced/before/loaded/after fields and admission watermark/mode; no full-game scheduling or persistence parity is claimed.',
        ],
        substitutions=[
            'Synthetic IStream Write/Read receivers collect or provide exact bytes, return S_OK, and pop16 argument bytes; failed Bullet reads return E_FAIL without consuming data.',
            'Pointer registration/fixup receivers 0x6CF240 and 0x6CF2C0 return zero and pop8/12 argument bytes. Fixture pointers remain unchanged.',
        ],
        entry_points=dict(proximity_ctor=0x4E1100, late_fire_begin=0x468A3F,
                          late_fire_end=0x468A98, check=0x4E11F0, save=0x46AFB0,
                          global_read=0x67F9C0, global_read_end=0x67FA22, load=0x46AE70,
                          admission_begin=0x467C0C, detonate_boundary=0x467CA9,
                          continue_boundary=0x467FBA),
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
