# Changelog

All notable changes to `lserve` will be documented here.

The project follows Semantic Versioning from v1.0.0 onward.

## [1.0.0] - Unreleased

First stable-contract release after the pre-1.0 prototype series.

### Added

- local/private bind policy
- static HTTPS server with HTTP redirect and HTTP-only origin mode
- local CA and project/global leaf certificate management
- static CORS, HEAD/OPTIONS, symlink opt-out, hidden server-state paths
- single byte-range streaming for media/large files
- server-side GFM-style Markdown rendering and syntax highlighting
- Host/path-based loopback-only HTTP reverse proxy with longest-prefix path matching
- stable default ports `8080/8443` regardless of root/non-root execution
- rooted-Android certificate-store selection with Termux CA reuse and `/data/local/tmp/lserve` fallback
- canonical `lserve ca export PATH` and explicit `lserve flush all` commands
- context-aware Bash completion for document roots, paths, enums, subcommands, and `HOST=http://` routes
- streaming/SSE/WebSocket proxy support
- access/error file logging with timestamp rotation
- verbose error opt-in
- shell completion and annotated config generation
- English/Korean documentation and release/license checklists
