use crate::bind::BindSpec;
use anyhow::{Context, Result, anyhow, bail};
use std::{
    collections::BTreeMap,
    env, fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
};

pub const DEFAULT_HTTP_PORT: u16 = 8080;
pub const DEFAULT_HTTPS_PORT: u16 = 8443;
pub const PRIVILEGED_HTTP_PORT: u16 = 80;
pub const PRIVILEGED_HTTPS_PORT: u16 = 443;
pub const DEFAULT_LOG_MAX_SIZE: u64 = 1024 * 1024;
pub const DEFAULT_LOG_GENERATIONS: usize = 5;

pub fn default_ports() -> (u16, u16) {
    if crate::certs::can_use_privileged_ports() {
        (PRIVILEGED_HTTP_PORT, PRIVILEGED_HTTPS_PORT)
    } else {
        (DEFAULT_HTTP_PORT, DEFAULT_HTTPS_PORT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownTheme {
    Auto,
    Light,
    Dark,
}

impl MarkdownTheme {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => bail!("invalid markdown theme {value:?}; expected auto, light, or dark"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub binds: Vec<IpAddr>,
    pub http_port: u16,
    pub https_port: u16,
    pub http_only: bool,
    pub follow_symlinks: bool,
    pub markdown: bool,
    pub markdown_theme: MarkdownTheme,
    pub access_log: Option<PathBuf>,
    pub error_log: Option<PathBuf>,
    pub log_max_size: u64,
    pub log_generations: usize,
    pub verbose_errors: bool,
    pub routes: BTreeMap<String, String>,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Command {
    Serve(Config),
    Ca(CaCommand),
    Cert(CertCommand),
    Flush(FlushCommand),
    Config(ConfigCommand),
    Completions(Shell),
    Help,
    Version,
}

#[derive(Debug)]
pub enum CaCommand {
    Show,
    Generate { force: bool },
    Export { path: PathBuf },
}

#[derive(Debug)]
pub enum CertCommand {
    Generate { force: bool },
}

#[derive(Debug, Clone, Copy)]
pub enum FlushCommand {
    All,
    Ca,
    Cert,
}

#[derive(Debug)]
pub enum ConfigCommand {
    Print,
    Write { path: Option<PathBuf> },
}

#[derive(Debug, Clone, Copy)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug, Default)]
struct RawConfig {
    binds: Vec<BindSpec>,
    http_port: Option<u16>,
    https_port: Option<u16>,
    http_only: Option<bool>,
    follow_symlinks: Option<bool>,
    markdown: Option<bool>,
    markdown_theme: Option<MarkdownTheme>,
    access_log: Option<PathBuf>,
    error_log: Option<PathBuf>,
    log_max_size: Option<u64>,
    log_generations: Option<usize>,
    verbose_errors: Option<bool>,
    routes: BTreeMap<String, String>,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct ServeCli {
    root: Option<PathBuf>,
    raw: RawConfig,
}

pub fn parse_command() -> Result<Command> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Ok(Command::Serve(resolve_serve(ServeCli::default())?));
    }

    // Help/version are process-wide switches. Handling them before option parsing
    // also avoids accidentally treating a preceding option value as a document root.
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(Command::Help);
    }
    if args.iter().any(|arg| arg == "-V" || arg == "--version") {
        return Ok(Command::Version);
    }

    match args[0].as_str() {
        "-h" | "--help" | "help" => return Ok(Command::Help),
        "-V" | "--version" => return Ok(Command::Version),
        "ca" => return parse_ca(&args[1..]).map(Command::Ca),
        "cert" => return parse_cert(&args[1..]).map(Command::Cert),
        "flush" => return parse_flush(&args[1..]).map(Command::Flush),
        "config" => return parse_config_command(&args[1..]).map(Command::Config),
        "completions" => return parse_completions(&args[1..]).map(Command::Completions),
        _ => {}
    }

    let cli = parse_serve_args(&args)?;
    Ok(Command::Serve(resolve_serve(cli)?))
}

fn parse_serve_args(args: &[String]) -> Result<ServeCli> {
    let mut cli = ServeCli::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                if i >= args.len() {
                    bail!("expected document root after --");
                }
                if cli.root.is_some() || i + 1 != args.len() {
                    bail!("only one document root may be specified");
                }
                cli.root = Some(PathBuf::from(&args[i]));
            }
            "--bind" => {
                cli.raw.binds.push(
                    next_value(args, &mut i, "--bind")?
                        .parse()
                        .context("invalid --bind selector")?,
                );
            }
            "--http-port" => {
                cli.raw.http_port = Some(parse_port(next_value(args, &mut i, "--http-port")?)?)
            }
            "--https-port" | "-p" => {
                cli.raw.https_port = Some(parse_port(next_value(args, &mut i, arg)?)?)
            }
            "--http-only" => cli.raw.http_only = Some(true),
            "--no-symlinks" => cli.raw.follow_symlinks = Some(false),
            "--no-markdown" => cli.raw.markdown = Some(false),
            "--markdown-theme" => {
                cli.raw.markdown_theme = Some(MarkdownTheme::parse(next_value(
                    args,
                    &mut i,
                    "--markdown-theme",
                )?)?);
            }
            "--access-log" => {
                cli.raw.access_log = Some(PathBuf::from(next_value(args, &mut i, "--access-log")?))
            }
            "--error-log" => {
                cli.raw.error_log = Some(PathBuf::from(next_value(args, &mut i, "--error-log")?))
            }
            "--log-max-size" => {
                cli.raw.log_max_size =
                    Some(parse_size(next_value(args, &mut i, "--log-max-size")?)?)
            }
            "--log-generations" => {
                let n: usize = next_value(args, &mut i, "--log-generations")?
                    .parse()
                    .context("invalid --log-generations")?;
                cli.raw.log_generations = Some(n);
            }
            "--verbose-errors" => cli.raw.verbose_errors = Some(true),
            "--route" => {
                let (selector, target) = parse_route(next_value(args, &mut i, "--route")?)?;
                cli.raw.routes.insert(selector, target);
            }
            "--cert" => cli.raw.cert = Some(PathBuf::from(next_value(args, &mut i, "--cert")?)),
            "--key" => cli.raw.key = Some(PathBuf::from(next_value(args, &mut i, "--key")?)),
            _ if arg.starts_with('-') => bail!("unknown option: {arg}"),
            _ => {
                if cli.root.is_some() {
                    bail!("only one document root may be specified");
                }
                cli.root = Some(PathBuf::from(arg));
            }
        }
        i += 1;
    }
    Ok(cli)
}

fn resolve_serve(cli: ServeCli) -> Result<Config> {
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let root = fs::canonicalize(&root)
        .with_context(|| format!("failed to resolve document root {}", root.display()))?;
    if !root.is_dir() {
        bail!("document root is not a directory: {}", root.display());
    }

    let file_cfg = read_project_config(&root)?;

    let bind_specs = select_bind_specs(&cli.raw.binds, &file_cfg.binds);
    let binds = crate::bind::resolve_binds(&bind_specs)?;

    // Privileged ports (80/443) are selected automatically when the process
    // has sufficient privilege (root or CAP_NET_BIND_SERVICE on Linux, and
    // both root + CAP_NET_BIND_SERVICE on Android). Otherwise 8080/8443 are used.
    let (default_http, default_https) = default_ports();
    let http_port = cli
        .raw
        .http_port
        .or(file_cfg.http_port)
        .unwrap_or(default_http);
    let https_port = cli
        .raw
        .https_port
        .or(file_cfg.https_port)
        .unwrap_or(default_https);
    let http_only = cli.raw.http_only.or(file_cfg.http_only).unwrap_or(false);
    if !http_only && http_port == https_port {
        bail!("HTTP and HTTPS ports must differ unless --http-only is used");
    }
    let follow_symlinks = cli
        .raw
        .follow_symlinks
        .or(file_cfg.follow_symlinks)
        .unwrap_or(true);
    let markdown = cli.raw.markdown.or(file_cfg.markdown).unwrap_or(true);
    let markdown_theme = cli
        .raw
        .markdown_theme
        .or(file_cfg.markdown_theme)
        .unwrap_or(MarkdownTheme::Auto);
    let verbose_errors = cli
        .raw
        .verbose_errors
        .or(file_cfg.verbose_errors)
        .unwrap_or(false);
    let log_max_size = cli
        .raw
        .log_max_size
        .or(file_cfg.log_max_size)
        .unwrap_or(DEFAULT_LOG_MAX_SIZE);
    let log_generations = cli
        .raw
        .log_generations
        .or(file_cfg.log_generations)
        .unwrap_or(DEFAULT_LOG_GENERATIONS);

    if log_max_size == 0 {
        bail!("log-max-size must be greater than zero");
    }

    let access_log = cli
        .raw
        .access_log
        .or(file_cfg.access_log)
        .map(|p| resolve_relative(&root, p));
    let error_log = cli
        .raw
        .error_log
        .or(file_cfg.error_log)
        .map(|p| resolve_relative(&root, p));
    if let (Some(access), Some(error)) = (&access_log, &error_log)
        && (access == error
            || fs::canonicalize(access)
                .ok()
                .zip(fs::canonicalize(error).ok())
                .is_some_and(|(a, b)| a == b))
    {
        bail!("access-log and error-log must use different files");
    }
    let cert = cli
        .raw
        .cert
        .or(file_cfg.cert)
        .map(|p| resolve_relative(&root, p));
    let key = cli
        .raw
        .key
        .or(file_cfg.key)
        .map(|p| resolve_relative(&root, p));
    if cert.is_some() != key.is_some() {
        bail!("--cert and --key must be specified together");
    }

    let mut routes = file_cfg.routes;
    for (host, target) in cli.raw.routes {
        routes.insert(host, target);
    }

    Ok(Config {
        root,
        binds,
        http_port,
        https_port,
        http_only,
        follow_symlinks,
        markdown,
        markdown_theme,
        access_log,
        error_log,
        log_max_size,
        log_generations,
        verbose_errors,
        routes,
        cert,
        key,
    })
}

pub fn current_project_config() -> Result<Config> {
    resolve_serve(ServeCli::default())
}

fn read_project_config(root: &Path) -> Result<RawConfig> {
    let path = root.join(".lserve");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(RawConfig::default()),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
    };

    let mut cfg = RawConfig::default();
    for (idx, original) in text.lines().enumerate() {
        let line = original.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("{}:{}: expected key=value", path.display(), idx + 1))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            bail!("{}:{}: empty configuration key", path.display(), idx + 1);
        }
        match key {
            "bind" => {
                cfg.binds.push(value.parse().with_context(|| {
                    format!("{}:{}: invalid bind selector", path.display(), idx + 1)
                })?)
            }
            "http-port" => cfg.http_port = Some(parse_port(value)?),
            "https-port" => cfg.https_port = Some(parse_port(value)?),
            "http-only" => cfg.http_only = Some(parse_bool(value)?),
            "symlinks" => cfg.follow_symlinks = Some(parse_bool(value)?),
            "markdown" => cfg.markdown = Some(parse_bool(value)?),
            "markdown-theme" => cfg.markdown_theme = Some(MarkdownTheme::parse(value)?),
            "access-log" => cfg.access_log = Some(PathBuf::from(value)),
            "error-log" => cfg.error_log = Some(PathBuf::from(value)),
            "log-max-size" => cfg.log_max_size = Some(parse_size(value)?),
            "log-generations" => {
                cfg.log_generations = Some(value.parse().with_context(|| {
                    format!("{}:{}: invalid log-generations", path.display(), idx + 1)
                })?)
            }
            "verbose-errors" => cfg.verbose_errors = Some(parse_bool(value)?),
            _ if key.starts_with("route.") => {
                let selector = normalize_route_selector(&key["route.".len()..])?;
                if value.is_empty() {
                    bail!("{}:{}: empty route target", path.display(), idx + 1);
                }
                cfg.routes.insert(selector, value.to_string());
            }
            _ => bail!(
                "{}:{}: unknown configuration key {key:?}",
                path.display(),
                idx + 1
            ),
        }
    }
    Ok(cfg)
}

fn resolve_relative(root: &Path, path: PathBuf) -> PathBuf {
    lexical_normalize(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn lexical_normalize(path: PathBuf) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn parse_port(value: &str) -> Result<u16> {
    let port: u16 = value
        .parse()
        .with_context(|| format!("invalid port {value:?}"))?;
    if port == 0 {
        bail!("port must be between 1 and 65535");
    }
    Ok(port)
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => bail!("invalid boolean {value:?}; expected true or false"),
    }
}

pub fn parse_size(value: &str) -> Result<u64> {
    let s = value.trim();
    if s.is_empty() {
        bail!("size must not be empty");
    }
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let number: u64 = s[..split].parse().context("invalid size")?;
    let suffix = s[split..].trim().to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => bail!("invalid size suffix {suffix:?}; use B, K/KiB, M/MiB, or G/GiB"),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("size is too large"))
}

fn parse_route(value: &str) -> Result<(String, String)> {
    let (selector, target) = value
        .split_once('=')
        .ok_or_else(|| anyhow!("route must be HOST[/PATH]=URL"))?;
    let target = target.trim();
    if target.is_empty() {
        bail!("route target must not be empty");
    }
    Ok((normalize_route_selector(selector)?, target.to_string()))
}

pub fn route_selector_host(selector: &str) -> &str {
    selector.split_once('/').map_or(selector, |(host, _)| host)
}

pub fn route_selector_path(selector: &str) -> &str {
    selector.find('/').map_or("/", |index| &selector[index..])
}

fn normalize_route_selector(selector: &str) -> Result<String> {
    let selector = selector.trim();
    if selector.is_empty() || selector.chars().any(char::is_whitespace) {
        bail!("invalid route selector {selector:?}");
    }

    let (host, raw_path) = match selector.split_once('/') {
        Some((host, path)) => (host, Some(path)),
        None => (selector, None),
    };
    let host = normalize_route_host(host)?;
    let Some(path) = raw_path else {
        return Ok(host);
    };

    if path.contains('?') || path.contains('#') {
        bail!("route path must not contain query or fragment: {selector:?}");
    }
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(host);
    }
    Ok(format!("{host}/{trimmed}"))
}

fn normalize_route_host(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.contains('/')
        || host.contains(':')
        || host.chars().any(char::is_whitespace)
    {
        bail!("invalid route host {host:?}");
    }
    Ok(host)
}

fn select_bind_specs(cli: &[BindSpec], file: &[BindSpec]) -> Vec<BindSpec> {
    if !cli.is_empty() {
        cli.to_vec()
    } else if !file.is_empty() {
        file.to_vec()
    } else {
        vec![BindSpec::Address(IpAddr::V4(Ipv4Addr::LOCALHOST))]
    }
}

pub fn validate_bind(ip: IpAddr) -> Result<()> {
    let allowed = match ip {
        IpAddr::V4(ip) => is_allowed_v4(ip),
        IpAddr::V6(ip) => is_allowed_v6(ip),
    };
    if !allowed {
        bail!("bind address {ip} is outside lserve's local/private address policy");
    }
    if ip.is_unspecified() {
        bail!("wildcard bind address {ip} is not allowed");
    }
    Ok(())
}

fn is_allowed_v4(ip: Ipv4Addr) -> bool {
    let oct = ip.octets();
    ip.is_loopback()
        || oct[0] == 10
        || (oct[0] == 172 && (16..=31).contains(&oct[1]))
        || (oct[0] == 192 && oct[1] == 168)
        || (oct[0] == 100 && (64..=127).contains(&oct[1]))
}

fn is_allowed_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn next_value<'a>(args: &'a [String], i: &mut usize, option: &str) -> Result<&'a str> {
    *i += 1;
    args.get(*i)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("{option} requires a value"))
}

fn parse_ca(args: &[String]) -> Result<CaCommand> {
    match args {
        [] => Ok(CaCommand::Show),
        [cmd, path] if cmd == "export" => Ok(CaCommand::Export {
            path: PathBuf::from(path),
        }),
        [cmd] if cmd == "generate" => Ok(CaCommand::Generate { force: false }),
        [cmd, flag] if cmd == "generate" && flag == "--force" => {
            Ok(CaCommand::Generate { force: true })
        }
        _ => bail!("usage: lserve ca [generate [--force] | export PATH]"),
    }
}

fn parse_cert(args: &[String]) -> Result<CertCommand> {
    match args {
        [cmd] if cmd == "generate" => Ok(CertCommand::Generate { force: false }),
        [cmd, flag] if cmd == "generate" && flag == "--force" => {
            Ok(CertCommand::Generate { force: true })
        }
        _ => bail!("usage: lserve cert generate [--force]"),
    }
}

fn parse_flush(args: &[String]) -> Result<FlushCommand> {
    match args {
        [] => Ok(FlushCommand::All),
        [what] if what == "all" => Ok(FlushCommand::All),
        [what] if what == "ca" => Ok(FlushCommand::Ca),
        [what] if what == "cert" => Ok(FlushCommand::Cert),
        _ => bail!("usage: lserve flush [all|ca|cert]"),
    }
}

fn parse_config_command(args: &[String]) -> Result<ConfigCommand> {
    match args {
        [] => Ok(ConfigCommand::Print),
        [flag] if flag == "--write" => Ok(ConfigCommand::Write { path: None }),
        [flag, path] if flag == "--write" => Ok(ConfigCommand::Write {
            path: Some(PathBuf::from(path)),
        }),
        _ => bail!("usage: lserve config [--write [PATH]]"),
    }
}

fn parse_completions(args: &[String]) -> Result<Shell> {
    match args {
        [shell] if shell == "bash" => Ok(Shell::Bash),
        [shell] if shell == "zsh" => Ok(Shell::Zsh),
        [shell] if shell == "fish" => Ok(Shell::Fish),
        _ => bail!("usage: lserve completions bash|zsh|fish"),
    }
}

pub const EXAMPLE_CONFIG: &str = r#"# lserve configuration
#
# CLI options always override values in this file.
# This file is read only from the selected document root.

# Bind address selectors. Repeat bind= for multiple listeners.
# Default if omitted: 127.0.0.1 only.
# CIDR selects UP local interface addresses at startup; NO wildcard socket.
# Entire CIDR must be loopback/private/shared. Public ranges are rejected.
# A CIDR matching no local address fails startup; restart on IP changes.
# Repeated CLI --bind options replace the entire .lserve bind list.
#
# bind=127.0.0.1
# bind=::1
# bind=100.64.0.0/10

# Ports.
# Default: HTTP 80 / HTTPS 443 when running with root/CAP_NET_BIND_SERVICE
# capability (on Android, both root and CAP_NET_BIND_SERVICE are required);
# otherwise HTTP 8080 / HTTPS 8443.
#
# http-port=8080
# https-port=8443

# Serve HTTP directly instead of redirecting to HTTPS.
# Useful behind Cloudflare Tunnel or another access layer.
#
# http-only=false

# Follow symbolic links.
# Default: true
#
# symlinks=true

# Render .md files as HTML. Add ?raw to retrieve original Markdown.
# Default: true
#
# markdown=true

# Markdown theme: auto, light, dark
# Default: auto
#
# markdown-theme=auto

# Optional file logs. Console logging remains enabled.
# Relative paths are resolved from the document root.
#
# access-log=./logs/access.log
# error-log=./logs/error.log

# Default: 1M
# log-max-size=1M

# Default: 5
# log-generations=5

# Add limited internal details to 5xx responses.
# Default: false
#
# verbose-errors=false

# Host/path-based reverse proxy.
# Upstream scheme must be http. The actual destination selected for a
# connection must resolve to loopback, even when a custom hostname is used.
# A path prefix is optional; the longest matching path prefix wins and is
# forwarded unchanged to the upstream.
#
# route.local.example.com=http://127.0.0.1:3000
# route.local.example.com/api=http://127.0.0.1:4000
# route.admin.local.example.com=http://localhost:3001
"#;

pub const HELP: &str = r#"lserve 1.0.2
A lightweight local/private HTTPS static server, Markdown viewer, and Host/path-based reverse proxy.

USAGE:
  lserve [DIR] [OPTIONS]
  lserve ca [generate [--force] | export PATH]
  lserve cert generate [--force]
  lserve flush [all|ca|cert]
  lserve config [--write [PATH]]
  lserve completions bash|zsh|fish

SERVE OPTIONS:
  --bind IP[/PREFIX]         Repeatable private IP or private CIDR local-address selector
  --http-port PORT           HTTP redirect/origin port (default 80 if privileged, else 8080)
  --https-port PORT          HTTPS port (default 443 if privileged, else 8443)
  -p PORT                    Alias for --https-port
  --http-only                Serve directly over HTTP
  --no-symlinks              Do not follow symlinks
  --no-markdown              Serve .md as raw files
  --markdown-theme THEME     auto, light, or dark
  --access-log PATH          Add an access log file
  --error-log PATH           Add an internal-error log file
  --log-max-size SIZE        Rotation threshold (default 1M)
  --log-generations N        Rotated files retained (default 5)
  --verbose-errors           Include limited internal detail in 5xx responses
  --route HOST[/PATH]=URL    Reverse proxy route; longest path prefix wins
  --cert PATH --key PATH     Use an explicit PEM certificate/key pair
  -h, --help                 Show this help
  -V, --version              Show version
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_bind_policy() {
        assert!(validate_bind("127.0.0.1".parse().unwrap()).is_ok());
        assert!(validate_bind("10.1.2.3".parse().unwrap()).is_ok());
        assert!(validate_bind("100.100.1.2".parse().unwrap()).is_ok());
        assert!(validate_bind("fd7a:115c:a1e0::1".parse().unwrap()).is_ok());
        assert!(validate_bind("0.0.0.0".parse().unwrap()).is_err());
        assert!(validate_bind("8.8.8.8".parse().unwrap()).is_err());
        assert!(validate_bind("::".parse().unwrap()).is_err());
    }

    #[test]
    fn cli_bind_list_overrides_all_file_binds() {
        let args = vec![
            "--bind".to_string(), "::1".to_string(),
            "--bind".to_string(), "100.64.0.0/10".to_string(),
        ];
        let cli = parse_serve_args(&args).unwrap();
        assert_eq!(cli.raw.binds.len(), 2);
        let file = vec!["127.0.0.1".parse().unwrap()];
        assert_eq!(select_bind_specs(&cli.raw.binds, &file), cli.raw.binds);
        assert_eq!(select_bind_specs(&[], &file), file);
        assert_eq!(
            select_bind_specs(&[], &[]),
            vec![BindSpec::Address(IpAddr::V4(Ipv4Addr::LOCALHOST))]
        );
    }

    #[test]
    fn sizes_are_binary() {
        assert_eq!(parse_size("1M").unwrap(), 1_048_576);
        assert_eq!(parse_size("4KiB").unwrap(), 4096);
    }

    #[test]
    fn ca_export_and_flush_all_are_canonical() {
        assert!(matches!(
            parse_ca(&["export".to_string(), "out.crt".to_string()]).unwrap(),
            CaCommand::Export { .. }
        ));
        assert!(matches!(
            parse_flush(&["all".to_string()]).unwrap(),
            FlushCommand::All
        ));
        assert!(parse_ca(&["--export".to_string(), "out.crt".to_string()]).is_err());
    }

    #[test]
    fn route_selectors_support_optional_paths() {
        assert_eq!(
            normalize_route_selector("LOCAL.EXAMPLE.").unwrap(),
            "local.example"
        );
        assert_eq!(
            normalize_route_selector("local.example/api/").unwrap(),
            "local.example/api"
        );
        assert_eq!(route_selector_host("local.example/api"), "local.example");
        assert_eq!(route_selector_path("local.example"), "/");
        assert_eq!(route_selector_path("local.example/api"), "/api");
        assert!(normalize_route_selector("local.example/api?x=1").is_err());
    }
}
