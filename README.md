**한국어** · [English](README.en.md)

# Ferrite

Rust로 작성한 **Minecraft Java 서버**. Folia 스타일 리전 멀티스레딩을 쓰고, 내장 JVM으로
Bukkit/Spigot/Paper/Folia 플러그인을 실행하는 것이 목표입니다.

- **대상**: Minecraft Java **26.3**, 프로토콜 **777**
- **상태 (2026-11-08)**: 생존·게임 시스템을 프로토콜 봇과 Rust/Java 테스트로 검증했습니다. 기본 월드는 이제 `world/`에 Anvil 파일로 저장되고 재시작 뒤에도 유지됩니다. 전체 Rust 테스트, clippy, fmt, Java Gradle 테스트와 Linux/Windows CI가 통과했습니다. **실제 26.3 클라이언트 접속은 아직 검증하지 않았습니다.**
- **라이선스**: GPL-3.0-or-later

> **English** — Ferrite is a Minecraft Java server in Rust (26.3, protocol 777) with Folia-style
> regionized multithreading and an embedded JVM for Bukkit/Paper plugins. It runs a bot-verified
> survival server and has **not** been tested with a real 26.3 client. No Mojang code or assets are
> distributed; vanilla data is generated locally from the official jar.

## 상태

| 구분 | 내용 |
|---|---|
| ✅ 동작 (봇 검증) | 접속·암호화·서명 채팅, 서버 목록, 리전 멀티스레딩, 월드 IO·조명과 기본 월드 저장(`world/`), 블록·아이템, 인벤토리·제작·레시피북·컨테이너, 생존·전투·몹(바이옴별 자연 스폰, 번식, 늑대 길들이기), 스폰 알, 몹·아이템 추적, 주민 거래, 떠돌이 상인 스폰 규칙·거래 중 수명·라마 동반 생명주기(서버 회귀 테스트), 주크박스·밀랍·로드스톤 같은 블록 사용과 그 발전과제, 레드스톤·호퍼·명령 블록, 양조·비컨, 네더와 포탈, 유체·화재, 명령 대부분(`/give`·`/tick`·`/forceload`·`/place`·`/damage` 등, 상세는 [`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
| 🟡 부분 구현 | 피스톤(움직이는 블록·엔티티 이동과 클라이언트 오버레이; 애니메이션은 실제 클라이언트 미검증, [`docs/pistons.md`](docs/pistons.md)), 엔드 전투(아래, [`docs/end-dimension.md`](docs/end-dimension.md)) |
| 🔧 진행 중 | 플러그인 호환(Paper/Bukkit API 일부, NMS 일부; 미지원 호출은 명시적으로 실패) |
| ⛔ 아직 없음 | 실제 클라이언트 검증 |

**엔드**: 포탈 활성화와 차원 이동, 도착 플랫폼, 바닐라 월드젠 호스트, 공허 판정, 드래곤 보스바·전투·수정 회복/파괴, 출구 포탈과 게이트웨이를 구현했습니다. 드래곤의 선회·돌진(strafe)·착지/대기/이륙·숨결·화염구 행동, 8개 서브 히트박스, 4개 수정 기반 단계별 재소환, 전투/단계 저장·복원, 게이트웨이 생성 빔(`Age` 블록 엔티티; 서버가 나이를 올리고 저장해 재로드 때 빔이 처음부터 다시 시작되지 않음), 첫 귀환 때의 크레딧 신호(`WIN_GAME`)와 "Free the End" 업적도 서버 쪽에는 구현되어 있습니다. 남은 서버 쪽 차이는 착지 후 스캔/공격/불꽃 서브페이즈의 정확한 타이밍, 보라색 텔레포트 빔과 2400틱 자동 빔, 외곽 섬 탐색 반경입니다. 빔 렌더링, 드래곤 움직임, 보스바, 크레딧 화면 등 실제 클라이언트 화면은 확인하지 않았습니다([`docs/end-dimension.md`](docs/end-dimension.md), 수동 점검표 [`docs/manual-checklist.md`](docs/manual-checklist.md)).

플러그인 API는 Paper `Server`/`Player`의 일부만 처리합니다. 처리하지 않는 호출은
`UnsupportedOperationException("Ferrite: not implemented: …")`로 드러납니다. EssentialsX 같은
대형 플러그인의 모든 경로와 NMS 의존 플러그인은 아직 동작하지 않습니다.

## 빌드와 실행

필요: **Rust stable**(`rust-toolchain.toml`이 Windows MSVC를 고정, Linux/macOS는 `RUSTUP_TOOLCHAIN=stable`),
**Java 25**, **Python 3**. Windows GUI에는 **Microsoft Edge WebView2 Runtime**이 필요합니다
(Windows 10/11에는 기본 포함).

```sh
source tools/env.sh                        # Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # 바닐라 데이터 생성 (최초 1회, 네트워크 + Java 25)
cargo run --release -p ferrite-server      # 오프라인 모드 로컬 서버
```

처음 실행하면 `eula.txt`를 만들고 종료합니다. [Minecraft EULA](https://aka.ms/MinecraftEULA)를 읽고
직접 `eula=true`로 바꾼 뒤 다시 실행하세요. Ferrite가 대신 동의하지 않습니다.

`vanilla/`는 git에 포함되지 않습니다. 패킷 ID·블록·레지스트리 수치는 추측하지 말고
`vanilla/<버전>/generated/` 리포트에서 읽습니다. 설정 키는 [`docs/CONFIG.md`](docs/CONFIG.md).

기본 월드 디렉터리는 서버 작업 폴더의 `world/`입니다. 청크는 플레이 중 메모리 캐시에
유지되고, 60초마다 저장되며 언로드와 정상 종료 때도 Anvil 형식으로 기록됩니다. 엔티티는
로드된 청크별 스냅샷을, 접속 중인 플레이어 상태도 60초마다 저장합니다. 강제 종료 직전의
자동 저장 주기 안 변경분은 유실될 수 있습니다. Windows에서 `ferrite-server.exe`를
직접 실행하면 서버 시작·종료와 명령 입력, 로그 보기를 제공하는 Tauri 2 GUI가 열립니다.
실행 파일은 `vanilla/26.3/generated/`가 준비된 Ferrite 작업 폴더에 두어야 합니다.
`--headless`를 붙이거나 Linux에서 실행하면 GUI 없이 터미널의 표준 입력을 받는
헤드리스 서버로 실행됩니다. GUI 동작과 제약은 [`docs/windows-console.md`](docs/windows-console.md)에
정리되어 있습니다.
처음 실행 전 EULA를 읽고 동의한 경우에만 `eula.txt`를 직접 수정해야 합니다.

## 검증

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

서버 동작은 `crates/ferrite-server/tests`의 프로토콜 봇 세션으로 검증합니다. 2026-11-08 기준
`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `java/ferrite-api-impl`의 `./gradlew test`가 통과했습니다.
[main CI run 42398148054](https://github.com/yxshadxw/ferrite-mc/actions/runs/42398148054)도 Linux와
Windows에서 fmt·clippy·전체 Rust 테스트를 통과했습니다. 봇 테스트 성공은 실제 Minecraft 클라이언트
화면 검증을 의미하지 않습니다.

## 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| 1 | 기반: 프로토콜 코덱, NBT, 네트워킹, 리전화 | 완료 |
| 2 | 월드 IO·이동·블록·채팅 | 완료 (봇 검증) |
| 3 | 리전라이저 벤치마크 | 진행 중 |
| 4 | 플러그인 브리지(Bukkit 이벤트·명령·스케줄러) | 진행 중, 최우선 |
| 5 | 게임플레이: 인벤토리·엔티티·전투·제작 | 진행 중 |
| 6 | Paper 기능 동등성 | 예정 |
| 7 | 플러그인 생태계 호환 확대 | 예정 |

## 크레이트

| 크레이트 | 담당 |
|---|---|
| `ferrite-protocol`, `ferrite-net` | 패킷 코덱, 연결·암호화·로그인 |
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

- 남은 게임·플러그인 작업: 드래곤 착지 서브페이즈 타이밍과 보라색 텔레포트 빔, 여러 월드의
  플러그인 블록 엔티티 접근, Paper/Bukkit 및 NMS 호환 범위 확대.
- 실제 클라이언트 검증: 차원 이동/도착 플랫폼/공허 사망, 엔드 보스바·드래곤·게이트웨이,
  피스톤 애니메이션, 동물 상호작용, 볼트·거래 UI, 플러그인 동작을 통합 서버에서 확인.
- 최적화 조사 참고: [서버 최적화 모드](https://wisehosting.com/blog/10-best-minecraft-server-optimization-mods),
  [최적화 모드팩](https://modrinth.com/modpack/server-optimizationn),
  [최적화 가이드](https://github.com/YouHaveTrouble/minecraft-optimization),
  [Pumpkin Rust 서버](https://github.com/Pumpkin-MC/Pumpkin),
  [Pumpkin PatchBukkit 브리지](https://github.com/Pumpkin-MC/PatchBukkit).
  최적화 조사 메모: [`OPTIMIZATIONS.md`](docs/OPTIMIZATIONS.md).
  최적화와 브리지 설계 참고 자료로 조사하고, 구현·호환성을 그대로 가정하지 않는다.

## 후속 로드맵

- [Advanced Slime World Manager (ASWM)](https://github.com/Paul19988/Advanced-Slime-World-Manager) /
  [Advanced Slime Paper](https://github.com/InfernalSuite/AdvancedSlimePaper) 참고해 Slime Region Format
  월드 로드·저장·전환 및 플러그인 연동을 구현한다.
- **마지막 단계:** Java 호스트를 대체할 Rust 월드젠을 구현한다. 바닐라 26.3 대조와 기존 사용자
  월드젠 호스트 병행을 완료한 뒤 진행한다. 성능 조사에는 [C2ME](https://github.com/RelativityMC/C2ME-fabric),
  [Moonrise](https://github.com/Tuinity/Moonrise), [Noisium](https://github.com/Steveplays28/noisium),
  C2ME의 실험적 [OpenCL 가속 모듈](https://github.com/RelativityMC/C2ME-fabric/tree/dev/26.3.0/c2me-opts-accel-opencl)
  등 월드젠 최적화 프로젝트를 참고한다. 기법과 병목을 조사해 Ferrite에 독립 구현하고, 바닐라 대조·성능
  측정으로 검증한다. OpenCL은 CPU 경로와 결과 동등성 및 장치 호환성을 확인한 뒤 선택적으로 평가한다.

Rust 월드젠을 제외한 작업은 독립된 파일 소유권과 명시적인 인수 조건을 정해 병렬 처리한다.
남은 작업 목록은 [`docs/BACKLOG.md`](docs/BACKLOG.md)가 유일한 기준이다.
