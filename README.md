# cpu-floor

Keeps total CPU utilization at or above a floor (default 25%) so an
[OCI Always Free](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm#compute__idleinstances)
instance is never reclaimed as idle.

Every second it reads `/proc/stat`, subtracts its own CPU time to find what everything
else is using, and burns only the gap. When your real workloads already exceed the
floor it burns nothing. All threads run under `SCHED_IDLE` with nice 19, so any other
process preempts them.

Ships as a single static binary (musl) for `x86_64` and `aarch64`, so it runs on any
Linux distro: Oracle Linux, Ubuntu, Debian, and others.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/yehudaNahon/cpu-floor/main/install.sh | sudo bash
```

The installer picks the binary for your architecture from the latest release, verifies
its checksum, and installs it as a hardened systemd service that starts on boot. Run it
again at any time to upgrade.

Installer options, passed as environment variables:

| Variable | |
|---|---|
| `CPU_FLOOR_TARGET` | Set the floor, e.g. `curl … \| sudo CPU_FLOOR_TARGET=30 bash` |
| `CPU_FLOOR_VERSION` | Install a specific release tag instead of the latest, e.g. `v0.1.0` |
| `CPU_FLOOR_BINARY` | Install a local binary instead of downloading one |

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/yehudaNahon/cpu-floor/main/install.sh | sudo bash -s -- --uninstall
```

## Configure

Settings live in `/etc/cpu-floor.env`:

```sh
sudo nano /etc/cpu-floor.env
sudo systemctl restart cpu-floor
journalctl -u cpu-floor -f
```

| Env var | Flag | Default | |
|---|---|---|---|
| `CPU_FLOOR_TARGET` | `--target` | `25` | Minimum total CPU %, across all cores |
| `CPU_FLOOR_INTERVAL_MS` | `--interval-ms` | `1000` | Measure/adjust cadence |
| `CPU_FLOOR_PERIOD_MS` | `--period-ms` | `100` | Burn/sleep cycle per worker |
| `CPU_FLOOR_THREADS` | `--threads` | one per CPU | Burner threads |
| `CPU_FLOOR_REPORT_SECS` | `--report-secs` | `60` | Status log interval (`0` disables) |

`RUST_LOG=debug` logs every tick.

## Notes for OCI

- Oracle reclaims an Always Free instance only when CPU, network, and (on A1) memory
  are all under 20% at the 95th percentile over 7 days. Keeping CPU up is enough.
- On the burstable E2.1.Micro shape, Oracle's metric may account for usage differently
  than the guest sees it. After a day, check the instance's CPU Utilization chart in
  the OCI console and raise `CPU_FLOOR_TARGET` if needed.

## Development

```sh
cargo test
./build-linux.sh aarch64   # or x86_64; static build via Docker into dist/
```

CI lints, tests, and builds static binaries for both architectures on every push.
To publish a release, bump `version` in `Cargo.toml`, then:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The tag build attaches `cpu-floor-x86_64-linux`, `cpu-floor-aarch64-linux`,
`install.sh`, and `SHA256SUMS` to a GitHub release.
