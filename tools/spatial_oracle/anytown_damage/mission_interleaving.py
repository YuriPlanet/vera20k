"""Native selected MTNK Logic replay with recorded production-v8 RNG interleaving.

Recorded Terrain calls execute original Next, not original Terrain AI. Original
Unit/Foot/Mission/Fire/Bullet code produces every selected-owner call and result.
No firing state or bridge result is imported from the production snapshots.
"""
import json
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)

from tools.native_oracle import RET_MAGIC, finish_vectors, run_checked
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.spatial_oracle.shrapnel_repair import packet_io
from . import mission as owner
from .mission_counter import difference, native_actor, production_actor, rng_state

HERE = Path(__file__).resolve().parent
INPUT_SHA256 = '602ae9d3b9e686956bc542ef6f23276fee5f6f781fa44f9e2d939c47ba9e544b'


def generate():
    raw = (HERE / 'mission_interleaving.input.json').read_bytes()
    assert packet_io.digest(raw) == INPUT_SHA256
    inputs = json.loads(raw)
    command = inputs['snapshots']['command_applied']
    a = command['actor']
    continuation = dict(
        provenance=dict(source=command['source'], source_sha256=command['source_sha256'],
                        boundary=inputs['provenance']),
        command_frame=command['frame'] - 1, rng=command['rng'],
        actor=dict(random_phase_raw_u16=a['techno_ctor_random_word']))
    q = owner.Mission(continuation)
    observed = []

    def raw_word(u, pc, _size, _data):
        if q.phase not in ('logic', 'supplied_external_next') or pc not in (0x65C79D, 0x65C84B):
            return
        ptr = u.reg_read(UC_X86_REG_ECX if pc == 0x65C79D else UC_X86_REG_EDX)
        observed.append(dict(frame=q.frame, phase=q.phase, pc=hex(pc),
                             stream=next(k for k, p in q.resident.rngs.items() if p == ptr),
                             before_indices=[q.m.read32(ptr + 4), q.m.read32(ptr + 8)],
                             value=u.reg_read(UC_X86_REG_ESI)))

    q.u.hook_add(UC_HOOK_CODE, raw_word)
    q.setup()
    # Recorded preceding Guard state is a supplied continuation boundary. The
    # actual queued Attack, target and first firing state still come from Event.
    m = a['mission']
    q.u.mem_write(q.src + 0xC4, dwords(m['ai_counter']))
    q.u.mem_write(q.src + 0xC8, dwords(m['dispatch_timer']['start_frame']))
    q.u.mem_write(q.src + 0xD0, dwords(m['dispatch_timer']['delay']))
    q.u.reg_write(UC_X86_REG_ESP, SP)
    q.u.reg_write(UC_X86_REG_EDI, 0)
    run_checked(q.u, 0x55DE73, 0x55DE87)
    q.frame = q.m.read32(0xA8ED84)
    boundary = dict(actor_difference=difference(native_actor(q), production_actor(command)),
                    rng_equal=rng_state(q) == command['rng'])
    assert boundary == dict(actor_difference={}, rng_equal=True), boundary
    frames, snapshots = [], {}
    for frame in inputs['frames']:
        assert q.frame == frame['logic_frame']
        start, event_start = len(observed), len(q.events)
        external = [r for r in frame['draws'] if r['supplied_external']]
        assert frame['draws'][:len(external)] == external, 'Recorded external calls must be a prefix'
        q.phase = 'supplied_external_next'
        for expected in external:
            before = rng_state(q)['scenario']
            assert [before['index_a'], before['index_b']] == expected['before_indices']
            actual = q.m.invoke(0x65C780, q.resident.rngs['scenario'])
            # invoke stops at the sentinel; this observer row has no native
            # caller to visit. Record the returned register without changing it.
            request = q.pending.pop(RET_MAGIC)
            request['returned_eax'] = actual
            assert actual == expected['value'], (q.frame, actual, expected)
        q.tick()
        words = observed[start:]
        expected_words = [{k: r[k] for k in ('before_indices', 'value')} for r in frame['draws']]
        actual_words = [{k: r[k] for k in ('before_indices', 'value')} for r in words]
        assert actual_words == expected_words, (frame['logic_frame'], actual_words, expected_words)
        assert all(r['stream'] == 'scenario' for r in words)
        frames.append(dict(logic_frame=frame['logic_frame'], supplied_external_count=len(external),
                           native_draws=words, requests=[r for r in q.events[event_start:]
                           if r['kind'] == 'rng'], actor=native_actor(q)))
        for name in ('first_tick', 'first_impact'):
            p = inputs['snapshots'][name]
            if q.frame == p['frame']:
                actual_rng = rng_state(q)
                native_cell = q.resident.snapshot(q.resident.ptrs[87, 54])
                comparison = dict(native=native_actor(q), production=production_actor(p),
                                  actor_difference=difference(native_actor(q), production_actor(p)),
                                  native_rng=actual_rng, rng_equal={k: actual_rng[k] == p['rng'][k]
                                                                  for k in actual_rng},
                                  native_cell=native_cell, production_cell=p['target_cell'],
                                  cell_equal={k: native_cell[k] == p['target_cell'][k]
                                              for k in ('overlay', 'land', 'level')})
                assert not comparison['actor_difference'], comparison['actor_difference']
                assert all(comparison['rng_equal'].values()), comparison['rng_equal']
                assert all(comparison['cell_equal'].values()), comparison['cell_equal']
                snapshots[name] = comparison
    assert set(snapshots) == {'first_tick', 'first_impact'}
    assert len(q.shots) == len(q.impacts) == 1
    assert not q.pending, q.pending
    text_hash = packet_io.digest(bytes(q.u.mem_read(0x401000, 0x3E0000)))
    assert text_hash == q.resident.code_hash
    return dict(schema=1, text_sha256=text_hash,
                input_sha256=INPUT_SHA256, boundary=boundary, frames=frames,
                snapshots=snapshots, shots=q.shots, impacts=q.impacts,
                effects=[r for r in q.events if r.get('frame', 0) >= 1 and r['kind'] in
                         ('select_anim', 'anim_ctor', 'sound_boundary')])


def metadata():
    result = owner.metadata()
    result['scope'] = __doc__
    result['assumptions'].extend([
        'Input records selected actor fields, full three-stream RNG states, and compact true-Next receipts from five SHA256-pinned production-v8 files. Each receipt retains original caller function/source location, pre-draw indices and raw value; full backtraces and unrelated world exports stay external.',
        'Original command/native source construction and placement execute before importing recorded retained RNG and constructor phase. The prior Guard counter1/dispatch(0,28) is supplied. No firing timer, launched projectile, collision or bridge result is supplied.',
        'Every native raw word is observed before its state-word store, at65C79D or65C84B, including native range retries. Matching includes all raw words/indices in Logic1..11 and full three-stream states plus selected actor fields at committed frames2/12.',
    ])
    result['substitutions'].append(
        'Each production Terrain prefix executes original65C780 directly in recorded order. Terrain AI bodies, their admission/cadence, other world actors and whole ScenarioLoad are not executed. This proves selected-owner agreement conditional on recorded global interleaving, not whole-world RNG scheduling parity.')
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'mission_interleaving.json', provenance=metadata)
