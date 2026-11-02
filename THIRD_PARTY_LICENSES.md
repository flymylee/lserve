# Third-party licenses

`lserve` itself is distributed under the MIT License; see [`LICENSE`](LICENSE).

This file separates two things deliberately:

1. a **source-level inventory of direct dependencies** declared by the v1.0.0 `Cargo.toml`; and
2. the **release-time resolved inventory**, which must be generated from the final `Cargo.lock` after `cargo` resolves all direct and transitive dependencies.

The current development environment could not run Cargo, so no `Cargo.lock`-exact transitive license manifest is claimed here yet. **Do not treat the table below as a complete binary attribution report.** Before publishing a binary release, update this file from the resolved dependency graph (`Cargo.lock`).

## Direct Rust dependencies

The following reflects the licenses advertised by the upstream crates for the dependency families declared in `Cargo.toml`.

| Crate | Manifest requirement | Upstream license | Project |
| --- | --- | --- | --- |
| `anyhow` | `1.0` | MIT OR Apache-2.0 | <https://github.com/dtolnay/anyhow> |
| `bytes` | `1.12` | MIT | <https://github.com/tokio-rs/bytes> |
| `comrak` | `0.55` | BSD-2-Clause | <https://github.com/kivikakk/comrak> |
| `futures-util` | `0.3` | MIT OR Apache-2.0 | <https://github.com/rust-lang/futures-rs> |
| `http-body-util` | `0.1` | MIT | <https://github.com/hyperium/http-body> |
| `hyper` | `1.11` | MIT | <https://github.com/hyperium/hyper> |
| `hyper-util` | `0.1.20` | MIT | <https://github.com/hyperium/hyper-util> |
| `libc` | `0.2` | MIT OR Apache-2.0 | <https://github.com/rust-lang/libc> |
| `mime_guess` | `2.0` | MIT | <https://github.com/abonander/mime_guess> |
| `percent-encoding` | `2.3` | MIT OR Apache-2.0 | <https://github.com/servo/rust-url> |
| `rcgen` | `0.14.10` | MIT OR Apache-2.0 | <https://github.com/rustls/rcgen> |
| `rustls` | `0.23.27` | Apache-2.0 OR ISC OR MIT | <https://github.com/rustls/rustls> |
| `rustls-pemfile` | `2.2` | MIT OR Apache-2.0 OR ISC | <https://github.com/rustls/pemfile> |
| `syntect` | `5.3` | MIT | <https://github.com/trishume/syntect> |
| `time` | `0.3.55` | MIT OR Apache-2.0 | <https://github.com/time-rs/time> |
| `tokio` | `1.53` | MIT | <https://github.com/tokio-rs/tokio> |
| `tokio-rustls` | `0.26.5` | MIT OR Apache-2.0 | <https://github.com/rustls/tokio-rustls> |
| `tokio-util` | `0.7` | MIT | <https://github.com/tokio-rs/tokio> |
| `x509-parser` | `0.18.1` | MIT OR Apache-2.0 | <https://github.com/rusticata/x509-parser> |

Versions in the second column are Cargo requirements, not a promise that the eventual lockfile resolves to exactly that textual version unless an exact version was specified.

## Syntax definitions and themes

`syntect` is built with its default syntax/theme packages enabled. Those data are distributed through the `syntect` dependency and must be covered by the final dependency/asset audit, including any notices shipped by the resolved crate package.

Project/user custom syntax files loaded from:

```text
~/.local/share/lserve/syntaxes/
./.syntaxes/
```

are **not** redistributed by `lserve`; the user is responsible for the licenses of custom files they add.

## D2Coding

The v1.0.0 source tree does **not** contain or redistribute a D2Coding font file. The Markdown CSS merely prefers a locally installed font named `D2Coding` and then falls back to other monospace fonts.

If a future release embeds D2Coding, that release must add the corresponding SIL Open Font License 1.1 text/notice and record the exact font source/version before distribution.

## Markdown CSS

The current Markdown styling is implemented in the `lserve` source rather than vendoring a copy of GitHub's CSS package. Documentation describes the visual goal as **GitHub-like**, not as an exact GitHub implementation.

If a future release vendors a third-party stylesheet, theme, grammar bundle, icon, or font, add it to this file with:

- name and version/commit
- upstream URL
- license
- copyright notice where required
- whether it was modified
- exact files included

## Release-time verification

The final release inventory must be based on the committed `Cargo.lock`. A recommended workflow is to use a license-audit tool such as `cargo-about` or `cargo-deny`, manually review non-Rust assets, and retain all license/notice text required by the selected licenses.
