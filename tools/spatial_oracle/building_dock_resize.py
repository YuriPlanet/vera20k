"""Original BuildingType growth caller and CoordStruct vector resize.

Only allocation/free are supplied by a deterministic scratch heap. Native copy
and zeroing loops execute unchanged, including an observed negative-index write.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, SP, dwords

OLD, NEW = SCRATCH + 0x8000, SCRATCH + 0xA000


def generate():
    rows = []
    for previous, capacity, count in [(1,1,4),(4,4,2),(2,4,4),(2,4,5),(0,4,5),(-1,4,5),(1,1,300)]:
        f = Fixture()
        u = f.u
        allocations, frees = [], []
        def heap_hook(uc, address, size, data):
            if address not in (0x7C8E17, 0x7C8B3D):
                return
            esp = uc.reg_read(UC_X86_REG_ESP)
            ret, argument = struct.unpack('<II', uc.mem_read(esp, 8))
            if address == 0x7C8E17:
                assert argument <= 3600
                allocations.append(argument)
                uc.mem_write(NEW-12, dwords(0x11223344,0x55667788,0x99AABBCC))
                uc.mem_write(NEW, bytes([0xCC])*argument)
                uc.reg_write(UC_X86_REG_EAX, NEW)
            else:
                frees.append(argument)
            uc.reg_write(UC_X86_REG_ESP, esp+4)
            uc.reg_write(UC_X86_REG_EIP, ret)
        u.hook_add(UC_HOOK_CODE, heap_hook)
        initial = [[i+1, -(i+1), 1000+i] for i in range(capacity)]
        u.mem_write(OLD, b''.join(dwords(*xyz) for xyz in initial))
        f.write(TYPE+0x1780, count)
        f.write(TYPE+0x1784, 0x7E4638)
        f.write(TYPE+0x1788, OLD)
        f.write(TYPE+0x178C, capacity)
        u.mem_write(TYPE+0x1790, bytes([1,1]))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, TYPE)
        u.reg_write(UC_X86_REG_EDI, previous)
        u.reg_write(UC_X86_REG_EAX, count)
        run_checked(u, 0x46494B, 0x46499D, count=100000,
                    required_addresses=[0x464951])
        assert u.reg_read(UC_X86_REG_ESP) == SP
        result_ptr, result_capacity = struct.unpack('<II', u.mem_read(TYPE+0x1788,8))
        slots=[list(struct.unpack('<iii',u.mem_read(result_ptr+12*i,12))) for i in range(result_capacity)]
        guard=list(struct.unpack('<III',u.mem_read(NEW-12,12))) if allocations else None
        rows.append(dict(previous=previous,capacity=capacity,count=count,initial=initial,
                         result_capacity=result_capacity,slots=slots,allocations=allocations,
                         freed_old=bool(frees),new_allocation_prefix_guard=guard))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original46494B..46499D growth branch and465E70 vector resize/copy; supplied parsed new count and previous count. Not complete Rules/ART caller.',
        assumptions=['Prior count may be smaller than allocated capacity after an earlier shrink.',
                     'Mapped prefix guard observes original out-of-bounds write for previous=-1; this is evidence of invalid native memory access, not an admitted portable coordinate value.'],
        substitutions=['7C8E17 allocation and7C8B3D free use deterministic scratch heap; gameplay copy/zero loops unchanged.'],
        entry_points={'growth':0x46494B,'resize':0x465E70}))
