"""Original retained MissileROTVar/MissileSafetyAltitude ctor and reader slice.

Full Rules665650 executes; only the two original General scalar reads execute
per pass. Cached INI loading and unrelated type discovery are supplied. Invalid
float strings are observed as native failed scans, not assigned host values.
"""
import hashlib
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.rules_oracle.weapon_speed_order import fresh
from tools.spatial_oracle.building_body_rules import RULES, SP

BEGIN, END = 0x66EC1B, 0x66EC62


def state(m, rules):
    raw = bytes(m.u.mem_read(rules + 0x598, 12))
    bits, altitude = struct.unpack('<Qi', raw)
    return dict(missile_rot_var_bits=f'{bits:016x}', safety_altitude=altitude)


def normalized_cache(sections):
    # Models only the existing loader's trimming and empty-key/value omission.
    # Values remain lexical strings; native ReadDouble/ReadInt own conversions.
    trim = ''.join(map(chr, range(33)))
    return {name: {key.strip(trim): value.strip(trim) for key, value in values.items()
                   if key.strip(trim) and value.strip(trim)}
            for name, values in sections.items()}


def sequence(cases, *, normalize=True):
    m, _unused_weapon, rules = fresh()
    u = m.u
    original = bytes(u.mem_read(BEGIN, END - BEGIN))
    constructor = state(m, rules)
    reads, scans = [], []

    def observe(uc, address, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in (0x5283D0, 0x5276D0):
            assert u.reg_read(UC_X86_REG_ECX) == RULES
            row = dict(reader=f'{address:08x}', section=m.string(m.read32(sp + 4)),
                       key=m.string(m.read32(sp + 8)))
            if address == 0x5283D0:
                row['default_bits'] = f'{struct.unpack("<Q", u.mem_read(sp + 12, 8))[0]:016x}'
            else:
                row['default'] = struct.unpack('<i', u.mem_read(sp + 12, 4))[0]
            reads.append(row)
        elif address == 0x52855D:
            scans.append(dict(assignments=struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0],
                              result_f32_bits=f'{m.read32(sp + 0x2C):08x}'))
        elif address in (0x65C780, 0x65C7E0, 0x68BCB0):
            raise AssertionError(('unexpected RNG or native identity assignment', hex(address)))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    rows = []
    try:
        for name, sections in cases:
            cache = normalized_cache(sections) if normalize else sections
            m.rules_cache(cache)
            reads.clear()
            scans.clear()
            before = state(m, rules)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, rules)
            u.reg_write(UC_X86_REG_EDI, RULES)
            run_checked(u, BEGIN, END, required_addresses=(0x5283D0, 0x5276D0, 0x66EC3C, 0x66EC5C))
            assert u.reg_read(UC_X86_REG_ESP) == SP
            assert original == bytes(u.mem_read(BEGIN, END - BEGIN))
            rows.append(dict(name=name, authored_sections=sections, cached_sections=cache,
                             before=before, reads=list(reads), float_scans=list(scans),
                             after=state(m, rules)))
    finally:
        u.hook_del(hook)
    return dict(constructor=constructor, rows=rows,
                original_slice=dict(start=f'{BEGIN:08x}', end_exclusive=f'{END:08x}',
                                    hex=original.hex(), sha256=hashlib.sha256(original).hexdigest()))


def controls():
    def general(name, rot=None, altitude=None):
        values = {}
        if rot is not None:
            values['MissileROTVar'] = rot
        if altitude is not None:
            values['MissileSafetyAltitude'] = altitude
        return name, {'General': values}

    return [
        ('missing_section_from_constructor', {}),
        general('missing_keys_from_constructor'),
        general('finite_authored_override', '.2', '750'),
        ('missing_section_retains', {}),
        general('empty_authored_values_omitted', '', ''),
        general('whitespace_authored_values_omitted', ' \t ', '\t '),
        general('fraction_precision', '0.1', '-25'),
        general('f64_text_narrows_to_f32', '0.25000000000000006', '2147483647'),
        general('adjacent_f32', '0.2500000298023223876953125', '-2147483648'),
        general('float_midpoint', '1.000000059604644775390625', '2147483648'),
        general('adjacent_one_f32', '1.00000011920928955078125', '4294967297'),
        general('percent', '25%', '$2EE'),
        general('percent_anywhere', '25suffix%', '2EEh'),
        general('negative_percent', '-12.5%', '$-1'),
        general('signed_zero', '-0', '-4294967297'),
        general('signed_float_and_atoi_suffix', ' +.75tail', ' +42suffix'),
        general('float_exponent', '2.5e-1', '1.5'),
        general('invalid_float', 'garbage', 'garbage'),
        ('missing_after_invalid_retains_bits', {}),
        general('restore_before_wrong_case', '.5', '123'),
        ('wrong_key_case', {'General': {'missilerotvar': '9', 'missilesafetyaltitude': '999'}}),
        ('wrong_section_case', {'general': {'MissileROTVar': '9', 'MissileSafetyAltitude': '999'}}),
        general('int_zero_x_atoi', None, '0x123'),
        general('int_signed_min_hex', None, '$80000000'),
        general('int_hex_wrap', None, '100000001h'),
        general('float_subnormal', '1.401298464324817070923729583289916131280e-45', '-1'),
        general('float_underflow', '1e-50', '0'),
        general('float_overflow', '3.5e38', '500'),
        ('missing_after_overflow_retains_bits', {}),
        general('restore_after_overflow', '.25', '500'),
    ]


def generate():
    return dict(sequential=sequence(controls()),
                raw_empty_cache=sequence([
                    ('set_nonzero', {'General': {'MissileROTVar': '.75', 'MissileSafetyAltitude': '321'}}),
                    ('cached_empty_not_physical_omission', {'General': {'MissileROTVar': '', 'MissileSafetyAltitude': ''}}),
                ], normalize=False))


def metadata():
    return provenance(scope=__doc__, entry_points={'rules_constructor': 0x665650,
        'guided_controls_reader': BEGIN, 'end_exclusive': END,
        'read_double': 0x5283D0, 'read_int': 0x5276D0, 'crt_float_scanner_init': 0x7C8F5E},
        assumptions=[
            'Full original RulesClass665650 constructor executes. The sequential passes execute only66EC1B..66EC62; no full General or Process chronology is claimed.',
            'Before each call the same retained Rules object supplies both reader defaults; no scalar output is computed or installed by the host.',
            'Main sequence supplies lexical General cache strings after ASCII trimming/empty omission, matching the production loader contract. Physical INI parsing/archive IO is not executed.',
            'Separate raw_empty_cache deliberately retains empty cached values and is not a physical empty-INI-value claim.',
            'ReadDouble scans into a binary32 result then widens to binary64; percent multiplication, failed scans and overflow execute natively. Results are preserved as bits to represent non-finite values without JSON NaN/Infinity.',
            'Malformed nonempty/empty cached float outputs expose the unchanged scanner destination in this caller/cache fixture. They are observations, not universal invalid-input defaults for the unexecuted full General body.',
            'The pinned minimum-subnormal decimal control differs from Rust parsing. It bounds the finite-normal-input production comparison; no custom subnormal or failed-scan emulation is supplied.',
            'The inherited fresh helper constructs an unrelated OrderProbe Weapon before Rules; it is not read by the selected slice. Runtime native ID/RNG calls inside the reader slice are rejected.',
            'Native x87 PC53/chop0E7F; no original code patches, scalar reader hooks or supplied arithmetic results.',
        ], substitutions=[
            'Inherited BulletReader supplies exact signed-CRC INI cache, allocator/CRT TLS/archive boundaries and a fixture Color fallback; none of those other behaviors are claimed.',
        ])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
