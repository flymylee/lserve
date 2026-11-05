#!/usr/bin/env bash
# Install the latest lserve Linux musl release for this machine.
# The script does not pin a version; each run reads GitHub releases/latest.
set -euo pipefail

repo="flymylee/lserve"
releases_page="https://github.com/${repo}/releases/latest"
api_url="https://api.github.com/repos/${repo}/releases/latest"
user_agent="lserve-install"

die() {
  echo "lserve install: $*" >&2
  echo "Download a release manually: ${releases_page}" >&2
  exit 1
}

os=$(uname -s)
machine=$(uname -m)

case "$os" in
  Linux) ;;
  Darwin)
    echo "lserve install: macOS builds are not published yet." >&2
    exit 1
    ;;
  MINGW* | MSYS* | CYGWIN*)
    echo "lserve install: Windows builds are not published yet." >&2
    exit 1
    ;;
  *)
    echo "lserve install: unsupported operating system: ${os}" >&2
    exit 1
    ;;
esac

case "$machine" in
  x86_64 | amd64) target="x86_64" ;;
  aarch64 | arm64) target="aarch64" ;;
  *)
    echo "lserve install: unsupported architecture: ${machine}" >&2
    echo "Published builds are Linux x86_64 and aarch64 only." >&2
    exit 1
    ;;
esac

if [[ -n "${LSERVE_BIN_DIR:-}" ]]; then
  bindir=$LSERVE_BIN_DIR
elif [[ -n "${TERMUX_VERSION:-}" && -n "${PREFIX:-}" ]]; then
  bindir="${PREFIX}/bin"
else
  bindir="${HOME}/.local/bin"
fi

json=$(curl -fsSL -A "$user_agent" "$api_url") || die "could not read the latest GitHub release"

asset_url=""
sums_url=""
while IFS= read -r candidate; do
  name=${candidate##*/}
  case "$name" in
    lserve-v[0-9]*-"${target}"-linux-musl.tar.gz)
      if [[ -z "$asset_url" ]]; then
        asset_url=$candidate
      fi
      ;;
    SHA256SUMS) sums_url=$candidate ;;
  esac
done < <(printf '%s\n' "$json" | grep -oE 'https://[^"]+')

if [[ -z "$asset_url" ]]; then
  die "latest release has no Linux ${target} musl archive"
fi

archive_name=${asset_url##*/}
workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

curl -fsSL -A "$user_agent" -o "${workdir}/${archive_name}" "$asset_url" \
  || die "could not download ${archive_name}"

if [[ -n "$sums_url" ]]; then
  curl -fsSL -A "$user_agent" -o "${workdir}/SHA256SUMS" "$sums_url" \
    || die "could not download SHA256SUMS"
  checksum=$(awk -v name="$archive_name" '$2 == name { print $1; exit }' "${workdir}/SHA256SUMS")
  if [[ -z "$checksum" ]]; then
    die "SHA256SUMS has no entry for ${archive_name}"
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$workdir" && printf '%s  %s\n' "$checksum" "$archive_name" | sha256sum -c -) \
      || die "checksum mismatch for ${archive_name}"
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$workdir" && printf '%s  %s\n' "$checksum" "$archive_name" | shasum -a 256 -c -) \
      || die "checksum mismatch for ${archive_name}"
  else
    die "SHA256SUMS was published but neither sha256sum nor shasum is available"
  fi
fi

mkdir -p "${workdir}/extract"
tar -xzf "${workdir}/${archive_name}" -C "${workdir}/extract"
binary=""
while IFS= read -r found; do
  binary=$found
  break
done < <(find "${workdir}/extract" -type f -name lserve -print)
if [[ -z "$binary" ]]; then
  die "archive does not contain an lserve binary"
fi

mkdir -p "$bindir"
if command -v install >/dev/null 2>&1; then
  install -m 755 "$binary" "${bindir}/lserve"
else
  cp "$binary" "${bindir}/lserve"
  chmod 755 "${bindir}/lserve"
fi

echo "Installed ${bindir}/lserve"
case ":${PATH}:" in
  *":${bindir}:"*) ;;
  *) echo "Add ${bindir} to PATH before running lserve." ;;
esac
