<div align="center">

# Ferrite

**Folia 스타일 리전 멀티스레딩 기반 순수 Rust Minecraft Java 26.3 서버**  
<sub>단일 틱 루프 병목을 없애고 바닐라 26.3 코어를 네이티브 Rust 병렬 아키텍처로 구현했습니다.</sub>

<p>
  <img src="https://img.shields.io/badge/Rust-Edition%202024-f97316?style=flat-square&logo=rust" alt="Rust Edition 2024">
  <img src="https://img.shields.io/badge/Minecraft-26.3%20(Protocol%20777)-22c55e?style=flat-square" alt="Minecraft 26.3">
  <img src="https://img.shields.io/badge/Worldgen-Pure%20Rust-ea580c?style=flat-square" alt="Pure Rust Worldgen">
  <img src="https://img.shields.io/badge/Storage-.fworld%20RAM--Blob-f59e0b?style=flat-square" alt=".fworld Storage">
  <img src="https://img.shields.io/badge/License-GPL--3.0--or--later-3b82f6?style=flat-square" alt="GPL-3.0-or-later">
</p>

<p>
  <a href="#-벤치마크-ferrite-vs-folia--paper--purpur--leaf">벤치마크</a> ·
  <a href="#-핵심-기능">핵심 기능</a> ·
  <a href="#-빠른-시작">빠른 시작</a> ·
  <a href="#-구현-상태">구현 상태</a> ·
  <a href="#-크레이트-구조">크레이트</a> ·
  <a href="docs/CONFIG.md">설정 가이드</a> ·
  <a href="README.en.md">English</a>
</p>

</div>

---

## ⚡ 벤치마크: Ferrite vs Pumpkin · Folia · Paper · Vanilla (2026-11-25)

> **2026-11-25 최신 전체 비교 측정 (SLP·Wire 최적화 반영)**: 12-vCPU Xeon Platinum 8358 (`2.60GHz`), 62 GiB RAM, Linux, OpenJDK `25.0.5.1`, `rustc 1.100.0` (Edition 2024, `lto = "fat"`) 환경에서 **Minecraft Java 26.3 (Protocol 777)** 동일 조건(`seed=0` 야생 월드, `view-distance=8`, `compression=256`, `online-mode=false`)으로 **Ferrite**, **Pumpkin 0.2.2+26.3-26.60**, **Folia 26.3**, **Paper 26.3**, **Vanilla 26.3** 5종을 직접 측정했습니다.

<p align="center">
  <img src="docs/perf/2026-11-25/detailed-comparison.svg" alt="Ferrite vs Pumpkin, Folia, Paper, Vanilla 26.3 — Detailed Benchmark (2026-11-25)" width="100%">
</p>

### 5종 서버 비교 (`seed=0` 야생 월드, `view=8`, `2026-11-25` 실측)

| 측정 항목 | Ferrite (Rust) | Pumpkin 0.2.2 (Rust) | Folia 26.3 (Java) | Paper 26.3 (Java) | Vanilla 26.3 (Java) |
|---|---:|---:|---:|---:|---:|
| **콜드 부팅 준비 시간** | **`118.2 ms`** | `145.6 ms` | `12,758.2 ms` | `13,331.4 ms` | `9,106.0 ms` |
| **유휴(Idle) `VmRSS`** | `203.70 MB` | **`65.55 MB`** | `2,131.01 MB` | `2,209.87 MB` | `1,735.89 MB` |
| **SLP `c=1` (200회)** | **`4,026.9/s`** *(p50 0.19 ms)* | `3,492.1/s` *(p50 0.23 ms)* | `925.3/s` *(p50 0.94 ms)* | `976.5/s` *(p50 0.90 ms)* | `1,139.6/s` *(p50 0.75 ms)* |
| **SLP `c=32` (500회)** | **`7,057.0/s`** *(p50 2.87 ms)* | `5,806.7/s` *(p50 3.58 ms)* | `4,663.3/s` *(p50 6.66 ms)* | `3,660.2/s` *(p50 7.06 ms)* | `3,760.7/s` *(p50 6.40 ms)* |
| **SLP `c=64` (2,000회)** | **`7,203.2/s`** *(p50 5.83 ms)* | `6,241.0/s` *(p50 7.28 ms)* | `4,614.5/s` *(p50 10.83 ms)* | `4,917.3/s` *(p50 10.25 ms)* | `5,131.0/s` *(p50 8.79 ms)* |
| **20명 Play 진입 (`p50`)** | **`12.19 ms`** | `28.62 ms` | `2,885.95 ms` | `1,491.55 ms` | `1,861.44 ms` |
| **50명 Play 진입 (`p50`)** | **`20.12 ms`** | `74.74 ms` | `1,697.07 ms` | `424.39 ms` | `148.93 ms` |
| **100명 Play 진입 (`p50` / `p99`)** | **`48.47 / 67.53 ms`** *(100/100)* | `113.98 / 219.90 ms` *(100/100)* | `3,427.23 / 6,571.28 ms` *(100/100)* | `602.62 / 2,526.74 ms` *(100/100)* | `181.73 / 730.64 ms` *(100/100)* |
| **100명 피크 `VmRSS`** | **`550.45 MB`** | `624.95 MB` | `2,526.85 MB` | `2,585.61 MB` | `2,095.33 MB` |

### `ferrite-server` E2E 부하 테스트 (`bots`, `view=10`, `--lighting-pool`, `2026-11-25`)

| 워크로드 / 동시 봇 | 접속 / 완주 | 접속 `p50` / `p99` | 평균 / `p99` MSPT | RSS | CPU |
|---|---:|---:|---:|---:|---:|
| **`superflat` 50봇** | 50 / 50 | **44.07 / 45.27 ms** | **0.601 / 0.914 ms** | **154.1 MiB** | 16.0% |
| **`fixture` 50봇** | 50 / 50 | **44.02 / 45.47 ms** | **0.868 / 8.555 ms** | **262.4 MiB** | 16.0% |
| **`superflat` 200봇** | 200 / 200 | **44.04 / 49.09 ms** | **1.339 / 2.033 ms** | **199.1 MiB** | 162.7% |
| **`fixture` 200봇** | 200 / 200 | **44.05 / 53.21 ms** | **1.458 / 9.134 ms** | **362.4 MiB** | 157.4% |
| **`superflat` 400봇** | 400 / 400 | **44.44 / 51.88 ms** | **2.816 / 9.483 ms** | **317.3 MiB** | 583.8% |
| **`fixture` 400봇** | 400 / 400 | **44.33 / 63.40 ms** | **2.399 / 14.556 ms** | **504.1 MiB** | 571.9% |

- **순수 Rust 월드젠 스케일링 (`worldgen_bench`, `2026-11-25`)**: Overworld `1T 315.5/s (p50 2.79 ms) → 4T 1,251.9/s (99.2% 효율) → 6T 1,845.9/s (97.5% 효율) → 12T 2,109.6/s (6.69x, 피크 2,136.7/s)`, Nether **`11,766.3/s` (피크 `12,365.9/s`, `0.085 ms/chunk`)**, End **`21,572.4/s` (`0.046 ms/chunk`, 피크 `21,605.9/s`)**
- **엔진 핫패스 (`flush_bench`, `2026-11-25`)**: `.fworld` v3 LZ4 64청크 저장 **`3.21 ms`** (직렬 Anvil Zlib `6.46 ms` 대비 **2.01x**), 200인 청크 팬아웃 **`0.21 ms`** (**186.6x**), `1,000x` 하이트맵 재계산 **`14.02 ms`** (`14.0 us/chunk`), `50,000x` 레이캐스트 **`1.09 ms`** (`21.9 ns/ray`)
- <sub>최신 상세 리포트: [2026-11-25 종합 벤치마크 리포트](docs/perf/2026-11-25/README.md) · 과거 기록: [2026-11-24 리포트](docs/perf/2026-11-24/README.md) · [2026-11-23 리포트](docs/perf/2026-11-23/server-comparison.md)</sub>


---

## ✨ 핵심 기능

### 🔀 Folia 스타일 리전 멀티스레딩 (`ferrite-region`)
월드 내 서로 떨어진 활성 구역을 독립적인 리전으로 분할해 병렬 틱 처리하며, 리전 병합·분할 및 크로스 리전 엔티티/레드스톤/포탈 이동을 소유권 이전 프로토콜로 안전하게 조율합니다. 4,096청크 홀더 배치 변경 시 코디네이터 시그널 시간을 `13.00 ms → 2.64 ms`로 단축했습니다.

### 🌍 순수 Rust 병렬 월드젠 (`ferrite-worldgen`)
별도 JVM 프로세스 없이 Overworld, Nether, End 지형과 바이옴, 동굴, 구조물(Jigsaw·장식)을 멀티코어로 직접 생성합니다(레거시 `ferrite-jvm` 호스트 제거 완료).

### 💡 ScalableLux 스타일 병렬 라이팅 풀 (`ferrite-light`)
`--lighting-pool` 활성화 시 `(chunk_x, chunk_z)` 전역 락 순서와 워커별 로컬 엔진을 사용해 락 경합 없이 청크 조명을 병렬 재계산합니다. 12개 전체 높이(`-4..=19`) 작업 기준 **8워커 5.23x 가속**을 달성했고, `201,326,592`개 블록/종류 대조에서 오차 0건을 검증했습니다. [라이팅 벤치 →](docs/perf/2026-11-21/lighting-bench.md)

### 💾 `.fworld` (`FWRD` v3) RAM-Blob & Anvil 스토리지 / 고속 네트워크 (`ferrite-world`, `ferrite-net`)
- **영구 저장 백엔드 (비저장 인메모리 모드 제거)**:
  - `--storage fworld` (기본값, `.fworld` / `FWRD` v3): 청크별 독립 압축 레코드(`FworldColdStore`) + 스트리밍 디코딩으로 RAM 상주 메모리를 절약하면서 미변경 청크 재압축 없이 일괄 병렬 NBT 직렬화·압축 후 즉시 원자적 저장하며, 멀티코어 병렬 압축/해제·`xxh32` 무결성 체크섬·사전 크기 상한 검증을 수행합니다(64청크 LZ4 저장 **`3.85 ms`** — 직렬 Anvil `41.18 ms` 대비 **10.71x**, 증분 4/64청크 저장 **`0.84 ms`**, 콜드 로드 **`0.08 ms`**).
  - `--storage anvil`: 최대 8-리전 병렬 플러시(`2.62x`–`7.67x` LZ4), `Arc<Chunk>` Copy-on-Write 청크 스냅샷, `RwLock` + `O(1)` 좌표 인덱싱 캐시를 지원하는 표준 Anvil(`mca`) 호환 저장소. [설정 가이드 →](docs/CONFIG.md)
- **네트워크 파이프라인**: `libdeflate` 레벨 1–12 압축·비압축성 패킷 자동 생략, **1회 압축 팬아웃(`Outbound::SharedFrame`)**·**스레드 로컬 패킷 스크래치 직렬화** 및 **`GlobalRegion` 경량 `TickPlayer`/`AddEntity` 캐시**로 다수 플레이어 브로드캐스트 및 **청크 패킷(`level_chunk_with_light`) 스트리밍 중복 압축 제거**, `4 KiB` 이상 배치 블로킹 풀 오프로드(128연결 p99 `184.7 ms → 35.5 ms`), IP별 연결 토큰 버킷·채팅/명령어 속도 제한·최대 연결 수 제한·30초 keepalive/configuration 타임아웃·세션 인증 HTTP 타임아웃(5초/10초) 내장. [오프로드 설계 →](docs/NET-WRITER-OFFLOAD.md)

### 🎮 바닐라 26.3 게임플레이 & 최적화 (`ferrite-entity`, `ferrite-command`, `ferrite-net`)
62종 바닐라 몹(Piglin Hoglin 사냥·댄스, Witch 레이더 치유, Goat 뿔 부러짐 드롭, Fox 은신 예외/열매 채집, Killer Bunny/당근 서리, Mooshroom 번개 변이, Hard 거미 스폰 포션 등)·디스펜서/피스톤 골렘 조립·생존/전투·번식·주민 거래·떠돌이 상인·레드스톤/호퍼/피스톤·양조/비컨·네더/엔드 드래곤 전투와 바닐라 94개 명령어 루트(`strict` 블록 배치, 내장 `vanilla` 데이터팩, `/test create|pos|run*`, 런타임 월드 기본 컨텍스트 포함 `91/94` 트리 일치)를 구현했습니다. 16샤드 `settings` 및 코디네이터 활동 배치, `FxMap`/`FxSet`·2x2 `ChunkReadCache`, 무락 `SharedFrame`/큐 버퍼 재사용, TLS 월드젠/선형 스플라인·팔레트 재사용 및 `raycast_shapes`/`no_collision` 최적화를 적용했으며, 공식 26.3 코덱 검증(`tools/wirecheck`)에서 라이브 캡처 **`ok=543 bad=0`** 및 62종 전 몹 메타데이터 **`ok=144 bad=0`** 통과를 확인했습니다. [명령어 현황 →](docs/COMMANDS-STATUS.md)

---

## 🚀 빠른 시작

필요: **Rust stable** (`rust-toolchain.toml`은 Linux GNU 고정, Windows/macOS는 `RUSTUP_TOOLCHAIN=stable`), **Python 3**, 최초 1회 바닐라 데이터 추출용 **Java 25**. (Windows GUI 실행 시 Edge WebView2 Runtime 필요)

```sh
source tools/env.sh                        # Windows Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # 공식 26.3 데이터 로컬 추출 (최초 1회)
cargo run --release -p ferrite-server -- --headless --storage fworld --lighting-pool
```

처음 실행하면 `eula.txt`를 생성하고 종료합니다. [Minecraft EULA](https://aka.ms/MinecraftEULA)를 읽고 동의하는 경우 직접 `eula=true`로 수정한 뒤 다시 실행하세요.  
Windows에서는 인자 없이 `ferrite-server.exe`를 실행하면 **Tauri 2 GUI** 콘솔이 열리며, `--headless` 또는 Linux 환경에서는 표준 입출력 콘솔로 동작합니다. [콘솔/GUI 안내 →](docs/windows-console.md)

### 테스트 및 검증

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

---

## 📊 구현 상태

| 구분 | 내용 |
|---|---|
| ✅ **동작 (봇·자동 테스트 검증)** | 로그인·암호화·서명 채팅·Proxy 포워딩(Velocity/Bungee), 리전 멀티스레딩, `.fworld`/Anvil IO·병렬 라이팅·순수 Rust 월드젠, 인벤토리·제작·컨테이너·주민 거래·떠돌이 상인, 62종 몹/AI·번식·생존, 레드스톤·호퍼·피스톤·디스펜서/명령 블록·양조·비컨, 네더·엔드 포탈·드래곤 전투/게이트웨이, 26.3 플레이 패킷(게임모드 전환·픽 블록/엔티티·책 편집·직소·클라이언트 정보 등) 및 94개 명령 루트([명령어 현황](docs/COMMANDS-STATUS.md)) |
| 🟡 **부분 구현** | 피스톤 애니메이션·드래곤 돌진/화염구 근사([`docs/pistons.md`](docs/pistons.md), [`docs/end-dimension.md`](docs/end-dimension.md)), 일부 몹 세부 골 패리티([`docs/mob-ai-parity.md`](docs/mob-ai-parity.md)) |
| ⛔ **미완료** | **실제 Minecraft 26.3 클라이언트 화면/UI 수동 검증** ([`docs/manual-checklist.md`](docs/manual-checklist.md)) — 모든 자동 검증은 프로토콜 봇·코덱 대조 기준입니다. |

남은 작업과 로드맵은 [`docs/BACKLOG.md`](docs/BACKLOG.md)에서 단일 관리합니다. *(바닐라 전용 범위이며 Bukkit/Paper/Spigot 플러그인 API 호환성은 지원하지 않습니다.)*

---

## 📦 크레이트 구조

| 크레이트 | 역할 |
|---|---|
| `ferrite-protocol`, `ferrite-net` | 패킷 코덱(26.3/777), TCP 프레이밍, 암호화, 압축·배치 오프로드, 속도 제한 |
| `ferrite-region` | Folia 스타일 리전라이저 및 리전 스케줄러 |
| `ferrite-world`, `ferrite-light`, `ferrite-worldgen`, `ferrite-nbt` | 청크·`.fworld`/Anvil IO, ScalableLux 병렬 라이팅 풀, 순수 Rust 월드젠, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | 엔티티·몹 AI, 유체, 메뉴·아이템·레시피·전리품 |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | 명령 디스패처, 게임룰·권한, `/tps`·`/mspt`·`/profile` |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | 바닐라 데이터 테이블, 텍스트 컴포넌트, 수학, 설정, 콘솔 |
| `ferrite-server` | 서버 바이너리: CLI/Tauri GUI, 설정, 월드 루프 배선 |

---

## ⚖️ 법적 고지 및 라이선스

- 이 저장소는 **Mojang의 코드나 에셋을 배포하지 않습니다.** 바닐라 데이터는 사용자 환경에서 공식 서버 jar로부터 로컬 추출(`vanilla/`, gitignore)됩니다. Minecraft는 Mojang Studios의 상표이며, 이 프로젝트는 Mojang·Microsoft와 무관한 비공식 프로젝트입니다.
- **라이선스**: **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). Paper·Folia·Lithium 등 GPL/LGPL 기반 최적화 설계 및 코드를 참고/포팅하기 위해 GPLv3를 채택했으며, 출처는 [`NOTICE.md`](NOTICE.md)에 명시되어 있습니다.
