# Lane 2 — Bukkit/Paper API facade 지시서

- 레인: `lane/plugin-b-api`
- 선행: Lane 1의 `docs/plugin-b-host-contract.md` 동결과 `java/settings.gradle.kts` 스캐폴드.
- 관련 인덱스: [plugin-b-3lane-2026-11-16.md](plugin-b-3lane-2026-11-16.md)

## 목표

플러그인이 실제로 사용하는 Bukkit/Paper API의 **첫 번째 수직 슬라이스**를 Java facade로
구현한다. JVM 시작/클래스로더/RPC 전송은 Lane 1이 소유하고, 이 레인은 그 계약 위에서
`JavaPlugin`, 이벤트, 명령, 스케줄러, 서버/월드/플레이어/아이템의 의미론을 구현한다.

## 먼저 읽을 것

- `docs/plugin-b-host-contract.md` — op 이름/payload/스레드 규칙. 이 문서를 임의로 바꾸지 않는다.
- `docs/handoff/plugin-b-lane-1-host.md` — Java 호스트의 `HostApi`/`Bootstrap` 경계.
- `AGENTS.md` — Mojang 코드/에셋 금지, Java 25, Gradle wrapper 규칙.
- Paper API `26.3.build.227-stable`의 실제 인터페이스. 로컬 Gradle 캐시/설치된 API로
  서명을 확인하고, 문서만 믿고 메서드를 만들지 않는다.
- 삭제된 `java/ferrite-api-impl`은 참고용으로만 본다. 통째로 복원하거나 과거 NMS 코드를
  가져오지 않는다.

## 산출물

### 1. Gradle 모듈과 빌드

- `java/ferrite-api-impl`을 새로 만든다. `java/settings.gradle.kts`는 Lane 1 소유이므로
  모듈 include가 필요하면 Lane 1에 요청한다.
- 의존성은 Paper API `io.papermc.paper:paper-api:26.3.build.227-stable`, Java 25 toolchain,
  JUnit 5. 필요한 런타임 라이브러리(SQLite/JDBC, commons-lang3, log4j, netty 등)는 실제
  플러그인이 기대하는 범위만 추가하고, 추가 이유를 `build.gradle.kts`에 적는다.
- Gradle wrapper(`java/gradlew`)를 사용한다. 시스템 전역 Gradle에 의존하지 않는다.
- `build`/`shadedJar` 산출물이 Lane 1 호스트 classpath에서 로드 가능해야 한다.

### 2. 호스트/로더 연결

- Lane 1의 `HostApi`/`HostBridge`를 구현한다. Java 호스트가 넘긴 플러그인 메타데이터와
  classloader를 사용하고, JVM/클래스로더를 새로 만들지 않는다.
- Rust op 호출은 계약의 타임아웃/오류 규칙을 지킨다. 실패를 `null`/빈 값으로 숨기지 않는다.
- `dev.ferrite.server.FerriteServer`는 `Server` 인터페이스를 구현하거나, 인터페이스가 큰
  경우 동적 프록시로 알려진 메서드만 라우팅한다. 모르는 메서드는 플러그인/메서드 이름이
  들어간 `UnsupportedOperationException`을 던진다.
- `World`/`Player`도 같은 원칙을 쓴다. 라이브 Rust 상태를 Java 객체로 직접 노출하지 않고,
  호스트 op 또는 스냅샷을 사용한다.

### 3. 첫 수직 슬라이스 API

아래는 “전체 Bukkit”이 아니라 첫 통합 테스트를 통과시키는 범위다.

- **JavaPlugin**: `onLoad`/`onEnable`/`onDisable`, `getLogger`, `getDataFolder`,
  `getConfig`/`saveDefaultConfig`/`saveConfig`, `getServer`, `getDescription`,
  `getCommand`, `getPluginMeta`(가능한 범위).
- **PluginManager**: `getPlugin`, `getPlugins`, `isPluginEnabled`, `registerEvents`,
  `getPermission`, `getDefaultPermissions`, `disablePlugin`. 리스너 등록은 plugin disable 시
  자동 해제한다.
- **이벤트**: 최소 `PlayerJoinEvent`, `PlayerQuitEvent`, `BlockBreakEvent`(취소 가능),
  `PlayerInteractEvent`(우클릭 블록, 취소 가능). 필요하면 `PlayerCommandPreprocessEvent`
  또는 API의 비-deprecated 대체 이벤트를 사용해 명령 가로채기를 검증한다.
  이벤트 우선순위(`LOWEST`..`MONITOR`), `ignoreCancelled`, 취소 전파, plugin disable 시
  리스너 제거를 구현한다.
- **명령**: `plugin.yml`의 commands, `PluginCommand#setExecutor`/`setTabCompleter`,
  `CommandSender`, permission Default fallback. 콘솔 sender와 플레이어 sender를 구분한다.
  Paper `LifecycleEvents.COMMANDS`는 첫 슬라이스에서 명시적으로 지원/거부를 결정하고,
  거부하면 테스트로 고정한다.
- **스케줄러**: `BukkitScheduler`의 sync immediate/delayed/repeating, async, cancel,
  plugin disable 시 정리. `RegionScheduler`/`EntityScheduler`는 호스트 계약에 op가 있으면
  라우팅하고, 없으면 가짜 즉시 실행 대신 명시적 거부한다.
- **Server**: `getName`, `getVersion`, `getBukkitVersion`, `getLogger`, `getPluginManager`,
  `getScheduler`, `getWorlds`, `getWorld`, `getPlayer`/`getPlayerExact`,
  `getOnlinePlayers`, `broadcastMessage`, `getConsoleSender`, `getPluginCommand`,
  `getMaxPlayers`, `getPort`, `isPrimaryThread`.
- **World**: `getName`, `getUID`, `getEnvironment`, `getSpawnLocation`, `getBlockAt`,
  `getPlayers`, `getTime`/`setTime`, `getWeatherDuration`/`setStorm`/`setThundering`,
  `getGameRuleValue`의 읽기. 쓰기/월드 생성은 호스트 op가 있을 때만.
- **Player**: `getName`, `getUniqueId`, `getWorld`, `getLocation`, `teleport`,
  `sendMessage`, `kick`, `getGameMode`/`setGameMode`, `getHealth`/`setHealth`,
  `getFoodLevel`/`setFoodLevel`, `getInventory`(기본 슬롯 읽기/쓰기).
- **ItemStack**: material, amount, display name, lore. PDC/컴포넌트는 호스트 왕복이
  준비된 타입만 지원하고, 미지원 타입은 명시적으로 거부한다.
- **권한**: `hasPermission`은 호스트의 op/권한 fallback을 사용한다. 권한 플러그인은
  첫 슬라이스의 범위가 아니다.

### 4. Typical fixture plugin — `java/fixtures/typical-plugin`

- `plugin.yml`을 가진 실제 jar로 빌드한다. `paper-plugin.yml` 변형도 하나 둔다.
- `onEnable`에서:
  - `getLogger().info`, `getDataFolder`, `saveDefaultConfig`, config 읽기.
  - `PlayerJoinEvent` 리스너 등록 및 join 시 메시지/파일 기록.
  - `BlockBreakEvent` 취소 조건과 `PlayerInteractEvent` 확인.
  - `plugin.yml` 명령 `/ferrite-hello` 실행 및 permission 거부 경로.
  - sync 반복 task 1회, async task 1회, disable 시 취소 확인.
  - `ItemStack` display name/PDC(지원 타입) 왕복.
  - 월드 블록 읽기와 가능한 경우 블록 쓰기.
  - 지원하지 않는 API 호출이 명시적 예외를 내는지 확인하는 별도 테스트 메서드.
- 플러그인은 실제 클라이언트 없이도 검증 가능한 로그/파일/명령 응답을 남긴다.
- Lane 1의 호스트 통합 테스트가 이 jar를 사용할 수 있도록 jar 경로를 문서화한다.

### 5. 테스트

- 순수 Java 단위 테스트: fake host로 이벤트 우선순위/취소, 명령 permission, scheduler 취소,
  config/PDC, proxy의 unsupported 예외를 검증한다.
- 계약 테스트: Lane 1의 `HostBridge` fake/real 구현과 op 이름·payload·오류가 일치한다.
- 통합 테스트: 실제 임베디드 JVM에서 typical fixture를 로드해 join 이벤트, `/ferrite-hello`,
  sync/async 스케줄, `onDisable` 정리를 확인한다.
- 스레드 테스트: 이벤트/명령/동기 스케줄러 콜백이 서버 메인 스레드에서 실행되고, async는
  별도 스레드임을 확인한다.
- 검증 명령: `cd java && ./gradlew :ferrite-api-impl:test :fixtures:typical-plugin:jar`,
  관련 `cargo test -p ferrite-server plugin_host`, integration 브랜치의 전체 게이트.

## 비목표

- Bukkit/Paper 전체 API parity. 미지원 메서드는 정직하게 거부한다.
- NMS/CraftBukkit 래퍼. Lane 3이 소유한다.
- 전체 Paper lifecycle event, bootstrap, 채팅/메뉴/맵/inventory GUI의 완성.
- 실제 Minecraft 클라이언트 렌더링 검증.
- 월드젠/엔티티/프로토콜의 새 기능.

## 핸드오프

`docs/handoff/plugin-b-lane-2-api.md`에 브랜치/커밋, 구현한 API 표, 명시적 거부 목록,
fixture jar 경로, 계약 버전, 정확한 Gradle/Rust 테스트 결과, 공유 파일 hunk, 남은
limitation(특히 이벤트 지연, region scheduler, PDC/컴포넌트 범위)을 적는다.

---

## Lane 2 실행 결과 (2026-11-16)

- 브랜치/커밋: `yxshadxw/plugin-b-api` / `9bb08ba` (`feat(plugin-api): add Bukkit/Paper API
  core facade`, 68 files). Lane 4 소유 경로(`dev.ferrite.api.item.**`,
  `dev.ferrite.api.inventory.**`, `FerritePlayerInventory`,
  `org/bukkit/inventory/ItemStack.java`, ItemStack 템플릿/테스트)와 Lane 1 계약 문서,
  5레인 인덱스는 이 커밋에서 제외했고 워크트리에 untracked로 남아 있다.
- 계약: Lane 1 계약 **v1** (`../plugin-b-host/docs/plugin-b-host-contract.md`)을 읽었다.
  op 이름/payload 스키마는 이 facade에서 임의로 만들지 않았다. 이 워크트리에는
  `java/settings.gradle.kts`, `java/gradlew`, `java/ferrite-host`,
  `dev.ferrite.host.HostApi`가 없어 `Bootstrap`/`FerriteHostApi` 어댑터와 계약 JSON
  왕복은 구현·검증하지 못했다(아래 블로커). 현재 in-process 통합 지점은
  `dev.ferrite.api.FerriteRuntime`이다.
- 기준: Java 25, Gradle 9.8.1, Paper API `26.3.build.227-stable`.

### 구현한 API

| 영역 | 구현 내용 |
|---|---|
| JavaPlugin | 실제 Paper `JavaPlugin`을 사용. `FerritePluginClassLoader`가 `ConfiguredPluginClassLoader`를 구현해 `init`/`onLoad`/`onEnable`/`onDisable`, logger, dataFolder, description/pluginMeta, `getConfig`/`saveDefaultConfig`/`saveConfig`, `getCommand`를 제공. `InternalAPIBridge`/`RegistryAccess`/`PluginLoader`/`ServerBuildInfo` 서비스 shim 포함. |
| PluginManager | `getPlugin(s)`, `isPluginEnabled`, `registerEvents`/`registerEvent`, `callEvent`, `enablePlugin`/`disablePlugin`/`disablePlugins`/`clearPlugins`, permission add/remove/default/subscription 조회, `useTimings`. disable 시 `HandlerList.unregisterAll` + 스케줄러 취소. |
| 이벤트 | `PlayerJoinEvent`, `PlayerQuitEvent`, `BlockBreakEvent`, `PlayerInteractEvent`, `PlayerCommandPreprocessEvent` 및 `HandlerList` 기반 커스텀 이벤트. Bukkit priority 순서, `ignoreCancelled`, 취소 전파, 리스너 예외의 event/plugin 래핑. |
| 명령 | `plugin.yml` commands/aliases/permission, `PluginCommand#setExecutor`/`setTabCompleter`, 기본 executor로서 JavaPlugin, 콘솔/플레이어 sender, permission denial, `plugin:command` 정규화. |
| 스케줄러 | sync immediate/delayed/repeating, async delayed/repeating, `callSyncMethod`, `cancelTask(s)`, `getPendingTasks`, disable/plugin 소유권 정리, `getMainThreadExecutor`. sync는 tick에서만, async는 daemon pool에서 실행. |
| Server | `getName`, `getVersion`, `getBukkitVersion`, `getMinecraftVersion`, `getLogger`, `getPluginManager`, `getScheduler`, `getWorlds`, `getWorld(String/UUID/Key)`, `getPlayer(String/Exact/UUID)`, `getPlayerUniqueId`, `getOnlinePlayers`, `getMaxPlayers`, `getPort`, `isPrimaryThread`, `getConsoleSender`, `getPluginCommand`, `broadcastMessage`, `permissionMessage`. |
| World | `getName`, `getUID`, `getEnvironment`, `getSpawnLocation`, `getBlockAt`, `getPlayers`, `getTime`/`setTime`, storm/thundering/weather/thunder duration, `getGameRules`, 문자열 `getGameRuleValue` 읽기. |
| Block | `getType`/`setType`, `getData`, 좌표, `getWorld`, `getLocation`, equality/hash. in-memory block store. |
| Player | `getName`, `getUniqueId`, `isOnline`, `getServer`, `getWorld`, `getLocation`, `teleport(Location[, cause])`, `sendMessage` overloads, `name()`, permission/op, `getGameMode`/`setGameMode`, health/food, `getInventory`, `kickPlayer`/`kick`, `spigot()` refusal. |
| PlayerInventory | 41-slot view, `getItem`/`setItem`/`getContents`/`setContents`, clear/empty/contains, max stack size, main/off hand, held slot, armor/extra contents 및 equipment slot 접근, holder/location/type/viewers. |
| ItemStack | shadow `org.bukkit.inventory.ItemStack` (Paper의 `craftDelegate` 회피): material/amount, type/amount copies, display name, lore, PDC, clone/equals/isSimilar, asOne/asQuantity/add/subtract/serialize. |
| ItemMeta/PDC | display name/lore Component 변환, clone/serialize/equals, PDC set/get/getOrDefault/has/getKeys/isEmpty/getSize/remove/copyTo. STRING PDC 왕복을 fixture에서 검증. |
| 권한 | `PermissionResolver`/`DefaultPermissionResolver`, op/default permission fallback, `PermissionAttachment` 기본 처리(미지원 세부는 UOE). |

### 명시적 거부 (테스트로 고정)

- 생성된 `Unsupported*Base`가 `Server`/`World`/`Player`/`Block`/`PlayerInventory`/
  `ItemMeta`/`PersistentDataContainer`/`BukkitScheduler`/`PluginManager`/
  `ConsoleCommandSender`의 슬라이스 밖 메서드를 `UnsupportedOperationException`으로
  거부한다. 메시지에 API 메서드와 `called by plugin 'X'`를 포함한다.
- 대표 거부: `Server#getRegionScheduler`, `World#getWorldBorder`,
  `World#setGameRuleValue`, `PlayerInventory#addItem`, `Player#spigot`,
  `PluginManager#loadPlugin(s)`, `PluginLoader#loadPlugin`/`getPluginDescription`,
  Paper lifecycle event registration, 월드 생성/월드 보더, inventory GUI/menu,
  NMS/CraftBukkit 접근.
- `ItemStack`은 template이 지원하는 메서드 외 46개를 명시적으로 거부한다
  (components/enchantments/durability/data 등).
- `RegistryAccess`는 의도적으로 빈 registry를 제공한다. 따라서 `Material.isBlock()`,
  registry 기반 `Material`/`ItemType`/`GameRule` 상수 조회는 실제 바닐라 데이터가 아니라
  “없음”으로 동작한다. `ItemStack#getMaxStackSize`는 registry 대신 이름 기반 근사표다.
- jar 로딩/클래스로더/RPC/이벤트 전달은 Lane 1 소유이므로 이 모듈에서 구현하지 않았다.

### Fixture

- jar: `java/fixtures/typical-plugin/build/libs/typical-plugin-1.0.0.jar`
  (9,887 bytes, `plugin.yml` 기반). `paper-plugin.yml` 변형은
  `java/fixtures/typical-plugin/src/paper/resources/paper-plugin.yml`에 있다.
- in-process 통합 테스트는 `TestServer` + `PluginJarLoader`(parent-first
  `FerritePluginClassLoader`, plugin resource child-first)로 실제 jar를 로드한다.
  Lane 1 임베디드 JVM 통합은 아직 아니다.

### 테스트 결과

- `cd java/ferrite-api-impl && ./gradlew clean` → exit 0.
- `./gradlew test :fixtures:typical-plugin:jar` → **BUILD SUCCESSFUL**, 7 test classes,
  **20 tests, 0 skipped, 0 failures, 0 errors**. fixture jar 생성 확인.
- 2026-11-16 커밋 후 재실행: `./gradlew cleanTest test :fixtures:typical-plugin:jar`
  → **BUILD SUCCESSFUL**, 동일하게 **20 tests, 0 skipped, 0 failures, 0 errors**
  (Command 3, EventDispatch 3, ItemStack 4, PluginLifecycle 3, Scheduler 4,
  TypicalFixtureIntegration 1, UnsupportedApi 2). ItemStack 4개는 Lane 4 소유 파일이
  워크트리에 있어 함께 실행된 것이다.
- `./gradlew test --tests 'dev.ferrite.api.TypicalFixtureIntegrationTest'` → 통과.
- canonical `cd java && ./gradlew :ferrite-api-impl:test :fixtures:typical-plugin:jar`는
  실행 불가: 이 워크트리에 `java/settings.gradle.kts`, `java/gradlew`, `java/ferrite-host`가
  없다. Lane 1 산출물이 있는 워크트리에서 재실행해야 한다.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo clean` → exit 0.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test -p ferrite-server plugin_host`
  → **exit 101**. `crates/ferrite-data/build.rs`가
  `vanilla/26.3/generated/reports/blocks.json` 부재로 실패한다(이 워크트리에 `vanilla/`가
  없음). 또한 `crates/ferrite-server`에 `plugin_host` 참조/테스트가 없어, 이 명령은
  현재 통과를 주장할 수 없다.
- `git diff --check` → exit 0.

### 블로커 / limitation

1. **Lane 1 통합 블로커**: 계약 v1은 존재하지만 이 워크트리에 `java/ferrite-host`와
   `dev.ferrite.host.HostApi`/`Bootstrap`이 없다. `FerriteHostApi`, 계약 JSON 파서,
   `fireEvent`/`runTask`/`runCommand` RPC 어댑터, 실제 임베디드 JVM 로드 테스트는
   Lane 1 산출물과 합쳐진 뒤에만 가능하다.
2. **클래스로더 계약**: Paper `JavaPlugin` 생성자는 defining classloader가
   `ConfiguredPluginClassLoader`이길 요구한다. Lane 1은 plain `URLClassLoader` 대신
   `FerritePluginClassLoader`(또는 동일 인터페이스 구현)를 사용해야 한다. parent-first
   규칙은 유지한다.
3. **클래스패스 순서**: `org.bukkit.inventory.ItemStack` shadow가 Paper API보다 먼저
   로드되어야 한다. host classpath에서 `ferrite-api-impl` jar를 `paper-api` 앞에 둔다.
4. **스레드**: sync 이벤트/명령/sync 스케줄러는 `FerriteRuntime`의 main thread에서만
   실행된다. Lane 1은 호스트 메인 스레드로 `FerriteMainThread`를 구성하고, 계약의
   동기 `player_join`/`player_quit` 호출을 이 스레드로 마샬링해야 한다. async 스케줄러만
   별도 daemon pool이다.
5. **registry 데이터 없음**: 빈 registry로 인해 Material/ItemType/GameRule 기반 메타데이터와
   `ItemStack#getMaxStackSize`가 근사값이다. 실제 바닐라 registry 연동은 Lane 1/월드
   데이터 계층의 후속 작업이다.
6. **지연 이벤트 취소**: 계약의 `block_break`/`player_interact` 지연 전달에서는 취소가
   이미 적용된 동작을 되돌리지 못한다(계약 v1 한계). facade는 Bukkit event의 cancelled
   플래그만 제공한다.
7. **NMS/실제 클라이언트 없음**: Lane 3 NMS 래퍼와 실제 Minecraft 클라이언트 렌더링은
   이 레인 범위 밖이며 검증하지 않았다.
8. **Lane 4 파일 의존**: 이 커밋은 Lane 4 소유 파일을 제외했으므로 브랜치 단독으로는
   컴파일되지 않는다. `FerritePlayer`/`PlayerState`가 `FerritePlayerInventory`를 참조하고
   typical fixture가 `org.bukkit.inventory.ItemStack` shadow를 사용한다. 테스트 green은
   Lane 4 파일이 존재하는 워크트리 기준이며, 병합 순서 2→4에 따라 Lane 4 diff가 적용된
   뒤 컴파일된다.
9. **push 차단**: `origin/yxshadxw/plugin-b-api`가 이미 `b894d44`(Lane 4 경로 포함,
   79 files)를 가리켜 `9bb08ba`가 non-fast-forward로 거부되었다. force-push 금지 규칙에
   따라 원격을 덮어쓰지 않았다. 코디네이터 결정 필요: (a) `9bb08ba` force-push 승인,
   또는 (b) 원격 `b894d44` 기준으로 이 커밋을 cherry-pick.

### 테스트 정정 및 공유 파일

- 공유 파일 hunk 없음: `java/settings.gradle.kts`, `java/gradlew`, `java/ferrite-host`,
  `crates/**`를 수정하지 않았다. 이 워크트리에는 해당 Lane 1 산출물이 없다.
- 실패를 숨기기 위해 테스트를 skip/disable하거나 assertion을 낮추지 않았다. 아래는
  Bukkit 의미와 맞지 않던 테스트 기대값을 바로잡은 것이다.
  - `SchedulerTest`: task ID는 연속이고 `callSyncMethod`는 다음 tick에 실행되므로,
    잘못된 ID 차이(2)를 연속(1)으로, future 조회 전에 `tick(1)`을 추가.
  - `PluginLifecycleTest`: `JavaPlugin#getConfig`는 파일을 만들지 않으므로 plugin이
    `saveDefaultConfig()`를 호출한 뒤 파일을 검증.
  - `TypicalFixtureIntegrationTest`: `world:name` 로그는 time/uid를 포함하므로 prefix
    검사, `preprocess:`는 명령 dispatch 뒤에 prefix 검사.
  - fixture/이벤트 테스트는 실제 Bukkit 동작에 맞춰 assertion 메시지만 보강.
  - `UnsupportedApiTest`: `LifecycleEvents.COMMANDS`는 Paper lifecycle service provider가
    없으면 static 초기화에 실패한다. facade가 구현한 configuration overload
    (`registerEventHandler(null)`)로 명시적 거부를 검증해, 테스트 환경 문제와 facade
    거부 동작을 분리했다.

### 검증 결과 (2026-11-16, HEAD `0e53c52`)

- 브랜치 `yxshadxw/plugin-b-api`, tracked worktree clean.
- `cd java/ferrite-api-impl && ./gradlew clean test :fixtures:typical-plugin:jar
  :fixtures:typical-plugin:paperJar shadedJar --console=plain`
  → **BUILD SUCCESSFUL**, **20 tests, 0 skipped, 0 failures, 0 errors**.
  Lane 2 소유 테스트는 16개(`CommandTest` 3, `EventDispatchTest` 3,
  `PluginLifecycleTest` 3, `SchedulerTest` 4, `TypicalFixtureIntegrationTest` 1,
  `UnsupportedApiTest` 2)이고, 나머지 4개는 이 워크트리에 untracked로 있는 Lane 4
  `ItemStackTest`다.
- 같은 `test` task를 `--rerun-tasks`로 2회 더 실행 → 모두 **BUILD SUCCESSFUL**. 총 3회
  연속 green.
- 산출물:
  - `java/fixtures/typical-plugin/build/libs/typical-plugin-1.0.0.jar` (9,887 bytes)
  - `java/fixtures/typical-plugin/build/libs/typical-plugin-1.0.0-paper.jar` (9,766 bytes)
  - `java/ferrite-api-impl/build/libs/ferrite-lane-2-0.1.0-SNAPSHOT-shaded.jar` (140,738 bytes)
- `git diff --check` → exit 0.
- 반복 실행 중 Gradle test worker가 한 번 `java.io.EOFException`으로 종료됐다(JUnit 실패가
  아님). 이후 10회 연속 재실행은 모두 통과했고 재현되지 않았다.
- canonical `cd java && ./gradlew :ferrite-api-impl:test :fixtures:typical-plugin:jar`는
  이 워크트리에 `java/settings.gradle.kts`/`java/gradlew`/`java/ferrite-host`가 없어 실행할
  수 없다. 모듈의 standalone `java/ferrite-api-impl/gradlew`를 사용했다.
- `cargo test -p ferrite-server plugin_host`는 이 워크트리에 `ferrite-plugin-host` crate가
  없고 `crates/ferrite-data` 빌드에 필요한 `vanilla/`도 없어 실행할 수 없다.
