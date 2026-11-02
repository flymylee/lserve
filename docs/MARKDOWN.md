# Markdown viewer

Markdown rendering is enabled by default.

```text
GET /README.md      -> rendered HTML
GET /README.md?raw  -> original Markdown
```

Disable rendering with:

```sh
lserve --no-markdown
```

or:

```ini
markdown=false
```

## Rendering policy

The renderer uses Comrak with common GFM-style extensions including tables, task lists, strikethrough, autolinks, and fenced code blocks.

Raw HTML is **not enabled**. This is deliberate because local Markdown may come from AI-generated, downloaded, or otherwise untrusted sources.

Heading IDs are generated so fragment links such as:

```text
/README.md#installation
```

work without an `lserve`-specific prefix.

## Theme

```ini
markdown-theme=auto
```

Available values:

```text
auto
light
dark
```

`auto` follows the browser's `prefers-color-scheme` setting.

## Syntax highlighting

Fenced code blocks are highlighted server-side with Syntect. The generated page contains light and dark highlighted spans; CSS selects the appropriate one for the active theme.

The built-in syntax set is supplemented, when present, by:

```text
~/.local/share/lserve/syntaxes/
./.syntaxes/
```

Project syntax definitions are loaded after global definitions. v1.0.0 targets `.sublime-syntax` files. Unknown languages fall back to plain text rather than failing the Markdown render.

## Fonts

Body text uses a system UI sans-serif stack. Code prefers:

```text
D2Coding -> Noto Sans Mono -> DejaVu Sans Mono -> monospace
```

The v1.0.0 source package does **not** redistribute a D2Coding font binary; it uses the font only when it is installed on the client system.

## CSP

Rendered Markdown responses disable scripts through a restrictive Content-Security-Policy and do not require browser-side JavaScript for rendering or highlighting.
