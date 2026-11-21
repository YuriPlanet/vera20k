"""Extract PAL/SHP-like ASCII strings from a PE binary for palette research."""
import re
import sys

path = sys.argv[1]
needles = sys.argv[2:] or ["MPLS", "LDSCRN", "MPYL"]
data = open(path, "rb").read()
# ASCII runs of filename-safe bytes ending in a known asset extension
pat = re.compile(rb"[ -~]{3,40}")
found = set()
for m in pat.finditer(data):
    s = m.group()
    if not re.search(rb"\.(PAL|SHP|PCX)$", s, re.I):
        continue
    u = s.decode("ascii", "ignore").upper()
    if any(n.upper() in u for n in needles):
        found.add((m.start(), u))
for off, u in sorted(found):
    print(f"{off:#08x}  {u}")
