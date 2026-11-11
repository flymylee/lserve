mod bind;
mod certs;
mod completions;
mod config;
mod logging;
mod markdown;
mod proxy;
mod response;
mod server;
mod static_files;

use anyhow::{Context, Result};
use config::{CaCommand, CertCommand, Command, ConfigCommand, FlushCommand};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("lserve: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    match config::parse_command()? {
        Command::Serve(config) => server::run(config).await,
        Command::Ca(command) => run_ca(command),
        Command::Cert(command) => run_cert(command),
        Command::Flush(command) => run_flush(command),
        Command::Config(command) => run_config(command),
        Command::Completions(shell) => write_stdout(completions::script(shell).as_bytes()),
        Command::Help => write_stdout(config::HELP.as_bytes()),
        Command::Version => {
            write_stdout(format!("lserve {}\n", env!("CARGO_PKG_VERSION")).as_bytes())
        }
    }
}

fn run_ca(command: CaCommand) -> Result<()> {
    match command {
        CaCommand::Show => certs::show_ca(),
        CaCommand::Generate { force } => certs::generate_ca(force),
        CaCommand::Export { path } => certs::export_ca(&path),
    }
}

fn run_cert(command: CertCommand) -> Result<()> {
    match command {
        CertCommand::Generate { force } => {
            let config = config::current_project_config()?;
            certs::generate_for_project(&config, force)?;
            Ok(())
        }
    }
}

fn run_flush(command: FlushCommand) -> Result<()> {
    match command {
        FlushCommand::All | FlushCommand::Ca => certs::flush_ca(),
        FlushCommand::Cert => certs::flush_cert(),
    }
}

fn run_config(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Print => write_stdout(config::EXAMPLE_CONFIG.as_bytes()),
        ConfigCommand::Write { path } => {
            let path = path.unwrap_or_else(|| PathBuf::from(".lserve"));
            write_new_file(&path, config::EXAMPLE_CONFIG.as_bytes())?;
            println!("wrote {}", path.display());
            Ok(())
        }
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("refusing to overwrite existing file {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.flush()?;
    Ok(())
}

fn write_stdout(bytes: &[u8]) -> Result<()> {
    // Arbitrary shell/config text can contain `{}` and `${...}`. Avoid Rust
    // formatting macros here so output is copied byte-for-byte.
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}
