#!/bin/sh
# The Arch Nightly launcher: the GUI starts with this channel's own launch profile, so the AUR
# package never reads or writes stable's settings, window state or recovery files.
set -eu

real_bin="$(dirname "$0")/.cadcraft-nightly/cadcraft"
case "${1-}" in
  # The Launcher already named the profile (its version query does too); never pass it twice. A
  # plain version query answers without touching any profile.
  --launch-profile | --profile-root | --version | -V) exec "$real_bin" "$@" ;;
esac
profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
exec "$real_bin" --launch-profile nightly-v1 --profile-root "$profile_base/cadcraft-nightly" -- "$@"
