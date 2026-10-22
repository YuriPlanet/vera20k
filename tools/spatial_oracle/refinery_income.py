"""Original deposit arithmetic and its bounded IncomeMult/PurifierBonus readers.

Numeric rows supply raw fields; separate reader rows execute native field readers.
No executable code, credit return, storage return or gameplay callback is replaced.
"""
from functools import lru_cache
from itertools import product
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.ai_base_building_oracle import FAKE, HOUSE, HOUSE_TYPE, RULES, SCENARIO
from tools.house_money_oracle import (
    ADD_TIBERIUM_CREDITS, BALANCE, CAPACITY, SCORE, SPENT, STORAGE,
    TIBERIUM, TIBERIUM_LIST, TIBERIUMS, f32, house,
)
from tools.native_oracle import (
    IMAGE_BASE, NATIVE_FPCW, OracleError, STACK_BASE, STACK_SIZE,
    _sections, finish_vectors, image_bytes, provenance, run_checked,
)

UNIT_BEGIN = 0x73E40C
UNIT_ENDS = (0x73E4DA, 0x73E4DC)
UNIT, UNIT_TYPE = FAKE + 0x60000, FAKE + 0x62000
MAIN_RNG, SCENARIO_RNG = 0x886B88, SCENARIO + 0x218
RNG_SIZE = 1012
INT_MIN, INT_MAX = -0x80000000, 0x7FFFFFFF
# An arbitrary raw pattern below nearest-rounded .333, not a reader result.
CHOPPED_0333_PATTERN = 0x3EAA7EF9


def bits(value):
    return f'{value & 0xFFFFFFFF:08X}'


@lru_cache(maxsize=1)
def original_executable_sections():
    data = image_bytes()
    result = []
    for rva, raw, raw_size, virtual_size, flags in _sections(data):
        if flags & 0x20000000:
            size = max(raw_size, virtual_size)
            expected = data[raw:raw + raw_size].ljust(size, b'\0')
            result.append((IMAGE_BASE + rva, expected))
    if not result:
        raise OracleError('no original executable sections')
    return result


def check_original_code_vm(uc):
    for address, expected in original_executable_sections():
        if bytes(uc.mem_read(address, len(expected))) != expected:
            raise OracleError(f'original executable section differs at {address:#x}')


def check_original_code(emu):
    check_original_code_vm(emu.uc)


def wallet(emu):
    return dict(balance=emu.read_i32(HOUSE + BALANCE),
                score=emu.read_i32(HOUSE + SCORE))


def prepare(case):
    emu = house(case['balance'], spent=7, score=case['score'])
    emu.write32(HOUSE_TYPE + 0x148, int(case['income_bits'], 16))
    emu.write32(TIBERIUMS, TIBERIUM_LIST)
    for slot, value in enumerate(case['values']):
        ptr = TIBERIUM + slot * 0x100
        emu.write32(TIBERIUM_LIST + slot * 4, ptr)
        emu.write32(ptr + 0xB8, value)
    # Original seed bodies establish nontrivial complete buffers. The measured
    # interval begins afterwards; seeding is not a deposit RNG draw.
    for ptr in (MAIN_RNG, SCENARIO_RNG):
        emu.invoke(0x65C6D0, ecx=ptr, args=(0x12345678,))
    rng = [bytes(emu.uc.mem_read(ptr, RNG_SIZE))
           for ptr in (MAIN_RNG, SCENARIO_RNG)]
    for address in (0x7C8E17, 0x7C8B3D, 0x7C978A):
        emu.mark(address, 'inherited CRT transport entered')
    check_original_code(emu)
    return emu, rng


def finish(emu, rng):
    check_original_code(emu)
    if rng != [bytes(emu.uc.mem_read(ptr, RNG_SIZE))
               for ptr in (MAIN_RNG, SCENARIO_RNG)]:
        raise OracleError('deposit changed a Main/Scenario RNG buffer')
    if emu.events:
        raise OracleError(f'unexpected inherited transport: {emu.events}')
    if emu.uc.reg_read(UC_X86_REG_FPCW) != NATIVE_FPCW:
        raise OracleError('deposit changed the ambient x87 control word')
    result = wallet(emu)
    result.update(spent=emu.read_i32(HOUSE + SPENT),
                  capacity=emu.read_i32(HOUSE + CAPACITY),
                  house_amount_bits=[bits(emu.read32(HOUSE + STORAGE + 4 * slot))
                                     for slot in range(4)])
    if result['spent'] != 7 or result['capacity'] != 200 or any(
            word != '00000000' for word in result['house_amount_bits']):
        raise OracleError('deposit changed unrelated supplied House storage fields')
    return result


def common(name, amount, income, *, slot=0, values=(25, 50, 75, 100),
           balance=0, score=0):
    return dict(name=name, amount_bits=bits(amount), income_bits=bits(income),
                values=list(values), slot=slot, balance=balance, score=score)


def primitive_case(case):
    emu, rng = prepare(case)
    before = wallet(emu)
    emu.invoke(ADD_TIBERIUM_CREDITS, ecx=HOUSE,
               args=(int(case['amount_bits'], 16), case['slot']),
               required=(ADD_TIBERIUM_CREDITS, 0x7C5F00))
    output = finish(emu, rng)
    call = dict(amount_bits=case['amount_bits'], slot=case['slot'],
                value=case['values'][case['slot']], before=before, after=wallet(emu))
    return dict(input=case, output=output, calls=[call])


def unit_case(case):
    emu, rng = prepare(case)
    uc = emu.uc
    emu.write32(RULES + 0xF3C, int(case['purifier_bits'], 16))
    emu.write32(UNIT + 0x6C4, UNIT_TYPE)
    # +E0F Weeder is false. Cargo itself is supplied, not harvested here.
    uc.mem_write(UNIT_TYPE + 0xE0F, b'\0')
    if case['slot'] >= 0:
        emu.write32(UNIT + 0x33C + case['slot'] * 4, int(case['amount_bits'], 16))
    emu.write32(UNIT + 0xF8, 123)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    emu.write32(sp + 0x30, case['effective_purifiers'])
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.reg_write(UC_X86_REG_ESI, UNIT)
    uc.reg_write(UC_X86_REG_EBX, HOUSE)
    uc.reg_write(UC_X86_REG_EBP, case['slot'] & 0xFFFFFFFF)
    calls, bonus = [], []

    def observe(_uc, address, _size, _data):
        if address == 0x73E445:
            bonus.append(bits(emu.read32(sp + 0x30)))
        elif address == ADD_TIBERIUM_CREDITS:
            if uc.reg_read(UC_X86_REG_ECX) != HOUSE:
                raise OracleError('Unit credited a different House receiver')
            current_sp = uc.reg_read(UC_X86_REG_ESP)
            slot = emu.read_i32(current_sp + 8)
            calls.append(dict(amount_bits=bits(emu.read32(current_sp + 4)),
                              slot=slot, value=case['values'][slot], before=wallet(emu)))
        elif address in (0x73E4AE, 0x73E4CE):
            if not calls or 'after' in calls[-1]:
                raise OracleError('unmatched original credit return')
            calls[-1]['after'] = wallet(emu)

    hook = uc.hook_add(UC_HOOK_CODE, observe)
    try:
        run_checked(uc, UNIT_BEGIN, UNIT_ENDS, count=10000,
                    required_addresses=(UNIT_BEGIN, 0x73E441),
                    context={'case': case['name']})
    finally:
        uc.hook_del(hook)
    if uc.reg_read(UC_X86_REG_ESP) != sp or len(bonus) != 1 or any(
            'after' not in call for call in calls):
        raise OracleError('incomplete Unit numeric interior')
    output = finish(emu, rng)
    output.update(bonus_bits=bonus[0],
                  cargo_bits=[bits(emu.read32(UNIT + 0x33C + 4 * slot))
                              for slot in range(4)],
                  stage=emu.read_i32(UNIT + 0xF8),
                  stop_before=f'0x{uc.reg_read(UC_X86_REG_EIP):08X}')
    return dict(input=case, output=output, calls=calls)


def primitive_inputs():
    yield common('40 units, Value25, IncomeMult .9', f32(40), f32(.9))
    for amount in range(1, 41):
        yield common(f'whole amount {amount}, IncomeMult .9', f32(amount), f32(.9),
                     slot=(amount - 1) % 4)
    amounts = (0, 0x80000000, 1, 0x007FFFFF, 0x00800000, f32(.333), f32(.9),
               f32(1.5), f32(-1.5), 0x7F7FFFFF, 0xFF7FFFFF,
               0x7F800000, 0xFF800000, 0x7FC01234)
    incomes = (0, f32(.9), f32(1), f32(1.25), f32(-.9), 0x7F7FFFFF,
               0x7F800000, 0xFF800000, 0x7FC05678)
    for index, (amount, income) in enumerate(product(amounts, incomes)):
        yield common(f'raw primitive {index}', amount, income, slot=index % 4,
                     values=(25, -25, INT_MIN, INT_MAX),
                     balance=(0, -1, INT_MIN, INT_MAX)[index % 4],
                     score=(0, -20, INT_MIN, INT_MAX)[(index // 4) % 4])
    for slot, value in product(range(4), (INT_MIN, -1, 0, 1, 25, INT_MAX)):
        values = [20, 40, 60, 80]
        values[slot] = value
        yield common(f'live signed Value {value}, slot {slot}', f32(40), f32(.9),
                     slot=slot, values=values, balance=INT_MAX - 10, score=INT_MAX - 5)


def unit_inputs():
    for amount in range(1, 41):
        case = common(f'whole Unit amount {amount}', f32(amount), f32(.9),
                      slot=(amount - 1) % 4, balance=500, score=70)
        yield dict(case, bales=amount, purifier_bits=bits(CHOPPED_0333_PATTERN), effective_purifiers=3)
    bonuses = (CHOPPED_0333_PATTERN, f32(.333), CHOPPED_0333_PATTERN | 0x80000000, 0, 1,
               0x7F7FFFFF, 0x7F800000, 0xFF800000, 0x7FC01234)
    counts = (-3, 0, 1, 3, INT_MAX, INT_MIN)
    for index, (amount, bonus, count) in enumerate(product((1, 3, 7, 40), bonuses, counts)):
        case = common(f'Unit bonus gate {index}', f32(amount),
                      (f32(.9), f32(1), f32(1.25))[index % 3], slot=index % 4,
                      values=(25, 50, -25, INT_MAX),
                      balance=(0, -1, INT_MIN, INT_MAX)[index % 4],
                      score=(0, -20, INT_MIN, INT_MAX)[(index // 4) % 4])
        yield dict(case, bales=amount, purifier_bits=bits(bonus), effective_purifiers=count)
    for slot in (0, -1):
        case = common(f'Unit empty amount, slot {slot}', 0, f32(.9), slot=slot)
        yield dict(case, bales=0, purifier_bits=bits(CHOPPED_0333_PATTERN), effective_purifiers=3)


def reader_rows():
    # Reuse the existing cached-INI, native scanner initialization, Country
    # constructor, TLS/heap transport and timer/RNG authority unchanged.
    from tools.rules_oracle.country_veterancy import fresh, rng, bounds
    from tools.spatial_oracle.building_body_rules import RULES as INI_RULES, SP
    from unicorn.x86_const import UC_X86_REG_EDI

    rows = []
    cases = (('decimal1.2', '1.2', None), ('decimal.9', '.9', None),
             ('decimal.333', '.333', None), ('percent33.3', '33.3%', None),
             ('absent', None, None), ('retained_default', None, '1.2'))
    for field in ('IncomeMult', 'PurifierBonus'):
        for name, raw, prefix_raw in cases:
            m = fresh()
            u = m.u
            if field == 'IncomeMult':
                section, target, offset = 'SYNTHCOUNTRY', m.country, 0x148
            else:
                section, target, offset = 'General', m.alloc(0x2000), 0xF3C
                # This exact Rules caller is run without the Rules constructor.
                u.mem_write(target + offset, struct.pack('<I', f32(.25)))
                if m.string(m.read32(0x7F0C9C)) != section:
                    raise OracleError('original PurifierBonus section pointer differs')

            def read(value):
                keys = {} if value is None else {field: value}
                m.rules_cache({section: keys})
                before = rng(m)
                m.timer_writes.clear()
                check_original_code_vm(u)
                calls = []

                def observe(_u, pc, _size, _data):
                    if pc != 0x5283D0:
                        return
                    sp = u.reg_read(UC_X86_REG_ESP)
                    key = m.string(m.read32(sp + 8))
                    if key != field:
                        return
                    if u.reg_read(UC_X86_REG_ECX) != INI_RULES:
                        raise OracleError('reader used a different INI receiver')
                    calls.append(dict(section=m.string(m.read32(sp + 4)), key=key,
                                      default_f64_bits=f'{struct.unpack("<Q", u.mem_read(sp + 12, 8))[0]:016X}',
                                      fpcw=f'{u.reg_read(UC_X86_REG_FPCW):04X}'))

                hook = u.hook_add(UC_HOOK_CODE, observe)
                try:
                    if field == 'IncomeMult':
                        admitted = m.invoke(0x511850, target, (INI_RULES,)) & 0xFF
                        if admitted != 1:
                            raise OracleError('Country reader rejected its supplied section')
                    else:
                        u.reg_write(UC_X86_REG_ESP, SP)
                        u.reg_write(UC_X86_REG_ESI, target)
                        u.reg_write(UC_X86_REG_EDI, INI_RULES)
                        run_checked(u, 0x66FC56, 0x66FC7B, count=100000,
                                    required_addresses=(0x5283D0, 0x66FC75))
                        if u.reg_read(UC_X86_REG_ESP) != SP:
                            raise OracleError('PurifierBonus reader changed caller stack')
                finally:
                    u.hook_del(hook)
                check_original_code_vm(u)
                bounds(m, before)
                if len(calls) != 1 or calls[0]['section'] != section:
                    raise OracleError('selected ReadDouble call was not observed once')
                return calls

            if prefix_raw is not None:
                read(prefix_raw)
            prior = bits(m.read32(target + offset))
            calls = read(raw)
            rows.append(dict(input=dict(name=name, field=field, section=section, raw=raw,
                                        default_bits=prior, prefix_raw=prefix_raw),
                             output=dict(bits=bits(m.read32(target + offset))), calls=calls))
    return rows


def generate():
    return dict(schema_version=1,
                primitive_rows=[primitive_case(case) for case in primitive_inputs()],
                unit_rows=[unit_case(case) for case in unit_inputs()],
                reader_rows=reader_rows())


def metadata():
    record = provenance(
        scope='Full original House4F9610, Unit73E40C numeric deposit interior, and bounded native field readers.',
        assumptions=[
            'Deposit rows: fresh data-shaped House/HouseType, four Tiberium slots, Unit/UnitType, Rules and '
            'stack; raw amount/IncomeMult/PurifierBonus fields, signed32 slot Values, wallet '
            'and effective purifier count are supplied. No native object constructor, '
            'INI reader, harvesting, difficulty/virtual-count addition or mission admission runs.',
            'Unit begins after effective-count selection with ESI Unit, EBX recipient House, '
            'EBP selected slot and count at ESP+30. Original GetAmount6C9680 and '
            'RemoveAmount6C96B0 execute; Weeder is false. End73E4DA or73E4DC is not executed. '
            'No Refinery smoke/radio, stage dispatch, map lookup or later mission suffix runs.',
            'The full House4F9610 body and original7C5F00 execute for every credit call, '
            'including separate base/positive-bonus score and cash updates. Unit73E441 '
            'stores bonus to float32 before its comparison and optional second call.',
            'Ambient x87 control0E7F supplies masked exceptions, 53-bit precision and chop. '
            'Raw quiet NaNs, infinities, subnormals and conversion overflow are component '
            'inputs, not claims that stock gameplay produces them. Unit cargo is whole '
            'amounts1..40 plus two empty controls; arbitrary fractional cargo histories remain open.',
            'Numeric rows supply raw3EAA7EF9 without parsing; it is not the no-percent .333 '
            'reader result. Raw3EAA7EFA is separate. Reader rows establish .333=>3EAA7EFA '
            'and33.3%=>3EAA7EF9 for both fields.',
            'Every original executable section is compared byte-for-byte to its PE mapping '
            'before and after each row. Original65C6D0 seeds complete1012-byte Main and '
            'Scenario RNG buffers before the measured interval; both remain byte-identical. '
            'Inherited Emu heap/CRT transports are guarded and never entered.',
            'Reader rows independently reuse country_veterancy.fresh/BulletReader: original '
            'scanner initializer7C8F5E, Country constructor5113F0 and complete Country '
            'ReadINI511850 execute for IncomeMult. PurifierBonus executes exact '
            'Rules66FC56..unexecuted66FC7B, including ReadDouble5283D0 and float store66FC75; '
            'its initial .25 field is supplied, without Rules construction. Six cached '
            'string/default cases per field include a prior native1.2 read before retained absence.',
            'Reader measured intervals begin after fixture/native Country constructor setup '
            'and check every executable section, all three complete inherited '
            'RNG buffers and the action-timer sentinel. Cached strings are supplied; '
            'physical INI/MIX loading is excluded. No decimal parse or selected reader result '
            'is substituted. The reader helper supplies its existing unrelated color registry.',
        ], substitutions=[
            'Reader rows only: existing BulletReader bump allocation7C8E17, inert '
            'delete7C8B3D and CRT TLS accessor7D140B returning thread storage. '
            'Cached exact-case INI entries replace physical file/archive loading. '
            'Deposit rows do not enter their separate inherited heap/CRT transports.'
        ],
        entry_points=dict(house=ADD_TIBERIUM_CREDITS, unit_begin=UNIT_BEGIN,
                          unit_positive_end_before=UNIT_ENDS[0],
                          unit_empty_end_before=UNIT_ENDS[1],
                          get_amount=0x6C9680, remove_amount=0x6C96B0, ftol=0x7C5F00,
                          country_constructor=0x5113F0, country_reader=0x511850,
                          purifier_reader_begin=0x66FC56, purifier_reader_end_before=0x66FC7B,
                          read_double=0x5283D0, scanner_init=0x7C8F5E))
    record['row_counts'] = dict(primitive=191, unit=258, reader=12)
    record['observations'] = (
        'Raw input words, per-slot signed live Values, each original credit-call amount '
        'and before/after wallet, final wallet/spent/capacity/House store, Unit narrowed '
        'bonus/cargo/stage and unexecuted stop boundary. Reader input strings/current defaults, '
        'original ReadDouble default/section/key and final float field bits. No executable bytes or executable '
        'identity hashes are published.')
    return record


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
