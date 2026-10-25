"""Extract a declared outside-call schedule using observed Logic ownership.

This helper consumes production diagnostics only to declare constructor inputs
and outside raw calls. It never supplies selected-object state or expected native
outcomes. The native generator imports neither this module nor production data.
"""
from pathlib import Path
import re

REPO = Path(__file__).resolve().parents[3]
SCENARIO_PRODUCERS = (
    'src/sim/ore_growth.rs:', 'src/sim/tiberium/mod.rs:',
    'src/sim/terrain_spawn.rs:', 'src/sim/infantry.rs:',
    'src/sim/world/techno_ai/building_missions.rs:',
    'src/sim/world/techno_ai/mission_handlers.rs:',
    'src/sim/world/techno_ai/target_scan.rs:', 'src/sim/anim_class.rs:',
    'src/sim/mission/authority.rs:', 'src/sim/combat/rof.rs:',
    'src/sim/world/bridge_damage_dispatch.rs:',
    'src/sim/projectile.rs:', 'src/sim/combat/inviso_scatter.rs:',
)
OUTSIDE_GLOBAL_PRODUCERS = (
    'src/sim/ore_growth.rs:', 'src/sim/tiberium/mod.rs:', 'src/sim/infantry.rs:',
)


def selected_logic_ids(production):
    sources = {row['stable_id'] for row in production['diagnostics']}
    assert len(sources) == 1
    result = {next(iter(sources)): 'FV'}
    for row in production['diagnostics']:
        for bullet in row['bullets']:
            assert bullet['source_id'] in sources
            prior = result.setdefault(bullet['id'], 'selected Bullet')
            assert prior == 'selected Bullet'
        for anim in row['anims']:
            prior = result.setdefault(anim['stable_id'], 'selected new Anim')
            assert prior == 'selected new Anim'
    return result


def source_line(site, receipts=None, source_root=REPO):
    file, line, column = site.rsplit(':', 2)
    if receipts is not None:
        key = (file, int(line), int(column))
        assert key in receipts, ('Missing sealed historical producer receipt', site)
        return receipts[key]
    path = source_root / file
    raw = path.read_bytes()
    return dict(file=file, line=int(line), column=int(column),
                source=raw.decode().splitlines()[int(line)-1])


def classify(production, frame, receipt, selected=None, *, receipts=None, source_root=REPO):
    """Object identity decides ownership; caller source decides RNG stream."""
    if selected is None:
        selected = selected_logic_ids(production)
    paths = re.findall(r'at \./(src/[^\n]+)', receipt['callers'])
    assert paths, receipt
    producer = next(path for path in paths if not path.startswith('src/sim/rng.rs:'))
    context = receipt['logic_object']
    if context is None:
        assert producer.startswith(OUTSIDE_GLOBAL_PRODUCERS), ('unattributed global producer', frame, producer)
        outside = True
        reason = 'global producer outside the live FV/Bullet/Anim pass'
    else:
        outside = context not in selected
        reason = 'outside Logic object' if outside else selected[context]
    if not outside:
        # Selected calls stay in original execution. Their stream/value does not
        # enter the supplied schedule, so no later checkout source is consulted.
        return False, None, reason, paths, None
    proof = source_line(producer, receipts, source_root)
    if producer.startswith('src/sim/world/mod.rs:') and 'self.main_rng.next_u32()' in proof['source']:
        stream = 'main'
        reason += '; Foot MoveSound Main Next'
    else:
        assert producer.startswith(SCENARIO_PRODUCERS), ('unattributed RNG stream', frame, producer, proof)
        stream = 'scenario'
    return outside, stream, reason, paths, proof


def schedule(production, *, receipts=None, source_root=REPO):
    selected = selected_logic_ids(production)
    result = []
    for frame in production['draw_trace']:
        rows = []
        for index, receipt in enumerate(frame['draws']):
            outside, stream, reason, paths, proof = classify(production, frame['frame'], receipt, selected, receipts=receipts, source_root=source_root)
            rows.append(dict(index=index, supplied_external=outside, stream=stream,
                             reason=reason, callers=paths, source_proof=proof,
                             logic_object=receipt['logic_object'],
                             before_indices=receipt['before_indices'], value=receipt['value']))
        native = [i for i, row in enumerate(rows) if not row['supplied_external']]
        if native:
            begin, end = native[0], native[-1]+1
            assert all(not row['supplied_external'] for row in rows[begin:end]), (
                'Outside calls interleave selected calls inside one native pass', frame['frame'])
        else:
            begin = end = len(rows)
        assert all(row['supplied_external'] for row in rows[:begin]+rows[end:])
        result.append(dict(logic_frame=frame['frame'], rows=rows,
                           prefix=begin, selected=end-begin, suffix=len(rows)-end))
    return result
