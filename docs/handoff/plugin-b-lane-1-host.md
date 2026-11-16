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
