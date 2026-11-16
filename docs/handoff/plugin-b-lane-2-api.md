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
