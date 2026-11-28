"""Android 11+ device regression. With a debug build at the main menu, run
python android/tools/test_clean_exit.py, then select Exit Game and confirm.
"""

import re
import subprocess
import time


PACKAGE = "org.vera20k"
SHUTDOWN = "VERA20k shut down cleanly"


def shell(*args):
    return subprocess.check_output(["adb", "shell", *args], encoding="utf-8", errors="replace").strip()


def main():
    pid = shell("pidof", PACKAGE)
    print("Select Exit Game and confirm on the device.", flush=True)
    for _ in range(90):
        history = shell("dumpsys", "activity", "exit-info", PACKAGE)
        entry = re.search(rf"\bpid={pid}\b[^\n]*\n\s*process=org\.vera20k (.*)", history)
        if entry:
            break
        time.sleep(1)
    else:
        raise SystemExit("No exit record for the running game")

    reason = entry.group(1)
    print(reason)
    if not re.search(r"\breason=1\b.*\bstatus=0\b", reason):
        raise SystemExit("Expected the game to exit normally with status 0")
    log = shell("run-as", PACKAGE, "tail", "-n", "5", "files/logs/ra2.log")
    if SHUTDOWN not in log:
        raise SystemExit("Engine shutdown did not finish")

    shell("am", "start", "-W", "-n", f"{PACKAGE}/.VeraActivity")
    new_pid = shell("pidof", PACKAGE)
    if new_pid == pid:
        raise SystemExit("The next launch must start in a new process")
    for _ in range(45):
        log = shell("run-as", PACKAGE, "tail", "-n", "200", "files/logs/ra2.log")
        if "GPU initialized:" in log.rsplit(SHUTDOWN, 1)[-1] and shell("pidof", PACKAGE) == new_pid:
            break
        time.sleep(1)
    else:
        raise SystemExit("The new game process did not initialize the renderer")
    print(f"Cold relaunch: {pid} -> {new_pid}")


if __name__ == "__main__":
    main()
