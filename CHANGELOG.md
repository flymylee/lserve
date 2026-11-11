# Changelog

All notable changes to `lserve` will be documented here.

The project follows Semantic Versioning (`{major}.{minor}.{patch}`) from v1.0.0 onward.

## [1.0.2] - 2026-11-11

### Added

- Repeated CLI `--bind IP[/PREFIX]` and repeated `.lserve` `bind=IP[/PREFIX]`, with full CLI-list override.
- Private CIDR selectors expand to UP locally assigned IPv4/IPv6 addresses at startup, then bind individual HTTP/HTTPS sockets.
- De-duplicate overlapping addresses, pre-bind every listener before serving, and cover all non-loopback bind IPs in project certificate SANs.

### Security and behavior

- Reject public-overlapping CIDRs and wildcard listeners; unchanged loopback-only reverse proxy upstream policy.
- Fail startup if any CIDR matches no local IP or any listener fails to bind.
- CIDR matches are startup snapshots: restart after interface/IP changes.

## [1.0.1] - 2026-11-02

### Changed

- Unified global certificate and syntax data directory to `~/.local/share/lserve/` across Linux and Termux (removed `.local/lserve/` split).
- Added runtime Android OS detection so a single static `*-linux-musl` binary can run seamlessly on both Android (`aarch64`) and Linux servers (e.g., Ubuntu/RHEL/Oracle A1 `aarch64`).
- Updated automatic default port selection: uses `80/443` when running with privileged port capability (`EUID == 0` or `CAP_NET_BIND_SERVICE` via `setcap` on Linux; both `EUID == 0` and `CAP_NET_BIND_SERVICE` on Android), and falls back to `8080/8443` otherwise.
- Preserved the invoking user's `~/.local/share/lserve/` directory and file ownership when running under `sudo` (`SUDO_USER`/`SUDO_UID`/`SUDO_GID`) on Linux.
- Cleaned up Rust 2024 edition Clippy lints (`let_chains`, unused imports).

## [1.0.0] - 2026-09-26

First stable-contract release after the pre-1.0 prototype series.

### Added

- local/private bind policy
- static HTTPS server with HTTP redirect and HTTP-only origin mode
- local CA and project/global leaf certificate management
- static CORS, HEAD/OPTIONS, symlink opt-out, hidden server-state paths
- single byte-range streaming for media/large files
- server-side GFM-style Markdown rendering and syntax highlighting
- Host/path-based loopback-only HTTP reverse proxy with longest-prefix path matching
- rooted-Android certificate-store selection with Termux CA reuse and `/data/local/tmp/lserve` fallback
- canonical `lserve ca export PATH` and explicit `lserve flush all` commands
- context-aware Bash completion for document roots, paths, enums, subcommands, and `HOST=http://` routes
- streaming/SSE/WebSocket proxy support
- access/error file logging with timestamp rotation
- verbose error opt-in
- shell completion and annotated config generation
- English/Korean documentation and license notices
