#!/bin/sh
# Hagency native service installer (ADR-127 systemd / ADR-133 launchd).
#
# FLOW (both OSes): init fresh state -> render the unit with explicit
# placeholders -> install -> enable -> START GATE IS /ready, never /health
# (health is 200-while-live and proves nothing at cutover; ready is the 503
# boundary that names components).
#
# REFUSALS (named, not silent):
#   - non-empty state directory that is not fresh native state (init refuses)
#   - missing binary or unwritable state dir
#   - an existing unit whose rendered ExecStart differs, unless --overwrite
#   - systemd absent on Linux (the alternative is a foreground run, not a
#     pretended unit)
#
# PLACEHOLDERS ARE NOT DEFAULTS: --install-dir and --state-dir are required
# except where noted; the loopback listen is hard-coded in the units.
set -eu

USAGE="usage: $0 --install-dir DIR --state-dir DIR [--overwrite]
  Linux : writes <systemd-dir>/hagency-native.service (default /etc/systemd/system), daemon-reload, enable --now
  macOS : writes ~/Library/LaunchAgents/io.hagency.native.plist, bootstrap"

INSTALL_DIR=""
STATE_DIR=""
SYSTEMD_DIR="${HAGENCY_SYSTEMD_DIR:-/etc/systemd/system}"
OVERWRITE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --install-dir) INSTALL_DIR="${2:?}"; shift 2 ;;
    --state-dir)   STATE_DIR="${2:?}";   shift 2 ;;
    --systemd-dir) SYSTEMD_DIR="${2:?}"; shift 2 ;;
    --overwrite)   OVERWRITE=1; shift ;;
    *) echo "refused: unknown argument $1" >&2; echo "$USAGE" >&2; exit 2 ;;
  esac
done
[ -n "$INSTALL_DIR" ] || { echo "refused: --install-dir is required (placeholders are not defaults)" >&2; exit 2; }
[ -n "$STATE_DIR" ]   || { echo "refused: --state-dir is required (placeholders are not defaults)" >&2; exit 2; }

BIN="$INSTALL_DIR/hagency"
[ -x "$BIN" ] || { echo "refused: missing binary $BIN" >&2; exit 1; }
[ -d "$STATE_DIR" ] && [ -n "$(ls -A "$STATE_DIR" 2>/dev/null)" ] \
  && { echo "refused: state directory $STATE_DIR is not empty; hagency init requires a fresh empty dir" >&2; exit 1; }

# init first (fail-closes on a non-empty dir; mints operator.token).
"$BIN" init --state-dir "$STATE_DIR" || { echo "refused: hagency init failed for $STATE_DIR" >&2; exit 1; }

wait_ready() {
  i=0
  while [ "$i" -lt 60 ]; do
    if "$1" -fsS -o /dev/null http://127.0.0.1:13300/ready 2>/dev/null; then
      return 0
    fi
    i=$((i+1)); sleep 1
  done
  echo "refused: /ready did not answer 200 within 60s (start gate; /health proves nothing)" >&2
  return 1
}

case "$(uname -s)" in
  Linux)
    command -v systemctl >/dev/null 2>&1 || { echo "refused: systemd absent; run the binary in the foreground instead" >&2; exit 1; }
    UNIT="$SYSTEMD_DIR/hagency-native.service"
    if [ -e "$UNIT" ] && [ "$OVERWRITE" -ne 1 ]; then
      echo "refused: $UNIT exists and --overwrite was not given" >&2; exit 1
    fi
    sed -e "s|__INSTALL_DIR__|$INSTALL_DIR|g" -e "s|__STATE_DIR__|$STATE_DIR|g" -e "s|__USER__|$(id -un)|g" \
      "$(dirname "$0")/../deploy/hagency-native.service" > "$UNIT"
    systemctl daemon-reload
    systemctl enable --now hagency-native.service
    wait_ready curl && systemctl is-active --quiet hagency-native.service \
      || { echo "refused: unit enabled but not active after the /ready gate" >&2; exit 1; }
    echo "installed: $UNIT (state: $STATE_DIR)"
    ;;
  Darwin)
    PLIST="$HOME/Library/LaunchAgents/io.hagency.native.plist"
    mkdir -p "$HOME/Library/LaunchAgents" "$INSTALL_DIR/logs"
    if [ -e "$PLIST" ] && [ "$OVERWRITE" -ne 1 ]; then
      echo "refused: $PLIST exists and --overwrite was not given" >&2; exit 1
    fi
    sed -e "s|__INSTALL_DIR__|$INSTALL_DIR|g" -e "s|__STATE_DIR__|$STATE_DIR|g" \
      "$(dirname "$0")/../deploy/io.hagency.native.plist" > "$PLIST"
    launchctl bootstrap "gui/$(id -u)" "$PLIST"
    wait_ready curl || { echo "refused: agent bootstrapped but /ready gate failed" >&2; exit 1; }
    echo "installed: $PLIST (state: $STATE_DIR)"
    echo "stop with: launchctl bootout gui/$(id -u) $PLIST  # NOT kill-by-pid; KeepAlive restarts a killed process"
    ;;
  *) echo "refused: unsupported OS $(uname -s)" >&2; exit 1 ;;
esac
