"""Transport-only checks; these are not native audio behavior goldens."""
from pathlib import Path
import struct
from types import SimpleNamespace
import unittest

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.spatial_oracle.engineer_repair_admission import NativeAudioPlatform


class BufferTransportTests(unittest.TestCase):
    def setUp(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        self.u.mem_map(0x10000, 0x10000)
        self.sp, self.output = 0x11000, 0x12000
        self.owner = SimpleNamespace(u=self.u, phase='transport_test', read32=self.read)
        self.owner.ret = self.return_from_com
        self.platform = NativeAudioPlatform(self.owner, Path('.'))
        self.a = self.platform.interface('buffer', 64)
        self.b = self.platform.interface('buffer', 64)

    def read(self, pointer):
        return struct.unpack('<I', self.u.mem_read(pointer, 4))[0]

    def return_from_com(self, value=0, cleanup=0):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value)
        self.u.reg_write(UC_X86_REG_EIP, self.read(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + cleanup + 4)

    def call(self, method, pointer, *args):
        self.u.mem_write(self.sp, struct.pack('<' + 'I' * (2 + len(args)),
                                            0x12345678, pointer, *args))
        self.u.reg_write(UC_X86_REG_ESP, self.sp)
        pc = self.read(self.read(pointer) + method * 4)
        self.assertTrue(self.platform.hook(self.u, pc, 1))
        self.assertEqual(self.u.reg_read(UC_X86_REG_EIP), 0x12345678)
        self.assertEqual(self.u.reg_read(UC_X86_REG_ESP), self.sp + 8 + len(args) * 4)

    def test_buffer_status_cursor_and_stop_are_independent(self):
        devices = {self.a: dict(value=1, play_cursor=12, write_cursor=20),
                   self.b: dict(value=3, play_cursor=28, write_cursor=36)}
        self.platform.configure_transport(buffer_devices=devices)
        self.platform.device_stop_updates_status = True
        for pointer, expected in ((self.a, devices[self.a]), (self.b, devices[self.b])):
            self.call(9, pointer, self.output)
            self.assertEqual(self.read(self.output), expected['value'])
            self.call(4, pointer, self.output, self.output + 4)
            self.assertEqual([self.read(self.output), self.read(self.output + 4)],
                             [expected['play_cursor'], expected['write_cursor']])
        self.call(18, self.a)
        self.assertEqual([devices[self.a]['value'], devices[self.b]['value']], [0, 3])
        self.call(9, self.b, self.output)
        self.assertEqual(self.read(self.output), 3)

    def test_existing_global_transport_and_zero_cursor_defaults(self):
        self.call(4, self.a, self.output, self.output + 4)
        self.assertEqual([self.read(self.output), self.read(self.output + 4)], [0, 0])
        with self.assertRaisesRegex(ValueError, 'explicit OS device'):
            self.call(9, self.a, self.output)
        device = dict(value=1, play_cursor=7, write_cursor=9)
        self.platform.configure_transport(device=device)
        self.call(4, self.b, self.output, self.output + 4)
        self.assertEqual([self.read(self.output), self.read(self.output + 4)], [7, 9])
        self.call(18, self.a)
        self.assertEqual(device['value'], 1)
        self.platform.device_stop_updates_status = True
        self.call(18, self.a)
        self.call(9, self.b, self.output)
        self.assertEqual(self.read(self.output), 0)

    def test_unknown_buffer_identity_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'returned device-buffer identities'):
            self.platform.configure_transport(buffer_devices={0xDEADBEEF: {}})


if __name__ == '__main__':
    unittest.main()
