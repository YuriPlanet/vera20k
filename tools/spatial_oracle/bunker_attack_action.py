"""Bounded original action dispatch; range verdicts and environmental leaves supplied.

No game code is patched. Full DetermineAction -> Unit/Foot/Techno WhatAction
entries execute; the complete ordinary Display4AAE90 path resolves a CellClass
target before its normal action switch. Supplied palette/waypoint storage has
no graphics backend; the original cursor setter is an observed sink.
This establishes action control flow, not weapon/range arithmetic or UI capture.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, image_sha256
from tools.spatial_oracle.map_queries import dwords
from tools import native_oracle
from tools.native_oracle import finish_vectors, provenance

ACTOR, TYPE, HOUSE, TARGET, TARGET_TYPE, OTHER, CELL, WEAPON, SLOT, COORD, PROJECTILE, WARHEAD = [SCRATCH+i*0x2000 for i in range(12)]
UNIT_VT=0x7F5C70
SP=STACK_BASE+STACK_SIZE-0x1000

def run(case):
    u=Uc(UC_ARCH_X86,UC_MODE_32); load_image(u)
    u.mem_map(SCRATCH,0x30000);u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(RET_MAGIC,0x1000)
    read=lambda p:struct.unpack('<I',u.mem_read(p,4))[0]
    def ret(value,argc):
        sp=u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX,value & 0xffffffff)
        u.reg_write(UC_X86_REG_EIP,read(sp));u.reg_write(UC_X86_REG_ESP,sp+4+argc*4)
    for p,t,h in ((ACTOR,TYPE,HOUSE),(TARGET,TARGET_TYPE,HOUSE if case['relation']=='own' else OTHER)):
        u.mem_write(p,dwords(UNIT_VT));u.mem_write(p+0x6C4,dwords(t));u.mem_write(p+0x6C0,dwords(t));u.mem_write(p+0x21C,dwords(h))
        u.mem_write(p+0x14,dwords(5));u.mem_write(p+0x6C,dwords(100));u.mem_write(p+0x90,b'\x01')
        u.mem_write(p+0x9C,dwords(2688,2688,0));u.mem_write(p+0x6A0,dwords(-1))
        u.mem_write(p+0x6D8,dwords(-1))
        u.mem_write(t+0x898,dwords(WEAPON));u.mem_write(t+0xB4,dwords(100))
        u.mem_write(t+0x230,b'\x01') # normal ObjectType Selectable=true
        u.mem_write(t+0x231,b'\x01') # LegalTarget=true
    u.mem_write(ACTOR+0x2E4,dwords(TARGET if case['installed'] else 0))
    u.mem_write(HOUSE+0x30,dwords(0));u.mem_write(OTHER+0x30,dwords(1))
    u.mem_write(0xA8B238,dwords(1));u.mem_write(0xA83D4C,dwords(HOUSE))
    u.mem_write(0xA8B230,dwords(SCRATCH+0x20000));u.mem_write(0x8871E0,dwords(SCRATCH+0x22000))
    u.mem_write(0xA8ECC8,dwords(1));u.mem_write(0xA8ECBC,dwords(COORD+0x100));u.mem_write(COORD+0x100,dwords(ACTOR))
    for addr,key in ((0xA8EC00,17),(0xA8EC04,17),(0xA8EBF8,18),(0xA8EBFC,18),(0xA8EC08,16),(0xA8EC0C,16)):
        u.mem_write(addr,dwords(key))
    u.mem_write(COORD,struct.pack('<hh',30,30));u.mem_write(CELL,dwords(0x7E4EEC));u.mem_write(CELL+0x24,struct.pack('<hh',30,30));u.mem_write(CELL+0x50,dwords(-1))
    u.mem_write(CELL+0x44,dwords(-1))
    u.mem_write(SLOT,dwords(WEAPON));u.mem_write(WEAPON+0xA0,dwords(PROJECTILE));u.mem_write(WEAPON+0xAC,dwords(WARHEAD));u.mem_write(WEAPON+0xB4,dwords(1280))
    u.mem_write(TYPE+0xE0E,bytes([case.get('harvester',False)]))
    u.mem_write(TYPE+0xE13,bytes([case.get('simple_deployer',False)]))
    u.mem_write(TYPE+0x6AC,bytes([case.get('deploy_fire',False)]))
    u.mem_write(ACTOR+0x6E0,bytes([case.get('deployed',False)]))
    u.mem_write(CELL+0xEC,dwords(case.get('land_type',0)))
    trail=[];trace=[]
    watch={0x692610:'DetermineAction',0x73FD50:'UnitObject',0x4DDED0:'FootObject',0x6FFEC0:'TechnoObject',0x7404B0:'UnitCell',0x4DDDE0:'FootCell',0x700600:'TechnoCell',0x700C40:'IsControllable',0x700C66:'BunkerGate',0x700C26:'NoneReturn',0x70055D:'ObjectAttackReturn',0x700AA8:'CellAttackReturn',0x7005DA:'SelectReturn'}
    if case.get('name'):
        watch.update({0x740623:'HarvesterMoveOnly',0x74037A:'ObjectSimpleDeployerTail',0x74072B:'CellSimpleDeployerTail',0x7403C7:'ObjectDeploySelectWeaponNULL',0x740793:'CellDeploySelectWeaponNULL'})
    watch.update({0x4AAE90:'DisplayEntry',0x4AAFC9:'DisplayNoObject',0x4AAFEC:'DisplayResolveCell',0x4AAFF1:'DisplayStoreCell',0x70F0B0:'DisplayModifierAction',0x4AB449:'DisplayNormalSwitch',0x4AB4EE:'DisplayAttackArm'})
    def hook(_u,pc,size,data):
        trail.append(hex(pc));trail[:]=trail[-30:]
        if pc in watch:trace.append({'kind':'original','pc':hex(pc),'name':watch[pc]})
        sp=u.reg_read(UC_X86_REG_ESP)
        if pc==0x4AAFF1:
            trace.append({'kind':'display_target_producer','pc':hex(pc),'target':u.reg_read(UC_X86_REG_EAX),'is_cell':u.reg_read(UC_X86_REG_EAX)==CELL})
        if pc==0x4AB4EE:
            trace.append({'kind':'display_target_consumer','pc':hex(pc),'target':read(sp+0x18),'is_cell':read(sp+0x18)==CELL})
        if pc==0x5353D0: # Single-selection object has no competitor.
            trace.append({'kind':'seam','name':'SelectBest-singleton','result':ACTOR});ret(ACTOR,0)
        elif pc==0x54F5C0:ret(int(read(sp+4)==17 and case.get('ctrl',True)),1)
        elif pc in (0x5657A0,0x565730):ret(CELL,1)
        elif pc==0x578080:ret(0,1)
        elif pc in (0x586360,0x5865E0):ret(0,1)
        elif pc==0x578460:ret(1,2)
        elif pc==0x74CD60:ret(0,0) # no terrain object
        elif pc==0x746CD0:
            entry={'kind':'seam','name':'SelectWeapon','result':0}
            if case.get('name'): entry['target']=read(sp+4)
            trace.append(entry);ret(0,1)
        elif pc==0x6F3970:ret(60,1) # positive stock weapon damage, not repair unit
        elif pc==0x6F7970 or pc==0x6F77B0:
            trace.append({'kind':'seam','name':'CanFireAtCell' if pc==0x6F7970 else 'CanFireAtObject','pc':hex(pc),'verdict':case['in_range'],'target':read(sp+4),'weapon_index':read(sp+8)})
            ret(int(case['in_range']),2)
        elif pc==0x6F7780:
            trace.append({'kind':'seam','name':'DisplayCanFireAtTarget','verdict':case['in_range']})
            ret(int(case['in_range']),1)
        elif pc==read(UNIT_VT+0x3C0):ret(0,3) # GetFireError OK; no ROF/legality parity claimed
    u.hook_add(UC_HOOK_CODE,hook)
    before=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    u.mem_write(SP,dwords(RET_MAGIC,COORD,TARGET if case['route']=='object' else 0,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,0x87F7E8)
    try:run_checked(u,0x692610,RET_MAGIC,count=100000,required_addresses=[0x692610,0x73FD50 if case['route']=='object' else 0x7404B0,0x6FFEC0 if case['route']=='object' else 0x700600])
    except Exception as e:raise RuntimeError(f'{case}: {e}; trail={trail}; trace={trace}') from e
    action=u.reg_read(UC_X86_REG_EAX)
    assert before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    # Execute the actual Display prefix: a missing hovered Object is NOT a
    # missing Abstract target. Valid Cell coordinates are promoted to CellClass*
    # at4AAFEC..4AAFF1 before4AB4EE asks the selected actor's vt+3AC.
    receiver,vt,sink=SCRATCH+0x28000,SCRATCH+0x29000,RET_MAGIC+0x100
    surface,pixels,waypoints=SCRATCH+0x2A000,SCRATCH+0x2A200,SCRATCH+0x2B000
    u.mem_write(receiver,dwords(vt));u.mem_write(vt+0x48,dwords(sink))
    # Display's unchanged palette/waypoint prefix has harmless storage, rather
    # than skipping the Cell target producer or supplying its stack output.
    u.mem_write(0x87F6C8,dwords(surface));u.mem_write(surface+0x174,dwords(pixels))
    for index in range(12):
        u.mem_write(HOUSE+0x210+index*4,dwords(waypoints+index*0x40))
    u.mem_write(0xB0FE58,b'\x00')  # persistent attack-move toggle absent
    u.mem_write(0xA8ECC8,dwords(case.get('display_selected_count',1)))
    assert bytes(u.mem_read(COORD,4)) != bytes(u.mem_read(0x8A03F8,4))
    u.mem_write(SP+0x20,dwords(RET_MAGIC,COORD,0,TARGET if case['route']=='object' else 0,action,0))
    u.reg_write(UC_X86_REG_ESP,SP+0x20);u.reg_write(UC_X86_REG_ECX,receiver)
    required=[0x4AAE90,0x70F0B0,0x4AB449]
    if case['route']=='cell': required += [0x4AAFC9,0x4AAFEC,0x4AAFF1]
    run_checked(u,0x4AAE90,sink,count=10000,required_addresses=required)
    sp=u.reg_read(UC_X86_REG_ESP);row,arg=struct.unpack('<II',u.mem_read(sp+4,8))
    assert u.reg_read(UC_X86_REG_ECX)==receiver and sp==SP-12 and arg==0
    assert before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    descriptor=bytes(u.mem_read(0x82D028+row*28,28))
    cursor={'output':{'entry':'004AAE90','selected_count':case.get('display_selected_count',1),'cursor_row':row,'descriptor_hex':descriptor.hex(),'descriptor_dwords':struct.unpack('<7i',descriptor)}}
    return {'input':case,'action':action,'trace':trace,'cursor':cursor,'text_sha256':before}

def generate():
    cases=[dict(route=route,installed=installed,in_range=near,relation=relation) for route in ('cell','object') for installed in (False,True) for near in (False,True) for relation in (('enemy','own') if route=='object' else ('none',))]
    cases += [dict(route='object', installed=installed,in_range=near,relation='enemy',ctrl=False) for installed in (False,True) for near in (False,True)]
    from itertools import product
    postprocess_cases=[dict(route='cell',installed=False,in_range=near,relation='none',ctrl=True,
        harvester=harv,land_type=land,name='harvester')
        for harv,land,near in product((False,True),(0,5),(False,True))]
    postprocess_cases += [dict(route=route,installed=False,in_range=near,
        relation='enemy' if route=='object' else 'none',ctrl=True,
        simple_deployer=True,deployed=deployed,deploy_fire=fire,name='simple_deployer')
        for route,deployed,fire,near in product(('cell','object'),(False,True),(False,True),(False,True))]
    display_cases=[dict(route=route,installed=True,in_range=True,
        relation='enemy' if route=='object' else 'none',ctrl=True,display_selected_count=count)
        for route,count in product(('cell','object'),(2,3))]
    return {'schema':1,'rows':[run(c) for c in cases],
            'postprocess_rows':[run(c) for c in postprocess_cases],
            'display_controls':[run(c) for c in display_cases]}

def metadata():
    return provenance(scope=__doc__,
        entry_points={'determine_action':0x692610,'unit_cell':0x7404B0,'foot_cell':0x4DDDE0,
            'techno_cell':0x700600,'unit_object':0x73FD50,'foot_object':0x4DDED0,
            'techno_object':0x6FFEC0,'is_controllable':0x700C40,
            'display_entry':0x4AAE90,'display_cell_target':0x4AAFF1,'display_normal_dispatch':0x4AB449,'cursor_table':0x82D028},
        assumptions=['One selected live normal Unit with an original vtable; supplied complete type/house pointers, positive weapon, Selectable/LegalTarget=true, idle death frame=-1, no transport capability. Normal rows have no deploy capability; postprocess rows supply the retained UnitType Harvester/IsSimpleDeployer/DeployFire and Unit+6E0 deployed fields. This is a bounded actor action fixture, not original construction/load or retail data validation.',
            'Installed controls supply nonnull Techno+2E4. Native IsControllable reads it; install lifecycle and weapon range arithmetic belong to locomotor_force_track.bunker_install and walk_cell_range_bunker corpora.',
            'Map lookup supplies one clear revealed playable flat Cell. Ctrl state supplied via original keyboard-call seam; four normal enemy Object controls have Ctrl=false.',
            'Eight Harvester controls vary Harvester flag, clear/ore LandType0/5 and range verdict under Ctrl. Sixteen SimpleDeployer controls vary Cell/Object route, deployed byte, DeployFire and range. Both wrappers select weapon with NULL in the deployed tail; trace records this input. No native deployment lifecycle, INI or weapon-selection arithmetic is claimed.',
            'Display4AAE90 executes from its real entry. The original prefix resolves valid no-Object coordinates through Map5657A0 to CellClass* at4AAFF1;4AB4EE consumes that target. Twelve empty House waypoint containers and palette storage support unchanged prefix instructions; the persistent attack-move toggle is0 and ordinary Ctrl does not select Ctrl+Shift. No Windows renderer, setter/frame/SHP output or full waypoint lifecycle is claimed.'],
        substitutions=['SelectBest5353D0 returns the supplied actor. Ordinary action rows have one selected actor; four display_controls change only the later Display selected-count input to2/3 after action calculation, establishing4AB4F6 count admission without claiming multi-object SelectBest or selection lifecycle.',
            'Keyboard54F5C0 answers the supplied Ctrl state; map lookups5657A0/565730 return supplied Cell; height578080=0, shroud586360/5865E0=false, playable578460=true and terrain-object74CD60=null.',
            'SelectWeapon746CD0 supplies index0; damage6F3970 supplies60; CanFireAt6F77B0 and coordinate wrapper6F7970 supply the case range verdict. This establishes caller control flow, not selection or range arithmetic.',
            'GetFireError virtual3C0 supplies OK; Display CanFireAtTarget6F7780 supplies the same range verdict. No reload/ROF/legality timing claim.',
            'Display receiver virtual48 terminates at an observed sink; complete immutable text SHA checked before/after both runs.'])

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,
        source_paths={'harness':Path(__file__),'native_oracle':Path(native_oracle.__file__)})
