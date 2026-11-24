"""Original high perpendicular helpers,56EB80 and47D2B0 on supplied retail cells.

Extract Ovrps07..16.ubn with asset_extract, then point
VERA20K_BRIDGE_MIDDLE_ASSETS at their directory. The default is the task-local
.local/bridge-middle-native-assets/extract. Native loading and object fallout,
radar queues and display are excluded; the corresponding sinks are explicit.
"""
import os
import hashlib
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.spatial_oracle.bridge_rim import OriginalRim, MAP, COORD, DUMMY
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_FPCW

LAT = {
    'WaterBridge': 0xAA1090, 'ShorePieces': 0xABAD28,
    'ClearToRoughLat': 0xAA1134, 'ClearToSandLat': 0xAA0E24,
    'ClearToGreenLat': 0xAA0748, 'ClearToPaveLat': 0xAA10A8,
    'MiscPaveTile': 0xAA10A4, 'PavedRoads': 0xABBEC8, 'Medians': 0xAA0E20,
    'RampSmooth': 0xAA1058, 'RampBase': 0xABC1D8,
    'RoughTile': 0xABC2B8, 'SandTile': 0xABB104,
    'GreenTile': 0xAA0E18, 'PaveTile': 0xABC2B0,
}


class OriginalMiddle(OriginalRim):
    def __init__(self, case):
        self.recalculations = []
        self.entries = {}
        super().__init__(case)
        u = self.uc
        u.mem_write(DUMMY + 0x38, dwords(0xFFFF))
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        u.mem_map(0x44000000, 0x1000000)
        u.mem_write(0xA8ED2C, dwords(0x44000000))
        u.mem_write(0xA8ED38, dwords(300))
        self.assets = {}
        for number in range(7, 17):
            path = Path(os.environ.get('VERA20K_BRIDGE_MIDDLE_ASSETS', str(ROOT / '.local/bridge-middle-native-assets/extract'))) / f'ovrps{number:02d}.ubn'
            source = path.read_bytes()
            self.assets[path.name] = hashlib.sha256(source).hexdigest()
            tmp = bytearray(source)
            width, height = struct.unpack_from('<II', tmp)
            assert (width, height) == ((2, 5) if number < 12 else (5, 2))
            data = 0x44100000 + (number - 7) * 0x10000
            for subtile in range(width * height):
                offset = struct.unpack_from('<I', tmp, 16 + subtile * 4)[0]
                assert offset
                struct.pack_into('<I', tmp, 16 + subtile * 4, data + offset)
            head = 0x44001000 + (number - 7) * 0x400
            u.mem_write(0x44000000 + (case['bridge_base'] + number - 1) * 4, dwords(head))
            u.mem_write(head, dwords(0x7ECC48))
            u.mem_write(head + 0xA4, dwords(data))
            u.mem_write(head + 0x2C8, dwords(-1))
            u.mem_write(head + 0x2D4, dwords(-1))
            u.mem_write(head + 0x2F0, dwords(1))
            u.mem_write(data, bytes(tmp))
        # Supplied resident OverlayType BRIDGE1/2 defaults. No native loader is claimed.
        u.mem_write(0xA83D84, dwords(0x44007000))
        for overlay in (24, 25):
            u.mem_write(0x44007000 + overlay * 4, dwords(0x44008000 + overlay * 0x400))
        width, height = case['size']
        count = (width + height + 1) ** 2
        u.mem_write(MAP + 0x68, dwords(0x44200000, count, 0x44300000))
        u.mem_write(MAP + 0xFC, dwords(*case['local_size']))
        u.mem_write(0x8871E0, dwords(0x44400000))
        u.mem_write(0x44400664, bytes((2,)))
        for land, wheel in ((0, 1.0), (1, 1.0), (2, 0.0), (3, 0.0), (7, 0.5)):
            u.mem_write(0x89EA48 + land * 36, struct.pack('<f', wheel))
        self.lat_inputs = case['lat_inputs']
        for key,address in LAT.items():
            u.mem_write(address,dwords(self.lat_inputs[key]))

    def snapshot(self, p):
        result = super().snapshot(p)
        u = self.uc
        result.update(tile=struct.unpack('<i', u.mem_read(p + 0x38, 4))[0],
                      land=struct.unpack('<i', u.mem_read(p + 0xEC, 4))[0],
                      zone=struct.unpack('<i', u.mem_read(p + 0x4C, 4))[0],
                      attributes=list(u.mem_read(p + 0x11A, 4)))
        return result

    def observe(self, u, address, size, data):
        self.entries[address] = self.entries.get(address, 0) + 1
        if address in (0x547020, 0x56E990, 0x727FD0, 0x421EA0, 0x547370):
            raise AssertionError(f'Unexpected loader/pavement/Tube/animation/shadow: {address:08x}')
        if address == 0x47D2B0:
            p = u.reg_read(UC_X86_REG_ECX)
            sp = u.reg_read(UC_X86_REG_ESP)
            level = struct.unpack('<i', u.mem_read(sp + 4, 4))[0]
            row = self.snapshot(p)
            assert 283 <= row['tile'] <= 292, row
            self.recalculations.append(dict(cell=row, level=level))
        if address == 0x56DAE0:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.events.append(dict(kind='zone_connectivity_sink'))
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_EIP, struct.unpack('<I', u.mem_read(sp, 4))[0])
            u.reg_write(UC_X86_REG_ESP, sp + 8)
            return
        super().observe(u, address, size, data)


PHASES = {
    'NS': {'DamageA':0x572230,'DamageB':0x572330,'CollapseA':0x572440,'CollapseB':0x5727E0},
    'EW': {'DamageA':0x572B80,'DamageB':0x572C90,'CollapseA':0x572DA0,'CollapseB':0x573170},
}
SEQUENCES = [
    ['DamageA','DamageB','CollapseA','CollapseA'],
    ['DamageB','DamageA','CollapseB','CollapseB'],
    ['CollapseA','CollapseA'], ['CollapseB','CollapseB'],
    ['DamageB','CollapseA','CollapseA'], ['DamageA','CollapseB','CollapseB'],
]

def body_sequences(case):
    """Admitted original576BA0 visits on both sides of a physical middle.

    One neighboring span can collapse while the middle remains at level4.
    The opposite span's collapse publishes level0. Object/radar sinks retain
    their existing OriginalMiddle/OriginalRim scope; outer damage is excluded.
    """
    result = []
    for axis, points in [
        ('NS', [(91, 50), (91, 50), (93, 50), (93, 50)]),
        ('EW', [(111, 118), (111, 118), (111, 120), (111, 120)]),
    ]:
        native = OriginalMiddle(case)
        def scalars(pointer):
            cell = native.snapshot(pointer)
            return dict(coord=cell['coord'], level=cell['attributes'][1],
                        flags=cell['flags'], tile=cell['tile'],
                        overlay=cell['overlay'], state=cell['state'])
        steps = []
        for point in points:
            before = {coord: scalars(pointer) for coord, pointer in native.ptrs.items()}
            native.uc.mem_write(COORD, packed(*point))
            returned = native.call(0x576BA0, args=(COORD,))
            changes = [dict(before=before[coord], after=scalars(pointer))
                       for coord, pointer in native.ptrs.items()
                       if before[coord] != scalars(pointer)]
            steps.append(dict(input_coord=point, returned=returned, changed_cells=changes))
        result.append(dict(axis=axis, steps=steps))
    return result


def cases():
    source=Path(__file__).with_name('bridge_middle_stock_inputs.json')
    case=json.loads(source.read_text(encoding='utf-8'))
    rows=[]
    for axis,key,subtile,direction in [('NS','BridgeMiddle1',4,2),('EW','BridgeMiddle2',2,4)]:
      for phases in SEQUENCES:
        native=OriginalMiddle(case)
        target=next(r for r in case['cells'] if r[2]==case['bridge_base']+case['rim_keys'][key]-1 and r[3]==subtile)
        input_coord=(target[0]-(direction==2),target[1]-(direction==4))
        steps=[]
        for phase in phases:
            before={c:native.snapshot(p) for c,p in native.ptrs.items()}
            native.events.clear();native.recalculations.clear();native.writes.clear()
            native.uc.mem_write(COORD,packed(*input_coord))
            native.call(PHASES[axis][phase],args=(COORD,direction))
            changes=[dict(before=before[c],after=native.snapshot(p)) for c,p in native.ptrs.items() if before[c]!=native.snapshot(p)]
            steps.append(dict(phase=phase,changes=changes,recalc=list(native.recalculations),events=list(native.events),writes=list(native.writes)))
        rows.append(dict(axis=axis,target=target[:2],input=input_coord,direction=direction,steps=steps))
    return dict(cases=rows,body_sequences=body_sequences(case),asset_sha256=native.assets,stock_input_sha256=hashlib.sha256(source.read_bytes()).hexdigest())

if __name__=='__main__':
    finish_vectors(cases,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='Twelve sequences through all eight high perpendicular helpers with original connected tile replacement and Recalc; supplied retail middle cells, plus admitted576BA0 visits on both neighboring spans',
        assumptions=[
            'Retailc3y03md.map NEWURBAN source/hash and loader commit are recorded in bridge_middle_stock_inputs.json; native loading excluded',
            '338 supplied scalar cells; two13x13 crops match full18258-cell outputs for DamageA,DamageB,CollapseA on both orientations',
            'Actual ten Ovrps07..16.ubn pristine TMP files, hashes recorded; offset-to-pointer relocation supplied, native TMP loader excluded',
            'Native tile heads supplied with ctor no-animation/shadow policy, one pristine file; stock middle metadata admission independently checked',
            'Actual theater LAT bases supplied from recorded urbannmd.ini; CliffBack2 and relevant stock Wheel values supplied',
            'No objects, CellTags, occupation, or Tube inputs; unsupplied cached+11D/zone fields initiallyzero. Compare after-Recalc fields only for visited cells',
            'Non-self anchors reconstructed from supplied literal coordinates; self-anchor+2C initializedzero, compare anchor only after a recorded native+2C write',
            'Inputs target road subtiles4(NS) and2(EW); terminal recursion covers both halves and preserves duplicate fallout',
            'Two additional four-visit original576BA0 body sequences execute perpendicular/flood/Recalc on both physical neighboring structural spans. The first collapsed span leaves middle level4; collapse of the opposite span writes level0. No direct level replacement is supplied.',
            'No weapon dispatch, damage RNG, fallout reentry, runtime navigation connectivity, radar color, or rendered-output equivalence claim',
        ],substitutions=[
            '47DD70 records ordered receiver state then returns; object damage,DropIn,debris and synchronous reentry excluded',
            '6551C0 records radar coordinates then returns; radar color query and queue excluded',
            '6D2140 supplies screen0,0 and6D2790 records screen call; projection/display excluded',
            '56DAE0 records connectivity sink and returns; this sink is not reached by the admitted perpendicular sequences',
        ],entry_points={**{axis+'_'+phase:address for axis,phases in PHASES.items() for phase,address in phases.items()},
            'high_body':0x576BA0,'flood':0x56EB80,'recalc':0x47D2B0,'lat':0x47CA80,'zone':0x483C80,'slope':0x5471B0,'land':0x544BE0,'dimensions':0x547150}))
