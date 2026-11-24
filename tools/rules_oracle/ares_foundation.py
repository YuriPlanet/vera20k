"""Ares custom foundation reader, getter, list and hidden-occupation controls.

Synthetic lexical INI caches reuse bridge_anim_inputs.Reader.make_ini. EmpProbe
owns the single VM, original CRT, PE mapping and RNG/code guards. No assets load.
"""
import argparse
import os
from pathlib import Path
import struct

from unicorn.x86_const import *
from tools import native_oracle as native
from tools.rules_oracle.ares_emp import EmpProbe
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, identified_bytes
from tools.rules_oracle.bridge_anim_inputs import Reader

HERE = Path(__file__).with_suffix('.json')
ART = 0x887180
SP = 0x2000c000


class FoundationProbe(EmpProbe):
    cstring = EmpProbe.text

    def __init__(self, images):
        self.numeric_formats = []
        self.read_calls = []
        super().__init__(images)
        self.typ = self.alloc(0x1900)
        self.ext = self.alloc(0x400)
        self.call(0x1000cae0, self.typ, ecx=self.ext)
        self.write(self.typ + 0xe24, self.ext)
        self.u.mem_write(self.typ + 0x24, b'CUSTOM_TEST\0')
        self.u.mem_write(self.typ + 0x1f8, b'CUSTOM_ART\0')
        vt = self.alloc(0x200)
        self.write(vt + 0x108, 0x45ec20)
        self.write(self.typ, vt)

    def hook(self, u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x528a10, 0x5276d0):
            self.read_calls.append(dict(reader=f'{pc:08x}',
                section=self.string(self.read(sp + 4)), key=self.string(self.read(sp + 8))))
            # Execute the real cached CCINI readers, not Probe's string seam.
            return
        if pc == 0x7c978a:
            # Process-exit callback registration; GetFoundation's original
            # lazy static/sentinel writes still execute. No CRT shutdown runs.
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, self.read(sp))
            return
        if pc == 0x10003a40:
            target, capacity, count, pattern, value = struct.unpack('<5I', u.mem_read(sp + 4, 20))
            fmt = self.string(pattern)
            assert fmt in ('Foundation.%d', 'FoundationOutline.%d')
            payload = (fmt % struct.unpack('<i', struct.pack('<I', value))[0]).encode('ascii')
            assert len(payload) < capacity and count == 0xffffffff
            u.mem_write(target, payload + b'\0')
            self.numeric_formats.append(dict(format=fmt, index=value, key=payload.decode()))
            u.reg_write(UC_X86_REG_EAX, len(payload))
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, self.read(sp))
            return
        super().hook(u, pc, size, data)

    def checksum(self, text):
        buf = self.alloc(16)
        raw = self.text(text)
        return self.call(0x4a1de0, raw, len(text), ecx=buf)

    def ini(self, keys):
        Reader.make_ini(self, {'CUSTOM_ART': keys}, ini_address=ART, checksum=self.checksum)

    def reader(self):
        self.u.mem_write(SP, bytes(0x100))
        for offset, value in ((0x10, ART), (0x18, self.typ + 0x24),
                              (0x24, self.typ + 0x1f8), (0x2c, self.ext), (0x30, self.typ)):
            self.write(SP + offset, value)
        for reg, val in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EDI, self.ext),
                         (UC_X86_REG_EBX, self.typ + 0x1f8), (UC_X86_REG_EBP, self.typ + 0x24)):
            self.u.reg_write(reg, val)
        native.run_checked(self.u, 0x1000de0f, 0x1000e11a, count=500000,
            required_addresses=(0x1000e0fa, 0x1000e114))
        self.check_code()

    def pairs(self, address, count=None):
        out = []
        for i in range(count if count is not None else 1024):
            pair = list(struct.unpack('<hh', self.u.mem_read(address + i * 4, 4)))
            out.append(pair)
            if count is None and pair == [0x7fff, 0x7fff]:
                return out
        if count is None:
            raise AssertionError('Missing sentinel')
        return out

    def state(self):
        return dict(custom=bool(self.u.mem_read(self.ext + 0x10, 1)[0]),
            selector=self.read(self.typ + 0xef0),
            width=self.read(self.ext + 0x14), height=self.read(self.ext + 0x18),
            cells=self.pairs(self.read(self.ext + 0x1c)),
            outline=self.pairs(self.read(self.ext + 0x28)),
            type_cells_is_extension=self.read(self.typ + 0xdfc) == self.read(self.ext + 0x1c),
            type_outline_is_extension=self.read(self.typ + 0xed4) == self.read(self.ext + 0x28))

    def hook_query(self, entry, **fields):
        frame = self.alloc(0x30)
        for offset, value in fields.items():
            self.write(frame + int(offset, 16), value)
        continuation = self.call(entry, frame)
        return dict(continuation=f'{continuation:08x}', result=self.read(frame + 0x24))

    def getters(self):
        result = dict(width=self.hook_query(0x10015310, **{'20': self.typ}),
            max_pips=self.hook_query(0x10010150, **{'20': self.typ}))
        args = self.alloc(8)
        for bib in (False, True):
            self.u.mem_write(self.typ + 0x1570, bytes([bib]))
            for include in (False, True):
                self.u.mem_write(args + 4, bytes([include]))
                result[f'height_bib{int(bib)}_include{int(include)}'] = self.hook_query(
                    0x100152d0, **{'20': self.typ, '14': args})
        out = self.hook_query(0x10010120, **{'20': self.typ})
        out['points'] = self.pairs(out.pop('result'))
        result['outline'] = out
        bound = self.alloc(16)
        self.call(0x10011980, bound, self.read(self.typ + 0xdfc))
        result['occupied_bounds'] = list(struct.unpack('<4i', self.u.mem_read(bound, 16)))
        return result

    def mark_controls(self):
        table = self.alloc(0x100000)
        self.write(0x87f924, table)
        coords = {}
        for y in range(12, 28):
            for x in range(12, 28):
                cell = self.alloc(0x110)
                self.write(table + (y * 512 + x) * 4, cell)
                coords[x, y] = cell
        oldsp = self.alloc(0x900)
        origin = self.alloc(4)
        self.write(origin, 20 | (20 << 16))
        self.write(oldsp + 0x8b4, origin)
        fields = {'8': self.typ, '10': 2, '14': oldsp, '1c': self.typ}
        # P1 returns actual foundation pointer in the saved EBP slot+10.
        frame = self.alloc(0x30)
        self.write(frame + 8, self.typ)
        continuation = self.call(0x10015490, frame)
        point_match = self.read(frame + 0x10) == self.read(self.typ + 0xdfc)
        self.hook_query(0x10015420, **fields)
        after_add = [[x, y, self.read(cell + 0x100)] for (x, y), cell in coords.items()
                     if self.read(cell + 0x100)]
        self.hook_query(0x100154b0, **fields)
        after_remove = [[x, y, self.read(cell + 0x100)] for (x, y), cell in coords.items()
                        if self.read(cell + 0x100)]
        return dict(get_foundation_points_match=point_match, after_add=after_add,
                    after_remove=after_remove, add_p1_continuation=f'{continuation:08x}')


def control(images, name, keys):
    m = FoundationProbe(images)
    constructor = dict(custom=bool(m.u.mem_read(m.ext + 0x10, 1)[0]),
        width=m.read(m.ext + 0x14), height=m.read(m.ext + 0x18))
    m.ini(keys)
    m.reader()
    initial = m.state()
    getters = m.getters()
    mark = m.mark_controls()
    m.ini({'Foundation': 'Custom', 'Foundation.X': '1', 'Foundation.Y': '1', 'Foundation.0': '0,0'})
    before = len(m.read_calls)
    m.reader()
    retained = m.state()
    m.check_code()
    return dict(name=name, authored=keys, constructor=constructor, installed=initial,
        getters=getters, mark=mark, retained_after_changed_ini=retained,
        reload_reader_calls=len(m.read_calls)-before,
        numeric_format_service_calls=m.numeric_formats,
        code_unchanged=True,
        rng_states_before=[raw.hex() for raw in m.rng_before],
        rng_states_after=[bytes(m.u.mem_read(at, 0x3f4)).hex() for at in m.rng])


def synthetic_cases():
    rectangle = {'Foundation': 'Custom', 'Foundation.X': '2', 'Foundation.Y': '4',
                 'FoundationOutline.Length': '16'}
    rectangle.update({f'Foundation.{i}': f'{x},{y}' for i, (x, y) in enumerate(
        [(0,0),(1,0),(0,1),(1,1),(0,2),(1,2),(0,3),(1,3)])})
    outline = [(-1,-1),(0,-1),(1,-1),(2,-1),(2,0),(2,1),(2,2),(2,3),
               (2,4),(1,4),(0,4),(-1,4),(-1,3),(-1,2),(-1,1),(-1,0)]
    rectangle.update({f'FoundationOutline.{i}': f'{x},{y}' for i, (x,y) in enumerate(outline)})
    sparse = {'Foundation': 'Custom', 'Foundation.X': '3', 'Foundation.Y': '2',
              'FoundationOutline.Length': '2', 'Foundation.0': '1,1',
              'Foundation.1': '0,0', 'Foundation.2': '-1,0',
              'Foundation.3': '1,1', 'Foundation.4': '0,1',
              'FoundationOutline.0': '2,1', 'FoundationOutline.1': '-2,-1'}
    return rectangle, sparse


def controls(images):
    rectangle, sparse = synthetic_cases()
    return [control(images, 'rectangle_2x4', rectangle), control(images, 'sparse_holes', sparse)]


def outline_backing_control(images):
    keys = dict(synthetic_cases()[1])
    keys.update({'FoundationOutline.Length': '16', 'FoundationOutline.10': '4,-1'})
    m = FoundationProbe(images)
    m.ini(keys)
    m.reader()
    installed = m.state()
    begin, end = m.read(m.ext + 0x28), m.read(m.ext + 0x2c)
    backing = m.pairs(begin, (end-begin)//4)
    bounds = m.alloc(4)
    m.call(0x4a94f0, bounds, m.read(m.typ + 0xdfc))
    tactical = m.alloc(0x1200)
    m.u.mem_write(tactical + 0x1174, struct.pack('<hh', 20, 20))
    m.u.mem_write(SP, bytes(0x100))
    for reg, value in ((UC_X86_REG_EAX,bounds),(UC_X86_REG_EBX,tactical),(UC_X86_REG_ESP,SP)):
        m.u.reg_write(reg,value)
    native.run_checked(m.u,0x4a8c9e,0x4a8d11,count=1000,
        required_addresses=(0x4a8ca6,0x4a8cda,0x4a8d0e))
    mark_argument = m.read(m.u.reg_read(UC_X86_REG_ESP))
    m.check_code()
    return dict(name='outline_gap_direct_slot_and_cursor',authored=keys,installed=installed,
        outline_backing=backing,index10=m.pairs(begin+40,1)[0],
        cursor_input=[20,20],cursor_bounds=m.pairs(bounds,1)[0],
        cursor_offset=m.pairs(tactical+0x1178,1)[0],
        marker_origin=m.pairs(mark_argument,1)[0],
        marker_bool=m.read(m.u.reg_read(UC_X86_REG_ESP)+4),
        code_unchanged=True,rng_states_before=[raw.hex() for raw in m.rng_before],
        rng_states_after=[bytes(m.u.mem_read(at,0x3f4)).hex() for at in m.rng])


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--ares', type=Path, default=os.environ.get('VERA20K_ARES_DLL'))
    p.add_argument('--gamemd-exe', type=Path)
    args, remaining = p.parse_known_args()
    if not args.ares:
        p.error('Set --ares or VERA20K_ARES_DLL')
    if args.gamemd_exe:
        os.environ['VERA20K_GAMEMD_EXE'] = str(args.gamemd_exe)
    images = (identified_bytes(Path(args.ares), ARES_SHA256), native.image_bytes())
    native.finish_vectors(lambda:dict(schema=1, controls=controls(images),
        outline_backing=outline_backing_control(images)), HERE,
        provenance=metadata, argv=remaining)


def metadata():
    return native.provenance(
        scope='Two synthetic Ares Custom foundations and one required outline/cursor continuation: constructor, exact cached INI readers, point sort/unique, outline, getter/list and hidden-map add/remove bodies; not whole-game rendering parity.',
        assumptions=[
            'Ares binary SHA256 '+ARES_SHA256+'; retail executable identity retained in the local execution receipt, not published. Both supported retail variants have equal inspected Foundation/ReadInt/Map instructions.',
            'EmpProbe/Probe is the only VM/PE/CRT owner. Existing Reader.make_ini supplies lexical CCINI lookup caches, with CRC4A1DE0 executed in the same VM. Original ReadString528A10 and ReadInt5276D0 execute; no scanner or parser result is supplied.',
            'Actual extension constructor1000CAE0 and reader interior1000DE0F..1000E11A execute on synthetic BuildingType fields. Whole BuildingType constructor, phase wrapper and physical INI loader are not executed. Image is CUSTOM_ART; native cell/outline pointer installation executes.',
            'CRT snprintf key formatting is a transport for the actual caller index, not numeric INI parsing. CRT atexit registration is a returning-zero process shutdown service; native lazy GetFoundation sentinel initialization executes.',
            'Getter hook frames are supplied and hook return continuations observed; original executable jumps are not patched. Virtual+108 is explicitly directed to original Type GetFoundation45EC20. Whole BuildingClass Mark/Unmark and pathfinding are excluded from native execution; Rust integration tests cover their shared owners.',
            'Map origin20,20, hidden diagonal depth2 and allocated cell shells12..27 are explicit world inputs. Original helper10011B00 and Add/Remove counter writes execute, including deduplication. The fixture is interior; signed/map-edge and huge allocation behavior are not certified.',
            'Two Map histories record terminated outline views. One additional reader-only history captures complete backing storage, a meaningful index10 behind an earlier missing-slot sentinel, and original cursor4A94F0/4A8C9E..4A8D11 through argument publication. Stops before Mark4A95A0; no previous Map history replays for that added evidence.',
            'Three complete native RNG states, seeded31 in EmpProbe setup, are retained before/after. All streams and code bytes stay unchanged. No timer or detach call is expected on this selected geometry path.',
            'Two development prefixes faulted before completed histories: missing CRT atexit service and a packed Cell value supplied where the hook requires Cell*. Both boundaries were corrected; each final history completed once. No older corpus was replayed.',
        ],
        substitutions=['Lexical INI cache layout, CRT locale/format/shutdown, allocation/free services, hook frames, Type virtual foundation dispatch, allocated Cell shells; no geometry/sort/counter/INI conversion outcome is injected.'],
        entry_points=dict(constructor=0x1000cae0, reader=0x1000de0f,
            reader_end_exclusive=0x1000e11a, width=0x10015310,
            height=0x100152d0, max_pips=0x10010150, outline=0x10010120,
            bounds=0x10011980, map_add_list=0x10015490,
            map_add_hidden=0x10015420, map_remove_hidden=0x100154b0,
            hidden_diagonal_list=0x10011b00))


if __name__ == '__main__':
    main()
