# Lane 1 — JVM 호스트·로더·생명주기 지시서

- 레인: `lane/plugin-b-host`
- 선행: 없음. 이 레인이 계약과 Java Gradle 스캐폴드를 먼저 만든다.
- 관련 인덱스: [plugin-b-3lane-2026-11-16.md](plugin-b-3lane-2026-11-16.md)

## 목표

플러그인 jar를 발견하고, **프로세스당 하나뿐인 임베디드 JVM**을 시작해 Java 호스트를
띄우고, 플러그인 클래스로더와 `onLoad`/`onEnable`/`onDisable` 생명주기를 소유한다.
Bukkit/Paper API의 의미론과 NMS 래퍼는 이 레인이 소유하지 않는다.

## 먼저 읽을 것

- `AGENTS.md` — 금지 사항, 빌드, Mojang 코드/에셋 규칙.
- `crates/ferrite-jvm/src/lib.rs` — `Jvm::start`, `Jvm::call_static`, `Jvm::register_natives`,
  `Jvm::attach_thread`, `Jvm::with_env`, `JvmConfig`, `JvmThreadMode`.
- `crates/ferrite-plugin-meta/src/lib.rs` — `scan_directory`, `LoadPlan::{startup,postworld}`,
  `LoadedPlugin`, `ThreadingMode`.
- `crates/ferrite-server/src/game/vanilla_gen.rs` — 현재 worldgen은 외부 Java 프로세스다.
  임베디드 JVM은 플러그인 호스트만 소유한다.
- `crates/ferrite-server/src/main.rs` — 새 호스트를 연결할 서버 시작/종료 지점.

## 산출물

### 1. 호스트 계약 — `docs/plugin-b-host-contract.md`

Lane 2/3이 코딩을 시작하기 전에 아래를 확정한다.

- 계약 버전(정수)과 호환 규칙.
- Java → Rust op 이름, 요청/응답 스키마, 타임아웃, 오류 코드.
- Rust → Java 콜백(이벤트, 스케줄러, 로그, 종료) 이름과 payload.
- 동기/비동기 경계: 어떤 op가 즉시 답하고, 어떤 op가 소유 리전으로 라우팅되며, 어느
  스레드에서 완료되는지.
- 이벤트 이름과 payload: 최소 `player_join`, `player_quit`, `block_break`,
  `player_interact`. Bukkit 이벤트 클래스는 Lane 2가 소유한다.
- 스케줄러 op: sync/delayed/repeating/async, 취소 토큰, 플러그인 disable 시 정리 규칙.
- 플러그인 메타데이터 전달 형식: 이름, 버전, main class, dependencies, load phase,
  commands/permissions(원문 또는 구조화된 값), data folder.
- op 추가/변경은 계약 버전을 올리고 Lane 2/3 핸드오프에 알린다.

계약은 “구현하면서 나중에 정한다”가 아니라 **첫 커밋에서 고정**한다. op 이름을 바꿀 때
구버전 이름을 조용히 다른 의미로 재사용하지 않는다.

### 2. Rust 호스트 crate — `crates/ferrite-plugin-host`

- `PluginHostConfig`: `enabled`, `plugins_dir`, `data_dir`, `java_home`, `max_heap_mb`,
  `jvm_args`, `api_version`. 기본값은 `enabled = false`; 플러그인이 없으면 JVM을 시작하지 않는다.
- `PluginHost::start`:
  1. `ferrite_plugin_meta::scan_directory`로 `LoadPlan`을 만든다.
  2. `startup`을 먼저, `postworld`를 나중에 로드한다. 실패한 jar는 서버 전체를 죽이지 않고
     플러그인별 오류로 보고한다. 중복 이름은 `ferrite-plugin-meta`의 결정을 따른다.
  3. `Jvm::start(JvmConfig { classpath: [host jar], .. })`를 **한 번만** 호출한다.
     `Jvm::get()`이 이미 있으면 새 JVM을 만들지 않는다.
  4. Java `Bootstrap`에 플러그인 목록/데이터 디렉터리/API 버전을 넘기고, Java 호스트가
     `onLoad`/`onEnable`을 호출하게 한다.
- Java 호스트 jar의 classpath에는 **호스트/API 런타임 jar만** 넣는다. 플러그인 jar를
  시스템 classpath에 넣어 플러그인 간 클래스가 새지 않게 한다.
- 플러그인별 `URLClassLoader`는 Java 호스트가 소유한다. 부모는 호스트/API 클래스로더,
  자식은 해당 플러그인 jar다. `onDisable` 뒤 classloader를 닫고, JVM이 클래스를 완전히
  언로드하지 못한다는 한계를 문서화한다.
- 플러그인 데이터 디렉터리는 `plugins/<plugin-name>/`; 없으면 만든다.
- 로그는 `tracing`과 Java logging을 연결한다. 최소한 플러그인 이름이 붙은 stderr/stdout
  캡처와 `onEnable` 예외 스택을 남긴다.
- Java 호스트의 예외/시간 초과/프로세스 종료를 Rust 오류로 변환한다. 플러그인 하나의
  실패가 다른 플러그인이나 서버 tick을 멈추지 않게 한다.
- 공개 API는 `PluginHost::{start, shutdown, fire_event, schedule, call}` 정도로 좁게
  유지한다. 게임 루프가 Java 내부 객체를 직접 들고 있지 않게 한다.

### 3. Java 호스트 모듈 — `java/ferrite-host`

- Lane 1이 `java/settings.gradle.kts`와 `java/ferrite-host` 스캐폴드를 만든다.
  `ferrite-api-impl`과 `ferrite-nms`는 빈 모듈/디렉터리만 만들고, 각 레인이 채운다.
- `dev.ferrite.host.Bootstrap`:
  - Rust가 넘긴 플러그인 목록을 받아 classloader를 만들고 `JavaPlugin` 구현을 로드한다.
  - `onLoad` → `onEnable` → (서버 실행) → `onDisable` 순서를 지킨다.
  - 플러그인 `main` 클래스가 없거나 `JavaPlugin`이 아니면 해당 플러그인만 실패시킨다.
  - `HostBridge` native/client를 통해 Rust op를 호출한다.
- `dev.ferrite.host.HostApi`/SPI는 Lane 2가 구현한다. Lane 1은 이 인터페이스의 소유자이며,
  Lane 2가 구현할 수 있도록 최소 메서드만 둔다.
- 실제 Bukkit `Server`/`World`/`Player` 구현을 이 모듈에 넣지 않는다. Lane 2의 facade를
  로드하는 역할만 한다.
- `getLogger`, `getDataFolder`, plugin.yml/paper-plugin.yml 메타데이터, dependency/load
  order는 호스트가 제공한다. 명령/이벤트/스케줄러의 Bukkit 의미론은 Lane 2가 제공한다.

### 4. 서버 배선

- `ferrite-server`에 `plugin_host` 모듈을 추가하고, 기존 `main.rs`의 서버 시작/종료에
  `enabled`일 때만 호스트를 연결한다.
- `plugins.enabled=false`일 때 JVM, Java 호스트, 플러그인 디렉터리 접근이 전혀 없어야 한다.
- 시작 순서: 월드/리전 초기화 전 `startup` 플러그인 로드, 월드 준비 후 `postworld` 로드.
  이 순서가 서버 tick을 막지 않도록 JVM 시작과 plugin enable은 시작 단계에서 완료한다.
- 종료 순서: 네트워크/월드 종료 전에 플러그인 `onDisable`, 스케줄러 취소, host shutdown.
  플러그인 종료가 실패해도 서버 저장/종료는 계속된다.
- 이벤트 소스는 게임 루프의 최소 훅에서 `HostBridge.fire_event`로 전달한다. 이벤트
  취소 결과가 필요한 블록 파괴/상호작용은 **소유 리전 스레드에서 동기 대기하지 않는다**.
  계약에 정한 비동기/즉시 경로를 사용하고, 결과가 늦으면 기본 동작을 적용한 뒤 로그를 남긴다.
- `game/mod.rs`, `game/region.rs` 수정은 이벤트/스케줄러 배선에 필요한 최소 hunk로 제한한다.
  대규모 포맷팅이나 플러그인 타입 재도입을 하지 않는다.

### 5. 테스트

- `crates/ferrite-plugin-host` 단위 테스트:
  - `enabled=false`에서 JVM이 시작되지 않는다.
  - 두 번 `PluginHost::start`를 호출해도 JVM을 두 개 만들지 않는다.
  - 잘못된 jar/메타데이터/의존성 누락이 서버를 죽이지 않고 `failed`로 보고된다.
  - `startup`/`postworld` 순서와 중복 이름 결정이 `ferrite-plugin-meta`와 일치한다.
  - Java 호출 타임아웃과 예외가 Rust 오류로 변환된다.
- JVM smoke: `java/ferrite-host`의 static 테스트 클래스로 `Jvm::call_static`과 native
  콜백을 왕복한다. 실제 Bukkit API가 없어도 된다.
- 통합(가능해진 뒤): Lane 2의 typical fixture jar를 로드해 `onEnable` 로그, join 이벤트,
  플러그인 명령, sync/async 스케줄, `onDisable`을 확인한다. 이 테스트는 Lane 2 병합 뒤
  integration 브랜치에서 skip 없이 실행한다.
- 검증 명령: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test -p ferrite-plugin-host`, 관련 `cargo test -p ferrite-server plugin_host`,
  `cd java && ./gradlew :ferrite-host:test`.

## 비목표

- `Player`, `World`, `ItemStack` 등의 Bukkit 메서드 구현.
- NMS/CraftBukkit 클래스.
- 전체 Paper lifecycle event, 플러그인 명령 tree, 채팅 이벤트.
- 실제 Minecraft 클라이언트 검증.
- worldgen을 임베디드 JVM으로 이전하는 작업.

## 핸드오프

`docs/handoff/plugin-b-lane-1-host.md`에 브랜치/커밋, 계약 버전, 변경 파일, 공유 파일 hunk,
정확한 테스트 결과, JVM 하나 보장 방식, 남은 limitation(클래스 언로드, 이벤트 지연 등)을
적는다. Lane 2/3이 바로 시작할 수 있도록 `docs/plugin-b-host-contract.md` 링크를 남긴다.

## 핸드오프 (2026-11-16)

- 브랜치: `yxshadxw/plugin-b-host` (지시서의 `lane/plugin-b-host`에 해당)
- 커밋: **없음**. 모든 변경은 작업 트리에 uncommitted 상태로 남아 있다. 푸시하지 않았다.
- 계약: [`docs/plugin-b-host-contract.md`](../plugin-b-host-contract.md) **v2** (2026-11-16 동결; v1 하위호환)
- 범위: Lane 1만. Bukkit/Paper facade와 NMS 래퍼는 구현하지 않았고, 삭제된
  `java/ferrite-api-impl`/`java/test-plugin*`도 복원하지 않았다.

### 변경 파일

- `docs/plugin-b-host-contract.md` — JNI 전송, 생명주기, 4개 이벤트, 스케줄러, 오류,
  Lane 2 SPI, classpath/API 구현 설정. 계약 v1.
- `crates/ferrite-plugin-host/` — `Cargo.toml`, `src/{lib,config,error,protocol,scheduler,
  native,host}.rs`, `tests/jvm_smoke.rs`. `PluginHost` singleton, 전용 host 스레드,
  Rust 소유 스케줄러, JNI native 브리지, 플러그인 스캔/로드/종료.
- `crates/ferrite-server/src/plugin_host/{mod.rs,hooks.rs}` — `PluginsSettings`,
  `RunningHost`, `HostHooks` 어댑터.
- `crates/ferrite-server/tests/plugin_host_hooks.rs` — 실제 Java host를 통한 `HostHooks`
  왕복 smoke.
- `crates/ferrite-server/{Cargo.toml,src/lib.rs,src/main.rs}` — 의존성, 모듈 export,
  `[plugins]` 설정, 시작/종료/postworld 배선.
- `java/settings.gradle.kts`, `java/build.gradle.kts`, `java/ferrite-host/**`,
  `java/gradle/wrapper/**`, `java/gradlew*` — Lane 1 Gradle 스캐폴드와 Java 호스트/테스트.
- `java/ferrite-api-impl/.gitkeep`, `java/ferrite-nms/.gitkeep` — Lane 2/3용 빈 모듈.
- `ferrite.toml.example` — `[plugins]` 예시를 파일 끝으로 옮기고 classpath/API 필드 추가.
- `Cargo.lock` — 새 crate 의존성 반영.

공유 파일 hunk는 `ferrite-server/src/main.rs`의 `Config`/`DEFAULT_CONFIG`/시작·종료 배선과
`src/lib.rs`의 모듈 export, `Cargo.toml`의 의존성뿐이다. `game/mod.rs`, `game/region.rs`는
수정하지 않았다. 기존 `ServerHooks` 호출 지점(`game/mod.rs`의 `player_joined`/`player_left`/
`tick`/`run_plugin_command`, `game/region.rs`의 `block_broken`/`player_interacted`)을 그대로
사용한다.

### JVM 하나 보장

- `PluginHost::start`는 `START_LOCK` 아래에서 `HOST: OnceLock<PluginHost>`를 확인한다. 두 번째
  호출은 `HostError::AlreadyStarted`이며 JVM을 만들지 않는다.
- `Jvm::get()`이 이미 있으면 새 JVM을 만들지 않고 classpath에 host jar/runtime jar가 모두
  포함됐는지 확인한다. 다르면 `JvmAlreadyRunning`이다.
- `jvm_smoke`가 첫 host 시작 뒤 두 번째 `PluginHost::start`를 호출해 `AlreadyStarted`를
  검증한다. 플러그인 디렉터리가 없거나 비어 있으면 JVM 없이 dormant host를 돌려준다.

### classpath와 API 구현

- `host_classpath()`가 host jar를 **항상 첫 번째**로 두고 `runtime-classpath`를 나열 순서대로
  붙인다. 단위 테스트
  `classpath_puts_the_host_jar_first_and_keeps_runtime_order`가 이를 고정한다.
- `api-impl-class`는 `host-jar` 또는 `runtime-classpath`에서 로드한다. 빈 문자열/생략은
  `NoopHostApi`다. `plugin_host_hooks` smoke는 SPI 구현을 별도 `api-classes` 디렉터리에
  컴파일해 `runtime_classpath`로 넘기고 실제로 로드되는지 확인한다.
- `java/settings.gradle.kts`는 PaperMC Maven 저장소를 포함한다. `:fixtures:typical-plugin`,
  `:fixtures:nms-plugin`은 디렉터리가 존재할 때만 include하므로 fixture가 없는 Lane 1 단독
  체크아웃에서도 Gradle 설정이 깨지지 않는다.

### v1 검증 결과

```sh
cargo fmt --all -- --check
# exit 0

cargo clippy --workspace --all-targets -- -D warnings
# exit 0 (기존 workspace manifest의 unused dependency 경고만 남음)

cargo test -p ferrite-plugin-host
# 9 unit + 1 jvm_smoke + 1 doctest, all passed

cargo test -p ferrite-server plugin_host
# 2 plugin_host unit + 3 main config tests + 1 plugin_host_hooks integration,
# all passed

cargo test --workspace
# exit 0 (server lib 포함 전체 통과)

cd java && ./gradlew clean :ferrite-host:test --no-daemon
# BUILD SUCCESSFUL; 13 Java tests (5+3+4+1), 0 failures

git diff --check
# exit 0
```

- 최초 `cargo test --workspace`는 로컬 `vanilla/26.3/fixture-world`가 없어 기존 piston/redstone
  통합 테스트 10개가 실패했다. `python tools/fetch_vanilla.py`와
  `python tools/make_fixture_world.py`로 gitignored fixture를 생성한 뒤 재실행해 통과했다.
  테스트를 skip/약화하지 않았다.
- 주요 검증 전 `cargo clean -p ferrite-plugin-host -p ferrite-server`와 Gradle `clean`을
  실행했다.

### 남은 blocker/limitation

- Lane 2/3의 `java/fixtures/**`가 아직 없다. 따라서 `:fixtures:typical-plugin`과
  `:fixtures:nms-plugin`은 현재 include되지 않고, 실제 typical/NMS fixture를 같은 JVM에
  로드하는 통합 테스트는 Lane 2/3 산출물이 생긴 뒤에만 가능하다. `ferrite-api-impl`과
  `ferrite-nms`는 여전히 빈 스캐폴드다.
- 계약 v1은 `player_join`, `player_quit`, `block_break`, `player_interact`만 지원한다.
  `HostHooks`의 `chat`은 통과, `player_moved`/`block_placed`는 허용, `player_command`는
  미처리(false)다. 이벤트/명령 tree의 Bukkit 의미론은 Lane 2 소유다.
- 블록 파괴/상호작용은 리전 스레드에서 Java를 기다리지 않고 다음 tick에 전달한다. 따라서
  리스너의 취소는 이미 적용된 동작을 되돌리지 못하고 경고만 남긴다. 지연 큐는 4096개로
  제한되어 넘치면 가장 오래된 이벤트를 버린다.
- 콘솔 명령 메시지는 플레이어 UUID가 없어 `Uuid::nil()`로 전달한다. 게임의 콘솔 경로는
  UUID를 비교하지 않고 모든 메시지를 로그하므로 출력은 유지된다.
- JVM은 프로세스당 하나이며 재시작/플러그인 재로드를 지원하지 않는다. classloader를 닫아도
  JVM이 클래스를 완전히 언로드하지 않는다. host jar를 먼저 빌드해야 하고 Java 25+가 필요하다.
- `api_impl_class`가 classpath에 없거나 계약 버전이 다르면 host 시작이 실패한다(서버는
  플러그인 없이 계속). 실제 Minecraft 클라이언트 렌더링 검증은 하지 않았다.
- world 이름은 현재 `minecraft:overworld`로 고정되어 있다(계약 v1 어댑터의 한계).

Lane 2/3은 [`docs/plugin-b-host-contract.md`](../plugin-b-host-contract.md)와 위 SPI/클래스패스
규칙에서 바로 시작할 수 있다.

## v2 수직 슬라이스 (2026-11-16, Lane 1)

- 계약: `CONTRACT_VERSION = 2` (Rust/Java 모두). v1 op/이벤트/필드는 유지했다.
- Rust:
  - `BlockGetRequest`, `BlockSetRequest`, `BlockSnapshot`, `BlockSetAccepted`, `UuidRequest`와
    `HostStateProvider` trait를 `ferrite-plugin-host`에 추가했다.
  - `NativeState`가 `world.block_get`, `world.block_set_async`, `world.chunk_loaded`,
    `player.state_get`, `entity.state_get`를 파싱·디스패치한다. 알 수 없는 op는 여전히
    `unknown_op`이며 성공 no-op이 아니다.
  - `host.info`/`host.ping`은 `contract_version: 2`를 돌려주고, `host.info.capabilities`에
    v1 op, **실제 provider가 광고한** v2 op, `state_provider`를 노출한다. provider가 없으면
    state op는 `state_unavailable`이다.
  - `PluginHost::install_state_provider`와 `PluginHost::tick`의 provider drain을 추가했다.
  - 기본 seam은 `HostStateProvider`다. `world.block_get`은 실제 dimension source를 읽고,
    `world.block_set_async`는 검증 후 큐에 넣기만 하며, 게임 루프의 `PluginHost::tick`이
    `compare_set_block`으로 적용한다. 접수와 적용을 같은 성공으로 표시하지 않는다.
  - `ferrite-server/src/plugin_host/state.rs`의 `WorldStateProvider`가
    `DimensionRegistry`의 실제 `AnvilChunkSource`에 연결된다. 월드 생성 뒤 `main.rs`에서
    설치하므로 POSTWORLD plugin도 provider를 본다.
- Java:
  - `Bootstrap.CONTRACT_VERSION = 2`; v2 transport helper 5개 추가.
  - `HostApi`의 v2 default 메서드는 `UnsupportedOperationException`이다(향후 in-process
    adapter용; Lane 1 native transport가 현재 경로).
  - `dev.ferrite.host.integration.HostIntegrationHarness` 추가. `start(Path,Path)`은 Java
    호스트를 띄우고 plugin.yml이 있는 jar를 스캔·로드하며, `reports()`는 jar별 load 결과를,
    `close()`는 host shutdown을 수행한다. Rust `PluginHost`를 대체하지 않는 Lane 3용 seam이다.
- 테스트(실제 실행 결과는 아래 최종 검증 참고):
  - Rust: JSON round-trip/validation 2, v2 handshake+provider dispatch 1, unknown/invalid 1,
    실제 dimension source read/write-accept/tick 2.
  - Java: `HostIntegrationHarnessTest`가 test plugin jar 하나로 start → report → close를 검증.
- 미구현/후속:
  - `player.state_get`/`entity.state_get`는 후보이며 기본 provider가 `unsupported`다.
  - v2 write의 완료 상태 조회 op(`state.request.status`에 해당하는 기능)는 아직 없다. 현재는
    `request_id` 접수와 서버 로그/실제 적용만 있다.
  - Java `HostApi` v2 default 메서드는 native transport를 호출하지 않는다. Lane 2가 이 SPI를
    실제로 소비하려면 별도 합의가 필요하다.
  - `world.block_set_async`의 `queue_full`/`stale_request`는 접수 시점 기준이다. 큐 적용 시
    경쟁으로 실패한 경우 완료 상태를 조회할 수 없다.
  - Lane 3의 NMS/typical fixture와 `java/fixtures/nms-plugin`은 이 워크트리에 없으므로,
    harness를 사용하는 실제 NMS 통합은 아직 실행할 수 없다.
- 검증 명령/exit는 이 문서의 최종 검증 절에 기록한다.

### 최종 검증 (v2 수직 슬라이스 + harness)

```sh
cargo fmt --all -- --check
cargo test -p ferrite-plugin-host
cargo test -p ferrite-server plugin_host
cd java && ./gradlew :ferrite-host:test
cd .. && git diff --check
```

- Rust `cargo test -p ferrite-plugin-host`: **13 unit + 1 jvm_smoke + 1 doctest** 통과.
- Rust `cargo test -p ferrite-server plugin_host`: **4 lib + 3 main + 1 hook integration** 통과.
  `plugin_host_integration_skeleton`는 외부 Lane 2/3 fixture가 없어 `#[ignore]`이며 통과로
  계산하지 않는다.
- Java `:ferrite-host:test`: **14 tests, 0 failures** (기존 13 + `HostIntegrationHarnessTest` 1).
  harness test는 test classpath의 plugin main과 plugin.yml만 담은 jar로 start → report →
  close를 실제 실행한다.
- `git diff --check`: exit 0. 커밋/푸시 없음.
- `cargo clippy --workspace --all-targets -- -D warnings`: 별도 실행에서 exit 0(기존 workspace
  manifest 경고만).

정확한 명령별 최종 exit는 작업 세션 로그에 남아 있으며, 실패한 `:ferrite-nms:test`는
Lane 3 문서에 blocked로 기록했다. NMS/typical 외부 fixture 통합은 이 워크트리에서 실행하지
않았다.

## v2 마감 검증 — Linux 워크트리 (2026-11-16)

- 브랜치: `yxshadxw/plugin-b-host`, 원격 `origin/yxshadxw/plugin-b-host`.
- 이 절과 아래 수정을 포함한 커밋이 브랜치 HEAD이며 원격 브랜치로 푸시한다. `main` 반영은
  코디네이터 지시를 기다린다.
- 환경: 이 워크트리는 Linux인데 `rust-toolchain.toml`은 Windows MSVC 툴체인을 고정한다.
  게이트는 `RUSTUP_TOOLCHAIN=stable`로 실행했다. 저장소의 툴체인 고정은 바꾸지 않았다(Windows
  개발 환경에 영향 없음).
- 계약은 v2 동결을 유지했다. 새 op/필드/버전 증가는 없다.

### v2 계약 충실성 수정

- `host.info.capabilities.state_provider`가 provider가 광고한 op 유무가 아니라 **provider 설치
  여부**를 따르도록 고쳤다. op를 하나도 광고하지 않는 provider도 `state_provider=true`,
  `v2_ops=[]`다(계약 §3.2: provider가 없을 때만 false).
- `host.ping`도 종료 중에는 `shutdown` 오류를 돌려준다. 이전에는 ping만 stopped 검사를
  우회해 계약 §3.1 오류 표와 달랐다. 이제 종료 중 모든 op가 같은 규칙을 쓴다.
- 회귀 테스트 `installed_provider_is_advertised_and_shutdown_closes_every_op` 추가: 무광고
  provider의 capability와 `host.ping`/`host.info`/`world.block_get`의 `shutdown` 오류를
  고정한다.

### 게이트 결과 (실제 실행)

```sh
source tools/env.sh && export RUSTUP_TOOLCHAIN=stable
cargo fmt --all -- --check                               # exit 0
cargo clippy --workspace --all-targets -- -D warnings     # exit 0 (기존 manifest unused-dependency 경고만)
cargo test -p ferrite-plugin-host                         # 14 unit + 1 jvm_smoke + 1 doctest, 0 failed
cargo test -p ferrite-server plugin_host                  # 4 lib + 3 main + 1 plugin_host_hooks, 0 failed
cd java && ./gradlew :ferrite-host:cleanTest :ferrite-host:test --no-daemon
                                                          # BUILD SUCCESSFUL; 14 tests, 0 failures
git diff --check                                          # exit 0
```

- `plugin_host_integration_skeleton`는 Lane 2/3 fixture jar가 없어 여전히 `#[ignore]`다.
  통과로 계산하지 않았고 skip으로 약화하지도 않았다.
- Java `:ferrite-host:test`는 `cleanTest`로 강제 재실행했고 XML 기준 testcase 14개,
  failures 0이다.

### 남은 항목과 판단

- `player.state_get`/`entity.state_get`: 계약상 후보이고 기본 provider는 `unsupported`.
  플레이어/엔티티 상태의 의미론은 Lane 2/5 소유라 이 레인에서 구현하지 않는다.
- v2 write 완료 상태 조회 op: 새 op이므로 계약 버전 증가가 필요하다. v2 동결 중에는 추가하지
  않았다. 접수 시점의 `queue_full`/`stale_request`만 관측 가능하다는 한계는 그대로다.
- `HostApi` v2 default의 native 미호출: 계약 §12가 기본 `UnsupportedOperationException`과
  `Bootstrap` transport helper를 지원 경로로 명시하므로 결함이 아니다.
- `HostIntegrationHarness`는 Lane 3 핸드오프가 고정한 `start(Path,Path)`/`reports()`/`close()`
  reflection 계약을 그대로 만족한다. 추가 메서드는 만들지 않았다.
- Lane 3 fixture가 이 워크트리에 없어 실제 typical+NMS 동시 로드는 실행하지 않았다.
