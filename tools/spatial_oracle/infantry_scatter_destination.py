"""Original damage-form Scatter through the full ordinary Walk destination chain."""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.infantry_source_scatter import query


def generate():
    cases = [
        dict(process_probe=True), dict(power_off=True), dict(warp_in=True), dict(warp_out=True),
        dict(swap_active=True), dict(open_transport=True), dict(bunker=True), dict(mission=7),
        dict(head=[3200, 3200, 0], process_probe=True), dict(moving=True),
        dict(moving=True, power_off=True), dict(moving=True, raw=[[10, 10, 0x1C, 0]]),
        dict(moving=True, raw=[[10, 10, 0x20, 0]]),
        dict(moving=True, blocked_terrain=[[10, 10]]),
        dict(moving=True, cells=[[10, 10, 0, 0x100]], raw=[[10, 10, 0x20, 0]]),
        dict(moving=True, cells=[[10, 10, 0, 0x100]], raw=[[10, 10, 0, 0x20]]),
        dict(moving=True, swap_active=True),
        dict(cells=[[x, y, 0, 0x100] for x, y in
                    [(10, 9), (11, 9), (11, 10), (11, 11), (10, 11), (9, 11), (9, 10), (9, 9)]]),
    ]
    return [query(dict(case, live_entry=True, live_setter=True)) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Actual source-aware Infantry Scatter, CanEnter, QueueMission, Infantry/Foot destination setter, Walk MoveTo/Stop; supplied ordinary non-JumpJet Fraidycat. Optional separate next-Process probes stop at first FindPath or paid-step boundary.',
        entry_points={'scatter': 0x51D0D0, 'entry': 0x51BF90, 'infantry_set_destination': 0x51AA40,
                      'foot_set_destination': 0x4D94B0, 'walk_constructor': 0x75AA90,
                      'walk_move_to': 0x75ACB0, 'failed_path_receiver': 0x51DAF0,
                      'clear_cell': 0x4834A0, 'walk_stop': 0x75ADA0, 'walk_process': 0x75AEC0},
        assumptions=[
            'Inherits infantry_source_scatter live-entry fixture: widened synthetic playfield, 32x32 allocated original Cell vtables, empty lists/overlays, supplied raw occupation and nonzero land0/zero land1 speed rows. Original heading startup and seeded ScenarioRandom execute.',
            'Original Walk constructor and actual Infantry/Foot/Cell vtables. Linked owner and reference count1 supplied. No JumpJet, reciprocal Rocker/lift links, retained fire particles, EMP or +6AC suppression latch. Nonhuman Fraidycat avoids the unrelated human prone DoAction7 branch.',
            'Frame100 and blocked delay22; initial movement timer50/5, blocked timer40/6, retry count7, path backing2/3/4/5, reference9/8, null NavCom and aux123. Foot gate/warp bytes, optional paid head, power and moving prestates are supplied. Moving rows retain an old destination7808,2688,0.',
            'Failed-path receiver executes actual DoAction with supplied zero sequence counts (no animation acceptance), actual CanEnter and Stop. Initial +6DC is1 so receiver writes are observable. No invented result substituted for any gameplay callable.',
            'Source-aware Scatter never calls Process. Two explicit later probes stop at first FindPath or paid numeric approach; pathfinder core and complete movement are outside this native corpus.',
            'Original 6D1830/6D18C0/6D1BF0 initialize translation scale with FPCW0E7F, including actual structural-bridge destination rise414.',
        ],
        substitutions=['Only OS InterlockedIncrement/Decrement imports implement their pointed-count stdcall operations. No gameplay callable substituted.']))
