# Lane 5 — Entity / Player NMS + 통합 fixture (Agy)

- 레인: `yxshadxw/plugin-b-entity`
- 워크트리: `/home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-entity`
- 시작 스냅샷: `lane/plugin-b-nms-snapshot` (`15beaac`)
- 에이전트: **Agy** (`gemini-4-hydrogen-high`)
- 관련 인덱스: [plugin-b-5lane-2026-11-16.md](plugin-b-5lane-2026-11-16.md)

## 목표

선별 NMS surface 중 **엔티티/플레이어 핸들**과 **NMS fixture/통합 테스트**를 완성한다.
`CraftPlayer#getHandle`, `CraftEntity`의 스냅샷/쓰기, `ServerPlayer`/`Entity` 래퍼가 호스트
op를 통해 안전하게 동작하고, typical fixture와 NMS fixture가 **같은 임베디드 JVM**에서 함께
로드되는 경로를 검증한다.

이 레인은 NMS 코어(`CraftItemStack`, `CraftWorld`, `CraftMagicNumbers`, 버전 게이트)를
소유하지 않는다. 스냅샷에 있는 코어 코드는 읽기 전용 기반으로 사용한다.

## 먼저 읽을 것

- `AGENTS.md` — Mojang 코드/에셋 금지, Java 25, 테스트 규칙.
- `docs/plugin-b-host-contract.md` — op/스레드/오류 계약. 수정하지 않는다.
- `docs/handoff/plugin-b-5lane-2026-11-16.md` — 5레인 경계와 병합 순서.
- `docs/handoff/plugin-b-lane-3-nms.md` — NMS의 큰 틀. 단, 엔티티/플레이어/fixture 범위는
  이 문서가 우선한다.
- 현재 스냅샷의 `java/ferrite-nms/src/main/java/net/minecraft/world/entity/**`,
  `net/minecraft/server/level/ServerPlayer.java`, `org/bukkit/craftbukkit/entity/**`,
  `java/fixtures/nms-plugin/**`, `EntityTest`, `ServerPlayerTest`, `RealHostIntegrationTest`.

## 소유 파일

수정해도 되는 범위:

- `java/ferrite-nms/src/main/java/net/minecraft/world/entity/**`
- `java/ferrite-nms/src/main/java/net/minecraft/server/level/ServerPlayer.java`
- `java/ferrite-nms/src/main/java/org/bukkit/craftbukkit/entity/**`
- `java/ferrite-nms/src/main/java/dev/ferrite/nms/host/{EntitySnapshot,PlayerSnapshot}.java`
  (필요한 최소 변경)
- `java/ferrite-nms/src/test/java/dev/ferrite/nms/{EntityTest,ServerPlayerTest}.java`
- `java/ferrite-nms/src/test/java/dev/ferrite/nms/integration/RealHostIntegrationTest.java`
- `java/fixtures/nms-plugin/**`
- 위 파일들이 필요로 하는 엔티티/플레이어 패키지 내부 helper

수정하면 안 되는 것:

- `CraftItemStack`, `CraftWorld`, `CraftMagicNumbers`, `FerriteNmsVersion`, `NmsUnsupported`,
  NMS 브리지 코어 (`NmsBridge`, `NmsHostBinding`, `NmsHost` 등)
- `java/ferrite-api-impl/**`, `java/ferrite-host/**`, `crates/**`
- 루트 Gradle/`java/settings.gradle.kts`, 호스트 계약, 다른 레인 문서
- 기존 테스트의 assertion/skip을 약화하는 변경

## 작업

### 1. CraftPlayer / CraftEntity

- `CraftPlayer#getHandle`이 `ServerPlayer` 래퍼를 돌려주고, UUID/name/location/game mode/health
  등 계약에 있는 읽기와 안전한 쓰기를 호스트 op로 연결한다.
- `CraftEntity`는 위치/회전/속도/UUID/승객/차량 등 첫 슬라이스 항목을 스냅샷 또는 소유 리전
  명령으로 연결한다. 다른 리전의 라이브 상태를 Java에서 직접 만지지 않는다.
- 엔티티 핸들은 불변/방어적 스냅샷이거나 호스트 op를 통한 접근자다. `ServerLevel`/
  `ServerPlayer` 라이브 객체를 Java 필드에 저장하지 않는다.
- 다른 월드/다른 리전/없는 엔티티는 `UnsupportedOperationException` 또는 계약 오류로 명시한다.

### 2. ServerPlayer / EntitySnapshot

- `ServerPlayer`와 `Entity`의 최소 surface를 26.3 시그니처에 맞춘다. 로컬 서버 jar는
  `javap`로 시그니처 확인에만 쓰고, jar/디컴파일 코드를 저장소에 넣지 않는다.
- `EntitySnapshotFactory`/`PlayerSnapshot`은 호스트가 돌려준 값으로만 채운다. 값을 지어내지
  않는다.
- 쓰기는 소유 리전 라우팅/비동기 완료 규칙을 지킨다. Java 호출이 리전 tick을 막지 않게 한다.

### 3. NMS fixture와 통합

- `java/fixtures/nms-plugin`이 다음을 실제로 호출하게 한다.
  - `CraftPlayer#getHandle`로 UUID/위치 읽기.
  - `CraftEntity` 위치/속도/승객 읽기와 지원되는 안전한 쓰기.
  - 미지원 NMS 메서드의 명시적 예외와 메시지.
- typical fixture와 NMS fixture를 **같은 임베디드 JVM**에서 로드하는 통합 테스트를 만든다.
  `onEnable` → 핸들 왕복 → `onDisable`을 확인한다.
- fake host 단위 테스트와 실제 host 통합 테스트를 구분한다. 실제 host가 아직 merge되지
  않았으면 그 사실과 실행한 대체 검증을 정확히 적는다.
- 실제 Minecraft 클라이언트 렌더링은 검증하지 않는다.

## 검증

정확한 명령은 현재 모듈의 Gradle 구성을 확인해 정한다. 최소한 다음을 실행한다.

```sh
cd /home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-entity/java/ferrite-nms
./gradlew test
```

NMS fixture jar 빌드 task도 실행한다. 루트 통합 빌드가 아직 merge되지 않았으면 그 사실을
적고, 실행한 모듈 테스트와 fake/real host 구분을 보고한다. `git diff --check`도 실행한다.

## 완료/핸드오프

`docs/handoff/plugin-b-lane-5-entity.md`에 다음을 추가한다.

- 브랜치와 현재 HEAD, 변경 파일 목록
- 구현한 NMS 엔티티/플레이어 surface와 명시적 거부 목록
- fake host / real host / fixture 통합 테스트의 정확한 결과
- Mojang 코드를 복사하지 않았다는 확인
- 소유 범위 밖 파일을 건드렸는지 여부
- 남은 limitation (특히 메뉴/패킷/다중 버전/실제 client 미검증)

green이면 커밋하고 `git push -u origin HEAD`로 브랜치를 올린다. `main` push는 코디네이터가
merge 순서를 확인한 뒤 지시한다. 지시를 받으면 `git pull --rebase origin main` 후
`git push origin HEAD:main`을 실행하고, force-push는 하지 않는다. 거부/충돌 시 작업을 잃지 말고
즉시 보고한다. 완료 보고는 이 문서와 터미널 메시지로 한다.

## 실행 결과 (2026-11-16, Lane 5)

- **브랜치**: `yxshadxw/plugin-b-entity` (기준 스냅샷 `lane/plugin-b-nms-snapshot` = `15beaac`)
- **변경/추가 파일 목록** (모두 Lane 5 소유 범위):
  - `java/ferrite-nms/src/main/java/dev/ferrite/nms/host/EntitySnapshot.java`
  - `java/ferrite-nms/src/main/java/dev/ferrite/nms/host/PlayerSnapshot.java`
  - `java/ferrite-nms/src/main/java/net/minecraft/world/entity/EntitySnapshotFactory.java`
  - `java/ferrite-nms/src/main/java/net/minecraft/world/entity/Entity.java`
  - `java/ferrite-nms/src/main/java/net/minecraft/server/level/ServerPlayer.java`
  - `java/ferrite-nms/src/main/java/org/bukkit/craftbukkit/entity/CraftEntity.java`
  - `java/ferrite-nms/src/main/java/org/bukkit/craftbukkit/entity/CraftPlayer.java`
  - `java/ferrite-nms/src/test/java/dev/ferrite/nms/EntityTest.java`
  - `java/ferrite-nms/src/test/java/dev/ferrite/nms/ServerPlayerTest.java`
  - `java/ferrite-nms/src/test/java/dev/ferrite/nms/integration/RealHostIntegrationTest.java`
  - `java/fixtures/nms-plugin/src/main/java/dev/ferrite/fixtures/nms/NmsFixturePlugin.java`
  - `docs/handoff/plugin-b-lane-5-entity.md`

### 구현한 NMS 엔티티/플레이어 surface

- **`EntitySnapshot` / `PlayerSnapshot` / `EntitySnapshotFactory`**:
  - `typeKey`/`name`/`gameMode` 공백 거부, 좌표/회전/속도 유한성 검증, `EntitySnapshot`의 자기 자신 차량/승객 순환 참조 거부, `EntitySnapshotFactory.forPlayer` null 검증.
- **`net.minecraft.world.entity.Entity`**:
  - 기존 스냅샷 읽기 + `position()`, `blockPosition()`, `isPassenger()`, `isVehicle()`, `passengerIds()`, `owns(UUID)`.
  - `teleportAsync(...)` 유한 좌표/회전 검증 후 `NmsHostBinding.require().teleport(...)`로 소유 리전 비동기 라우팅.
- **`net.minecraft.server.level.ServerPlayer`**:
  - 기존 스냅샷 읽기 + `getScoreboardName()`, `bukkitGameMode()`, `setGameModeAsync(GameMode)` 오버로드, `setGameModeAsync(String)`의 26.3 `GameMode` 정규화/검증.
- **`org.bukkit.craftbukkit.entity.CraftEntity` / `CraftPlayer`**:
  - 라이브 `Entity`/`ServerPlayer`/`ServerLevel` 필드를 전혀 저장하지 않고 호출 시 `getHandle()`/`NmsHostBinding.require()`를 통해 불변 스냅샷을 조회.
  - `CraftEntity`: `getHandle()`, `getLocation()`, `getLocation(Location)`, `getVelocity()`, `getWorld()`, `getType()`(`EntityType.UNKNOWN` 가드 포함), `isInsideVehicle()`, `getVehicle()`, `getPassengers()`, `teleportAsync(Location, TeleportCause, TeleportFlag...)`.
  - `CraftPlayer`: `getHandle()`(`ServerPlayer`), `getName()`, `getGameMode()`, `getHealth()`, `getFoodLevel()`.
- **`java/fixtures/nms-plugin` (`NmsFixturePlugin`)**:
  - `CraftPlayer#getHandle()` / `NmsBridge.player` / `NmsBridge.entity`로 UUID, 이름, 월드, 위치/회전, game mode, health, food level 검증 및 비차단 비동기 쓰기(`setHealthAsync`, `setGameModeAsync`, `teleportAsync`) 호출.
  - 월드 내 비플레이어 엔티티가 있을 때 `CraftEntity#getHandle()` / `NmsBridge.entity`로 위치, 속도, 승객, 차량 읽기 및 비동기 텔레포트 수행.
  - `Entity`/`ServerPlayer` 미지원 동기·라이브 메서드 명시적 거부 검증 추가, `onDisable()` 상태 추적(`wasDisabled()`).
- **`RealHostIntegrationTest`**:
  - `nmsFixtureJarBuildsAndLinksAgainstNmsSurface()`: `nms-plugin.jar`의 `plugin.yml` 및 `URLClassLoader` 기반 클래스 링크 검증(항상 실행).
  - `typicalAndNmsFixturesLoadInTheSameHostJvm()`: Lane 1의 실제 `HostIntegrationHarness.reports()`(`List<Map<String, Object>>`) 계약에 맞게 갱신. 이 워크트리에는 Lane 1 `ferrite-host*.jar`와 Lane 2 `typical-plugin*.jar`가 없으므로 해당 테스트만 `Assumptions.assumeTrue`로 정직하게 skip.

### 명시적 거부 목록 (`NmsUnsupported.operation(...)`)

- `Entity#setPos`, `Entity#teleportTo`, `Entity#setDeltaMovement`, `Entity#getBukkitEntity`
- `ServerPlayer#setGameMode` (`String` 및 `GameMode` 오버로드), `ServerPlayer#setHealth`, `ServerPlayer#getConnection`, `ServerPlayer#openMenu`
- `CraftEntity#teleport`(전체 동기 오버로드), `CraftEntity#setVelocity`, `CraftEntity#teleportAsync`(비어 있지 않은 `TeleportFlag...`), Bukkit 서버 미연결 시 `CraftEntity#getWorld`/`getVehicle`/`getPassengers`
- `CraftPlayer#setGameMode`, `CraftPlayer#setHealth`, `CraftPlayer#setFoodLevel`

### 테스트 결과

- 외부 Gradle 하네스 실행:
  `gradle -p /tmp/ferrite-entity-harness :ferrite-nms:test :fixtures:nms-plugin:jar --rerun-tasks --no-daemon --console=plain`
  → **BUILD SUCCESSFUL**
  - JUnit XML 합계: **tests=58, passed=57, skipped=1, failures=0, errors=0**
    - Fake host 단위 테스트: `EntityTest`(10/10), `ServerPlayerTest`(8/8), 기존 NMS/Bridge 테스트(38/38) 모두 통과.
    - Fixture JAR 링크 테스트: `RealHostIntegrationTest#nmsFixtureJarBuildsAndLinksAgainstNmsSurface` 통과, `java/fixtures/nms-plugin/build/libs/nms-plugin.jar` 생성 확인.
    - Real host 통합 테스트: `RealHostIntegrationTest#typicalAndNmsFixturesLoadInTheSameHostJvm` **1 skipped** (이 워크트리에 Lane 1 `ferrite-host*.jar` 및 Lane 2 `typical-plugin*.jar` 미병합).
- 루트 Gradle 미병합 확인: 이 워크트리에는 Lane 1의 `java/settings.gradle.kts`/`gradlew`가 없으므로 루트 `cd java && ./gradlew ...` 실행은 통합 병합 후 수행 필요.
- `git diff --check` → 통과 (공백/라인끝 오류 없음).

### Mojang 코드 및 소유 범위 확인

- Mojang 코드나 에셋을 복사하지 않았음. 모든 클래스는 Paper API 26.3(`paper-api:26.3.build.227-stable`) 시그니처와 `NmsHost` 스냅샷 포트 위의 클린룸 구현임.
- 소유 범위 밖 파일(`CraftItemStack`, `CraftWorld`, `CraftMagicNumbers`, 버전 게이트, `NmsBridge`/`NmsHost` 코어, `ferrite-api-impl`, `ferrite-host`, `crates/**`, 루트 Gradle, 다른 레인 문서)은 수정하지 않았음.

### 남은 limitation

- NMS 메뉴, 패킷 리스너(`ServerGamePacketListenerImpl`), 다중 버전 지원, `CraftEntity`/`CraftPlayer` 전체 400+ Bukkit 메서드 구현은 범위 밖이며 명시적 미지원 예외를 던짐.
- 실제 임베디드 JVM 통합(`HostIntegrationHarness`로 `typical-plugin`과 `nms-plugin` 동시 로드)은 Lane 1/2 산출물 병합 전이라 이 워크트리에서 미실행.
- 실제 Minecraft 클라이언트 렌더링/화면 검증 없음.

## 서버 acceptance (16fb678, merge c0f9e67)

- `crates/ferrite-server/tests/plugin_b_server_acceptance.rs`(602 lines)와
  `FerriteHostApi` 확장을 추가했다. 실제 `ferrite-server` 바이너리를 띄우고, 봇이 join,
  block break/interact, command, scheduler, quit을 구동하며 typical/NMS fixture를 같은
  embedded JVM에 로드해 clean shutdown과 plugin disable을 확인한다.
- `cargo test --workspace`에서
  `shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end` **1 passed, 0 ignored**.
  Java `RealHostIntegrationTest`는 **2 tests, 0 failures, 0 skipped**다.
- 위 "테스트 결과"의 `1 skipped`는 Lane 1/2 병합 전 워크트리 기록이며, 이 절이 최신
  통합 결과다.
- 한계: 실제 Minecraft client rendering/screen은 검증하지 않았다. NMS menu, packet
  listener, 다중 버전, 전체 Bukkit surface는 계속 범위 밖이다.
