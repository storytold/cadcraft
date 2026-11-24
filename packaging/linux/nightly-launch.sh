#!/bin/sh
# The launcher inside CADCraft Nightly's flat tar. An unpacked copy starts the GUI the flavor's way
# -- with this channel's own launch profile -- so it never reads or writes stable's settings,
# window state or recovery files. The tar ships the real binary beside it.
set -eu

here="$(dirname "$0")"
case "${1-}" in
  # The Launcher already named the profile (its version query does too); never pass it twice. A
  # plain version query answers without touching any profile.
  --launch-profile | --version | -V) exec "$here/cadcraft" "$@" ;;
esac
profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
exec "$here/cadcraft" --launch-profile nightly-v1 --profile-root "$profile_base/cadcraft-nightly" -- "$@"
