"""Android 11+ device regression. With a debug build at the main menu, run
python android/tools/test_clean_exit.py, then select Exit Game and confirm.
"""

import subprocess
import time


PACKAGE = "org.vera20k"
SHUTDOWN = "VERA20k shut down cleanly"


def shell(*args):
    return subprocess.check_output(["adb", "shell", *args], encoding="utf-8", errors="replace").strip()


def find_exit_record(history, pid):
    # Android 11 and 12 print one field per line. Later versions group fields.
    for record in history.split("ApplicationExitInfo #")[1:]:
        fields = record.split()
        if f"pid={pid}" in fields and f"process={PACKAGE}" in fields:
            return fields
    return None


def main():
    pid = shell("pidof", PACKAGE)
    print("Select Exit Game and confirm on the device.", flush=True)
    for _ in range(90):
        history = shell("dumpsys", "activity", "exit-info", PACKAGE)
        entry = find_exit_record(history, pid)
        if entry is not None:
            break
        time.sleep(1)
    else:
        raise SystemExit("No exit record for the running game")

    print(" ".join(field for field in entry if field.startswith(("reason=", "status="))))
    if "reason=1" not in entry or "status=0" not in entry:
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
