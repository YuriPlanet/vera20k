"""Compatibility imports for existing RMG/projectile fixtures.

New oracles use tools.native_oracle directly. Existing call arguments and result
payloads are preserved; identity and completion checks now apply centrally.
Importing this legacy adapter still resolves the retail path.
"""
from pathlib import Path
import sys

if not __package__:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.native_oracle import (
    IMAGE_BASE, IMAGE_SIZE, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE,
    RET_MAGIC, NATIVE_FPCW, call, configured_gamemd, image_bytes, load_image,
)

GAMEMD = configured_gamemd()
_configured_gamemd = configured_gamemd
_image_bytes = image_bytes
_load_image = load_image


if __name__ == '__main__':
    import runpy
    runpy.run_module('tools.rmg_oracle.gen_rng_vectors', run_name='__main__')
