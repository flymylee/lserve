# Logging

Console logging is always enabled:

```text
access/server activity -> stdout
internal/program errors -> stderr
```

File logging is additive and opt-in.

## Access log

```sh
lserve --access-log ./logs/access.log
```

or:

```ini
access-log=./logs/access.log
```

The access stream records request outcomes for static and proxy paths, including redirects, 4xx, and 5xx responses. A normal HTTP 4xx is not treated as an internal program error.

## Error log

```sh
lserve --error-log ./logs/error.log
```

or:

```ini
error-log=./logs/error.log
```

The error stream is reserved for failures such as listener/TLS/certificate/filesystem/proxy/logger errors. If an internal failure causes an HTTP 500/502, the request status appears in the access log and the underlying cause appears in the error stream.

## Rotation

Defaults:

```text
max size    1 MiB
retention   5 rotated generations
```

The active filename remains unchanged. Rotation renames it with a local timestamp:

```text
access.log
access.log.20260924_231500
access.log.20260924_231500.1
```

The collision suffix is used only when more than one rotation receives the same timestamp.

Configure:

```sh
lserve --log-max-size 4M --log-generations 10
```

or:

```ini
log-max-size=4M
log-generations=10
```

Retention is generation-based, not time-based; 5 generations may represent months at very low traffic or only a short period at high traffic.

## Concurrency

Request handlers send log events to a single logger task. That task serializes console/file writes and rotation so concurrent requests do not race the rotation operation.

## Static-file exclusion

If a configured log path is inside the document root, the active log and timestamp-rotation names are treated as private server state and return 404 through the static server.
