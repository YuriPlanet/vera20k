"""Original BB relayout/help and NewEdit Enter -> LAN chat send controls.

Reuses the shared shell window VM. UTF-16 editing/string operations, the
5EDD00 cap calculation, 4B3 getter and 5DE360 clear/admission run natively.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_FPCW

from tools.native_oracle import NATIVE_FPCW, OracleError, finish_vectors, provenance, run_checked, STACK_BASE, STACK_SIZE
from tools.rmg_oracle.gen_rng_vectors import seeded_struct, STRUCT_LEN
from tools.storage_oracle.shell_relayout import Image, Machine, PARENT, relayout
from tools.storage_oracle.shell_help_keys import help_label

NEW_EDIT = 0x614B30
LOBBY = 0x5DDBD0


class ChatMachine(Machine):
    def __init__(self, units):
        super().__init__(0xBB, 800, 600)
        self.chat = next(h for h in self.children if self.windows[h]["id"] == 0x540)
        self.messages = []
        self.cap_writes = []
        self.visits = {"enter_append": 0, "chat_cap": 0, "chat_send": 0}
        value = self.alloc(4)
        raw = self.alloc(2 * (len(units) + 1))
        self.uc.mem_write(raw, struct.pack("<" + "H" * (len(units) + 1), *units, 0))
        self.call(0x7B66D0, ecx=value, stack_args=(raw,))
        self.w32(self.record(self.chat) + 0x3C, value)
        self.w32(self.record(self.chat) + 0x54, 4)  # RT_DIALOG chat ES_MULTILINE.
        self.uc.mem_write(0xA8B380, b"Tester\0")
        self.w32(0xA8DA54, 1)  # Own lobby node, no transport destinations.
        self.w32(0xAC0264, 0)
        self.wide = value
        for address, name in Image(self.uc).imports().items():
            if name == "SendMessageA":
                self.w32(address, self.stub(self.send_message))
            elif name == "CallWindowProcA":
                self.w32(address, self.stub(lambda: self.ret(20, 0)))
        self.uc.hook_add(UC_HOOK_CODE, self.observe)

    def units_at(self, address):
        units = []
        while unit := struct.unpack("<H", self.uc.mem_read(address + 2 * len(units), 2))[0]:
            units.append(unit)
        return units

    def edit_units(self):
        pointer = self.r32(self.wide)
        return self.units_at(pointer) if pointer else []

    def send_message(self):
        window, message, _count, data = (self.arg(i) for i in range(4))
        if window == self.chat:
            if message == 0x4B2:
                # DefaultHandler owns message text transport; NewEdit owns
                # the resulting WideWstring assignment/limit/caret update.
                self.w32(self.record(window) + 0x28, data)
                self.cap_writes.append(self.units_at(data))
            if message not in (0xE, 0x4B2, 0x4B3):
                raise OracleError(f"unexpected chat message {message:#x}")
            self.uc.reg_write(UC_X86_REG_EIP, NEW_EDIT)
        elif window == PARENT and message == 0x111:
            self.uc.reg_write(UC_X86_REG_EIP, LOBBY)
        elif window == PARENT and message == 0x4DA:
            self.ret(16, 0)  # BB has no default-Enter handler.
        else:
            raise OracleError(f"unexpected message {window:#x}/{message:#x}")

    def observe(self, _uc, address, _size, _data):
        names = {0x614DEB: "enter_append", 0x5EDD00: "chat_cap", 0x5DE360: "chat_send"}
        if address in names:
            self.visits[names[address]] += 1
        if address == 0x5E2700:
            self.messages.append(self.units_at(self.arg(3)))
            self.ret(0)


def chat_case(name, units):
    machine = ChatMachine(units)
    machine.call(NEW_EDIT, stack_args=(machine.chat, 0x100, 13, 0), count=500_000)
    if any(machine.visits[key] != 1 for key in ("enter_append", "chat_cap", "chat_send")):
        raise OracleError(f"{name}: missing caller chain {machine.visits}")
    return dict(name=name, input=units, sent=machine.messages,
                edit_after=machine.edit_units(), writes=machine.cap_writes)


class AssignmentMachine(Machine):
    """Full original69B8C0, actual Battle vtable country callbacks and RNG."""

    def __init__(self, seed, humans, ai):
        super().__init__(0xBB, 800, 600)
        self.rng = self.r32(0xA8B230) + 0x218
        self.uc.mem_write(self.rng, seeded_struct(seed))
        mode = self.alloc(0x40)
        self.w32(mode, 0x7EE184)  # Battle ctor5C0DF8's original vtable.
        self.w32(0xA8B23C, mode)
        self.nodes = []
        pointers = self.alloc(4 * len(humans))
        for index, row in enumerate(humans):
            node = self.alloc(0x80)
            self.nodes.append(node)
            self.uc.mem_write(node, (f"Human{index}\0").encode("utf-16-le"))
            for offset, value in zip((0x4B, 0x4F, 0x53, 0x57), row):
                self.w32(node + offset, value)
            self.w32(node + 0x6B, -1 if row[0] == -3 or row[1] == -3 else 0)
            self.w32(pointers + index * 4, node)
        self.w32(0xA8DA78, pointers)
        self.w32(0xA8DA84, len(humans))
        for index in range(8):
            country, color = ai[index] if index < len(ai) else (-1, -1)
            self.w32(0xA8B29C + index * 4, country)
            self.w32(0xA8B2BC + index * 4, color)
        self.draws = []
        self.pending_return = None
        self.uc.hook_add(UC_HOOK_CODE, self.observe_assignment)

    def observe_assignment(self, uc, address, _size, _data):
        if address == 0x4068E0:
            self.ret(0)  # Debug output only; caller cleans variadic arguments.
        elif address == 0x65C7E0:
            if uc.reg_read(UC_X86_REG_ECX) != self.rng:
                raise OracleError("assignments changed Scenario RNG owner")
            self.draws.append(dict(low=self.arg(0), high=self.arg(1), raw=[]))
            self.pending_return = self.r32(uc.reg_read(UC_X86_REG_ESP))
        elif address == 0x65C84B:
            self.draws[-1]["raw"].append(uc.reg_read(UC_X86_REG_ESI))
        elif address == self.pending_return:
            self.draws[-1]["result"] = uc.reg_read(UC_X86_REG_EAX)
            self.pending_return = None

    def state(self):
        return dict(humans=[list(struct.unpack("<4i", self.uc.mem_read(node + 0x4B, 16))) for node in self.nodes],
                    ai=[list(struct.unpack("<i", self.uc.mem_read(0xA8B29C + index * 4, 4))) +
                        list(struct.unpack("<i", self.uc.mem_read(0xA8B2BC + index * 4, 4))) for index in range(8)],
                    rng_hex=bytes(self.uc.mem_read(self.rng, STRUCT_LEN)).hex())


def assignment_case(name, seed, humans, ai):
    machine = AssignmentMachine(seed, humans, ai)
    initial = machine.state()
    native_nodes = [bytes(machine.uc.mem_read(node, 0x80)).hex() for node in machine.nodes]
    machine.call(0x69B8C0, ecx=0xA8B238)
    return dict(name=name, seed=seed, initial=initial, input_node_hex=native_nodes,
                active_ai_count=sum(country not in (-1, -3) for country, _color in ai),
                active_ai_slots=[index for index, (country, _color) in enumerate(ai) if country not in (-1, -3)],
                result=machine.state(), draws=machine.draws)


def assignment_cases():
    return [
        assignment_case("source_order_before_color_sort", 1234,
                        [[0, -2, 6, -1], [0, -2, 1, -1]], [[-1, -1], [-1, -1], [-2, 3]]),
        assignment_case("observer_between_random_humans", 0,
                        [[0, -2, -1, -2], [-3, -1, -1, -2], [0, -2, -1, -2]], [[-1, -1], [-1, -1], [-1, -1], [-2, -2]]),
        assignment_case("explicit_human_and_ai_colors_force_retries", 1,
                        [[3, -1, 0, -1], [0, -2, -1, -2]], [[-1, -1], [-1, -1], [-2, -2], [9, 1], [2, 2], [7, 3], [4, 4]]),
        assignment_case("sparse_ai_raw_slots", 1234,
                        [[0, -2, 6, -1], [0, -2, 1, -1]], [[-1, -1], [-1, -1], [-2, -2], [-1, -1], [9, 3]]),
        assignment_case("all_explicit_no_draw", 0,
                        [[0, -1, 7, -1], [9, -1, 2, -1]], [[-1, -1], [-1, -1], [3, 5]]),
    ]



class HostSeedMachine(AssignmentMachine):
    def __init__(self, seed):
        super().__init__(seed, [], [])
        self.rng = 0x886B88
        self.uc.mem_write(self.rng, seeded_struct(seed))
        self.thread = self.alloc(0x100)
        self.w32(self.thread + 0x14, 73)
        self.history = []
        self.uc.hook_add(UC_HOOK_CODE, self.observe_seed)

    def observe_seed(self, uc, address, _size, _data):
        if address == 0x7D140B: self.ret(0, self.thread)
        elif address == 0x65C7E0: self.history.append("Main.RandomRanged")
        elif address == 0x7CB49D: self.history.append("CRT.srand")
        elif address == 0x7CB4AA: self.history.append("CRT.rand")


def loading_wait_progress_cases():
    """Original peer setter643C50 + aggregate6433D0 + completion branch684465."""
    rows = []
    controls = [(2, [(0, 100), (1, value)]) for value in (0, 50, 99, 100, 101, -1, -2147483648)]
    controls += [(8, [(i, 100) for i in range(7)] + [(7, value)]) for value in (99, 100)]
    controls += [(2, [(0, 100), (1, 50), (1, 25)]), (2, [(0, 100), (1, 100), (1, -1)])]
    for count, updates in controls:
        machine = AssignmentMachine(0, [], [])
        progress = 0xAC4F58
        machine.uc.mem_write(progress, bytes(0x84))
        machine.uc.mem_write(progress + 0x48, struct.pack("<d", 100.0))
        machine.uc.mem_write(progress + 0x61, bytes([count]))
        def redraw(uc, address, _size, _data):
            if address == 0x643AE0:
                machine.ret(8)  # Drawing only; setter and aggregate execute.
        machine.uc.hook_add(UC_HOOK_CODE, redraw)
        for index, percent in updates:
            lo, hi = struct.unpack("<II", struct.pack("<d", float(percent)))
            machine.call(0x643C50, ecx=progress, stack_args=(index, lo, hi, 0xFFFFFFFF, 0xFFFFFFFF))
        sp = STACK_BASE + STACK_SIZE - 0x2000
        machine.uc.reg_write(UC_X86_REG_ESP, sp)
        run_checked(machine.uc, 0x68444C, (0x68446B, 0x6845E7), count=100_000,
                    required_addresses=[0x6433D0, 0x684456, 0x684465])
        rows.append(dict(count=count, updates=updates,
                         values=list(struct.unpack("<" + "d" * count, machine.uc.mem_read(progress + 8, 8 * count))),
                         fraction=struct.unpack("<d", machine.uc.mem_read(sp + 0x18, 8))[0],
                         complete=machine.uc.reg_read(UC_X86_REG_EIP) == 0x6845E7))
    return rows


def starting_mcv_no_match_cases():
    """Original5D7030 ->505310 ->5117D0; no constructor substitution."""
    result = []
    for name, masks in (("empty_baseunit", []), ("incompatible_owner", [2, 4]), ("empty_owner", [0])):
        machine = AssignmentMachine(1234, [], [])
        country = machine.alloc(0x200)
        for offset in (0x24, 0x64, 0x98):
            machine.uc.mem_write(country + offset, b"Americans\0")
        machine.w32(country + 0xB8, 0)
        countries = machine.alloc(4)
        machine.w32(countries, country)
        machine.w32(0xA83C9C, countries)
        machine.w32(0xA83CA8, 1)
        house = machine.alloc(0x100)
        machine.w32(house + 0x34, country)
        rules = machine.alloc(0x2000)
        machine.w32(0x8871E0, rules)
        entries = machine.alloc(max(4, 4 * len(masks)))
        for index, mask in enumerate(masks):
            unit_type = machine.alloc(0x1000)
            machine.w32(unit_type + 0x6CC, mask)
            machine.w32(entries + 4 * index, unit_type)
        machine.w32(rules + 0xB24, entries)
        machine.w32(rules + 0xB30, len(masks))
        machine.uc.mem_write(0xA8B258, b"\1")
        visits = []
        def observe(uc, address, _size, _data):
            if address in (0x505310, 0x5117D0, 0x7353C0, 0x5D7064, 0x5D70DB):
                visits.append(address)
            if address == 0x7353C0:
                raise OracleError("no-match callback unexpectedly constructs a Unit")
        machine.uc.hook_add(UC_HOOK_CODE, observe)
        before = bytes(machine.uc.mem_read(machine.rng, STRUCT_LEN)).hex()
        machine.call(0x5D7030, stack_args=(house, 0))
        callback_result = machine.uc.reg_read(UC_X86_REG_EAX) & 0xFF
        after = bytes(machine.uc.mem_read(machine.rng, STRUCT_LEN)).hex()
        callback_draws = machine.draws.copy()
        # GenerateStartingUnits5D6FFE performs this separate synchronization draw.
        machine.call(0x65C7E0, ecx=machine.rng, stack_args=(0, 0xFFFF))
        result.append(dict(name=name, owner_masks=masks, initial_rng_hex=before,
                           callback_rng_hex=after, callback_result=callback_result,
                           callback_draws=callback_draws, visits=visits,
                           force_tail_rng_hex=bytes(machine.uc.mem_read(machine.rng, STRUCT_LEN)).hex()))
    return result


def host_seed_cases():
    cases = []
    for seed in (0, 1234, 0xFFFFFFFF):
        machine = HostSeedMachine(seed)
        entries = []
        for _ in range(2):
            before = bytes(machine.uc.mem_read(machine.rng, STRUCT_LEN)).hex()
            machine.draws.clear()
            machine.history.clear()
            sp = STACK_BASE + STACK_SIZE - 0x2000
            machine.uc.reg_write(UC_X86_REG_ESP, sp)
            run_checked(machine.uc, 0x5DEC3B, 0x5DEC61, count=100_000,
                        required_addresses=[0x65C7E0,0x7CB49D,0x7CB4AA,0x5DEC5C])
            if machine.uc.reg_read(UC_X86_REG_ESP) != sp: raise OracleError("BC seed block stack imbalance")
            entries.append(dict(main_before=before, main_after=bytes(machine.uc.mem_read(machine.rng, STRUCT_LEN)).hex(),
                                transmitted_seed=machine.r32(0xA8ED94), crt_state=machine.r32(machine.thread+0x14),
                                history=machine.history.copy(), draws=machine.draws.copy()))
        cases.append(dict(main_seed=seed, entries=entries))
    return cases


class CooperativeMachine(AssignmentMachine):
    """Original5C30C0/5C2710 and their live country/vector/RNG callees."""
    def __init__(self, seed, stages, roster, current_stage, human_colors):
        super().__init__(seed, [[0, -1, color, -1] for color in human_colors], [])
        self.mode = self.alloc(0x350)
        self.w32(self.mode, 0x7EE27C)
        self.w32(0xA8B23C, self.mode)
        progress = self.alloc(0x70)
        self.call(0x49B610, ecx=progress)
        self.w32(progress + 0x44, 0)
        self.w32(progress + 0x38, current_stage)
        self.uc.mem_write(progress + 0x6C, b"\x01")
        self.w32(self.mode + 0x40, progress)
        self.w32(self.mode + 0x33C, 0)
        for base in (0x1A4, 0x264):
            for row in range(8):
                vector = self.mode + base + 0x18 * row
                self.call(0x477BE0, ecx=vector, stack_args=(0, 0))
                self.w32(vector, 0x7E4E78)
                self.w32(vector + 0x14, 10)
        definition = self.alloc(0x4C)
        self.w32(0x89F570, definition)
        self.w32(0x89F574, 1)
        self.uc.mem_write(0x89F578, b"\x01")
        self.uc.mem_write(definition + 8, bytes([len(stages)]))
        for offset, role in ((0x10, "player"), (0x28, "enemy")):
            values = []
            for stage in stages:
                vector = self.alloc(0x18)
                tokens = []
                for token in stage[role]:
                    address = self.alloc(len(token) + 1)
                    self.uc.mem_write(address, token.encode("latin-1") + b"\0")
                    tokens.append(address)
                self.fill_vector(vector, tokens)
                values.append(vector)
            self.fill_vector(definition + offset, values)
        types = []
        for row in roster:
            country = self.alloc(0x200)
            self.uc.mem_write(country + 0x24, row["id"].encode("latin-1") + b"\0")
            self.uc.mem_write(country + 0x64, row["name"].encode("latin-1") + b"\0")
            ui_name = self.alloc(2)
            self.w32(country + 0x60, ui_name)
            types.append(country)
        self.fill_vector(0xA83C98, types)
        self.message = None
        self.capacity = 0
        self.uc.hook_add(UC_HOOK_CODE, self.observe_cooperative)

    def fill_vector(self, vector, values):
        data = self.alloc(max(4, 4 * len(values)))
        for index, value in enumerate(values): self.w32(data + index * 4, value)
        self.w32(vector + 4, data)
        self.w32(vector + 0x10, len(values))

    def observe_cooperative(self, uc, address, _size, _data):
        if address == 0x5E3720:
            self.message = Image(uc).cstr(uc.reg_read(UC_X86_REG_ECX))
            self.ret(0)
        elif address == 0x5E6520:
            self.ret(0, self.capacity)

    def rounds(self, count):
        return [[[struct.unpack("<i", self.uc.mem_read(self.r32(self.mode + base + row * 0x18 + 4) + stage * 4, 4))[0]
                  for base in (0x1A4, 0x264)] for row in range(8)] for stage in range(count)]



def cooperative_preparation_cases(stages, roster):
    result = []
    for name, capacity, first, stage, difficulty in (
        ("easy_suffix", 6, 2, 0, 0),
        ("hard_suffix_after_three_human_starts", 7, 3, 0, 5),
        ("nonpositive_preserves_count_and_rows", 4, 4, 0, 1),
        ("first_beyond_array_writes_count_without_loop", 12, 9, 0, 1),
        ("stale_stage_writes_count_without_rows", 6, 2, len(stages), 1),
    ):
        m = CooperativeMachine(1234, stages, roster, stage, [0, 1])
        m.capacity = capacity
        m.w32(m.r32(0xA8B230) + 0x11E4, first)
        m.w32(0xA8EB68, difficulty)
        m.w32(0xA8B274, 17)
        arrays = (0xA8B27C, 0xA8B29C, 0xA8B2BC, 0xA8B2DC, 0xA8B2FC)
        for row in range(8):
            for address in arrays: m.w32(address + row*4, -1)
        for address, value in zip(arrays, (0, 9, 3, 1, 2)): m.w32(address + 5*4, value)
        def read_rows():
            return [[struct.unpack("<i",m.uc.mem_read(address + row*4,4))[0] for address in arrays] for row in range(8)]
        initial = read_rows()
        m.call(0x5C30C0, ecx=m.mode)
        result.append(dict(name=name, capacity=capacity, first=first, current_stage=stage, difficulty=difficulty,
                           initial_count=17, initial=initial, count=m.r32(0xA8B274), rows=read_rows(),
                           returned=m.uc.reg_read(UC_X86_REG_EAX)&255, draws=m.draws))
    return result

def cooperative_cases():
    # Existing lexical collector only identifies authored entries. Original
    # ReadString/ReadInt and strtok establish every supplied native list/value.
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.ini_token_readers import Reader, Crt, READ_INT
    import hashlib
    root = Path(__file__).resolve().parents[2] / "ini"
    raw = (root / "coopcampmd.ini").read_bytes()
    rules = (root / "rulesmd.ini").read_bytes()
    sections, _ = lexical(raw, {"Allied", "Soviet", "Yuri", "World"})
    country_sections, _ = lexical(rules, {"Countries"})
    ids = list(country_sections["Countries"].values())
    country_sections, _ = lexical(rules, set(ids))
    reader, crt = Reader(), Crt()
    roster = [dict(id=id, name=reader.read_string(country_sections.get(id, {}).get("Name"), id, 0x31)[0]) for id in ids]
    result = []
    preparation = None
    for name, seed, campaign, current, colors in (
        ("allied_first", 1234, "Allied", 0, [6, 1]),
        ("allied_second_keeps_current_stage_for_all_rounds", 1, "Allied", 1, [0, 7]),
        ("world", 0, "World", 0, [3, 4]),
        ("completed_stage_returns_country_zero", 7, "Allied", 3, [1, 2]),
    ):
        section = sections[campaign]
        count = reader.scalar_latin1(section.get("NumberOfCampaignMaps"), 0, READ_INT)
        stages = [dict(player=crt.strtok(reader.read_string(section.get(f"CampaignPlayer{i+1}"), "", 128)[0]),
                       enemy=crt.strtok(reader.read_string(section.get(f"CampaignEnemy{i+1}"), "", 128)[0])) for i in range(count)]
        if preparation is None: preparation = cooperative_preparation_cases(stages, roster)
        machine = CooperativeMachine(seed, stages, roster, current, colors)
        initial = machine.state()["rng_hex"]
        machine.call(0x5C2710, ecx=machine.mode)
        result.append(dict(name=name, seed=seed, campaign=campaign, current_stage=current, human_colors=colors,
                           roster=roster, stages=stages, rng_before=initial, rng_after=machine.state()["rng_hex"],
                           rounds=machine.rounds(count), draws=machine.draws, message=machine.message))
    return dict(campaign_ini=raw.decode("latin-1"), campaign_sha256=hashlib.sha256(raw).hexdigest(),
                rules_sha256=hashlib.sha256(rules).hexdigest(), cases=result, preparation=preparation)

def observer_start_case(name, seed, observers, preassigned):
    humans = [[-3 if observer else 0, -1, 8 if observer else index, -1]
              for index, observer in enumerate(observers)]
    machine = AssignmentMachine(seed, humans, [])
    machine.w32(0xA8B238, 3)
    machine.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    machine.uc.mem_write(0x822D80, struct.pack("<H", NATIVE_FPCW))
    houses = machine.alloc(4 * len(humans))
    for index, node in enumerate(machine.nodes):
        house = machine.alloc(0x160B8)
        machine.uc.mem_write(house + 0x1EC, b"\1")
        machine.uc.mem_write(house + 0x1602A, (f"Human{index}\0").encode("utf-16-le"))
        machine.w32(houses + 4 * index, house)
    machine.w32(0xA8022C, houses)
    starts = [(20, 20), (40, 40), (20, 40)]
    coordinates = machine.alloc(4 * len(starts))
    machine.uc.mem_write(coordinates, b"".join(struct.pack("<hh", *point) for point in starts))
    vector = machine.alloc(24)
    machine.w32(vector + 4, coordinates)
    machine.w32(vector + 0x10, len(starts))
    occupied = machine.alloc(16)
    scenario = machine.r32(0xA8B230)
    table = list(preassigned) + [-1] * (16 - len(preassigned))
    for index, owner in enumerate(table):
        machine.w32(scenario + 0x1180 + index * 4, owner)
        machine.uc.mem_write(occupied + index, bytes([int(owner != -1)]))
    machine.uc.mem_write(0xABFDD4, b"\0\0")
    initial_rng = machine.state()["rng_hex"]
    placements = []
    for index in range(len(humans)):
        if index in preassigned:
            placements.append(list(starts[preassigned.index(index)]))
            continue
        output = machine.alloc(4)
        machine.call(0x5D6890, stack_args=(output, index, vector, occupied))
        placements.append(list(struct.unpack("<hh", machine.uc.mem_read(output, 4))))
    return dict(name=name, observers=observers, starts=starts, initial_table=preassigned,
                initial_rng_hex=initial_rng, placements=placements,
                table=list(struct.unpack("<3i", machine.uc.mem_read(scenario + 0x1180, 12))),
                observer_flags=list(machine.uc.mem_read(0xABFDD4, 2)),
                rng_hex=machine.state()["rng_hex"], draws=machine.draws)


def observer_start_cases():
    return [observer_start_case("observer_shares_explicit_player", 73, [False, False, True], [1, 0, -1]),
            observer_start_case("observer_after_automatic_players", 1234, [False, False, True], [-1, -1, -1])]


def timing_case(role, name, protocol, responses, connected=True):
    # Active LANRun host5DD48D..5DD541 and guest5DD2CD..5DD877.
    # Execute both manager5424C0 and connection48BA80 reads; change only
    # the service-owned response field before each read, never its result.
    m = Machine(0xBB, 800, 600)
    record, response_object = m.alloc(16), m.alloc(64)
    m.w32(0xA8E9C0 + 0x48, record if connected else 0)
    m.w32(record + 4, response_object)
    m.w32(0xA8B24C, protocol)
    for address in (0xA8B550, 0xA8B554, 0xA8DB9C, 0xA8B56C, 0xA8B570):
        m.w32(address, 0xDEADBEEF)
    calls, leaf_calls = [], []
    def observe(uc, address, _size, _data):
        if address == 0x5424C0:
            if uc.reg_read(UC_X86_REG_ECX) != 0xA8E9C0:
                raise OracleError('wrong response getter receiver')
            if len(calls) >= len(responses):
                raise OracleError('unexpected extra getter read')
            response = responses[len(calls)]
            m.w32(response_object + 0x1C, response)
            calls.append(dict(response=response, return_address=m.r32(uc.reg_read(UC_X86_REG_ESP))))
        elif address == 0x48BA80:
            if uc.reg_read(UC_X86_REG_ECX) != response_object:
                raise OracleError('wrong original response leaf receiver')
            leaf_calls.append(m.r32(response_object + 0x1C))
    m.uc.hook_add(UC_HOOK_CODE, observe)
    sp = STACK_BASE + STACK_SIZE - 0x2000
    m.uc.reg_write(UC_X86_REG_ESP, sp)
    m.uc.reg_write(UC_X86_REG_EBP, 0)
    begin, end = (0x5DD48D, 0x5DD541) if role == 'host' else (0x5DD2CD, 0x5DD877)
    run_checked(m.uc, begin, end, count=10000)
    if m.uc.reg_read(UC_X86_REG_ESP) != sp:
        raise OracleError('getter stack imbalance')
    if len(leaf_calls) != (len(calls) if connected else 0):
        raise OracleError('original getter leaf not executed')
    result = dict(name=name, role=role, protocol=protocol, connected=connected,
                  responses=responses, calls=calls, leaf_calls=leaf_calls,
                  max_ahead=m.r32(0xA8B550), frame_send_rate=m.r32(0xA8B554),
                  resets=[m.r32(a) for a in (0xA8DB9C, 0xA8B56C, 0xA8B570)])
    if result['frame_send_rate'] != 5 or result['resets'] != [0, 0, 0]:
        raise OracleError('caller timing writes missing')
    return result


def timing_cases():
    controls = [
        ('null_connection', 2, [0xFFFFFFFF], False),
        ('p2_zero', 2, [0], True),
        ('p2_79', 2, [79], True),
        ('p2_80', 2, [80], True),
        ('p2_87', 2, [87], True),
        ('p2_88', 2, [88, 88], True),
        ('p2_119', 2, [119, 119], True),
        ('p2_120', 2, [120, 120], True),
        ('p2_127', 2, [127, 127], True),
        ('p2_128', 2, [128, 128], True),
        ('p2_unsigned_negative_one', 2, [0xFFFFFFFF, 0xFFFFFFFF], True),
        ('p2_signed_min', 2, [0x80000000, 0x80000000], True),
        ('p2_second_zero', 2, [88, 0], True),
        ('p2_second_lower', 2, [88, 8], True),
        ('p2_second_higher', 2, [88, 128], True),
        ('p0_zero', 0, [0], True),
        ('p0_23', 0, [23], True),
        ('p0_24', 0, [24, 24], True),
        ('p1_negative_one', 1, [0xFFFFFFFF], True),
        ('p1_negative_seven', 1, [0xFFFFFFF9], True),
        ('p1_negative_eight', 1, [0xFFFFFFF8], True),
        ('p1_signed_min', 1, [0x80000000], True),
        ('p1_signed_max', 1, [0x7FFFFFFF, 0x7FFFFFFF], True),
        ('p3_second_zero', 3, [24, 0], True),
        ('p3_second_negative', 3, [24, 0xFFFFFFF8], True),
        ('p3_second_signed_min', 3, [24, 0x80000000], True),
        ('p3_second_higher', 3, [24, 88], True),
        ('unsigned_protocol_other', 0xFFFFFFFF, [24, 31], True),
    ]
    cases = [timing_case(role, *case) for case in controls for role in ('host', 'guest')]
    for host, guest in zip(cases[::2], cases[1::2]):
        if any(host[key] != guest[key] for key in ('max_ahead', 'frame_send_rate', 'resets', 'leaf_calls')):
            raise OracleError(f"host/guest timing differs: {host['name']}")
    return cases

def generate():
    ascii_units = lambda text: list(text.encode("ascii"))
    rows = [
        ("empty", []), ("one_char", ascii_units("x")),
        ("spaces_retained", ascii_units("  a  ")),
        ("latin_108", [65] * 108), ("latin_109", [65] * 109),
        ("latin_121", [65] * 121),
        ("wide_at_zero", [0x4E00] + [65] * 120),
        ("wide_at_one", [65, 0x4E00] + [65] * 120),
        ("wide_at_40", [65] * 40 + [0x4E00] + [65] * 80),
        ("wide_at_42", [65] * 42 + [0x4E00] + [65] * 80),
        ("u0100_only", [0x100] * 121),
        ("u0100_then_wide", [0x100, 0x4E00] + [65] * 120),
    ]
    children = relayout(0xBB, 800, 600)
    return dict(source="unicorn/gamemd.exe", chat=[chat_case(*row) for row in rows],
                layout_800=children, loading_wait_progress=loading_wait_progress_cases(), starting_mcv_no_match=starting_mcv_no_match_cases(), timing=timing_cases(), host_seed=host_seed_cases(), assignments=assignment_cases(), observer_starts=observer_start_cases(), cooperative=cooperative_cases(),
                setup_layouts=[dict(dialog=d,width=w,height=h,children=relayout(d,w,h)) for d in (0xBC,0xBD,0xBC6) for w,h in ((800,600),(640,480))],
                setup_help=[dict(dialog=d,control=c,key=help_label(d,c)) for d in (0xBC,0xBD) for c in
                            (0x411,0x6A1,0x6A2,0x6A3,0x76D,0x529,0x402,0x54F,0x3E8,0x54E,0x69A,0x69D,0x54D,0x540,0x53F,0x468,0x5A8,0x6D2,0x6DA,2,(0x588 if d==0xBC else 0x59F))]
                          + [dict(dialog=0xBC6,control=c,key=help_label(0xBC6,c)) for c in
                             (0x6D1,0x5C0,0x3EA,0x781,0x783,0x785,0x787,0x789,0x412,0x6A1,0x6DA,0x522,0x6A3,0x732,0x540,0x53F,0x468,0x5A8,0x77E,0x6D2)],
                help=[dict(control=control, key=help_label(0xBB, control)) for control in
                      (0x6CC, 0x6CD, 0x6CE, 0x5C0, 0x540, 0x541, 0x549, 0x53E, 0x53F, 0x75F, 0x760)])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Original host/guest LAN MaxAhead blocks 5DD48D/5DD2CD and response getters 5424C0/48BA80; BC host seed block 5DEC3B..5DEC61 (Main RNG -> CRT srand/rand), Battle assignments 69B8C0, observer starts 5D6890, Cooperative AI preparation 5C30C0 and campaign round assignments 5C2710 with full Scenario RNG state/draw order; BB/BC/BD/BC6 relayout/help and NewEdit 614B30 -> BB 5DDBD0 -> chat cap 5EDD00 -> send 5DE360",
        assumptions=[
            "Loading wait controls execute original643C50 signed-percent updates (including regressions/upper clamp),6433D0 average and68444C..684465 completion comparison. Only643AE0 drawing is suppressed. Explicit2/8-row controls include99/100 and signed negative input; native stored doubles and branch destinations are observed, not calculated by Python.",
            "Starting-MCV no-match controls execute complete5D7030, Owner-only505310 and originalCountryName5117D0 with explicit Owner masks and Country0. Unit constructor entry is fail-closed; completeScenario RNG bytes and callback result are observed. Separate original65C7E0(0,FFFF) records the orchestrator tail5D6FFE; no constructor/RNG result is substituted.",
            "LAN MaxAhead controls enter original host5DD48D and guest5DD2CD with caller-established EBP=0, explicit protocol and successive raw32 connection response fields. Stop before hostSetTiming5DD541/guestcontinuation5DD877. Both paths observe frame-send-rate5 and all3 reset writes; second-read controls establish absence of another floor check.",
            "BC5DEC3B..5DEC61 executes original MainRandomRanged(1,32767), CRTsrand7CB49D andCRTrand7CB4AA across two successive entries. ExplicitMainseeds initialize real65C6D0 state; TLSthreadcell starts73 but eachentry replacesit. FullMainstates, rawdraws, callhistory, CRTstate andtransmittedseed observed.",
            "Cooperative5C2710 executes all8 rows for each campaign round on actual7EE27C vtable, originalcountry callbacks/ID-Name resolver/vector allocation/RNG. OriginalReadInt/ReadString/strtok read physical CoopCampMD and RULESMD country list/Name inputs through existing lexical+reader owners. Progressstage and humanresolvedcolors are controlled inputs; no Rust values seed native results.",
            "Observer-start controls execute whole5D6890 in supplied House order with explicit House human/UIName fields, source nodes, three start cells and initial start table; original50B730/wcscmp/RNG/distance math execute. FPCW and cached ftol word0E7F match active startup. Normal rows precede observercolor8, as native CreateHouses sorts resolved colors.",
            "Assignment cases supply original128-byte human nodes (+4B country,+4F country selector,+53 color,+57 color selector,+6B observer), ordered vector and eight raw AI arrays; unfilled AI slots are-1. Battle vtable7EE184 from ctor5C0DF8 selects original5D6430/5D6440/69B670 country callbacks. seeded_struct executes native65C6D0; full RNG states and ordered ranged/raw draws retained.",
            "Shared shell_relayout.Machine supplies original resource children, 6x13 dialog base units and known shape dimensions; layout ends before NewEdit497's extra1px inset.",
            "Chat WideWstring constructed by original7B66D0; OwnerDraw record3C points to it, style54=4 is resource ES_MULTILINE, all other editor fields zero. No typing limit; UTF16 inputs are explicit supplied controls.",
            "Sender name Tester, lobby membership0, own-node-only list count1; game hash input empty. No packets leave this fixture.",
            "ASCII, U0100 and U4E00 controls bound the unsigned DEC and distinct IsLatin1/first-wide predicates; not exhaustive all Unicode or native keyboard composition coverage.",
        ], substitutions=[
            "LAN timing manager+48 points to supplied connection record, record+4 to responseobject, responseobject+1C changes at getter5424C0 entry. Null-connection control uses manager+48=0. Original5424C0 and48BA80 execute unchanged; no getter/arithmetic output substitution.",
            "Host seed block _getptd7D140B supplies one retained thread-context pointer; full srand/rand andMainRNG execute unchanged, stopping immediately after transmittedseed write5DEC5C.",
            "Cooperative5E3720 transport captures original C1 text and returns;5E6520 selectedmap-capacity query supplied as explicit upstream input for preassignment controls. No country/RNG/vector output substituted. Native49B610 and477BE0 initialize progress/vectors; already-loaded registry fixture supplies their reader-established fields.",
            "Assignment DebugLog4068E0 is a sink. Full69B8C0, collision helper69B600, original country vtable callbacks, name narrowing7350C0 and RNG65C7E0 execute unchanged; no RNG or assignment outputs substituted.",
            "Existing Machine window manager, allocation/deallocation and import fail-closed hooks; no alternate VM/PE loader.",
            "SendMessage transports chat E/4B2/4B3 to original NewEdit and parent111 to original BB. Text-message default owner supplies record28 pointer. Parent4DA is unhandled; CallWindowProcA fallback returns0.",
            "Chat display5E2700 records its original message argument instead of painting; original5DE360 packet construction/hash and no-recipient return still execute.",
        ], entry_points={"loading_progress": 0x643C50, "loading_average": 0x6433D0, "loading_wait": 0x684370,"starting_mcv": 0x5D7030, "first_ownable": 0x505310,"timing_host_begin": 0x5DD48D, "timing_host_end": 0x5DD541,
                         "timing_guest_begin": 0x5DD2CD, "timing_guest_end": 0x5DD877,
                         "timing_getter": 0x5424C0, "timing_response_leaf": 0x48BA80,
                         "host_seed_begin": 0x5DEC3B, "host_seed_end": 0x5DEC61,
                         "crt_srand": 0x7CB49D, "crt_rand": 0x7CB4AA,
                         "cooperative_prepare_ai": 0x5C30C0, "cooperative_round_assignments": 0x5C2710,
                         "cooperative_human_country": 0x5C34F0, "cooperative_ai_country": 0x5C35F0,
                         "country_lookup": 0x49D9F0, "progress_constructor": 0x49B610,
                         "vector_constructor": 0x477BE0, "campaign_read_int": 0x5276D0,
                         "campaign_read_string": 0x528A10, "campaign_strtok": 0x7C9CC2,
                         "new_edit": NEW_EDIT, "lobby": LOBBY, "chat_cap": 0x5EDD00, "chat_send": 0x5DE360,
                         "observer_start": 0x5D6890, "assignments": 0x69B8C0, "battle_country": 0x5D6430, "rng": 0x65C7E0, "relayout": 0x60C4A0, "help": 0x6040B0}))
