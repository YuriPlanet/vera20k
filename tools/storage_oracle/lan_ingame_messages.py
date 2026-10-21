"""Original in-game LAN application packet construction slices."""

from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP,
                               UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

from tools.native_oracle import NATIVE_FPCW, OracleError, STACK_BASE, STACK_SIZE, provenance, run_checked
from tools.storage_oracle.lan_messages import PacketMachine
from tools.storage_oracle.saved_game_layout import code_guard, check_code


class InGameMachine(PacketMachine):
    def __init__(self, name, game, color):
        super().__init__()
        self.uc.mem_write(self.node, name.encode('utf-16-le') + b'\0\0')
        self.uc.mem_write(0xA8D610, game.encode('utf-16-le') + b'\0\0')
        self.w32(0xA8B394, color)
        self.writes = set()
        self.uc.hook_add(UC_HOOK_CODE, self.observe_ingame)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)
        self.guard = code_guard(self.uc)

    def observe_write(self, _uc, _access, address, size, _value, _data):
        self.writes.update(range(address, address + size))

    def capture_packet(self, pointer, length):
        return dict(payload=bytes(self.uc.mem_read(pointer, length)).hex(),
                    written_offsets=sorted(at - pointer for at in self.writes
                                           if pointer <= at < pointer + length))

    def observe_ingame(self, _uc, address, _size, _data):
        if address in (0x4068E0, 0x752B70):
            self.ret(0)  # Debug log and terminal local audio only.

    def stack(self):
        stack = STACK_BASE + STACK_SIZE - 0x4000
        self.uc.mem_write(stack, b'\xA5' * 0x3000)
        self.uc.reg_write(UC_X86_REG_ESP, stack)
        return stack

    def loading(self, percent):
        self.stack()
        self.uc.reg_write(UC_X86_REG_EBP, 0xFFFFFFFF)
        self.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.w32(0x822D80, NATIVE_FPCW)
        self.uc.mem_write(0xAC4F58 + 8, struct.pack('<d', percent))
        self.uc.mem_write(0xAC4F58 + 0x48, struct.pack('<d', 100.0))
        self.uc.mem_write(0xAC4F58 + 0x61, b'\x01')
        self.writes.clear()
        run_checked(self.uc, 0x69AF45, (0x5410F0, 0x541260), count=100_000,
                    required_addresses=[0x643E90, 0x7C5F00, 0x7350C0])
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        pointer, length, reliable = (self.r32(sp + 4 + 4 * i) for i in range(3))
        if length != 51:
            raise OracleError(f'loading record is {length} bytes')
        check_code(self.uc, self.guard)
        return dict(**self.capture_packet(pointer, length), reliable=reliable,
                    wire_percent=struct.unpack('<i', self.uc.mem_read(pointer + 47, 4))[0],
                    transport_entry=f'{self.uc.reg_read(UC_X86_REG_EIP):08X}')

    def chat(self, text):
        stack = self.stack()
        self.w32(stack + 0x10, 4)
        self.uc.reg_write(UC_X86_REG_EBP, 4)
        self.uc.reg_write(UC_X86_REG_EDI, 4)
        self.uc.reg_write(UC_X86_REG_EBX, 0)
        self.uc.mem_write(0xA8D638, b'\xA5' * 455)
        self.uc.mem_write(0xA8BDCC, text.encode('utf-16-le') + b'\0\0')
        self.writes.clear()
        run_checked(self.uc, 0x55EDBF, 0x55EE60, count=100_000,
                    required_addresses=[0x7350C0, 0x5DAC00, 0x7CA489])
        check_code(self.uc, self.guard)
        return self.capture_packet(0xA8D638, 455)

    def taunt(self, country, number):
        stack = self.stack()
        command = self.alloc(8)
        self.w32(command + 4, number)
        self.w32(self.node + 0x4B, country)
        self.w32(0xA80238, 1)
        self.uc.reg_write(UC_X86_REG_EBX, command)
        self.uc.reg_write(UC_X86_REG_EAX, 6789)
        self.writes.clear()
        run_checked(self.uc, 0x5363C6, 0x536459, count=100_000,
                    required_addresses=[0x7350C0, 0x5DAC00, 0x752B70])
        # At this boundary the two saved registers place the local record at
        # entry ESP+0C. Capturing here avoids membership/destination routing.
        check_code(self.uc, self.guard)
        packet = self.capture_packet(stack + 0xC, 455)
        packet['byte'] = self.uc.mem_read(stack + 0xC + 0x30, 1)[0]
        return packet


def generate():
    name, game, color = 'Host-é', 'Game-ÿ', 6
    rows = []
    for progress in (0, 50, 99, 100):
        machine = InGameMachine(name, game, color)
        rows.append(dict(kind='loading', name=name, game=game, color=color,
                         progress=progress, **machine.loading(progress)))
    for chat in ('Hello', 'é A\u0100 \U0001F600'):
        machine = InGameMachine(name, game, color)
        rows.append(dict(kind='chat', name=name, game=game, color=color,
                         text=chat, **machine.chat(chat)))
    machine = InGameMachine(name, game, color)
    rows.append(dict(kind='taunt', name=name, game=game, color=color,
                     country=9, number=3, **machine.taunt(9, 3)))
    return dict(source='unicorn/gamemd.exe', packets=rows)


def metadata():
    return provenance(
        scope='Original in-game LAN loading0F, chat0C and taunt1D application packet construction; typed codec byte fields only',
        assumptions=[
            'Loading enters69AF45 after its caller gates and display update. ProgressScreen row0 and maximum100 are explicit double inputs; original643E90, x87 multiply,7C5F00,7350C0 and packet zero/stores execute. FPCW and cached ftol word822D80 are0E7F, matching active startup. The99 control emits98 after native division/multiply/truncation; this is observed output, not an instruction for the codec to transform caller values.',
            'Loading stops at5410F0 or541260 before transport. The51-byte payload, reliable argument and selected transport entry are captured. Progress0/50/99 exercise broadcast-unreliable;100 exercises direct-guaranteed to the first remote source.',
            'Chat enters55EDBF with caller edit-result4 and EBP4, using the original overflow-text branch. Original7350C0,5DAC00 and CRTwide copy execute through55EE60. Host-E9/Game-00FF/color6 and each UTF16 text are supplied caller fields. Upstream keyboard/editor splitting and recipient selection are outside this fixture.',
            'Taunt enters5363C6 after admission with original caller EBX pointing to the command number and EAX containing supplied time6789. Original name/hash, country+4B/number packing, timestamp store and relay reset execute through536459 before the first recipient iteration. Supplied country9/number3 produce the byte captured for the existing app taunt owner to consume; this codec does not pack taunts.',
            'Chat global buffer and taunt stack startA5. Native does not clear their unused bytes; written_offsets observes original memory writes and selects only defined constructor bytes for outbound comparisons. Decoder comparisons retain every raw byte. Loading executes its original whole455-byte clear but only the first51 bytes are sent.',
        ], substitutions=[
            'Shared PacketMachine and shell_relayout.Machine allocation/import boundaries only. DebugLog4068E0 and terminal local audio752B70 return without effect. No packet store, hash result, string copy, progress getter or arithmetic instruction is substituted. Executable sections are compared before/after every control.',
        ], entry_points={
            'loading_begin': 0x69AF45, 'loading_fraction': 0x643E90,
            'loading_ftol': 0x7C5F00, 'global_send': 0x5410F0,
            'node_send': 0x541260, 'chat_begin': 0x55EDBF,
            'chat_end': 0x55EE60, 'taunt_begin': 0x5363C6,
            'taunt_end': 0x536459, 'game_name_hash': 0x5DAC00,
        })


SOURCE_PATHS = {
    'in_game_packets': Path(__file__),
    'packet_machine': Path(__import__('tools.storage_oracle.lan_messages', fromlist=['x']).__file__),
    'native_oracle': Path(__import__('tools.native_oracle', fromlist=['x']).__file__),
    'shared_machine': Path(__import__('tools.storage_oracle.shell_relayout', fromlist=['x']).__file__),
    'code_guard': Path(__import__('tools.storage_oracle.saved_game_layout', fromlist=['x']).__file__),
}


