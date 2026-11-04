# Ferrite

Rust로 작성한 **Minecraft Java 서버**. Folia 스타일 리전 멀티스레딩을 쓰고, 내장 JVM으로
Bukkit/Spigot/Paper/Folia 플러그인을 실행하는 것이 목표입니다.

- **대상**: Minecraft Java **26.3**, 프로토콜 **777**
- **상태**: 프로토콜 봇과 Java 대조 테스트로 검증한 생존 기본기. **실제 26.3 클라이언트 접속은 아직 검증하지 않았습니다.**
- **라이선스**: GPL-3.0-or-later

> **English** — Ferrite is a Minecraft Java server in Rust (26.3, protocol 777) with Folia-style
> regionized multithreading and an embedded JVM for Bukkit/Paper plugins. It runs a bot-verified
> survival server and has **not** been tested with a real 26.3 client. No Mojang code or assets are
> distributed; vanilla data is generated locally from the official jar.

## 상태

| 구분 | 내용 |
|---|---|
| ✅ 동작 (봇 검증) | 접속·암호화·서명 채팅, 서버 목록, 리전 멀티스레딩, 월드 IO·조명, 블록·아이템, 인벤토리·제작·컨테이너, 생존·전투·몹, 주민 거래, 레드스톤·호퍼·명령 블록, 양조·비컨, 네더와 포탈, 유체·화재, 명령 약 70개, 다중 버전(26.3 ↔ 26.2) |
| 🔧 진행 중 | 플러그인 호환(Paper/Bukkit API, NMS), 피스톤 |
| ⛔ 아직 없음 | 실제 클라이언트 검증, 엔드 차원 |

플러그인 API는 Paper `Server`/`Player`의 일부만 처리합니다. 처리하지 않는 호출은
`UnsupportedOperationException("Ferrite: not implemented: …")`로 드러납니다. EssentialsX 같은
대형 플러그인의 모든 경로와 NMS 의존 플러그인은 아직 동작하지 않습니다.

## 빌드와 실행

필요: **Rust stable**(`rust-toolchain.toml`이 Windows MSVC를 고정, Linux/macOS는 `RUSTUP_TOOLCHAIN=stable`),
**Java 25**, **Python 3**.

```sh
source tools/env.sh                        # Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # 바닐라 데이터 생성 (최초 1회, 네트워크 + Java 25)
cargo run --release -p ferrite-server      # 오프라인 모드 로컬 서버
```

처음 실행하면 `eula.txt`를 만들고 종료합니다. [Minecraft EULA](https://aka.ms/MinecraftEULA)를 읽고
직접 `eula=true`로 바꾼 뒤 다시 실행하세요. Ferrite가 대신 동의하지 않습니다.

`vanilla/`는 git에 포함되지 않습니다. 패킷 ID·블록·레지스트리 수치는 추측하지 말고
`vanilla/<버전>/generated/` 리포트에서 읽습니다. 설정 키는 [`docs/CONFIG.md`](docs/CONFIG.md).

## 검증

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

서버 동작은 `crates/ferrite-server/tests`의 프로토콜 봇 세션으로 검증합니다. CI는 Linux/Windows에서
fmt·clippy·test를 돌리고 바닐라 데이터가 커밋되지 않았는지도 확인합니다.

## 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| 1 | 기반: 프로토콜 코덱, NBT, 네트워킹, 리전화 | 완료 |
| 2 | 월드 IO·이동·블록·채팅 | 완료 (봇 검증) |
| 3 | 리전라이저 벤치마크 | 진행 중 |
| 4 | 플러그인 브리지(Bukkit 이벤트·명령·스케줄러) | 진행 중, 최우선 |
| 5 | 게임플레이: 인벤토리·엔티티·전투·제작 | 진행 중 |
| 6 | 다중 버전 지원 | 진행 중 |
| 7 | Paper 기능 동등성 | 예정 |
| 8 | 플러그인 생태계 호환 확대 | 예정 |

## 크레이트

| 크레이트 | 담당 |
|---|---|
| `ferrite-protocol`, `ferrite-net`, `ferrite-translate` | 패킷 코덱, 연결·암호화·로그인, 다중 버전 변환 |
| `ferrite-region` | Folia 스타일 리전라이저와 스케줄러 |
| `ferrite-world`, `ferrite-light`, `ferrite-nbt` | 청크·Anvil IO, 조명, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | 엔티티·몹, 유체, 메뉴·아이템·레시피·전리품 |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | 명령 디스패처, 게임룰·권한, `/tps`·`/profile` |
| `ferrite-plugin`, `ferrite-plugin-meta`, `ferrite-jvm` | 플러그인 로딩, `plugin.yml`, 내장 JVM |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | 바닐라 데이터, 텍스트, 수학, 설정, 콘솔 |
| `ferrite-server` | 바이너리: 설정·콘솔·배선 |

## 법적 고지

이 저장소는 **Mojang의 코드나 에셋을 배포하지 않습니다.** 바닐라 데이터는 사용자의 환경에서 공식 서버 jar로
생성되어 `vanilla/`에 저장되고 git에서 제외됩니다. Minecraft는 Mojang Studios의 상표이며, 이 프로젝트는
Mojang·Microsoft와 무관한 비공식 프로젝트입니다.

## 라이선스

GPL-3.0-or-later ([`LICENSE`](LICENSE)). Paper·Folia·Lithium 등 GPL/LGPL 코드를 포팅하기 위해 MIT/Apache에서
GPLv3로 바꿨고, 출처는 [`NOTICE.md`](NOTICE.md)와 각 소스 파일 주석에 적습니다.

## 해야 할 일

- 플러그인 브리지 완성(NMS 호환 포함), 피스톤, 엔드 차원, 실제 클라이언트 검증
- 최적화 참고: [서버 최적화 모드](https://wisehosting.com/blog/10-best-minecraft-server-optimization-mods),
  [최적화 모드팩](https://modrinth.com/modpack/server-optimizationn),
  [최적화 가이드](https://github.com/YouHaveTrouble/minecraft-optimization)
- 모든 작업을 병렬 처리
