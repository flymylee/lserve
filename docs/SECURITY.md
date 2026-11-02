# Security model

`lserve` is designed for **local and private-network use**, not as a directly Internet-facing production edge.

## Network boundary

Default bind:

```text
127.0.0.1
```

Explicit binds are restricted to:

- IPv4 loopback `127.0.0.0/8`
- RFC1918 `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- shared address space `100.64.0.0/10` (including Tailscale IPv4)
- IPv6 loopback `::1`
- IPv6 ULA `fc00::/7`

Wildcard and public addresses are rejected. In particular, `0.0.0.0` and `::` are not supported.

For temporary external access, keep `lserve` private and put a tunnel/access proxy in front of it. Authentication, SSO, ACLs, sessions, rate limiting, and public edge hardening belong in that layer.

## Static filesystem

- URL path traversal is rejected after percent decoding.
- Automatic directory listing is never generated.
- Static write methods are not implemented.
- Symlinks are intentionally followed by default for local convenience; use `--no-symlinks` when the document tree should be self-contained.
- `.lserve`, `.certs`, `.syntaxes`, the reserved `__lserve` URL namespace, configured log files, and recognized rotated logs are not served.

## Error disclosure

Client responses are terse by default. Detailed filesystem, TLS, proxy, and runtime failures go to stderr and the optional error log.

```sh
lserve --verbose-errors
```

is an opt-in development aid; it may expose more internal detail and should not be enabled unnecessarily behind an external access layer.

## Proxy boundary

Proxy targets must use `http://`. The configured hostname is resolved for each connection, and the actual destination selected by `lserve` must be loopback. This prevents a route from turning `lserve` into a general remote/LAN proxy.

Incoming `X-Forwarded-*` values are not trusted. `lserve` replaces the forwarding headers it provides to the upstream with connection facts it observed itself.

`TRACE` and `CONNECT` are rejected for proxy routes.

## TLS and private keys

The Root CA private key is generated locally and is never embedded in the executable. Do not copy or install `rootCA-key.pem` into a trust store. Only the public `rootCA.pem`/exported `.crt` should be installed as a trusted CA.

Private keys written by `lserve` are mode `0600` on Unix. Rooted-Android fallback state under `/data/local/tmp/lserve/` is owned by `0:0` and the directory is mode `0700`; it is not assigned to Android shell/Shizuku UID 2000.

## TLS cryptographic provider

The v1.0.0 manifest explicitly enables rustls with the `aws_lc_rs` and `prefer-post-quantum` features. With the rustls AWS-LC provider, that preference puts the hybrid `X25519MLKEM768` key-exchange group ahead of classical-only groups when the peer supports it, while retaining normal compatibility fallback. This concerns TLS key exchange; it does not turn the local CA/leaf signature scheme into a post-quantum certificate format.

## Public exposure

Recommended pattern:

```text
Internet
   ↓
Tunnel / Access Proxy
(authentication / authorization / public TLS / rate limiting)
   ↓
127.0.0.1 or private lserve address
```

For large-scale production, move the standard HTTP/TLS concepts to a production web server/proxy rather than expanding `lserve` into one.
