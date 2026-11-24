"""One original Logic pass from an explicit production command boundary.

The supplied production snapshot is a continuation input, not proof of native
ScenarioLoad or whole-world RNG order. This reproduces the frozen native v7
counter comparison and records original instruction/vtable bytes separately.
"""
from pathlib import Path
import json
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESP,
)

from tools.native_oracle import _canonical, finish_vectors, provenance, run_checked
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.spatial_oracle.shrapnel_repair import packet_io
from . import mission as owner

HERE = Path(__file__).resolve().parent
ORDER_PCS = (0x736465, 0x73646B, 0x736473, 0x5B3570, 0x5B35B9,
             0x736479, 0x4DA530, 0x6F9E50, 0x6FA646, 0x6FA64F, 0x6FA655)


def production_actor(row):
    a = row['actor']
    p, mission, f, life = a['position'], a['mission'], a['barrel_facing'], a['lifecycle']
    return dict(frame=row['frame'], mission=mission['current'], queued=mission['queued'],
                status=mission['handler_state'],
                dispatch=[mission['dispatch_timer']['start_frame'], mission['dispatch_timer']['delay']],
                rearm=[a['rearm_timer']['start_frame'], a['rearm_timer']['duration']],
                target=a['attack_target']['target']['Cell'] if a['attack_target'] else None,
                alive=int(life['object_alive']), limbo=int(life['in_limbo']),
                marked=int(life['cell_marked']), health=a['health']['current'],
                position=[p['rx'] * 256 + p['sub_x']['bits'] // 65536,
                          p['ry'] * 256 + p['sub_y']['bits'] // 65536,
                          p['exact_z_leptons']],
                random_phase_raw_u16=a['techno_ctor_random_word'],
                mission_visit_count=mission['ai_counter'],
                primary_facing=[a['facing'] * 256, a['facing'] * 256],
                turret_facing=[f['current'], f['prev']])


def native_actor(q):
    s = q.state()
    s.pop('global_techno_count')
    s.pop('global_unit_count')
    s.pop('logic_registered')
    target = q.m.read32(q.src + 0x2B4)
    s['target'] = list(q.resident.coord(target)) if target else None
    return s


def difference(native, production):
    return {key: dict(native=native.get(key), production=production.get(key))
            for key in native.keys() | production.keys()
            if native.get(key) != production.get(key)}


def rng_state(q):
    return {k: owner.base.sr.rng_state(q.u, p) for k, p in q.resident.rngs.items()}


def instruction_readback(q):
    cs = Cs(CS_ARCH_X86, CS_MODE_32)
    rows = []
    for name, begin, end in (
        ('unit_prefix', 0x736461, 0x736480),
        ('unit_ready', 0x744270, 0x74446B),
        ('infantry_prefix', 0x51BC18, 0x51BCA4),
        ('infantry_ready', 0x521B60, 0x521C0F),
        ('commence', 0x5B3570, 0x5B35D2),
        ('foot_entry', 0x4DA530, 0x4DA53E),
        ('techno_counter', 0x6FA646, 0x6FA65A),
    ):
        raw = bytes(q.u.mem_read(begin, end - begin))
        rows.append(dict(name=name, begin=hex(begin), end=hex(end), bytes=raw.hex(),
                         instructions=[dict(pc=hex(i.address), bytes=i.bytes.hex(),
                                            text=f'{i.mnemonic} {i.op_str}')
                                       for i in cs.disasm(raw, begin)]))
    tables = []
    for name, address in (('unit', 0x7F5C70), ('infantry', 0x7EB058)):
        tables.append(dict(name=name, address=hex(address),
                           ready=hex(q.m.read32(address + 0x200)),
                           commence=hex(q.m.read32(address + 0x1EC))))
    return dict(code=rows, vtables=tables,
                boundary='Readback only for Infantry; the executed actor remains MTNK.')


def generate():
    inputs_raw = (HERE / 'mission_counter.input.json').read_bytes()
    inputs = json.loads(inputs_raw)
    identities = json.loads((HERE / 'mission_promotion.json').read_bytes())['counter']
    assert packet_io.digest(inputs_raw) == identities['production_input_sha256']
    command, first = inputs['command_applied'], inputs['first_tick']
    actor = command['actor']
    continuation = dict(
        provenance=dict(source=command['source'], source_sha256=command['source_sha256'],
                        boundary='Recorded after production frame0 command tail; includes prior whole-world RNG, not native ScenarioLoad'),
        command_frame=command['frame'] - 1, rng=command['rng'],
        actor=dict(random_phase_raw_u16=actor['techno_ctor_random_word']))
    q = owner.Mission(continuation)
    promotion = []

    def observe_order(u, pc, n, d):
        if q.src and q.phase == 'logic' and q.frame == 1 and pc in ORDER_PCS:
            promotion.append(dict(pc=hex(pc), counter=q.m.read32(q.src + 0xC4),
                                  mission=owner.base.i32(u, q.src + 0xAC),
                                  queued=owner.base.i32(u, q.src + 0xB4),
                                  eax=u.reg_read(UC_X86_REG_EAX),
                                  edx=u.reg_read(UC_X86_REG_EDX)))

    q.u.hook_add(UC_HOOK_CODE, observe_order)
    q.setup()
    before_guard = q.state()
    mission = actor['mission']
    timer = mission['dispatch_timer']
    # Declared prior Guard inputs, not a transplanted firing timer or result.
    q.u.mem_write(q.src + 0xC4, dwords(mission['ai_counter']))
    q.u.mem_write(q.src + 0xC8, dwords(timer['start_frame']))
    q.u.mem_write(q.src + 0xD0, dwords(timer['delay']))
    q.u.reg_write(UC_X86_REG_ESP, SP)
    q.u.reg_write(UC_X86_REG_EDI, 0)
    run_checked(q.u, 0x55DE73, 0x55DE87)
    q.frame = q.m.read32(0xA8ED84)
    result = dict(boundary=dict(
        before_prior_guard_import=before_guard,
        imported=dict(ai_counter=mission['ai_counter'], dispatch=timer),
        native=native_actor(q), production=production_actor(command),
        actor_difference=difference(native_actor(q), production_actor(command)),
        rng_equal=rng_state(q) == command['rng']))
    begin = len(q.events)
    q.tick()
    current = rng_state(q)
    result['first_tick'] = dict(
        native=native_actor(q), production=production_actor(first),
        actor_difference=difference(native_actor(q), production_actor(first)),
        native_rng=current, production_rng=first['rng'],
        rng_equal={k: current[k] == first['rng'][k] for k in current},
        events=q.events[begin:], shots=list(q.shots), first_frame=q.frames[0],
        promotion_order=promotion)
    # Exact measured prefix of the frozen full external continuation.
    assert packet_io.digest(_canonical(result)) == identities['comparison_projection_sha256']
    text_hash = packet_io.digest(bytes(q.u.mem_read(0x401000, 0x3E0000)))
    assert text_hash == q.resident.code_hash
    assert not q.pending, q.pending
    return dict(text_sha256=text_hash,
                input_sha256=packet_io.digest(inputs_raw), comparison=result,
                instruction_readback=instruction_readback(q))


def metadata():
    result = owner.metadata()
    result['scope'] = __doc__
    result['assumptions'].extend([
        'The complete comparison field is byte-equivalent after canonicalization to boundary/first_tick from the independently frozen external v7 native continuation. Its original payload/projection hashes are recorded in mission_promotion.json.',
        'Production command-applied input supplies complete recorded RNG states, source constructor phase word54697 and the preceding Guard counter1/dispatch(0,28). Original native constructors, Unlimbo and Event execute; no firing timer/result is transplanted. This retained-state experiment does not establish whole-native ScenarioLoad or preceding full-world Guard parity.',
        'Native first-tick counter1 is an executed golden. Production-v7 counter0 is a historical comparison observation, not an expected native value. First tick frame2 is the committed global frame; actual Logic/firing frame is1.',
        'Original code/vtable readback corroborates Unit and Infantry ordering. The actual executed source is MTNK; no new Infantry counter golden is claimed.',
    ])
    result['substitutions'].append(
        'Other actors/global phases after the imported boundary remain excluded. Native rearm61 versus full-production62 uses different RNG caller histories and is not an established arithmetic defect.')
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'mission_counter.json', provenance=metadata)

