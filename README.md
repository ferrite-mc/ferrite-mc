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
  <a href="#-벤치마크-ferrite-vs-paper-vs-vanilla">벤치마크</a> ·
  <a href="#-핵심-기능">핵심 기능</a> ·
  <a href="#-빠른-시작">빠른 시작</a> ·
  <a href="#-구현-상태">구현 상태</a> ·
  <a href="#-크레이트-구조">크레이트</a> ·
  <a href="docs/CONFIG.md">설정 가이드</a> ·
  <a href="README.en.md">English</a>
</p>

</div>

---

## ⚡ 벤치마크: Ferrite vs Paper vs Vanilla

> **동일 조건 공정 비교**: 동일 호스트(12-vCPU Xeon Platinum 8358, Linux 7.0, Java 25 `-Xms1G -Xmx4G`)에서 한 번에 한 서버씩 **단독 순차 실행**하며, 모두 **빈 `seed=0` 월드**에서 실시간 지형·라이팅 생성 + **200봇 전원 완주(`BOTS_STILL=1`)** 를 측정했습니다.

<p align="center">
  <img src="docs/perf/2026-11-21/server-comparison.svg" alt="Ferrite vs Paper 26.3 vs Vanilla 26.3 Benchmark" width="100%">
</p>

### 1) 동일 조건 실시간 월드젠 + 200봇 (`fresh seed=0`)

| 측정 항목 | Ferrite (`.fworld` RAM) | Ferrite (`memory`) | Ferrite (`anvil`) | Paper 26.3 *(무플러그인)* | Vanilla 26.3 *(Mojang)* |
|---|---:|---:|---:|---:|---:|
| **200봇 평균 MSPT** *(목표 ≤ 50 ms)* | **5.84 ms** *(p99 16.77 ms)* | **5.42 ms** *(p99 14.95 ms)* | **5.53 ms** *(p99 13.49 ms)* | 27.47 ms *(11.2–16.0 TPS)* | 18.33 ms *(p99 110.43 ms)* |
| **초기 청크 스트리밍 (`view=10`)** | **100.1 chunks/s** *(2.9 s)* | **102.9 chunks/s** *(2.8 s)* | **100.9 chunks/s** *(2.9 s)* | 64.0 chunks/s *(6.3 s)* | 58.1 chunks/s *(5.7 s)* |
| **200봇 접속 완료 `p99`** | **4,443 ms** | **4,431 ms** | **4,549 ms** | 8,134 ms | 5,596 ms |
| **콜드 부트 (리스너 준비)** | **0.004 s** | **0.004 s** | **0.004 s** | 7.08 s | 6.48 s |
| **유휴 메모리 (`0 bots`, `VmRSS`)** | **40.7 MiB** | **39.7 MiB** | **39.8 MiB** | 1,387.0 MiB | 1,026.7 MiB |
| **200봇 평균 / 피크 `VmRSS`** | 1,393 / 2,130 MiB | 1,439 / 2,177 MiB | 1,449 / 2,334 MiB | 1,636 / 1,687 MiB | **1,391 / 1,476 MiB** |
| **200봇 평균 CPU (12 vCPU)** | 590.9% | 614.6% | 618.3% | 297.7% | 356.9% |

- **멀티코어 월드젠 + 안정적 20 TPS**: 200명이 동시에 빈 월드로 진입할 때 Ferrite는 12코어 병렬 월드젠과 `--lighting-pool`을 활용해 Paper(`8,134 ms`, TPS `11.2`까지 하락)보다 **약 1.8배 빠르게 200명을 접속(`4,431–4,549 ms`)** 시키면서 **5.42–5.84 ms MSPT(20.0 TPS)** 를 유지했습니다.
- **기생성 월드(`fixture-world`, 월드젠 없음) 로드 시**: Ferrite(Anvil)는 **`690.8 chunks/s`** 청크 스트리밍과 200봇 **평균 `2.21 ms` MSPT (`120.5%` CPU)** 를 기록했습니다(동일 `fixture-world`에서 Paper `64.2/s`·`147.1 ms`, Vanilla `88.8/s`·`110.7 ms`).
- **800봇 스트레스 테스트 (`examples/bots 800`, `--lighting-pool`)**: `800/800` 동시 접속·이동 완료 — `fixture` 기준 **평균 `10.42 ms` MSPT** (`p99 33.64 ms`, `1,620 MiB` RSS, `47.30 MiB/s`).

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

### 💾 `.fworld` RAM-Blob & 다중 스토리지 / 고속 네트워크 (`ferrite-world`, `ferrite-net`)
- **3가지 저장 모드**:
  - `--storage memory` (기본값): 디스크 저장 없는 순수 인메모리 월드.
  - `--storage fworld` (`.fworld` / `FWRD` v2): 디스크 지연 없이 시작 시 전체 월드를 RAM에 올려 두고 메모리에서 즉시 청크를 읽고 쓰며, 저장 시 `xxh32` 무결성 체크섬·사전 압축 해제 크기 상한 검증·clean-skip 더티 추적을 적용해 원자적으로 저장합니다(64청크 LZ4 저장 `19.05 ms` — 직렬 Anvil `68.59 ms` 대비 **3.60x**, 콜드 로드 `7.22 ms`).
  - `--storage anvil`: 최대 8-리전 병렬 플러시(`3.18x`)를 지원하는 표준 Anvil(`mca`) 호환 저장소. [설정 가이드 →](docs/CONFIG.md)
- **네트워크 파이프라인**: `libdeflate` 레벨 1–12 압축·비압축성 패킷 자동 생략, `4 KiB` 이상 배치 블로킹 풀 오프로드(128연결 p99 `184.7 ms → 35.5 ms`), IP별 연결 토큰 버킷·채팅/명령어 속도 제한·최대 연결 수 제한 내장. [오프로드 설계 →](docs/NET-WRITER-OFFLOAD.md)

### 🎮 바닐라 26.3 게임플레이 & 명령어 (`ferrite-entity`, `ferrite-command`)
41종 몹 AI·생존·전투·번식·주민 거래·떠돌이 상인·레드스톤/호퍼/피스톤·양조/비컨·네더/엔드 드래곤 전투와 바닐라 94개 명령어 루트(`91/94` 트리 완전 일치)를 구현했습니다. 공식 26.3 코덱 검증 도구(`tools/wirecheck`)에서 **2,908/2,908** 통과를 확인했습니다. [명령어 현황 →](docs/COMMANDS-STATUS.md)

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
| ✅ **동작 (봇·자동 테스트 검증)** | 로그인·암호화·서명 채팅·Proxy 포워딩(Velocity/Bungee), 리전 멀티스레딩, Memory/Anvil/`.fworld` IO·병렬 라이팅·순수 Rust 월드젠, 인벤토리·제작·컨테이너·주민 거래·떠돌이 상인, 41종 몹 AI·전투·번식, 레드스톤·호퍼·명령 블록·양조·비컨, 네더·엔드 포탈·드래곤 전투/게이트웨이, 94개 명령 루트([명령어 현황](docs/COMMANDS-STATUS.md)) |
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
