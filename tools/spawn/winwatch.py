# Report on-screen windows owned by vera20k processes (CoreGraphics window list).
import ctypes, ctypes.util, subprocess, sys, time, json
cg = ctypes.CDLL(ctypes.util.find_library('CoreGraphics'))
cf = ctypes.CDLL(ctypes.util.find_library('CoreFoundation'))
cg.CGWindowListCopyWindowInfo.restype = ctypes.c_void_p
cg.CGWindowListCopyWindowInfo.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
cf.CFArrayGetCount.restype = ctypes.c_long; cf.CFArrayGetCount.argtypes = [ctypes.c_void_p]
cf.CFArrayGetValueAtIndex.restype = ctypes.c_void_p; cf.CFArrayGetValueAtIndex.argtypes = [ctypes.c_void_p, ctypes.c_long]
cf.CFDictionaryGetValue.restype = ctypes.c_void_p; cf.CFDictionaryGetValue.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
cf.CFStringCreateWithCString.restype = ctypes.c_void_p; cf.CFStringCreateWithCString.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint32]
cf.CFNumberGetValue.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
key = lambda s: cf.CFStringCreateWithCString(None, s.encode(), 0x08000100)
PID = key('kCGWindowOwnerPID')
def onscreen_pids():
    arr = cg.CGWindowListCopyWindowInfo(1, 0)  # kCGWindowListOptionOnScreenOnly
    pids = []
    for i in range(cf.CFArrayGetCount(arr)):
        d = cf.CFArrayGetValueAtIndex(arr, i)
        v = ctypes.c_int(0); cf.CFNumberGetValue(cf.CFDictionaryGetValue(d, PID), 3, ctypes.byref(v)); pids.append(v.value)
    return pids
deadline = time.time() + float(sys.argv[1]); seen = 0; samples = 0
while time.time() < deadline:
    ours = set(int(p) for p in subprocess.run(['pgrep', '-f', 'fast/vera20k -SPAWN'], capture_output=True, text=True).stdout.split())
    if ours:
        samples += 1
        hits = [p for p in onscreen_pids() if p in ours]
        if hits: seen += 1; print('VISIBLE window for pids', hits, flush=True)
    time.sleep(0.5)
print(f'samples_with_clients={samples} visible_samples={seen}')
