"""Original post-PerCell Walk completion, including real destination/Stop receivers."""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ACTOR, TYPE, HOUSE, LOCO, VT, RULES = [SCRATCH + i * 0x2000 for i in range(6)]


def query(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x20000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)

    def read32(p):
        return struct.unpack('<I', u.mem_read(p, 4))[0]

    def words(p, count):
        return list(struct.unpack('<' + 'i' * count, u.mem_read(p, 4 * count)))

    def ret(cleanup, value):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xffffffff)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    events = []

    def observe(_u, address, _size, _data):
        if address in [read32(0x7E11C8), read32(0x7E11CC)]:
            p = read32(u.reg_read(UC_X86_REG_ESP) + 4)
            value = read32(p) + (1 if address == read32(0x7E11C8) else -1)
            u.mem_write(p, dwords(value))
            ret(4, value)
        elif address in [0x51AA40, 0x4D94B0, 0x75ADA0, 0x4D3710]:
            events.append(hex(address))

    u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(VT, bytes(u.mem_read(0x7EB058, 0x600)))
    u.mem_write(ACTOR, dwords(VT))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(HOUSE + 0x1EC, bytes([row['human']]))
    u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    u.mem_write(ACTOR + 0x6C4, dwords(row['doing']))
    u.mem_write(ACTOR + 0x684, b'\xff')
    u.mem_write(ACTOR + 0xAC, dwords(row['mission']))
    u.mem_write(ACTOR + 0xB4, dwords(row['queued_mission']))
    if row['contact']:
        u.mem_write(ACTOR + 0xE4, dwords(SCRATCH + 0x1A100))
        u.mem_write(ACTOR + 0xE8, dwords(1))
        u.mem_write(SCRATCH + 0x1A100, dwords(TYPE))
    u.mem_write(ACTOR + 0x90, bytes([row['alive']]))
    u.mem_write(ACTOR + 0x81, bytes([row['limbo']]))
    u.mem_write(ACTOR + 0x8D, bytes([row['falling']]))
    u.mem_write(ACTOR + 0x9C, dwords(*row['current']))
    u.mem_write(ACTOR + 0x5A0, dwords(123))
    u.mem_write(ACTOR + 0x5A4, dwords(TYPE))
    u.mem_write(ACTOR + 0x5E0, dwords(2, 3, 4, 5))
    u.mem_write(ACTOR + 0x598, dwords(2))
    u.mem_write(ACTOR + 0x640, dwords(50, 777, 5))
    u.mem_write(ACTOR + 0x668, dwords(40, 888, 6))
    u.mem_write(ACTOR + 0x64C, dwords(row['retries']))
    u.mem_write(ACTOR + 0x6B7, b'\x00')  # paid completion cleared it at75BE11
    u.mem_write(ACTOR + 0x578, struct.pack('<d', 0.75))
    u.mem_write(0xA8ED84, dwords(row['frame']))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x1768, dwords(row['blockage']))
    u.mem_write(0xB45BE8, dwords(0, 0, 0))
    u.mem_write(0xB45C28, dwords(104))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, LOCO)
    run_checked(u, 0x75AA90, RET_MAGIC, count=1000, required_addresses=[0x75AA90])
    u.mem_write(LOCO + 0xC, dwords(ACTOR))
    u.mem_write(LOCO + 0x14, dwords(1))
    u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
    u.mem_write(LOCO + 0x1C, dwords(*row['destination']))
    u.mem_write(LOCO + 0x28, dwords(*row['head']))
    u.mem_write(LOCO + 0x34, b'\x01\x00\x01')
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_EBX, LOCO + 0x28)
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_EDX, 0)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x75BE42, (0x75BF64, 0x75C1F1), count=10000,
                required_addresses=[0x75BE42])
    return dict(input=row, events=events, destination=words(LOCO + 0x1C, 3),
                head=words(LOCO + 0x28, 3),
                nav=read32(ACTOR + 0x5A4) != 0, aux=read32(ACTOR + 0x5A0),
                queue=words(ACTOR + 0x5E0, 4), nav_queue_count=read32(ACTOR + 0x598),
                movement_timer=words(ACTOR + 0x640, 3), blocked_timer=words(ACTOR + 0x668, 3),
                retries=read32(ACTOR + 0x64C), moving=u.mem_read(LOCO + 0x34, 1)[0],
                motion=u.mem_read(LOCO + 0x36, 1)[0],
                speed=struct.unpack('<d', u.mem_read(ACTOR + 0x578, 8))[0])


def generate():
    base = dict(human=1, doing=0, mission=1, queued_mission=-1, contact=0,
                alive=1, limbo=0, falling=0, head=[0, 0, 0],
                current=[16*256+192, 15*256+64, 0],
                destination=[16*256+128, 15*256+128, 0],
                retries=0xffffffff, frame=123, blockage=65536)
    variants = [{}]
    variants += [dict(destination=[16*256+128, 15*256+128, z])
                 for z in [-209, -208, -207, 207, 208, 209, -2147483648]]
    variants += [dict(destination=[0, 0, 0]), dict(destination=[17*256+128, 15*256+128, 0])]
    variants += [dict(doing=d, human=h) for d in [26, 27, 28, 29, 30, 31] for h in [0, 1]]
    variants += [dict(mission=7), dict(alive=0), dict(limbo=1), dict(falling=1)]
    variants += [dict(frame=f, blockage=b) for f in [0xffffffff, 0x80000000]
                 for b in [-2147483648, -1, 0, 60]]
    variants += [dict(queued_mission=7), dict(queued_mission=7, contact=1),
                 dict(mission=7, contact=1),
                 dict(head=[16*256+128, 15*256+128, 0]),
                 dict(head=[17*256+128, 15*256+128, 0]),
                 dict(current=[(16+65536)*256+192, 15*256+64, 0]),
                 dict(current=[-255, -255, 0], destination=[128, 128, 0]),
                 dict(current=[-256, -256, 0], destination=[128, 128, 0])]
    return [query(base | changes) for changes in variants]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Walk post-PerCell survival/destination completion75BE42..75BF64, including actual Infantry51AA40, Foot4D94B0, Foot speed4D3710 and WalkStop75ADA0. Does not execute prior movement/PerCell or final Mark1.',
        entry_points={'completion': 0x75BE42, 'infantry_setter': 0x51AA40,
                      'foot_setter': 0x4D94B0, 'walk_stop': 0x75ADA0, 'foot_speed': 0x4D3710},
        assumptions=['Interior entry retains EBP=Walk object, EBX=&head and ECX=owner. Default cleared head and paid-completion latch0 reflect earlier75BE11/75BE18; replacement-head variants supply post-PerCell mutations. Callbacks before75BE42 are not emulated.',
                     'Original Infantry vtable and actual Walk constructor. Ordinary type/House, no bunker/transport/team, default empty radio contacts with explicit nonnull-contact variants, live NavCom sentinel, existing queue2,3,4,5 and NavQueue count2. No map lookup needed by the ordinary null setter.',
                     'CurrentXYZ, retained head and live post-PerCell destination are supplied independently. Cases cover null, cell mismatch, signed height bounds including INT_MIN wrap, low16 XY alias/negative division, replacement heads, human Doing refusal, current/queued Enter and radio-contact queue preservation, lifecycle exits and signed timer values.',
                     'Native timer middle words777/888 are retained and observed, not assumed paddingzero. Foot retries supplied0xffffffff. Startup FPCW0E7F and height104 are supplied.'],
        substitutions=['Only OS InterlockedIncrement/Decrement imports emulate their integer operation and stdcall cleanup. No gameplay callback is substituted.']))
