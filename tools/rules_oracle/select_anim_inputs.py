"""Retained inputs consumed by original SelectAnim48A4F0.

Full Rules/Warhead constructors and full Warhead reader execute. Rules reads
are isolated original WeatherConBoltExplosion, WeaponNullifyAnim, LightningWarhead and SplashList
blocks, in their relative Process order; physical loader/full Process omitted.
"""
import struct
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_ECX
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.rules_oracle.weapon_speed_order import fresh
from tools.rules_oracle.guided_controls import normalized_cache
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords


def pointer_name(m, ptr):
    return m.string(ptr + 0x24) if ptr else None


def vector_names(m, owner, offset):
    ptr, count = m.read32(owner + offset + 4), m.read32(owner + offset + 0x10)
    return [pointer_name(m, m.read32(ptr + 4 * index)) for index in range(count)]


def state(m, rules, wh):
    return dict(conventional=bool(m.u.mem_read(wh + 0x14D, 1)[0]),
                em_effect=bool(m.u.mem_read(wh + 0x154, 1)[0]),
                anim_list=vector_names(m, wh, 0x104),
                lightning_warhead=pointer_name(m, m.read32(rules + 0x17B4)),
                weather_con_bolt_explosion=pointer_name(m, m.read32(rules + 0x2F4)),
                weapon_nullify_anim=pointer_name(m, m.read32(rules + 0x350)),
                splash_list=vector_names(m, rules, 0xBC0))


def controls():
    return [
        ('missing_sections', {}),
        ('base', {'WH': {'Conventional':'yes','EMEffect':'yes','AnimList':'A,B'},
                  'General': {'LightningWarhead':'WH','WeatherConBoltExplosion':'WX','WeaponNullifyAnim':'NULLFX'},
                  'CombatDamage': {'SplashList':'S1,S2'}}),
        ('missing_retains', {}),
        ('empty_authored_omitted', {'WH': {'Conventional':'','EMEffect':'','AnimList':''},
                  'General': {'LightningWarhead':'','WeatherConBoltExplosion':'','WeaponNullifyAnim':''},
                  'CombatDamage': {'SplashList':''}}),
        ('invalid_bool_retains', {'WH': {'Conventional':'garbage','EMEffect':'garbage'}}),
        ('numeric_bool_false', {'WH': {'Conventional':'0','EMEffect':'0'}}),
        ('false_and_untrimmed_list', {'WH': {'Conventional':'false','EMEffect':'no','AnimList':'A, B,,<none>'}}),
        ('exact_none_clears', {'WH': {'AnimList':'none'},
                  'General': {'LightningWarhead':'none','WeatherConBoltExplosion':'<none>','WeaponNullifyAnim':'none'},
                  'CombatDamage': {'SplashList':',,,'}}),
        ('missing_after_clear', {}),
        ('case_insensitive_factory', {'WH': {'Conventional':'true','EMEffect':'true','AnimList':'a,b,A'},
                  'General': {'LightningWarhead':'wh','WeatherConBoltExplosion':'wx','WeaponNullifyAnim':'nullfx'},
                  'CombatDamage': {'SplashList':'s2,s1,s2'}}),
        ('wrong_key_case', {'WH': {'conventional':'no','emeffect':'no','animlist':'C'},
                  'General': {'lightningwarhead':'OTHER','weatherconboltexplosion':'OTHER','weaponnullifyanim':'OTHER'},
                  'CombatDamage': {'splashlist':'OTHER'}}),
        ('list_readstring128', {'WH': {'AnimList': ','.join('A' for _ in range(70))},
                               'CombatDamage': {'SplashList': ','.join('S1' for _ in range(50))}}),
    ]


def read_general_blocks(m, rules):
    for start, end in ((0x66DF19, 0x66DF60), (0x66E2AF, 0x66E2E5), (0x671053, 0x671072)):
        m.u.reg_write(UC_X86_REG_ESP, SP)
        m.u.reg_write(UC_X86_REG_ESI, rules)
        m.u.reg_write(UC_X86_REG_EDI, RULES)
        if start == 0x66E2AF:
            # Reproduce live prolog arguments already pushed at 66E2A4.
            m.u.reg_write(UC_X86_REG_ESP, SP - 4)
            m.u.mem_write(SP - 4, dwords(128))
            m.u.reg_write(UC_X86_REG_ECX, SP + 0x50)
        run_checked(m.u, start, end)


def generate():
    m, _, rules = fresh()
    m.u.mem_write(0x8874C0, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    wh = m.invoke(0x75E3B0, m.cstring('WH'))
    assert m.read32(0x8874D0) == 1
    constructor = state(m, rules, wh)
    rows = []
    for name, authored in controls():
        cache = normalized_cache(authored)
        m.rules_cache(cache)
        before = state(m, rules, wh)
        read_general_blocks(m, rules)
        # The selected reference always denotes our already allocated WH when
        # nonnull; unrelated newly named types would need their own sweep.
        admitted = m.invoke(0x75D3A0, wh, (RULES,)) & 255
        m.u.reg_write(UC_X86_REG_ESP, SP)
        m.u.reg_write(UC_X86_REG_ESI, rules)
        m.u.reg_write(UC_X86_REG_EDI, RULES)
        run_checked(m.u, 0x66C184, 0x66C287)
        rows.append(dict(name=name, authored_sections=authored, cached_sections=cache,
                         before=before, warhead_body_admitted=admitted, after=state(m, rules, wh)))
    return dict(constructor=constructor, rows=rows)


def metadata():
    return provenance(scope=__doc__, entry_points={'rules_ctor':0x665650,'warhead_ctor':0x75CEC0,
        'warhead_reader':0x75D3A0,'weather_anim_reader':0x66DF19,'lightning_warhead_reader':0x671053,
        'nullify_anim_reader':0x66E2AF,'splash_list_reader':0x66C184,'select_anim_consumer':0x48A4F0}, assumptions=[
        'Same native objects retain fields across all passes. Full original constructors/Warhead reader execute; only named Rules blocks execute, not full Process.',
        'Physical INI loading is replaced by lexical CRC caches with empty-value omission. Original ReadBool, ReadString128, strtok, factories and vector-copy blocks own all scalar and reference results.',
        'Warhead+154 is EMEffect, not RandomAnims; Rules+17B4/+2F4 are LightningWarhead/WeatherConBoltExplosion, not IonCannonWarhead/IonBlast.',
        'Inherited fresh helper constructs one unrelated Weapon before Rules. No SelectAnim call, runtime effect construction or destructive Type reset executes.',
    ], substitutions=['Inherited fresh helper supplies allocator/CRT/TLS/archive and lexical cache boundaries.'])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
