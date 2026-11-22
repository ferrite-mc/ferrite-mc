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

## ⚡ 벤치마크: Ferrite vs Folia · Paper · Purpur · Leaf

> **동일 조건 공정 비교**: 동일 호스트(12-vCPU Xeon Platinum 8358, Linux 7.0, Java 25 `-Xms1G -Xmx4G`)에서 한 번에 한 서버씩 **단독 순차 실행**하며, 모두 **빈 `seed=0` 월드**에서 실시간 지형·라이팅 생성 + **50~400봇 (`BOTS_STILL=1`)** 및 **사전 생성 월드(`fixture-world`)** 를 비교 측정했습니다.

<p align="center">
  <img src="docs/perf/2026-11-21/server-comparison.svg" alt="Ferrite vs Folia, Leaf, Paper, Purpur 26.3 Benchmark" width="100%">
</p>

### 1) 동일 조건 실시간 월드젠 + 50~400봇 스케일링 & 시야 거리 비교 (`fresh seed=0`)

| 측정 항목 | Ferrite (`.fworld`) | Ferrite (`anvil`) | Folia 26.3 | Leaf 26.3 | Paper 26.3 | Purpur 26.3 |
|---|---:|---:|---:|---:|---:|---:|
| **50봇 평균 MSPT** *(p99 / TPS)* | **0.35 ms** *(0.40 / 20.0)* | **0.35 ms** *(0.39 / 20.0)* | 12.56 ms *(19.5)* | 6.80 ms *(20.0)* | 5.87 ms *(18.5)* | 5.00 ms *(20.0)* |
| **100봇 평균 MSPT** *(p99 / TPS)* | **0.42 ms** *(0.57 / 20.0)* | **0.45 ms** *(0.60 / 20.0)* | 19.35 ms *(18.9)* | 14.73 ms *(19.6)* | 11.63 ms *(16.8)* | 15.97 ms *(19.3)* |
| **200봇 평균 MSPT** *(p99 / TPS)* | **1.20 ms** *(1.57 / 20.0)* | **1.25 ms** *(1.93 / 20.0)* | 24.29 ms *(19.1)* | 30.70 ms *(17.5)* | 28.63 ms *(13.9)* | 23.67 ms *(18.3)* |
| **200봇 접속 완료 `p50` / `p99`** | **44.5 / 52.8 ms** | **44.5 / 52.4 ms** | 9,500 / 15,926 ms | 5,131 / 8,797 ms | 4,596 / 8,573 ms | 4,292 / 7,754 ms |
| **400봇 평균 MSPT** *(목표 ≤ 50 ms)* | **4.92 ms** *(7.37 / 20.0)* | **4.43 ms** *(6.57 / 20.0)* | 31.58 ms† *(298/400 드롭)* | 82.67 ms *(13.6 TPS)* | 80.23 ms *(9.8 TPS)* | 92.80 ms *(13.4 TPS)* |
| **400봇 접속 완료 `p50` / `p99`** | **44.9 / 59.0 ms** | **45.0 / 59.8 ms** | *로그인 폭주 중 연결 끊김†* | 7,796 / 12,972 ms | 7,205 / 12,742 ms | 8,369 / 12,847 ms |
| **청크 스트리밍 (`view=6 / 10 / 16`)** | **153.5 / 154.7 / 138.4/s** | **153.6 / 157.5 / 136.2/s** | 62.6 / 63.9 / 70.7/s | 52.8 / 66.7 / 71.1/s | 56.1 / 67.0 / 69.1/s | 57.8 / 67.8 / 72.2/s |
| **콜드 부트 (리스너 준비)** | **0.004 s** | **0.004–0.005 s** | 6.98–7.13 s | 7.28–7.63 s | 6.98–7.18 s | 7.24–7.48 s |
| **초기 / 200봇 피크 `VmRSS`** | **192.9 / 891.2 MiB** | **195.3 / 849.1 MiB** | 1,383.8 / 1,592.2 MiB | 1,298.0 / 1,640.8 MiB | 1,395.9 / 1,682.5 MiB | 1,392.6 / 1,664.8 MiB |

<sub>† Folia 26.3은 400봇 5 ms 간격 동시 접속 폭주 중 298명 시점(`31.58 ms` / `18.98 TPS` / `464.9%` CPU)에서 일부 클라이언트 연결을 종료(`server closed the connection`)했습니다. Ferrite·Leaf·Paper·Purpur는 400/400 전원 완주.</sub>

- **400명 동시 진입·월드젠에서도 4.4–4.9 ms MSPT & 45 ms 조인 유지**: 코디네이터 리비전 게이팅, 무락 TLS 프레이밍, 16샤드 `ChunkCache`·전용 월드젠 워커 풀, `Tracker`/`Chunk` 무할당 직렬화, 4-way direct-mapped `EvalCache`·분기 없는 `GRADS` LUT, 비트팩 `u64` 라이팅 큐 등 20개 핫패스 최적화를 통해 Ferrite는 **200봇 `1.20 ms` MSPT (`54.9%` CPU, 피크 `891.2 MiB`)**, **400봇 `4.43–4.92 ms` MSPT (`109.5%–120.7%` CPU, `44.9–45.0 ms` 조인 p50, 피크 `948–994 MiB` RSS)** 를 기록해 Paper(`80.23 ms`), Leaf(`82.67 ms`), Purpur(`92.80 ms`) 대비 **16.3x~20.9x** 낮은 틱 시간을 달성했습니다.
- **기생성 월드(`fixture-world`, 월드젠 없음) 로드 시**: Ferrite(`anvil`)는 **`962.4 chunks/s`** 청크 스트리밍(`441`청크 `0.5 s`)과 200봇 **평균 `1.19 ms` MSPT (`56.0%` CPU, `974.2 MiB` 피크)** 를 기록했습니다(동일 `fixture-world`에서 Folia `65.3/s`·`25.54 ms`, Leaf `64.3/s`·`101.57 ms`, Purpur `64.9/s`·`113.00 ms`, Paper `64.9/s`·`113.20 ms`).
- **단독 바이너리 생 야생(`seed=0`, `view=10`, 200봇) 재측정**: `ferrite-server` 단독 실행에서도 1분 평균 **`3.55 ms` MSPT** (정상화 후 10초 `3.50 ms`, `p50=54.8 ms` 조인, 피크 `1,309.3 MiB`)를 유지한 반면, 동일 조건에서 Paper `84.6 ms` (`9.3 TPS`), Leaf `53.2 ms` (`13.1 TPS`), Purpur `52.8 ms` (`13.1 TPS`), Folia `21.87 ms` (`20.0 TPS`)를 기록했습니다.
- **800봇 스트레스 테스트 (`examples/bots 800`, `--lighting-pool`)**: `800/800` 동시 접속·이동 완료 — `fixture` 기준 **평균 `10.42 ms` MSPT** (`p99 33.64 ms`, `1,620 MiB` RSS, `47.30 MiB/s`).

<p align="center">
  <img src="docs/perf/2026-11-21/concurrency-and-view-scaling.svg" alt="Concurrency and View Distance Scaling" width="100%">
</p>
<p align="center">
  <img src="docs/perf/2026-11-21/ferrite-scaling.svg" alt="Ferrite Subsystem Scaling" width="100%">
</p>

<sub>전체 벤치마크 방법론·원시 수치 및 피크 RSS(`mimalloc` + 16 MiB 버스트 큐)·CPU 해석은 [상세 벤치마크 리포트](docs/perf/2026-11-21/server-comparison.md)와 [성능 베이스라인](docs/perf/README.md)에 공개되어 있습니다.</sub>

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
- **네트워크 파이프라인**: `libdeflate` 레벨 1–12 압축·비압축성 패킷 자동 생략, **1회 압축 팬아웃(`Outbound::SharedFrame`)**·**스레드 로컬 패킷 스크래치 직렬화** 및 **`GlobalRegion` 경량 `TickPlayer`/`AddEntity` 캐시**로 다수 플레이어 브로드캐스트 및 **청크 패킷(`level_chunk_with_light`) 스트리밍 중복 압축 제거**, `4 KiB` 이상 배치 블로킹 풀 오프로드(128연결 p99 `184.7 ms → 35.5 ms`), IP별 연결 토큰 버킷·채팅/명령어 속도 제한·최대 연결 수 제한 내장. [오프로드 설계 →](docs/NET-WRITER-OFFLOAD.md)

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
