#!/usr/bin/env bash
set -euo pipefail

# Starting a lingering user manager is appropriate only on an ephemeral CI host.
if [[ "${GITHUB_ACTIONS:-}" != "true" || -z "${GITHUB_ENV:-}" ]]; then
  echo "CI containment setup requires GITHUB_ACTIONS and GITHUB_ENV" >&2
  exit 1
fi

test -r /sys/fs/cgroup/cgroup.controllers
uid=$(id -u)
export XDG_RUNTIME_DIR="/run/user/$uid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
if ! systemctl --user show-environment >/dev/null 2>&1; then
  sudo install -d -m 0755 "/run/systemd/system/user@$uid.service.d"
  printf '[Service]\nDelegate=cpu memory pids\n' |
    sudo tee "/run/systemd/system/user@$uid.service.d/verification.conf" >/dev/null
  sudo systemctl daemon-reload
  sudo loginctl enable-linger "$(id -un)"
  sudo systemctl start "user@$uid.service"
fi
systemctl --user show-environment >/dev/null
printf 'XDG_RUNTIME_DIR=%s\nDBUS_SESSION_BUS_ADDRESS=%s\n' \
  "$XDG_RUNTIME_DIR" "$DBUS_SESSION_BUS_ADDRESS" >> "$GITHUB_ENV"

# The worker checks the effective cgroup limits and whole-unit cleanup.
python3 scripts/verify_local.py --seconds 30 -- true
