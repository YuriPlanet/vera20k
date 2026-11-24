"""Original Building gap operational admission and ordered shroud observations.

Run python -B -m tools.spatial_oracle.gap_admission --check / --write.
"""
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP

from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance,
    run_checked,
)
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.shroud_current_sight import execute

BUILDING, TYPE, HOUSE = 0xB00000, 0xB10000, 0xB20000


def operational_case(name, **changes):
    values = dict(has_power=1, special_state=0, disabled_count=0, health=600,
                  powered=1, drain=100, output=200, current_mission=5,
                  queued_mission=-1, powered_special=0, needs_engineer=0,
                  engineer=0, house_timer_start=-1, house_timer_duration=0,
                  house_special=0, frame=0)
    values.update(changes)
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 4096)
    u.mem_write(BUILDING, bytes(0x2000))
    u.mem_write(TYPE, bytes(0x2000))
    u.mem_write(HOUSE, bytes(0x6000))
    u.mem_write(BUILDING, dwords(0x7E3EBC))
    u.mem_write(BUILDING + 0x520, dwords(TYPE))
    u.mem_write(BUILDING + 0x21C, dwords(HOUSE))
    for offset, key in [(0x67C, 'special_state'), (0x504, 'disabled_count'),
                        (0x6C, 'health'), (0xAC, 'current_mission'),
                        (0xB4, 'queued_mission')]:
        u.mem_write(BUILDING + offset, dwords(values[key]))
    for offset, key in [(0x660, 'has_power'), (0x6CC, 'engineer')]:
        u.mem_write(BUILDING + offset, bytes([values[key]]))
    for offset, key in [(0x1573, 'powered'), (0x1574, 'powered_special'),
                        (0x1552, 'needs_engineer')]:
        u.mem_write(TYPE + offset, bytes([values[key]]))
    u.mem_write(TYPE + 0xEE4, dwords(values['drain']))
    u.mem_write(HOUSE + 0x53A4, dwords(values['output'], values['drain']))
    u.mem_write(HOUSE + 0x2A4, dwords(values['house_timer_start']))
    u.mem_write(HOUSE + 0x2AC, dwords(values['house_timer_duration']))
    u.mem_write(HOUSE + 0x577B, bytes([values['house_special']]))
    u.mem_write(0xA8ED84, dwords(values['frame']))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, BUILDING)
    run_checked(u, 0x4555D0, RET_MAGIC, count=1000)
    return dict(name=name, inputs=values, admitted=u.reg_read(UC_X86_REG_EAX) & 0xFF)


def vectors():
    cases = [
        ('powered', {}), ('power_deficit', dict(output=99)),
        ('power_equal', dict(output=100)), ('zero_output', dict(output=0)),
        ('no_power_flag', dict(has_power=0)),
        ('signed_special_override', dict(has_power=0, special_state=2, output=0)),
        ('disabled_positive', dict(disabled_count=1)),
        ('disabled_negative', dict(disabled_count=-1)),
        ('zero_health', dict(health=0)), ('negative_health', dict(health=-1)),
        ('construction', dict(current_mission=0x12)),
        ('selling', dict(current_mission=0x13)),
        ('queued_selling_while_guard', dict(queued_mission=0x13)),
        ('fallback_selling', dict(current_mission=-1, queued_mission=0x13)),
        ('fallback_guard', dict(current_mission=-1, queued_mission=5)),
        ('unpowered_type', dict(powered=0, output=0)),
        ('engineer_required', dict(needs_engineer=1)),
        ('engineer_present', dict(needs_engineer=1, engineer=1)),
    ]
    prefix = ['reveal', 'gap', 'leave', 'gap', 'reveal', 'remove', 'frame120']
    return dict(
        operational=[operational_case(name, **changes) for name, changes in cases],
        order=[execute('source_before_gap', prefix + ['leave', 'reveal', 'remove', 'frame240']),
               execute('gap_before_source', prefix + ['remove', 'leave', 'reveal', 'frame240'])],
    )


if __name__ == '__main__':
    finish_vectors(vectors, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Building4555D0 predicate and selected gap/Foot event ordering',
        assumptions=[
            'Complete4555D0 executes with original Building vtable, Mission5B3040 and House4FCE30 callees; supplied receiver/Type/House fields are explicit',
            'Optional gate cases demonstrate original predicate inputs only, not retail producer admission for disabled_count/special_state/NeedsEngineer',
            'Ordered cases reuse original shroud leaves and complete120-frame sweep; callbacks and registration order are supplied, not a whole Logic or power-system execution',
            'The reachable prefix leaves one sight receipt and one hostile gap, counter-1 and closed knowledge after frame120; no raw shroud state is fabricated',
            'Stock GAGAP power fields and active Building43FB20 edge caller are bound separately in the mechanism report',
        ],
        substitutions=[],
        entry_points={'operational': 0x4555D0, 'mission': 0x5B3040,
                      'power_ratio': 0x4FCE30, 'map_cell': 0x4A9CA0,
                      'gap_add': 0x6FB2F7, 'gap_remove': 0x6FB5E1,
                      'periodic_logic': 0x55B29A},
    ))
