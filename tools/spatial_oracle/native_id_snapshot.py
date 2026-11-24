"""Original Scenario save/load, ID continuation and saved view bookmarks.

Only IStream transport, diagnostic logging and clock are supplied callbacks.
The raw save/read and Scenario post-read initialization execute original code.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_EIP
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance
from tools import native_oracle
from tools.spatial_oracle.map_queries import dwords

SCENARIO, STREAM, VTABLE = 0x20000000, 0x20008000, 0x20009000
RETURN, READ, WRITE = 0x30000000, 0x30000100, 0x30000200


class ScenarioFixture:
    """Shared isolated Scenario receiver and exact IStream transport.

    Consumers may supply extra Scenario/process state before round_trip, and
    observe the same original Save/Load/post-read bodies. The fixture does not
    import RNG producers: menu controls compose this owner, not the reverse.
    """

    def __init__(self, value=1010038):
        self.u = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(0x10000000, 0x100000)
        u.mem_map(SCENARIO, 0x10000)
        u.mem_map(RETURN, 0x1000)
        u.mem_write(STREAM, dwords(VTABLE))
        u.mem_write(VTABLE + 12, dwords(READ, WRITE))
        u.mem_write(SCENARIO + 0x214, dwords(value))
        u.mem_write(SCENARIO + 0x614, dwords(-1))
        self.value, self.data, self.position = value, bytearray(), 0
        u.hook_add(UC_HOOK_CODE, self.hook)

    def ret(self, cleanup=0):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        address = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        u.reg_write(UC_X86_REG_EIP, address)

    def hook(self, u, address, _size, _data):
        if address in (0x4068E0, 0x6C8C40):
            self.ret()
            return
        if address not in (READ, WRITE):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        owner, pointer, length, out = struct.unpack('<4I', u.mem_read(sp + 4, 16))
        assert owner == STREAM
        if address == WRITE:
            self.data.extend(u.mem_read(pointer, length))
        else:
            assert self.position + length <= len(self.data)
            u.mem_write(pointer, bytes(self.data[self.position:self.position + length]))
            self.position += length
        if out:
            u.mem_write(out, dwords(length))
        self.ret(16)

    def call(self, address, args=(), required=()):
        u = self.u
        sp = 0x100FF000
        u.mem_write(sp, dwords(RETURN, *args))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, SCENARIO)
        run_checked(u, address, RETURN, count=1000000, required_addresses=required)

    def round_trip(self, *, overwrite_before_load=None):
        assert not self.data and self.position == 0
        self.call(0x689310, (STREAM,))
        saved = struct.unpack_from('<I', self.data, 0x214)[0]
        self.u.mem_write(SCENARIO + 0x214, dwords(0xABADCAFE))
        for address, raw in (overwrite_before_load or {}).items():
            self.u.mem_write(address, raw)
        self.call(0x689470, (STREAM,), (0x683560,))
        loaded = struct.unpack('<I', self.u.mem_read(SCENARIO + 0x214, 4))[0]
        self.call(0x68BCB0)
        return dict(input=self.value, saved=saved, loaded=loaded,
                    next=self.u.reg_read(UC_X86_REG_EAX),
                    stream_bytes=len(self.data), read_bytes=self.position)


def native_case(value):
    return ScenarioFixture(value).round_trip()


def bookmark_case(cells):
    fixture = ScenarioFixture()
    raw = struct.pack('<8h', *(coordinate for cell in cells for coordinate in cell))
    fixture.u.mem_write(SCENARIO + 0x348E, raw)
    # Isolated fixture mutations may touch supplied state, never executable bytes.
    code = [(native_oracle.IMAGE_BASE + rva,
             bytes(fixture.u.mem_read(native_oracle.IMAGE_BASE + rva, size)))
            for rva, _, size, _, flags in native_oracle._sections(native_oracle.image_bytes())
            if flags & 0x20000000 and size]
    owner = fixture.round_trip(overwrite_before_load={SCENARIO + 0x348E: b'\x55' * 16})
    loaded = bytes(fixture.u.mem_read(SCENARIO + 0x348E, 16))
    assert loaded == raw
    assert all(bytes(fixture.u.mem_read(at, len(data))) == data for at, data in code)
    values = struct.unpack('<8h', loaded)
    return dict(input=cells, loaded=[list(values[i:i + 2]) for i in range(0, 8, 2)],
                reused_owner_result=owner, code_unchanged=True)


def cases():
    return {'cases': [native_case(value) for value in
                     (0, 1, 1010038, 0x7FFFFFFE, 0x7FFFFFFF, 0xFFFFFFFE, 0xFFFFFFFF)],
            'scenario_bookmarks': [bookmark_case(cells) for cells in (
                [[11, 22], [33, 44], [55, 66], [77, 88]],
                [[0, 0], [511, 511], [1, 510], [510, 1]],
                [[-1, -1], [-32768, 32767], [32767, -32768], [0, -1]],
            )]}


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: dict(provenance(
        scope='Scenario+214 ID cursor and four packed cell bookmarks+348E through original Scenario save/load/post-read, followed by the native ID increment',
        assumptions=['Valid stopped elapsed timer, three empty dynamic arrays, false11F8 string flag',
                     'Supplied in-memory IStream; no native file chooser, disk transport or complete game save claim',
                     'Three bookmark rows overwrite outgoing bytes before Load; signed extremes are raw persistence witnesses, not valid-map recall or command admission claims',
                     'ScenarioFixture is reused by menu RNG controls; this owner does not seed or emulate their RNG state'],
        substitutions=['IStream Read/Write byte transport', '4068E0 diagnostic logging returns',
                       '6C8C40 clock returns0'],
        entry_points={'save': 0x689310, 'load': 0x689470, 'post_read': 0x683560, 'next_id': 0x68BCB0}), command='python -m tools.spatial_oracle.native_id_snapshot --check'),
        source_paths={'producer': Path(__file__), 'shared_runner': Path(native_oracle.__file__)})
