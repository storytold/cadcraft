#!/bin/sh
# The launcher inside CADCraft Nightly's flat tar. The tar's layout is the kit's: `cadcraft` is this
# script and `cadcraft-bin` the GUI beside it. An unpacked copy starts the GUI with this channel's
# own launch profile, so it never reads or writes stable's settings, window state or recovery files.
set -eu

here="$(dirname "$0")"
real_bin="$here/cadcraft-bin"
# Every path is resolved beside this script: there is no relative fallback, so a copy unpacked
# anywhere cannot be pointed at another program by its parent folder.
if [ ! -f "$real_bin" ] || [ -L "$real_bin" ]; then
  echo "error: cadcraft-bin is missing beside $0" >&2
  exit 2
fi
case "${1-}" in
  # The Launcher already named the profile (its version query does too); never pass it twice. A
  # plain version query answers without touching any profile.
  --launch-profile | --profile-root | --version | -V) exec "$real_bin" "$@" ;;
esac
profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_base/cadcraft-nightly" -- "$@"
