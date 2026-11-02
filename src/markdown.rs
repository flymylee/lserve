use crate::{certs, config::MarkdownTheme};
use anyhow::{Context, Result};
use comrak::{
    adapters::SyntaxHighlighterAdapter,
    markdown_to_html_with_plugins,
    options::{Options, Plugins},
};
use std::{borrow::Cow, collections::HashMap, fmt, path::Path};
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    html::{IncludeBackground, append_highlighted_html_for_styled_line},
    parsing::{SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};

pub struct MarkdownRenderer {
    highlighter: DualThemeHighlighter,
}

impl MarkdownRenderer {
    pub fn new(project_root: &Path) -> Result<Self> {
        Ok(Self {
            highlighter: DualThemeHighlighter::new(project_root)?,
        })
    }

    pub fn render(&self, source: &str, title: &str, theme: MarkdownTheme) -> String {
        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.autolink = true;
        options.extension.tasklist = true;
        // An empty prefix enables stable GitHub-like heading slugs without adding
        // an lserve-specific namespace to URLs such as README.md#installation.
        options.extension.header_id_prefix = Some(String::new());
        options.extension.header_id_prefix_in_href = true;
        // Keep Comrak's safe-by-default renderer. In particular, do not enable
        // render.unsafe: Markdown often comes from AI/downloaded sources and raw
        // HTML should not silently become executable local browser content.
        options.render.r#unsafe = false;

        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&self.highlighter);
        let body = markdown_to_html_with_plugins(source, &options, &plugins);

        format!(
            "<!doctype html>\n<html lang=\"en\" data-theme=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>{}</title>\n<style>{}</style>\n</head>\n<body><main class=\"markdown-body\">{}</main></body>\n</html>\n",
            theme.as_str(),
            escape_html(title),
            MARKDOWN_CSS,
            body
        )
    }
}

struct DualThemeHighlighter {
    syntax_set: SyntaxSet,
    light: Theme,
    dark: Theme,
}

impl DualThemeHighlighter {
    fn new(project_root: &Path) -> Result<Self> {
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();

        if let Ok(global) = certs::data_dir().map(|p| p.join("syntaxes"))
            && global.is_dir()
        {
            builder.add_from_folder(&global, true).with_context(|| {
                format!(
                    "failed to load syntax definitions from {}",
                    global.display()
                )
            })?;
        }

        let project = project_root.join(".syntaxes");
        if project.is_dir() {
            builder.add_from_folder(&project, true).with_context(|| {
                format!(
                    "failed to load syntax definitions from {}",
                    project.display()
                )
            })?;
        }

        let syntax_set = builder.build();
        let themes = ThemeSet::load_defaults();
        let light = themes
            .themes
            .get("InspiredGitHub")
            .cloned()
            .or_else(|| themes.themes.get("Solarized (light)").cloned())
            .context("syntect contains no usable light theme")?;
        let dark = themes
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .or_else(|| themes.themes.get("Solarized (dark)").cloned())
            .context("syntect contains no usable dark theme")?;

        Ok(Self {
            syntax_set,
            light,
            dark,
        })
    }

    fn syntax_for<'a>(&'a self, lang: Option<&str>, code: &str) -> &'a SyntaxReference {
        let token = lang
            .and_then(|l| l.split_once(',').map(|(left, _)| left).or(Some(l)))
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if let Some(token) = token {
            // Custom definitions are appended built-in -> global -> project. Search
            // backwards so a project grammar can deliberately override a global or
            // built-in token without mutating syntect internals.
            if let Some(syntax) = self.syntax_set.syntaxes().iter().rev().find(|syntax| {
                syntax.name.eq_ignore_ascii_case(token)
                    || syntax
                        .file_extensions
                        .iter()
                        .any(|ext| ext.eq_ignore_ascii_case(token))
            }) {
                return syntax;
            }
        }

        self.syntax_set
            .find_syntax_by_first_line(code)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }

    fn highlight(&self, code: &str, syntax: &SyntaxReference, theme: &Theme) -> Result<String> {
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut out = String::new();
        for line in LinesWithEndings::from(code) {
            let regions = highlighter.highlight_line(line, &self.syntax_set)?;
            append_highlighted_html_for_styled_line(&regions, IncludeBackground::No, &mut out)?;
        }
        Ok(out)
    }
}

impl SyntaxHighlighterAdapter for DualThemeHighlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        let syntax = self.syntax_for(lang, code);
        let light = self
            .highlight(code, syntax, &self.light)
            .unwrap_or_else(|_| escape_html(code));
        let dark = self
            .highlight(code, syntax, &self.dark)
            .unwrap_or_else(|_| escape_html(code));
        write!(
            output,
            "<span class=\"lserve-code-light\">{light}</span><span class=\"lserve-code-dark\">{dark}</span>"
        )
    }

    fn write_pre_tag<'s>(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'s, str>>,
    ) -> fmt::Result {
        write_open_tag(output, "pre", attributes, Some("lserve-code"))
    }

    fn write_code_tag<'s>(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'s, str>>,
    ) -> fmt::Result {
        write_open_tag(output, "code", attributes, None)
    }
}

fn write_open_tag(
    output: &mut dyn fmt::Write,
    tag: &str,
    mut attributes: HashMap<&'static str, Cow<'_, str>>,
    extra_class: Option<&str>,
) -> fmt::Result {
    output.write_char('<')?;
    output.write_str(tag)?;

    if let Some(extra) = extra_class {
        let merged = match attributes.remove("class") {
            Some(existing) if !existing.is_empty() => format!("{existing} {extra}"),
            _ => extra.to_string(),
        };
        write!(output, " class=\"{}\"", escape_attr(&merged))?;
    }

    for (key, value) in attributes {
        write!(output, " {key}=\"{}\"", escape_attr(&value))?;
    }
    output.write_char('>')
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_attr(input: &str) -> String {
    let mut out = escape_html(input);
    out = out.replace('"', "&quot;").replace('\'', "&#39;");
    out
}

const MARKDOWN_CSS: &str = r#"
:root {
  color-scheme: light dark;
  --bg: #ffffff; --fg: #1f2328; --muted: #59636e; --border: #d1d9e0;
  --link: #0969da; --quote: #59636e; --code-bg: #f6f8fa; --table-alt: #f6f8fa;
}
html[data-theme="dark"] {
  color-scheme: dark;
  --bg: #0d1117; --fg: #f0f6fc; --muted: #9198a1; --border: #3d444d;
  --link: #4493f8; --quote: #9198a1; --code-bg: #151b23; --table-alt: #151b23;
}
html[data-theme="light"] { color-scheme: light; }
@media (prefers-color-scheme: dark) {
  html[data-theme="auto"] {
    --bg: #0d1117; --fg: #f0f6fc; --muted: #9198a1; --border: #3d444d;
    --link: #4493f8; --quote: #9198a1; --code-bg: #151b23; --table-alt: #151b23;
  }
  html[data-theme="auto"] .lserve-code-light { display: none; }
  html[data-theme="auto"] .lserve-code-dark { display: inline; }
}
@media (prefers-color-scheme: light) {
  html[data-theme="auto"] .lserve-code-light { display: inline; }
  html[data-theme="auto"] .lserve-code-dark { display: none; }
}
html[data-theme="light"] .lserve-code-light { display: inline; }
html[data-theme="light"] .lserve-code-dark { display: none; }
html[data-theme="dark"] .lserve-code-light { display: none; }
html[data-theme="dark"] .lserve-code-dark { display: inline; }
* { box-sizing: border-box; }
body {
  margin: 0; background: var(--bg); color: var(--fg);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Noto Sans KR", "Apple SD Gothic Neo", Arial, sans-serif;
  line-height: 1.6;
}
.markdown-body { max-width: 980px; margin: 0 auto; padding: 40px 32px 80px; overflow-wrap: break-word; }
.markdown-body h1, .markdown-body h2 { border-bottom: 1px solid var(--border); padding-bottom: .3em; }
.markdown-body h1 { font-size: 2em; } .markdown-body h2 { font-size: 1.5em; }
.markdown-body h3 { font-size: 1.25em; } .markdown-body h4 { font-size: 1em; }
.markdown-body a { color: var(--link); text-decoration: none; } .markdown-body a:hover { text-decoration: underline; }
.markdown-body .anchor { margin-left: .35em; opacity: 0; } .markdown-body h1:hover .anchor, .markdown-body h2:hover .anchor, .markdown-body h3:hover .anchor { opacity: 1; }
.markdown-body blockquote { margin: 0; padding: 0 1em; color: var(--quote); border-left: .25em solid var(--border); }
.markdown-body hr { border: 0; border-top: .25em solid var(--border); }
.markdown-body img { max-width: 100%; height: auto; }
.markdown-body table { border-spacing: 0; border-collapse: collapse; display: block; overflow: auto; width: max-content; max-width: 100%; }
.markdown-body tr:nth-child(2n) { background: var(--table-alt); }
.markdown-body th, .markdown-body td { border: 1px solid var(--border); padding: 6px 13px; }
.markdown-body code, .markdown-body pre {
  font-family: "D2Coding", "Noto Sans Mono", "DejaVu Sans Mono", monospace;
}
.markdown-body :not(pre) > code { background: var(--code-bg); border-radius: 6px; padding: .15em .35em; font-size: 85%; }
.markdown-body pre { background: var(--code-bg); border-radius: 6px; padding: 16px; overflow: auto; line-height: 1.45; }
.markdown-body pre code { background: transparent; padding: 0; font-size: 85%; white-space: pre; }
.markdown-body input[type="checkbox"] { margin-right: .45em; }
@media (max-width: 700px) { .markdown-body { padding: 24px 18px 56px; } }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn raw_html_stays_inert() {
        let renderer = MarkdownRenderer::new(Path::new(".")).unwrap();
        let html = renderer.render("<script>alert(1)</script>", "x", MarkdownTheme::Light);
        assert!(!html.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn heading_ids_are_enabled() {
        let renderer = MarkdownRenderer::new(Path::new(".")).unwrap();
        let html = renderer.render("# Hello World", "x", MarkdownTheme::Light);
        assert!(html.contains("id=\"hello-world\""));
    }
}
