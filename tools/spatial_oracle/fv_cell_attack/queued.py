"""Original Unit/Foot Approach with a supplied valid retained Cell Nav queue.

The queue layout is a declared input boundary. Original 7414E0/4D5690 and
741970/4D94B0 execute the decision, false-argument setter, and caller pop/shift.
"""
from pathlib import Path
import struct

from . import admission
from .publication import finish_vectors
from tools.spatial_oracle.building_body_rules import dwords

HERE = Path(__file__).resolve().parent


class Queued(admission.Admission):
    def run(self, *args, **kwargs):
        queue = self.active_row.get('nav_queue', [])
        pointer = self.m.alloc(4 * max(1, len(queue)))
        for index, xy in enumerate(queue):
            self.u.mem_write(pointer + index * 4, dwords(self.cells[tuple(xy)]))
        self.u.mem_write(
            self.src + 0x58C,
            dwords(pointer, len(queue), 0x101, len(queue), 10),
        )
        return super().run(*args, **kwargs)

    def snapshot(self):
        result = super().snapshot()
        count = self.m.read32(self.src + 0x598)
        pointer = self.m.read32(self.src + 0x58C)
        assert count <= 16
        result['nav_queue_cells'] = [
            list(struct.unpack(
                '<hh',
                self.u.mem_read(self.m.read32(pointer + index * 4) + 0x24, 4),
            ))
            for index in range(count)
        ]
        return result


def generate():
    query = Queued(dict(name='prepare_healthy_queue', stage='healthy'))
    query.plane()
    query.construct(True)
    query.pathfinder()
    query.graphs()
    query.checkpoint()
    rows = [
        dict(name='queued_two_no_nav', source_y_delta=-2),
        dict(
            name='queued_two_retained_near_nav',
            source_y_delta=-2, retained_nav=[87, 49],
        ),
        dict(
            name='queued_two_retained_far_nav',
            source_y_delta=-2, retained_nav=[87, 47],
        ),
        dict(name='queued_two_already_in_range'),
    ]
    for row in rows:
        row.update(stage='healthy', nav_queue=[[87, 49], [87, 50]])
    try:
        return dict(
            schema=1,
            cases=[query.execute(row) for row in rows],
        )
    finally:
        query.close()


def metadata():
    result = admission.metadata()
    result['scope'] = __doc__
    # This four-row runner keeps invoke's ordinary budget; only the exhaustion
    # runner installs admission.generate's larger outer-call limit.
    result['outer_approach_instruction_limit'] = 2_000_000
    result['assumptions'] = [
        item for item in result['assumptions']
        if not item.startswith('Each control restores')
        and not item.startswith('Exhaustion supplies')
    ]
    result['assumptions'].append(
        'Two retained Cell pointers are supplied in a valid two-element Foot Nav '
        'queue at +58C with capacity=2, valid=1, allocated=1, count=2, '
        'increment=10. The original queue producer is not claimed. UnitApproach, '
        'destination setters and caller shift/pop execute; no returned call '
        'value is supplied. No active paid Drive head is seeded.'
    )
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'queued.json.gz', provenance=metadata)
