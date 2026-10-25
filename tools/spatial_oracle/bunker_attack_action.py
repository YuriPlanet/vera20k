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
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import *
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, NativeCallTrace
from tools.spatial_oracle.map_queries import dwords
from tools import native_oracle
from tools.native_oracle import finish_vectors, provenance

ACTOR, TYPE, HOUSE, TARGET, TARGET_TYPE, OTHER, CELL, WEAPON, SLOT, COORD, PROJECTILE, WARHEAD = [SCRATCH+i*0x2000 for i in range(12)]
UNIT_VT=0x7F5C70
SP=STACK_BASE+STACK_SIZE-0x1000

def run(case, continuation=None):
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
    if case.get('allied_objects'):
        # Actual UnitType7470D0 constructor vtable (7471BF); fixture state,
        # not a constructor/load claim. Required by transport-capacity controls.
        u.mem_write(TYPE,dwords(0x7F6218));u.mem_write(TARGET_TYPE,dwords(0x7F6218))
        for house, name in ((HOUSE, 'actor'), (OTHER, 'target')):
            u.mem_write(house+0x30,dwords(case[name+'_house_index']))
            u.mem_write(house+0x5788,dwords(case[name+'_alliance_mask']))
        u.mem_write(TARGET+0x83,bytes([case.get('target_selected',False)]))
        u.mem_write(TYPE+0x6C0,bytes([case.get('attack_friendlies',False)]))
        u.mem_write(TYPE+0x6C1,bytes([case.get('attack_cursor_on_friendlies',False)]))
        u.mem_write(TARGET_TYPE+0x5E0,dwords(case.get('target_passengers',0)))
        u.mem_write(WARHEAD+0x157,bytes([case.get('ivan_bomb',False)]))
        u.mem_write(WARHEAD+0x14B,bytes([case.get('sonic',False)]))
        u.mem_write(TARGET+0x694,dwords(SCRATCH+0x2F000 if case.get('infested',False) else 0))
    u.mem_write(0xA8ECC8,dwords(1));u.mem_write(0xA8ECBC,dwords(COORD+0x100));u.mem_write(COORD+0x100,dwords(ACTOR))
    for addr,key in ((0xA8EC00,17),(0xA8EC04,17),(0xA8EBF8,18),(0xA8EBFC,18),(0xA8EC08,16),(0xA8EC0C,16)):
        u.mem_write(addr,dwords(key))
    if case.get('cursor_contract'):
        # Original type tables and empty House waypoint containers are inputs
        # to the new modifier controls. Legacy cases never queried these.
        u.mem_write(TYPE,dwords(0x7F6218))
        u.mem_write(TARGET_TYPE,dwords(0x7F6218))
        # Explicit non-WaterSet tile: the uninitialized WaterSet singleton is
        # zero, so a zero-filled TileIndex would offer a water attack.
        u.mem_write(CELL+0x38,dwords(2))
        if case.get('unarmed'):
            u.mem_write(TYPE+0x898,dwords(0))
        if case.get('mixed_unarmed_selection'):
            member,member_type=SCRATCH+0x2C000,SCRATCH+0x2D000
            u.mem_write(member,dwords(UNIT_VT))
            u.mem_write(member+0x14,dwords(5))
            u.mem_write(member+0x6C4,dwords(member_type))
            u.mem_write(member_type,dwords(0x7F6218))
            u.mem_write(COORD+0x100,dwords(ACTOR,member))
            u.mem_write(0xA8ECC8,dwords(2))
        waypoints=SCRATCH+0x2B000
        for index in range(12):
            u.mem_write(HOUSE+0x210+index*4,dwords(waypoints+index*0x40))
    u.mem_write(COORD,struct.pack('<hh',30,30));u.mem_write(CELL,dwords(0x7E4EEC));u.mem_write(CELL+0x24,struct.pack('<hh',30,30));u.mem_write(CELL+0x50,dwords(-1))
    u.mem_write(CELL+0x44,dwords(-1))
    u.mem_write(SLOT,dwords(WEAPON));u.mem_write(WEAPON+0xA0,dwords(PROJECTILE));u.mem_write(WEAPON+0xAC,dwords(WARHEAD));u.mem_write(WEAPON+0xB4,dwords(1280))
    u.mem_write(TYPE+0xE0E,bytes([case.get('harvester',False)]))
    u.mem_write(TYPE+0xE13,bytes([case.get('simple_deployer',False)]))
    u.mem_write(TYPE+0x6AC,bytes([case.get('deploy_fire',False)]))
    if case.get('repair'):
        # Supplied FV-shaped slot/health/control inputs. The original selector,
        # GetWeapon, signed Damage+AmbientDamage and health ratio execute below.
        u.mem_write(TYPE+0x808,dwords(18))
        u.mem_write(ACTOR+0x138,dwords(case.get('weapon_mode',1)))
        u.mem_write(TYPE+0x8B4,dwords(WEAPON+0x500))
        u.mem_write(WEAPON+0x98,dwords(60))
        u.mem_write(WEAPON+0x500+0x98,dwords(case.get('damage',-50)))
        u.mem_write(WEAPON+0x500+0xA4,dwords(case.get('ambient_damage',0)))
        u.mem_write(WEAPON+0x500+0xA0,dwords(PROJECTILE))
        u.mem_write(WEAPON+0x500+0xAC,dwords(WARHEAD))
        u.mem_write(TARGET_TYPE+0xA0,dwords(100))
        u.mem_write(TARGET+0x6C,dwords(case.get('health',50)))
        u.mem_write(TARGET_TYPE+0xE1B,bytes([case.get('non_vehicle',False)]))
        u.mem_write(SCRATCH+0x22000+0x16F8,struct.pack('<d',1.0))
        u.mem_write(HOUSE+0x5788,dwords(2 if case.get('allied',False) else 0))
        u.mem_write(ACTOR+0x298,bytes([case.get('actor_berserk',False)]))
    u.mem_write(ACTOR+0x6E0,bytes([case.get('deployed',False)]))
    u.mem_write(CELL+0xEC,dwords(case.get('land_type',0)))
    trail=[];trace=[]
    watch={0x692610:'DetermineAction',0x73FD50:'UnitObject',0x4DDED0:'FootObject',0x6FFEC0:'TechnoObject',0x7404B0:'UnitCell',0x4DDDE0:'FootCell',0x700600:'TechnoCell',0x700C40:'IsControllable',0x700C66:'BunkerGate',0x700C26:'NoneReturn',0x70055D:'ObjectAttackReturn',0x700AA8:'CellAttackReturn',0x7005DA:'SelectReturn'}
    if case.get('name'):
        watch.update({0x740623:'HarvesterMoveOnly',0x74037A:'ObjectSimpleDeployerTail',0x74072B:'CellSimpleDeployerTail',0x7403C7:'ObjectDeploySelectWeaponNULL',0x740793:'CellDeploySelectWeaponNULL'})
    watch.update({0x4AAE90:'DisplayEntry',0x4AAFC9:'DisplayNoObject',0x4AAFEC:'DisplayResolveCell',0x4AAFF1:'DisplayStoreCell',0x70F0B0:'DisplayModifierAction',0x4AB449:'DisplayNormalSwitch',0x4AB4EE:'DisplayAttackArm'})
    if case.get('repair'):
        watch.update({0x6F3970:'GetWeaponDamageValue',0x746CD0:'UnitSelectWeapon',
            0x6F3330:'TechnoSelectWeapon',0x70E140:'GetWeapon',0x5F5C60:'HealthRatio',
            0x73FE22:'UnitRepair',0x738890:'UnitClickedObject',0x7388D0:'RepairToAttack',
            0x4D74E0:'FootClickedObject',0x4D769F:'AttackMissionProducer'})
    call_trace=NativeCallTrace(u,read)
    if case.get('allied_objects'):
        watch.update({0x7003BD:'AttackFriendliesGrantReady',0x7003CD:'AllyGrantTest',
            0x70056C:'TechnoNonAttackTail',0x7005E6:'TechnoNoneReturn',
            0x7403EB:'UnitNonePostprocess',0x4AB9B0:'LeftUp',
            0x4ABE4A:'LeftUpSelect',0x4AE750:'LeftUpDispatch',
            0x738890:'UnitClickedObject',0x4D74E0:'FootClickedObject',
            0x4D77BD:'NoMoveObjectReceiver',0x4D77FB:'MoveAdmission',
            0x6FFBE0:'ClickedMission',0x6FFCBD:'OrderVoiceEnableGate',
            0x6FFD9B:'OrderMoveVoice',0x708FC0:'VoiceMove',0x708DB5:'QueueVoiceStore'})
    observed_specs={0x4F9A90:('IsAllyByObject',1,4),
        0x65C780:('MainRandom',0,0),0x65C7E0:('RandomRanged',2,8),
        0x708D90:('QueueVoice',1,4),0x70D150:('ActionTimerStart',0,0),
        0x7258D0:('Detach',2,8),0x5F5280:('ObjectDetach',1,4)}
    def hook(_u,pc,size,data):
        trail.append(hex(pc));trail[:]=trail[-30:]
        if pc in watch:trace.append({'kind':'original','pc':hex(pc),'name':watch[pc]})
        sp=u.reg_read(UC_X86_REG_ESP)
        if case.get('allied_objects'):
            call_trace.returned(pc,sp)
            if pc in observed_specs:call_trace.entered(pc,sp,observed_specs[pc])
        if pc==0x6FFBE0 and case.get('repair'):
            trace.append({'kind':'command_sink','pc':hex(pc),
                'mission':read(sp+4),'target':read(sp+8),
                'destination':read(sp+12),'queue':read(sp+16)})
            ret(1,4)
        if pc==0x4AAFF1:
            trace.append({'kind':'display_target_producer','pc':hex(pc),'target':u.reg_read(UC_X86_REG_EAX),'is_cell':u.reg_read(UC_X86_REG_EAX)==CELL})
        if pc==0x4AB4EE:
            trace.append({'kind':'display_target_consumer','pc':hex(pc),'target':read(sp+0x18),'is_cell':read(sp+0x18)==CELL})
        if pc==0x5353D0: # Single-selection object has no competitor.
            trace.append({'kind':'seam','name':'SelectBest-singleton','result':ACTOR});ret(ACTOR,0)
        elif pc==0x54F5C0:ret(int((read(sp+4)==17 and case.get('ctrl',True))
            or (read(sp+4)==18 and case.get('alt',False))
            or (read(sp+4)==16 and case.get('shift',False))),1)
        elif pc in (0x5657A0,0x565730):ret(CELL,1)
        elif pc==0x578080:ret(0,1)
        elif pc in (0x586360,0x5865E0):ret(0,1)
        elif pc==0x578460:ret(1,2)
        elif pc==0x74CD60:ret(0,0) # no terrain object
        elif pc==0x746CD0 and not case.get('repair') and not case.get('voice_native_selector'):
            entry={'kind':'seam','name':'SelectWeapon','result':0}
            if case.get('name'): entry['target']=read(sp+4)
            trace.append(entry);ret(0,1)
        elif pc==0x6F3970 and not case.get('repair'):ret(60,1) # positive supplied damage
        elif pc==0x6F7970 or pc==0x6F77B0:
            trace.append({'kind':'seam','name':'CanFireAtCell' if pc==0x6F7970 else 'CanFireAtObject','pc':hex(pc),'verdict':case['in_range'],'target':read(sp+4),'weapon_index':read(sp+8)})
            ret(int(case['in_range']),2)
        elif pc==0x6F7780:
            trace.append({'kind':'seam','name':'DisplayCanFireAtTarget','verdict':case['in_range']})
            ret(int(case['in_range']),1)
        elif pc==read(UNIT_VT+0x3C0):ret(0,3) # GetFireError OK; no ROF/legality parity claimed
    u.hook_add(UC_HOOK_CODE,hook)
    before=hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    techno_only=case.get('techno_only',False)
    object_target=(ACTOR if case.get('self_target') else TARGET) if case['route']=='object' else 0
    u.mem_write(SP,dwords(RET_MAGIC,*((object_target,0) if techno_only else (COORD,object_target,int(case.get('mouse_over',False))))));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ACTOR if techno_only else 0x87F7E8)
    required=[0x6FFEC0] if techno_only else [0x692610,0x73FD50 if case['route']=='object' else 0x7404B0,0x6FFEC0 if case['route']=='object' else 0x700600]
    try:run_checked(u,0x6FFEC0 if techno_only else 0x692610,RET_MAGIC,count=100000,required_addresses=required)
    except Exception as e:raise RuntimeError(f'{case}: {e}; trail={trail}; trace={trace}') from e
    action=u.reg_read(UC_X86_REG_EAX)
    assert before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    if techno_only:
        call_trace.returned(RET_MAGIC,u.reg_read(UC_X86_REG_ESP))
        assert not call_trace.pending
        assert not any(row['name']!='IsAllyByObject' for row in call_trace.calls)
        return dict(input=case,entry='0x006FFEC0',action=action,trace=trace,
                    ally_queries=[bool(row['return_eax']&255) for row in call_trace.calls if row['name']=='IsAllyByObject'],
                    main_draw_count=0,timer_start_calls=0,detach_calls=0)
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
    u.mem_write(SP+0x20,dwords(RET_MAGIC,COORD,0,object_target,action,0))
    u.reg_write(UC_X86_REG_ESP,SP+0x20);u.reg_write(UC_X86_REG_ECX,receiver)
    required=[0x4AAE90,0x70F0B0,0x4AB449]
    if case['route']=='cell': required += [0x4AAFC9,0x4AAFEC,0x4AAFF1]
    run_checked(u,0x4AAE90,sink,count=10000,required_addresses=required)
    sp=u.reg_read(UC_X86_REG_ESP);row,arg=struct.unpack('<II',u.mem_read(sp+4,8))
    assert u.reg_read(UC_X86_REG_ECX)==receiver and sp==SP-12 and arg==0
    assert before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    descriptor=bytes(u.mem_read(0x82D028+row*28,28))
    cursor={'output':{'entry':'004AAE90','selected_count':case.get('display_selected_count',1),'cursor_row':row,'descriptor_hex':descriptor.hex(),'descriptor_dwords':struct.unpack('<7i',descriptor)}}
    if case.get('repair'):
        # ClickedAction re-queries WhatAction, then converts Repair29 to Attack5.
        # Stop enemy Move2 at the existing coordinate receiver: its locomotor
        # and event-byte production are outside this bounded action fixture.
        u.mem_write(SP,dwords(RET_MAGIC,action,TARGET,0))
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ACTOR)
        stop=run_checked(u,0x738890,(RET_MAGIC,0x4D78BA),count=100000,
            required_addresses=[0x738890,0x73FD50])
        trace.append({'kind':'clicked_action_stop','pc':hex(stop)})
        assert before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    result={'input':case,'action':action,'trace':trace,'cursor':cursor,'text_sha256':before}
    if continuation is not None:
        result.update(continuation(u,read,ret,call_trace,result,receiver))
        assert before==hashlib.sha256(bytes(u.mem_read(0x401000,0x3E0000))).hexdigest()
    return result

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
    repair_base=dict(route='object',installed=False,in_range=True,relation='own',ctrl=False,repair=True)
    repair_cases=[repair_base | dict(name=name,**changes) for name,changes in (
        ('damaged_friend',{}),('full_friend',dict(health=100)),
        ('overfull_friend',dict(health=101)),('empty_ifv',dict(weapon_mode=0)),
        ('enemy',dict(relation='enemy')),('non_vehicle',dict(non_vehicle=True)),
        ('ctrl_friend',dict(ctrl=True)),('alt_friend',dict(alt=True)),
        ('ctrl_alt_friend',dict(ctrl=True,alt=True)),
        ('shift_friend',dict(shift=True)),('far_friend',dict(in_range=False)),
        ('one_way_ally',dict(relation='enemy',allied=True)),
        ('zero_total_damage',dict(ambient_damage=50)),
        ('positive_total_damage',dict(ambient_damage=51)),
        ('berserk_friend',dict(actor_berserk=True)),
    )]
    return {'schema':1,'rows':[run(c) for c in cases],
            'postprocess_rows':[run(c) for c in postprocess_cases],
            'display_controls':[run(c) for c in display_cases],
            'repair_rows':[run(c) for c in repair_cases]}

def metadata():
    return provenance(scope=__doc__,
        entry_points={'determine_action':0x692610,'unit_cell':0x7404B0,'foot_cell':0x4DDDE0,
            'techno_cell':0x700600,'unit_object':0x73FD50,'foot_object':0x4DDED0,
            'techno_object':0x6FFEC0,'is_controllable':0x700C40,
            'display_entry':0x4AAE90,'display_cell_target':0x4AAFF1,'display_normal_dispatch':0x4AB449,'cursor_table':0x82D028,
            'unit_clicked_object':0x738890,'foot_clicked_object':0x4D74E0,
            'weapon_damage_value':0x6F3970,'get_weapon':0x70E140,'health_ratio':0x5F5C60},
        assumptions=['One selected live normal Unit with an original vtable; supplied complete type/house pointers, positive weapon, Selectable/LegalTarget=true, idle death frame=-1, no transport capability. Normal rows have no deploy capability; postprocess rows supply the retained UnitType Harvester/IsSimpleDeployer/DeployFire and Unit+6E0 deployed fields. This is a bounded actor action fixture, not original construction/load or retail data validation.',
            'Installed controls supply nonnull Techno+2E4. Native IsControllable reads it; install lifecycle and weapon range arithmetic belong to locomotor_force_track.bunker_install and walk_cell_range_bunker corpora.',
            'Map lookup supplies one clear revealed playable flat Cell. Ctrl state supplied via original keyboard-call seam; four normal enemy Object controls have Ctrl=false.',
            'Eight Harvester controls vary Harvester flag, clear/ore LandType0/5 and range verdict under Ctrl. Sixteen SimpleDeployer controls vary Cell/Object route, deployed byte, DeployFire and range. Both wrappers select weapon with NULL in the deployed tail; trace records this input. No native deployment lifecycle, INI or weapon-selection arithmetic is claimed.',
            'Repair rows supply an authorized local Unit, TurretCount18, current slot1, Damage=-50 and AmbientDamage=0 (except sign controls), a positive slot0, target Strength100 with health50/100/101, and Rules+16F8=1.0. These are supplied control inputs, not native retail reader or construction claims. GetWeaponDamageValue6F3970, Unit/Techno SelectWeapon and GetWeapon70E140 execute unchanged. No Engineer flag, cargo type, warhead name or repair-result decision is supplied. Own/allied/enemy/NonVehicle, near/far and Ctrl/Alt/Shift controls execute the complete original action/display paths. The caller sets one-way alliance bit1 in House+5788.',
            'Repair ClickedAction738890 re-queries the object action and executes the original Repair29-to-Attack5 conversion and Foot4D769F mission/target/destination argument producer. The berserk_friend control differs from damaged_friend only by the supplied actor+298 Berserk byte1: action/display still resolve Repair29/cursor33, while unchanged UnitClickedAction suppresses the command. This is a bounded NoCommand producer observation, not berserk initialization, expiry or Hunt lifecycle validation. PlayerSendCommand6FFBE0 is an observed argument sink, not a supplied outcome. Enemy Move2 stops at4D78BA before its coordinate/locomotor dependency. Complete event encoding/delivery, movement, firing/repair cadence, RNG, audio and Windows pixels are excluded.',
            'Display4AAE90 executes from its real entry. The original prefix resolves valid no-Object coordinates through Map5657A0 to CellClass* at4AAFF1;4AB4EE consumes that target. Twelve empty House waypoint containers and palette storage support unchanged prefix instructions; the persistent attack-move toggle is0 and ordinary Ctrl does not select Ctrl+Shift. No Windows renderer, setter/frame/SHP output or full waypoint lifecycle is claimed.'],
        substitutions=['SelectBest5353D0 returns the supplied actor. Ordinary action rows have one selected actor; four display_controls change only the later Display selected-count input to2/3 after action calculation, establishing4AB4F6 count admission without claiming multi-object SelectBest or selection lifecycle.',
            'Keyboard54F5C0 answers the supplied Ctrl state; map lookups5657A0/565730 return supplied Cell; height578080=0, shroud586360/5865E0=false, playable578460=true and terrain-object74CD60=null.',
            'Ordinary/postprocess rows: SelectWeapon746CD0 supplies index0 and damage6F3970 supplies60. Repair rows execute both bodies unchanged. CanFireAt6F77B0 and coordinate wrapper6F7970 supply the case range verdict for every row. This establishes caller control flow, not range arithmetic.',
            'GetFireError virtual3C0 supplies OK; Display CanFireAtTarget6F7780 supplies the same range verdict. No reload/ROF/legality timing claim.',
            'Display receiver virtual48 terminates at an observed sink; complete immutable text SHA checked before/after both runs.'])


def allied_case(name, **changes):
    value=dict(name=name,route='object',installed=False,in_range=True,relation='other',
        ctrl=False,alt=False,shift=False,allied_objects=True,
        actor_house_index=0,target_house_index=1,actor_alliance_mask=2,
        target_alliance_mask=1,target_selected=False,attack_friendlies=False,
        attack_cursor_on_friendlies=False,target_passengers=0,ivan_bomb=False,
        sonic=False,infested=False)
    value.update(changes)
    if value['relation']=='own':value['target_house_index']=value['actor_house_index']
    return value


def allied_continuation(u, read, ret, calls, result, receiver):
    """Continue the existing action VM to literal UI/receiver boundaries."""
    action=result['action']
    u.mem_write(COORD+0x40,dwords(2688,2688,0))
    u.mem_write(SP,dwords(RET_MAGIC,COORD+0x40,COORD,TARGET,action,0))
    u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,receiver)
    stop=run_checked(u,0x4AB9B0,
        (0x4AE750,0x4ABE4A,0x4ABD63,0x4ABD94,0x4AC294,RET_MAGIC),
        count=100000,required_addresses=[0x4AB9B0,0x4ABD1D])
    boundary={0x4AE750:'object_dispatch',0x4ABE4A:'replace_selection',
        0x4ABD63:'shift_selected',0x4ABD94:'shift_unselected',
        0x4AC294:'no_dispatch',RET_MAGIC:'returned'}[stop]
    left_up=dict(entry='0x004AB9B0',stop=hex(stop),boundary=boundary)
    clicked=None
    if stop==0x4AE750:
        # Actual per-object receiver. The ordinary NoMove path stops BEFORE
        # native navigation/passability selection; no destination is supplied.
        u.mem_write(SP,dwords(RET_MAGIC,action,TARGET,0))
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ACTOR)
        stop=run_checked(u,0x738890,(RET_MAGIC,0x4D78BA,0x6FFBE0),
            count=100000,required_addresses=[0x738890,0x73FD50])
        clicked=dict(entry='0x00738890',stop=hex(stop),
            boundary={RET_MAGIC:'refused',0x4D78BA:'object_navigation_coordinates',
                      0x6FFBE0:'mission_arguments'}[stop])
        if stop==0x6FFBE0:
            sp=u.reg_read(UC_X86_REG_ESP)
            clicked['mission']=read(sp+4)
            clicked['target_is_hovered_object']=read(sp+8)==TARGET
    if calls.pending:
        raise RuntimeError(f'Unreturned original calls: {calls.pending}')
    assert not any(row['name'] in ('MainRandom','RandomRanged','ActionTimerStart','Detach','ObjectDetach')
                   for row in calls.calls)
    return dict(left_up=left_up,clicked_action=clicked,
        ally_queries=[dict(asking_house='actor' if int(row['ecx'],16)==HOUSE else 'target',
                           target_is_hovered_object=row['args'][0]==TARGET,
                           allied=bool(row['return_eax']&255))
                      for row in calls.calls if row['name']=='IsAllyByObject'],
        main_draw_count=0,timer_start_calls=0,detach_calls=0)


def voice_control(spec, *, attack=False, move=False):
    """Use the SAME action bootstrap; shared RNG owner supplies seed/continuation."""
    from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, seeded_struct, draws

    def continuation(u, read, ret, calls, result, receiver):
        rngs={'main':0x886B88,'scenario':SCRATCH+0x20000+0x218,'mapgen':0xABE890}
        for pointer in rngs.values():u.mem_write(pointer,seeded_struct(spec['seed']))
        before={name:bytes(u.mem_read(pointer,STRUCT_LEN)) for name,pointer in rngs.items()}
        voice_data=SCRATCH+0x2D000
        values=spec['voice_ids']
        u.mem_write(voice_data,dwords(*(values or [0])))
        u.mem_write(TYPE+(0x488 if attack else 0x46C),dwords(voice_data,len(values)))
        u.mem_write(TYPE+(0x494 if attack else 0x478),dwords(len(values)))
        assert not (attack and move)
        events=[];target_reads=[];selector_active=False
        if attack:
            # Values come from the original reader below. Copy values, never
            # pointers, between the two EXISTING reader/action runtime owners.
            case['voice_native_selector']=True
            for off,value in zip((0x580,0x584,0x588,0x58C),spec['weapon_voice_slots']):
                u.mem_write(TYPE+off,dwords(value))
            u.mem_write(TYPE+0x24,spec['type_name'].encode()+b'\0')
            u.mem_write(TYPE+0x808,dwords(spec.get('turret_count',0)))
            u.mem_write(TYPE+0xCD5,b'\0')
            u.mem_write(TYPE+0x8B4,dwords(0))
            u.mem_write(WEAPON+0xA4,dwords(spec['damage']))
            u.mem_write(WEAPON+0x98,dwords(spec['ambient_damage']))
            if spec.get('missing_warhead'):u.mem_write(WEAPON+0xAC,dwords(0))
            u.mem_write(ACTOR+0x150,dwords(spec['raw_veterancy_bits']))
            # Native DeployFire is an explicit selector boundary for scalar
            # secondary/index-1 controls; ordinary stock rows remain undeployed.
            if spec.get('selector_boundary') is not None:
                u.mem_write(ACTOR+0x6E0,b'\1');u.mem_write(TYPE+0x6AC,b'\1')
                u.mem_write(TYPE+0x6A8,dwords(spec['selector_boundary']))
            if spec.get('null_weapon'):u.mem_write(TYPE+0x898,dwords(0))
            u.mem_write(SCRATCH+0x22000+0x230,dwords(spec['voice_ifv_repair']))
            if spec['target_type']=='GAPOWR':
                u.mem_write(TARGET,dwords(0x7E3EBC))
                u.mem_write(TARGET+0x520,dwords(TARGET_TYPE))
                u.mem_write(TARGET_TYPE,dwords(0x7E4570))
            u.mem_write(TARGET_TYPE+0x24,spec['target_type'].encode()+b'\0')
            def observe(machine,pc,*_):
                nonlocal selector_active
                if pc==0x746CD0:selector_active=True
                if pc==0x7090B3:selector_active=False
                if pc in (0x7090A0,0x746CD0,0x6F3330,0x717880,0x41BFB0,
                          0x7090B3,0x7090C0,0x7090E5,0x70918D,0x709193):
                    row=dict(entry=f'0x{pc:08X}')
                    if pc==0x7090B3:row['selected_index']=signed(machine.reg_read(UC_X86_REG_EAX))
                    if pc==0x7090C0:
                        slot=machine.reg_read(UC_X86_REG_EAX)
                        row['slot_is_null']=slot==0
                        if slot:
                            weapon=read(slot);row['weapon_is_null']=weapon==0
                            if weapon:row.update(damage=signed(read(weapon+0xA4)),ambient_damage=signed(read(weapon+0x98)))
                    events.append(row)
            def target_read(_u,_access,address,length,*_):
                if selector_active and (TARGET<=address<TARGET+0x900 or TARGET_TYPE<=address<TARGET_TYPE+0x1900):
                    target_reads.append([hex(address),length])
            observer=u.hook_add(UC_HOOK_CODE,observe)
            memory_observer=u.hook_add(UC_HOOK_MEM_READ,target_read)
        elif move:
            for offset,value in zip((0x558,0x55C,0x57C),spec['scalar_voice_slots']):
                u.mem_write(TYPE+offset,dwords(value))
            entries={0x6FFCBD:'CallerVoiceGate',0x708FC0:'VoiceMove',
                0x709020:'VoiceEnter',0x708DC0:'VoiceCapture',0x709060:'VoiceHarvest',
                0x65C780:'MainRandom',0x708D90:'QueueVoice',0x50B6F0:'HouseInputOwner',
                0x708DB5:'PendingStore'}
            def observe(machine,pc,*_):
                if pc not in entries:return
                row=dict(entry=f'0x{pc:08X}',name=entries[pc])
                if pc==0x708D90:
                    row['requested_voice']=signed(read(machine.reg_read(UC_X86_REG_ESP)+4))
                if pc==0x708DB5:row['stored_voice']=signed(machine.reg_read(UC_X86_REG_EDI))
                events.append(row)
            observer=u.hook_add(UC_HOOK_CODE,observe)
        u.mem_write(ACTOR+0x4F0,dwords(777))
        u.mem_write(ACTOR+0x21C,dwords(HOUSE if spec['actor_owner']=='local' else OTHER))
        u.mem_write(OTHER+0x1EC,bytes([spec.get('other_human',False)]))
        u.mem_write(OTHER+0x1ED,bytes([spec.get('other_player_control',False)]))
        u.mem_write(0xA8B238,dwords(spec.get('mode',1)))
        u.mem_write(0x822CF2,bytes([spec['voices_enabled']]))
        first_call=len(calls.calls)
        first_trace=len(result['trace'])
        # Non-planning ordinary input. Planning637AA0 runs unchanged; its
        # original false return selects6FFCBD. Stop before target/event codecs.
        entry=int(spec['caller_entry'],16)
        target=0 if spec.get('null_target') else TARGET
        args=((1,target,0,0) if attack else (2,0,TARGET,0)) if entry==0x6FFBE0 else ((target,) if attack else ())
        stop=0x6FFDA5 if entry==0x6FFBE0 else RET_MAGIC
        u.mem_write(SP,dwords(RET_MAGIC,*args))
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ACTOR)
        producer=None
        if entry==0x6FFBE0 and attack:
            # Original attack admission/argument producer, with the
            # Foot local-stack target slot supplied at this explicit boundary.
            u.mem_write(SP+0x110,dwords(target))
            u.reg_write(UC_X86_REG_ESI,ACTOR)
            run_checked(u,0x4D769F,entry,required_addresses=[0x4D76B1,0x4D7CB8],context={'name':spec['name']})
            sp=u.reg_read(UC_X86_REG_ESP)
            producer=dict(entry='0x004D769F',mission=read(sp+4),
                target_is_supplied_object=read(sp+8)==target,
                destination_is_null=read(sp+12)==0,planning_cell_is_null=read(sp+16)==0)
        if args and not attack:
            # Native Move producer after the declared spatial choice seam.
            # EDI=chosen Object / EAX=null planning Cell are supplied inputs;
            # the mission2 and NULL attack-target stack values are NOT supplied.
            u.mem_write(SP,dwords(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESI,ACTOR)
            u.reg_write(UC_X86_REG_EDI,TARGET)
            u.reg_write(UC_X86_REG_EAX,0)
            run_checked(u,0x4D7C76,entry,required_addresses=[0x4D7CB8])
            sp=u.reg_read(UC_X86_REG_ESP)
            producer=dict(entry='0x004D7C76',mission=read(sp+4),
                attack_target_is_null=read(sp+8)==0,destination_is_supplied_object=read(sp+12)==TARGET,
                planning_cell_is_null=read(sp+16)==0)
        fault=None
        try:
            run_checked(u,entry,stop,count=100000,required_addresses=[entry])
        except native_oracle.OracleError:
            if not attack or not spec.get('expect_null_slot_fault'):raise
            fault=dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),slot_is_null=u.reg_read(UC_X86_REG_EAX)==0)
            assert fault==dict(pc='0x7090c0',slot_is_null=True)
        if attack:
            u.hook_del(observer);u.hook_del(memory_observer)
        elif move:u.hook_del(observer)
        calls.returned(stop,u.reg_read(UC_X86_REG_ESP))
        if calls.pending:raise RuntimeError(f'Unreturned voice calls: {calls.pending}')
        after={name:bytes(u.mem_read(pointer,STRUCT_LEN)) for name,pointer in rngs.items()}
        observed=calls.calls[first_call:]
        assert all(int(row['ecx'],16)==rngs['main'] for row in observed if row['name']=='MainRandom')
        assert not any(row['name'] in ('RandomRanged','ActionTimerStart','Detach','ObjectDetach') for row in observed)
        raw=[row['return_eax'] for row in observed if row['name']=='MainRandom']
        reference,reference_after=draws(before['main'],len(raw))
        assert reference==raw and reference_after==after['main']
        assert all(before[name]==after[name] for name in ('scenario','mapgen'))
        output=dict(
            main_before_hex=before['main'].hex(),main_after_hex=after['main'].hex(),
            main_draws=raw,main_next_four=draws(after['main'],4)[0],
            scenario_unchanged=True,mapgen_unchanged=True,
            queued_voice_before=777,queued_voice_after=signed(read(ACTOR+0x4F0)),
            queue_requests=[signed(row['args'][0]) for row in observed if row['name']=='QueueVoice'],
            voice_move_calls=sum(row.get('name')=='VoiceMove' for row in result['trace'][first_trace:]),
            move_producer=producer,timer_start_calls=0,detach_calls=0,
            trace=result['trace'][first_trace:],stop=hex(stop))
        if attack:
            output.pop('voice_move_calls');output.pop('move_producer')
            output.update(events=events,selector_target_memory_reads=target_reads,
                voice_attack_calls=sum(row['entry']=='0x007090A0' for row in events),
                accepted_mission=producer['mission'] if producer else None,attack_producer=producer,
                scenario_before_hex=before['scenario'].hex(),scenario_after_hex=after['scenario'].hex(),
                mapgen_before_hex=before['mapgen'].hex(),mapgen_after_hex=after['mapgen'].hex(),
                fault=fault,stop=fault['pc'] if fault else hex(stop))
        elif move:
            output.update(events=events,
                voice_enter_calls=sum(row['name']=='VoiceEnter' for row in events),
                voice_capture_calls=sum(row['name']=='VoiceCapture' for row in events),
                voice_harvest_calls=sum(row['name']=='VoiceHarvest' for row in events),
                scenario_before_hex=before['scenario'].hex(),scenario_after_hex=after['scenario'].hex(),
                mapgen_before_hex=before['mapgen'].hex(),mapgen_after_hex=after['mapgen'].hex())
        return dict(voice_output=output)

    signed=lambda x:struct.unpack('<i',struct.pack('<I',x))[0]
    case=allied_case(spec['name'],**(dict(relation='enemy',actor_alliance_mask=0,target_alliance_mask=0) if attack else {}))
    result=run(case,continuation)
    return dict(name=spec['name'],input=spec,output=result['voice_output'])


def infantry_bomb_control(spec):
    """Original51EB24 consumer, with canonical base action explicitly supplied.

    No Infantry WhatAction wrapper or preceding weapon/ability result is
    substituted. A failed admission stops BEFORE the unrelated Infantry tail.
    """
    def continuation(u, read, ret, calls, result, receiver):
        u.mem_write(ACTOR,dwords(0x7EB058))  # actual Infantry vtable
        u.mem_write(TYPE,dwords(0x7EB610))  # original5237E8 InfantryType table
        u.mem_write(ACTOR+0x21C,dwords(HOUSE if spec['actor_owner']=='local' else OTHER))
        u.mem_write(TYPE+0x6C0,bytes([spec['attack_friendlies']]))
        u.mem_write(TYPE+0x6C1,bytes([spec['attack_cursor_on_friendlies']]))
        u.mem_write(TYPE+0xEAE,bytes([spec['ivan']]))
        u.mem_write(TARGET_TYPE+0x22E,bytes([spec['bombable']]))
        u.mem_write(TARGET+0x38,dwords(SCRATCH+0x2F000 if spec['has_bomb'] else 0))
        u.mem_write(0xA8B238,dwords(spec['mode']))
        native_calls=NativeCallTrace(u,read)
        first_call=len(calls.calls)
        visited=[]
        points={0x51EB24,0x51EB33,0x51EB38,0x51EB48,0x51EB5C,0x51EB66,0x51EB75}

        def observe(machine,pc,size,data):
            sp=u.reg_read(UC_X86_REG_ESP)
            native_calls.returned(pc,sp)
            if pc==0x50B6F0:native_calls.entered(pc,sp,('HouseControl',0,0))
            if pc in points:visited.append(hex(pc))

        observer=u.hook_add(UC_HOOK_CODE,observe)
        # Recreate the real51E3B0 prologue's saved-register/local stack shape.
        # The fragment's native epilogue pops4 registers, adds28h and RET8.
        u.mem_write(SP,dwords(0,0,0,0))
        u.mem_write(SP+0x38,dwords(RET_MAGIC,TARGET,0))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_EDI,ACTOR);u.reg_write(UC_X86_REG_ESI,TARGET)
        u.reg_write(UC_X86_REG_EBP,spec['base_action'])
        try:
            stop=run_checked(u,0x51EB24,(RET_MAGIC,0x51EB81),
                count=1000,required_addresses=[0x51EB24,0x50B6F0])
            native_calls.returned(stop,u.reg_read(UC_X86_REG_ESP))
            assert not native_calls.pending
            # The existing shared observer would record RNG/timer/detach
            # entries; none occurs in this body or its real getter/House calls.
            assert not calls.calls[first_call:]
        finally:u.hook_del(observer)
        returned=stop==RET_MAGIC
        if returned:assert u.reg_read(UC_X86_REG_ESP)==SP+0x44
        return dict(infantry_bomb_output=dict(entry='0x0051EB24',stop=hex(stop),
            boundary='returned' if returned else 'infantry_tail',
            action=u.reg_read(UC_X86_REG_EAX) if returned else None,
            base_action_at_tail=None if returned else u.reg_read(UC_X86_REG_EBP),
            house_control=bool(native_calls.calls[0]['return_eax']&255),
            visited=visited,main_draw_count=0,timer_start_calls=0,detach_calls=0))

    result=run(allied_case(spec['name']),continuation)
    return dict(name=spec['name'],input=spec,output=result['infantry_bomb_output'])


def generate_allied_objects():
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.projectile_oracle.bridge_render_inputs import lexical
    keys={'MTNK':('Primary','AttackFriendlies','AttackCursorOnFriendlies','LegalTarget','VoiceMove'),
          'CMIN':('Primary','Secondary','Harvester'),
          '105mm':('Warhead','Damage','TerrainFire'),'AP':('IvanBomb','Sonic')}
    retail_inputs=[]
    for filename,archive,raw in physical_layers():
        if raw is None:
            retail_inputs.append(dict(file=filename,absent=True))
            continue
        sections,_=lexical(raw,set(keys))
        retail_inputs.append(dict(file=filename,archive=archive,
            sha256=hashlib.sha256(raw).hexdigest(),
            selected_strings={name:{key:sections[name].get(key) for key in fields}
                              for name,fields in keys.items() if name in sections}))
    controls=[allied_case(name,**changes) for name,changes in (
        ('own_unselected',dict(relation='own')),
        ('own_selected',dict(relation='own',target_selected=True)),
        ('other_ally',{}),
        ('actor_only_ally',dict(target_alliance_mask=0)),
        ('target_only_ally',dict(actor_alliance_mask=0)),
        ('enemy',dict(actor_alliance_mask=0,target_alliance_mask=0)),
        ('same_index_other_pointer',dict(target_house_index=0,actor_alliance_mask=0,target_alliance_mask=0)),
        ('unowned_index',dict(target_house_index=-1,actor_alliance_mask=0,target_alliance_mask=0)),
        ('ctrl_ally',dict(ctrl=True)),
        ('ctrl_own',dict(relation='own',ctrl=True)),
        ('ctrl_enemy',dict(actor_alliance_mask=0,target_alliance_mask=0,ctrl=True)),
        ('shift_ally',dict(shift=True)),
        ('shift_own',dict(relation='own',shift=True)),
        ('alt_ally',dict(alt=True)),
        ('attack_friendlies',dict(attack_friendlies=True)),
        ('attack_cursor_friendlies',dict(attack_cursor_on_friendlies=True)),
        ('ivan_empty_keeps_cursor_grant',dict(attack_cursor_on_friendlies=True,ivan_bomb=True)),
        ('sonic_infested_ally',dict(sonic=True,infested=True)),
        ('sonic_uninfested_ally',dict(sonic=True)),
        ('ordinary_infested_ally',dict(infested=True)),
    )]
    base=dict(seed=1,voice_ids=[101],voices_enabled=True,actor_owner='local',
              mode=1,other_human=False,other_player_control=False,caller_entry='0x006FFBE0')
    voices=[dict(base,name=name,**changes) for name,changes in (
        ('move_singleton',{}),('move_empty',dict(voice_ids=[])),
        ('move_disabled_caller',dict(voices_enabled=False)),
        ('move_nonhuman_owner',dict(actor_owner='other')),
        ('move_campaign_human_owner',dict(actor_owner='other',mode=0,other_human=True)),
        ('move_campaign_player_control',dict(actor_owner='other',mode=0,other_player_control=True)),
        ('move_minus_one',dict(voice_ids=[-1])),
        ('move_multiple',dict(voice_ids=[101,202,303])),
        ('direct_disabled_still_draws',dict(caller_entry='0x00708FC0',voices_enabled=False)),
    )]
    return dict(schema=1,
        retail_inputs=retail_inputs,
        action_controls=[run(case,allied_continuation) for case in controls],
        unarmed_object_controls=[run(allied_case(
            f'unarmed_{"ctrl" if ctrl else "normal"}_{"selected" if selected else "unselected"}',
            cursor_contract=True,unarmed=True,actor_alliance_mask=0,target_alliance_mask=0,
            ctrl=ctrl,target_selected=selected),allied_continuation)
            for selected in (False,True) for ctrl in (False,True)],
        grant_controls=[run(allied_case(name,techno_only=True,**changes)) for name,changes in (
            ('ivan_transport_blocks_cursor_grant',dict(attack_cursor_on_friendlies=True,ivan_bomb=True,target_passengers=1)),
            ('attack_friendlies_overrides_ivan',dict(attack_friendlies=True,ivan_bomb=True,target_passengers=1)),
        )],
        voice_controls=[voice_control(spec) for spec in voices],
        infantry_bomb_controls=[infantry_bomb_control(dict(
            dict(base_action=5,attack_friendlies=True,attack_cursor_on_friendlies=False,
                 ivan=True,bombable=True,has_bomb=False,actor_owner='local',mode=1),
            name=name,**changes)) for name,changes in (
            ('attack_friendlies_only_ivan',{}),
            ('ivan_nonbombable',dict(bombable=False)),
            ('ivan_already_bombed',dict(has_bomb=True)),
            ('non_ivan_falls_through',dict(ivan=False)),
            ('nonhuman_ivan_falls_through',dict(actor_owner='other')),
            ('nonattack_base_falls_through',dict(base_action=2)),
        )])


def allied_metadata():
    return provenance(scope='Ordinary MTNK allied-object action/cursor/LeftUp boundaries, shared attack grants, Move voice admission and bounded Infantry Ivan consumer on the existing bunker action VM. Whole action functions and full Move/event/audio lifecycles remain partial.',
        entry_points={'determine_action':0x692610,'unit_object':0x73FD50,
            'foot_object':0x4DDED0,'techno_object':0x6FFEC0,'is_ally_by_object':0x4F9A90,
            'display_cursor':0x4AAE90,'left_up':0x4AB9B0,'left_up_dispatch':0x4AE750,
            'unit_clicked_object':0x738890,'foot_clicked_object':0x4D74E0,
            'navigation_coordinate_boundary':0x4D78BA,'move_argument_producer':0x4D7C76,'clicked_mission':0x6FFBE0,
            'order_voice_gate':0x6FFCBD,'voice_move':0x708FC0,'queue_voice':0x708D90,
            'house_player_control_query':0x50B6F0,'rng_raw':0x65C780,
            'infantry_bomb_consumer':0x51EB24,'infantry_bomb_tail_boundary':0x51EB81},
        assumptions=[
            'Original Unit/UnitType vtables and supplied live actor/type/House/weapon state reuse run(). This is not native constructor/scenario/INI execution. Ordinary actor/target represent stock MTNK: Selectable/LegalTarget=true, AttackFriendlies/AttackCursorOnFriendlies=false, positive weapon sign corresponding to105mm/AP; no disguise/transport/deploy/EMP/bunker/berserk. Other-grant and same-index/unowned controls are explicit input boundaries, not stock MTNK configuration/lifecycle claims.',
            'Four unarmed_object_controls reuse the same VM with native Primary+898=NULL, no infiltration ability, an enemy target and its selected byte false/true, with Ctrl false/true. Original IsArmed701120 executes; no action, cursor or release outcome is supplied. Existing damage query seam remains nonnegative, taking the same non-repair Unit arm as an empty weapon slot. Actual CMIN Primary=none is listed in physical layered strings and Rust tests read the production retail layers; these native rows supply valid type/actor state rather than claiming constructor/reader or full Chrono Miner lifecycle execution.',
            'Original ctor710AF0 stores AttackFriendlies+6C0=false at71117B and AttackCursorOnFriendlies+6C1=false at711181. Reader712170 admits through ObjectType5F92D0; selected current-default ReadBool5295F0 calls71523D and71525E store715242/715263 from exact keys843620/843604. LegalTarget current-default reader5F94AE/store5F94C2 retains constructor5F7168=true. No clamp. Physical RULESMD/LANGRULE-if-present/MPBattleMD/XMP03T4 have no MTNK overrides of these fields. Fixed ART is not layered and does not decide these action gates.',
            'MTNK physical selected keys Primary=105mm,105mm.Warhead=AP,VoiceMove=GenAllVehicleMove. Warhead IvanBomb+157 and Sonic+14B false for ordinary AP; explicit controls supply each branch and target Type.Passengers+5E0 / target Foot+694 pointer. They do not execute IvanBomb or parasite creation/cleanup. grant_controls execute complete Techno6FFEC0 only: the rejected passenger-capacity case would subsequently enter a separate Unit transport mechanism absent from this ordinary fixture. No transport verdict is substituted. Other special target/disguise/terrain/LegalTarget and arbitrary RTTI routes remain partial.',
            'House pointers, signed indices and directional+5788 alliance masks are declared inputs. Original4F9A90 executes target+3C=6F9DC0 real owner; same pointer/index or asking-House bit decides. Target+38=6F9DB0 returns real owner index for70058D..59B; no disguised-owner result is substituted.',
            'LeftUp4AB9B0 executes from entry to original selection/dispatch boundary. UnitClickedAction738890 re-queries native action and executes Foot4D74E0. NoMove2 reaches4D78BA before navigation coordinates, nearby-cell search or command enqueue; it can issue Move and is not a no-command result. Instruction4D7C76..7CBA emits mission2 with the chosen destination. No destination or movement/RNG result is invented.',
            'Voice controls execute in that same VM, with supplied list+468 and fixture-relative sound IDs; MTNK name binding is physical but original type/sound readers and full catalog IDs are outside this bounded action fixture. Reuse shared gen_rng_vectors.seeded_struct/draws for original seed/continuation. Three RNG states have equal supplied native seed. The original Foot Move producer4D7C76..7CBA runs with declared ESI=actor, EDI=chosen target Object, EAX=null planning Cell after the spatial choice seam. It produces mission2/NULL attack target and enters complete6FFBE0 prefix; nonplanning637AA0 executes unchanged and control stops6FFDA5 before target/event codecs. This component does not assert that the bounded Foot receiver produced that destination.',
            'VoiceMove708FC0 consumes one actual Main65C780 draw for every nonempty list including singleton/-1. Original QueueVoice708D90 then tests global enable, selected ID!=-1 and real House50B6F0 before storing+4F0. QueueMegaMission6FFBE0 tests enable BEFORE calling VoiceMove, so disabled caller spends zero. Direct disabled VoiceMove is a separate ordering control. Scenario and MapGen remain byte-identical. No OS audio playback claim.',
            'infantry_bomb_controls reuse the same action VM, then supply actual Infantry7EB058/InfantryType7EB610 vtables, EDI=actor, ESI=ordinary target and EBP=canonical base action at original51EB24. The real51E3B0 saved-register/local stack shape supports its unchanged RET8 epilogue. Native House50B6F0, Ivan Type+EAE, target+88 ObjectType getter, Bombable+22E and attachedBomb+38 execute without substituted virtual answers. BaseAction5/localIvan/Bombable/noBomb returns53; nonbombable or bombed returns54. Rejected House/Ivan/base-action admission stops BEFORE51EB81 and records final action=null plus unchanged base action: no final Infantry-tail outcome is invented. Whole Infantry51E3B0 prefix/ability/transport/lifecycle and scalar native reader execution remain excluded. AttackFriendlies=true/AttackCursorOnFriendlies=false are declared canonical-admission priors; this consumer does not read those grant bytes again. No RNG, action-timer or detach entry occurs.',
            'Action/cursor/LeftUp-prefix/receiver controls reach no RNG draw, timer write or detach. Full LeftUp dispatch later clears822CF2 after every actor/restores1 (4AE852/85F), restarts action-line timer70D150 to25 at4ABFAE, and dispatches through existing Move/event/voice owners. These later effects are instruction evidence, not a whole-window execution claim.'],
        substitutions=[
            'Reuse unchanged historical run() substitutions: singleton SelectBest, supplied keyboard, revealed flat Cell/map/height/playfield/terrain, weapon slot/damage, range and FireError. Original House alliance/owner queries, complete DetermineAction/class action bodies, Display cursor switch, LeftUp prefix, UnitClickedAction/Foot prefix, VoiceMove/RNG/QueueVoice/House admission execute; no native instructions are patched.',
            'The mock Display virtual48 remains an observed cursor setter sink with palette/empty-waypoint storage. UI cursor SHP/glyph raster is outside the fixture. Original text hash is checked before/after every shared-VM continuation.'])


def order_voice_reader_inputs(wanted_names):
    """One shared original type/Rules/Sound bootstrap for order-voice readers."""
    import os
    from tools.projectile_oracle.bridge_render_inputs import BulletReader
    from tools.spatial_oracle.anytown_damage.mtnk_attack import sound_inputs
    root=Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    m=BulletReader({},root);u=m.u
    initialization_events=[]
    def initialization_observer(_u,pc,*_):
        if pc in (0x65C780,0x65C7E0,0x70D150,0x7258D0,0x5F5280):
            initialization_events.append(dict(entry=f'0x{pc:08X}',ecx=hex(u.reg_read(UC_X86_REG_ECX))))
    initialization_hook=u.hook_add(UC_HOOK_CODE,initialization_observer)
    sound=sound_inputs(m,root,{},wanted_names=wanted_names)
    for registry in (0xA8EB00,0x887568):
        u.mem_write(registry,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    rules=m.alloc(0x5000);m.invoke(0x665650,rules)
    u.mem_write(0x8871E0,dwords(rules))
    typ=m.alloc(0x1900);m.invoke(0x7470D0,typ,(m.cstring('MTNK'),))
    return m,sound,rules,typ,initialization_events,initialization_hook


def attack_voice_readers():
    """Selected unchanged constructor/read blocks in the EXISTING INI owner.

    SoundList source links, Voc construction/name lookup and sample-name
    transport belong to Sound/sound_inputs; no cache or binder is copied here.
    """
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.building_body_rules import RULES, SP as reader_sp
    from tools.procedural_drawing_oracle.shroud import _status_reader_block
    signed=lambda x:struct.unpack('<i',dwords(x))[0]
    m,sound,rules,typ,initialization_events,initialization_hook=order_voice_reader_inputs({
        'GenAllVehicleAttackCommand','GenAllVehicleMove','IFVMove'})
    u=m.u
    weapon=m.alloc(0x200);m.invoke(0x771C70,weapon,(m.cstring('105mm'),))
    repair=m.alloc(0x200);m.invoke(0x771C70,repair,(m.cstring('RepairBullet'),))
    keys=('VoiceAttack','VoicePrimaryWeaponAttack','VoicePrimaryEliteWeaponAttack',
          'VoiceSecondaryWeaponAttack','VoiceSecondaryEliteWeaponAttack',
          'Primary','Secondary','TurretCount','WeaponCount','IsGattling')
    def fields():
        count=m.read32(typ+0x494);ptr=m.read32(typ+0x488)
        primary=m.read32(typ+0x898);secondary=m.read32(typ+0x8B4)
        return dict(primary=m.string(primary+0x24) if primary else None,
            secondary=m.string(secondary+0x24) if secondary else None,
            turret_count=signed(m.read32(typ+0x808)),
            is_gattling=bool(u.mem_read(typ+0xCD5,1)[0]),
            voice_attack=[signed(m.read32(ptr+i*4)) for i in range(count)],
            weapon_voice_slots=[signed(m.read32(typ+off)) for off in (0x580,0x584,0x588,0x58C)],
            damage=signed(m.read32(weapon+0xA4)),ambient_damage=signed(m.read32(weapon+0x98)))
    def read_type(*,all_fields=False):
        registers={UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24,UC_X86_REG_ESI:RULES}
        if all_fields:
            for begin,end in ((0x71284A,0x712898),(0x714016,0x714030),(0x7129AB,0x712A1D)):
                _status_reader_block(m,begin,end,registers)
        _status_reader_block(m,0x712C5B,0x712CC5,registers)
        _status_reader_block(m,0x713679,0x713775,registers|{UC_X86_REG_EAX:m.read32(typ+0x57C)})
    def read_ifv():
        _status_reader_block(m,0x669D63,0x669DA7,{
            UC_X86_REG_ESI:rules,UC_X86_REG_EDI:RULES,
            UC_X86_REG_ECX:reader_sp+0x14,UC_X86_REG_EAX:m.read32(rules+0x22C)})
    def read_damage(pointer):
        for begin,end in ((0x7722D4,0x7722EE),(0x7720B2,0x7720CC)):
            _status_reader_block(m,begin,end,{
                UC_X86_REG_ESI:pointer,UC_X86_REG_EBX:pointer+0x24,UC_X86_REG_EDI:RULES})
    observed=[]
    def observe(_u,pc,*_):
        if pc in (0x528A10,0x5276D0):
            sp=u.reg_read(UC_X86_REG_ESP)
            observed.append(dict(reader=f'0x{pc:08X}',section=m.string(m.read32(sp+4)),
                key=m.string(m.read32(sp+8)),
                default=signed(m.read32(sp+12)) if pc==0x5276D0 else m.string(m.read32(sp+12))))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    constructor=fields();ifv_constructor=signed(m.read32(rules+0x230))
    layers=[]
    for name,archive,raw in physical_layers():
        if raw is None:
            layers.append(dict(file=name,archive=archive,absent=True));continue
        sections,_=lexical(raw,{'MTNK','105mm','FV','RepairBullet','AudioVisual'})
        m.rules_cache(sections);mark=len(observed)
        admitted=bool(m.invoke(0x526810,RULES,(typ+0x24,)))
        if admitted:read_type(all_fields=True)
        weapon_admitted=bool(m.invoke(0x526810,RULES,(weapon+0x24,)))
        if weapon_admitted:read_damage(weapon)
        repair_admitted=bool(m.invoke(0x526810,RULES,(repair+0x24,)))
        if repair_admitted:read_damage(repair)
        audiovisual_admitted=bool(m.invoke(0x526810,RULES,(m.cstring('AudioVisual'),)))
        if audiovisual_admitted:read_ifv()
        selected={s:{k:v for k,v in values.items() if k in keys+('Damage','AmbientDamage','VoiceIFVRepair')}
                  for s,values in sections.items()}
        layers.append(dict(file=name,archive=archive,sha256=hashlib.sha256(raw).hexdigest(),
            section_admitted=admitted,weapon_section_admitted=weapon_admitted,
            audiovisual_section_admitted=audiovisual_admitted,selected=selected,
            reads=observed[mark:],after=fields(),voice_ifv_repair=signed(m.read32(rules+0x230)),
            repair_damage=signed(m.read32(repair+0xA4)),repair_ambient_damage=signed(m.read32(repair+0x98))))
    stock=fields();stock_ifv=signed(m.read32(rules+0x230))
    controls=[]
    # Retained state is produced by earlier ORIGINAL reads, including unknown,
    # empty and absent values. No scalar/list golden is calculated in Python.
    names=[row['name'] for row in sound['rows']]
    for name,sections in (
        ('multiple_and_four_overrides',{'MTNK':dict(VoiceAttack=','.join(names),**dict(zip(keys[1:5],names+names[:1])))}),
        ('absent_section',{}),('unrelated_admitted_section',{'MTNK':{'Strength':'300'}}),
        ('empty_values',{'MTNK':dict.fromkeys(keys[:5],'')}),
        ('mixed_unknown_and_duplicates',{'MTNK':{'VoiceAttack':'GenAllVehicleMove,NOT_A_RETAIL_SOUND,GenAllVehicleMove,IFVMove'}}),
        ('unresolved_values',{'MTNK':dict.fromkeys(keys[:5],'NOT_A_RETAIL_SOUND')}),
        ('comma_only_list',{'MTNK':{'VoiceAttack':',,,'}}),
        ('restore_stock_list',{'MTNK':{'VoiceAttack':'GenAllVehicleAttackCommand'}}),
        ('wrong_case_keys',{'MTNK':{'voiceattack':'IFVMove','voiceprimaryweaponattack':'IFVMove'}}),
    ):
        before=fields();m.rules_cache(sections);mark=len(observed)
        admitted=bool(m.invoke(0x526810,RULES,(typ+0x24,)))
        if admitted:read_type()
        controls.append(dict(name=name,input=dict(sections=sections),admitted=admitted,
                             before=before,after=fields(),reads=observed[mark:]))
    ifv_controls=[]
    for name,sections in (
        ('valid',{'AudioVisual':{'VoiceIFVRepair':'IFVMove'}}),
        ('absent_section',{}),('unrelated_section',{'AudioVisual':{'Gravity':'3'}}),
        ('empty',{'AudioVisual':{'VoiceIFVRepair':''}}),
        ('unresolved',{'AudioVisual':{'VoiceIFVRepair':'NOT_A_RETAIL_SOUND'}}),
    ):
        before=signed(m.read32(rules+0x230));m.rules_cache(sections);mark=len(observed)
        admitted=bool(m.invoke(0x526810,RULES,(m.cstring('AudioVisual'),)))
        if admitted:read_ifv()
        ifv_controls.append(dict(name=name,sections=sections,admitted=admitted,before=before,
            after=signed(m.read32(rules+0x230)),reads=observed[mark:]))
    u.hook_del(hook)
    u.hook_del(initialization_hook)
    return dict(constructor=constructor,layers=layers,sound=sound,result=stock,
        initialization_events=initialization_events,
        retained_controls=controls,voice_ifv_repair=dict(constructor=ifv_constructor,
            result=stock_ifv,controls=ifv_controls,
            repair_damage=layers[0]['repair_damage'],repair_ambient_damage=layers[0]['repair_ambient_damage']))


def generate_attack_order_voice():
    readers=attack_voice_readers();facts=readers['result'];ifv=readers['voice_ifv_repair']
    slots=readers['retained_controls'][0]['after']['weapon_voice_slots']
    base=dict(seed=1,voices_enabled=True,actor_owner='local',mode=1,
        other_human=False,other_player_control=False,caller_entry='0x006FFBE0',
        type_name='MTNK',target_type='MTNK',voice_ids=facts['voice_attack'],
        weapon_voice_slots=facts['weapon_voice_slots'],damage=facts['damage'],
        ambient_damage=facts['ambient_damage'],raw_veterancy_bits=0,
        voice_ifv_repair=ifv['result'])
    voices=[dict(base,name=name,**changes) for name,changes in (
        ('stock_mtnk_target',{}),('stock_gapowr_target',dict(target_type='GAPOWR')),
        ('disabled_caller',dict(voices_enabled=False)),
        ('direct_disabled_after_draw',dict(caller_entry='0x007090A0',voices_enabled=False)),
        ('nonhuman_after_draw',dict(actor_owner='other',caller_entry='0x007090A0')),
        ('empty_list',dict(voice_ids=[])),
        ('multiple_list',dict(voice_ids=readers['retained_controls'][0]['after']['voice_attack'])),
        ('minus_one_list',dict(voice_ids=[-1])),
        ('null_target_primary_only',dict(caller_entry='0x007090A0',null_target=True)),
        ('null_primary_weapon',dict(caller_entry='0x007090A0',null_weapon=True)),
        ('null_slot_index_minus_one',dict(caller_entry='0x007090A0',selector_boundary=-1,expect_null_slot_fault=True)),
        ('fv_negative_damage',dict(type_name='FV',damage=ifv['repair_damage'],ambient_damage=ifv['repair_ambient_damage'])),
        ('fv_negative_unbound_repair_voice',dict(type_name='FV',damage=ifv['repair_damage'],voice_ifv_repair=ifv['constructor'])),
        ('fv_negative_unbound_bypasses_weapon_overrides',dict(type_name='FV',damage=ifv['repair_damage'],voice_ifv_repair=ifv['constructor'],weapon_voice_slots=slots)),
        ('non_fv_negative_damage',dict(damage=ifv['repair_damage'])),
        ('fv_negative_missing_warhead',dict(type_name='FV',damage=ifv['repair_damage'],missing_warhead=True)),
        ('non_fv_negative_missing_warhead',dict(damage=ifv['repair_damage'],missing_warhead=True)),
    )]
    for index,name in enumerate(('primary','primary_elite','secondary','secondary_elite')):
        voices.append(dict(base,name=name+'_override',weapon_voice_slots=slots,
            raw_veterancy_bits=0x40000000 if index%2 else 0,
            **(dict(selector_boundary=1) if index>=2 else {})))
    return dict(schema=1,readers=readers,
                voice_controls=[voice_control(spec,attack=True) for spec in voices])


def move_voice_readers():
    """Original retained Move vector and Enter/Capture/Harvest bound scalars."""
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.weapon_laser import physical_layers
    from tools.spatial_oracle.building_body_rules import RULES
    from tools.procedural_drawing_oracle.shroud import _status_reader_block
    m,sound,_rules,typ,initialization_events,initialization_hook=order_voice_reader_inputs({
        'GenAllVehicleMove','IFVMove'})
    u=m.u
    signed=lambda value:struct.unpack('<i',dwords(value))[0]
    scalar_offsets=dict(VoiceEnter=0x558,VoiceCapture=0x55C,VoiceHarvest=0x57C)
    keys=('VoiceMove',*scalar_offsets)

    def fields():
        ptr,count=m.read32(typ+0x46C),m.read32(typ+0x478)
        return dict(voice_move=[signed(m.read32(ptr+i*4)) for i in range(count)],
            voice_enter=signed(m.read32(typ+0x558)),
            voice_capture=signed(m.read32(typ+0x55C)),
            voice_harvest=signed(m.read32(typ+0x57C)))

    def read_type():
        registers={UC_X86_REG_EBP:typ,UC_X86_REG_EBX:typ+0x24,UC_X86_REG_ESI:RULES}
        _status_reader_block(m,0x712AB3,0x712B1D,registers)
        # Adjacent scalar reads delay their preceding stores. Preserve those
        # native current values; each complete block has a balanced stack.
        _status_reader_block(m,0x713091,0x71310F,
            registers|{UC_X86_REG_EAX:m.read32(typ+0x554)})
        _status_reader_block(m,0x71363D,0x7136C1,
            registers|{UC_X86_REG_EAX:m.read32(typ+0x578)})

    observed=[]
    def observe(_u,pc,*_):
        if pc!=0x528A10:return
        sp=u.reg_read(UC_X86_REG_ESP)
        key=m.string(m.read32(sp+8))
        row=dict(reader='0x00528A10',section=m.string(m.read32(sp+4)),key=key,
            default_string=m.string(m.read32(sp+12)),capacity=m.read32(sp+20))
        if key in scalar_offsets:row['current_id']=signed(m.read32(typ+scalar_offsets[key]))
        if key=='VoiceMove':row['current_list']=fields()['voice_move']
        observed.append(row)
    hook=u.hook_add(UC_HOOK_CODE,observe)
    constructor=fields()
    layers=[]
    for name,archive,raw in physical_layers():
        if raw is None:
            layers.append(dict(file=name,archive=archive,absent=True));continue
        sections,_=lexical(raw,{'MTNK'});m.rules_cache(sections);mark=len(observed)
        admitted=bool(m.invoke(0x526810,RULES,(typ+0x24,)))
        if admitted:read_type()
        layers.append(dict(file=name,archive=archive,sha256=hashlib.sha256(raw).hexdigest(),
            section_admitted=admitted,
            selected={'MTNK':{key:value for key,value in sections.get('MTNK',{}).items() if key in keys}},
            reads=observed[mark:],after=fields()))
    stock=fields()
    controls=[]
    for name,values in (
        ('unknown_with_unbound_scalars',{'VoiceMove':'NOT_A_RETAIL_SOUND'}),
        ('two_entries',{'VoiceMove':'GenAllVehicleMove,IFVMove'}),
        ('enter_only',{'VoiceEnter':'IFVMove'}),
        ('absent_section',None),('admitted_missing',{'Strength':'300'}),
        ('empty_values',dict.fromkeys(keys,'')),
        ('mixed_case_duplicates_and_scalars',dict(VoiceMove='gEnAllVehicleMove,IFVMove,GenAllVehicleMove',
            VoiceEnter='iFvMoVe',VoiceCapture='gEnAllVehicleMove',VoiceHarvest='IFVMove')),
        ('unknown_values',dict.fromkeys(keys,'NOT_A_RETAIL_SOUND')),
        ('unknown_first',{'VoiceMove':'NOT_A_RETAIL_SOUND,GenAllVehicleMove'}),
        ('wrong_case_keys',{'voicemove':'IFVMove','voiceenter':'GenAllVehicleMove',
                            'voicecapture':'IFVMove','voiceharvest':'GenAllVehicleMove'}),
        ('scalar_list_tokens',dict.fromkeys(scalar_offsets,'GenAllVehicleMove,IFVMove')),
        ('scalar_none_tokens',dict.fromkeys(scalar_offsets,'<none>')),
        ('comma_only_move',{'VoiceMove':',,,'}),
        ('127_byte_complete_token',{'VoiceMove':'x'*119+',IFVMove'}),
        ('128_byte_cut_token',{'VoiceMove':'x'*120+',IFVMove'}),
        ('127_byte_cut_later_token',{'VoiceMove':'GenAllVehicleMove,'+'x'*109+',IFVMove'}),
        ('restore_stock_move',{'VoiceMove':'GenAllVehicleMove'}),
    ):
        sections={} if values is None else {'MTNK':values}
        before=fields();m.rules_cache(sections);mark=len(observed)
        admitted=bool(m.invoke(0x526810,RULES,(typ+0x24,)))
        if admitted:read_type()
        controls.append(dict(name=name,input=dict(sections=sections),admitted=admitted,
                             before=before,after=fields(),reads=observed[mark:]))
    u.hook_del(hook);u.hook_del(initialization_hook)
    return dict(constructor=constructor,layers=layers,sound=sound,result=stock,
                initialization_events=initialization_events,retained_controls=controls)


def generate_move_order_voice():
    readers=move_voice_readers()
    controls={row['name']:row['after'] for row in readers['retained_controls']}
    states=dict(controls,stock=readers['result'])
    def spec(name,reader_case,**changes):
        fields=states[reader_case]
        return dict(name=name,seed=1,voices_enabled=True,actor_owner='local',mode=1,
            other_human=False,other_player_control=False,caller_entry='0x006FFBE0',
            reader_case=reader_case,
            voice_ids=fields['voice_move'],scalar_voice_slots=[fields[key] for key in
                ('voice_enter','voice_capture','voice_harvest')])|changes
    two='two_entries';bound='mixed_case_duplicates_and_scalars'
    voices=[spec('stock_caller','stock'),spec('two_entries_caller',two),
        spec('unknown_first_caller','unknown_first'),
        spec('unknown_only_caller','unknown_values'),
        spec('retained_after_empty_caller','empty_values'),
        spec('duplicates_caller',bound),
        spec('disabled_caller','stock',voices_enabled=False),
        spec('direct_move_disabled',two,caller_entry='0x00708FC0',voices_enabled=False),
        spec('direct_move_nonhuman',two,caller_entry='0x00708FC0',actor_owner='other'),
        spec('enter_scalar',bound,caller_entry='0x00709020'),
        spec('enter_move_fallback',two,caller_entry='0x00709020'),
        spec('capture_scalar',bound,caller_entry='0x00708DC0'),
        spec('capture_enter_fallback','enter_only',caller_entry='0x00708DC0'),
        spec('capture_move_fallback',two,caller_entry='0x00708DC0'),
        spec('harvest_scalar',bound,caller_entry='0x00709060'),
        spec('harvest_move_fallback',two,caller_entry='0x00709060'),
        spec('capture_empty_fallback','unknown_with_unbound_scalars',caller_entry='0x00708DC0'),
        spec('scalar_direct_nonhuman',bound,caller_entry='0x00709020',actor_owner='other')]
    return dict(schema=1,readers=readers,
                voice_controls=[voice_control(case,move=True) for case in voices])


def move_voice_metadata():
    return provenance(scope='Retained fixed-catalog MTNK VoiceMove list and required Enter/Capture/Harvest scalar bindings, complete reached voice bodies and native Main/QueueVoice order; bounded input/caller priors.',
        entry_points=dict(unit_type_ctor=0x7470D0,techno_type_ctor=0x710AF0,
            voice_move_reader=0x712AB3,voice_enter_reader=0x713091,
            voice_capture_reader=0x7130D3,voice_harvest_reader=0x71363D,
            read_sound_list=0x525430,read_string=0x528A10,
            sound_types=0x7510D0,sound_lookup=0x7514D0,
            move_argument_producer=0x4D7C76,accepted_order=0x6FFBE0,
            voice_enable_gate=0x6FFCBD,voice_move=0x708FC0,voice_enter=0x709020,
            voice_capture=0x708DC0,voice_harvest=0x709060,
            queue_voice=0x708D90,house_admission=0x50B6F0,main_raw=0x65C780),
        assumptions=[
            'The existing shared order-voice reader bootstrap owns BulletReader, Rules665650 and UnitType7470D0 construction, fixed physical SOUNDMD through Sound/sound_inputs and original lookup. Move vector+468/count+478 defaults empty at710DFD/710E03. Enter+558/Capture+55C/Harvest+57C default-1 at710F19/710F1F/710F4F, with EBP=-1 from710CED.',
            'Selected original reader blocks execute at712AB3..712B1D,713091..71310F and71363D..7136C1 after original526810 section admission. Adjacent delayed scalar stores retain their actual current values. Whole type/file readers are excluded. Actual RULESMD/LANGRULE/MPBattleMD/XMP03T4 layers come from physical_layers; SOUNDMD is fixed and ART is irrelevant to these decisions.',
            'Native ReadString capacity128 bounds tokens to127bytes. Missing/empty list reads retain the current vector; a nonempty unresolved/comma-only list can replace it with empty. Order/duplicates and case behavior are original reader outputs. Scalar names resolve through7514D0 and retain the current signed ID for missing/empty/unresolved inputs; they are not raw unbound strings.',
            'Native reader values transfer by value into the existing run/voice_control action VM, never as transplanted pointers. Full Move708FC0, Enter709020, Capture708DC0, Harvest709060, raw Main65C780 and QueueVoice708D90/House50B6F0 execute unchanged. Capture falls to Enter then Move; Harvest falls to Move. Bound scalars spend no raw draw; every nonempty reached Move list including singleton spends one before QueueVoice admission.',
            'Ordinary Move controls execute original Foot4D7C76 argument production with declared selected Object/NULL planning Cell, then mission2/NULL attack target through6FFBE0. Stop6FFDA5 precedes event encoding. Fallback and disabled/nonhuman direct controls enter the named voice body explicitly; they do not establish complete Enter/Capture/Harvest gameplay actions or ordinary nonhuman input admission.',
            'Existing canonical seeded_struct/draws owns seed and native four-word continuation. All3complete RNG states are retained; Scenario/MapGen stay unchanged. There are no measured timer-start or detach calls in the voice continuation. The later ordinary input action-line timer remains outside it.'],
        substitutions=[
            'Historical run owns the sole action VM and its supplied selection/keyboard/map/Cell/height/shroud/playfield/terrain/range/FireError and cursor-sink boundaries. Actor/type/House state, pending777 and initial RNG seed are consumer priors, not whole scenario construction or movement/path/event scheduling.',
            'Sound owns source-order cache/registry/binding. AudioIndex names map to local selected-subset indices; numeric sound IDs are fixture-relative. No audio service/sample RNG, PCM, hardware or complete playback claim. Native instructions/vtables are unchanged and original text is checked before/after each action continuation.'])


def attack_voice_metadata():
    return provenance(scope='Ordinary selected MTNK Attack acknowledgement: native retained type/sound readers, accepted mission1 prefix, unchanged primary-only selector, complete VoiceAttack and QueueVoice; bounded shared scalar/FV/absence controls.',
        entry_points=dict(unit_type_ctor=0x7470D0,weapon_type_ctor=0x771C70,rules_ctor=0x665650,
            voice_list_reader=0x712C5B,weapon_voice_readers=0x713679,
            voice_ifv_repair_reader=0x669D63,sound_types=0x7510D0,sound_lookup=0x7514D0,
            accepted_order=0x6FFBE0,voice_enable_gate=0x6FFCBD,
            unit_select_weapon=0x746CD0,techno_select_weapon=0x6F3330,get_weapon=0x70E140,
            voice_attack=0x7090A0,queue_voice=0x708D90,main_raw=0x65C780),
        assumptions=[
            'Selected original per-layer constructor/read blocks use the existing BulletReader runtime/cache. Full UnitType7470D0, WeaponType771C70 and Rules665650 constructors execute. Original section-count admission526810 guards selected reads; whole type/file readers are excluded. Physical RULESMD then optional LANGRULE then MPBattleMD then XMP03T4 are selected by existing weapon_laser.physical_layers. Fixed SOUNDMD is not layered. ART has no effect on these voice decisions.',
            'VoiceAttack vector+484/default empty reads at712C5B..712CC5. Four signed scalar slots580/584/588/58C default-1 at710F55..710F68, current-default named Voc reads713679..713775. Rules+230 defaults-1 at665976, exact AudioVisual VoiceIFVRepair reader669D63..669DA7; selected physical IFVMove binds through the same Sound owner. SoundList keys and sample names are physical; registry/sample numeric IDs are fixture-relative, excluding complete catalog/audio device/PCM.',
            'Existing run() is the sole action/Display VM. Actual reader values are copied by value into its supplied live Unit/type/House prior, not transplanted pointers. Action/Display initialization retains its historical environmental/damage/range seams; only the measured voice continuation releases the existing selector seam. No whole Unit constructor, target creation, complete mission/event encoding or playback claim.',
            'Ordinary undeployed MTNK uses original746CD0->6F3330, stock TurretCount0/IsGattlingfalse, original41BFB0 false and SecondaryNULL; it returns primary0 before any target reads. Target labels MTNK/GAPOWR are explicit object/vtable priors, not full constructors/target-reader claims. Actor raw veterancy and pending777 are supplied consumer state. Scalar-secondary controls use original DeployFire index prior explicitly; they do not claim stock MTNK secondary/deploy lifecycle.',
            'Original Foot4D769F attack admission/argument producer executes with ESI=actor and the existing local stack target slot supplied. It produces mission1/target/NULL destination/planning and enters6FFBE0. This is not the complete Unit/Foot ClickedAction wrapper. Original637AA0 planning query/enable gate run; stop6FFDA5 precedes event encoding. Nonhuman rejection enters direct7090A0: ordinary Foot command admission would reject it. Direct controls separate enable-before-call from reached QueueVoice rejection. Whole7090A0, native GetWeapon/rawelite/IsTypeFV, Main65C780, QueueVoice/House50B6F0 execute. Weapon Damage is+A4; AmbientDamage is+98. Negative Damage literalFV selects Rules+230 without a Main draw when bound. An unbound repair ID bypasses all four valid weapon overrides and falls to the generic list/Main at7090EB->70914A. Its type-name/weapon/mode is a consumer boundary, not full IFV repair/transport lifecycle. Missing-warhead controls show voice does not require resolving a Warhead.',
            'NULL target with primary-only selector and slot.weaponNULL are distinct from selected index-1: original GetWeapon returnsNULL slot and7090C0 faults before queue/RNG. The explicit fault control stops on that native dereference; no fallback is invented. Shared seed/continuation owner records complete Main states/raw/next4; Scenario/MapGen unchanged. No measured action-timer start or detach call. Later LeftUp action-line timer/voice-enable changes remain existing input owner effects.'],
        substitutions=[
            'Reuse historical singleton selection, keyboard/map/height/shroud/playfield/terrain/range/FireError and mock Display cursor sink. Measured primary-only SelectWeapon and all voice/RNG/queue bodies have no return substitution. Native instruction bytes/vtables remain immutable; original text hash checked before/after continuations.',
            'Reuse BulletReader, Sound.make_ini and mtnk_attack.sound_inputs for selected physical SoundList/Defaults/type/source-order cache and original name binding. AudioIndex sample-name resolution uses their existing local-index transport. No second VM/cache/parser/RNG/voice binder implementation is introduced.'])


NO_MANUAL_FIRE_HOOKS={0x700536:0x1004A460,0x7008D4:0x1004A430}


def no_manual_fire_readers(ares, gamemd):
    """Reuse the enrolled Ares VM for constructor and exact field block."""
    from tools.rules_oracle.ares_prerequisites import Probe
    Probe.input_bytes=ares,gamemd
    p=Probe();ext,typ=p.alloc(0x1000),p.alloc(0x2000)
    p.write(typ,0x7F6218);p.u.mem_write(typ+0x24,b'CONTROL\0')
    p.u.mem_write(ext+0x4C4,b'\xCC'*4)
    code=bytes(p.u.mem_read(0x10001000,0x92000))
    returned=p.call(0x1003C710,typ,ecx=ext)
    constructor=dict(entry='1003C710',initial_bytes='CCCCCCCC',
        after_bytes=bytes(p.u.mem_read(ext+0x4C4,4)).hex().upper(),
        no_manual_fire=p.u.mem_read(ext+0x4C6,1)[0],returned_self=returned==ext)
    section=p.text('CONTROL');sp=0x2000F000;rows=[]
    for prior,value in [(0,None),(1,None),(1,''),(0,'yes'),(1,'no'),
            (0,'true'),(1,'false'),(0,'1'),(1,'0'),(1,'junk'),
            (0,'yeSplease'),(0,'y'),(0,'t'),(1,'n'),(1,'f'),
            (0,'on'),(1,'off'),(0,' yes '),(1,' no '),(0,'YES'),
            (1,'FALSE'),(0,' true\t'),(1,'00'),(0,'11')]:
        p.events.clear();p.reads={('CONTROL','NoManualFire'):value or ''}
        p.u.mem_write(ext+0x4C6,bytes([prior]));p.write(sp+0x10,0)
        p.u.reg_write(UC_X86_REG_ESP,sp);p.u.reg_write(UC_X86_REG_EBX,ext)
        p.u.reg_write(UC_X86_REG_ESI,section)
        run_checked(p.u,0x1003F002,0x1003F018,
            required_addresses=(0x1000EBF0,0x1000FC50,0x10007860,0x528A10))
        shim,actor=p.alloc(0x40),p.alloc(0x1000)
        p.write(shim+0x0C,actor);p.write(actor,UNIT_VT);p.write(actor+0x6C4,typ)
        p.write(typ+0x2FC,ext)
        rows.append(dict(prior=prior,value=value,after=p.u.mem_read(ext+0x4C6,1)[0],
            reader_events=list(p.events),object_hook_return=p.call(0x1004A460,shim),
            cell_hook_return=p.call(0x1004A430,shim)))
    assert bytes(p.u.mem_read(0x10001000,0x92000))==code
    return dict(constructor=constructor,controls=rows)


def no_manual_fire_physical(ares):
    """Actual physical INI loading and ReadString, on the existing Rules VM."""
    from tools.rules_oracle.weapon_speed_order import retail_history
    from tools.native_oracle import map_pe32_image
    layers=[('RULESMD.INI','[CONTROL]\nNoManualFire=yes\n'),
        ('LANGRULE.INI','[CONTROL]\nName=present\n'),
        ('MODE.INI','[CONTROL]\nName=present\nNoManualFire=\n'),
        ('MAP.MAP','[CONTROL]\nNoManualFire=junk\n'),
        ('RESET.INI','[CONTROL]\nNoManualFire=no\n'),
        ('WRONG.INI','[OTHER]\nNoManualFire=yes\n'),
        ('KEYCASE.INI','[CONTROL]\nNOMANUALFIRE=yes\n'),
        ('TRIM.INI','[CONTROL]\nNoManualFire= \tyeSplease \t\n'),
        ('SECOND.INI','[CONTROL]\nNoManualFire= \tno \t\n')]
    class Inputs:
        def lookup(self,name):return dict(layers)[name.upper()].encode('ascii')
    m=retail_history(Inputs())
    try:
        loaded=[(name,body,*m.load_physical_ini(name)) for name,body in layers]
        ext,typ=m.alloc(0x1000),m.alloc(0x2000)
        m.u.mem_write(typ,dwords(0x7F6218));m.u.mem_write(typ+0x24,b'CONTROL\0')
        # Move fixture storage only; both original images keep preferred bases.
        m.u.mem_unmap(native_oracle.STACK_BASE,native_oracle.STACK_SIZE)
        map_pe32_image(m.u,ares,image_base=0x10000000,image_size=0xE0000,header_size=0x400)
        m.u.mem_map(0x33000000,0x20000);sp=0x33010000
        code=bytes(m.u.mem_read(0x10001000,0x92000))
        m.u.mem_write(sp,dwords(RET_MAGIC,typ));m.u.reg_write(UC_X86_REG_ESP,sp)
        m.u.reg_write(UC_X86_REG_ECX,ext);run_checked(m.u,0x1003C710,RET_MAGIC)
        default=m.u.mem_read(ext+0x4C6,1)[0];diagnostics=[];reads=[]
        def diagnostic(u,pc,*_):
            if pc==0x528A10:
                esp=u.reg_read(UC_X86_REG_ESP)
                reads.append(dict(section=m.string(m.read32(esp+4)),
                    key=m.string(m.read32(esp+8)),default=m.string(m.read32(esp+12)),
                    capacity=m.read32(esp+20)))
                return
            if pc!=0x10061960:return
            esp=u.reg_read(UC_X86_REG_ESP)
            diagnostics.append(dict(section=m.string(m.read32(esp+4)),
                key=m.string(m.read32(esp+8)),token=m.string(m.read32(esp+12))))
            m.ret(0,16)
        handle=m.u.hook_add(UC_HOOK_CODE,diagnostic)
        rows=[]
        for name,body,ini,receipt in loaded:
            before=m.u.mem_read(ext+0x4C6,1)[0];mark=len(diagnostics);read_mark=len(reads)
            m.u.mem_write(sp,dwords(RET_MAGIC,ini));m.u.reg_write(UC_X86_REG_ESP,sp)
            m.u.reg_write(UC_X86_REG_ECX,ext)
            stop=run_checked(m.u,0x1003DFB0,(0x1003DFD9,RET_MAGIC),
                required_addresses=(0x100091C0,))
            admitted=stop==0x1003DFD9
            if admitted:
                m.u.mem_write(sp,bytes(0x200));m.u.mem_write(sp+0x10,dwords(ini))
                m.u.reg_write(UC_X86_REG_ESP,sp);m.u.reg_write(UC_X86_REG_EBX,ext)
                m.u.reg_write(UC_X86_REG_ESI,typ+0x24)
                run_checked(m.u,0x1003F002,0x1003F018,
                    required_addresses=(0x1000EBF0,0x1000FC50,0x10007860,0x528A10))
            rows.append(dict(file=name,physical_ini=body,load=receipt,before=before,
                section_admitted=admitted,after=m.u.mem_read(ext+0x4C6,1)[0],
                diagnostics=diagnostics[mark:],reads=reads[read_mark:]))
        m.u.hook_del(handle);m.check_code()
        assert bytes(m.u.mem_read(0x10001000,0x92000))==code
        return dict(constructor_default=default,rows=rows)
    finally:m.close()


def no_manual_fire_action(ares,flag,case):
    """Compose original exports with this owner's unchanged action/click VM."""
    from tools.native_oracle import map_pe32_image
    global run_checked,STACK_BASE,SP
    original_runner,original_stack,original_sp=run_checked,STACK_BASE,SP
    STACK_BASE=0x31000000;SP=STACK_BASE+STACK_SIZE-0x1000
    extra=0x32000000;events=[];queries=[];returns=[];mapped=False;code=None
    def wrapped(u,begin,end,**kwargs):
        nonlocal mapped,code
        if not mapped:
            map_pe32_image(u,ares,image_base=0x10000000,image_size=0xE0000,header_size=0x400)
            u.mem_map(extra,0x10000);u.mem_write(TYPE+0x2FC,dwords(extra))
            u.mem_write(extra+0x4C6,bytes([flag]));mapped=True
            code=bytes(u.mem_read(0x10001000,0x92000))
        ends=(end,) if isinstance(end,int) else tuple(end)
        wanted=set(kwargs.pop('required_addresses',()));seen=set()
        def observe(_u,pc,*_):
            if pc in wanted:seen.add(pc)
            if pc in (0x73FD50,0x7404B0):
                esp=u.reg_read(UC_X86_REG_ESP)
                queries.append(dict(entry=f'{pc:08X}',
                    phase='hover' if begin==0x692610 else 'clicked_requery' if begin==0x738890 else f'{begin:08X}',
                    second_argument=struct.unpack('<I',u.mem_read(esp+8,4))[0]))
            if pc in (0x4DDDFE,0x7404FB):
                returns.append(dict(caller=f'{pc:08X}',
                    callee='TechnoCell' if pc==0x4DDDFE else 'FootCell',
                    action=u.reg_read(UC_X86_REG_EAX)))
        observer=u.hook_add(UC_HOOK_CODE,observe)
        try:
            active=set(NO_MANUAL_FIRE_HOOKS);pc=begin
            for _part in range(6):
                stop=original_runner(u,pc,ends+tuple(active),**kwargs)
                if stop in ends:
                    assert wanted<=seen,(case,sorted(wanted-seen))
                    assert bytes(u.mem_read(0x10001000,0x92000))==code
                    return stop
                assert stop in active
                context=u.context_save();actor=u.reg_read(UC_X86_REG_ESI)
                shim,stack,returned=extra+0x1000,extra+0x2000,extra+0x3000
                u.mem_write(shim+0x0C,dwords(actor));u.mem_write(stack,dwords(returned,shim))
                u.reg_write(UC_X86_REG_ESP,stack)
                original_runner(u,NO_MANUAL_FIRE_HOOKS[stop],returned)
                result=u.reg_read(UC_X86_REG_EAX)
                events.append(dict(site=f'{stop:08X}',export=f'{NO_MANUAL_FIRE_HOOKS[stop]:08X}',
                    returned=f'{result:08X}'))
                u.context_restore(context);pc=result or stop
                active.remove(stop) # zero executes the original displaced bytes once
            raise RuntimeError(('Too many Ares hook continuations',case))
        finally:u.hook_del(observer)
    run_checked=wrapped
    try:result=run(case,allied_continuation if case['route']=='object' else None)
    finally:run_checked,STACK_BASE,SP=original_runner,original_stack,original_sp
    return dict(input=case,no_manual_fire_native_reader_value=flag,action=result['action'],
        cursor_row=result['cursor']['output']['cursor_row'],hook_events=events,
        query_inputs=queries,cell_return_values=returns,
        left_up=result.get('left_up'),clicked_action=result.get('clicked_action'),
        native_action_trace=[row for row in result['trace'] if row.get('kind')!='seam'])


def generate_no_manual_fire():
    import os
    from itertools import product
    from tools.rules_oracle.ares_prerequisites import ARES_SHA256,identified_bytes
    ares=identified_bytes(Path(os.environ['VERA20K_ARES_DLL']),ARES_SHA256)
    gamemd=native_oracle.image_bytes();readers=no_manual_fire_readers(ares,gamemd)
    yes=next(row['after'] for row in readers['controls'] if row['value']=='yes')
    no=next(row['after'] for row in readers['controls'] if row['value']=='no')
    rows=[]
    for route,ctrl,shift,alt,flag in product(('object','cell'),(False,True),
            (False,True),(False,True),(no,yes)):
        case=dict(name='no_manual_fire',route=route,installed=False,in_range=True,
            relation='enemy' if route=='object' else 'none',ctrl=ctrl,shift=shift,
            alt=alt,cursor_contract=True,mouse_over=True)
        if route=='object':
            case.update(allied_objects=True,actor_house_index=0,target_house_index=1,
                actor_alliance_mask=0,target_alliance_mask=0,target_selected=False,
                attack_friendlies=False,attack_cursor_on_friendlies=False,
                target_passengers=0,ivan_bomb=False,sonic=False,infested=False)
        rows.append(no_manual_fire_action(ares,flag,case))
    return dict(schema_version=1,binaries={name:dict(bytes=len(raw),
        sha256=hashlib.sha256(raw).hexdigest()) for name,raw in
        [('Ares.dll',ares)]},readers=readers,
        physical_readers=no_manual_fire_physical(ares),rows=rows)


def no_manual_fire_metadata():
    return provenance(scope='Ares NoManualFire constructor, retained parser/physical INI controls and normal Unit action/Display/click admission; bounded shared VM priors.',
        entry_points=dict(type_ext_ctor=0x1003C710,type_reader=0x1003F002,
            bool_wrapper=0x1000EBF0,bool_reader=0x1000FC50,bool_parser=0x10076CC0,
            read_string=0x528A10,type_loader_hook=0x10040DF0,
            object_hook=0x1004A460,object_site=0x700536,object_skip=0x70056C,
            cell_hook=0x1004A430,cell_site=0x7008D4,cell_skip=0x700AB7),
        assumptions=[
            'Original full TypeExt1003C710 initializes NoManualFire+4C6=false. Original field block1003F002..1003F018 uses current Type.ID section, current passed INI, literal NoManualFire, ReadString capacity2048 and empty default; no ART switch or field-specific postpass. Missing/empty/invalid retain the prior field. Bool parser uses first ASCII character 1/T/Y or 0/F/N, case insensitive.',
            'Ares prerequisites.Probe owns lexical controls and original native vtables. Its ReadString input seam supplies raw already-read text without trimming; raw-space controls deliberately distinguish this boundary from physical ReadString. Native parser, constructor and hook exports are not replaced.',
            'Existing retail_history owns physical INI bytes/load and exact section admission. Nine declared file inputs execute real528A10 trim and exact-case lookup plus the same Ares block. This is a declared file sequence, not a complete session Process/Rules reset/language digest/mode/map startup execution. Unrelated extension-reader keys are skipped.',
            'Historical run() remains the only action/Display/click VM. 32 hover rows supply mouse_over=true, matching69300B, and cover Object/Cell and all Ctrl/Shift/Alt combinations with false/true values transferred by value from executed yes/no reader controls. Original DetermineAction passes1 to the Cell query but hard-codes0 for Object; Object ClickedAction re-query also passes0. Actual Unit query arguments and Cell callee returns are observed separately. Synthetic Syringe frames invoke actual exports at original hook sites, restore complete CPU context, and follow the returned continuation; no original instruction is patched.',
            'Ordinary enemy Object left click re-queries the action in UnitClickedObject. False reaches mission1/target argument producer6FFBE0; true preserves native nonattack fallback and stops4D78BA before navigation coordinates. Complete event delivery/navigation/firing is outside the boundary. Cell rows compare action/cursor and hook reachability; supplied movement state does not certify every movement fallback.',
            'NoManualFire has two observed action-query consumers. It does not establish a global firing ban. Script-issued/already-queued Attack and automatic targeting bypassing these UI queries remain outside executable controls; no simulation CanFire/AttackAI check is introduced. In the bounded object click continuation, existing observer asserts zero Main/RandomRanged, action-timer-start and detach calls; later command/voice/action-line effects are excluded.'],
        substitutions=[
            'Retain historical singleton selection, keyboard, map/Cell, shroud, playfield, terrain, range/FireError, palette and cursor-sink services. Actor/type/House/weapon state is supplied; no full native scenario, complete constructors/range arithmetic or Windows pixels.',
            'Reuse ares_prerequisites.Probe and physical Rules retail_history; no PCX/private path/assets, second VM, alternate parser, action or country implementation. Move fixture stack storage away from Ares preferred base only. Whole original gamemd text and Ares text10001000..10093000 remain unchanged.'])


if __name__=='__main__':
    allied='--allied-objects' in sys.argv
    attack='--attack-order-voice' in sys.argv
    move='--move-order-voice' in sys.argv
    manual='--no-manual-fire' in sys.argv
    assert sum((allied,attack,move,manual))<=1
    sources={'harness':Path(__file__),'native_oracle':Path(native_oracle.__file__)}
    if allied or attack or move:
        sources.update(rng_owner=Path(__file__).parents[1]/'rmg_oracle/gen_rng_vectors.py',
            physical_inputs_owner=Path(__file__).parents[1]/'rules_oracle/weapon_laser.py',
            lexical_owner=Path(__file__).parents[1]/'projectile_oracle/bridge_render_inputs.py')
    if attack or move:
        sources.update(sound_owner=Path(__file__).parents[1]/'rules_oracle/bridge_child_sound.py',
            sound_composition=Path(__file__).parent/'anytown_damage/mtnk_attack.py',
            reader_blocks=Path(__file__).parents[1]/'procedural_drawing_oracle/shroud.py')
    if manual:
        sources.update(ares_vm_owner=Path(__file__).parents[1]/'rules_oracle/ares_prerequisites.py',
            physical_rules_owner=Path(__file__).parents[1]/'rules_oracle/weapon_speed_order.py')
    finish_vectors(generate_no_manual_fire if manual else generate_move_order_voice if move else generate_attack_order_voice if attack else generate_allied_objects if allied else generate,
        Path(__file__).with_name('no_manual_fire.json' if manual else 'move_order_voice.json' if move else 'attack_order_voice.json' if attack else 'allied_object_action.json') if allied or attack or move or manual else Path(__file__).with_suffix('.json'),
        provenance=no_manual_fire_metadata if manual else move_voice_metadata if move else attack_voice_metadata if attack else allied_metadata if allied else metadata,
        argv=[arg for arg in sys.argv[1:] if arg not in ('--allied-objects','--attack-order-voice','--move-order-voice','--no-manual-fire')],
        source_paths=sources)
