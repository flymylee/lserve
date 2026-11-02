# Certificates

`lserve` uses a device/user-local Root CA and short-lived server leaf certificates for local/private HTTPS.

## Global state

On Linux and non-root Android/Termux, the existing location remains:

```text
~/.local/share/lserve/
├── rootCA.pem
├── rootCA-key.pem
├── localhost.pem
└── localhost-key.pem
```

On rooted Android (detected at runtime with EUID 0), `lserve` does **not** trust the root shell's `$HOME`. It first checks the Termux home from `$TERMUX__HOME`, then `/data/data/com.termux/files/home`, looking for a complete existing CA in:

```text
.local/share/lserve/
```

If no complete Termux CA pair exists, global certificate state falls back to:

```text
/data/local/tmp/lserve/
```

The fallback directory is owned by `0:0` and protected with mode `0700`; private keys are mode `0600`. When rooted Android writes certificate files under the Termux home, ownership is set back to the actual UID/GID of the Termux home directory rather than hard-coding Android shell UID 2000. On standard Linux, running under `sudo` preserves the invoking user's `~/.local/share/lserve/` directory and file ownership (`SUDO_USER`/`SUDO_UID`/`SUDO_GID`).

`rootCA-key.pem` is private. Never distribute it.

## Global leaf SANs

The default global server certificate contains one leaf certificate covering:

```text
DNS: localhost
IP: 127.0.0.1
IP: ::1
```

These are SAN entries in one certificate, not separate certificates.

## Project certificate

When routes or a non-loopback private bind require project-specific names/addresses, `lserve` uses:

```text
./.certs/server.pem
./.certs/server-key.pem
```

The project leaf is signed by the same global Root CA. Its required SANs include the baseline localhost entries plus exact route hostnames and the explicit private bind IP when applicable.

Wildcard SANs are not generated automatically.

If an existing project certificate does not cover the current configuration, `lserve` stops rather than silently replacing it and instructs the operator to run:

```sh
lserve cert generate --force
```

## Selection order

```text
--cert + --key
  ↓
./.certs/server.pem + server-key.pem
  ↓
~/.local/share/lserve/localhost.pem + localhost-key.pem
```

## Commands

```sh
lserve ca
lserve ca generate
lserve ca generate --force
lserve ca export PATH

lserve cert generate
lserve cert generate --force

lserve flush all
lserve flush cert
lserve flush ca
lserve flush
```

`flush`, `flush all`, and `flush ca` remove the global CA and global leaf files because a leaf signed by a removed CA is no longer useful. Project-local `.certs` directories are not searched and deleted automatically.

Removing files from `lserve` storage does **not** remove a CA already installed in Android/OS/browser trust stores; remove that trust entry manually.

## Explicit certificates

`--cert` and `--key` accept a PEM certificate/key pair and take precedence over automatic project/global certificate selection. They are CLI-only in v1.0.0.
