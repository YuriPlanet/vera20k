"""Original Unit admission at playfield boundaries through Foot+320."""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.unit_entry import query


def generate():
    cases = []
    for bounds in ([16, 0, 0, 16, 16], [16, 0, 3, 16, 16], [16, 0, 2, 16, 16]):
        for mode in (False, True):
            for in_playfield in (False, True):
                for mission_only in (False, True):
                    for mission, queued in ((5, -1), (4, -1), (-1, 4)):
                        cases.append(dict(bounds=bounds, game_mode_nonzero=mode, in_playfield=in_playfield,
                                          mission_only=mission_only, mission=mission, queued=queued))
    for level in (255, 0, 1, 4):
        for slope in (0, 1):
            for mission in (5, 4):
                cases.append(dict(bounds=[16, 0, 2, 16, 16], in_playfield=True, mission=mission,
                                  cells=[[11, 10, level, 0]], slopes=[[11, 10, slope]]))
    for active in (False, True):
        for cursor in (-1, 0, 1):
            for mission in (5, 4):
                cases.append(dict(bounds=[16, 0, 3, 16, 16], in_playfield=True, mission=mission,
                                  team=dict(active=active, cursor=cursor, action=2)))
    return [query(dict(case, trace_boundary=True)) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='100 complete Unit73F0A0 calls through original retained-Cell578540 and Foot4DA1D0: GameMode, retained3D4/3D5, current/queued Retreat, signed level/slope boundaries and non-action3 Team exits. Excludes IsTrain and active action3 Team waypoint/lifecycle branches.',
        entry_points={'unit_entry': 0x73F0A0, 'retained_cell_bounds': 0x578540, 'foot_edge': 0x4DA1D0, 'team_edge': 0x6EC300},
        assumptions=['Real Unit/Drive and map/type/house fixture from unit_entry. Map LocalSize fields are supplied raw, not clipped by a loader. Target Cell11,10 and retained flags/missions are supplied prestates.',
                     'Team cases execute original Script readers with one non-action3 record; outcomes are independent of7F. No production Team activation, waypoints, IsTrain or boundary-dummy proof.'],
        substitutions=['Only OS Interlocked imports inherited from the Unit/Drive fixture. No gameplay substitution.']))
