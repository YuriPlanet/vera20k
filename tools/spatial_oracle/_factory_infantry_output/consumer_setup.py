"""Selected native input/runtime caller prior for the UnitReady controls.

Promoted once from the sealed GI/Sale caller composition (ee744441…). Native
readers/constructors/audio/map/allocator owners are imported, never copied.
The historical41-entry AnyTown alias and admitted prior actors are deliberate
comparison inputs, not a complete match or physical map-load certificate.
"""
from pathlib import Path
import hashlib, struct
from unicorn.x86_const import *
from tools import native_oracle as native
from tools.native_oracle import run_checked
from tools.rules_oracle.bridge_child_sound import sections
from tools.spatial_oracle import building_death_anims as owner, building_construction as bc, engineer_repair_admission as er
from tools.spatial_oracle.building_body_rules import INI, SP


def prepare(root, eva_physical):
    ROOT = Path(root)
    report = {}
    eva_cache = None
    def prepare_eva_cache():
        nonlocal eva_cache
        saved = bytes(m.u.mem_read(INI, 0x40))
        m.make_ini(eva_physical)
        eva_cache = m.alloc(0x40)
        m.u.mem_write(eva_cache, bytes(m.u.mem_read(INI, 0x40)))
        m.u.mem_write(INI, saved)
        receipt = []
        saved_ini = bytes(m.u.mem_read(INI, 0x40))
        for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP'):
            p = ROOT / filename
            if not p.exists():
                continue
            raw = sections(p.read_bytes())
            m.make_ini({'General': raw['General']} if 'General' in raw else {})
            before = {hex(o): m.read32(m.rules + o) for o in (0x78, 0x7C, 0x80, 0x84)}
            if m.invoke(0x526810, INI, (m.cstring('General'),)):
                for start, end in ((0x6719AC, 0x6719F5), (0x671AA2, 0x671ADA)):
                    m.u.mem_write(SP, bytes(0x300))
                    for reg, v in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, INI)):
                        m.u.reg_write(reg, v)
                    native.run_checked(m.u, start, end)
            receipt.append(dict(layer=filename, sha256=hashlib.sha256(p.read_bytes()).hexdigest(), before=before,
                physical_keys={k:v for k,v in raw.get('General', {}).items() if k.startswith('RadarEvent')},
                after={hex(o):m.read32(m.rules+o) for o in (0x78,0x7C,0x80,0x84)}))
        m.u.mem_write(INI, saved_ini)
        report['radar_scalar_readers'] = receipt
    m = owner.joined_inputs(ROOT, damage_fires=True)
    u, read = (m.u, m.read32)
    physical = sections((ROOT / 'RULESMD.INI').read_bytes())
    art = sections((ROOT / 'ARTMD.INI').read_bytes())
    sound = sections((ROOT / 'SOUNDMD.INI').read_bytes())
    wanted = {'BuildingGarrisoned', 'AlliedOccupiedAttack', 'GIAttack', 'GIAttackDeployed'}
    sound_input = {name: sound[name] for name in ('Defaults', *sorted(wanted))}
    sound_input['SoundList'] = {key: value for key, value in sound['SoundList'].items() if value in wanted}
    m.make_ini(sound_input)
    m.invoke(0x007510D0, INI)
    report['selected_sounds'] = [dict(name=name, index=m.invoke(0x007514D0, m.cstring(name))) for name in sorted(wanted)]
    art_names = ('CAGAS01', 'GI', 'GISequence', 'UCFLASH')
    m.make_ini({name: art[name] for name in art_names if name in art})
    art_cache = bytes(u.mem_read(INI, 64))
    btype = m.alloc(8192)
    m.invoke(0x0045DD90, btype, (m.cstring('CAGAS01'),))
    neutral = next((row['pointer'] for row in m.death_country_sides['countries'] if row['name'] == 'Neutral'))
    refs = ('Primary', 'Secondary', 'ElitePrimary', 'EliteSecondary', 'OccupyWeapon', 'EliteOccupyWeapon')
    weapon_names = [physical['E1'][key] for key in refs]
    weapon_ptrs = {name: m.invoke(0x00772FA0, m.cstring(name)) for name in weapon_names}
    report['layers'] = []
    for filename in ['RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP']:
        path = ROOT / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            report['layers'].append(dict(name=filename, absent=True))
            continue
        layer = sections(path.read_bytes())
        names = {'CAGAS01', 'E1', 'Neutral', 'AudioVisual', 'CombatDamage', 'General', *weapon_names}
        for name in weapon_names:
            names.update((physical[name][key] for key in ('Projectile', 'Warhead')))
        retained = {name: layer[name] for name in names if name in layer}
        m.make_ini(retained)
        rule_ini = m.alloc(64)
        u.mem_write(rule_ini, bytes(u.mem_read(INI, 64)))
        u.mem_write(INI, art_cache)
        br = m.invoke(0x0045FE50, btype, (rule_ini,)) & 255
        ir = m.invoke(0x005240A0, m.types['E1'], (rule_ini,)) & 255
        cr = m.invoke(0x00511850, neutral, (rule_ini,)) & 255
        weapons = []
        for name, pointer in weapon_ptrs.items():
            wr = m.invoke(0x00772080, pointer, (rule_ini,)) & 255
            pr = read(pointer + 160)
            wh = m.invoke(0x0075E3B0, m.cstring(physical[name]['Warhead']))
            assert read(pointer + 172) == wh
            if pr:
                m.invoke(0x0046BEE0, pr, (rule_ini,))
            if wh:
                m.invoke(0x0075D3A0, wh, (rule_ini,))
            m.invoke(0x007729F0, pointer)
            weapons.append(dict(name=name, read=wr, pointer=pointer, projectile=pr, warhead=wh))
        m.make_ini(retained)
        u.mem_write(SP, bytes(768))
        for reg, value in [(UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, INI)]:
            u.reg_write(reg, value)
        if m.invoke(0x00526810, INI, (m.cstring('AudioVisual'),)):
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x0066B337, 0x0066B35E)
        if m.invoke(0x00526810, INI, (m.cstring('CombatDamage'),)):
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x0066C670, 0x0066C6DA)
        if m.invoke(0x00526810, INI, (m.cstring('General'),)):
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x0067019A, 0x006701D9)
        u.mem_write(SP, bytes(768))
        for reg, value in [(UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, INI)]:
            u.reg_write(reg, value)
        u.reg_write(UC_X86_REG_EAX, read(m.rules + 460))
        run_checked(u, 0x00669BD5, 0x00669C68)
        report['layers'].append(dict(name=filename, sha256=hashlib.sha256(path.read_bytes()).hexdigest(), physical_sections=retained, building_reader=br, infantry_reader=ir, country_reader=cr, weapons=weapons, garrison_sound=read(m.rules + 444), abandoned_sound=read(m.rules + 448), condition_red_f64_hex=bytes(u.mem_read(m.rules + 5896, 8)).hex(), occupy_fields_hex=bytes(u.mem_read(m.rules + 3904, 12)).hex(), targeting_delays_s32=list(struct.unpack('<2i', u.mem_read(m.rules + 3588, 8)))))
    report['native_types'] = dict(cagas_pointer=btype, cagas_name=m.string(btype + 36), cagas_strength=read(btype + 160), can_be_occupied=u.mem_read(btype + 5499, 1)[0], max_occupants=read(btype + 5504), foundation=read(btype + 3824), array_index=read(btype + 3576), image_name=m.string(btype + 504), image_pointer=read(btype + 164), asset_loads=m.asset_loaded, neutral_pointer=neutral, neutral_side=read(neutral + 188), neutral_multiplay_passive=u.mem_read(neutral + 422, 1)[0])
    u.mem_write(INI, art_cache)
    flash = m.invoke(0x00428B80, m.cstring('UCFLASH'))
    mark = len(m.asset_loaded)
    flash_read = m.invoke(0x00427D00, flash, (INI,))
    report['flash_native_art'] = dict(pointer=flash, **m.result('UCFLASH', flash, flash_read), assets=m.asset_loaded[mark:], physical_keys=art['UCFLASH'])
    m.types['GAPOWR'] = btype
    player_country = m.country
    m.country = neutral
    native_ctor_fields = {}
    original_invoke = bc.invoke

    def observe_native_ctor_call(machine, entry, this, *args):
        answer = original_invoke(machine, entry, this, *args)
        if entry == 0x0043B740 and this == er.BLD:
            native_ctor_fields.update(entry='0x0043B740', pointer=this, actual_hp_6c=struct.unpack('<i', machine.mem_read(this + 108, 4))[0], estimated_hp_70=struct.unpack('<i', machine.mem_read(this + 112, 4))[0], buildup_6e9=machine.mem_read(this + 1769, 1)[0], actually_placed_6ea=machine.mem_read(this + 1770, 1)[0], ai_sellable_6dc=machine.mem_read(this + 1756, 1)[0])
        return answer
    bc.invoke = observe_native_ctor_call
    prepare_eva_cache()
    try:
        f = owner.joined_fixture(m, 1000, seed=2, building_ai=True)
    finally:
        bc.invoke = original_invoke
    m.country = player_country
    u, read = (f.u, f.read32)
    report['cagas_original_ctor'] = native_ctor_fields
    assert native_ctor_fields['buildup_6e9'] == 0 and native_ctor_fields['ai_sellable_6dc'] == 0
    report['cagas_admitted_prior_correction'] = dict(inherited_gapowr_buildup_6e9=f.u.mem_read(er.BLD + 1769, 1)[0], retained_original_cagas_buildup_6e9=native_ctor_fields['buildup_6e9'], inherited_engineer_estimated_hp_70=f.read32(er.BLD + 112), retained_original_cagas_estimated_hp_70=native_ctor_fields['estimated_hp_70'], rng_before=f.rng())
    f.u.mem_write(er.BLD + 1769, bytes([native_ctor_fields['buildup_6e9']]))
    assert native_ctor_fields['actual_hp_6c'] == native_ctor_fields['estimated_hp_70'] == 1000
    f.u.mem_write(er.BLD + 112, bc.dwords(native_ctor_fields['estimated_hp_70']))
    report['cagas_admitted_prior_correction']['rng_after'] = f.rng()
    assert report['cagas_admitted_prior_correction']['rng_before'] == f.rng()
    report['startup'] = dict(infantry=f.infantry_startup, subcell=f.subcell_startup, object=f.original_object_startup)
    player, neutral_house = f.owner_houses
    u.mem_write(player + 52, bc.dwords(player_country))
    house_items = read(0x00A8022C)
    u.mem_write(house_items, bc.dwords(player, neutral_house))
    u.mem_write(0x00A80238, bc.dwords(2))
    u.mem_write(neutral_house + 52, bc.dwords(neutral))
    u.mem_write(neutral_house + 492, b'\x00')
    index = read(btype + 3576)
    assert index < 128
    for house, quantity in [(player, 0), (neutral_house, 1)]:
        for off in [21760, 21840]:
            items = read(house + off + 4)
            u.mem_write(items, bytes(128 * 4))
            u.mem_write(items + index * 4, bc.dwords(quantity))
            u.mem_write(house + off + 16, bc.dwords(index + 1))
    f.types['ENGINEER'] = m.types['E1']
    f.phase = 'supplied_actor_admission'
    actor = f.actor((9, 10))
    return dict(f=f, m=m, u=u, r=read, actor=actor, player=player, neutral_house=neutral_house, neutral=neutral, btype=btype, report=report, eva_cache=eva_cache)
