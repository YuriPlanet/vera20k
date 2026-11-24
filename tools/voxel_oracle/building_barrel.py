"""Original building voxel loader and draw submission boundaries.

python -m tools.voxel_oracle.building_barrel --check (or explicit --write)

--raster instead rasterizes every stock building gun through the shared
original Render core at its 43DA80 matrix, reading the physical models from
the configured install's MIX archives.
"""
from pathlib import Path
import hashlib
import struct
import sys
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EIP, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked,
)
from tools.voxel_oracle.barrel_pitch import draw_frame, facing_class, bits
from tools.voxel_oracle.lighting import native_camera

TYPE, ACTOR, CELL, POINT = SCRATCH, SCRATCH + 0x3000, SCRATCH + 0x5000, SCRATCH + 0x6000
MOT_T, MOT_B, MODEL_T, MODEL_B = (SCRATCH + n for n in (0x6100, 0x6200, 0x6300, 0x6400))


def u32(value):
    return struct.pack('<I', value & 0xffffffff)


class Machine:
    def __init__(self):
        self.u, self.sp = draw_frame(bytes(8), native_camera())
        self.events = []
        self.phase = 'none'
        self.files = {}
        self.available = set()
        self.heap = SCRATCH + 0x7000
        self.failure = None
        self.allocations = 0
        self.u.hook_add(UC_HOOK_CODE, self.hook)

    def read32(self, address):
        return struct.unpack('<I', self.u.mem_read(address, 4))[0]

    def string(self, address):
        data = bytearray()
        while True:
            byte = self.u.mem_read(address + len(data), 1)[0]
            if not byte:
                return data.decode('latin1')
            data.append(byte)
            if len(data) > 512:
                raise ValueError('unterminated fixture string')

    def ret(self, value, pop):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value & 0xffffffff)
        self.u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + pop)

    def hook(self, u, pc, size, user):
        sp = u.reg_read(UC_X86_REG_ESP)
        obj = u.reg_read(UC_X86_REG_ECX)
        if pc in (0x65C780, 0x65C7E0):
            raise AssertionError('unexpected Scenario RNG')
        if self.phase == 'draw':
            if pc == 0x5657A0:
                self.ret(CELL, 4)
            elif pc == 0x706640:
                args = [self.read32(sp + 4 + 4*i) for i in range(10)]
                self.events.append(dict(part='turret' if args[0] == TYPE+0xb8 else 'barrel',
                    frame=struct.unpack('<i',u32(args[1]))[0], cache_key=args[2],
                    cache_offset=hex(args[3]-TYPE),
                    point=list(struct.unpack('<2i', u.mem_read(args[5],8))),
                    matrix_bits=bits(bytes(u.mem_read(args[6],48))),
                    cell_top=args[7], brightness=args[8], flags=args[9]))
                self.ret(0,40)
        elif self.phase == 'loader':
            if pc == 0x4739F0:
                self.files[obj] = self.string(self.read32(sp+4))
                self.ret(obj, 4)
            elif pc == 0x473C50:
                name = self.files[obj]
                available = name.upper() in self.available
                self.events.append(dict(event='available', name=name, result=available))
                self.ret(available, 4)
            elif pc == 0x7C8E17:
                self.allocations += 1
                ptr = 0 if self.failure == self.allocations else self.heap
                self.heap += 0x40
                self.ret(ptr, 0)
            elif pc in (0x755CD0, 0x5BD570):
                name = self.files[self.read32(sp+4)]
                self.events.append(dict(event='voxel' if pc == 0x755CD0 else 'motion', name=name))
                u.mem_write(obj, bytes([1]))
                self.ret(obj, 8 if pc == 0x755CD0 else 4)
            elif pc in (0x755D10, 0x5BD5A0, 0x7C8B3D, 0x43AE50, 0x431B80):
                if pc in (0x755D10, 0x5BD5A0):
                    self.events.append(dict(event='destroy_voxel' if pc == 0x755D10 else 'destroy_motion'))
                self.ret(0,0)


def loader(name, *, turret=True, barrel=False, explicit='', available=(), prior=False, failure=None):
    m = Machine()
    m.phase, m.available, m.failure = 'loader', set(available), failure
    u = m.u
    u.mem_write(TYPE+0x11b0, name.encode('latin1')+b'\0')
    u.mem_write(TYPE+0x1714, explicit.encode('latin1')+b'\0')
    u.mem_write(TYPE+0x16c5, bytes([turret, barrel]))
    if prior:
        u.mem_write(TYPE+0xb8, struct.pack('<4I',MODEL_T,MOT_T,MODEL_B,MOT_B))
    u.mem_write(m.sp,u32(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX,TYPE)
    run_checked(u, 0x45FA90, RET_MAGIC, count=100000,
                required_addresses=[0x7C9FF0, 0x7DCFC4, 0x7CA4B0] if turret or barrel else [])
    slots = []
    for offset in (0xb8,0xbc,0xc0,0xc4):
        p=m.read32(TYPE+offset)
        slots.append('null' if p==0 else 'prior' if p in (MODEL_T,MOT_T,MODEL_B,MOT_B) else 'loaded')
    return dict(name=name,turret_anim_is_voxel=turret,barrel_anim_is_voxel=barrel,
                explicit_barrel=explicit,available=sorted(available),prior=prior,
                allocation_failure=failure,slots=slots,events=m.events)


def draw(raw=0x4000,pitch=0x4000,offset=0,frame=0,counts=(1,1),turret=True,barrel=True,
         vpl=True,recoil=(0.0,0.0),recoil_states=None):
    m=Machine();m.phase='draw';u=m.u
    if recoil_states is None:
        recoil_states=tuple(int(x!=0) for x in recoil)
    u.mem_write(ACTOR,u32(0x7E3EBC));u.mem_write(ACTOR+0x520,u32(TYPE))
    u.mem_write(TYPE+0xb8,struct.pack('<4I',MODEL_T if turret else 0,MOT_T if turret else 0,
                                    MODEL_B if barrel else 0,MOT_B if barrel else 0))
    u.mem_write(TYPE+0x720,u32(offset))
    u.mem_write(TYPE+0x11e0,struct.pack('<2i',3,-60))
    u.mem_write(MOT_T+8,u32(counts[0]));u.mem_write(MOT_B+8,u32(counts[1]))
    u.mem_write(ACTOR+0x148,u32(frame))
    u.mem_write(ACTOR+0x388,facing_class(raw));u.mem_write(ACTOR+0x370,facing_class(pitch))
    for i,base in enumerate((0x3ec,0x40c)):
        u.mem_write(ACTOR+base,struct.pack('<fI',recoil[i],recoil_states[i]))
    u.mem_write(CELL+0x10a,struct.pack('<h',7))
    u.mem_write(POINT,struct.pack('<2i',100,200))
    u.mem_write(m.sp+0x130,u32(POINT))
    u.mem_write(m.sp+0x134,u32(POINT+8))
    u.mem_write(m.sp+0x1c,u32(1000))
    u.mem_write(0x887418,bytes([vpl]));u.reg_write(UC_X86_REG_EBP,ACTOR)
    run_checked(u,0x43DFC1,(0x43E410,0x43E5CC,0x43E795),count=100000,
                required_addresses=[0x4C93D0,0x5AE860,0x5AF080,0x5AF1A0,0x754BE0,0x5AF980,0x706640])
    return dict(primary_raw=raw,barrel_raw=pitch,turret_offset=offset,turret_anim_frame=frame,
                motion_counts=list(counts),turret_present=turret,barrel_present=barrel,vpl=vpl,
                recoil=list(recoil),recoil_states=list(recoil_states),draws=m.events)


def generate():
    names=['GTGCANTUR','gtgcantur','FLAKTUR','SMINTUR','SAM','LASER','YAGGUN','OUTP','CAHEAD',
           'TURMODEL','ABCTUR','ABCDTURSUFFIX','ABCDTURTUR']
    loader_rows=[]
    for name in names:
        # Supplied availability admits every candidate, so actual native names
        # and slot assignment are visible even for stock absent optional parts.
        candidates=[name[:4]+name[4:].upper()+'.VXL',name.upper()+'.VXL',name[:4]+name[4:].upper()+'.HVA']
        for prefix in (name[:4],name[:4].upper()):
            candidates += [prefix+'BARL.VXL',prefix+'BARL.HVA']
        if name.upper()=='GTGCANTUR':candidates+=['GTGCANBARL.VXL','gtgcANBARL.VXL']
        loader_rows.append(loader(name,available=[x.upper() for x in candidates]))
    loader_rows += [
        loader('GTGCANTUR',available=['GTGCANTUR.VXL','GTGCANBARL.VXL']),
        loader('GTGCANTUR',available=['GTGCANTUR.VXL']),
        loader('GTGCANTUR',available=['GTGCANBARL.VXL']),
        loader('GTGCANTUR',prior=True),
        loader('GTGCANTUR',turret=False),
        loader('SAM',turret=False,barrel=True,explicit='CUSTOM',available=['CUSTOM.VXL']),
        loader('GTGCANTUR',barrel=True,explicit='CUSTOM',available=['CUSTOM.VXL','GTGCANTUR.VXL','GTGCANBARL.VXL']),
        loader('GTGCANTUR',available=['GTGCANTUR.VXL','GTGCANBARL.VXL'],failure=1),
    ]
    draw_rows=[draw(raw=i<<11) for i in range(32)]
    draw_rows += [draw(raw=raw,pitch=pitch,offset=offset) for raw,pitch,offset in [
        (0x3000,0x3800,70),(0x5800,0x2000,-100),(0xD800,0x4800,50),
        (0x4000,0x3800,-9),(0x4000,0x3800,-8),(0x4000,0x3800,-7),
        (0x4000,0x3800,7),(0x4000,0x3800,8),(0x4000,0x3800,9),
    ]]
    draw_rows += [draw(raw=0x6000,pitch=pitch,frame=frame,counts=(3,5),vpl=vpl,recoil=recoil)
                  for pitch,frame,vpl,recoil in [
                    (0x4000,7,True,(0,0)),(0x3800,7,True,(0,0)),(0x4000,-7,True,(0,0)),
                    (0x4000,7,False,(0,0)),(0x4000,7,True,(2.5,0)),(0x4000,7,True,(0,2.5))]]
    draw_rows += [draw(raw=0x6000,pitch=pitch,frame=7,counts=(3,5),turret=False)
                  for pitch in (0x4000,0x3800)]
    draw_rows += [draw(raw=0x6000,frame=7,counts=(3,5),barrel=False)]
    draw_rows += [draw(raw=raw,pitch=pitch,offset=offset,recoil=travel,recoil_states=states)
                  for raw,pitch,offset,travel,states in [
                    (0x2000,0x4000,0,(0.,0.),(1,1)),
                    (0x6000,0x3800,0,(2.5,1.25),(1,2)),
                    (0x3000,0x3800,70,(2.5,1.25),(1,2)),
                    (0xD800,0x2000,-70,(-2.5,-1.25),(2,1)),
                  ]]
    draw_rows += [draw(raw=0x6000,pitch=0x3800,offset=70,recoil=(2.5,1.25),turret=t,barrel=b)
                  for t,b in ((True,False),(False,True))]
    return dict(source='unicorn/gamemd.exe',loader_cases=loader_rows,draw_cases=draw_rows)


# Stock TurretAnimIsVoxel buildings and the B8/C0 images 45FA90 gives them.
STOCK_GUNS = (('GTGCAN', 'GTGCANTUR', 'GTGCANBARL'), ('NAFLAK', 'FLAKTUR', None),
              ('YAREFN', 'SMINTUR', None), ('NASAM', None, 'SAM'), ('NALASR', None, 'LASER'),
              ('YAGGUN', None, 'YAGGUN'), ('CAOUTP', None, 'OUTP'), ('CAEAST02', None, 'CAHEAD'))


def gun_raster():
    """Each stock gun part's original 754510 raster at its 43DA80 matrix.

    The crop is what 706ED0 blits through 4AF2A0 and 6C89E0 stores for 707480:
    rect[2..5] of the fixed 256x256 raster buffer, drawn at point + rect[0..1].
    """
    from tools.sidebar_oracle.stock import local_file
    from tools.voxel_oracle.raster import render_native
    assets, files = {}, {}

    def read(name):
        if name not in files:
            archive, raw = local_file(name)
            files[name] = raw
            assets[name] = dict(archive=archive, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
        return files[name]

    vpl = read('VOXELS.VPL')
    rows = []
    for building, turret, barrel in STOCK_GUNS:
        images = dict(turret=turret, barrel=barrel)
        # GTGCAN's barrel also draws raised one FireAngle step.
        for pitch in (0x4000, 0x3800) if building == 'GTGCAN' else (0x4000,):
            for step in range(0, 32, 4):
                submitted = draw(raw=step << 11, pitch=pitch, turret=turret is not None,
                                 barrel=barrel is not None)
                for event in submitted['draws']:
                    if pitch != 0x4000 and event['part'] != 'barrel':
                        continue
                    # One HVA frame: the raster owner reads frame zero.
                    assert event['frame'] == 0, event
                    image = images[event['part']]
                    native = render_native(read(image + '.VXL'), read(image + '.HVA'), vpl,
                                           struct.pack('<12I', *event['matrix_bits']))
                    _, _, x, y, width, height = native['rect']
                    buffer = bytearray(65536)
                    for index, value in native['pixels']:
                        buffer[index] = value
                    crop = b''.join(buffer[(y + row) * 256 + x:(y + row) * 256 + x + width]
                                    for row in range(height))
                    lit = [(i % width, i // width) for i, value in enumerate(crop) if value]
                    # Every lit raster byte lies inside the blitted crop.
                    assert len(lit) == len(native['pixels']), (building, image, step)
                    rows.append(dict(building=building, part=event['part'], image=image,
                                     primary_raw=step << 11, barrel_raw=pitch, point=event['point'],
                                     matrix_bits=event['matrix_bits'], rect=native['rect'],
                                     crop_sha256=hashlib.sha256(crop).hexdigest(), crop_lit=len(lit),
                                     crop_lit_bounds=[min(p[0] for p in lit), min(p[1] for p in lit),
                                                      max(p[0] for p in lit), max(p[1] for p in lit)]))
    return dict(source='unicorn/gamemd.exe', assets=assets, draws=rows)


def gun_raster_metadata():
    return provenance(
        scope='Original 754510 raster of the nine stock building gun images at eight 43DA80 facings, level, plus the GTGCAN barrel raised one step; native rect and blitted crop',
        assumptions=[
            'Physical retail VXL/HVA and VOXELS.VPL entries from ra2md.mix -> localmd.mix, then ra2.mix -> local.mix; asset metadata identifies them, no file bytes are serialized. Rust consumers load the same files through the production asset owner and check length/SHA256.',
            'Matrices come from this module\'s 43DA80 draw: TurretOffset 0, no recoil, HVA frame 0, supplied point [100,200] and TurretAnimX/Y [3,-60]. Stock ART leaves TurretOffset unset for all eight buildings; the Rust consumer checks that through the production reader.',
            'The shared raster owner executes 753D00, 753E00, the per-section products, 7540F0 and 754510 as 706ED0 orders them, with original x87 0E7F and startup camera/light/viewer. No mirror flag; UseBuffer clear.',
            'The crop is rect[2..5] of the fixed 256x256 buffer at 0xB2FF78; 706ED0 (0x007071CB..0x00707235) blits it at point + rect[0..1] through 4AF2A0 and 6C89E0 (0x006C8A16..0x006C8A3C) stores the same origin and rows for 707480. Those blits are read from instructions, not executed here.',
            'Each row keeps the six rect words, the crop SHA256, its lit pixel count and lit bounds. Every lit raster byte lies inside the crop. Lighting pages, cache lifetime, the RLE blit, palette conversion and surface pixels are outside this corpus.',
        ],
        substitutions=['Existing raster owner: immutable file vtable IO and bounded allocation.',
                       'Existing 43DA80 draw: supplied cell and Techno DrawVoxel interception, as the draw corpus.'],
        entry_points={'building_draw_arm': 0x43DFC1, 'techno_draw_voxel': 0x706640,
                      'render': 0x706ED0, 'submit_box': 0x7540F0, 'sort_rasterize': 0x754510,
                      'cache_store': 0x6C89E0, 'cache_blit': 0x707480})


SOURCES = {'producer': Path(__file__),
           'shared_barrel_pitch': Path('tools/voxel_oracle/barrel_pitch.py'),
           'shared_lighting': Path('tools/voxel_oracle/lighting.py')}


if __name__ == '__main__' and '--raster' in sys.argv:
    args = sys.argv[1:]
    args.remove('--raster')
    finish_vectors(gun_raster, Path(__file__).with_name('building_gun_raster.json'),
                   provenance=gun_raster_metadata, argv=args,
                   source_paths={**SOURCES, 'shared_raster': Path('tools/voxel_oracle/raster.py'),
                                 'stock_archives': Path('tools/sidebar_oracle/stock.py')})
elif __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        scope='BuildingType45FA90 loader names/slots/failure retention; Building43DA80 turret and barrel submissions, facing order, HVA frames, cache keys and original matrices',
        assumptions=['C locale CRT globals zero; original makepath, strupr, strstr and strncpy execute.',
            'Facing timers finished, rotation rate3; native startup camera from shared lighting owner.',
            'Draw entry begins after stock TurretAnimIsVoxel/construction/selling gates; GTGCAN active reach established by static callers and retail production reader.',
            'Model, motion frame counts and cell top are supplied boundaries, not derived from Rust.',
            'The fixture does not execute VXL/HVA decoding, cache storage, rasterization, or whole-game lifecycle.'],
        substitutions=['Loader file constructors and availability are supplied fixture files; allocation and asset constructors/destructors are bounded hooks, successful MotLib starts with scale-ready byte1.',
            'Draw Map GetCell returns supplied cell; Techno DrawVoxel records ten arguments and returns without rendering.'],
        entry_points={'building_loader':0x45FA90,'building_draw_arm':0x43DFC1,
            'facing_current':0x4C93D0,'rotate_y':0x5AF080,'rotate_z':0x5AF1A0,
            'translate':0x5AE8F0,'translate_x':0x5AE980,'camera_copy':0x754BE0,'matrix_product':0x5AF980}),
        source_paths=SOURCES)
