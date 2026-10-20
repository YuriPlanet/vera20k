"""Bounded original Ares 3.0 custom armor and ordinary weapon controls.

Reuses the existing Ares prerequisite guest fixture and native_oracle PE mapper.
INI and CRT leaves are explicit supplied boundaries; no mod assets or DLL/game
startup are executed. The adjacent metadata and Markdown bound each claim.
"""
from pathlib import Path
import argparse
import hashlib
import json
import re
import struct
from tools.native_oracle import OracleError
from tools.rules_oracle.ares_prerequisites import (
    ARES_SHA256, Probe, identified_bytes,
)
from unicorn.x86_const import *

FIXTURE = Path(__file__).with_suffix(".json")


class ArmorProbe(Probe):
    def __init__(self):
        self.extra_events=[]
        super().__init__()
        # Native game's established 53-bit/chop environment, matching existing
        # retail oracle fixtures; do not rely on Unicorn's zero control word.
        self.u.reg_write(UC_X86_REG_FPCW,0x0e7f)
        self.u.mem_map(0x31000000,0x1000)
        self.u.mem_write(0x31000000,b'\xdd\x05'+struct.pack('<I',0x31000800)+b'\xc3')

    def hook(self,u,pc,size,data):
        sp=u.reg_read(UC_X86_REG_ESP)
        args=lambda n:[self.read(sp+4+i*4) for i in range(n)]
        ret=self.read(sp)
        result=None;cleanup=0
        if pc==0x10005800:
            section,key,default,target,capacity=args(5)
            pair=(self.string(section),self.string(key))
            value=self.reads.get(pair,self.string(default))
            payload=value.encode()[:capacity-1]
            u.mem_write(target,payload+b'\0')
            self.events.append(dict(service='supplied_ReadString',section=pair[0],key=pair[1],value=value,capacity=capacity))
            result=len(payload);cleanup=20
        elif pc==0x10005fb0:
            section,key,default=args(3)
            pair=(self.string(section),self.string(key))
            value=self.reads.get(pair)
            result=default&255 if value is None else int(value.lower() in ('yes','true','1'))
            self.events.append(dict(service='supplied_ReadBool',section=pair[0],key=pair[1],fallback=default&255,value=value,result=result))
            cleanup=12
        elif pc==0x10078ed0:
            text,character=args(2);value=bytes(u.mem_read(text,len(self.string(text))))
            index=value.find(bytes([character]));result=0 if index<0 else text+index
        elif pc==0x1007c04e:
            text,=args(1);value=self.string(text)
            match=re.match(r'\s*([+-]?\d+)',value);result=int(match[1]) if match else 0
            self.events.append(dict(service='supplied_CRT_atoi',text=value,value=result))
        elif pc==0x100843ec:
            text,=args(1);value=self.string(text)
            try: result_float=float(value)
            except ValueError:result_float=0.0
            u.mem_write(0x31000800,struct.pack('<d',result_float))
            self.events.append(dict(service='supplied_CRT_atof',text=value,value=result_float))
            u.reg_write(UC_X86_REG_EIP,0x31000000);return
        elif pc in (0x10078000,0x10078580,0x10078b00):
            dest,source,count=args(3);u.mem_write(dest,bytes(u.mem_read(source,count)));result=dest
        elif pc==0x31000020:
            result=self.target_type
        if result is None:
            return super().hook(u,pc,size,data)
        u.reg_write(UC_X86_REG_EAX,result&0xffffffff)
        u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
        u.reg_write(UC_X86_REG_EIP,ret)

    def stdvector(self,at,items,capacity=128):
        data=self.alloc(capacity*16)
        self.write(at,data);self.write(at+4,data+16*len(items));self.write(at+8,data+16*capacity)
        for i,v in enumerate(items):self.verses_write(data+16*i,v)

    def verses_write(self,at,v):
        self.u.mem_write(at,struct.pack('<dBBB5x',*v))

    def verses_read(self,at):
        a,b,c,d=struct.unpack('<dBBB5x',self.u.mem_read(at,16))
        return dict(multiplier=a, multiplier_bits=f"{struct.unpack('<Q', self.u.mem_read(at, 8))[0]:016x}", force_fire=bool(b),retaliate=bool(c),passive_acquire=bool(d))

    def armor_registry(self,names):
        data=self.alloc(1024);result=[]
        for i,name in enumerate(names):
            at=self.alloc(0x38);self.call(0x10006100,self.text(name),ecx=at)
            self.write(data+4*i,at);result.append(at)
        self.write(0x100c18b4,data);self.write(0x100c18b8,data+4*len(names));self.write(0x100c18bc,data+1024)
        return result

    def warhead(self,items):
        wh=self.alloc(0x200);ext=self.alloc(0x200)
        self.u.mem_write(wh+0x24,b'TestWH\0');self.write(wh+0x1cc,ext)
        self.stdvector(ext+0x24,items)
        return wh,ext

    def hook_state(self,wh,armor=11):
        state=self.alloc(0x40)
        self.write(state+8,wh);self.write(state+0x20,wh);self.write(state+0x24,armor)
        return state

    def total_damage(self,damage,multiplier,armor=11,max_damage=10000,distance=0):
        wh,_=self.warhead([(1,1,1,1)]*11+[(multiplier,1,1,1)])
        # Native WH spread/percent-at-max defaults are fixture data, not inferred INI defaults.
        self.u.mem_write(wh+0x124,struct.pack('<f',0.0))
        self.u.mem_write(wh+0x12c,struct.pack('<f',1.0))
        rules=self.alloc(0x2000);self.write(0x8871e0,rules);self.write(rules+0x16c8,max_damage)
        flags=self.alloc(4);self.write(0xa8b230,flags)
        # A minimal guest-only Syringe REGISTERS adapter at the established hook site.
        # pushad/pushfd/origin matches the ABI inspected in the original hook bodies.
        at=0x31000100
        code=b'\x60\x9c\x68'+struct.pack('<I',0x489235)+b'\x54\xe8'
        code+=struct.pack('<i',0x10054230-(at+len(code)+4))
        code+=b'\x83\xc4\x04\x83\xc4\x04\x9d\x61\xe9'
        code+=struct.pack('<i',0x489249-(at+len(code)+4))
        self.u.mem_write(at,code)
        self.u.mem_write(0x489235,b'\xe9'+struct.pack('<i',at-(0x489235+5)))
        self.u.reg_write(UC_X86_REG_EDX,wh)
        result=self.call(0x489180,armor,distance,ecx=damage)
        return struct.unpack('<i',struct.pack('<I',result))[0]


def controls():
    results={"schema_version": 1, 'identity': identities(), 'fpcw':'0x0e7f',
        'scope':'Bounded original instruction controls; all CRT numeric inputs and INI reads are supplied fixture boundaries.',
        'cases':[]}
    def add(name,m,**kw):results['cases'].append(dict(name=name,**kw,events=m.events))
    for text in ('0%','1%','2%','3%','00%','1.5%','0.5%','-1%','0','0.001','0.002','0.01','0.011','0.012','0.02','0.021','0.022','-0.5','0.5'):
        m=ArmorProbe();target=m.alloc(16);m.call(0x100523e0,m.text(text),ecx=target)
        add('parse_'+text,m,fixture=dict(token=text),result=m.verses_read(target))
    names=['none','flak','plate','light','medium','heavy','wood','steel','concrete','special_1','special_2','paper','later','magic']
    m=ArmorProbe();types=m.armor_registry(names)
    for name,value in [('paper','steel'),('later','magic'),('magic','11%')]:
        m.reads[('ArmorTypes',name)]=value;m.call(0x10006890,0x1111,ecx=types[names.index(name)])
    for name in ['paper','later','magic']:
        at=types[names.index(name)];add('registry_'+name,m,default_index=struct.unpack('<i',m.u.mem_read(at+0x20,4))[0],default_verses=m.verses_read(at+0x28))
    wh,ext=m.warhead([(1,1,1,1)]*11)
    m.reads.update({('TestWH','Versus.steel'):'25%',('TestWH','Versus.steel.ForceFire'):'no',('TestWH','Versus.magic'):'50%',('TestWH','Versus.paper.PassiveAcquire'):'no'})
    m.call(0x10006460,0x1111,wh)
    data=m.read(ext+0x24)
    add('warhead_inline_inheritance',m,result={name:m.verses_read(data+16*i) for i,name in enumerate(names)})
    # Re-read without custom override: existing entries are retained, not re-inherited.
    m.reads={('TestWH','Versus.steel'):'75%'};m.events=[];m.call(0x10006460,0x1111,wh)
    add('warhead_reread_retains_custom',m,result={name:m.verses_read(data+16*i) for i,name in enumerate(names)})
    for value,ff in [(0,False),(0,True),(1,False),(-1,False)]:
        m=ArmorProbe();wh,ext=m.warhead([(1,1,1,1)]*11+[(value,ff,False,False)])
        state=m.hook_state(wh);result=m.call(0x100542a0,state)
        add(f'can_fire_{value}_{ff}',m,return_address=hex(result),accepted=result==0x6fcb8d)
    # FUCOMPI/LAHF/TEST AH,44h/JNP rejects ordered equality only. Avoid a
    # nonstandard JSON NaN literal: the fixture identity is explicit raw bits.
    m=ArmorProbe();nan_bits=0x7ff8000000000000
    nan=struct.unpack('<d',struct.pack('<Q',nan_bits))[0]
    wh,_=m.warhead([(1,1,1,1)]*11+[(nan,False,False,False)])
    state=m.hook_state(wh);result=m.call(0x100542a0,state)
    add('can_fire_nan_false',m,fixture=dict(multiplier_bits=f'{nan_bits:016x}',force_fire=False),
        return_address=hex(result),accepted=result==0x6fcb8d)
    for flag in (False,True):
        for entry,yes in [(0x10054270,0x6f7d55),(0x100543e0,0x708b0b)]:
            m=ArmorProbe();wh,ext=m.warhead([(1,1,1,1)]*11+[(0,True,flag,flag)])
            state=m.hook_state(wh);result=m.call(entry,state)
            add(f'admit_{entry:x}_{flag}',m,return_address=hex(result),accepted=result==yes)
    for damage,multiplier in [(101,.25),(-101,.25),(3,.5),(-3,.5),(123,0),(123,-.5)]:
        m=ArmorProbe();wh,ext=m.warhead([(1,1,1,1)]*11+[(multiplier,1,1,1)])
        state=m.hook_state(wh);m.write(state+0x1c,11);m.write(state+0x20,damage)
        result=m.call(0x10054230,state)
        add(f'damage_{damage}_{multiplier}',m,result=struct.unpack('<i',m.u.mem_read(state+0x24,4))[0],return_address=hex(result))
    for secondary,primary in [(0,1),(1,0),(1,1),(0,0),(-1,1)]:
        m=ArmorProbe();wh1,_=m.warhead([(1,1,1,1)]*11+[(secondary,1,1,1)]);wh2,_=m.warhead([(1,1,1,1)]*11+[(primary,1,1,1)])
        w1=m.alloc(0x100);w2=m.alloc(0x100);m.write(w1+0xac,wh1);m.write(w2+0xac,wh2)
        saved=m.alloc(0x40);m.write(saved+0x10,w1);m.write(saved+0x14,w2)
        target=m.alloc(0x100);vt=m.alloc(0x100);m.target_type=m.alloc(0x100);m.write(m.target_type+0x9c,11)
        m.write(target,vt);m.write(vt+0x84,0x31000020)
        state=m.alloc(0x40);m.write(state+0x10,target);m.write(state+0x14,saved)
        result=m.call(0x10054360,state)
        add(f'weapon_select_secondary{secondary}_primary{primary}',m,return_address=hex(result),fixture=dict(primary_multiplier=primary,secondary_multiplier=secondary))
    for damage,multiplier,armor,cap in [(101,.25,11,10000),(3,.5,11,10000),(-101,.25,11,10000),(-101,.25,7,10000),(123,-.5,11,10000),(123,10,11,300),(1,.01,11,10000),(123,0,11,10000)]:
        m=ArmorProbe();result=m.total_damage(damage,multiplier,armor,cap)
        add(f'total_damage_{damage}_{multiplier}_{armor}_{cap}',m,result=result,fixture=dict(armor=armor,damage=damage,multiplier=multiplier,max_damage=cap,distance=0))
    for value,fallback in [(None,7),('',7),('PAPER',7),('missing',7),(None,999)]:
        m=ArmorProbe();m.armor_registry(names);state=m.alloc(0x40);original_sp=m.alloc(0x20)
        m.write(state+0x14,original_sp);m.write(state+0x20,0x1111)
        for delta,v in [(4,m.text('Target')),(8,m.text('Armor')),(12,fallback)]:m.write(original_sp+delta,v)
        if value is not None:m.reads[('Target','Armor')]=value
        m.call(0x10006e10,state)
        add(f'read_armor_{value}_{fallback}',m,result=m.read(state+0x24))
    return results


def identities():
    ares = ArmorProbe.input_bytes[0]
    return {
        "ares": dict(file="Ares.dll", length=len(ares), sha256=hashlib.sha256(ares).hexdigest()),
    }


def metadata(raw, count):
    return dict(
        schema_version=1,
        scope="Ares 3.0 armor token/default/warhead entry inheritance, original armor binding hooks, ordinary weapon admission/selection and bounded GetTotalDamage",
        ares_sha256=ARES_SHA256,
        payload_sha256=hashlib.sha256(raw).hexdigest(), control_count=count,
        fpcw="0x0e7f",
        entry_points={
            "armor_constructor": "0x10006100", "armor_default_reader": "0x10006890",
            "armor_name_reader_hook": "0x10006e10", "verses_token_parser": "0x100523e0",
            "warhead_armor_entry_reader": "0x10006460", "can_fire": "0x100542a0",
            "passive_acquire": "0x10054270", "retaliate": "0x100543e0",
            "select_weapon": "0x10054360", "damage_hook": "0x10054230",
            "damage_hook_site": "0x00489235", "game_get_total_damage": "0x00489180",
        },
        boundaries=[
            "Synthetic ASCII ReadString/ReadBool returns, existing registries and valid hook register-state objects are supplied; physical INI loading and whole Rules Process are excluded.",
            "CRT atoi/atof, strchr, string comparison/copy, memory move, allocation and logging are supplied leaf services. Floating token conversion itself is not native-executed or certified; original flag derivation and multiplication execute on supplied binary64 input.",
            "Original armor constructor/default reader, WH append/copy/override loops, numeric flag derivation, admission and weapon-selection hook bodies execute unchanged; GetTechnoType virtual dispatch returns a fixture type.",
            "The eight total_damage cases execute original489180 with a guest-only pushad/pushfd/origin adapter for the actual installed489235 Ares hook. Rules MaxDamage, no-damage flag, spread, percent-at-max and distance are declared inputs; full Syringe startup is not executed.",
            "The six damage_ cases execute only the Ares multiplier hook. Negative input there is characterization only: complete489180 bypasses verses for incoming negative damage at distance<8. Use total_damage cases for the bounded complete receiver.",
            "No ordinary gameplay world, effects/lifecycle, snapshots, full DLL/extension reader, unknown/mod content or whole-mod/native-rendering parity is claimed.",
        ],
        provenance="Executed the recorded original DLL and executable bytes at preferred VAs with synthetic none/flak/paper/later/magic and TestWH inputs. No game or mod assets are distributed.",
        source_leads=[
            "https://ares-developers.github.io/Ares-docs/new/additionalarmortypesandverses.html",
            "https://github.com/Ares-Developers/Ares/blob/master/src/Enum/ArmorTypes.cpp",
            "https://github.com/Ares-Developers/Ares/blob/master/src/Ext/WarheadType/Hooks.Verses.cpp",
        ],
        source_limit="Public master differs from installed3.0 in WH initialization/override ordering, CanFire gating and hook sites; original instruction execution establishes this corpus.",
        unicorn_binding="2.1.4",
        command="python -m tools.rules_oracle.ares_armor --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --check",
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ares-dll", type=Path, required=True)
    parser.add_argument("--gamemd-exe", type=Path, required=True)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--check", action="store_true")
    action.add_argument("--output", type=Path)
    args = parser.parse_args()
    ArmorProbe.input_bytes = (
        identified_bytes(args.ares_dll, ARES_SHA256),
        args.gamemd_exe.read_bytes(),
    )
    payload = controls()
    raw = (json.dumps(payload, indent=2) + "\n").encode()
    if args.check:
        bundled = FIXTURE.read_bytes()
        meta = json.loads(FIXTURE.with_suffix(".meta.json").read_text(encoding="utf8"))
        if hashlib.sha256(bundled).hexdigest() != meta["payload_sha256"]:
            raise OracleError("Ares armor corpus does not match its metadata")
        if payload != json.loads(bundled):
            raise OracleError("Ares armor execution differs from the bundled corpus")
        print(f"Ares armor controls match: {len(payload['cases'])}")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(raw)
        args.output.with_suffix(".meta.json").write_text(
            json.dumps(metadata(raw, len(payload["cases"])), indent=2) + "\n", encoding="utf8")
        print(f"Wrote {len(payload['cases'])} Ares armor controls")


if __name__ == "__main__":
    main()
