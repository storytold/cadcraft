#!/bin/sh
set -eu

real_bin="$(dirname "$0")/.cadcraft-nightly/cadcraft"
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "-V" ]; then
  exec "$real_bin" "$@"
fi
if [ "${CADCRAFT_CONFIG_DIR+x}" != "x" ]; then
  profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
  export CADCRAFT_CONFIG_DIR="$profile_base/cadcraft-nightly"
fi
exec "$real_bin" "$@"
