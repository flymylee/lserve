# Configuration

`lserve` has three configuration layers, in strict precedence order:

```text
CLI > ./.lserve > automatic defaults
```

Only the `.lserve` file directly inside the selected document root is read. Parent directories are never searched.

## Document root

```sh
lserve
lserve ./dist
```

The current directory is used when no path is supplied. The path is canonicalized before the server starts.

## Supported keys

| `.lserve` key | CLI equivalent | Default | Meaning |
| --- | --- | --- | --- |
| `bind=ADDRESS` | `--bind ADDRESS` | `127.0.0.1` | Loopback/private/shared bind only |
| `http-port=PORT` | `--http-port PORT` | `80` (privileged) / `8080` | Redirect/origin HTTP port |
| `https-port=PORT` | `--https-port PORT`, `-p PORT` | `443` (privileged) / `8443` | HTTPS port |
| `http-only=true` | `--http-only` | `false` | Serve HTTP directly instead of redirecting to HTTPS |
| `symlinks=false` | `--no-symlinks` | `true` | Disable following symbolic links |
| `markdown=false` | `--no-markdown` | `true` | Serve Markdown as raw files |
| `markdown-theme=auto|light|dark` | `--markdown-theme ...` | `auto` | Markdown page theme |
| `access-log=PATH` | `--access-log PATH` | off | Additional access log file |
| `error-log=PATH` | `--error-log PATH` | off | Additional internal-error log file |
| `log-max-size=SIZE` | `--log-max-size SIZE` | `1M` | Rotation threshold |
| `log-generations=N` | `--log-generations N` | `5` | Rotated files retained |
| `verbose-errors=true` | `--verbose-errors` | `false` | Limited internal detail in 5xx responses |
| `route.HOST[/PATH]=URL` | `--route HOST[/PATH]=URL` | none | Host/path loopback HTTP proxy route |

`--cert PATH --key PATH` is CLI-only. Both must be supplied together.

Relative log paths are resolved from the document root.

## Boolean values

The parser accepts:

```text
true:  true yes on 1
false: false no off 0
```

## Log size values

Binary units are used. Examples:

```text
1024
4K
4KiB
1M
1MiB
1G
```

## Route precedence

Routes from `.lserve` are loaded first. A repeated CLI `--route` for the same normalized `HOST[/PATH]` selector replaces that selector only; unrelated routes remain active. Path selectors use segment-boundary prefix matching and the longest matching path prefix wins.

```sh
lserve --route admin.local.example/api=http://127.0.0.1:3001
```

## Generate an example

```sh
lserve config
lserve config --write
lserve config --write path/to/.lserve
```

`--write` refuses to overwrite an existing file.

The repository's [`.lserve.example`](../.lserve.example) is generated from the same source constant used by `lserve config` and should remain byte-for-byte synchronized.
