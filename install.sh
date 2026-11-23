#!/usr/bin/env bash
# Installs cpu-floor as a systemd service, or removes it with --uninstall.
# curl -fsSL https://raw.githubusercontent.com/yehudaNahon/cpu-floor/main/install.sh | sudo bash
set -euo pipefail

REPO="${CPU_FLOOR_REPO:-yehudaNahon/cpu-floor}"
VERSION="${CPU_FLOOR_VERSION:-latest}"
BIN=/usr/local/bin/cpu-floor
UNIT=/etc/systemd/system/cpu-floor.service
CONFIG=/etc/cpu-floor.env

log() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

download() {
  if command -v curl >/dev/null; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null; then
    wget -qO "$2" "$1"
  else
    die "curl or wget is required"
  fi
}

uninstall() {
  systemctl disable --now cpu-floor 2>/dev/null || true
  rm -f "$BIN" "$UNIT"
  systemctl daemon-reload
  log "removed cpu-floor (kept $CONFIG)"
}

install_binary() {
  local arch asset base
  case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
  asset="cpu-floor-$arch-linux"
  if [ -n "${CPU_FLOOR_BINARY:-}" ]; then
    log "using local binary $CPU_FLOOR_BINARY"
    cp "$CPU_FLOOR_BINARY" "$tmp/$asset"
  else
    if [ "$VERSION" = latest ]; then
      base="https://github.com/$REPO/releases/latest/download"
    else
      base="https://github.com/$REPO/releases/download/$VERSION"
    fi
    log "downloading $asset ($VERSION)"
    download "$base/$asset" "$tmp/$asset"
    download "$base/SHA256SUMS" "$tmp/SHA256SUMS"
    (cd "$tmp" && grep " $asset\$" SHA256SUMS | sha256sum -c --quiet -) || die "checksum mismatch for $asset"
  fi

  install -m 0755 "$tmp/$asset" "$BIN"
  if command -v restorecon >/dev/null; then restorecon "$BIN" || true; fi
  log "installed $("$BIN" --version) to $BIN"
}

write_config() {
  if [ ! -f "$CONFIG" ]; then
    cat >"$CONFIG" <<'CONF'
# cpu-floor settings. Apply changes with: sudo systemctl restart cpu-floor
CPU_FLOOR_TARGET=25
#CPU_FLOOR_INTERVAL_MS=1000
#CPU_FLOOR_PERIOD_MS=100
#CPU_FLOOR_THREADS=4
#CPU_FLOOR_REPORT_SECS=60
CONF
    log "wrote default config to $CONFIG"
  fi
  if [ -n "${CPU_FLOOR_TARGET:-}" ]; then
    [[ $CPU_FLOOR_TARGET =~ ^[0-9]+(\.[0-9]+)?$ ]] || die "CPU_FLOOR_TARGET must be a number"
    sed -i -E "s/^#?CPU_FLOOR_TARGET=.*/CPU_FLOOR_TARGET=$CPU_FLOOR_TARGET/" "$CONFIG"
    log "set target to $CPU_FLOOR_TARGET%"
  fi
}

write_unit() {
  cat >"$UNIT" <<UNIT
[Unit]
Description=Keep CPU utilization above a floor
Documentation=https://github.com/$REPO
After=multi-user.target

[Service]
EnvironmentFile=-$CONFIG
ExecStart=$BIN
Restart=always
RestartSec=5
Nice=19
CPUSchedulingPolicy=idle
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
PrivateNetwork=yes
MemoryMax=32M

[Install]
WantedBy=multi-user.target
UNIT
}

[ "$(id -u)" -eq 0 ] || die "must run as root (pipe into 'sudo bash')"
command -v systemctl >/dev/null || die "systemd is required"

case "${1:-}" in
  --uninstall) uninstall; exit 0 ;;
  "") ;;
  *) die "unknown argument: $1 (only --uninstall is supported)" ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

install_binary
write_config
write_unit
systemctl daemon-reload
systemctl enable cpu-floor >/dev/null 2>&1
systemctl restart cpu-floor
sleep 1
systemctl is-active --quiet cpu-floor || die "service failed to start; see: journalctl -u cpu-floor"
log "cpu-floor is running. Logs: journalctl -u cpu-floor -f"
