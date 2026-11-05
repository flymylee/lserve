# lserve

**로컬과 사설망에서 쓰는 가벼운 HTTPS 서버입니다.** 정적 파일과 Markdown을 보여주고, 같은 머신의 HTTP 앱으로 Host/path 리버스 프록시(reverse proxy)를 합니다.

musl로 만든 정적 바이너리입니다. `x86_64`와 `aarch64`를 각각 제공하고, Android와 Termux도 그 기기에 맞는 Linux 파일을 씁니다.

하는 일을 일부러 좁혀 두었습니다. 로컬 파일을 제공하고, 로컬 TLS를 맡습니다. 리버스 프록시는 같은 머신의 HTTP 앱만 연결합니다. 인증, SSO, 사용자 관리, rate limiting, 인터넷 공개, 고가용성은 다른 쪽이 합니다.

> **기본 보안 경계:** 기본 bind는 `127.0.0.1`입니다. `--bind`를 주어도 loopback, 사설망, shared(`100.64.0.0/10`) 주소만 허용합니다. `0.0.0.0` 같은 wildcard나 공인 IP는 거부합니다.

[English README](README.md)

## 시작하기

### 1. 설치

```sh
curl -fsSL https://raw.githubusercontent.com/flymylee/lserve/main/install.sh | bash
```

이 스크립트는 이 기기에 맞는 최신 Linux musl 릴리스(`x86_64` 또는 `aarch64`)를 설치합니다. Android와 Termux도 그 기기에 맞는 같은 Linux 파일을 씁니다. `~/.local/bin`이 `PATH`에 있어야 합니다. Termux에서는 `$PREFIX/bin`에 설치합니다.

Windows와 macOS 빌드는 아직 없습니다.

압축 파일을 직접 받으려면 [최신 릴리스](https://github.com/flymylee/lserve/releases/latest)를 엽니다. 있는 파일은 `lserve-v*-x86_64-linux-musl.tar.gz`, `lserve-v*-aarch64-linux-musl.tar.gz`, `SHA256SUMS`입니다.

### 2. 현재 디렉터리 열기

```sh
cd ~/my-site
lserve
```

기본 포트는 특권 포트를 열 수 있는지에 따라 달라집니다.

```text
일반 실행:
  http://localhost:8080  ->  https://localhost:8443
  https://localhost:8443

특권 포트가 가능할 때 (Linux: root 또는 setcap / Android: root + CAP_NET_BIND_SERVICE):
  http://localhost:80    ->  https://localhost:443
  https://localhost
```

다른 포트를 쓰려면 `--http-port`와 `--https-port`를 줍니다.

HTTPS를 처음 실행하면 이 기기에서만 쓰는 CA와 서버 인증서를 보통 다음 위치에 만듭니다.

```text
~/.local/share/lserve/
├── rootCA.pem
├── rootCA-key.pem
├── localhost.pem
└── localhost-key.pem
```

**`rootCA-key.pem`은 절대로 공유하지 마세요.**

**루팅 Android에서 EUID 0으로 실행**하면 `$TERMUX__HOME/.local/share/lserve/`(또는 `/data/data/com.termux/files/home/.local/share/lserve/`)에서 CA 파일과 키가 모두 있는 기존 CA를 먼저 찾습니다. 없으면 `/data/local/tmp/lserve/`를 쓰고, 그 디렉터리는 소유자 `0:0`, 모드 `0700`입니다. 자세한 내용은 [docs/CERTIFICATES.md](docs/CERTIFICATES.md)를 참고하세요.

### 3. 로컬 CA 신뢰

경로 보기:

```sh
lserve ca
```

공개 CA 인증서만 내보내기:

```sh
lserve ca export ~/Downloads/lserve-rootCA.crt
```

Android/Termux에서는 예를 들어 이렇게 둡니다.

```sh
lserve ca export ~/storage/downloads/lserve-rootCA.crt
```

OS나 브라우저의 인증서 설정에서 내보낸 파일을 **CA 인증서**로 등록합니다. `rootCA-key.pem`은 복사하거나 설치하지 않습니다.

## 기본 동작

- 문서 루트는 현재 디렉터리
- bind는 `127.0.0.1`
- 특권 포트를 열 수 있으면 `80`/`443`, 아니면 `8080`/`8443`
- HTTP는 HTTPS로 `308` 리다이렉트
- 정적 파일 메서드는 `GET`, `HEAD`, `OPTIONS`
- 정적 응답의 CORS는 열어 둠
- `index.html`은 찾고, 디렉터리 목록은 없음
- 경로 탈출은 막음
- 심볼릭 링크는 기본으로 따라가고, `--no-symlinks`로 끔
- `.lserve`, `.certs/`, `.syntaxes/`, 설정한 로그 파일은 HTTP로 주지 않음
- Markdown은 자동으로 그리고, `?raw`이면 원문
- 서버에서 구문 강조, 라이트/다크
- 영상과 큰 파일은 Range 요청 한 구간을 스트리밍
- 콘솔에 access/error 로그
- 파일 로그와 시각이 붙은 로테이션은 선택
- Host/path 리버스 프록시는 선택
- 리버스 프록시의 upstream은 같은 머신의 loopback HTTP만
- SSE, 스트리밍, WebSocket Upgrade

## Markdown

아래 주소를 열면 서버가 HTML로 그립니다.

```text
https://localhost:8443/README.md
```

원문은 이렇게 받습니다.

```text
https://localhost:8443/README.md?raw
```

raw HTML은 기본적으로 꺼져 있습니다. 코드 블록은 언어에 맞춰 서버에서 구문 강조를 합니다. 코드 글꼴은 설치되어 있으면 **D2Coding**을 쓰고, 없으면 일반 고정폭 글꼴을 씁니다. 이 저장소에는 D2Coding 글꼴 파일이 들어 있지 않습니다.

Markdown 렌더링을 끄려면:

```sh
lserve --no-markdown
```

자세한 내용은 [docs/MARKDOWN.md](docs/MARKDOWN.md)를 참고하세요.

## Host/path 리버스 프록시

`.lserve` 예:

```ini
route.local.example.com=http://127.0.0.1:3000
route.local.example.com/api=http://127.0.0.1:4000
route.local.example.com/api/admin=http://127.0.0.1:5000
route.admin.local.example.com=http://localhost:3001
```

라우트 키는 Host만 적어도 되고, `HOST/PATH`처럼 경로를 붙여도 됩니다. 경로는 `/`로 나뉜 구간 단위로 비교하고, 가장 길게 맞는 prefix를 고릅니다. 원래 경로는 그대로 upstream에 넘깁니다.

`hosts` 예:

```text
127.0.0.1 local.example.com
127.0.0.1 admin.local.example.com
```

동작:

```text
https://local.example.com/              -> http://127.0.0.1:3000/...
https://local.example.com/api/users     -> http://127.0.0.1:4000/api/users
https://local.example.com/api/admin/x   -> http://127.0.0.1:5000/api/admin/x
https://admin.local.example.com/        -> http://localhost:3001/...
https://localhost                       -> 정적 파일
```

upstream 호스트 이름은 직접 정해도 됩니다. 다만 **실제로 접속하는 주소는 loopback이어야 합니다.** LAN, 원격, 인터넷 주소로는 리버스 프록시를 하지 않습니다.

자세한 내용: [docs/PROXY.md](docs/PROXY.md)

## 사설망 bind

기본 주소는 localhost입니다. 사설망 주소가 필요할 때만 이렇게 지정합니다.

```sh
lserve --bind 100.100.10.20
```

쓸 수 있는 주소는 loopback, RFC1918 IPv4, `100.64.0.0/10`(Tailscale IPv4 포함), IPv6 loopback, IPv6 ULA입니다. 공인 IP, `0.0.0.0`, `::`는 거부합니다.

사설망 밖에서 잠깐 열어 보려면 공인 주소에 직접 붙이지 말고, 앞에 Cloudflare Tunnel이나 Access Proxy를 둡니다.

## Cloudflare Tunnel / HTTP-only

```sh
lserve --http-only
cloudflared tunnel --url http://127.0.0.1:8080
```

바깥 TLS와 접근 통제는 터널이 맡고, `lserve`는 이 머신 안에서만 응답합니다.

## 정적 파일 CORS

정적 파일은 다른 출처에서도 읽을 수 있게 열어 둡니다.

```http
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, HEAD, OPTIONS
Access-Control-Allow-Headers: *
Access-Control-Max-Age: 86400
```

브라우저의 다른 페이지나 bookmarklet이 localhost 파일을 가져갈 수 있게 하려는 것입니다. 리버스 프록시 응답의 CORS는 upstream 앱이 보낸 그대로 둡니다.

## 로그

```sh
lserve \
  --access-log ./logs/access.log \
  --error-log ./logs/error.log
```

파일 로그를 켜도 콘솔에는 계속 나옵니다. 파일은 1 MiB마다 자르고, 시각이 붙은 이전 파일을 5개까지 남깁니다.

```text
access.log
access.log.20260924_231500
access.log.20260923_101122
```

로그를 문서 루트 안에 두면, 지금 쓰는 로그와 시각이 붙은 이전 파일은 HTTP에서 404로 숨깁니다.

개발 중 5xx 응답에 내부 설명을 조금 더 보고 싶다면:

```sh
lserve --verbose-errors
```

자세한 내용: [docs/LOGGING.md](docs/LOGGING.md)

## 설정 파일

주석이 달린 예제를 보려면:

```sh
lserve config
```

이미 있는 파일은 덮어쓰지 않고 `./.lserve`를 만듭니다.

```sh
lserve config --write
```

우선순위:

```text
CLI > ./.lserve > 자동 기본값
```

고른 문서 루트의 `.lserve`만 읽습니다. 위쪽 디렉터리는 찾지 않습니다.

[docs/CONFIGURATION.md](docs/CONFIGURATION.md)와 [.lserve.example](.lserve.example)을 참고하세요.

## 인증서 명령

```sh
lserve ca generate
lserve ca generate --force
lserve ca export PATH
lserve cert generate
lserve cert generate --force
lserve flush all
lserve flush cert
lserve flush ca
```

프로젝트 호스트 이름이 필요하면 `./.certs/server.pem`과 `server-key.pem`을 쓰고, 같은 전역 Root CA로 서명합니다. SAN에는 그 호스트 이름과 IP만 넣습니다. 와일드카드 SAN은 만들지 않습니다.

자세한 내용: [docs/CERTIFICATES.md](docs/CERTIFICATES.md)

## 셸 자동 완성

Bash 자동 완성은 위치에 따라 제안이 달라집니다. 첫 인자에는 디렉터리를 제안하고, 경로를 받는 옵션에만 파일 경로를 제안합니다. enum은 가능한 값만 나오고, `--route`는 `HOST=http://`를 제안합니다.

```sh
lserve completions bash > ~/.local/share/bash-completion/completions/lserve
lserve completions zsh  > /path/to/_lserve
lserve completions fish > ~/.config/fish/completions/lserve.fish
```

## 보안 모델

`lserve`는 **로컬·사설망용**입니다. 인증과 인가, 사용자 계정, Google SSO, DB, 로드 밸런싱, 원격 upstream, ACME, 공인 wildcard bind는 하지 않습니다. 리버스 프록시도 같은 머신의 loopback HTTP만 대상입니다.

잠깐 밖에서 보려면 Cloudflare Tunnel이나 Access Proxy를 앞에 둡니다. 규모가 커지면 nginx, Caddy, Envoy, Traefik으로 옮깁니다. 나중에 그 서버로 옮길 때 그대로 가져가도록 HTTP 메서드와 상태 코드, Host, forwarding header, Range, WebSocket, PEM은 표준 그대로 둡니다.

자세한 내용: [docs/SECURITY.md](docs/SECURITY.md)

## 문서

- [설정](docs/CONFIGURATION.md)
- [보안 모델](docs/SECURITY.md)
- [리버스 프록시](docs/PROXY.md)
- [Markdown](docs/MARKDOWN.md)
- [인증서](docs/CERTIFICATES.md)
- [로그](docs/LOGGING.md)
- [변경 이력](CHANGELOG.md)

## 소스에서 빌드

소스 빌드에는 Rust **1.88+**가 필요합니다.

Termux:

```sh
pkg install rust
```

Ubuntu:

```sh
sudo apt install cargo rustc
```

RHEL 계열은 배포판 Rust 패키지나 rustup을 쓸 수 있습니다. `rustc --version`으로 확인합니다.

```sh
git clone https://github.com/flymylee/lserve.git
cd lserve
cargo build --release
install -Dm755 target/release/lserve ~/.local/bin/lserve
```

릴리스 빌드 전에는 다음을 실행합니다.

```sh
cargo fmt --check
cargo check
cargo test
cargo build --release
```

빌드를 같게 맞추려면 생성된 `Cargo.lock`도 커밋합니다.

## 라이선스

MIT License. Copyright (c) 2026 Mooyeon LEE.

서드파티 고지는 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)와 [NOTICE](NOTICE)를 참고하세요.
