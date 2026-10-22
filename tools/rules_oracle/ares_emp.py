"""Bounded Ares EMP numeric readers, ordinary ground delivery and AI cadence.

Reuses Probe's original PE mapping/call owner. No mod INIs/assets are inputs.
The corpus declares synthetic object/world boundaries; it is not full EMP parity.
"""
import argparse
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_MEM_READ
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, Probe, identified_bytes
from tools.input_oracle.fast_scroll import return_from_sink


def signed(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xffffffff))[0]


class EmpProbe(Probe):
    seeded_images = None
    seeded_states = None

    def __init__(self, images):
        self.input_bytes = images
        self.services = []
        self.visits = []
        self.deltas = []
        self.animations = []
        self.world = False
        self.monitor_rng = False
        super().__init__()
        self.enable_crt_thread()
        self.u.reg_write(UC_X86_REG_FPCW, 0x0e7f)
        self.original = [(at, bytes(self.u.mem_read(at, length))) for at, length in
            ((0x10001000, 0x92000), (0x401000, 0x3e1000))]
        scenario=self.alloc(0x1000);self.write(0xa8b230,scenario)
        self.rng=[scenario+0x218,0x886b88,0xabe890]
        if EmpProbe.seeded_images is not images:
            for at in self.rng:Probe.call(self,0x65c6d0,31,ecx=at)
            EmpProbe.seeded_images=images
            EmpProbe.seeded_states=[bytes(self.u.mem_read(at,0x3f4)) for at in self.rng]
        else:
            # Import the exact states executed above, never hand-computed RNG.
            for at,raw in zip(self.rng,EmpProbe.seeded_states):self.u.mem_write(at,raw)
        self.rng_before=[bytes(self.u.mem_read(at,0x3f4)) for at in self.rng]
        self.monitor_rng=True

    def check_code(self):
        assert all(bytes(self.u.mem_read(at, len(raw))) == raw for at, raw in self.original)
        if self.monitor_rng:
            assert self.rng_before==[bytes(self.u.mem_read(at,0x3f4)) for at in self.rng]

    def call(self, entry, *args, ecx=0):
        result = super().call(entry, *args, ecx=ecx)
        self.check_code()
        return result

    def hook(self, u, pc, size, data):
        if self.monitor_rng and pc in (0x65c6d0, 0x65c780, 0x65c7e0, 0x7258d0):
            raise RuntimeError(f'Unexpected RNG/Detach entry {pc:08x}')
        if pc in (0x10052870,0x10062240,0x10076660,0x10062320,
                  0x10062471, 0x10062476, 0x100624ae, 0x10062d90,
                  0x10062b40, 0x100627f0, 0x10053df0):
            self.visits.append(f'{pc:08x}')
        if pc == 0x10062476:
            self.deltas.append(signed(u.reg_read(UC_X86_REG_EAX)))
        if not self.world:
            return super().hook(u, pc, size, data)
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = lambda i: self.read(sp + 4 + 4 * i)
        result, cleanup = None, 0
        owner = u.reg_read(UC_X86_REG_ECX)
        if pc == 0x30000300:  # GetTechnoType virtual interface
            result = self.typ
        elif pc == 0x30000310:  # WhatAmI virtual interface
            result = self.type_kind if owner == self.typ else self.actor_kind
        elif pc == 0x30000320:  # IsIronCurtained virtual query
            result = self.iron_curtain
        elif pc == 0x30000330:  # cell coordinate interface for ground factory gate
            result = self.cell
        elif pc == 0x30000350:  # selected-state interface
            self.services.append(dict(service='Select', selected=bool(u.mem_read(self.actor+0x83, 1)[0])))
            u.mem_write(self.actor+0x83, b'\1')
            result = 1
        elif pc == 0x30000360:  # QueueMission owner boundary
            self.services.append(dict(service='QueueMission', mission=signed(arg(0)), force=bool(arg(1))))
            result, cleanup = 1, 8
        elif pc == 0x30000370:  # InAir virtual query, ordinary ground false
            result = 0
        elif pc == 0x30000380:  # EMP query returns the actual counter input
            result = int(self.read(self.actor+0x504) != 0)
        elif pc == 0x30000390:  # Object world-coordinate virtual interface
            u.mem_write(arg(0),bytes(u.mem_read(owner+0x9c,12)))
            result,cleanup=arg(0),4
        elif pc == 0x10011a60:  # cell's existing-building world query
            result = 0
        elif pc == 0x10048690:  # existing native Deactivate owner excluded
            self.services.append(dict(service='Deactivate', mission=signed(self.read(self.actor+0xac))))
            u.mem_write(self.actor+0x1c8, b'\1')
            # Shared owner's selection removal is a declared mutation input.
            u.mem_write(self.actor+0x83, b'\0')
            result = 0
        elif pc == 0x100487d0:  # existing native Reactivate owner excluded
            self.services.append(dict(service='Reactivate'))
            u.mem_write(self.actor+0x1c8, b'\0')
            result = 0
        elif pc == 0x10063070:  # spawn-manager boundary, ordinary victim has none
            self.services.append(dict(service='SpawnManagerBoundary', firer_present=bool(arg(1))))
            result, cleanup = 0, 8
        elif pc == 0x10001b60:  # animation storage allocator, not gameplay decision
            result = self.alloc(arg(0)); cleanup = 4
        elif pc == 0x1000aae0:  # Anim constructor world boundary
            self.animations.append(owner)
            self.services.append(dict(service='AnimConstructor', animation_type_present=bool(arg(0))))
            self.write(owner+0xc8,arg(0)); u.mem_write(owner+0x195,b'\xff')
            result, cleanup = owner, 28
        elif pc == 0x1000ac90:  # Anim::SetOwnerObject world boundary
            self.services.append(dict(service='AnimOwner', owner_is_victim=arg(0)==self.actor))
            result, cleanup = 0, 4
        if result is None:
            return super().hook(u, pc, size, data)
        return_from_sink(u, cleanup, result)

    def subject(self, fixture):
        self.world = True
        self.actor, self.typ, ext, self.cell, runtime, firer, house, other, wh, whext, verses = [
            self.alloc(n) for n in (0x1000, 0x2000, 0x600, 0x200, 0x200, 0x1000,
                                   0x6000, 0x6000, 0x300, 0x200, 0x100)]
        self.ext, self.firer, self.whext = ext, firer, whext
        vtable = self.alloc(0x400)
        for offset, sink in ((0x84,0x30000300),(0x2c,0x30000310),(0x160,0x30000320),
                             (0x1bc,0x30000330),(0x184,0x5b3040),(0x14c,0x30000350),
                             (0x1e8,0x30000360),(0x54,0x30000370),
                             (0x1dc,0x30000370),(0x290,0x30000370),(0x37c,0x30000380),
                             (0x48,0x30000390)):
            self.write(vtable+offset, sink)
        self.write(self.actor,vtable); self.write(self.typ,vtable)
        self.actor_kind = fixture.get('actor_kind', 1)
        self.type_kind = fixture.get('type_kind', 40)
        self.iron_curtain = fixture.get('iron_curtain', False)
        self.write(self.actor+0x154,runtime); self.write(self.actor+0x21c,house)
        self.write(self.actor+0x6c4,self.typ); self.write(self.typ+0x2fc,ext)
        self.write(ext,self.typ)
        self.write(self.actor+0x14,4); self.write(self.actor+0xac,fixture.get('mission',5))
        self.write(self.actor+0x504,fixture.get('old',0))
        self.write(0x100c2244,fixture.get('cpu_isa_level',0))
        self.u.mem_write(self.actor+0x83,bytes([fixture.get('selected',True)]))
        self.u.mem_write(self.actor+0x1c8,bytes([fixture.get('deactivated',False)]))
        self.u.mem_write(house+0x1ec,b'\1')
        self.write(house+0x30,0); self.write(other+0x30,1)
        source_house = house if fixture.get('same_owner',False) else other
        self.write(firer+0x21c,source_house)
        self.write(source_house+0x5788,1 if fixture.get('allied',False) else 0)
        self.write(wh+0x1cc,whext); self.write(whext,wh)
        self.u.mem_write(wh+0x179,bytes([fixture.get('affects_allies',False)]))
        self.u.mem_write(whext+0x48,bytes([fixture.get('affects_enemies',True)]))
        affects_owner=fixture.get('affects_owner')
        self.u.mem_write(whext+0x49,bytes([bool(affects_owner),affects_owner is not None]))
        self.write(whext+0xc,fixture.get('duration',120)); self.write(whext+0x10,fixture.get('cap',120))
        self.write(whext+0x24,verses)
        self.u.mem_write(verses,struct.pack('<d',fixture.get('verses',.03)))
        self.u.mem_write(ext+0x140,struct.pack('<d',fixture.get('modifier',1.0)))
        self.write(ext+0x13c,0)  # explicit no threshold destruction in selected ground chain
        immune = fixture.get('immune')
        self.u.mem_write(ext+0x13a,bytes([bool(immune),immune is not None]))
        self.u.mem_write(self.typ+0xd97,bytes([fixture.get('organic',False)]))
        self.u.mem_write(self.typ+0xeac,bytes([fixture.get('cyborg',False)]))
        self.u.mem_write(self.actor+0x150,struct.pack('<f',fixture.get('rank',0.0)))
        self.u.mem_write(ext+0x12c,bytes([fixture.get('veteran_immune',False)]))
        self.u.mem_write(ext+0x133,bytes([fixture.get('elite_immune',False)]))
        self.u.mem_write(self.typ+0xc8c,bytes([fixture.get('type_immune',False)]))
        self.write(self.typ+0x808,1); self.write(self.typ+0x80c,20)
        for key,inline,extra_offset in (('normal_emp_slot',0xa94,0x98),('elite_emp_slot',0x898,0x8c)):
            extra=self.alloc(56); self.write(ext+extra_offset,extra)
            slot=fixture.get(key)
            if slot is not None:
                weapon,weapon_wh,weapon_ext=[self.alloc(0x200) for _ in range(3)]
                self.write(weapon+0xac,weapon_wh);self.write(weapon_wh+0x1cc,weapon_ext)
                self.write(weapon_ext+0xc,120)
                self.write((self.typ+inline+28*slot) if slot<18 else extra+28*(slot-18),weapon)
        self.u.mem_write(self.actor+0x270,bytes([fixture.get('warping',False)]))
        rules=self.alloc(0x2000); self.write(0x8871e0,rules)
        sparkle_type=self.alloc(0x100)
        if fixture.get('sparkle',False): self.write(whext+0x14,sparkle_type)
        if fixture.get('default_sparkle',False): self.write(rules+0x17f4,sparkle_type)
        if fixture.get('type_sparkle') is not None:
            self.write(ext+0x148,sparkle_type if fixture['type_sparkle'] else 0)
            self.u.mem_write(ext+0x14c,b'\1')
        return self

    def state(self):
        return dict(counter=signed(self.read(self.actor+0x504)),
            deactivated=bool(self.u.mem_read(self.actor+0x1c8,1)[0]),
            selected=bool(self.u.mem_read(self.actor+0x83,1)[0]),
            saved_mission=signed(self.read(self.read(self.actor+0x154)+0x14)),
            immune_cache=list(self.u.mem_read(self.ext+0x13a,2)),
            sparkle_present=bool(self.read(self.read(self.actor+0x154)+0x10)),
            animation_iterations=[self.u.mem_read(a+0x195,1)[0] for a in self.animations])


def reader_case(images, kind, raw, prior):
    p = EmpProbe(images)
    ini, value = p.alloc(4), p.alloc(16)
    p.write(ini,0x887048)
    key = {'int':'EMP.Duration','double':'EMP.Modifier','threshold':'EMP.Threshold'}[kind]
    p.reads = {} if raw is None else {('Unit',key):raw}
    if kind == 'double':
        p.u.mem_write(value,struct.pack('<d',prior))
        result=p.call(0x1000eb00,ini,p.text('Unit'),p.text(key),ecx=value)
        output=dict(value_le_bits=bytes(p.u.mem_read(value,8)).hex(), success=bool(result&255))
    elif kind == 'int':
        p.write(value,prior)
        result=p.call(0x1000a570,ini,p.text('Unit'),p.text(key),ecx=value)
        output=dict(value=signed(p.read(value)),success=bool(result&255))
    else:
        # Exact production caller slice, using its real stack local INI ref.
        ext=p.alloc(0x200); p.write(ext+0x13c,prior)
        sp=0x2000e000; p.write(sp+0x10,0x887048)
        p.u.reg_write(UC_X86_REG_ESP,sp); p.u.reg_write(UC_X86_REG_EBX,ext)
        p.u.reg_write(UC_X86_REG_ESI,p.text('Unit'))
        run_checked(p.u,0x1003e557,0x1003e5b7,count=100000)
        p.check_code(); output=dict(value=signed(p.read(ext+0x13c)))
    return dict(kind=kind,fixture=dict(raw=raw,prior=prior),result=output)


def cap_case(images, old, delta, cap):
    p=EmpProbe(images); actor=p.alloc(0x600)
    for reg,value in ((UC_X86_REG_EAX,delta),(UC_X86_REG_EBX,old),
                      (UC_X86_REG_EDI,cap),(UC_X86_REG_ESI,actor)):
        p.u.reg_write(reg,value&0xffffffff)
    p.u.reg_write(UC_X86_REG_ESP,0x2000e000)
    run_checked(p.u,0x10062476,0x100624b4,count=100)
    p.check_code()
    return dict(fixture=dict(old=old,delta=delta,cap=cap),result=signed(p.read(actor+0x504)))


def flag_reader_case(images, kind, raw):
    p=EmpProbe(images); ini=p.alloc(4); p.write(ini,0x887048)
    target=p.alloc(16); p.u.mem_write(target,b'\x01\x00'+bytes(14))
    key={'enemies':'AffectsEnemies','owner':'AffectsOwner','abilities':'VeteranAbilities'}[kind]
    p.reads={} if raw is None else {('Unit',key):raw}
    entry={'enemies':0x1000ebf0,'owner':0x1000e9b0,'abilities':0x100405a0}[kind]
    result=p.call(entry,ini,p.text('Unit'),p.text(key),ecx=target)
    size=7 if kind=='abilities' else (2 if kind=='owner' else 1)
    return dict(kind=kind,fixture=dict(raw=raw,prior=[1]+[0]*(size-1)),
                result=dict(bytes=list(p.u.mem_read(target,size)),success=bool(result&255)))


def animation_reader_case(images, nullable, raw):
    p=EmpProbe(images); ini=p.alloc(4); p.write(ini,0x887048)
    target,prior,known,array=[p.alloc(0x100) for _ in range(4)]
    p.u.mem_write(known+0x24,b'SPARK_A\0');p.write(array,known)
    p.write(0x8b4154,array);p.write(0x8b4160,1);p.write(target,prior)
    p.reads={} if raw is None else {('Unit','EMP.Sparkles'):raw}
    result=p.call(0x1002b4d0 if nullable else 0x1000eb20,
                  ini,p.text('Unit'),p.text('EMP.Sparkles'),ecx=target)
    value=p.read(target)
    return dict(nullable=nullable,fixture=dict(raw=raw,prior='prior',prior_set=False),
        result=dict(reference='none' if not value else 'known' if value==known else 'prior',
                    set=bool(p.u.mem_read(target+4,1)[0]) if nullable else None,
                    success=bool(result&255)))


def registered_animation_reader_case(images, raw):
    p = EmpProbe(images)
    # Spare capacity is a storage boundary, not a supplied registered pointer.
    # Original427530 appends each object and writes its actual ID/index.
    for vector in (0x8b4150, 0xb0f670):
        p.write(vector+4, p.alloc(64))
        p.write(vector+8, 16)
        p.write(vector+0x10, 0)
    registered = ['SPARKLE_A', 'SPARKLE_B']
    pointers = {}
    registrations = []
    for name in registered:
        target = p.alloc(0x400)
        p.call(0x427530, p.text(name), ecx=target)
        pointers[target] = name
        registrations.append(dict(name=name, native_index=signed(p.read(target+0x294)),
                                  registry_count=p.read(0x8b4160)))
    ini, retained = p.alloc(4), p.alloc(8)
    p.write(ini, 0x887048)
    p.reads = {('Unit', 'EMP.Sparkles'):raw}
    before = p.read(0x8b4160)
    result = p.call(0x1000eb20, ini, p.text('Unit'), p.text('EMP.Sparkles'), ecx=retained)
    value = p.read(retained)
    if value and value not in pointers:
        raise RuntimeError('Reader returned an unregistered animation pointer')
    return dict(fixture=dict(registered=registered, raw=raw, prior=None),
        result=dict(registrations=registrations, reference=pointers.get(value),
                    success=bool(result&255), registry_count_before=before,
                    registry_count_after=p.read(0x8b4160)))


def sparkle_history(images, name, fixture):
    p=EmpProbe(images).subject(dict(duration=2,cap=2,**fixture))
    shim=p.alloc(0x40);p.write(shim+0xc,p.actor)
    states=[]
    p.call(0x10062320,p.actor,p.firer,p.whext);states.append(p.state())
    p.call(0x10062320,p.actor,p.firer,p.whext);states.append(p.state())
    for _ in range(2):
        p.call(0x10053df0,shim);states.append(p.state())
    return dict(name=name,fixture=fixture,result=states,services=p.services)


def collector_case(images,name,spread,objects,duplicate_neighbor=False,bridge=False,
                   building_foundation=None):
    p=EmpProbe(images).subject({})
    table=p.alloc(0x100000);p.write(0x87f924,table)
    visits=[]
    def observe_cell(_u,_access,address,size,_value,_data):
        assert size==4
        index=(address-table)//4
        visits.append([index%512,index//512])
    p.u.hook_add(UC_HOOK_MEM_READ,observe_cell,begin=table,end=table+0xfffff)
    coord=p.alloc(16);p.u.mem_write(coord,struct.pack('<iii',2688,2688,0))
    vtable=p.read(p.actor);heads={};actors=[]
    if building_foundation is not None:
        p.write(vtable+0x48, 0x447ac0)
        p.write(p.typ+0xef0, building_foundation)
    # Native linked-cell backing is a declared world input; the original walker
    # and native UID-ordered red-black tree own admission/deduplication/order.
    for item in reversed(objects):
        actor=p.alloc(0x800);p.write(actor,vtable);p.write(actor+0x10,item['uid'])
        p.write(actor+0x14,1 if item.get('techno',True) else 0)
        cell_key=tuple(item.get('cell',[10,10]))
        p.write(actor+0x30,heads.get(cell_key,0))
        p.u.mem_write(actor+0x9c,struct.pack('<iii',*item['coords']))
        if building_foundation is not None:p.write(actor+0x520,p.typ)
        heads[cell_key]=actor;actors.append(actor)
    # Preserve the legacy empty center-cell input and each linked-list order.
    if not heads:heads[(10,10)]=0
    for (x,y),head in heads.items():
        cell=p.alloc(0x200);p.write(cell+0xe4,head)
        if bridge:
            p.write(cell+0x140,0x100);p.write(cell+0xe8,head)
            p.write(cell+0xe4,p.actor);p.write(p.actor+0x10,999);p.write(p.actor+0x14,1)
            p.u.mem_write(p.actor+0x9c,struct.pack('<iii',2688,2688,0))
        p.write(table+(x+y*512)*4,cell)
        if duplicate_neighbor:p.write(table+(x+1+y*512)*4,cell)
    output=p.alloc(16);low,high=struct.unpack('<II',struct.pack('<d',spread))
    p.call(0x10076660,output,coord,low,high,1)
    sentinel=p.read(output);iterator=p.alloc(4);p.write(iterator,p.read(sentinel))
    ordered=[]
    while p.read(iterator)!=sentinel:
        if len(ordered)>len(objects):raise RuntimeError('Unbounded native set traversal')
        actor=p.read(p.read(iterator)+0x10)
        ordered.append(signed(p.read(actor+0x10)))
        p.call(0x1001e560,ecx=iterator)
    row=dict(name=name,fixture=dict(spread=spread,impact=[2688,2688,0],objects=objects,
                duplicate_neighbor=duplicate_neighbor,bridge=bridge),
                result=dict(ordered_uids=ordered,visited_cells=visits))
    if building_foundation is not None:
        row['fixture']['building_foundation']=building_foundation
        row['result']['foundation_size']=[p.call(0x45ec90,ecx=p.typ),
                                          p.call(0x45eca0,0,ecx=p.typ)]
        destination=p.alloc(12)
        coords=[]
        for actor in reversed(actors):
            p.call(0x447ac0,destination,ecx=actor)
            coords.append(list(struct.unpack('<iii',p.u.mem_read(destination,12))))
        row['result']['get_coords']=coords
    return row


def connected_case(images,name,duration):
    fixture=dict(duration=duration,cap=2,sparkle=True)
    p=EmpProbe(images).subject(fixture)
    p.write(p.actor+0x10,77);p.write(p.actor+0x14,5)
    p.u.mem_write(p.actor+0x9c,struct.pack('<iii',2688,2688,0))
    table=p.alloc(0x100000);p.write(0x87f924,table)
    cell=p.alloc(0x200);p.write(cell+0xe4,p.actor);p.write(table+(10+10*512)*4,cell)
    impact=p.actor+0x9c
    p.call(0x10052870,impact,p.firer,ecx=p.whext)
    states=[p.state()]
    shim=p.alloc(0x40);p.write(shim+0xc,p.actor)
    for _ in range(2):p.call(0x10053df0,shim);states.append(p.state())
    return dict(name=name,fixture=fixture,result=states,services=p.services,visits=p.visits,
                converted_deltas=p.deltas)


def constructor_defaults(images):
    rows=[]
    for name,entry,end in [('type',0x1003c8d2,0x1003c936),
                           ('runtime',0x10042d09,0x10042d2e)]:
        p=EmpProbe(images); target=p.alloc(0x600)
        p.u.reg_write(UC_X86_REG_ESI,target);p.u.reg_write(UC_X86_REG_ECX,0)
        p.u.reg_write(UC_X86_REG_ESP,0x2000e000)
        run_checked(p.u,entry,end,count=1000);p.check_code()
        result=(dict(immune=list(p.u.mem_read(target+0x13a,2)),
                     threshold=signed(p.read(target+0x13c)),
                     modifier_le_bits=bytes(p.u.mem_read(target+0x140,8)).hex(),
                     sparkle_present=bool(p.read(target+0x148)),
                     sparkle_set=bool(p.u.mem_read(target+0x14c,1)[0])) if name=='type'
                else dict(sparkle_present=bool(p.read(target+0x10)),last_mission=signed(p.read(target+0x14))))
        rows.append(dict(name=name,result=result))
    return rows


def pointer_expiry_case(images,matching):
    p=EmpProbe(images).subject(dict(duration=2,cap=2,sparkle=True))
    p.call(0x10062320,p.actor,p.firer,p.whext)
    shim=p.alloc(0x40);p.write(shim+0x20,p.actor)
    p.write(shim+0x24,p.animations[0] if matching else p.alloc(0x200))
    p.call(0x10046f20,shim)
    return dict(fixture=dict(matching=matching),result=p.state())


def delivery_case(images,name,fixture,repeat=1):
    p=EmpProbe(images).subject(fixture)
    states=[]
    for _ in range(repeat):
        p.call(0x10062320,p.actor,0 if fixture.get('no_firer') else p.firer,p.whext)
        states.append(p.state())
    return dict(name=name,fixture=fixture,repeat=repeat,result=states,
                services=p.services,visits=p.visits,converted_deltas=p.deltas)


def tick_case(images,name,fixture,missions):
    p=EmpProbe(images).subject(fixture); shim=p.alloc(0x40);p.write(shim+0xc,p.actor)
    states=[]
    for mission in missions:
        p.write(p.actor+0xac,mission)
        p.call(0x10053df0,shim)
        states.append(p.state())
    return dict(name=name,fixture=fixture,missions=missions,result=states,services=p.services)


class AiEmpProbe(EmpProbe):
    """Opt-in actual MissionClass queue owner for ordinary unteamed AI expiry."""

    def __init__(self, images):
        self.order = []
        self.queues = []
        super().__init__(images)

    def hook(self, u, pc, size, data):
        if pc in (0x100627f0, 0x100487d0, 0x10063070, 0x10062fd0,
                  0x1006293c, 0x5b35e0, 0x5b3614, 0x5b3641):
            self.order.append(f'{pc:08X}')
        if self.world and pc == 0x30000360:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.queues.append(dict(mission=signed(self.read(sp+4)),
                                    force_low_byte=self.read(sp+8) & 255))
            # Preserve thiscall arguments/return: the original queue body runs.
            u.reg_write(UC_X86_REG_EIP, 0x5b35e0)
            return
        return super().hook(u, pc, size, data)

    def rules_extension(self):
        ext = self.alloc(0x180)
        self.call(0x1002a360, self.read(0x8871e0), ecx=ext)
        self.write(0x100c1b80, ext)
        return ext

    def mission_value(self, ext):
        return dict(value=signed(self.read(ext+0x160)),
                    present=bool(self.u.mem_read(ext+0x164, 1)[0]))

    def read_mission(self, ext, raw):
        ini = self.alloc(4)
        self.write(ini, 0x887048)
        self.reads = {} if raw is None else {('CombatDamage', 'EMPAIRecoverMission'):raw}
        result = self.call(0x1002b4f0, ini, self.text('CombatDamage'),
                           self.text('EMPAIRecoverMission'), ecx=ext+0x160)
        return dict(raw=raw, **self.mission_value(ext), success=bool(result & 255))

    def mission_state(self):
        return dict(counter=signed(self.read(self.actor+0x504)),
            deactivated=bool(self.u.mem_read(self.actor+0x1c8, 1)[0]),
            current_mission=signed(self.read(self.actor+0xac)),
            queued_mission=signed(self.read(self.actor+0xb4)),
            bypass=self.u.mem_read(self.actor+0xb8, 1)[0],
            mission_timer=list(struct.unpack('<III', self.u.mem_read(self.actor+0xc0, 12))))


def ai_mission_reader_cases(images):
    rows = []
    histories = [('missing_default', [None]),
        ('guard_retention_then_hunt', ['Guard', None, '', 'junk', 'Hunt']),
        ('area_guard', ['Area Guard']), ('move', ['Move']), ('attack', ['Attack']),
        ('numeric_rejected', ['2']), ('none_rejected', ['none']),
        ('case_insensitive_hunt', ['hUnT'])]
    for name, raw_steps in histories:
        p = AiEmpProbe(images)
        ext = p.rules_extension()
        initial = p.mission_value(ext)
        steps = [p.read_mission(ext, raw) for raw in raw_steps]
        rows.append(dict(name=name, fixture=dict(raw_steps=raw_steps), initial=initial,
                         result=steps, code_unchanged=True, rng_streams_unchanged=True))
    return rows


def ai_expiry_case(images, name, raw, mode, human_a, human_b):
    p = AiEmpProbe(images).subject(dict(old=1, deactivated=True, selected=False))
    ext = p.rules_extension()
    parsed = p.read_mission(ext, raw)
    house = p.read(p.actor+0x21c)
    p.u.mem_write(house+0x1ec, bytes([human_a, human_b]))
    p.write(0xa8b238, mode)
    p.write(p.actor+0xac, 5)
    p.write(p.actor+0xb4, -1)
    p.u.mem_write(p.actor+0xb8, b'\1')
    # Distinct explicit input sentinels expose a spurious mission-timer reset.
    p.u.mem_write(p.actor+0xc0, struct.pack('<III', 0x11111111, 0x22222222, 0x33333333))
    p.write(p.read(p.actor+0x154)+0x14, 2)
    shim = p.alloc(0x40)
    p.write(shim+0xc, p.actor)
    p.order = []
    p.queues = []
    before = p.mission_state()
    p.call(0x10053df0, shim)
    return dict(name=name, fixture=dict(raw=raw, mode=mode, house_1ec=human_a,
        house_1ed=human_b, stored_old_mission=2), parsed=parsed, before=before,
        after=p.mission_state(), queue_calls=p.queues, order=p.order,
        services=p.services, code_unchanged=True, rng_streams_unchanged=True)

def sparkle_refresh_case(images, name, fixture):
    p = EmpProbe(images).subject(fixture)
    p.call(0x10062320, p.actor, p.firer, p.whext)
    first = p.state()
    shim = p.alloc(0x40)
    p.write(shim+0x20, p.actor)
    p.write(shim+0x24, p.animations[0])
    p.call(0x10046f20, shim)
    expired = p.state()
    p.services = []
    p.visits = []
    p.call(0x10062320, p.actor, p.firer, p.whext)
    return dict(name=name, fixture=fixture, first=first, expired=expired,
                after=p.state(), services=p.services, visits=p.visits)


class ActivationGateProbe(EmpProbe):
    """Execute the installed activation hook with explicit downstream interfaces."""

    def __init__(self, images):
        self.activation_visits = []
        super().__init__(images)

    def hook(self, u, pc, size, data):
        if pc in (0x1004ccb0, 0x100454f0, 0x10045440,
                  0x1004ccfa, 0x1004cd1e, 0x1004cda3):
            self.activation_visits.append(f'{pc:08X}')
        if self.world and pc == 0x300003a0:
            self.services.append(dict(service='Virtual3D0'))
            return_from_sink(u, 0, 0)
            return
        if self.world and pc == 0x300003b0:
            self.services.append(dict(service='LocomotorActivate'))
            return_from_sink(u, 4, 0)
            return
        return super().hook(u, pc, size, data)


def activation_case(images, name, counter, powered, center):
    p = ActivationGateProbe(images).subject(dict(old=counter, deactivated=True))
    runtime = p.read(p.actor+0x154)
    p.write(runtime, p.actor)
    vtable = p.read(p.actor)
    p.write(vtable+0x3d0, 0x300003a0)
    # Read the selected retail Unit virtual leaf, including its DeathFrame gate.
    p.write(vtable+0x37c, p.read(0x7f5c70+0x37c))
    p.write(p.actor+0x6d8, -1)
    locomotor, locomotor_vtable = p.alloc(0x100), p.alloc(0x100)
    p.write(locomotor, locomotor_vtable)
    p.write(locomotor_vtable+0x58, 0x300003b0)
    p.write(p.actor+0x674, locomotor)
    p.write(p.typ+0x5a8, -1)  # no unrelated activation audio in these controls
    p.u.mem_write(p.typ+0x410, bytes([powered]))
    if center is not None:
        house = p.read(p.actor+0x21c)
        building, building_type, array = p.alloc(0x1000), p.alloc(0x1800), p.alloc(4)
        building_vtable = p.alloc(0x400)
        p.u.mem_write(building_vtable, bytes(p.u.mem_read(vtable, 0x400)))
        p.write(building_vtable+0x37c, p.read(0x7e3ebc+0x37c))
        p.write(building, building_vtable)
        p.write(building+0x520, building_type)
        p.write(building_type+0x40c, p.typ)
        p.u.mem_write(building+0x662, b'\1')
        p.write(building+0x504, center)
        p.write(array, building)
        p.write(house+0x6c, array)
        p.write(house+0x78, 1)
    shim = p.alloc(0x40)
    p.write(shim+0x20, p.actor)  # Syringe register frame's ECX slot
    before = p.state()
    returned = p.call(0x1004ccb0, shim)
    return dict(name=name, fixture=dict(counter=counter, powered_unit=powered,
        center_counter=center, deactivated=True, death_frame=-1), before=before, after=p.state(),
        return_address=f'{returned:08X}', services=p.services, order=p.activation_visits)


class BuildingEmpProbe(AiEmpProbe):
    """Ordinary unlinked building, retaining the existing VM/queue owners.

    The adapter translates only the installed thiscall trampoline ABI to a
    Syringe register frame. Original exports own all gates and state writes.
    """

    def __init__(self, images):
        self.syringe_calls = []
        self.syringe_returns = []
        self.building_calls = []
        super().__init__(images)

    def hook(self, u, pc, size, data):
        if pc in (0x1004b320,0x1004b568,0x10062b40,0x100627f0,
                  0x1004ccb0,0x1004cdc0,0x70f850,0x5b2fd0,
                  0x10013250,0x452480,0x452410,0x425260,0x425270,
                  0x4549b0,0x4545d0,0x4547c0):
            self.building_calls.append(f'{pc:08X}')
        if self.world and pc in (0x10048690,0x100487d0):
            sp=u.reg_read(UC_X86_REG_ESP)
            frame=self.alloc(0x40)
            self.write(frame+0x20,u.reg_read(UC_X86_REG_ECX))
            self.syringe_calls.append((sp,self.read(sp),pc))
            work_sp=sp-0x100
            self.write(work_sp,0x30000400);self.write(work_sp+4,frame)
            u.reg_write(UC_X86_REG_ESP,work_sp)
            u.reg_write(UC_X86_REG_EIP,0x1004cdc0 if pc==0x10048690 else 0x1004ccb0)
            return
        if self.world and pc==0x30000400:
            sp,return_to,entry=self.syringe_calls.pop()
            self.syringe_returns.append(dict(entry=f'{entry:08X}',
                resume=f'{u.reg_read(UC_X86_REG_EAX):08X}'))
            u.reg_write(UC_X86_REG_ESP,sp+4)
            u.reg_write(UC_X86_REG_EIP,return_to)
            return
        if self.world and pc==0x10011a60:
            # The declared current cell contains this ordinary building.
            return_from_sink(u,0,self.actor)
            return
        return super().hook(u,pc,size,data)

    def building(self, fixture):
        self.subject(dict(actor_kind=6,type_kind=7,duration=5,cap=10,verses=.03,
                          immune=fixture.get('immune',False),selected=False,
                          mission=2))
        # Whole Ares Techno-head reads beyond the older scalar fixture's type
        # extension prefix. Explicit empty manager backing is owned here only.
        ext=self.alloc(0x800)
        self.u.mem_write(ext,bytes(self.u.mem_read(self.ext,0x600)))
        self.ext=ext;self.write(self.typ+0x2fc,ext)
        self.write(self.actor+0x520,self.typ);self.write(self.actor+0x14,1)
        self.write(self.read(self.actor+0x154),self.actor)
        for offset in (0x16f0,0x16f4,0x5a8,0x5ac,0xe80,0xe84):
            self.write(self.typ+offset,-1)
        self.u.mem_write(self.typ+0x1573,bytes([fixture.get('powered',True)]))
        self.write(self.typ+0xee4,fixture.get('drain',75))
        self.u.mem_write(self.typ+0x1701,bytes([fixture.get('invisible',False)]))
        for offset in (0x660,0x6ea,0x6c8):self.u.mem_write(self.actor+offset,b'\1')
        self.write(self.actor+0x6c,1000);self.write(self.actor+0x218,0x12345678)
        self.write(self.actor+0xb4,-1)
        self.u.mem_write(self.actor+0xb8,b'\1')
        self.u.mem_write(self.actor+0xc0,struct.pack('<III',101,202,303))
        self.house=self.read(self.actor+0x21c)
        self.u.mem_write(self.house+0x1ec,bytes([fixture.get('human',False),0]))
        self.write(self.house+0x53a4,1000);self.write(self.house+0x53a8,100)
        self.write(0xa8b238,3)
        self.rules_extension()
        vtable=self.alloc(0x600)
        self.u.mem_write(vtable,bytes(self.u.mem_read(self.read(self.actor),0x400)))
        self.write(self.actor,vtable)
        # Actual selected Building virtuals, including Stun's target and
        # navigation clears, mission assignment and the unselected deselect arm.
        for offset in (0x350,0x37c,0x3d0,0x480,0x3c8,0x1f0,0x284,0x150):
            self.write(vtable+offset,self.read(0x7e3ebc+offset))
        self.update_frame=self.alloc(0x40);self.write(self.update_frame+0x20,self.actor)
        self.tick_frame=self.alloc(0x40);self.write(self.tick_frame+0xc,self.actor)
        self.slot_anim=self.alloc(0x200);self.write(self.slot_anim,0x7e3354)
        self.write(self.actor+0x55c+3*4,self.slot_anim)
        for slot in range(21):self.u.mem_write(self.typ+0xf8c+slot*0x44,bytes(4))
        self.u.mem_write(self.typ+0xf8c+3*0x44,b'\1\0\0\0')
        for offset,value in ((0xac,7),(0xb4,123),(0xb8,456),(0xbc,789),
                             (0xc0,300),(0xc4,1),(0x184,22)):
            self.write(self.slot_anim+offset,value)
        self.slot_before=bytes(self.u.mem_read(self.slot_anim,0x200))
        return self

    def building_state(self):
        now=bytes(self.u.mem_read(self.slot_anim,0x200))
        return dict(**self.mission_state(),saved_mission=self.state()['saved_mission'],
            focus=self.read(self.actor+0x218),stuff=bool(self.u.mem_read(self.actor+0x6ea,1)[0]),
            online=bool(self.u.mem_read(self.actor+0x660,1)[0]),
            house_dirty=list(self.u.mem_read(self.house+0x1fc,1))+
                        list(self.u.mem_read(self.house+0x5778,2)),
            operational=bool(self.call(0x4555d0,ecx=self.actor)&255),
            operated_cached=bool(self.u.mem_read(self.read(self.actor+0x154)+0x9d,1)[0]),
            slot_paused=bool(now[0x19e]),
            slot_pointer_retained=self.read(self.actor+0x55c+3*4)==self.slot_anim,
            slot_timer_unchanged=now[0xac:0xc8]==self.slot_before[0xac:0xc8] and
                                  now[0x184:0x188]==self.slot_before[0x184:0x188])

    def operational_edge(self):
        sp=0x2000e000;self.write(sp,0x30000000)
        self.u.reg_write(UC_X86_REG_ESP,sp)
        self.u.reg_write(UC_X86_REG_ECX,self.actor)
        run_checked(self.u,0x43fb20,0x43fc39,count=20000,
                    required_addresses=(0x4555d0,))
        self.check_code()


def building_admission_cases(images):
    rows=[]
    for name,fixture in [('powered_drain_default',dict(immune=None)),
            ('plain_default',dict(immune=None,powered=False,drain=0)),
            ('invisible_default',dict(immune=None,invisible=True)),
            ('invisible_explicit_no',dict(immune=False,invisible=True))]:
        p=BuildingEmpProbe(images).building(fixture)
        immune=p.call(0x10062d90,p.actor,p.read(p.firer+0x21c))
        rows.append(dict(name=name,fixture=fixture,
            result=dict(immune=bool(immune&255),cache=list(p.u.mem_read(p.ext+0x13a,2)))))
    return rows


def building_chain_case(images,name,human=False,lowpower=False,repeat=False):
    fixture=dict(human=human,duration=5,cap=10,verses=.03,modifier=1.,
                 initial_mission=2,initial_focus=0x12345678,powered=True,drain=75,
                 slot=3,slot_flags=[1,0,0,0],lowpower=lowpower,repeat=repeat)
    p=BuildingEmpProbe(images).building(fixture)
    steps=[];states=[]
    def step(action,**values):
        start=len(p.building_calls)
        if action=='head':p.call(0x1004b320,p.update_frame)
        elif action=='hit':p.call(0x10062320,p.actor,p.firer,p.whext)
        elif action=='tick':p.call(0x10053df0,p.tick_frame)
        elif action=='edge':p.operational_edge()
        elif action=='power':p.write(p.house+0x53a4,values['output'])
        elif action=='mission':p.write(p.actor+0xac,values['value'])
        elif action=='ack_dirty':
            p.u.mem_write(p.house+0x1fc,b'\0');p.u.mem_write(p.house+0x5778,b'\0\0')
        else:raise ValueError(action)
        steps.append(dict(action=action,**values))
        states.append(dict(**p.building_state(),calls=p.building_calls[start:]))
    step('head');step('hit');step('edge')
    if repeat:
        step('mission',value=11)  # Explicit intervening diagnostic input.
        step('ack_dirty');step('hit');step('ack_dirty');step('hit')
        step('tick');step('tick');step('ack_dirty');step('hit');step('ack_dirty')
        for _ in range(10):step('tick')
    else:
        if lowpower:step('power',output=50)
        for _ in range(5):step('tick')
        if lowpower:
            step('head');step('power',output=1000)
            # Building operational/art edge precedes common Techno-head.
            step('edge');step('head')
    return dict(name=name,fixture=fixture,steps=steps,result=states,
                queue_calls=p.queues,syringe_returns=p.syringe_returns,
                code_unchanged=True,rng_streams_unchanged=True)


def building_slot_timer_case(images):
    p=BuildingEmpProbe(images).building({});anim=p.slot_anim
    p.write(anim+0xac,7);p.write(anim+0xb4,100);p.write(anim+0xb8,0);p.write(anim+0xbc,3)
    p.write(0xa8ed84,104);p.call(0x425260,ecx=anim)
    def state():
        return dict(frame=signed(p.read(anim+0xac)),paused=bool(p.u.mem_read(anim+0x19e,1)[0]),
                    timer_start=signed(p.read(anim+0xb4)),timer_state=signed(p.read(anim+0xb8)),
                    timer_duration=signed(p.read(anim+0xbc)))
    before=state();sp=0x2000e000;p.write(sp+0x40,0)
    p.u.reg_write(UC_X86_REG_ESP,sp);p.u.reg_write(UC_X86_REG_ESI,anim)
    run_checked(p.u,0x42449b,0x424b42,count=100)
    paused=state();p.call(0x425270,ecx=anim)
    p.u.reg_write(UC_X86_REG_ESP,sp);p.u.reg_write(UC_X86_REG_ESI,anim)
    run_checked(p.u,0x42449b,0x424507,count=1000,required_addresses=(0x426630,))
    resumed=state();p.check_code()
    return dict(name='pause_keeps_absolute_timer',fixture=dict(global_frame=104,
        timer_start=100,timer_duration=3,rate_reload=300,frame_step=1),
        result=dict(before=before,paused=paused,resumed=resumed),
        code_unchanged=True,rng_streams_unchanged=True)


def building_deferred_deactivation_case(images, current):
    """Actual effective-mission getter, first hit and delayed Deactivate tail."""
    p=BuildingEmpProbe(images).building({})
    # MissionClass5B3040 is also Building's actual virtual184. Construction is
    # literal18; a present current mission must take priority over that queue.
    assert p.read(0x7e3ebc+0x184)==0x5b3040
    p.write(p.actor+0xac,current);p.write(p.actor+0xb4,18)
    steps=[];states=[]
    def step(action,**values):
        start=len(p.building_calls)
        if action=='head':p.call(0x1004b320,p.update_frame)
        elif action=='hit':p.call(0x10062320,p.actor,p.firer,p.whext)
        elif action=='tick':p.call(0x10053df0,p.tick_frame)
        elif action=='queue_guard_and_ack':
            # Explicit completion/House inputs; not a Construction producer.
            p.write(p.actor+0xac,-1);p.write(p.actor+0xb4,5)
            p.u.mem_write(p.house+0x1fc,b'\0');p.u.mem_write(p.house+0x5778,b'\0\0')
        elif action!='prepared':raise ValueError(action)
        steps.append(dict(action=action,**values))
        states.append(dict(**p.building_state(),
            effective_mission=signed(p.call(0x5b3040,ecx=p.actor)),
            calls=p.building_calls[start:]))
    step('prepared');step('head');step('hit')
    if current==-1:
        step('tick');step('queue_guard_and_ack');step('head');step('tick')
    p.check_code()
    return dict(name='current_none_queued_construction' if current==-1 else
                'current_move_overrides_queued_construction',
        fixture=dict(current=current,queued=18,duration=5,cap=10,
                     initial_focus=0x12345678,mission_timer=[101,202,303]),
        steps=steps,result=states,queue_calls=p.queues,
        code_unchanged=True,rng_streams_unchanged=True)


def building_groups(images):
    return dict(building_admission=building_admission_cases(images),
        building_collectors=[collector_case(images,name,.7,objects,
            duplicate_neighbor=duplicate,building_foundation=0) for name,objects,duplicate in
            [('single_cell_exact',[dict(uid=7,coords=[2688,2688,0],cell=[10,10])],False),
             ('adjacent_center_outside',[dict(uid=7,coords=[2688,2688,0],cell=[10,10]),
                                       dict(uid=8,coords=[2944,2688,0],cell=[11,10])],False),
             ('structural_signed_uid_dedup',[dict(uid=7,coords=[2688,2688,0],cell=[10,10]),
                                           dict(uid=-2,coords=[2688,2688,0],cell=[10,10])],True)]],
        building_chains=[building_chain_case(images,'AI_fullpower'),
            building_chain_case(images,'human_fullpower',human=True),
            building_chain_case(images,'AI_lowpower_restore',lowpower=True),
            building_chain_case(images,'AI_repeat_refresh',repeat=True)],
        building_slot_timer=[building_slot_timer_case(images)])


def parent_impact_snap_case(images,name,fixture):
    """Actual parent pre-effect branch, stopping before any effect producer."""
    p=EmpProbe(images)
    bullet,target,typ=[p.alloc(n) for n in (0x200,0x800,0x2000)]
    p.write(bullet,0x7e46e4)
    p.u.mem_write(bullet+0x9c,struct.pack('<iii',*fixture['location']))
    kind=fixture['target_kind']
    p.write(bullet+0x10c,0 if kind=='none' else target)
    p.write(target,0x7e3ebc if kind=='building' else 0x7f5c70)
    p.u.mem_write(target+0x9c,struct.pack('<iii',*fixture['target_coords']))
    if kind=='building':
        p.write(target+0x520,typ)
        p.write(typ+0xef0,fixture.get('foundation',0))
        p.u.mem_write(typ+0xebc,struct.pack('<iii',*fixture.get('target_offset',[0,0,0])))
    distance=None
    if kind!='none':
        # Ares10053660 is the unchanged trampoline to planar stock5F6440,
        # not the 3-D distance5F6360 used by the original impact ladder.
        distance=signed(p.call(0x10053660,target,ecx=bullet))
    sp=0x2000e000
    p.u.mem_write(sp+0x10,struct.pack('<iii',*fixture['impact']))
    p.u.reg_write(UC_X86_REG_ESP,sp)
    p.u.reg_write(UC_X86_REG_ESI,bullet)
    required=() if kind=='none' else (0x10053660,0x5f6440,0x4cac40,0x7c5f00)
    stop=run_checked(p.u,0x10053727,(0x10053770,0x100537f2),count=20000,
                     required_addresses=required)
    p.check_code()
    return dict(name=name,fixture=fixture,result=dict(distance=distance,
        snapped=stop==0x10053770,
        impact=list(struct.unpack('<iii',p.u.mem_read(sp+0x10,12))),
        stop_address=f'{stop:08X}'),code_unchanged=True,rng_streams_unchanged=True)


def parent_impact_snap_cases(images):
    rows=[]
    for name,kind,dx,z,offset in [
            ('unit_63','unit',63,0,None),('unit_64','unit',64,0,None),
            ('unit_63_high_z','unit',63,9000,None),
            ('building_discount_below64','building',191,0,None),
            ('building_discount_64','building',192,0,None),
            ('building_uses_getcoords_not_offset','building',191,0,[16,-8,300]),
            ('no_target_keeps_input','none',63,0,None)]:
        fixture=dict(target_kind=kind,location=[2688+dx,2688,z],
                     impact=[2688+dx,2688,z],target_coords=[2688,2688,0])
        if kind=='building':fixture.update(foundation=0,target_offset=offset or [0,0,0])
        rows.append(parent_impact_snap_case(images,name,fixture))
    rows.append(parent_impact_snap_case(images,'distance_uses_bullet_location',
        dict(target_kind='unit',location=[2751,2688,0],impact=[3688,3688,300],
             target_coords=[2688,2688,0])))
    return rows


def generate(ares):
    images=(ares,native.image_bytes())
    readers=[reader_case(images,'int',raw,71) for raw in (None,'','junk','120','-30','$10','10h','2147483648')]
    readers += [reader_case(images,'double',raw,1.25) for raw in
                (None,'','junk','0.5','50%','0.1','-0.1','0.3','1.25','12.5%','7e1%','70%junk','1e309','-0%')]
    readers += [reader_case(images,'threshold',raw,71) for raw in
                (None,'','inair','InAiR','true','false','junk','2','$10','10h','1junk')]
    caps=[cap_case(images,*row) for row in
          ((0,120,120),(90,120,120),(200,120,120),(90,120,-1),(90,120,0),
           (90,-30,-1),(90,-30,10),(20,-30,-1),(2147483647,1,0),
           (2147483647,1,120),(90,-30,0),(90,0,0),(200,0,120))]
    cases=[('enemy_ground',{}),('allied_refused',dict(allied=True)),
        ('allied_admitted',dict(allied=True,affects_allies=True)),
        ('explicit_immune',dict(immune=True)),('organic_default_immune',dict(organic=True)),
        ('ordinary_infantry_explicit_no',dict(actor_kind=15,type_kind=16,immune=False)),
        ('ordinary_infantry_default_immune',dict(actor_kind=15,type_kind=16)),
        ('zero_verses_refused',dict(verses=0.0)),('threshold_verses_admitted',dict(verses=.001)),
        ('below_threshold_verses_refused',dict(verses=.000999)),
        ('half_fraction_truncates',dict(duration=121,modifier=.5)),
        ('noninteger_modifier',dict(duration=17,modifier=1.25,cap=0)),
        ('negative_ignores_modifier',dict(old=90,duration=-31,modifier=.5,cap=-1,deactivated=True)),
        ('negative_unlocks',dict(old=20,duration=-30,modifier=.5,cap=-1,deactivated=True)),
        ('positive_low_dword_overflow',dict(duration=2147483647,modifier=2.,cap=0)),
        ('positive_qword_overflow',dict(duration=120,modifier=1e30,cap=0)),
        ('iron_curtain_refused',dict(iron_curtain=True)),('warping_refused',dict(warping=True)),
        ('veteran_immunity',dict(rank=1.,veteran_immune=True)),
        ('elite_inherits_veteran_immunity',dict(rank=2.,veteran_immune=True))]
    deliver=[delivery_case(images,name,fixture) for name,fixture in cases]
    deliver += [delivery_case(images,'repeat120_cap120',{},repeat=2)]
    deliver += [delivery_case(images,name,fixture) for name,fixture in
        [('same_owner_fallback_refused',dict(same_owner=True)),
         ('same_owner_override_true',dict(same_owner=True,affects_owner=True)),
         ('same_owner_override_false',dict(same_owner=True,affects_allies=True,affects_owner=False)),
         ('enemy_filter_refused',dict(affects_enemies=False)),
         ('missing_firer_bypasses_house_filter',dict(no_firer=True,affects_enemies=False))]]
    deliver += [delivery_case(images,name,dict(same_owner=True,affects_owner=True,type_immune=True,**fixture))
        for name,fixture in
        [('same_owner_typeimmune_secondary',dict(normal_emp_slot=1)),
         ('same_owner_typeimmune_extended_slot19',dict(normal_emp_slot=19)),
         ('elite_ignores_ordinary_emp_weapon',dict(normal_emp_slot=1,rank=2.)),
         ('elite_typeimmune_extended_slot19',dict(elite_emp_slot=19,rank=2.))]]
    deliver += [delivery_case(images,'different_owner_ignores_typeimmune',
                             dict(type_immune=True,normal_emp_slot=1))]
    deliver += [delivery_case(images,'sse3_half_fraction',
                             dict(duration=121,modifier=.5,cpu_isa_level=2))]
    deliver += [delivery_case(images,name,fixture) for name,fixture in
        [('positive_sum_overflow',dict(old=2147483647,duration=1,cap=120,deactivated=True)),
         ('capzero_negative',dict(old=90,duration=-30,cap=0,deactivated=True)),
         ('capzero_zero_converted',dict(old=90,duration=1,modifier=0.,cap=0,deactivated=True))]]
    ticks=[tick_case(images,'zero_noop',dict(old=0,deactivated=True),[5]),
        tick_case(images,'one_expires',dict(old=1,deactivated=True),[5]),
        tick_case(images,'two_decrements',dict(old=2,deactivated=True),[5]),
        tick_case(images,'raw_negative_diagnostic',dict(old=-1,deactivated=True),[5]),
        tick_case(images,'unload_deferred_then_guard',dict(old=3),[18,5,5]),
        tick_case(images,'construction_deferred',dict(old=2),[19]),
        tick_case(images,'selling_deferred',dict(old=2),[16])]
    flag_readers=[flag_reader_case(images,kind,raw) for kind in ('enemies','owner','abilities')
                  for raw in (None,'','junk','yes','no','EMPIMMUNE','empimmune,radimmune','EMPIMMUNE RADIMMUNE')]
    animation_readers=[animation_reader_case(images,nullable,raw) for nullable in (False,True)
                       for raw in (None,'','none','<none>','unknown','spark_a')]
    sparkles=[sparkle_history(images,name,fixture) for name,fixture in
              [('explicit_warhead',dict(sparkle=True)),('general_fallback',dict(default_sparkle=True)),
               ('type_explicit_none_blocks_general',dict(default_sparkle=True,type_sparkle=False))]]
    collectors=[
        collector_case(images,'zero_spread_requires_exact_coord',0.,
            [dict(uid=30,coords=[2689,2688,0]),dict(uid=10,coords=[2688,2688,0])]),
        collector_case(images,'half_spread_boundary',.5,
            [dict(uid=30,coords=[2817,2688,0]),dict(uid=10,coords=[2816,2688,0]),
             dict(uid=20,coords=[2688,2688,1]),dict(uid=40,coords=[2816,2688,1])]),
        collector_case(images,'uid_order_and_duplicate_cell_membership',1.,
            [dict(uid=30,coords=[2688,2688,0]),dict(uid=10,coords=[2688,2688,0]),
             dict(uid=20,coords=[2688,2688,0])],duplicate_neighbor=True),
        collector_case(images,'signed_uid_order_structural_control',0.,
            [dict(uid=-1,coords=[2688,2688,0]),dict(uid=1,coords=[2688,2688,0]),
             dict(uid=-2147483648,coords=[2688,2688,0])]),
        collector_case(images,'radius_two_visitation',2.,[]),
        collector_case(images,'bridge_selects_upper_list',0.,
            [dict(uid=10,coords=[2688,2688,0])],bridge=True)]
    return dict(schema_version=1,ares_sha256=ARES_SHA256,fpcw='0e7f',
                readers=readers,flag_readers=flag_readers,animation_readers=animation_readers,
                caps=caps,deliver=deliver,ticks=ticks,sparkles=sparkles,collectors=collectors,
                connected=[connected_case(images,'ordinary_single_ground',2),
                           connected_case(images,'zero_duration_no_producer',0)],
                constructor_defaults=constructor_defaults(images),
                pointer_expiry=[pointer_expiry_case(images,matching) for matching in (False,True)],
                ai_mission_readers=ai_mission_reader_cases(images),
                ai_expiry=[ai_expiry_case(images,*row) for row in
                    [('skirmish_AI_default',None,3,0,0),
                     ('skirmish_AI_Guard','Guard',3,0,0),
                     ('skirmish_AI_AreaGuard','Area Guard',3,0,0),
                     ('skirmish_human',None,3,1,0),
                     ('campaign_flagB_control',None,0,0,1),
                     ('skirmish_flagB_not_human',None,3,0,1)]],

                sparkle_refresh=[sparkle_refresh_case(images,name,dict(duration=120,**fixture))
                    for name,fixture in
                    [('unchanged_explicit',dict(cap=120,sparkle=True)),
                     ('changed_explicit',dict(cap=0,sparkle=True)),
                     ('changed_general_fallback',dict(cap=0,default_sparkle=True)),
                     ('changed_type_fallback',dict(cap=0,type_sparkle=True))]],
                activation=[activation_case(images,*row) for row in
                    [('EMPactive_plain',1,False,None),('EMPexpired_plain',0,False,None),
                     ('EMPactive_center_live',1,True,0),('EMPexpired_center_missing',0,True,None),
                     ('EMPexpired_center_live',0,True,0),('EMPexpired_center_EMP',0,True,1)]],
                registered_animation_readers=[registered_animation_reader_case(images,raw)
                    for raw in ('SPARKLE_A','SPARKLE_B','UNREGISTERED')],
                parent_impact_snap=parent_impact_snap_cases(images),
                building_deferred_deactivation=[building_deferred_deactivation_case(images,current)
                    for current in (-1,2)],
                **building_groups(images))


def metadata():
    return provenance(scope='Ares ordinary ground EMP admission, numeric conversion/cap, transition and per-visit expiry controls; not whole-game parity.',
        assumptions=[
            'Synthetic ground victim/type/house/WarheadExtension and linked-cell backing are explicit inputs; no mod assets, physical INI loader or full type registration. Baseline collector controls bound zero/half/unit/two-cell spread, ground/deck lists and signed UID order/dedup; separately declared building groups add selected ordinary structures, while air remains excluded.',
            'ReadString lexical transport supplies values, actual Ares int/double/threshold parser and CRT sscanf execute. CRT per-thread accessor10085F5C returns a supplied C-locale thread block; original locale initializer10085839 executes. No scanner result is supplied.',
            'Original Ares deliver10062320, affect-house10051B70, immunity10062D90, enable10062B40, disable100627F0 and AI10053DF0 execute; synthetic virtual queries supply type/category/ground/iron-curtain and empty-cell-building interfaces; mission queries execute actual5B3040.',
            'Native Deactivate/Reactivate are existing downstream owner boundaries; supplied effects only update their deactivated/selection flags. Select and baseline QueueMission are recorded interfaces. Only ai_expiry redirects that virtual interface to unchanged native MissionClass QueueMission5B35E0, preserving arguments and recording its actual writes.',
            'Victims have no Team, Temporal, Capture, Spawn or Slave manager; threshold0. Spawn-manager is a sink. Actual sparkle selection/one-instance guard/expiry execute; animation storage/constructor/owner interface are supplied. Animation internal timers/RNG/rendering are not covered.',
            'FPCW0E7F is established explicitly. Original100772B0 conversion executes with declared CPU dispatch0 and selected SSE3 dispatch2 controls; out-of-i32 conversions yield INT_MIN, not a universal safe numeric input policy.',
            'SSE3 out-of-range FISTTP is deliberately excluded: Unicorn2.1.4 returned INT_MAX where an isolated actual-host execution of the original opcode returned INT_MIN. Software dispatch overflow rows are bounded to that executed original branch; no complete hardware/game numeric equivalence claim.',
            'Three original RNG streams are seeded31 once using native65C6D0; subsequent independent fixtures import those exact executed3F4-byte states, which remain unchanged. Observed RNG65C6D0/65C780/65C7E0 and Detach7258D0 fail controls after setup. This only bounds the declared no-manager/animation-constructor-boundary paths. Raw negative tick is malformed-state diagnostic, not admitted counter behavior.',
            'TypeImmune slot19 controls supply synthetic WeaponCount20, inline18 and extension backing arrays to actual1003DC50 getter/scanner; they do not establish production INI reachability or justify expanding the stock global weapon-slot count.',
            'Collector UID ordering is signed native Abstract+10; negative-ID row is structural comparator evidence, not ID-allocation reachability. Full3D FSQRT compare rejects dx128,z1 at radius128 whereas integer-truncated distance would incorrectly admit it.',
            'Connected controls execute actual pre-effect10052870→Create10062240→collector10076660→deliver10062320 and two AI visits on one ground victim; full Bullet detonation and original downstream Deactivate/Anim internals remain excluded.',
            'Pointer-expiry10046F20 controls clear only the matching TechnoExt sparkle reference, retaining counter and animation iteration byte. The animation is an external already-expiring owner input; whole retirement/destructor/detach scheduling is excluded.',
            'AI mission controls execute RulesExt constructor1002A360 and nullable mission reader1002B4F0 with ReadString2048 lexical inputs for [CombatDamage] EMPAIRecoverMission; native MissionNameToType5B3910 parses named missions. Missing/empty/invalid retention is executed, not supplied. Physical INI layers and the full rules loader are excluded.',
            'AI expiry controls bound an ordinary unteamed Drive victim with no complex managers: actual10053DF0→100627F0→QueueMission5B35E0 executes. Configured/default mission selection, human-mode gates, queue/current distinction and explicit mission-timer input bytes are recorded; ReadyToCommence/Commence and subsequent mission AI are not executed. Existing Reactivate/SpawnManager/Anim boundaries still apply, and no Team/harvester/air/building or whole-game AI parity is claimed.',

            'Sparkle refresh controls execute first activation, actual pointer expiry10046F20, then unchanged/changed positive delivery. Warhead-explicit versus Type/General fallback branches execute native1006256D/10062608/10062625; constructor/owner services remain supplied, not animation lifecycle/rendering parity.',
            'Activation controls execute actual Syringe export1004CCB0 with frame+20 victim and return70FC85; original PoweredUnit100454F0 and PoweredBy10045440 gates read supplied object/type/owner-list backing. Retail Unit vtable7F5C70+37C supplies actual746C90 (DeathFrame input-1 then signed-positive counter70EFD0), and Building vtable7E3EBC+37C supplies actual70EFD0. Virtual3D0 and locomotor COM activation are recorded downstream interfaces. No runtime hook installation or whole70FBE0 parity is claimed, and original Deactivate/Reactivate remain boundaries in the other groups.',
            'Registered-animation controls supply spare backing capacity for the native AnimType/AbstractType vectors, then actual constructor427530 writes IDs, registry entries, counts and indices. Actual Ares Valueable pointer reader1000EB20→1000C9C0→Find1000D3E0 resolves these constructed objects or rejects an unregistered name without allocation. Generic IDs are explicit inputs; physical registry enumeration, ART/image binding and animation lifetime are excluded. No returned pointer or lookup decision is supplied.',
            'Building admission executes IsImmune10062D90, whose true result means immune, on explicit Powered1573/positive drainEE4/Invisible1701/nullable ImmuneToEMP backing. Building collectors extend the same collector owner with actual GetCoords447AC0 and foundation width45EC90/height45ECA0; foundation index0 is an input, not a new ART reader. The physical 1x1 footprint has one occupied cell; overlapping signed-ID/duplicate-cell rows are structural controls only.',
            'Building chains execute actual shared Activate1004CCB0/Deactivate1004CDC0 through a thiscall-to-Syringe frame adapter. Actual Stun70F850, Building navigation455D50/target443B90, AssignMission5B2FD0 and unselected Deselect5F44A0 run. Empty targets/navigation, no Factory/manager/spotlight/light/gap/SW/Prism links and sound indices-1 bound the services; actual factory callback10013250 runs the no-factory arm. Current-cell GetBuilding returns the declared building, not an EMP decision.',
            'Building chains bind one native-vtable powered slot3 Anim. Actual452480/452410 and43FB20..43FC39→4549B0 loss/restore execute Pause425260/Resume425270. Prepared frame/timer input bytes are retained; Anim construction, image/rendering, full AI and loop lifetime are excluded. The separate original42449B pause gate→426630→424507 stage slice establishes absolute timer retention and a single resumed stage commit.',
            'Building AI and human controls execute the same reached EMP tail. Structure flags+14 lack Foot bit4, so actual100628B2 skips AI QueueMission recovery; no Hunt queue is supplied or expected. First delivery and full-power expiry preserve Focus around actual Stun, whereas later power recovery via whole Techno-head1004B320→1004B568→Activate clears Focus. House dirty acknowledge and changed-current-mission steps are explicit diagnostic input mutations, not a House scan or native command producer.',
            'Building phase controls call selected native regions in their original order: early operational/art edge precedes Techno-head; per-reached Techno tail follows other common work. They do not execute whole Building/Techno AI or prove Logic scheduling. Original Building43FE56 calls6F9E50; category6 skips generic stage6FABC4 because4509DE already advanced it, but still reaches shared flash and stock EMP6FAF0D. Ares replaces the stock counter tail and returns6FAFFD; stock active-retail positive-counter production is not established here.',
            'All victim virtual184 mission queries execute actual MissionClass5B3040: current+AC when present, otherwise queued+B4. Building deferred controls use raw current-1/queued Construction18 and a currentMove2 precedence control. Enable10062B98 and delayed tick10053E35 save raw current, not that effective value. Once an explicitly supplied queued Guard5 becomes advisable, actual10053DF0 calls only Deactivate10048690, not full enable10062B40: actual Stun clears Focus and mission timers while acknowledged House dirty bytes stay clear. Initial-hit Building enable instead preserves Focus and marks House/Stuff. Construction completion/House acknowledgement are explicit fixture inputs; no full Construction/Commence or whole AI is executed. Prepared slot timers retain their bytes; original code and all3 RNG states remain unchanged and guarded RNG/Detach entries are not reached.',
            'Parent pre-effect controls execute Ares10053727..10053770/100537F2 from its supplied Bullet/local-impact frame. Actual10053660 jumps planar ObjectDistance_To5F6440: concrete Bullet/Unit/Building GetCoords, XY-only Sqrt_Approx4CAC40/ftol7C5F00 and target foundation discount run unchanged. The signed distance<64 branch copies target vt48 into the local impact; it does not use incoming impact for distance or Building TargetCoordOffset/vtA4. Missing target skips query. Stops before pre-effect callbacks, AttachEffect/Airburst and EMP producer; no whole Bullet caller or detonation execution claim.',
        ],substitutions=['Existing Probe VM/call/arena, optional CRT thread block, lexical ReadString and diagnostics; declared world and existing downstream owners only. Original code bytes checked unchanged after each run.'],
        entry_points=dict(deliver=0x10062320,affect_house=0x10051b70,immunity=0x10062d90,
            enable=0x10062b40,disable=0x100627f0,tick=0x10053df0,
            int_reader=0x1000a570,double_reader=0x1000eb00,
            threshold_entry=0x1003e557,threshold_end_exclusive=0x1003e5b7,
            collector=0x10076660,cell_walker=0x10035ac0,pre_effect=0x10052870,
            create=0x10062240,sparkle=0x10062fd0,pointer_expiry=0x10046f20,
            rules_constructor=0x1002a360,nullable_mission_reader=0x1002b4f0,
            mission_name_lookup=0x5b3910,queue_mission=0x5b35e0,

            activation_hook=0x1004ccb0,powered_unit_gate=0x100454f0,
            powered_by_gate=0x10045440,unit_under_emp=0x746c90,
            techno_under_emp=0x70efd0,animation_type_constructor=0x427530,
            animation_pointer_reader=0x1000eb20,animation_pointer_find=0x1000d3e0,
            deactivation_hook=0x1004cdc0,techno_head=0x1004b320,
            building_operational=0x4555d0,building_edge=0x43fb20,
            building_enable_stuff=0x452410,building_disable_stuff=0x452480,
            building_art_dispatch=0x4549b0,building_get_coords=0x447ac0,
            foundation_width=0x45ec90,foundation_height=0x45eca0,
            stun=0x70f850,assign_mission=0x5b2fd0,effective_mission=0x5b3040,
            emp_deactivation_advisable=0x10062d60,building_navigation=0x455d50,
            building_set_target=0x443b90,anim_pause=0x425260,anim_resume=0x425270,
            anim_pause_stage=0x42449b,anim_pause_stage_end=0x424507,
            parent_snap_entry=0x10053727,parent_snap_distance=0x10053660,
            planar_object_distance=0x5f6440,parent_snap_end=0x10053770,
            parent_no_snap_end=0x100537f2))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ares-dll',type=Path,default=os.environ.get('VERA20K_ARES_DLL'))
    parser.add_argument('--gamemd-exe',type=Path)
    args,remaining=parser.parse_known_args()
    if args.ares_dll is None: parser.error('Set VERA20K_ARES_DLL or --ares-dll')
    if args.gamemd_exe: os.environ['VERA20K_GAMEMD_EXE']=str(args.gamemd_exe)
    ares=identified_bytes(args.ares_dll,ARES_SHA256)
    finish_vectors(lambda:generate(ares),Path(__file__).with_suffix('.json'),
        provenance=metadata,argv=remaining,source_paths={'producer':Path(__file__),
        'vm_owner':Path(__file__).with_name('ares_prerequisites.py'),
        'runner':Path(native.__file__),'return_owner':Path('tools/input_oracle/fast_scroll.py')})


if __name__ == '__main__':
    main()
