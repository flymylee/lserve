# lserve

**A lightweight local/private HTTPS static server, Markdown viewer, and Host/path-based reverse proxy for local applications.**

`lserve` is deliberately small in scope: it serves local/private content well, terminates local TLS, and can route selected hostnames to HTTP applications running on the same machine. Authentication, public exposure, SSO, rate limiting, high availability, and large-scale production concerns belong in a separate layer.

> **Default safety boundary:** `lserve` binds to `127.0.0.1`. An explicit `--bind` may use only loopback/private/shared address ranges. Public and wildcard binds such as `0.0.0.0` are rejected.

[한국어 README](README_ko.md)

## Getting Started

### 1. Install Rust

`lserve` v1.0.0 targets Rust **1.88+**.

Termux:

```sh
pkg install rust
```

Ubuntu (distribution package or rustup):

```sh
sudo apt install cargo rustc
```

RHEL-family systems may use the distribution Rust packages or rustup. Check the version with:

```sh
rustc --version
```

### 2. Build and install

```sh
git clone https://github.com/flymylee/lserve.git
cd lserve
cargo build --release
install -Dm755 target/release/lserve ~/.local/bin/lserve
```

Ensure `~/.local/bin` is in `PATH`.

### 3. Serve a directory

```sh
cd ~/my-site
lserve
```

The default ports depend on whether the process can bind privileged ports:

```text
Unprivileged default:
  http://localhost:8080  ->  https://localhost:8443
  https://localhost:8443

Privileged default (root or CAP_NET_BIND_SERVICE on Linux; root + CAP_NET_BIND_SERVICE on Android):
  http://localhost:80    ->  https://localhost:443
  https://localhost
```

Use `--http-port` / `--https-port` explicitly if you intentionally want different ports.

The first HTTPS run normally creates a device-local CA and server certificate under:

```text
~/.local/share/lserve/
├── rootCA.pem
├── rootCA-key.pem
├── localhost.pem
└── localhost-key.pem
```

**Never share `rootCA-key.pem`.**

On **rooted Android**, `lserve` first looks for an existing complete Termux CA in `$TERMUX__HOME/.local/share/lserve/` (or `/data/data/com.termux/files/home/.local/share/lserve/`). If none exists, global certificates are stored in `/data/local/tmp/lserve/`, owned by `0:0`; that fallback directory is mode `0700`. See [docs/CERTIFICATES.md](docs/CERTIFICATES.md).

### 4. Trust the local CA

Show its path:

```sh
lserve ca
```

Export only the public CA certificate:

```sh
lserve ca export ~/Downloads/lserve-rootCA.crt
```

On Android/Termux, a typical shared-storage destination is:

```sh
lserve ca export ~/storage/downloads/lserve-rootCA.crt
```

Install the exported file as a **CA certificate** using your OS/browser trust controls. Never install or export `rootCA-key.pem`.

## What you get by default

- document root = current directory
- bind = `127.0.0.1`
- default ports = `80` / `443` when privileged (`root` / `CAP_NET_BIND_SERVICE`), otherwise `8080` / `8443`
- HTTP -> HTTPS `308` redirect
- static methods = `GET`, `HEAD`, `OPTIONS`
- open CORS for static responses
- `index.html` lookup, **no directory listing**
- URL traversal blocking
- symlinks followed by default; opt out with `--no-symlinks`
- `.lserve`, `.certs/`, `.syntaxes/`, and configured log files never served
- Markdown rendering enabled (`?raw` returns source)
- server-side syntax highlighting, light/dark themes
- single-range static streaming (`206`/`416`) for media/large files
- console access/error logging
- optional file logs with timestamp rotation
- optional Host/path-based reverse proxy to **loopback HTTP upstreams only**
- WebSocket Upgrade, SSE, and streaming proxy responses

## Markdown

Opening a Markdown file renders it server-side:

```text
https://localhost:8443/README.md
```

Fetch the original source with:

```text
https://localhost:8443/README.md?raw
```

Markdown rendering uses safe defaults: raw HTML is not enabled. Fenced code is highlighted server-side. The code font stack prefers locally installed **D2Coding**, then falls back to common monospaced fonts. The source tree does **not** redistribute a D2Coding font binary.

Disable Markdown rendering:

```sh
lserve --no-markdown
```

See [docs/MARKDOWN.md](docs/MARKDOWN.md).

## Reverse proxy

Example `.lserve`:

```ini
route.local.example.com=http://127.0.0.1:3000
route.local.example.com/api=http://127.0.0.1:4000
route.local.example.com/api/admin=http://127.0.0.1:5000
route.admin.local.example.com=http://localhost:3001
```

A route may be Host-only or `HOST/PATH`. Path routes use segment-boundary prefix matching, the longest matching prefix wins, and the original path is forwarded unchanged.

With matching host entries:

```text
127.0.0.1 local.example.com
127.0.0.1 admin.local.example.com
```

then:

```text
https://local.example.com/              -> http://127.0.0.1:3000/...
https://local.example.com/api/users     -> http://127.0.0.1:4000/api/users
https://local.example.com/api/admin/x   -> http://127.0.0.1:5000/api/admin/x
https://admin.local.example.com/        -> http://localhost:3001/...
https://localhost                       -> static fallback
```

A custom upstream hostname is allowed only when the address selected for the actual connection resolves to loopback. `lserve` does not proxy to remote, LAN, or Internet upstreams.

See [docs/PROXY.md](docs/PROXY.md).

## Private-network bind

The default remains localhost. For a deliberate private-network use case:

```sh
lserve --bind 100.100.10.20
```

Allowed ranges include loopback, RFC1918 IPv4, `100.64.0.0/10` (including Tailscale IPv4), IPv6 loopback, and IPv6 ULA. Public IPs, `0.0.0.0`, and `::` are rejected.

If you need experimental access from outside the private boundary, keep `lserve` private and place a tunnel/access proxy in front of it.

## Cloudflare Tunnel / HTTP-only origin

```sh
lserve --http-only
cloudflared tunnel --url http://127.0.0.1:8080
```

The tunnel owns public TLS/access; `lserve` remains a local origin.

## Static CORS

Static responses intentionally allow cross-origin reads:

```http
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, HEAD, OPTIONS
Access-Control-Allow-Headers: *
Access-Control-Max-Age: 86400
```

This supports browser tools and bookmarklets that fetch local resources. Proxy responses keep the upstream application's CORS policy unchanged.

## Logs

```sh
lserve \
  --access-log ./logs/access.log \
  --error-log ./logs/error.log
```

Console logging remains enabled. File logs rotate at 1 MiB by default and retain five timestamped generations:

```text
access.log
access.log.20260924_231500
access.log.20260923_101122
```

If log files are inside the document root, the active files and recognized rotated generations are automatically hidden from HTTP.

For limited internal detail in 5xx client responses during development:

```sh
lserve --verbose-errors
```

See [docs/LOGGING.md](docs/LOGGING.md).

## Configuration

Print the annotated example:

```sh
lserve config
```

Write `./.lserve` without overwriting an existing file:

```sh
lserve config --write
```

Precedence:

```text
CLI > ./.lserve > automatic defaults
```

Only the selected document root's `.lserve` is read; parent directories are never searched.

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) and [.lserve.example](.lserve.example).

## Certificates

```sh
lserve ca generate
lserve ca generate --force
lserve ca export PATH
lserve cert generate
lserve cert generate --force
lserve flush all
lserve flush cert
lserve flush ca
```

Project-specific hostnames use `./.certs/server.pem` and `server-key.pem`, signed by the same global Root CA. The certificate contains exact SANs; wildcard SANs are not generated automatically.

See [docs/CERTIFICATES.md](docs/CERTIFICATES.md).

## Shell completion

Bash completion is context-aware: the optional first positional completes directories, path-taking options complete filesystem paths, enums complete only valid values, and `--route` starts from `HOST=http://`.

```sh
lserve completions bash > ~/.local/share/bash-completion/completions/lserve
lserve completions zsh  > /path/to/_lserve
lserve completions fish > ~/.config/fish/completions/lserve.fish
```

## Security model

`lserve` is **local/private infrastructure**, not a directly Internet-facing production server. It intentionally does not implement user authentication, SSO, account management, a database, load balancing, remote upstream proxying, ACME, or public wildcard binding.

For temporary external access, put Cloudflare Tunnel or an access-control proxy in front. For large-scale production, migrate the standard HTTP/TLS routing concepts to nginx, Caddy, Envoy, Traefik, or another production-oriented stack.

See [docs/SECURITY.md](docs/SECURITY.md).

## Documentation

- [Configuration](docs/CONFIGURATION.md)
- [Security model](docs/SECURITY.md)
- [Reverse proxy](docs/PROXY.md)
- [Markdown](docs/MARKDOWN.md)
- [Certificates](docs/CERTIFICATES.md)
- [Logging](docs/LOGGING.md)
- [Changelog](CHANGELOG.md)

## Build verification note

The v1.0.0 source reconstruction was written against current Hyper/rustls/rcgen/Comrak/syntect APIs. Before publishing a release, run:

```sh
cargo fmt --check
cargo check
cargo test
cargo build --release
```

and commit the generated `Cargo.lock` for reproducible application builds.

## License

`lserve` is MIT-licensed. Copyright (c) 2026 Mooyeon LEE.

See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) and [NOTICE](NOTICE) for third-party dependency/asset notes.
