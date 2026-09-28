#!/bin/sh
set -eu
exec python3 "$(dirname "$0")/plugins/tbg-notify/scripts/install.py" "$@"
