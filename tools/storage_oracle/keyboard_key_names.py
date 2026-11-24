"""Original61EF70 key-name composition and a bounded host Windows capture probe.

This fixture depends explicitly on the host Windows keyboard layout. It never
sends input to user windows: messages and thread-local key state are supplied to
a hidden disposable standard hot-key control, then restored/destroyed.
"""

import ctypes
from ctypes import wintypes as w
from pathlib import Path
import platform
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, load_image, provenance, run_checked


def windows():
    u = ctypes.WinDLL("user32", use_last_error=True)
    u.MapVirtualKeyA.argtypes = [w.UINT, w.UINT]
    u.MapVirtualKeyA.restype = w.UINT
    u.VkKeyScanW.argtypes = [w.WCHAR]
    u.VkKeyScanW.restype = ctypes.c_short
    u.GetKeyNameTextA.argtypes = [w.LONG, ctypes.c_char_p, ctypes.c_int]
    u.GetKeyNameTextA.restype = ctypes.c_int
    u.GetKeyboardLayoutNameW.argtypes = [w.LPWSTR]
    u.CreateWindowExW.argtypes = [w.DWORD, w.LPCWSTR, w.LPCWSTR, w.DWORD,
        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        w.HWND, w.HMENU, w.HINSTANCE, w.LPVOID]
    u.CreateWindowExW.restype = w.HWND
    u.SendMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
    u.SendMessageW.restype = w.LPARAM
    u.DestroyWindow.argtypes = [w.HWND]
    u.GetKeyboardState.argtypes = [ctypes.POINTER(w.BYTE)]
    u.SetKeyboardState.argtypes = [ctypes.POINTER(w.BYTE)]
    return u


def formatted(u, encoded):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, 0x10000)
    uc.mem_map(RET_MAGIC, 0x1000)
    stack = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(stack, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, stack)
    uc.reg_write(UC_X86_REG_ECX, encoded)
    uc.reg_write(UC_X86_REG_EDX, SCRATCH)
    targets = {0x7E13E8: SCRATCH + 0xF000, 0x7E1454: SCRATCH + 0xF010}
    for slot, address in targets.items():
        uc.mem_write(slot, struct.pack("<I", address))
    calls = []

    def get(address):
        return struct.unpack("<I", uc.mem_read(address, 4))[0]

    def hook(_uc, address, _size, _data):
        if address not in (*targets.values(), 0x7CA564):
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == targets[0x7E13E8]:
            vk, mode = get(sp + 4), get(sp + 8)
            result, arguments = u.MapVirtualKeyA(vk, mode), 2
            calls.append(dict(api="MapVirtualKeyA", vk=vk, mode=mode, result=result))
        elif address == targets[0x7E1454]:
            bits, output, size = get(sp + 4), get(sp + 8), get(sp + 12)
            buffer = ctypes.create_string_buffer(size)
            result, arguments = u.GetKeyNameTextA(bits, buffer, size), 3
            uc.mem_write(output, buffer.raw)
            calls.append(dict(api="GetKeyNameTextA", bits=bits, size=size,
                              result=result, text=buffer.value.decode("mbcs")))
        else:
            output, fmt, text = get(sp + 4), get(sp + 8), get(sp + 12)
            if bytes(uc.mem_read(fmt, 8)) != "%hs\0".encode("utf-16-le"):
                raise RuntimeError("Unexpected native wide formatter")
            narrow = bytes(uc.mem_read(text, 512)).split(b"\0")[0]
            wide = narrow.decode("mbcs").encode("utf-16-le")
            uc.mem_write(output, wide + b"\0\0")
            result, arguments = len(wide) // 2, 0  # cdecl; native caller pops arguments
        uc.reg_write(UC_X86_REG_EAX, result)
        uc.reg_write(UC_X86_REG_EIP, get(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 * (arguments + 1))

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x61EF70, RET_MAGIC, count=10000,
                required_addresses=[0x61EF8D, 0x61F180])
    output = bytes(uc.mem_read(SCRATCH, 1024)).decode("utf-16-le").split("\0")[0]
    return dict(hotkey_word=encoded, text=output, calls=calls)


def capture(u):
    class Init(ctypes.Structure):
        _fields_ = [("size", w.DWORD), ("classes", w.DWORD)]
    common = ctypes.WinDLL("comctl32")
    common.InitCommonControlsEx.argtypes = [ctypes.POINTER(Init)]
    if not common.InitCommonControlsEx(ctypes.byref(Init(ctypes.sizeof(Init), 0x40))):
        raise RuntimeError("Cannot initialize stock hot-key class")
    state_type = w.BYTE * 256
    original = state_type()
    if not u.GetKeyboardState(original):
        raise RuntimeError("Cannot save probe thread keyboard state")
    parent = u.CreateWindowExW(0, "STATIC", "", 0, 0, 0, 120, 40, None, None, None, None)
    if not parent:
        raise RuntimeError("Cannot create hidden probe parent")
    result = []
    try:
        child = u.CreateWindowExW(0, "msctls_hotkey32", "", 0x40000000,
                                 0, 0, 100, 20, parent, None, None, None)
        if not child:
            raise RuntimeError("Cannot create hidden probe child")
        for vk, extended in ((0x41, 0), (0x10, 0), (0x11, 0), (0x12, 0),
                             (0x0D, 0), (0x09, 0), (0x20, 0), (0x2E, 1),
                             (0x1B, 0), (0x08, 0), (0x6E, 0), (0x25, 1), (0x25, 0)):
            state = state_type()
            if not u.SetKeyboardState(state):
                raise RuntimeError("Cannot supply probe thread keyboard state")
            u.SendMessageW(child, 0x401, 0x42, 0)
            before = u.SendMessageW(child, 0x402, 0, 0)
            if before != 0x42:
                raise RuntimeError("Initial stock capture is not B")
            if vk in (0x10, 0x11, 0x12):
                state[vk] = 128
                if not u.SetKeyboardState(state):
                    raise RuntimeError("Cannot supply probe modifier state")
            bits = 1 | (u.MapVirtualKeyA(vk, 0) << 16) | (extended << 24)
            u.SendMessageW(child, 0x104 if vk == 0x12 else 0x100, vk, bits)
            down = u.SendMessageW(child, 0x402, 0, 0)
            if not u.SetKeyboardState(state_type()):
                raise RuntimeError("Cannot release probe modifier state")
            u.SendMessageW(child, 0x105 if vk == 0x12 else 0x101, vk, bits | (3 << 30))
            up = u.SendMessageW(child, 0x402, 0, 0)
            result.append(dict(vk=vk, extended=bool(extended), before=before, down=down, up=up))
    finally:
        u.SetKeyboardState(original)
        u.DestroyWindow(parent)
    return result


def generate():
    u = windows()
    layout = ctypes.create_unicode_buffer(9)
    if not u.GetKeyboardLayoutNameW(layout):
        raise RuntimeError("Cannot identify host keyboard layout")
    words = (0, 0x100, 0x200, 0x400, 0x700, 0x2E, 0x6E, 0x25, 0x825, 0x741)
    return dict(source="unicorn/gamemd.exe + host Windows class probe",
                host=dict(windows=platform.version(), keyboard_layout=layout.value),
                formatter=[formatted(u, word) for word in words], capture=capture(u),
                printable=[dict(character=character, mapping=u.VkKeyScanW(character))
                           for character in "+-/;,.[]\\æøå"])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Original full61EF70 formatter for10 ordinary words,13 host Windows capture cases and12 host printable mappings",
            assumptions=[
                "Formatter input is HKM word, not game encoded extended-bit identity",
                "Host keyboard layout and Windows version are explicit fixture inputs; other hosts may differ",
                "Original61ECA0 delegates nonpaint input to saved stock control procedure",
                "Capture probe calls current Windows class, not original retail window or modal message pump",
                "No focus traversal, IsDialogMessage interception, IME, non-ACP glyph or modifier-chord coverage",
            ],
            substitutions=[
                "Original formatter Win32 imports execute host MapVirtualKeyA/GetKeyNameTextA",
                "7CA564 sole %hs conversion is supplied using the host ANSI codec",
                "Hidden disposable hot-key HWND receives messages and supplied/restored thread-local keyboard state",
            ],
            entry_points={"formatter": 0x61EF70, "control": 0x61ECA0,
                          "capture_game_conversion": 0x5FB73A}))
