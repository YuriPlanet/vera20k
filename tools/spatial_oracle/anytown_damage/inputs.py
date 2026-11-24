"""Explicit local retail inputs; proprietary files are never bundled here."""
import os
from pathlib import Path

ASSETS = Path(os.environ.get('VERA20K_ANYTOWN_INPUTS', 'target/anytown-native-inputs/extract'))
ATTACK_SHPS = ('120mm.shp', 'gunfire.shp', 'S_CLSN16.SHP', 'S_CLSN22.SHP',
               'H2O_EXP1.SHP', 'H2O_EXP2.SHP', 'H2O_EXP3.SHP')
