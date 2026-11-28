"""On Android 14, open the animated main menu of a fresh debug build, then run
python android/tools/test_jni_refs.py with adb on PATH. No root is required.
"""

import shlex
import subprocess
import time


def shell(*args):
    return subprocess.check_output(["adb", "shell", *args], encoding="utf-8", errors="replace").strip()


def local_ref_bytes(pid):
    maps = shell("run-as", "org.vera20k", "cat", f"/proc/{pid}/maps")
    total = 0
    for line in maps.splitlines():
        if line.endswith("[anon:dalvik-local ref table]"):
            start, end = line.split()[0].split("-")
            total += int(end, 16) - int(start, 16)
    if not total:
        raise RuntimeError("ART did not expose named JNI local reference tables")
    return total


def last_frame(layer):
    rows = shell("dumpsys", "SurfaceFlinger", "--latency", shlex.quote(layer))
    # The middle timestamp is the actual presentation time, or INT64_MAX if pending.
    times = [int(row.split()[1]) for row in rows.splitlines() if len(row.split()) == 3]
    return max((stamp for stamp in times if 0 < stamp < 2**63 - 1), default=0)


def main():
    pid = shell("pidof", "org.vera20k")
    layers = shell("dumpsys", "SurfaceFlinger", "--list").splitlines()
    layer = next(
        layer for layer in layers
        if layer.startswith("SurfaceView[org.vera20k/") and "(BLAST)" in layer
    )
    start = local_ref_bytes(pid)
    first_frame = last_frame(layer)
    time.sleep(120)
    end = local_ref_bytes(pid)
    rendered_ns = last_frame(layer) - first_frame
    if shell("pidof", "org.vera20k") != pid or not first_frame or rendered_ns < 110_000_000_000:
        raise RuntimeError("Keep the same game process rendering the main menu for two minutes")
    print(f"JNI local reference tables: {start // 1024} -> {end // 1024} KiB")
    if end - start > 1024 * 1024:
        raise SystemExit("JNI local reference tables grew by more than 1 MiB")


if __name__ == "__main__":
    main()
