#!/bin/sh
# Finder opens this launcher in Terminal, preserving the local startup console.
set -eu
launcher_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
exec "$launcher_dir/mihomo-web-panel" --open
