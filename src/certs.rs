use crate::config::{Config, route_selector_host};
use anyhow::{Context, Result, bail};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{
    collections::BTreeSet,
    env, fs,
    io::{BufReader, Cursor},
    net::IpAddr,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{
    ffi::CString,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
};
use time::{Duration, OffsetDateTime};
use x509_parser::extensions::GeneralName;

const ROOT_CA_CERT: &str = "rootCA.pem";
const ROOT_CA_KEY: &str = "rootCA-key.pem";
const GLOBAL_CERT: &str = "localhost.pem";
const GLOBAL_KEY: &str = "localhost-key.pem";
const PROJECT_CERT: &str = "server.pem";
const PROJECT_KEY: &str = "server-key.pem";

#[derive(Debug, Clone)]
pub struct CertificatePaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

const ANDROID_TERMUX_HOME: &str = "/data/data/com.termux/files/home";
const ANDROID_ROOT_FALLBACK: &str = "/data/local/tmp/lserve";

/// Runtime OS detection so a single static `*-linux-musl` binary can move
/// between Android (Termux / rooted shell) and standard Linux servers (e.g.
/// Ubuntu, RHEL, Oracle A1 aarch64) while still applying platform-appropriate
/// paths and privilege rules.
pub fn is_android_runtime() -> bool {
    cfg!(target_os = "android")
        || env::var_os("ANDROID_ROOT").is_some()
        || env::var_os("TERMUX_VERSION").is_some()
        || env::var_os("TERMUX__HOME").is_some()
        || Path::new("/system/build.prop").exists()
        || Path::new(ANDROID_TERMUX_HOME).exists()
}

pub fn is_effective_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub fn has_cap_net_bind_service() -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        if let Ok(status) = fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(hex) = line.strip_prefix("CapEff:")
                    && let Ok(mask) = u64::from_str_radix(hex.trim(), 16)
                {
                    // CAP_NET_BIND_SERVICE is bit 10 in Linux/Android capabilities.
                    return (mask & (1u64 << 10)) != 0;
                }
            }
        }
        false
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        is_effective_root()
    }
}

/// Determines whether the current process should default to privileged ports
/// (80/443) or unprivileged ports (8080/8443).
///
/// - On Android, restricted root profiles or shell contexts may have EUID 0
///   without `CAP_NET_BIND_SERVICE` (or vice versa), so both EUID 0 and
///   `CAP_NET_BIND_SERVICE` are required.
/// - On standard Linux (PC/server), either root (`EUID == 0`) or a binary with
///   `setcap 'cap_net_bind_service=+ep'` (`CAP_NET_BIND_SERVICE`) qualifies.
pub fn can_use_privileged_ports() -> bool {
    if is_android_runtime() {
        is_effective_root() && has_cap_net_bind_service()
    } else {
        is_effective_root() || has_cap_net_bind_service()
    }
}

pub fn data_dir() -> Result<PathBuf> {
    if is_android_root() {
        // Non-certificate global assets (for example custom syntax grammars)
        // stay in the Termux data tree instead of following a root-shell HOME.
        if let Some(home) = termux_home_candidates()
            .into_iter()
            .find(|path| path.is_dir())
        {
            return Ok(home.join(".local/share/lserve"));
        }
    } else if is_effective_root()
        && let Some((sudo_home, _, _)) = sudo_user_identity()
    {
        return Ok(sudo_home.join(".local/share/lserve"));
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/share/lserve"))
}

fn certificate_data_dir() -> Result<PathBuf> {
    if is_android_root() {
        android_root_data_dir()
    } else {
        data_dir()
    }
}

fn is_android_root() -> bool {
    is_android_runtime() && is_effective_root()
}

fn android_root_data_dir() -> Result<PathBuf> {
    let mut incomplete = Vec::new();
    for home in termux_home_candidates() {
        let candidate = home.join(".local/share/lserve");
        if has_complete_ca(&candidate) {
            return Ok(candidate);
        }
        if has_any_cert_material(&candidate) {
            incomplete.push(candidate);
        }
    }

    if !incomplete.is_empty() {
        eprintln!(
            "warning: ignoring incomplete Termux lserve certificate store(s): {}",
            incomplete
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(PathBuf::from(ANDROID_ROOT_FALLBACK))
}

fn termux_home_candidates() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(home) = env::var_os("TERMUX__HOME") {
        push_unique(&mut homes, PathBuf::from(home));
    }
    push_unique(&mut homes, PathBuf::from(ANDROID_TERMUX_HOME));
    homes
}

#[cfg(unix)]
fn sudo_user_identity() -> Option<(PathBuf, u32, u32)> {
    let user = env::var("SUDO_USER").ok()?;
    if user.is_empty() || user == "root" {
        return None;
    }
    let uid: u32 = env::var("SUDO_UID").ok()?.parse().ok()?;
    let gid: u32 = env::var("SUDO_GID").ok()?.parse().ok()?;
    if let Ok(passwd) = fs::read_to_string("/etc/passwd") {
        for line in passwd.lines() {
            let mut parts = line.split(':');
            let name = parts.next()?;
            let _pw = parts.next()?;
            let pw_uid = parts.next()?.parse::<u32>().ok()?;
            let _pw_gid = parts.next()?;
            let _gecos = parts.next()?;
            let home = parts.next()?;
            if (name == user || pw_uid == uid) && !home.is_empty() {
                let home_path = PathBuf::from(home);
                if home_path.is_dir() {
                    return Some((home_path, uid, gid));
                }
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn sudo_user_identity() -> Option<(PathBuf, u32, u32)> {
    None
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn has_complete_ca(base: &Path) -> bool {
    base.join(ROOT_CA_CERT).is_file() && base.join(ROOT_CA_KEY).is_file()
}

fn has_any_cert_material(base: &Path) -> bool {
    [ROOT_CA_CERT, ROOT_CA_KEY, GLOBAL_CERT, GLOBAL_KEY]
        .iter()
        .any(|name| base.join(name).exists())
}

pub fn ca_path() -> Result<PathBuf> {
    Ok(certificate_data_dir()?.join(ROOT_CA_CERT))
}

fn global_ca_paths() -> Result<CertificatePaths> {
    let base = certificate_data_dir()?;
    Ok(CertificatePaths {
        cert: base.join(ROOT_CA_CERT),
        key: base.join(ROOT_CA_KEY),
    })
}

fn global_leaf_paths() -> Result<CertificatePaths> {
    let base = certificate_data_dir()?;
    Ok(CertificatePaths {
        cert: base.join(GLOBAL_CERT),
        key: base.join(GLOBAL_KEY),
    })
}

fn project_leaf_paths(root: &Path) -> CertificatePaths {
    let base = root.join(".certs");
    CertificatePaths {
        cert: base.join(PROJECT_CERT),
        key: base.join(PROJECT_KEY),
    }
}

pub fn show_ca() -> Result<()> {
    println!("{}", ca_path()?.display());
    Ok(())
}

pub fn export_ca(destination: &Path) -> Result<()> {
    ensure_ca(false)?;
    let source = ca_path()?;
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::copy(&source, destination).with_context(|| {
        format!(
            "failed to export CA certificate from {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    apply_android_root_ownership(destination)?;
    println!("exported {}", destination.display());
    Ok(())
}

pub fn generate_ca(force: bool) -> Result<()> {
    let paths = global_ca_paths()?;
    if paths.cert.exists() || paths.key.exists() {
        if !force {
            bail!("lserve CA already exists; use `lserve ca generate --force` to replace it");
        }
        // A new CA invalidates every leaf signed by the old CA. The global leaf is
        // under our control, so remove it immediately. Project leaves are deliberately
        // not searched for or deleted outside the current project tree.
        remove_if_exists(&global_leaf_paths()?.cert)?;
        remove_if_exists(&global_leaf_paths()?.key)?;
    }

    let parent = paths.cert.parent().expect("CA path has parent");
    prepare_certificate_dir(parent)?;

    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params
        .distinguished_name
        .push(DnType::CommonName, "lserve Local Root CA");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
    params.not_after = OffsetDateTime::now_utc() + Duration::days(3650);

    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    write_public(&paths.cert, cert.pem().as_bytes())?;
    write_private(&paths.key, key.serialize_pem().as_bytes())?;

    println!("generated CA certificate: {}", paths.cert.display());
    println!("generated CA private key: {}", paths.key.display());
    if force {
        eprintln!(
            "warning: project-local certificates signed by the previous CA must be regenerated manually"
        );
    }
    Ok(())
}

pub fn generate_for_project(config: &Config, force: bool) -> Result<CertificatePaths> {
    ensure_ca(false)?;
    let use_project = needs_project_leaf(config) || config.root.join(".certs").exists();
    let paths = if use_project {
        project_leaf_paths(&config.root)
    } else {
        global_leaf_paths()?
    };
    let sans = required_sans(config, use_project);
    generate_leaf(&paths, &sans, force)?;
    Ok(paths)
}

pub fn ensure_for_serve(config: &Config) -> Result<CertificatePaths> {
    if let (Some(cert), Some(key)) = (&config.cert, &config.key) {
        ensure_pair_exists(cert, key)?;
        return Ok(CertificatePaths {
            cert: cert.clone(),
            key: key.clone(),
        });
    }

    ensure_ca(false)?;
    let project = project_leaf_paths(&config.root);
    let project_has_any = project.cert.exists() || project.key.exists();
    let use_project = project_has_any || needs_project_leaf(config);
    let paths = if use_project {
        project
    } else {
        global_leaf_paths()?
    };

    match (paths.cert.exists(), paths.key.exists()) {
        (false, false) => {
            let sans = required_sans(config, use_project);
            generate_leaf(&paths, &sans, false)?;
        }
        (true, true) => {}
        _ => bail!(
            "certificate/key pair is incomplete: {} and {} must both exist",
            paths.cert.display(),
            paths.key.display()
        ),
    }

    if use_project {
        let required = required_sans(config, true);
        let missing = missing_sans(&paths.cert, &required)?;
        if !missing.is_empty() {
            let mut message = String::from("certificate does not cover:\n");
            for name in missing {
                message.push_str("  ");
                message.push_str(&name);
                message.push('\n');
            }
            message.push_str("\nRun:\n  lserve cert generate --force");
            bail!(message);
        }
    }

    Ok(paths)
}

pub fn flush_ca() -> Result<()> {
    let ca = global_ca_paths()?;
    let leaf = global_leaf_paths()?;
    for path in [&ca.cert, &ca.key, &leaf.cert, &leaf.key] {
        remove_if_exists(path)?;
    }
    println!("removed lserve global CA and global server certificate files (if present)");
    eprintln!("note: remove any installed lserve Root CA from the OS/Android trust store manually");
    Ok(())
}

pub fn flush_cert() -> Result<()> {
    let leaf = global_leaf_paths()?;
    for path in [&leaf.cert, &leaf.key] {
        remove_if_exists(path)?;
    }
    println!("removed lserve global server certificate files (if present)");
    Ok(())
}

fn ensure_ca(force: bool) -> Result<CertificatePaths> {
    let paths = global_ca_paths()?;
    match (paths.cert.exists(), paths.key.exists()) {
        (true, true) if !force => Ok(paths),
        (false, false) => {
            generate_ca(false)?;
            global_ca_paths()
        }
        (true, true) => {
            generate_ca(true)?;
            global_ca_paths()
        }
        _ => bail!(
            "lserve CA is incomplete: {} and {} must both exist; repair it with `lserve ca generate --force`",
            paths.cert.display(),
            paths.key.display()
        ),
    }
}

fn generate_leaf(paths: &CertificatePaths, sans: &[String], force: bool) -> Result<()> {
    if (paths.cert.exists() || paths.key.exists()) && !force {
        bail!(
            "server certificate already exists at {}; use `lserve cert generate --force` to replace it",
            paths.cert.display()
        );
    }

    let parent = paths
        .cert
        .parent()
        .context("certificate path has no parent")?;
    prepare_certificate_dir(parent)?;

    let ca = global_ca_paths()?;
    let ca_cert_pem = fs::read_to_string(&ca.cert)
        .with_context(|| format!("failed to read {}", ca.cert.display()))?;
    let ca_key_pem = fs::read_to_string(&ca.key)
        .with_context(|| format!("failed to read {}", ca.key.display()))?;
    let ca_key = KeyPair::from_pem(&ca_key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key)?;

    let mut params = CertificateParams::new(sans.to_vec())?;
    params
        .distinguished_name
        .push(DnType::CommonName, "lserve local server");
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    params.use_authority_key_identifier_extension = true;
    params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
    params.not_after = OffsetDateTime::now_utc() + Duration::days(365);

    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, &issuer)?;
    write_public(&paths.cert, cert.pem().as_bytes())?;
    write_private(&paths.key, key.serialize_pem().as_bytes())?;

    println!("generated server certificate: {}", paths.cert.display());
    println!("generated server private key: {}", paths.key.display());
    println!("SANs: {}", sans.join(", "));
    Ok(())
}

fn required_sans(config: &Config, project: bool) -> Vec<String> {
    let mut sans = BTreeSet::from([
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ]);
    if project {
        sans.extend(
            config
                .routes
                .keys()
                .map(|selector| route_selector_host(selector).to_string()),
        );
        if !config.bind.is_loopback() {
            sans.insert(config.bind.to_string());
        }
    }
    sans.into_iter().collect()
}

fn needs_project_leaf(config: &Config) -> bool {
    !config.routes.is_empty() || !config.bind.is_loopback()
}

fn missing_sans(cert_path: &Path, required: &[String]) -> Result<Vec<String>> {
    let pem = fs::read(cert_path)
        .with_context(|| format!("failed to read certificate {}", cert_path.display()))?;
    let mut reader = BufReader::new(Cursor::new(pem));
    let certs: Vec<_> =
        rustls_pemfile::certs(&mut reader).collect::<std::result::Result<Vec<_>, _>>()?;
    let der = certs
        .first()
        .context("certificate PEM contains no certificate")?;
    let (_, parsed) = x509_parser::parse_x509_certificate(der.as_ref())
        .map_err(|e| anyhow::anyhow!("failed to parse certificate {}: {e}", cert_path.display()))?;

    let mut dns = BTreeSet::new();
    let mut ips = BTreeSet::new();
    if let Some(san) = parsed
        .subject_alternative_name()
        .map_err(|e| anyhow::anyhow!("failed to read certificate SAN: {e}"))?
    {
        for name in &san.value.general_names {
            match name {
                GeneralName::DNSName(name) => {
                    dns.insert(name.to_ascii_lowercase());
                }
                GeneralName::IPAddress(raw) => {
                    if raw.len() == 4 {
                        ips.insert(IpAddr::from([raw[0], raw[1], raw[2], raw[3]]));
                    } else if raw.len() == 16 {
                        let mut bytes = [0u8; 16];
                        bytes.copy_from_slice(raw);
                        ips.insert(IpAddr::from(bytes));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(required
        .iter()
        .filter(|name| match name.parse::<IpAddr>() {
            Ok(ip) => !ips.contains(&ip),
            Err(_) => !dns.contains(&name.to_ascii_lowercase()),
        })
        .cloned()
        .collect())
}

fn ensure_pair_exists(cert: &Path, key: &Path) -> Result<()> {
    if !cert.is_file() {
        bail!("certificate file does not exist: {}", cert.display());
    }
    if !key.is_file() {
        bail!("private key file does not exist: {}", key.display());
    }
    Ok(())
}

fn write_public(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    apply_android_root_ownership(path)?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to protect {}", path.display()))?;
    apply_android_root_ownership(path)?;
    Ok(())
}

fn prepare_certificate_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))?;

    if is_android_root() && path.starts_with(Path::new(ANDROID_ROOT_FALLBACK)) {
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to protect {}", path.display()))?;
    }
    apply_android_root_ownership(path)?;
    Ok(())
}

fn apply_android_root_ownership(path: &Path) -> Result<()> {
    if is_android_root() {
        if path.starts_with(Path::new(ANDROID_ROOT_FALLBACK)) {
            return chown_path(path, 0, 0);
        }

        for home in termux_home_candidates() {
            if path.starts_with(&home) {
                let metadata = fs::metadata(&home)
                    .with_context(|| format!("failed to inspect Termux home {}", home.display()))?;
                #[cfg(unix)]
                return chown_path(path, metadata.uid(), metadata.gid());
                #[cfg(not(unix))]
                return Ok(());
            }
        }
        return Ok(());
    }

    if is_effective_root()
        && let Some((sudo_home, uid, gid)) = sudo_user_identity()
        && path.starts_with(&sudo_home)
    {
        return chown_path(path, uid, gid);
    }

    Ok(())
}

#[cfg(unix)]
fn chown_path(path: &Path, uid: u32, gid: u32) -> Result<()> {
    let raw = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path contains NUL byte: {}", path.display()))?;
    let rc = unsafe { libc::chown(raw.as_ptr(), uid as libc::uid_t, gid as libc::gid_t) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to chown {} to {uid}:{gid}", path.display()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn chown_path(_path: &Path, _uid: u32, _gid: u32) -> Result<()> {
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to remove {}", path.display())),
    }
}
