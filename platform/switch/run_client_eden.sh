#!/usr/bin/env bash
# Launch the current VERA20k client NSP with resources supplied on Eden's SD card.
set -euo pipefail
switch_port_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
export XDG_DATA_HOME="$switch_port_root/target/emulator/master-xdg-data"
export XDG_CONFIG_HOME="$switch_port_root/target/emulator/master-xdg-config"
eden_app="${EDEN_APP:-$switch_port_root/../../../../toolchains/switch/eden-src/build-master-cli/bin/eden-cli}"
exec "$eden_app" -u 0 -g "$switch_port_root/dist/ver20k-Yuris-Revenge-v0.0.6.nsp"
