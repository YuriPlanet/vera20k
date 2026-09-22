"""Original BulletType Image/Flat reader and Bullet GetLayer.

Each case supplies a retained Flat byte and cold INI lookup indexes. The original
Image gate, fixed-ART lookup, bool defaulting and layer query execute unchanged.
The complete Rules sweep, file parser and INI pointer-cache aliasing are excluded.
"""
from functools import lru_cache
from pathlib import Path

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    SCRATCH, RET_MAGIC, call, run_checked, finish_vectors, provenance,
)
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, RULES, SP
from tools.spatial_oracle.map_queries import dwords


@lru_cache
def crc(text):
    raw = text.encode('ascii')
    return call(0x4A1DE0, ecx=SCRATCH,
                stack_args=[SCRATCH + 0x100, len(raw)],
                writes={SCRATCH: bytes(16), SCRATCH + 0x100: raw})['eax']


def ini_index(u, ini, backing, section_name, key, raw):
    """One supplied section/key, with the original signed-CRC lookup format."""
    u.mem_write(ini, bytes(0x40))
    u.mem_write(backing, bytes(0x800))
    section, sections, entry, entries, value = [backing + n for n in
                                              (0, 0x100, 0x200, 0x300, 0x400)]
    u.mem_write(ini + 0x28, dwords(sections, 1, 1, 1, 0))
    u.mem_write(sections, dwords(crc(section_name), section))
    # The lexical loader drops blank values. Missing Flat retains its default.
    present = raw is not None and bool(raw.strip())
    u.mem_write(section + 0x2C, dwords(entries, int(present), 1, 1, 0))
    u.mem_write(entries, dwords(crc(key), entry))
    u.mem_write(entry + 0x10, dwords(value))
    u.mem_write(value, (raw or '').encode('ascii') + b'\0')


def execute(spec):
    f = Fixture()
    u = f.u
    u.mem_write(TYPE + 0x24, b'SHOT\0')
    u.mem_write(TYPE + 0x2F7, bytes([int(spec['prior'])]))
    ini_index(u, RULES, SCRATCH + 0x7000, 'SHOT', 'Image', spec['image'])
    ini_index(u, INI, SCRATCH + 0x8000, spec['art_section'], 'Flat', spec['flat'])
    u.reg_write(UC_X86_REG_ESI, TYPE)
    u.reg_write(UC_X86_REG_EBX, RULES)
    u.reg_write(UC_X86_REG_EDI, TYPE + 0x24)
    u.reg_write(UC_X86_REG_EAX, 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x46C1CC, 0x46C298, count=100000)
    assert u.reg_read(UC_X86_REG_ESP) == SP
    flat = bool(u.mem_read(TYPE + 0x2F7, 1)[0])
    image = bytes(u.mem_read(TYPE + 0x1F8, 25)).split(b'\0')[0].decode('ascii')
    bullet = SCRATCH + 0x9000
    u.mem_write(bullet + 0xAC, dwords(TYPE))
    u.reg_write(UC_X86_REG_ECX, bullet)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.mem_write(SP, dwords(RET_MAGIC))
    run_checked(u, 0x468B90, RET_MAGIC, count=50)
    return dict(**spec, effective_flat=flat, image_read=image,
                layer=u.reg_read(UC_X86_REG_EAX))


def generate():
    cases = []
    for prior in (False, True):
        for raw in (None, '', 'yes', 'no', 'true', 'false', '1', '0', 'on',
                    'off', 'nonsense', 'yep', 'unknown'):
            cases.append(dict(name=f'bool_{prior}_{raw}', prior=prior,
                              image='ART', art_section='ART', flat=raw))
        for name, image, section in (
            ('missing_image', None, 'SHOT'), ('blank_image', '', 'SHOT'),
            ('no_type_fallback', 'MISSING', 'SHOT'),
            ('trim_image', '  ART  ', 'ART'),
            ('capacity_truncates', 'ABCDEFGHIJKLMNOPQRSTUVWX_extra', 'ABCDEFGHIJKLMNOPQRSTUVWX'),
            ('long_section_not_read', 'ABCDEFGHIJKLMNOPQRSTUVWX_extra', 'ABCDEFGHIJKLMNOPQRSTUVWX_extra'),
        ):
            cases.append(dict(name=f'{name}_{prior}', prior=prior, image=image,
                              art_section=section, flat='no' if prior else 'yes'))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'image_art_read': 0x46C1CC, 'flat_read': 0x46C272,
                      'bullet_layer': 0x468B90, 'read_string': 0x528A10,
                      'read_bool': 0x5295F0},
        assumptions=['Supplied retained BulletType fields and cold single-section INI indexes; empty lexical values omitted.',
                     'Original Image/Trailer/SpawnDelay/Rotates/Flat branch executes; unrelated ART keys absent so no animation allocation occurs.',
                     'Constructor false is separately established by 46BCE0; cases explicitly exercise both retained Flat values.'],
        substitutions=[],
        scope='38 original Image/Flat reader and subsequent layer queries. Covers missing keys, defaults, first-character booleans, missing Image/type-ID fallback and 25-byte truncation. Excludes complete Rules sweeps, file loading, pointer-cache alias histories and rendering.',
    ))
