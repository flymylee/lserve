# lserve

**로컬/사설 환경을 위한 경량 HTTPS 정적 서버 + Markdown 뷰어 + Host/path 기반 reverse proxy**입니다.

`lserve`가 맡는 역할은 의도적으로 작습니다. 로컬/사설 데이터를 안정적으로 전달하고, 로컬 TLS를 처리하고, 필요하면 같은 머신의 HTTP 애플리케이션으로 Host별 라우팅합니다. 인증, SSO, 사용자 관리, rate limiting, 공개 인터넷 노출, 고가용성 같은 책임은 별도 레이어에 둡니다.

> **기본 보안 경계:** 기본 bind는 `127.0.0.1`입니다. `--bind`를 명시해도 loopback/사설/shared 주소만 허용하며 `0.0.0.0` 같은 wildcard나 public IP는 거부합니다.

[English README](README.md)

## Getting Started

### 1. Rust 설치

`lserve` v1.0.0의 기준 Rust는 **1.88+**입니다.

Termux:

```sh
pkg install rust
```

Ubuntu:

```sh
sudo apt install cargo rustc
```

RHEL 계열에서는 배포판 Rust 패키지 또는 rustup을 사용할 수 있습니다.

```sh
rustc --version
```

으로 버전을 확인합니다.

### 2. 빌드 및 설치

```sh
git clone https://github.com/flymylee/lserve.git
cd lserve
cargo build --release
install -Dm755 target/release/lserve ~/.local/bin/lserve
```

`~/.local/bin`이 `PATH`에 포함되어 있어야 합니다.

### 3. 현재 디렉터리 서빙

```sh
cd ~/my-site
lserve
```

Android/Linux, root/non-root 구분 없이 기본 포트는 동일합니다.

```text
http://localhost:8080  ->  https://localhost:8443
https://localhost:8443
```

80/443 등이 필요하면 `--http-port` / `--https-port`로 명시합니다.

최초 HTTPS 실행 시 일반적으로 다음 위치에 기기 전용 CA/서버 인증서를 만듭니다.

```text
~/.local/share/lserve/
├── rootCA.pem
├── rootCA-key.pem
├── localhost.pem
└── localhost-key.pem
```

**`rootCA-key.pem`은 절대로 공유하지 마세요.**

**루팅 Android에서 EUID 0으로 실행**하면 먼저 `$TERMUX__HOME/.local/lserve/`의 완전한 기존 CA를 찾고, 호환을 위해 기존 `.local/share/lserve/`도 확인합니다. 사용할 기존 CA가 없으면 `/data/local/tmp/lserve/`를 사용하며 fallback 디렉터리는 `0:0`, mode `0700`으로 관리합니다. 자세한 내용은 [docs/CERTIFICATES.md](docs/CERTIFICATES.md)를 참고하세요.

### 4. Root CA 신뢰 등록

경로 확인:

```sh
lserve ca
```

공개 CA 인증서만 export:

```sh
lserve ca export ~/Downloads/lserve-rootCA.crt
```

Android/Termux라면 예를 들어:

```sh
lserve ca export ~/storage/downloads/lserve-rootCA.crt
```

OS/브라우저의 인증서 설정에서 export한 파일을 **CA 인증서**로 등록합니다. `rootCA-key.pem`은 복사하거나 설치하지 않습니다.

## 기본 동작

- document root = 현재 디렉터리
- bind = `127.0.0.1`
- Android/Linux, root/non-root 공통 기본 포트 = `8080/8443`
- HTTP -> HTTPS `308` redirect
- static method = `GET`, `HEAD`, `OPTIONS`
- static CORS 기본 open
- `index.html` 지원, **directory listing 없음**
- path traversal 차단
- symlink 기본 허용, `--no-symlinks`로 opt-out
- `.lserve`, `.certs/`, `.syntaxes/`, 설정된 log 파일은 HTTP 차단
- Markdown 자동 렌더링, `?raw`로 원문
- 서버사이드 syntax highlighting + light/dark
- 동영상/대용량 파일용 single Range streaming
- console access/error log
- 선택적 file log + timestamp rotation
- 선택적 Host/path 기반 reverse proxy
- proxy upstream은 같은 머신의 loopback HTTP만 허용
- SSE/streaming/WebSocket Upgrade 지원

## Markdown

```text
https://localhost:8443/README.md
```

를 열면 서버에서 HTML로 렌더링합니다.

원본 Markdown:

```text
https://localhost:8443/README.md?raw
```

raw HTML은 기본 비활성입니다. fenced code는 서버에서 언어별 syntax highlighting을 수행합니다. 코드 폰트는 로컬에 설치된 **D2Coding**을 우선하고, 없으면 일반 monospace fallback을 사용합니다. 이 소스 패키지는 D2Coding 폰트 바이너리를 재배포하지 않습니다.

Markdown 렌더링을 끄려면:

```sh
lserve --no-markdown
```

자세한 내용은 [docs/MARKDOWN.md](docs/MARKDOWN.md)를 참고하세요.

## Host 기반 reverse proxy

`.lserve`:

```ini
route.local.example.com=http://127.0.0.1:3000
route.local.example.com/api=http://127.0.0.1:4000
route.local.example.com/api/admin=http://127.0.0.1:5000
route.admin.local.example.com=http://localhost:3001
```

route key는 Host만 쓰거나 `HOST/PATH`로 쓸 수 있습니다. path는 segment boundary 기준 prefix matching을 하며 가장 긴 prefix가 우선하고, upstream에는 원래 path를 그대로 전달합니다.

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
https://localhost                       -> static fallback
```

upstream hostname 자체는 사용자 정의 이름이어도 되지만 **실제 연결 직전에 resolve한 목적지 IP가 loopback이어야 합니다.** LAN/원격/인터넷 upstream은 지원하지 않습니다.

자세한 내용: [docs/PROXY.md](docs/PROXY.md)

## 사설망 bind

기본값은 계속 localhost입니다. 필요한 경우에만:

```sh
lserve --bind 100.100.10.20
```

처럼 명시합니다.

허용 대상에는 loopback, RFC1918 IPv4, `100.64.0.0/10`(Tailscale IPv4 포함), IPv6 loopback, IPv6 ULA가 포함됩니다. public IP, `0.0.0.0`, `::`는 거부합니다.

사설 범위 밖에서 시험적으로 접근하려면 public bind를 열지 말고 앞단에 Cloudflare Tunnel 또는 별도 Access Proxy를 둡니다.

## Cloudflare Tunnel / HTTP-only

```sh
lserve --http-only
cloudflared tunnel --url http://127.0.0.1:8080
```

외부 TLS/접근 통제는 tunnel 쪽이 맡고 `lserve`는 local origin으로 남습니다.

## Static CORS

정적 응답은 의도적으로 cross-origin read를 허용합니다.

```http
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, HEAD, OPTIONS
Access-Control-Allow-Headers: *
Access-Control-Max-Age: 86400
```

외부 웹페이지나 bookmarklet에서 localhost resource를 읽는 용도입니다. proxy 응답의 CORS는 upstream 애플리케이션 정책을 그대로 유지합니다.

## 로그

```sh
lserve \
  --access-log ./logs/access.log \
  --error-log ./logs/error.log
```

파일 로그를 켜도 console 출력은 유지됩니다. 기본 rotation은 1 MiB / 과거 5세대입니다.

```text
access.log
access.log.20260924_231500
access.log.20260923_101122
```

로그가 document root 안에 있으면 활성 로그와 인식 가능한 rotation 파일을 HTTP 404 처리합니다.

개발 중 5xx 응답에 제한적인 내부 설명까지 보고 싶다면:

```sh
lserve --verbose-errors
```

자세한 내용: [docs/LOGGING.md](docs/LOGGING.md)

## 설정 파일

주석이 포함된 예제 출력:

```sh
lserve config
```

기존 파일을 덮어쓰지 않고 `./.lserve` 생성:

```sh
lserve config --write
```

우선순위:

```text
CLI > ./.lserve > 자동 기본값
```

선택한 document root의 `.lserve` 하나만 읽고 상위 디렉터리는 탐색하지 않습니다.

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

project hostname이 필요하면 `./.certs/server.pem` / `server-key.pem`을 사용하며 global Root CA로 서명합니다. SAN은 exact hostname/IP로 구성하고 wildcard SAN은 자동 생성하지 않습니다.

자세한 내용: [docs/CERTIFICATES.md](docs/CERTIFICATES.md)

## Shell completion

Bash completion은 문맥을 구분합니다. 첫 positional은 디렉터리를 완성하고, path 인자를 받는 옵션에서만 filesystem completion이 나오며, enum은 유효 값만, `--route`는 `HOST=http://` 템플릿을 제안합니다.

```sh
lserve completions bash > ~/.local/share/bash-completion/completions/lserve
lserve completions zsh  > /path/to/_lserve
lserve completions fish > ~/.config/fish/completions/lserve.fish
```

## 보안/운영 경계

`lserve`는 **local/private infrastructure**입니다. 자체 authentication/authorization, 사용자 계정, Google SSO, DB, load balancing, 원격 upstream, ACME, public wildcard bind를 구현하지 않습니다.

일시적인 외부 접근은 Cloudflare Tunnel/Access Proxy를 앞에 두고, 규모가 커지면 nginx/Caddy/Envoy/Traefik 같은 production stack으로 옮기는 것을 전제로 합니다. HTTP method/status, Host, forwarding header, Range, WebSocket, PEM 인증서 같은 표준 개념을 유지하는 이유도 마이그레이션 충돌을 줄이기 위해서입니다.

자세한 내용: [docs/SECURITY.md](docs/SECURITY.md)

## 문서

- [설정](docs/CONFIGURATION.md)
- [보안 모델](docs/SECURITY.md)
- [Reverse proxy](docs/PROXY.md)
- [Markdown](docs/MARKDOWN.md)
- [인증서](docs/CERTIFICATES.md)
- [Logging](docs/LOGGING.md)
- [변경 이력](CHANGELOG.md)

## 빌드 검증 안내

v1.0.0 재구성 코드는 현재 Hyper/rustls/rcgen/Comrak/syntect API 기준으로 작성했습니다. release 전에 Rust toolchain이 있는 환경에서 반드시:

```sh
cargo fmt --check
cargo check
cargo test
cargo build --release
```

를 실행하고 application build 재현성을 위해 생성된 `Cargo.lock`도 commit하는 것을 권장합니다.

## License

MIT License. Copyright (c) 2026 Mooyeon LEE.

third-party 고지는 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)와 [NOTICE](NOTICE)를 참고하세요.
