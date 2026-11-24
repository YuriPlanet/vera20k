"""Guarded, bounded memory supplied to native execution fixtures.

Only allocation/free observations use observe(); scalar-input fixtures merely
preallocate data and check guard bytes after original instructions execute.
"""
import struct
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, STACK_BASE, STACK_SIZE

HEAP, HEAP_SIZE = 0x20000000, 0x04000000
NEW, DELETE = 0x007C8E17, 0x007C8B3D

def u32(uc, address):
    return struct.unpack('<I', uc.mem_read(address, 4))[0]

class Arena:
    """Monotonic supplied malloc/free, guarded individually; no native patch."""
    def __init__(self, uc):
        self.uc = uc
        uc.mem_map(HEAP, HEAP_SIZE)
        self.cursor = HEAP
        self.allocations = []
        self.by_pointer = {}
        self.frees = []
        self.phase = 'setup'

    def allocate(self, size, caller):
        size = int(size)
        assert 0 <= size < HEAP_SIZE
        payload = max(1, size)
        aligned = (payload + 15) & ~15
        address = self.cursor + 16
        end = address + aligned + 16
        assert end <= HEAP + HEAP_SIZE, 'declared successful-allocation arena exhausted'
        self.uc.mem_write(self.cursor, b'\xa5' * 16)
        self.uc.mem_write(address, bytes(payload))
        self.uc.mem_write(address + payload, b'\xa5' * (end - address - payload))
        record = dict(address=address, requested_size=size, payload_size=payload,
                      allocation_end=end, caller=caller, phase=self.phase)
        self.allocations.append(record)
        self.by_pointer[address] = record
        self.cursor = end
        return address

    def guard_check(self):
        for a in self.allocations:
            p, size, end = a['address'], a['payload_size'], a['allocation_end']
            assert bytes(self.uc.mem_read(p - 16, 16)) == b'\xa5' * 16, a
            assert bytes(self.uc.mem_read(p + size, end - p - size)) == b'\xa5' * (end - p - size), a
        for a, b in zip(self.allocations, self.allocations[1:]):
            assert a['allocation_end'] == b['address'] - 16
        assert self.cursor < RET_MAGIC and HEAP > STACK_BASE + STACK_SIZE

    def observe(self, uc, address, _size, _data):
        if address not in (NEW, DELETE):
            return
        esp = uc.reg_read(UC_X86_REG_ESP)
        caller, argument = u32(uc, esp), u32(uc, esp + 4)
        if address == NEW:
            uc.reg_write(UC_X86_REG_EAX, self.allocate(argument, caller))
        else:
            assert argument == 0 or argument in self.by_pointer, ('unknown free', hex(argument), hex(caller))
            self.frees.append(dict(address=argument, caller=caller, phase=self.phase))
        uc.reg_write(UC_X86_REG_EIP, caller)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
