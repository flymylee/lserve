#!/usr/bin/env python3
"""Fast source/package consistency checks that do not require a Rust toolchain.

This is deliberately not a substitute for cargo fmt/check/test. It catches the
packaging/documentation regressions that previously made interrupted work hard
to resume.
"""
from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ERRORS: list[str] = []


def fail(message: str) -> None:
    ERRORS.append(message)


cargo_path = ROOT / "Cargo.toml"
with cargo_path.open("rb") as f:
    cargo = tomllib.load(f)
package = cargo.get("package", {})
if package.get("name") != "lserve":
    fail("Cargo package name is not lserve")
if package.get("version") != "1.0.1":
    fail("Cargo package version is not 1.0.1")
if package.get("rust-version") != "1.88":
    fail("Cargo rust-version is not 1.88")

required = [
    "README.md", "README_ko.md", "LICENSE", "NOTICE", "THIRD_PARTY_LICENSES.md",
    ".lserve.example", "CHANGELOG.md",
    "docs/CONFIGURATION.md", "docs/SECURITY.md", "docs/PROXY.md", "docs/MARKDOWN.md",
    "docs/CERTIFICATES.md", "docs/LOGGING.md",
]
if (ROOT / ".worklog").is_dir():
    required.extend([
        ".worklog/WORKLOG-lserve.md",
        ".worklog/CHECKPOINT.md",
        ".worklog/RELEASE_CHECKLIST.md",
        ".worklog/lserve-design-v1.0.0.md",
    ])
for rel in required:
    if not (ROOT / rel).is_file():
        fail(f"missing required file: {rel}")

config_source = (ROOT / "src/config.rs").read_text(encoding="utf-8")
m = re.search(r'pub const EXAMPLE_CONFIG: &str = r#"(.*?)"#;', config_source, re.S)
if not m:
    fail("could not extract EXAMPLE_CONFIG from src/config.rs")
elif (ROOT / ".lserve.example").read_text(encoding="utf-8") != m.group(1):
    fail(".lserve.example is not synchronized with config::EXAMPLE_CONFIG")

for readme in (ROOT / "README.md", ROOT / "README_ko.md"):
    text = readme.read_text(encoding="utf-8")
    for target in re.findall(r'\]\(([^)]+\.md)\)', text):
        path = (readme.parent / target).resolve()
        if not path.is_file():
            fail(f"{readme.name}: broken Markdown link: {target}")


def rust_delimiters_balanced(text: str) -> tuple[bool, str]:
    """Conservative delimiter scan ignoring Rust comments and string literals."""
    stack: list[tuple[str, int]] = []
    pairs = {')': '(', ']': '[', '}': '{'}
    opens = set(pairs.values())
    i = 0
    n = len(text)
    block_depth = 0
    while i < n:
        if block_depth:
            if text.startswith('/*', i):
                block_depth += 1; i += 2; continue
            if text.startswith('*/', i):
                block_depth -= 1; i += 2; continue
            i += 1; continue
        if text.startswith('//', i):
            j = text.find('\n', i + 2)
            i = n if j < 0 else j + 1
            continue
        if text.startswith('/*', i):
            block_depth = 1; i += 2; continue

        # raw string / raw byte string: r###"..."### or br###"..."###
        raw_start = i
        if text.startswith('br', i): raw_start = i + 1
        if raw_start < n and text[raw_start] == 'r':
            j = raw_start + 1
            while j < n and text[j] == '#': j += 1
            if j < n and text[j] == '"':
                hashes = text[raw_start + 1:j]
                end = '"' + hashes
                k = text.find(end, j + 1)
                if k < 0: return False, f'unterminated raw string at byte {i}'
                i = k + len(end); continue

        # normal string / byte string
        quote_at = i + 1 if text.startswith('b"', i) else i
        if quote_at < n and text[quote_at] == '"':
            j = quote_at + 1
            while j < n:
                if text[j] == '\\': j += 2; continue
                if text[j] == '"': break
                j += 1
            if j >= n: return False, f'unterminated string at byte {i}'
            i = j + 1; continue

        # char/byte-char literal. Lifetimes such as 'a are intentionally not treated as chars.
        char_at = i + 1 if text.startswith("b'", i) else i
        if char_at < n and text[char_at] == "'":
            j = char_at + 1
            if j < n and text[j] == '\\': j += 2
            else: j += 1
            if j < n and text[j] == "'":
                i = j + 1; continue

        ch = text[i]
        if ch in opens:
            stack.append((ch, i))
        elif ch in pairs:
            if not stack or stack[-1][0] != pairs[ch]:
                return False, f'unmatched {ch!r} at byte {i}'
            stack.pop()
        i += 1
    if block_depth:
        return False, 'unterminated block comment'
    if stack:
        ch, pos = stack[-1]
        return False, f'unclosed {ch!r} from byte {pos}'
    return True, ''


for path in ROOT.rglob("*"):
    if ".worklog" in path.parts or ".git" in path.parts or "target" in path.parts:
        continue
    if not path.is_file():
        continue
    rel = path.relative_to(ROOT)
    lower = path.name.lower()
    if lower.endswith((".ttf", ".otf", ".woff", ".woff2")):
        fail(f"font binary unexpectedly bundled: {rel}")
    if lower in {"rootca-key.pem", "localhost-key.pem", "server-key.pem"}:
        fail(f"private key unexpectedly bundled: {rel}")
    if path.suffix == ".rs":
        text = path.read_text(encoding="utf-8")
        ok, detail = rust_delimiters_balanced(text)
        if not ok:
            fail(f"Rust delimiter scan failed in {rel}: {detail}")
        if any(marker in text for marker in ("<<<<<<<", "=======", ">>>>>>>")):
            fail(f"merge-conflict marker in {rel}")
        if re.search(r'\b(todo!|unimplemented!)\s*\(', text):
            fail(f"unfinished Rust macro in {rel}")
        if "FIXME" in text or "TODO" in text:
            fail(f"TODO/FIXME remains in {rel}")

# Targeted regressions found during the v1.0.0/v1.0.1 reconstruction.
if config_source.count("let file_cfg = read_project_config(&root)?;") != 1:
    fail("document-root config should be read exactly once in resolve_serve")
if "pub const DEFAULT_HTTP_PORT: u16 = 8080;" not in config_source or "pub const DEFAULT_HTTPS_PORT: u16 = 8443;" not in config_source:
    fail("unprivileged default ports 8080/8443 constants are missing")
if "pub const PRIVILEGED_HTTP_PORT: u16 = 80;" not in config_source or "pub const PRIVILEGED_HTTPS_PORT: u16 = 443;" not in config_source:
    fail("privileged default ports 80/443 constants are missing")
if 'cmd == "export"' not in config_source or '--export PATH' in config_source:
    fail("canonical CA export syntax must remain `ca export PATH`")
if 'what == "all"' not in config_source:
    fail("explicit `flush all` support is missing")
if 'HOST[/PATH]=URL' not in config_source:
    fail("Host/path route CLI contract is missing")
proxy = (ROOT / "src/proxy.rs").read_text(encoding="utf-8")
if "HeaderValue::from_static(scheme)" in proxy:
    fail("dynamic X-Forwarded-Proto incorrectly uses HeaderValue::from_static")
if "path_prefix_matches" not in proxy or "max_by_key" not in proxy:
    fail("Host/path longest-prefix proxy routing is missing")
certs = (ROOT / "src/certs.rs").read_text(encoding="utf-8")
if 'const ANDROID_ROOT_FALLBACK: &str = "/data/local/tmp/lserve";' not in certs:
    fail("rooted-Android certificate fallback path is missing")
if 'home.join(".local/share/lserve")' not in certs or ".local/lserve" in certs:
    fail("certificate store path must be unified to .local/share/lserve")
completions = (ROOT / "src/completions.rs").read_text(encoding="utf-8")
bash_match = re.search(r'const BASH: &str = r#"(.*?)"#;', completions, re.S)
bash_completion = bash_match.group(1) if bash_match else ""
if not bash_match:
    fail("could not extract Bash completion script")
elif "HOST=http://" not in bash_completion or "all ca cert" not in bash_completion or "generate export" not in bash_completion:
    fail("Bash completion does not reflect the current CLI grammar")
if bash_match and "--export" in bash_completion:
    fail("legacy --export completion reappeared")
markdown = (ROOT / "src/markdown.rs").read_text(encoding="utf-8")
if "options::{Options, Plugins}" not in markdown:
    fail("Comrak Plugins import is not using comrak::options")

if ERRORS:
    print("static audit: FAILED", file=sys.stderr)
    for error in ERRORS:
        print(f"- {error}", file=sys.stderr)
    sys.exit(1)

print("static audit: OK")
print(f"checked package {package['name']} {package['version']} (Rust {package['rust-version']}+)" )
