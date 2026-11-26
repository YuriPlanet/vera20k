"""The scale battle: armies a match map fields and keeps by its own triggers.

`python -m tools.scale_map <out.map> --starts N --battle` appends what
`battle_sections` writes to the match map. Played as an ordinary skirmish with
a house at every start, the map then holds a battle that goes on for as long
as the match runs, with no command from outside: sim-bench's `battle`
scenario times it.

Who gets what army. Every start's house gets `units_per_house` units in
squads of SQUAD, by the per-side mixes of src/bin/sim_bench_match.rs. A map
cannot know which country sits at a start, and a reinforcement constructs its
members without asking what the house may build, so the mix goes by the
start's place in the grid: the four neighbours of a start field the two other
mixes.

How a squad comes and what it does. A squad is two teams with one script: a
marker (one unit, the squad's commonest infantryman or vehicle by turns) and
a body (the other nine). Trigger action 80 reinforces both: the first squads
a few cells short of a front, from second FIRST_WAVE_SECOND on, later ones
beside the house's start. A front is the line between two neighbouring
starts; it has LANES waypoints that both houses use. A squad has a post, a
front and a lane, and its script patrols (action 16) to its lane's waypoint,
on to the waypoint two lanes along, and back (action 6). Patrolling, a team's
leader scans its own threat range every [AI] PatrolScan (14 frames, each team
at its own phase of them where scans are batched: actions.rs:1491) and the
team attacks what it finds, and a computer house's members also acquire
targets themselves while they move (`Aggressive=yes`). Both houses of a front
aim at the same waypoints, so they keep meeting.

How a death is replaced. Only the marker's team has a Tag: one event, 48
"destroyed by anything", repeat 0. When the marker dies its Trigger
reinforces a new marker and a new body at home, and they patrol out to the
post. What is left of the old body fights on and is not replaced. A squad is
so replaced once in a member's lifetime, and the army keeps to its size on
average without one Tag to a unit; there is one live Tag to a squad. The
markers used here outlive their squads a little, and every first squad meets
the enemy at once, so the army dips to seven eighths of its size by tick
1,200 and then climbs back: UNITS_PER_HOUSE allows for the dip. Two squads of
a house with the same members and post share their TeamTypes; every team
still gets a Tag of its own (lifecycle.rs:607).

Why not exactly one for one. A Tag reaches a unit only through its team
(lifecycle.rs:595, membership.rs:393), a computer house's unit loses it when
it leaves that team (membership.rs:443), and a Tag that is to spring again
must be a new one: a one-shot Tag leaves all its objects when it springs
(dispatch.rs:781), and a repeating Tag (repeat 2) keeps event 48 as occurred
after its first death (events.rs:564, lifecycle.rs:449) and then springs at
every event a member raises, each cell it enters among them. Exact
replacement therefore takes one team and one live Tag to a unit, and at
20,000 units their upkeep is most of a frame: every team is visited every
frame (where scans are batched, one in four unless it cannot wait:
team_ai.rs:216), every death walks every team (sim/team_script_vm.rs:1378),
and every emptied team that held an infantryman raises event 23 on every
live Tag (team_ai.rs:486, infantry_action.rs:498, dispatch.rs:159).

Engine facts this relies on. Paths are under src/; lifecycle.rs, dispatch.rs
and events.rs are in sim/trigger_runtime/, membership.rs, team_ai.rs,
actions.rs and reinforcements.rs in sim/team_script_vm/, infantry_action.rs
in sim/movement/, the house_*.rs in sim/ and house_defeat.rs in sim/world/.
- TeamType `House=<Player @ A>` is the house at start waypoint 0, and so on
  through `<Player @ AD>` (rules/native_processing.rs:200,
  reinforcements.rs:43). A pre-placed unit cannot name one.
- Action 80 constructs the TaskForce's members and puts them all on one cell,
  the passable cell nearest the waypoint whatever stands there
  (reinforcements.rs:728 and :951, sim/find_nearby_cell.rs:823). Its team
  operand is a name only from three characters up (map/actions.rs:82), its
  waypoint is letters, A = 0 to ZZ = 701 (map/actions.rs:170).
- A physical INI line is cut at 511 bytes (rules/ini_parser.rs:50), so a
  Trigger holds some seventeen such actions. A TaskForce has six entries and
  a ScriptType fifty lines (rules/team_ai_ini.rs:19).
- Event 13 counts seconds of 15 frames from the load
  (lifecycle.rs:441). Event 48 is raised on a dying
  object's Tag (sim/combat/record_kill.rs:506 and :636).
- A team whose script loops never ends; one whose last member dies is
  destroyed (team_ai.rs:486). Action 16: actions.rs:1406; a leg ends when
  every member is within [General] Stray of the waypoint (actions.rs:2534),
  and a squad that cannot gather there holds that waypoint. Aggressive
  members acquire on the move: sim/world/techno_ai/target_scan.rs:451.
- `Priority=` above every AIMD team's keeps a computer house's own teams
  from recruiting the members (membership.rs:306) and its base defense from
  emptying the team (membership.rs:773); either takes the Tag off them.
- In a stock Battle every computer house allies with every other as soon as
  one deploys its yard (house_diplomacy.rs:467, sim/world/world_spawn.rs:2539)
  and they fight only the human. `[General] AINearestEnemy=yes`
  (docs/ai-nearest-enemy.md) turns that off (house_diplomacy.rs:483).
- With the lobby's Short Game a house without a building or an MCV is
  defeated and blown up (house_defeat.rs:492, house_tracking.rs:1134);
  the match is decided when the local player falls or one side is left
  (house_defeat.rs:681) and its frames then stop (sim/world/master_frame.rs:780).
  A computer house without a `Factory=` building also takes every unit out
  of its team (house_strategy.rs:161 and :432). So the construction yards and
  MCVs are made permanent: `Immune=` (sim/combat/receiver_call.rs:563) and
  `LegalTarget=no` (sim/combat/greatest_threat.rs:2757). A computer house's
  first strategy visits find no factory yet and do the same, which is why
  the first squads wait for FIRST_WAVE_SECOND.
- A Trigger's house is looked up by country at every event its Tag hears
  (events.rs:519). Nothing here reads it, and a name that is
  no country ends the lookup soonest.
- Waypoint keys run from 0 to 701 (map/waypoints.rs:72); 0 to 7 are starts
  to the lobby (map/waypoints.rs:42) and 699 is the default HomeCell
  (map/basic.rs:240).

The same arguments always write the same bytes: nothing here is drawn.
"""

import math

# 30 houses field 23,400 units. The battle must hold 20,000 mobile objects in
# every 100 ticks of sim-bench's measured window, and the army dips to about
# seven eighths of its size around tick 1,200 before it climbs back (20,926
# mobile objects at the lowest in a 3,000-tick run).
UNITS_PER_HOUSE = 780
# Units in a squad: its marker and nine others. One live Tag to ten units.
SQUAD = 10

# The mixes of src/bin/sim_bench_match.rs (ALLIED, SOVIET, YURI `army`), less
# what is not a plain ground fighter: engineers, mind controllers, Chrono
# Legionnaire, Crazy Ivan, Terrorist, Demolition Truck, the drones, the
# Magnetron, the flyers, and the V3, whose rocket is a second object.
ARMIES = (
    (('E1', 30), ('GGI', 8), ('ADOG', 2), ('SNIPE', 3),
     ('MTNK', 24), ('FV', 12), ('SREF', 6), ('MGTK', 4), ('TNKD', 6)),
    (('E2', 30), ('FLAKT', 8), ('DOG', 2), ('SHK', 5), ('DESO', 3),
     ('HTNK', 24), ('HTK', 8), ('APOC', 5), ('TTNK', 6)),
    (('INIT', 30), ('BRUTE', 10), ('VIRUS', 3), ('LTNK', 26), ('YTNK', 10)),
)
INFANTRY = frozenset(('E1', 'GGI', 'ADOG', 'SNIPE', 'E2', 'FLAKT', 'DOG', 'SHK', 'DESO',
                      'INIT', 'BRUTE', 'VIRUS'))

# Waypoints on a front, and cells between them: a line of 28 cells, shorter
# than the shortest front of the 30-start map (59 cells).
LANES = 8
LANE_SPREAD = 4
# A squad patrols from its lane's waypoint to the one this many lanes along.
PATROL_STRIDE = 2
# The first squads arrive this many cells short of their front, on their own
# side, so two neighbours' squads are sixteen cells apart: they meet at once.
STAGE_BACK = 8
STAGING_PER_FRONT = 2
# A reinforcement puts its team on one cell, so the first squads come in
# waves: at most this many squads to a staging waypoint in one second.
SQUADS_PER_WAVE = 3
# Seconds (of 15 frames) before the first wave: past a computer house's
# strategy visits at frames 0 and 106 to 112, which take every unit out of
# its team while the house has no factory. Its yard is up before frame 100.
FIRST_WAVE_SECOND = 8
# Replacements arrive this many cells from the start, on the side away from
# its ore: the start's own cell is under the construction yard.
HOME_DISTANCE = 12
HOME_WAYPOINTS = 2
# Cells kept between a waypoint and the map's edge.
MAP_MARGIN = 8

# Above every AIMD team's Priority (50 at most) and [General] SuspendPriority.
TEAM_PRIORITY = 100
# No country: see the module doc.
TRIGGER_OWNER = 'X'
# Event 48, "destroyed by anything": COUNT,EVENT,PARAMETER TYPE,VALUE.
DESTROYED = '1,48,0,0'

# Rules the map sets for itself; its INI is the last Rules layer.
RULES = (
    ('General', ('AINearestEnemy=yes',)),
    ('AMCV', ('Immune=yes', 'LegalTarget=no', 'ImmuneToPsionics=yes')),
    ('SMCV', ('Immune=yes', 'LegalTarget=no', 'ImmuneToPsionics=yes')),
    ('PCV', ('Immune=yes', 'LegalTarget=no', 'ImmuneToPsionics=yes')),
    ('GACNST', ('Immune=yes', 'LegalTarget=no', 'ImmuneToPsionics=yes', 'Capturable=no')),
    ('NACNST', ('Immune=yes', 'LegalTarget=no', 'ImmuneToPsionics=yes', 'Capturable=no')),
    ('YACNST', ('Immune=yes', 'LegalTarget=no', 'ImmuneToPsionics=yes', 'Capturable=no')),
)

# Waypoints 0 to 7 are starts wherever a map has them: the lobby counts them
# for the map's players (map/waypoints.rs:42). The battle's own begin after
# them and after the map's starts, and keep off 98 and 99, which campaign
# maps give a meaning, and off the default HomeCell.
FIRST_WAYPOINT = 8
LAST_WAYPOINT = 701
SKIPPED_WAYPOINTS = frozenset((98, 99, 699))
INI_LINE_LIMIT = 500
TASK_FORCE_ENTRIES = 6


def waypoint_letters(index: int) -> str:
    """A = 0 to Z = 25, then AA = 26 to ZZ = 701."""
    if index < 26:
        return chr(65 + index)
    index -= 26
    return chr(65 + index // 26) + chr(65 + index % 26)


def smooth_mix(mix, count: int) -> list[str]:
    """`count` types by smooth weighted round robin, so that every stretch of
    the list holds the mix (sim_bench_match's army_types)."""
    total = sum(weight for _, weight in mix)
    credit = [0] * len(mix)
    out = []
    for _ in range(count):
        for index, (_, weight) in enumerate(mix):
            credit[index] += weight
        pick = max(range(len(mix)), key=lambda index: (credit[index], -index))
        credit[pick] -= total
        out.append(mix[pick][0])
    return out


def task_force_entries(types: list[str]) -> tuple[tuple[int, str], ...]:
    """A team's TaskForce entries, the commonest type first; the rarest are
    folded into the commonest while there are more than a TaskForce holds."""
    counts: dict[str, int] = {}
    for name in types:
        counts[name] = counts.get(name, 0) + 1
    order = sorted(counts, key=lambda name: (-counts[name], types.index(name)))
    while len(order) > TASK_FORCE_ENTRIES:
        counts[order[0]] += counts.pop(order.pop())
    return tuple((counts[name], name) for name in order)


def reinforce(team: str, waypoint: int) -> str:
    """Trigger action 80, reinforcement at a waypoint: eight fields."""
    return f'80,1,{team},0,0,0,0,{waypoint_letters(waypoint)}'


class _Ground:
    """The starts' grid, and the cells waypoints take."""

    def __init__(self, size: int, starts: list[tuple[int, int]], blocked: set[tuple[int, int]]):
        self.size = size
        self.starts = starts
        # tools.scale_map.grid_starts fills a grid of this many columns row by row.
        self.columns = math.ceil(math.sqrt(len(starts)))
        self.taken = set(blocked) | set(starts)
        self.fronts: dict[tuple[int, int], list[tuple[int, int]]] = {}

    def grid(self, slot: int) -> tuple[int, int]:
        return divmod(slot, self.columns)

    def neighbours(self, slot: int) -> list[int]:
        """The grid neighbours, in turn round the start."""
        row, column = self.grid(slot)
        found = []
        for d_row, d_column in ((0, 1), (1, 0), (0, -1), (-1, 0)):
            r, c = row + d_row, column + d_column
            if r >= 0 and 0 <= c < self.columns and r * self.columns + c < len(self.starts):
                found.append(r * self.columns + c)
        return found

    def claim(self, x: float, y: float) -> tuple[int, int]:
        """The nearest cell to (x, y) inside the Size diamond (as
        tools.scale_map.in_diamond), MAP_MARGIN clear of its edge, with no
        ore, tree or other waypoint."""
        size, cx, cy = self.size, round(x), round(y)
        for reach in range(2 * size):
            ring = [(cx + dx, cy + dy)
                    for dy in range(-reach, reach + 1) for dx in range(-reach, reach + 1)
                    if max(abs(dx), abs(dy)) == reach]
            ring.sort(key=lambda cell: (math.dist(cell, (x, y)), cell[1], cell[0]))
            for cell in ring:
                if (cell not in self.taken
                        and size + MAP_MARGIN < cell[0] + cell[1] <= 3 * size - MAP_MARGIN
                        and abs(cell[0] - cell[1]) < size - MAP_MARGIN):
                    self.taken.add(cell)
                    return cell
        raise ValueError('the map has no free cell for a waypoint')

    def front(self, slot: int, neighbour: int) -> list[tuple[int, int]]:
        """The LANES cells of the front between two neighbours, the same for both."""
        key = (min(slot, neighbour), max(slot, neighbour))
        if key not in self.fronts:
            a, b = self.starts[key[0]], self.starts[key[1]]
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy)
            along = (-dy / length, dx / length)
            self.fronts[key] = [
                self.claim((a[0] + b[0]) / 2 + offset * along[0],
                           (a[1] + b[1]) / 2 + offset * along[1])
                for offset in ((lane - (LANES - 1) / 2) * LANE_SPREAD for lane in range(LANES))]
        return self.fronts[key]

    def staging(self, slot: int, neighbour: int) -> list[tuple[int, int]]:
        """Where the house's first squads for a front arrive, spread along it."""
        start, other = self.starts[slot], self.starts[neighbour]
        dx, dy = other[0] - start[0], other[1] - start[1]
        length = math.hypot(dx, dy)
        back = ((start[0] + other[0]) / 2 - STAGE_BACK * dx / length,
                (start[1] + other[1]) / 2 - STAGE_BACK * dy / length)
        along = (-dy / length, dx / length)
        span = (LANES - 1) * LANE_SPREAD
        return [self.claim(back[0] + offset * along[0], back[1] + offset * along[1])
                for offset in ((index + 0.5) / STAGING_PER_FRONT * span - span / 2
                               for index in range(STAGING_PER_FRONT))]

    def homes(self, slot: int) -> list[tuple[int, int]]:
        """Beside the start, toward -v (rx + ry falling): the ore lies toward
        +v. The homes fan a quarter turn round that direction."""
        start = self.starts[slot]
        cells = []
        for index in range(HOME_WAYPOINTS):
            angle = (0.0 if HOME_WAYPOINTS == 1
                     else math.pi / 2 * index / (HOME_WAYPOINTS - 1))
            cells.append(self.claim(start[0] - HOME_DISTANCE * math.cos(angle),
                                    start[1] - HOME_DISTANCE * math.sin(angle)))
        return cells


def battle_sections(size: int, starts: list[tuple[int, int]], blocked: set[tuple[int, int]],
                    units_per_house: int = UNITS_PER_HOUSE,
                    ) -> tuple[dict[int, tuple[int, int]], str]:
    """The battle for a match map of `size` whose start waypoints 0.. are
    `starts`, in tools.scale_map.grid_starts' order, and whose cells in
    `blocked` hold ore or a tree.

    Returns the waypoints to add to the map's own, by number, and the
    sections to append to the map: the Rules it sets, [TaskForces],
    [ScriptTypes], [TeamTypes], [Triggers], [Events], [Actions] and [Tags],
    each with its entries' own sections.
    """
    if len(starts) < 2:
        raise ValueError('a battle needs two starts')
    ground = _Ground(size, starts, blocked)
    free = (index for index in range(max(len(starts), FIRST_WAYPOINT), LAST_WAYPOINT + 1)
            if index not in SKIPPED_WAYPOINTS)
    numbered: dict[tuple[int, int], int] = {}

    def waypoint(cell: tuple[int, int]) -> int:
        if cell not in numbered:
            try:
                numbered[cell] = next(free)
            except StopIteration:
                raise ValueError('the battle needs more waypoints than a map has') from None
        return numbered[cell]

    task_forces: dict[tuple, str] = {}
    scripts: dict[str, list[str]] = {}
    team_types: dict[str, list[str]] = {}
    triggers, events, actions, tags = [], [], [], []

    def task_force(entries: tuple[tuple[int, str], ...]) -> str:
        return task_forces.setdefault(entries, f'TF{len(task_forces):03}')

    def trigger(name: str, repeat: int, event: str, does: list[str]) -> str:
        """One Tag G<name> with one Trigger R<name>; returns the Tag."""
        line = f'R{name}={len(does)},' + ','.join(does)
        assert len(line) <= INI_LINE_LIMIT
        # ID=HOUSE,LINKED TRIGGER,NAME,DISABLED,EASY,MEDIUM,HARD,TRANSFER
        triggers.append(f'R{name}={TRIGGER_OWNER},<none>,{name},0,1,1,1,0')
        events.append(f'R{name}={event}')
        actions.append(line)
        # ID=REPEAT,NAME,TRIGGER
        tags.append(f'G{name}={repeat},{name},R{name}')
        return f'G{name}'

    first: dict[tuple[int, int], list[str]] = {}    # (second, slot): actions
    for slot in range(len(starts)):
        row, column = ground.grid(slot)
        mix = ARMIES[(row + column) % len(ARMIES)]
        units = smooth_mix(mix, units_per_house)
        neighbours = ground.neighbours(slot)
        posts = [(neighbour, lane) for lane in range(LANES) for neighbour in neighbours]
        homes = [waypoint(cell) for cell in ground.homes(slot)]
        staging = {neighbour: [waypoint(cell) for cell in ground.staging(slot, neighbour)]
                   for neighbour in neighbours}
        arrived = dict.fromkeys(neighbours, 0)
        house = f'<Player @ {waypoint_letters(slot)}>'
        templates: dict[tuple, int] = {}
        for squad, begin in enumerate(range(0, len(units), SQUAD)):
            members = units[begin:begin + SQUAD]
            post = squad % len(posts)
            neighbour, lane = posts[post]
            home = homes[post % len(homes)]
            # The marker: the squad's commonest infantryman or vehicle, by turns.
            wanted = (squad + squad // len(posts)) % 2 == 0
            choice = [name for name in members if (name in INFANTRY) == wanted] or members
            marker = max(dict.fromkeys(choice), key=choice.count)
            body = list(members)
            body.remove(marker)
            entries = task_force_entries(body)
            template = templates.setdefault((marker, entries), len(templates))
            key = f'{slot:02}_{template:02}_{post:02}'
            arriving = [f'TM{key}'] + ([f'TB{key}'] if body else [])
            if f'TM{key}' not in team_types:
                script = f'SC{slot:02}_{neighbour:02}_{lane}'
                if script not in scripts:
                    front = ground.front(slot, neighbour)
                    scripts[script] = [
                        f'0=16,{waypoint(front[lane])}',
                        f'1=16,{waypoint(front[(lane + PATROL_STRIDE) % LANES])}',
                        '2=6,1',
                    ]
                common = [
                    f'House={house}',
                    f'Script={script}',
                    f'Waypoint={waypoint_letters(home)}',
                    f'Priority={TEAM_PRIORITY}',
                    'AreTeamMembersRecruitable=no',
                    'Aggressive=yes',
                ]
                if body:
                    team_types[f'TB{key}'] = [f'TaskForce={task_force(entries)}', *common]
                tag = trigger(f'M{key}', 0, DESTROYED,
                              [reinforce(team, home) for team in arriving])
                team_types[f'TM{key}'] = [f'TaskForce={task_force(((1, marker),))}', *common,
                                          f'Tag={tag}']
            cells = staging[neighbour]
            second = FIRST_WAVE_SECOND + arrived[neighbour] // (len(cells) * SQUADS_PER_WAVE)
            cell = cells[arrived[neighbour] % len(cells)]
            arrived[neighbour] += 1
            first.setdefault((second, slot), []).extend(reinforce(team, cell) for team in arriving)

    # The first squads: one-shot Tags on event 13, "elapsed time", as many
    # actions to a Trigger as its line holds.
    for (second, slot), waiting in sorted(first.items()):
        part = 0
        while waiting:
            name = f'W{slot:02}_{second:03}_{part}'
            taken, length = [], len(f'R{name}=99,')
            while waiting and length + len(waiting[0]) + 1 <= INI_LINE_LIMIT:
                length += len(waiting[0]) + 1
                taken.append(waiting.pop(0))
            trigger(name, 0, f'1,13,0,{second}', taken)
            part += 1

    out = []
    for section, lines in RULES:
        out += [f'[{section}]', *lines, '']
    out += ['[TaskForces]', *(f'{index}={name}' for index, name in enumerate(task_forces.values()))]
    for entries, name in task_forces.items():
        out += ['', f'[{name}]',
                *(f'{index}={count},{kind}' for index, (count, kind) in enumerate(entries))]
    out += ['', '[ScriptTypes]', *(f'{index}={name}' for index, name in enumerate(scripts))]
    for name, lines in scripts.items():
        out += ['', f'[{name}]', *lines]
    out += ['', '[TeamTypes]', *(f'{index}={name}' for index, name in enumerate(team_types))]
    for name, lines in team_types.items():
        out += ['', f'[{name}]', *lines]
    out += ['', '[Triggers]', *triggers, '', '[Events]', *events,
            '', '[Actions]', *actions, '', '[Tags]', *tags]
    waypoints = {index: cell for cell, index in sorted(numbered.items(), key=lambda item: item[1])}
    return waypoints, '\n'.join(out) + '\n'
