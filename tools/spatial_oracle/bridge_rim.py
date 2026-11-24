"""Original rim loops over supplied stock-loaded scalar cells.

The concrete family runs 576770/576200/47E040. The wooden family runs its
twins 571050/570AE0/47E470 over the same cells, with the stock BridgeSet base
supplied through g_WoodBridgeSet_TileSetBase and a MapClass+0x124..+0x130
search rectangle in place of the Size diamond.

Explicit output sinks replace object fallout, radar and screen work. Initial
damage inputs use complete native setters plus supplied overlay-clear writes;
this is not a whole-game damage execution or native loader comparison.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE, CELLS, COORD, DUMMY = 0x87F7E8, 0xC00000, 0x40000000, 0xB00000, 0xABDC50
GLOBALS = dict(BridgeTopLeft1=0xABC2B4, BridgeTopLeft2=0xAA1130,
               BridgeBottomRight1=0xABC1E8, BridgeBottomRight2=0xAA0E38,
               BridgeTopRight1=0xAA1548, BridgeTopRight2=0xAA0740,
               BridgeBottomLeft1=0xABC1D0, BridgeBottomLeft2=0xAA1540,
               BridgeMiddle1=0xABAD30, BridgeMiddle2=0xAA1028)


FAMILIES = dict(
    high=dict(selector=0x576770, edge=0x576200, setter=0x47E040, base=0xAA0E28),
    low=dict(selector=0x571050, edge=0x570AE0, setter=0x47E470, base=0xABAD1C),
)


class OriginalRim:
    def __init__(self, case, family='high', rect=None):
        self.family = FAMILIES[family]
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        u.mem_map(CELLS, 0x2000000)
        self.ptrs = {(r[0], r[1]): CELLS + i * 0x200 for i, r in enumerate(case['cells'])}
        self.coords = {v: k for k, v in self.ptrs.items()}
        table = bytearray(0x100000)
        for x, y, tile, sub, flags, overlay, state, anchor, level, land in case['cells']:
            p = self.ptrs[x, y]
            u.mem_write(p + 0x24, packed(x, y))
            # Anchor slot's +2C is not the derived self relation. It is unused
            # by the selector while the self bit is set; initialize to zero.
            ap = self.ptrs[tuple(anchor)] if anchor and not flags & 0x80 else 0
            u.mem_write(p + 0x2C, dwords(ap))
            u.mem_write(p + 0x38, dwords(tile))
            u.mem_write(p + 0x44, dwords(-1 if overlay is None else overlay))
            u.mem_write(p + 0xEC, dwords(land))
            u.mem_write(p + 0x11A, bytes((sub, level)))
            u.mem_write(p + 0x11E, bytes((state,)))
            u.mem_write(p + 0x140, dwords(flags))
            struct.pack_into('<I', table, (y * 512 + x) * 4, p)
        u.mem_write(TABLE, bytes(table))
        u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
        u.mem_write(MAP + 0xF4, dwords(*case['size']))
        u.mem_write(DUMMY, bytes(0x200))
        u.mem_write(DUMMY + 0x38, dwords(-1))
        u.mem_write(DUMMY + 0x44, dwords(-1))
        u.mem_write(self.family['base'], dwords(case['bridge_base']))
        if rect is not None:
            u.mem_write(MAP + 0x124, dwords(*rect))
        for key, address in GLOBALS.items():
            value = case['rim_keys'][key]
            u.mem_write(address, dwords(-1 if value is None else value))
        self.events, self.writes = [], []
        self.call(0x49F2F0, count=100)
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.write)

    def coord(self, pointer):
        return list(struct.unpack('<hh', self.uc.mem_read(pointer + 0x24, 4)))

    def snapshot(self, p):
        u = self.uc
        anchor = struct.unpack('<I', u.mem_read(p + 0x2C, 4))[0]
        return dict(coord=self.coord(p), flags=struct.unpack('<I', u.mem_read(p + 0x140, 4))[0],
                    overlay=struct.unpack('<i', u.mem_read(p + 0x44, 4))[0],
                    state=bytes(u.mem_read(p + 0x11E, 1))[0],
                    anchor=self.coord(anchor) if anchor else None)

    def observe(self, u, address, size, _):
        edge, setter = self.family['edge'], self.family['setter']
        if address not in (0x47DD70, 0x6551C0, 0x6D2140, 0x6D2790, edge, setter, 0x575EE0):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        receiver = u.reg_read(UC_X86_REG_ECX)
        args = struct.unpack('<5I', u.mem_read(sp + 4, 20))
        if address == edge:
            self.events.append(dict(kind='edge_entry', coord=list(struct.unpack('<hh', u.mem_read(args[0],4))), direction=args[1]))
            return
        if address == setter:
            self.events.append(dict(kind='setter', cell=self.snapshot(receiver), direction=args[0], set=args[1]))
            return
        if address == 0x575EE0:
            self.events.append(dict(kind='notify', endpoints=[list(struct.unpack('<hh', dwords(a))) for a in args[:2]]))
            return  # Original loop executes; source CellTags are all absent.
        if address == 0x47DD70:
            self.events.append(dict(kind='fallout_sink', cell=self.snapshot(receiver)))
            cleanup = 0
        elif address == 0x6551C0:
            self.events.append(dict(kind='radar_sink', coord=list(struct.unpack('<hh', u.mem_read(args[0],4)))))
            cleanup = 4
        elif address == 0x6D2140:
            # Projection/dirty rectangles do not select spatial writes.
            u.mem_write(args[1], dwords(0, 0))
            cleanup = 8
        else:
            self.events.append(dict(kind='screen_sink'))
            cleanup = 20
        destination = struct.unpack('<I', u.mem_read(sp,4))[0]
        u.reg_write(UC_X86_REG_ESP, sp+4+cleanup)
        u.reg_write(UC_X86_REG_EIP, destination)

    def write(self, u, access, address, size, value, _):
        if not CELLS <= address < CELLS + len(self.ptrs)*0x200:
            return
        base = CELLS + ((address-CELLS)//0x200)*0x200
        offset = address-base
        if offset in (0x2C, 0x44, 0x11E, 0x140, 0x141):
            self.writes.append(dict(coord=self.coord(base), offset=offset, size=size, value=value))

    def call(self, address, this=MAP, args=(), count=1000000):
        u = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, this)
        run_checked(u,address,RET_MAGIC,count=count)
        return u.reg_read(UC_X86_REG_EAX)

    def collapse_input(self, coord):
        p = self.ptrs[tuple(coord)]
        flags = self.snapshot(p)['flags']
        direction = 0 if flags & 0x800 else 6
        self.call(self.family['setter'], this=p, args=(direction,0))
        self.uc.mem_write(p+0x44,dwords(-1))
        self.uc.mem_write(p+0x11E,b'\0')

    def adjacent(self, coord):
        self.uc.mem_write(COORD, packed(*coord))
        return self.call(self.family['selector'], args=(COORD,))



SCENARIOS = [
    ('intact', []),
    ('single_break', [(112,140)]),
    ('adjacent_breaks', [(112,140),(112,141)]),
    ('isolated_three_rows', [(112,140),(112,144)]),
    ('reverse_break_order', [(112,144),(112,140)]),
]


def run_scenarios(case, family, scenarios, rect=None):
    results = []
    for name, breaks in scenarios:
        native = OriginalRim(case, family, rect)
        events = []
        def refresh(coord):
            before = {c:native.snapshot(p) for c,p in native.ptrs.items()}
            native.events.clear();native.writes.clear()
            native.adjacent(coord)
            changes = [dict(before=before[c],after=native.snapshot(p)) for c,p in native.ptrs.items()
                       if before[c] != native.snapshot(p)]
            events.append(dict(coord=coord,writes=list(native.writes),calls=list(native.events),changes=changes))
        if not breaks:
            refresh((112,140))
        for coord in breaks:
            # Complete original setter plus the caller's two supplied field
            # writes. Outer damage/RNG/ramp walkers are explicitly excluded.
            native.collapse_input(coord)
            refresh(coord)
        # Repeated cleanup does not write bridge fields in these cases, but
        # still repeats span notifications. Preserve those calls in the trace.
        refresh((112,140))
        results.append(dict(name=name,breaks=breaks,refreshes=events))
    return results


# MapClass::Resize 0x00565C10 writes +0x124..+0x130 as (1, 1, W+H-1, W+H-1).
# The stock span's northern start tile is at y=135: an inclusive top edge
# there keeps the search, one row lower clips it.
LOW_RECTS = dict(resize=(1, 1, 275, 275), edge=(1, 135, 275, 140), clipped=(1, 136, 275, 139))


def stock_cases():
    source = Path(__file__).with_name('bridge_rim_stock_inputs.json')
    case = json.loads(source.read_text())
    low = []
    for rect_name, rect in LOW_RECTS.items():
        scenarios = SCENARIOS if rect_name == 'resize' else SCENARIOS[3:4]
        for result in run_scenarios(case, 'low', scenarios, rect):
            low.append(dict(result, rect=list(rect), name=f"{rect_name}_{result['name']}"))
    return dict(stock_input_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                cases=run_scenarios(case, 'high', SCENARIOS), low_cases=low)


if __name__ == '__main__':
    finish_vectors(stock_cases,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Original concrete bridge selector576770, edge cleanup576200, complete setter47E040; wooden twins571050/570AE0/47E470; untagged575EE0 notification traversal',
        assumptions=[
            'Stock xbayopigs.map SHA25615b761afdc3403cac20f32ad65b60cc158b385900a73f46f78cadf021160dd15; no CellTags/Tags/Events',
            '225 supplied scalar cells from the current Rust production loader; native loader is not executed',
            'Two-break sequence on the225-cell crop matches the full37940-cell input outputs exactly',
            'Non-self anchor pointers reconstructed from supplied relationships; self anchor+2C starts0 and is unused by admitted selector paths',
            'Native49F2F0 initializes direction deltas; all ten theater keys are signed values from the supplied stock INI',
            'Break inputs execute complete native47E040, followed by supplied overlay=-1/state0 writes; outer damage, RNG and ramp walkers excluded',
            'No CellTags or objects supplied; actual575EE0 executes without gameplay callbacks',
            'Only the stock vertical high span and named break sequences covered; no repair, malformed map, or pixel equivalence claim',
            'Wooden twins run over the same concrete-span cells with the stock BridgeSet base in 0xABAD1C; no stock wooden span is loaded',
            'Wooden search rectangles: the Resize-derived (1,1,275,275), top edge on the start tile (1,135,275,140), and one row past it (1,136,275,139); the last two run isolated_three_rows only',
        ],substitutions=[
            '47DD70 records ordered receiver fields then returns; actual object damage/drop/debris excluded',
            '6551C0 records radar coordinate then returns; radar queue excluded',
            '6D2140 writes screen point0,0; projection and rectangle values excluded from equivalence',
            '6D2790 records dirty-screen call then returns; display work excluded',
        ],entry_points={'selector':0x576770,'edge':0x576200,'setter':0x47E040,
                        'low_selector':0x571050,'low_edge':0x570AE0,'low_setter':0x47E470,
                        'notification':0x575EE0,'lookup':0x5657A0,'directions':0x49F2F0}))
