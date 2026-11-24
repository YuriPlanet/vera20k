"""Execute original bridge OverlayClass constructor and its reached callees.

Supplied sparse cells, registered types and preallocated registry capacity are
explicit fixture inputs. Original instructions/calls are never patched.
"""
from pathlib import Path
import hashlib
import json
import struct

from capstone import Cs, CS_ARCH_X86, CS_MODE_32

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, RET_MAGIC,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.terrain_recalc import LAT_GLOBALS

D = 0x44000000
REGISTRIES = (0xA8E360, 0xB0F720, 0xB0F670, 0xB0F618, 0xA8EC50)
MAP, DUMMY = 0x87F7E8, 0xABDC50


class OriginalBridgeConstructor:
    @classmethod
    def on_map(cls, native_map, typ, requested):
        """Bind the constructor owner to an already established native map VM.

        Cells, resident TMPs, OverlayType, map bounds and RNG belong to the
        supplied map owner. Only object storage and empty registry capacity are
        supplied here; the same original constructor and Mark execute.
        """
        self = cls.__new__(cls)
        self.uc, self.ptrs = native_map.uc, native_map.ptrs
        self.kind, self.typ, self.requested = 'resident', typ, tuple(requested)
        self.overlay_id = self.read_u32(typ + 0x294)
        self.obj, self.coord = native_map.allocate(0xB0), native_map.allocate(4)
        self.cell = self.ptrs[self.requested]
        self.scenario = self.read_u32(0xA8B230)
        self.uc.mem_write(self.coord, packed(*requested))
        self.required = [0x5F3900, 0x410230, 0x68BCB0, 0x47C550]
        return self

    @staticmethod
    def restore_bridge_data(native_map, coordinate, data):
        """Original bridge-only saved-byte restoration in the identity pass."""
        u = native_map.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.reg_write(UC_X86_REG_ESP, sp)
        from unicorn.x86_const import UC_X86_REG_EBX
        u.reg_write(UC_X86_REG_EBX, data)
        u.mem_write(sp + 0x18, packed(*coordinate))
        run_checked(u, 0x5FD4F3, 0x5FD508, count=1000,
                    required_addresses=(0x5657A0, 0x5FD502))

    @staticmethod
    def construct_authored(native_map, coordinate, overlay_id):
        """Execute the original decoded OverlayPack admission/constructor slice.

        Only decoded id/coordinate caller locals are supplied. Image/CellAnim,
        mode/crate, native allocation, operator new and full constructor/Mark
        all execute, including rejected membership.
        """
        from unicorn.x86_const import UC_X86_REG_EBP
        u = native_map.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_EBP, 0)
        u.mem_write(sp + 0x14, dwords(overlay_id))
        u.mem_write(sp + 0x18, packed(*coordinate))
        receipt = dict(image=None, allocated=None, constructor=False, unlimbo=False, marks=[], hierarchy=[])
        mark_return = []
        def observe(_u, pc, _n, _d):
            if mark_return and pc == mark_return[-1][0]:
                _,pointer,row=mark_return.pop()
                row.update(returned_low_byte=u.reg_read(UC_X86_REG_EAX)&255,marked_after=u.mem_read(pointer+0x74,1)[0])
            if pc == 0x584550:
                receiver=u.reg_read(UC_X86_REG_ECX)
                roots=struct.unpack('<3I',u.mem_read(receiver+0x80,12))
                counts=[struct.unpack('<I',u.mem_read(root+8,4))[0] for root in roots]
                assert receiver==0x87F7E8 and counts==[256]*3,(coordinate,hex(receiver),counts)
                receipt['hierarchy'].append(dict(receiver=f'{receiver:08X}',root_bucket_counts=counts))
            if pc == 0x5FD44C:receipt['image'] = bool(u.reg_read(UC_X86_REG_EAX))
            if pc == 0x5FD491:receipt['allocated'] = u.reg_read(UC_X86_REG_EAX) & 255
            if pc == 0x5FC380:receipt['constructor'] = True
            if pc == 0x5F4EC0:
                receipt['unlimbo'] = True
                pointer=u.reg_read(UC_X86_REG_ECX)
                receipt['unlimbo_prior']=dict(game_active=u.mem_read(0xA8E9A0,1)[0],limbo=u.mem_read(pointer+0x81,1)[0],marked=u.mem_read(pointer+0x74,1)[0])
            if pc == 0x5FC4A5:receipt['building_present'] = bool(u.reg_read(UC_X86_REG_EAX))
            if pc == 0x5F5850:
                pointer = u.reg_read(UC_X86_REG_ECX)
                row=dict(limbo=u.mem_read(pointer+0x81,1)[0],marked=u.mem_read(pointer+0x74,1)[0])
                receipt['marks'].append(row)
                destination=struct.unpack('<I',u.mem_read(u.reg_read(UC_X86_REG_ESP),4))[0]
                mark_return.append((destination,pointer,row))
        hook = u.hook_add(UC_HOOK_CODE, observe)
        # Full physical-map wall Mark can synchronously rebuild several
        # hierarchy blocks. Retain the instruction bound and verify real
        # receiver/root invariants before allowing this larger map budget.
        run_checked(u, 0x5FD41A, 0x5FD508, count=10000000,timeout_us=120000000)
        u.hook_del(hook)
        return receipt

    @staticmethod
    def apply_authored_data(native_map, coordinate, data):
        """Original unconditional second OverlayDataPack pass after identities."""
        u = native_map.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp + 0x18, packed(*coordinate))
        u.mem_write(sp + 0x13, bytes([data]))
        run_checked(u, 0x5FD61A, 0x5FD646, count=1000,
                    required_addresses=(0x568300, 0x5657A0, 0x5FD640))

    @staticmethod
    def empty_registries(u, allocate, capacity=1024, *, preserve=()):
        """Original startup vector vtables with supplied spare storage.

        Composed map owners may retain already-populated registries. No live
        vector is erased when its address is explicitly preserved.
        """
        for registry, vtable in zip(REGISTRIES, (0x7E4F64, 0x7E91EC, 0x7E91EC, 0x7E91EC, 0x7E9D24)):
            if registry not in preserve:
                u.mem_write(registry, dwords(vtable, allocate(capacity * 4), capacity, 0, 0, 0))
        if 0xB0F698 not in preserve:
            u.mem_write(0xB0F698, dwords(0x7E91EC, allocate(capacity * 4), capacity, 0, 0, 0))

    def __init__(self, kind, overlay_id, requested=(16, 16)):
        self.kind, self.overlay_id, self.requested = kind, overlay_id, requested
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(D, 0x200000)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.obj, typ = D + 0x1000, D + 0x2000
        terrain, terrain_vtable = D + 0x3000, D + 0x4000
        self.scenario, self.coord = D + 0x5000, D + 0x6000
        table, cells = D + 0x100000, D + 0x10000
        self.ptrs = {(x, y): cells + i * 0x200 for i, (x, y) in enumerate(
            (x, y) for y in range(12, 21) for x in range(12, 21))}
        for (x, y), cell in self.ptrs.items():
            u.mem_write(table + (y * 512 + x) * 4, dwords(cell))
            u.mem_write(cell + 0x24, packed(x, y))
            u.mem_write(cell + 0x44, dwords(-1))
            u.mem_write(cell + 0x11B, b'\x06')
        self.cell, self.typ = self.ptrs[16, 16], typ
        u.mem_write(MAP + 0x13C, dwords(table, 0x40000))
        u.mem_write(MAP + 0xF4, dwords(16, 16, 0, 0, 16, 16))
        u.mem_write(DUMMY, bytes(0x200))
        u.mem_write(DUMMY + 0x38, dwords(0xffff))
        u.mem_write(DUMMY + 0x44, dwords(-1))
        # Actual startup vtable stores:4E7AFD,7252ED,72536D,7253ED,4E68FD;
        # queue startup72586D. Preserve this fixture's established addresses.
        registry_storage = iter([D + 0x30000 + n * 0x100 for n in range(5)] + [D + 0x31000])
        self.empty_registries(u, lambda _size: next(registry_storage), capacity=16)
        u.mem_write(0xA8B230, dwords(self.scenario))
        u.mem_write(self.scenario + 0x214, dwords(1000))
        u.mem_write(0xA8E9A0, b'\x01')
        u.mem_write(0xA8E7AC, dwords(0))
        self.call(0x5FC310)  # Original startup EmptyCell initialization.
        assert bytes(u.mem_read(0xAC1528, 4)) == bytes(4)
        u.mem_write(self.coord, packed(*requested))
        u.mem_write(typ, dwords(0x7EF600))
        u.mem_write(typ + 0x294, dwords(overlay_id))
        self.required = [0x5F3900, 0x410230, 0x68BCB0, 0x47C550]
        if kind == 'terrain':
            u.mem_write(self.cell + 0xE4, dwords(terrain))
            u.mem_write(terrain, dwords(terrain_vtable))
            u.mem_write(terrain_vtable + 0x2C, dwords(0x71D300))
            self.required += [0x71D300]
        elif kind == 'slope':
            u.mem_write(self.cell + 0x11C, b'\x05')
            self.required += [0x5F4EC0, 0x5FC570, 0x5FC5E3, 0x5F521C]
        else:
            assert kind in ('success', 'overrides')
            self.prepare_success(kind)
        if requested == (0, 0):
            self.required = [0x5F3900, 0x410230, 0x68BCB0]

    def call(self, entry, receiver=0, args=(), required=()):
        u = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, receiver)
        run_checked(u, entry, RET_MAGIC, count=100000, required_addresses=required)
        return u.reg_read(UC_X86_REG_EAX)

    def prepare_success(self, kind):
        u = self.uc
        # Original direction initialization; no supplied direction table.
        self.call(0x49F2F0)
        rules, overlays, old = D + 0x40000, D + 0x44000, D + 0x45000
        head, tmp, tiles = D + 0x46000, D + 0x47000, D + 0x48000
        zones, levels = D + 0x50000, D + 0x60000
        # Startup40B563 initializes this empty listener owner's vector vtable.
        u.mem_write(0x87F5D8, dwords(0x7E17CC))
        u.mem_write(0x8871E0, dwords(rules))
        u.mem_write(0xA83D84, dwords(overlays))
        u.mem_write(overlays + self.overlay_id * 4, dwords(self.typ))
        u.mem_write(overlays + 7 * 4, dwords(old))
        u.mem_write(old + 0x294, dwords(7))
        u.mem_write(old + 0x2B2, b'\x01')
        if kind == 'overrides':
            u.mem_write(self.cell + 0x44, dwords(7))
        u.mem_write(MAP + 0x68, dwords(zones, 33 * 33, levels))
        u.mem_write(head, dwords(0x7ECC48))
        u.mem_write(head + 0xA4, dwords(tmp))
        u.mem_write(head + 0x2C8, dwords(-1))
        u.mem_write(head + 0x2D4, dwords(-1))
        u.mem_write(tmp, dwords(1, 1, 8, 4, tmp + 20))
        u.mem_write(tmp + 60, bytes((3, 11, 0)))
        u.mem_write(0xA8ED2C, dwords(tiles))
        u.mem_write(0xA8ED38, dwords(1))
        u.mem_write(tiles, dwords(head))
        for address in LAT_GLOBALS:
            u.mem_write(address, dwords(-1))
        for address in (0x89EA48, 0x89EA48 + 36):
            u.mem_write(address, struct.pack('<f', 1.0))
        self.required += [0x5F4EC0, 0x5FC570, 0x47D2B0, 0x5FD216]

    def read_u32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    @staticmethod
    def prepare_deferred_services(u):
        """Supply the existing Windows SEH/IsBadReadPtr boundary for drains.

        Original RTTI, deferred-queue operations and destructors still execute.
        Reused by the natural-ore owner after original Overlay construction.
        """
        u.mem_map(0, 0x1000)
        u.mem_write(0, dwords(-1))
        pointer_probe = RET_MAGIC + 0x100
        u.mem_write(0x7E115C, dwords(pointer_probe))

        def pointer_service(_u, address, _size, _data):
            if address != pointer_probe:
                return
            sp = u.reg_read(UC_X86_REG_ESP)
            ret, pointer, length = struct.unpack('<3I', u.mem_read(sp, 12))
            u.mem_read(pointer, length)  # Validate every supplied success.
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp + 12)
            u.reg_write(UC_X86_REG_EIP, ret)

        u.hook_add(UC_HOOK_CODE, pointer_service)

    def construct(self):
        """Execute construction independently of the optional queue drain."""
        returned = self.call(0x5FC380, self.obj, (self.typ, self.coord, -1), self.required)
        assert returned == self.obj
        return returned

    def run(self):
        self.construct()
        u = self.uc
        b = u.mem_read(self.obj, 0xB0)
        result = dict(kind=self.kind, overlay_id=self.overlay_id, requested=self.requested,
                    native_id=struct.unpack_from('<I', b, 0x10)[0],
                    cursor=self.read_u32(self.scenario + 0x214),
                    alive=b[0x90], limbo=b[0x81], on_map=b[0x74], redraw=b[0x80],
                    registry_counts=[self.read_u32(r + 16) for r in REGISTRIES],
                    queue_count=self.read_u32(0xB0F6A8),
                    object_world=struct.unpack_from('<3i', b, 0x9C),
                    overlay=struct.unpack('<i', u.mem_read(self.cell + 0x44, 4))[0])
        def cells():
            rows = []
            for coord, pointer in self.ptrs.items():
                flags = self.read_u32(pointer + 0x140)
                anchor = self.read_u32(pointer + 0x2C)
                overlay = struct.unpack('<i', u.mem_read(pointer + 0x44, 4))[0]
                state = bytes(u.mem_read(pointer + 0x11E, 1))[0]
                if flags or anchor or overlay != -1 or state:
                    rows.append(dict(coord=coord, flags=flags, overlay=overlay, state=state,
                                     anchor=next((c for c, p in self.ptrs.items() if p == anchor), None)))
            return rows
        result['cells'] = before_cells = cells()
        def dummy():
            return dict(coord=struct.unpack('<2h', u.mem_read(DUMMY + 0x24, 4)),
                        flags=self.read_u32(DUMMY + 0x140),
                        overlay=struct.unpack('<i', u.mem_read(DUMMY + 0x44, 4))[0],
                        state=bytes(u.mem_read(DUMMY + 0x11E, 1))[0],
                        anchor_is_self=self.read_u32(DUMMY + 0x2C) == DUMMY)
        result['dummy'] = before_dummy = dummy()
        frees = []
        # Original typeid uses Windows SEH and IsBadReadPtr during finalization.
        # Supply those OS facilities after the complete constructor has returned.
        self.prepare_deferred_services(u)
        def external_services(_u, address, _size, _data):
            if address != 0x7C8B3D:
                return
            sp = u.reg_read(UC_X86_REG_ESP)
            ret, pointer = struct.unpack('<2I', u.mem_read(sp, 8))
            assert pointer == self.obj
            frees.append('overlay')
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, ret)
        u.hook_add(UC_HOOK_CODE, external_services)
        self.call(0x725C70, required=(0x5FDF70, 0x5F3B80) if result['queue_count'] else ())
        result['after_drain'] = dict(registry_counts=[self.read_u32(r + 16) for r in REGISTRIES],
                                    queue_count=self.read_u32(0xB0F6A8), frees=frees,
                                    cursor=self.read_u32(self.scenario + 0x214), cells=cells(), dummy=dummy())
        assert result['after_drain']['cells'] == before_cells
        assert result['after_drain']['dummy'] == before_dummy
        return result


def setter_equivalence():
    probe = OriginalBridgeConstructor('success', 24)
    decoder = Cs(CS_ARCH_X86, CS_MODE_32)
    functions = []
    for base in (0x47E040, 0x47E470):
        rows = []
        for insn in decoder.disasm(bytes(probe.uc.mem_read(base, 0x430)), base):
            operand = insn.op_str
            if insn.mnemonic.startswith('j') and operand.startswith('0x'):
                target = int(operand, 16)
                assert base <= target < base + 0x420, (insn.address, target)
                operand = str(target - base)
            rows.append((insn.address - base, insn.mnemonic, operand))
            if insn.mnemonic == 'ret':
                break
        assert len(rows) == 301 and rows[-1] == (0x41f, 'ret', '8')
        functions.append(rows)
    assert functions[0] == functions[1]
    return dict(instructions=301, functions=['0x0047E040', '0x0047E470'],
                normalized_sha256=hashlib.sha256(json.dumps(functions[0]).encode()).hexdigest(),
                normalization='Only internal jump targets become relative; external call operands remain absolute')


def cases():
    return {'setter_equivalence': setter_equivalence(), 'cases': [OriginalBridgeConstructor(kind, overlay).run()
                     for kind in ('terrain', 'slope', 'success', 'overrides')
                     for overlay in (24, 25, 237, 238)] +
            [OriginalBridgeConstructor('success', overlay, requested).run()
             for requested in ((0, 0), (-1, 16), (-496, 17), (-32768, 16), (32767, 16))
             for overlay in (24, 25, 237, 238)]}


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original5FC380 bridge constructor over supplied sparse live map and type inputs',
        assumptions=[
            'Raw IDs24/25/237/238, frame-1; Terrain reject, slope5 reject, success and existing Overrides paths only',
            'One constructor per fresh registry/queue fixture; direct drain, not MainTick admission or mixed-queue ordering',
            'Twenty extra success-type requests cover initialized EmptyCell, negative centers, a fixed-stride real alias and signed16 extrema',
            'Supplied allocated object memory, empty registry/queue capacity16, Scenario cursor1000 and active game',
            'Sparse real9x9 cells centered16,16; source terrain type construction excluded',
            'Terrain list record uses original TerrainClass RTTI71D300',
            'Selected OverlayType uses original vtable with supplied raw ID and otherwise zero fields',
            'Success Recalc receives resident synthetic1x1 Road TMP; no native loader, artwork or whole repair claim',
            'Original startup vector vtables supplied with empty preallocated storage; other pointer-expiration recipients absent',
            'Empty Windows SEH chain supplied for original typeid during the post-constructor drain',
        ], substitutions=['7C8B3D external free and Windows IsBadReadPtr transport only during the post-constructor drain; no constructor call is substituted'],
        entry_points={'empty_initializer': 0x5FC310, 'constructor': 0x5FC380, 'mark': 0x5FC570, 'drain': 0x725C70,
                      'overlay_destructor': 0x5FDF70, 'base_destructor': 0x5F3B80}),
                   source_paths={'producer': Path(__file__)})
