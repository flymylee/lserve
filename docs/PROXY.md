# Reverse proxy

`lserve` can map request `Host` values, optionally combined with a path prefix, to HTTP applications running on the **same machine**.

## Configuration

```ini
route.local.example=http://127.0.0.1:3000
route.local.example/api=http://127.0.0.1:4000
route.local.example/api/admin=http://127.0.0.1:5000
route.admin.local.example=http://localhost:3001
```

or:

```sh
lserve \
  --route local.example=http://127.0.0.1:3000 \
  --route local.example/api=http://127.0.0.1:4000 \
  --route admin.local.example=http://localhost:3001
```

An unmatched Host falls back to static serving. If a Host has only path-specific routes, unmatched paths on that Host also fall back to static serving.


## Path-prefix matching

A route selector may be either `HOST` or `HOST/PATH`. Path matching is case-sensitive and respects segment boundaries:

```text
/api        matches /api and /api/...
/api        does not match /apix
```

When multiple routes match, the longest path prefix wins:

```ini
route.local.example/api=http://127.0.0.1:4000
route.local.example/api/admin=http://127.0.0.1:5000
```

```text
/api/users       -> :4000
/api/admin/users -> :5000
```

The matched prefix is **not stripped**. The upstream receives the original path and query unchanged. A Host-only route behaves like a `/` fallback for that Host.

## Upstream scope

The route URL must:

- use `http://`
- contain only an origin (`host[:port]`), with no configured path/query
- resolve at connection time to a loopback address

A custom hostname is allowed:

```ini
route.admin.local.example=http://backend.local.example:3001
```

but `backend.local.example` must resolve to `127.0.0.0/8` or `::1` when the connection is made.

Remote, LAN/private-network, Internet, HTTPS, and load-balanced upstreams are deliberately out of scope.

## Request semantics

Normal application methods and request bodies are streamed to the upstream. `TRACE` and `CONNECT` are rejected.

The original `Host` is retained for virtual-host-aware applications. `lserve` supplies:

```text
X-Forwarded-Host
X-Forwarded-Proto
X-Forwarded-For
X-Forwarded-Port
```

Incoming values for those fields are not trusted as authoritative connection facts.

When `--http-only` is used, `X-Forwarded-Proto` reflects the direct connection observed by `lserve` (`http`). `lserve` intentionally does not trust an arbitrary inbound `X-Forwarded-Proto`; an upstream application that needs the outer tunnel scheme should obtain that information from the trusted access layer by an explicit deployment contract.

## Streaming, SSE, WebSocket

The proxy keeps request/response bodies streaming rather than buffering them in full. This supports ordinary streaming responses and SSE.

For WebSocket Upgrade requests, `lserve` performs HTTP/1.1 upgrades on both sides and then tunnels bytes bidirectionally. This is intended to support development servers such as Vite/HMR.

## CORS

`lserve` does not add or replace CORS headers on proxied responses. The upstream application owns its CORS policy.
