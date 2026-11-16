# Lane 3 — 선별 NMS 어댑터 지시서

- 레인: `lane/plugin-b-nms`
- 선행: Lane 1의 호스트 계약, Lane 2의 `java/ferrite-api-impl` 모듈 스캐폴드.
- 관련 인덱스: [plugin-b-3lane-2026-11-16.md](plugin-b-3lane-2026-11-16.md)

## 목표

Bukkit API만으로는 부족한 플러그인을 위해 **26.3의 선별된 NMS/CraftBukkit surface**를
클린룸 래퍼로 제공한다. 전체 NMS를 재현하거나 Mojang 내부 구현을 복사하지 않는다.
래퍼는 실제 Rust 상태를 소유하지 않고 Lane 1의 호스트 op를 통해 읽고 쓴다.

## 먼저 읽을 것

- `docs/plugin-b-host-contract.md` — op/스레드/오류 규칙.
- `docs/handoff/plugin-b-lane-2-api.md` — facade의 `World`/`Player`/`ItemStack` 경계.
- `AGENTS.md` — Mojang 코드/에셋 금지, Java 25, 로컬 `vanilla/`는 gitignored.
- 로컬 `vanilla/26.3/lib/server-26.3.jar`의 시그니처는 `javap`로 확인할 수 있지만,
  jar/디컴파일 코드/바닐라 클래스를 저장소에 추가하지 않는다.

## 산출물

### 1. Java 모듈과 버전 게이트

- `java/ferrite-nms` 모듈을 만든다. `java/settings.gradle.kts`는 Lane 1 소유이므로 include가
  필요하면 Lane 1에 요청한다.
- 컴파일 대상은 Java 25, Paper API `26.3.build.227-stable`; NMS 래퍼는 저장소 안의
  **자체 최소 클래스**로 정의한다. 로컬 서버 jar에 대한 컴파일 의존성을 CI 필수 조건으로
  만들지 않는다.
- `FerriteNmsVersion` 같은 단일 상수에 `26.3`/data version을 두고, 다른 버전을 지원하는
  척하지 않는다. 버전이 맞지 않으면 클래스 초기화 시 명확한 오류를 낸다.
- 패키지 이름은 플러그인이 기대하는 바이너리 이름(`net.minecraft.*`,
  `org.bukkit.craftbukkit.*`)을 최소한으로 맞추되, 구현은 Ferrite 호스트 호출이다.
  Mojang 코드를 복사하지 않았다는 사실을 각 파일 헤더/핸드오프에 적는다.

### 2. 첫 번째 NMS surface

첫 통합 테스트에 필요한 것만 구현한다. 각 항목은 “클래스가 존재한다”가 아니라 실제
호스트 왕복과 오류 경로를 테스트한다.

- **`CraftItemStack`**
  - `asNMSCopy(org.bukkit.inventory.ItemStack)`와 `asBukkitCopy(net.minecraft.world.item.ItemStack)`.
  - material, amount, display name, lore, 지원하는 PDC/컴포넌트를 왕복한다.
  - 미지원 메타는 조용히 버리지 말고 타입별 명시적 오류를 낸다.
- **`CraftWorld#getHandle`**
  - `net.minecraft.server.level.ServerLevel` 래퍼를 반환한다.
  - world key/UID/environment/seed/time/spawn, block read/write, loaded chunk 여부 등
    Lane 1 계약에 있는 최소 op만 노출한다.
  - 다른 월드의 상태를 읽거나 쓰는 핸들을 만들지 않는다. 기본 월드/네더/엔드/런타임 월드
    구분은 world id로 검증한다.
- **`CraftMagicNumbers`**
  - `getMaterial`, `getKey`, `getBlock`, `getItem`, `getBlockState`,
    전역 block-state id, `getDataVersion`, `isLegacy`/`fromLegacy` 중 첫 슬라이스에 필요한
    것만. 26.3 데이터와 맞지 않는 값은 공기/빈 아이템으로 대체하지 않는다.
- **`CraftPlayer#getHandle`**
  - `net.minecraft.server.level.ServerPlayer` 래퍼. UUID/name/location/game mode/health 등
    읽기와 안전한 쓰기만 호스트 op로 연결한다.
  - 엔티티/플레이어 라이브 객체를 Java에 직접 노출하지 않는다.
- **`CraftEntity`/엔티티 핸들**
  - 위치/회전/속도/UUID/승객/차량 등 첫 슬라이스 항목을 스냅샷 또는 소유 리전 명령으로
    연결한다. 다른 리전의 라이브 상태를 직접 만지지 않는다.
- **명시적 비지원**: NMS 메뉴/`ServerGamePacketListenerImpl`, 전체 `net.minecraft` 필드,
  raw `Unsafe` 메모리 주소, 다른 버전의 클래스는 첫 슬라이스에서 구현하지 않는다.
  호출되면 `UnsupportedOperationException`에 클래스/메서드/이유를 넣는다.

### 3. 호스트 경계

- 모든 래퍼는 Lane 1의 `HostBridge`/계약 op를 통한다. JNIEnv나 Rust 포인터를 Java 필드에
  저장하지 않는다.
- 블록/엔티티/플레이어 쓰기는 소유 리전으로 라우팅한다. Java 스레드에서 리전을 동기
  대기하지 않는다. 계약이 비동기 완료를 요구하면 `CompletableFuture`/콜백으로 노출한다.
- 반환 스냅샷은 불변 또는 방어적 복사본이다. Java가 들고 있는 `ServerLevel`/`ServerPlayer`가
  월드 tick 상태를 직접 가리키지 않게 한다.
- 오류는 “NMS 클래스가 없다”와 “이 버전에서 의도적으로 미지원”을 구분한다. 전자는 클래스
  로딩/버전 게이트, 후자는 명시적 예외로 처리한다.

### 4. NMS fixture plugin — `java/fixtures/nms-plugin`

- Paper API 플러그인 하나를 만들고, 다음을 실제로 호출한다.
  - `CraftItemStack.asNMSCopy` → `asBukkitCopy` 왕복(material/amount/name/lore/PDC).
  - `CraftWorld#getHandle`로 현재 월드 key/block 읽기.
  - `CraftMagicNumbers`로 material/key/block state/data version 변환.
  - `CraftPlayer#getHandle`로 UUID/위치 읽기(쓰기는 지원 범위만).
  - 일부러 미지원 NMS 메서드를 호출해 명시적 예외와 메시지를 확인.
- jar는 Lane 1 호스트가 로드하고, Lane 2 facade와 같은 JVM에서 동작해야 한다.
- 실제 NMS 플러그인 jar를 추가로 시험할 수 있지만, 저장소에는 커밋하지 않는다.

### 5. 테스트

- Java 단위 테스트: fake host로 item/world/player/magic-number 왕복, 잘못된 world id,
  미지원 메서드 예외, 버전 게이트.
- 계약 테스트: Lane 1의 fake/real host와 op 이름/payload 일치.
- 통합 테스트: 실제 임베디드 JVM에서 typical fixture와 NMS fixture를 함께 로드하고,
  `onEnable` → NMS 왕복 → `onDisable`을 확인한다. 선택 surface에서
  `NoSuchMethodError`/`ClassCastException`이 없어야 한다.
- 스레드 테스트: NMS 쓰기가 소유 리전/서버 스레드 규칙을 위반하지 않고, Java 호출이
  리전 tick을 막지 않는지 확인한다.
- 검증 명령: `cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar`,
  integration 브랜치의 전체 Rust/Java 게이트.

## 비목표

- 전체 CraftBukkit/NMS 구현, Mojang 내부 클래스/필드 복사.
- NMS 메뉴, 패킷 리스너, 전체 엔티티 메타데이터.
- 다중 Minecraft 버전 지원.
- 실제 Minecraft 클라이언트 검증.
- Bukkit API 일반 메서드의 의미론 구현(Lane 2 소유).

## 핸드오프

`docs/handoff/plugin-b-lane-3-nms.md`에 브랜치/커밋, 구현한 NMS surface 표, 버전 게이트,
fixture jar 경로, fake/real host 테스트 결과, 명시적 거부 목록, Mojang 코드를 복사하지
않았다는 확인, 남은 limitation(특히 메뉴/패킷/다중 버전)을 적는다.

## 실행 결과 (2026-11-16, Lane 3 워커)

- 브랜치/커밋: `yxshadxw/plugin-b-nms`, 커밋 없음(변경은 작업 트리에만 남김).
- 계약: 작업 중 Lane 1이 `docs/plugin-b-host-contract.md` **v1**을 동결했다(09:01). 읽은 결과
  v1은 JNI/lifecycle/event/scheduler/`HostApi`만 정의하고 world/block/player/registry RPC op는
  없다. §12에 따라 Lane 3은 Lane 2를 통해 상태를 써야 하므로
  `dev.ferrite.nms.host.NmsHost`는 **Lane 2/3 로컬 포트**로 유지했고, Lane 1 op로 주장하지
  않는다. Lane 2 스캐폴드가 이 워크트리에 없어 실제 바인딩은 미완이다. 계약 §6의
  `onEnable` 전용 host 스레드와 §10의 비동기/타임아웃 규칙을 전제로, NMS 쓰기는 모두
  `CompletableFuture`로만 노출한다.

### 구현한 NMS surface

| 클래스 | 구현 | 호스트 왕복 |
|---|---|---|
| `CraftItemStack` | `asNMSCopy`/`asBukkitCopy`/`asCraftCopy`, whole-stack defensive copy | 값 객체, 월드 상태 없음 |
| `CraftWorld#getHandle` | `ServerLevel` snapshot handle | world/block/chunk 읽기, async block 쓰기 |
| `CraftMagicNumbers` | `getMaterial`/`getKey`/`getBlock`/`getItem`/`getBlockState`/`getBlockStateId`/`getDataVersion`/`isLegacy` | `registryBlockStateId`, host version |
| `CraftPlayer#getHandle` | `ServerPlayer` snapshot + async safe writes | player snapshot, `setGameMode`/`setHealth` |
| `CraftEntity#getHandle` | `Entity` snapshot + async teleport | entity snapshot/teleport |
| `ServerLevel`/`ServerChunkCache`/`ServerPlayer`/`Entity` | snapshot wrappers | 위와 동일 |
| `net.minecraft...Block/BlockState/Item/ItemStack/BlockPos/Vec3` | clean-room value types | 없음 |

### 버전 게이트

- `FerriteNmsVersion`: 26.3 / protocol 777 / data 5023 / Paper API 26.3.build.227-stable.
- static initializer consistency check; `requireSupported` rejects any other triple with
  `NmsVersionMismatchException`; `NmsHostBinding.install` gates the host before binding.
- `CraftMagicNumbers.getDataVersion()` returns the attached host's 5023 after the same check.

### fixture

- jar: `java/fixtures/nms-plugin/build/libs/nms-plugin.jar`
- descriptor: `plugin.yml`, `load: POSTWORLD`, `folia-supported: true`.
- exercises item (material/amount/name/lore/PDC via `CraftItemStack`), world key/UID/block read,
  `CraftMagicNumbers` conversions/data version, player UUID/name/world/health read + async health
  write, and explicit refusals (`ServerLevel#setBlock`, `CraftMagicNumbers#fromLegacy`,
  `CraftItemStack#asCraftMirror`).
- No Minecraft client check; no real embedded JVM load (Lane 1 absent).

### 테스트 결과

- `gradle -p /tmp/ferrite-nms-harness :ferrite-nms:test :fixtures:nms-plugin:jar --no-daemon`
  → BUILD SUCCESSFUL, 34 tests passed, fixture jar produced. The harness is a throwaway
  settings file outside the repository; it mirrors Lane 1's `FAIL_ON_PROJECT_REPOS` mode, adds
  the Paper repository, and includes `java/ferrite-nms` plus `java/fixtures/nms-plugin` because
  Lane 1's real root currently has neither. The intended command remains
  `cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar`.
- fake host coverage: version mismatch, binding/unbinding, unknown world/player/entity, block
  state reads, global id resolution, async write routing/completion/exception, sync refusal,
  registry/data-version reads, unsupported messages, item value operations.
- real host integration: **not run** (blocker). No `NoSuchMethodError`/`ClassCastException`
  integration evidence yet.

### 명시적 거부

- `CraftItemStack#asCraftMirror` (mirror needs a live server-side stack),
  `ServerLevel#setBlock` sync (use `setBlockAsync`), `CraftMagicNumbers#fromLegacy`
  (needs Lane 2 `UnsafeValues`).
- Not provided at all: NMS menus, `ServerGamePacketListenerImpl`/packet listener surface,
  arbitrary `net.minecraft` fields, raw `Unsafe`/memory addresses, multi-version classes.

### Mojang

- No Mojang code/assets copied. All NMS classes are clean-room wrappers over Paper API types and
  the local host port. No server jar or decompiled code is in the worktree; no vanilla data used.
  Paper API signatures were checked from the resolved API artifact only.

### 계약 v1과의 차이

- v1 §13 includes only `ferrite-host`, `ferrite-api-impl`, `ferrite-nms`; the NMS fixture
  include is missing.
- v1 has no state RPC ops, so `NmsHost` cannot be a Lane 1 wire-op adapter under v1. It must be
  implemented by Lane 2 (the `HostApi` owner, §12) or added as an explicit v2 contract change.
- v1 `host.info` carries `api_version: "26.3"` but not protocol/data version. The adapter can
  pair the contract version with `FerriteNmsVersion.PROTOCOL_VERSION/DATA_VERSION` because both
  jars ship together; `CraftMagicNumbers.getDataVersion()` still checks the triple.
- v1 `onEnable` runs on the dedicated host thread, not a server tick. The fixture reads
  postworld state there and never blocks on a region future.
- Lane 2 is now present in its own worktree. `FerriteWorld` and `FerritePlayer` are **final** and
  extend `UnsupportedWorldBase`/`UnsupportedPlayerBase`, so they do not satisfy the fixture's
  `world instanceof CraftWorld` / `player instanceof CraftPlayer` checks. Without a Lane 2 change
  (extend the Craft bases or implement an agreed interface), `CraftWorld#getHandle` and
  `CraftPlayer#getHandle` cannot be reached by a cast from a Ferrite world/player. This is the
  main remaining integration blocker.
- Lane 2 shadows `org.bukkit.inventory.ItemStack` with its own supported slice. Our
  `CraftItemStack` uses only `getType`/`getAmount`/`clone`/`setAmount`/`getItemMeta`/`setItemMeta`,
  which that class provides, but no real-runtime metadata roundtrip was executed.

### 블로커 / 다음 단계

1. Lane 1's `java/settings.gradle.kts` includes `ferrite-nms` but not `:fixtures:nms-plugin`;
   add the fixture include. Its `dependencyResolutionManagement` uses `FAIL_ON_PROJECT_REPOS`
   with only Maven Central, so add `https://repo.papermc.io/repository/maven-public/` (Paper API
   is not on Maven Central). The module build files now declare no repositories to comply.
2. Contract v1 has no state ops. Bind the local `NmsHost` port from Lane 2's `HostApi` facade
   implementation (or extend the contract in v2): world snapshot, block read, async block write,
   registry block-state id, player/entity snapshot, async teleport/game mode/health.
3. Lane 2's `World`/`Player` implementations must extend `CraftWorld`/`CraftPlayer` so the
   fixture casts work; `CraftItemStack` metadata roundtrip needs Lane 2's `RegistryAccess` and
   item factory.
4. Then run the embedded JVM integration (typical + NMS fixture in one JVM) and check
   onEnable → roundtrips → onDisable, errors and thread routing.

### Limitation

- Item metadata tests are not yet runnable: Paper's `org.bukkit.inventory.ItemStack` requires a
  live server/facade even to construct. The NMS code preserves metadata via a defensive copy,
  but only the real Lane 2 integration can prove it.
- `CraftMagicNumbers` global IDs come from the host; no vanilla ID table is committed.
- Snapshot handles do not expose live tick state or mutable NMS fields by design.

## 계약 v2 제안 — NMS state ops (2026-11-16)

> 상태: **제안만**. 이 절은 Lane 1이 소유한 `docs/plugin-b-host-contract.md`를 수정하지 않는다.
> Lane 3은 어떤 Rust op, wire 이름, payload도 구현하거나 동결하지 않았고, 아래 이름을
> `NmsHost`에 결선하지도 않았다. Lane 1이 v2를 동결하면 Lane 3이 그 이름으로 로컬 포트를
> 교체한다.

v1은 JNI/lifecycle/event/scheduler/`HostApi`만 정의하고 world/block/player/registry 상태 op가
없다. NMS 표면은 모두 상태가 필요하므로, 현재 `dev.ferrite.nms.host.NmsHost`는 Lane 2/3
로컬 포트로만 존재한다. v2는 아래 op를 **추가**한다. 기존 v1 op/payload는 바꾸지 않는다.

### 버전과 능력 협상

- `host.info` 응답에 `contract_version: 2`를 추가한다. v1 호스트는 이 필드가 없거나 1이다.
- 같은 응답에 `minecraft_version`, `protocol_version`, `data_version`, `state_ops`, 선택적으로
  `nms_surface_version`을 추가한다. NMS는 이 네 값이 `FerriteNmsVersion`과 일치할 때만
  `NmsHostBinding.install`을 허용한다. v1처럼 triple이 빠진 응답을 추측으로 보완하지 않는다.
- `state_ops`는 이 호스트가 실제로 제공하는 op 이름의 배열이다. NMS가 요구하는 op가 하나라도
  빠지면 설치 시점에 `NmsHostException("unsupported_contract", ...)`로 거부하고, 런타임
  fallback/가짜 성공을 만들지 않는다.
- op 추가는 minor, 기존 op의 의미 변경은 major다. 구버전 이름을 새 의미로 재사용하지 않는다.

### 공통 envelope와 스레드 규칙

v1이 이미 정한 request/response envelope, request id, timeout, plugin 이름 필드를 그대로 쓴다.
아래는 payload와 완료 의미만 제안한다. v1에 비동기 완료 채널이 없다면 `op.complete`
콜백을 추가한다.

```json
{"op":"world.snapshot","request_id":17,"plugin":"FerriteNmsFixture",
 "payload":{"world_id":"00000000-0000-0000-0000-000000000001"}}
```

```json
{"request_id":17,"ok":true,"payload":{"name":"world","dimension_key":"minecraft:overworld"}}
{"request_id":17,"ok":false,"error":{"code":"unknown_world","message":"..."}}
```

쓰기는 접수와 적용을 분리한다.

```json
{"request_id":18,"ok":true,"payload":{"accepted":true}}
{"type":"op.complete","request_id":18,"ok":true}
{"type":"op.complete","request_id":18,"ok":false,
 "error":{"code":"region_unavailable","message":"..."}}
```

- **읽기**(`*.snapshot`, `*.get`, `*.chunk_loaded`, `registry.block_state_id`)는 불변
  스냅샷/레지스트리에서 동기로 답한다. 소유 리전의 live lock을 잡거나 리전 tick을 기다리지
  않는다. 호스트가 그 보장을 못 하면 `read_not_snapshot_safe`로 실패하고, NMS 호출자는
  비동기 경로를 써야 한다. 조용히 live 상태를 읽지 않는다.
- **쓰기**(`world.block_state.set`, `entity.teleport`, `player.game_mode.set`,
  `player.health.set`)는 호출 스레드에서 접수만 하고, 적용은 소유 리전/서버 스레드에서 한다.
  future/콜백은 적용 뒤 그 소유 스레드에서 완료된다. 호출 스레드는 리전을 동기 대기하지
  않는다(계약 §6/§10).
- 같은 world/entity/player 키의 쓰기는 요청 순서를 지킨다. 서로 다른 키 사이의 완료 순서는
  보장하지 않는다. 텔레포트가 다른 리전/월드로 넘어가면 source→target 핸드오프 뒤 target이
  적용하고 완료한다. 어느 리전도 다른 리전을 동기 대기하지 않는다.
- 타임아웃은 v1 규칙을 따른다. 타임아웃된 future는 `timeout`으로 예외 완료되고, 늦게 도착한
  적용 결과는 `stale_completion`으로 로그만 남긴다(두 번 완료하지 않는다). 플러그인 disable
  시 아직 시작되지 않은 쓰기는 `plugin_disabled`로 취소하고, 이미 적용된 쓰기는 롤백하지
  않는다.
- 스냅샷은 불변이다. `player.snapshot`/`entity.snapshot` 응답에 live 객체, Rust 포인터,
  `JNIEnv`를 넣지 않는다.

### op 표

| op | 요청 payload | 응답 payload | 스레드 | 주요 오류 |
|---|---|---|---|---|
| `world.snapshot` | `world_id` | world 메타데이터 | sync snapshot | `unknown_world`, `timeout` |
| `world.block_state.get` | `world_id,x,y,z` | `loaded,global_state_id,name,properties,air` | sync snapshot | `unknown_world`, `timeout` |
| `world.block_state.set` | `world_id,x,y,z,global_state_id` | `accepted` + `op.complete` | accept caller, apply owner region | `unknown_world`, `unknown_block_state`, `position_out_of_bounds`, `region_unavailable`, `timeout` |
| `world.chunk_loaded` | `world_id,chunk_x,chunk_z` | `loaded` | sync snapshot | `unknown_world`, `timeout` |
| `registry.block_state_id` | `block_state` (canonical string) | `global_state_id` | sync, immutable registry | `invalid_block_state`, `unknown_block_state`, `timeout` |
| `player.snapshot` | `player_id` | player 스냅샷 | sync snapshot | `unknown_player`, `player_offline`, `timeout` |
| `entity.snapshot` | `entity_id` | entity 스냅샷 | sync snapshot | `unknown_entity`, `entity_not_loaded`, `timeout` |
| `entity.teleport` | `entity_id,target_world_id,x,y,z,yaw,pitch` | `accepted` + `op.complete` | accept caller, apply target owner region | `unknown_entity`, `unknown_world`, `invalid_position`, `region_unavailable`, `timeout` |
| `player.game_mode.set` | `player_id,game_mode` | `accepted` + `op.complete` | accept caller, apply player region | `unknown_player`, `player_offline`, `invalid_game_mode`, `region_unavailable`, `timeout` |
| `player.health.set` | `player_id,health` | `accepted` + `op.complete` | accept caller, apply player region | `unknown_player`, `player_offline`, `invalid_health`, `region_unavailable`, `timeout` |

### payload 상세

- `world.snapshot` 응답: `world_id`, `name`, `dimension_key`, `environment`, `seed`,
  `day_time`, `spawn{x,y,z}`, `min_y`, `max_y`. 현재 `WorldSnapshot` 필드와 1:1이다.
- `world.block_state.get` 요청의 `x/y/z`는 정수 블록 좌표다. `y`가 월드 높이 밖이거나 청크가
  로드되지 않았으면 오류 대신 `loaded:false`, `name:"minecraft:air"`, `air:true`,
  호스트의 canonical air `global_state_id`를 반환한다(바닐라 `getBlockState` 의미). `loaded`가
  false인 것과 실제 공기 블록을 구분할 수 있어야 한다.
- `world.block_state.set`의 `global_state_id`는 26.3 전역 block-state id다. 호스트는 큐에 넣기
  전에 id를 검증한다. 첫 슬라이스에는 compare-and-set/expected-state를 넣지 않는다.
- `registry.block_state_id`는 `minecraft:oak_stairs[facing=north,...]` 같은 canonical 문자열을
  받는다. 속성 순서/누락은 정규화하지 않고 그대로 레지스트리에 묻는다. 모르는 값은 공기가
  아니라 `unknown_block_state`다.
- `player.snapshot` 응답: `player_id`, `name`, `world_id`, `x,y,z`, `yaw,pitch`, `game_mode`,
  `health`, `food_level`. 현재 `PlayerSnapshot`과 1:1이다.
- `entity.snapshot` 응답: `entity_id`, `type_key`, `world_id`, `x,y,z`, `yaw,pitch`,
  `velocity{x,y,z}`, `vehicle_id`(nullable), `passengers`(UUID 배열, 빈 배열 허용). 현재
  `EntitySnapshot`과 1:1이다.
- `entity.teleport`는 `target_world_id`를 명시한다. 같은 월드 텔레포트도 생략하지 않는다.
  `yaw/pitch`는 float, 좌표는 double, 유한값만 허용한다.
- `player.game_mode`는 `SURVIVAL|CREATIVE|ADVENTURE|SPECTATOR` 대문자 토큰만 허용한다. 모르는
  값은 `invalid_game_mode`다. `player.health`는 유한한 0 이상 값이며, 최대 체력 초과를
  거부할지는 Lane 1이 정한다(제안: `invalid_health`로 거부, 조용한 clamp 금지).

### 공통 오류 코드

| code | 의미 | 재시도 |
|---|---|---|
| `invalid_payload` | 필드 누락/타입 오류 | 아니오 |
| `unsupported_op` | 이 호스트/계약 버전에 없는 op | 아니오 |
| `contract_version_mismatch` | v2 필수 필드/능력 없음 | 아니오(재설치) |
| `unknown_world` / `unknown_player` / `unknown_entity` | 해당 id가 없음 | 아니오 |
| `unknown_block_state` / `invalid_block_state` | 레지스트리에 없음/문자열 오류 | 아니오 |
| `player_offline` | 온라인 목록에서 사라짐 | 아니오 |
| `position_out_of_bounds` / `invalid_position` | 좌표/회전 오류 | 아니오 |
| `invalid_health` / `invalid_game_mode` | 값 검증 실패 | 아니오 |
| `read_not_snapshot_safe` | 동기 읽기 보장 불가 | 비동기 경로로 재시도 |
| `region_unavailable` | 소유 리전 종료/이동 | 예 |
| `host_unavailable` | 호스트/JVM 종료 중 | 예(수명 내) |
| `timeout` | v1 타임아웃 | 예 |
| `plugin_disabled` | disable로 취소 | 아니오 |
| `stale_completion` | 타임아웃 뒤 늦은 완료(로그 전용) | 아니오 |

### NMS wrapper → op 매핑

| NMS 호출 | 계약 op |
|---|---|
| `CraftWorld#getHandle` → `ServerLevel.of` | `world.snapshot` |
| `ServerLevel#getBlockState` | `world.block_state.get` |
| `ServerLevel#isChunkLoaded` / `ServerChunkCache#isChunkLoaded` | `world.chunk_loaded` |
| `ServerLevel#setBlockAsync` | `world.block_state.set` |
| `CraftMagicNumbers#getBlockStateId` | `registry.block_state_id` |
| `CraftPlayer#getHandle` → `ServerPlayer.of` | `player.snapshot` |
| `CraftEntity#getHandle` → `Entity.of` | `entity.snapshot` |
| `Entity#teleportAsync` | `entity.teleport` |
| `ServerPlayer#setGameModeAsync` | `player.game_mode.set` |
| `ServerPlayer#setHealthAsync` | `player.health.set` |
| `CraftMagicNumbers#getDataVersion` | `host.info`의 `data_version` |

### Lane 1이 정해야 할 것

1. 위 op 이름과 payload가 v1의 명명 규칙과 맞는지, 더 짧은 이름이 필요한지.
2. 비동기 완료를 callback으로 할지, request id 기반 long-poll/queue로 할지.
3. `world.block_state.get`/`player.snapshot`/`entity.snapshot`을 플러그인 스레드에서 동기로
   허용할지. 허용하지 않으면 NMS 읽기도 future로 바꿔야 한다.
4. 스냅샷에 `server_tick`/`sequence`를 넣어 stale 여부를 노출할지.
5. 공기 `global_state_id`의 canonical 값과, 로드되지 않은 청크를 `loaded:false`로 표현하는
   방식.
6. `player.health.set`의 최대 체력 처리(거부 vs clamp)와 `entity.teleport`의 cross-region
   완료 시점.
7. `host.info`에 protocol/data/state_ops를 넣는 위치(flat vs nested).
8. 계약 버전 필드가 없는 v1 호스트를 NMS 설치 단계에서 거부하는 방식.

## CraftWorld/CraftPlayer cast 블로커 — 최소 handle provider 제안

> 상태: **제안 + 후속 구현 완료**. Lane 2 소스는 수정하지 않았다. 이 절의 SPI/캐시는 아래
> "구현: NmsBridge SPI + canonical weak handle cache" 절에서 코드로 완료됐고, Lane 2 adapter
> 연결과 real-host 통합만 남아 있다.

### 문제

Lane 2의 `FerriteWorld`/`FerritePlayer`는 `final`이고
`UnsupportedWorldBase`/`UnsupportedPlayerBase`를 상속한다. 이 베이스들은
`org.bukkit.craftbukkit.CraftWorld`/`CraftPlayer`가 아니므로 NMS fixture의
`world instanceof CraftWorld`, `player instanceof CraftPlayer`와 cast가 실패한다. NMS 모듈이
Lane 2 클래스를 직접 참조하거나, Lane 2가 NMS 클래스를 상속하는 방식은 두 레인의 빌드
그래프와 단일 상속을 건드리므로 첫 슬라이스에 적합하지 않다.

### 제안: Lane 2 facade가 구현하는 `NmsHandleProvider`

Lane 3이 중립 인터페이스와 binding만 제공하고, Lane 2가 **별도 adapter 클래스**로 구현한다.
`FerriteWorld`/`FerritePlayer` 자체는 수정하지 않는다.

```java
package dev.ferrite.nms.bridge;

public interface NmsHandleProvider {
    ServerLevel level(org.bukkit.World world);
    ServerPlayer player(org.bukkit.entity.Player player);
    net.minecraft.world.entity.Entity entity(org.bukkit.entity.Entity entity);
}
```

```java
package dev.ferrite.nms.bridge;

public final class NmsBridge {
    public static void install(NmsHandleProvider provider) { /* one per JVM, version-gated */ }
    public static void uninstall(NmsHandleProvider expected) { /* owner check */ }

    public static ServerLevel level(org.bukkit.World world) { /* provider or existing CraftWorld */ }
    public static ServerPlayer player(org.bukkit.entity.Player player) { /* provider */ }
    public static net.minecraft.world.entity.Entity entity(org.bukkit.entity.Entity entity) { /* provider */ }
}
```

동작 규칙:

- `NmsBridge.level/world`는 먼저 `world instanceof org.bukkit.craftbukkit.CraftWorld`인지 본다.
  그렇다면 상속된 `getHandle()`을 쓰고, 아니면 설치된 provider에 위임한다. 이렇게 하면 나중에
  shared bridge로 상속을 바꿔도 fixture 호출은 그대로다.
- provider는 facade 객체의 UUID/타입을 검증해 해당 스냅샷 handle만 반환한다. 다른 월드의
  handle을 만들거나 ownership을 넓히지 않는다. 모르는 객체는
  `NmsHostException("unknown_world"|"unknown_player"|"unknown_entity", ...)`다.
- provider는 `NmsHost` snapshot만 사용하고 live Rust 객체/포인터/JNIEnv를 반환하지 않는다.
  읽기는 즉시, 쓰기는 `CompletableFuture`(소유 리전 완료)다.
- provider는 thread-safe하고 idempotent해야 한다. Lane 1이 facade 초기화 뒤, POSTWORLD
  플러그인 enable 전에 설치하고, 모든 플러그인 disable 뒤에 해제한다.
- 설치 전/해제 후 호출은 명시적 bridge 오류(`NmsBridgeException`)로 실패한다. fallback 없음.

Lane 2 adapter는 대략 다음 형태이며, 기존 world/player 클래스를 상속/수정하지 않는다.

```java
final class FerriteHandleProvider implements NmsHandleProvider {
    @Override public ServerLevel level(org.bukkit.World world) {
        if (!(world instanceof dev.ferrite.api.FerriteWorld ferrite)) {
            throw new NmsBridgeException("unknown_world", world.getName() + " is not a Ferrite world");
        }
        return ServerLevel.of(ferrite.getUID());
    }
    // player/entity도 같은 방식으로 facade 타입에서 UUID를 읽는다.
}
```

fixture 변경(구현 완료; 정확한 동작은 아래 구현 절 참고):

```java
ServerLevel level = NmsBridge.level(world);       // ((CraftWorld) world).getHandle() 대신
ServerPlayer handle = NmsBridge.player(player);   // ((CraftPlayer) player).getHandle() 대신
```

### 왜 shared bridge를 첫 슬라이스에 쓰지 않는가

`(CraftWorld) world` 자체를 성공시키려면 `FerriteWorld`의 상위 클래스가 결국
`CraftWorld`여야 한다. 이는 Lane 2의 `UnsupportedWorldBase`/`UnsupportedPlayerBase` 계층과
`FerriteWorld`/`FerritePlayer`의 단일 상속을 바꾸고, `ferrite-api-impl` → `ferrite-nms`
빌드 의존을 만든다. 플레이어 쪽은 `UnsupportedEntityBase` 체인까지 재배치해야 해서 첫
슬라이스의 최소 변경이 아니다. provider는 이 비용 없이 fixture와 플러그인이 쓸 단일 진입점을
만들고, 나중에 shared bridge를 추가해도 `NmsBridge` 구현만 바꾸면 된다.

**정직한 한계:** provider는 임의의 서드파티 플러그인이 쓰는 `(CraftWorld)`/
`(CraftPlayer)` cast를 성공시키지 않는다. 그 cast 호환은 별도 shared bridge 단계가 필요하며,
그때 Lane 2의 베이스 계층 변경과 모듈 의존을 다시 합의해야 한다. 이 제안은 선택된 NMS
fixture/플러그인의 cast 필요를 없애는 것이지, CraftBukkit 클래스 계층을 흉내내는 것이 아니다.

### Lane 1/2 합의가 필요한 것

1. Lane 2: adapter 클래스의 패키지/이름, facade 타입에서 UUID를 읽는 공개 접근자, 설치 시점.
2. Lane 1: `NmsBridge.install` 호출 위치(host 초기화 직후, POSTWORLD enable 전),
   uninstall 시점, host classpath에 `ferrite-nms`가 있는지.
3. coordinator: `ferrite-api-impl`이 `ferrite-nms`의 bridge 인터페이스에 의존해도 되는지.
   안 되면 인터페이스만 중립 모듈로 옮긴다.
4. shared bridge를 지금 할지, provider로 첫 통합을 통과한 뒤로 미룰지.

## real host 통합 테스트 skeleton (2026-11-16)

- 파일: `java/ferrite-nms/src/test/java/dev/ferrite/nms/integration/RealHostIntegrationTest.java`
- 목적: typical fixture와 NMS fixture를 **같은 임베디드 JVM**에 로드하고, `onEnable` → NMS
  왕복 → `onDisable`을 확인하는 실제 호스트 테스트의 골격.
- 현재 동작: Lane 1의 host jar 또는 Lane 2의 typical jar가 없을 때만
  `Assumptions.assumeTrue`로 **정직하게 skip**한다. NMS fixture jar가 없거나 Lane 1의 harness
  seam이 없으면 skip하지 않고 assertion failure다. 가짜 성공이나 빈 assertion은 없다. 이
  워크트리에는 Lane 1의 `java/settings.gradle.kts`, `ferrite-host` jar, Lane 2의
  `fixtures/typical-plugin` jar가 없으므로 skip이 정상이다.
- 전제(Lane 1/2가 채워야 함):
  1. `java/settings.gradle.kts`가 `:ferrite-nms`, `:fixtures:nms-plugin`,
     `:fixtures:typical-plugin`을 include하고 `https://repo.papermc.io/repository/maven-public/`를
     추가한다.
  2. `:ferrite-host`가 `build/libs/ferrite-host*.jar`를 만든다.
  3. Lane 2가 `fixtures/typical-plugin/build/libs/typical-plugin*.jar`를 만든다.
  4. Lane 1이 test seam `dev.ferrite.host.integration.HostIntegrationHarness`를 제공한다:
     `static HostIntegrationHarness start(Path pluginsDir, Path dataDir)`와
     `Map<String,String> reports()`(plugin 이름 → `"ok"`/오류). 이름/시그니처는 Lane 1이
     확정할 수 있으며, 그때 skeleton의 reflection 문자열만 맞추면 된다. seam은 fixture를
     enable하기 전에 Lane 2의 `NmsHandleProvider`를 `NmsBridge.install(provider)`로 설치해야
     한다. provider가 없으면 NMS fixture가 `provider_not_installed`로 실패하고 report가
     `"ok"`가 아니므로 이 테스트도 실패한다.
- skeleton은 jar의 `plugin.yml`/main class를 먼저 검증하고, seam이 있으면 두 jar를 하나의
  plugins 디렉터리에 넣어 한 번에 start한 뒤 두 report가 모두 `ok`인지 확인한다. seam이
  없으면 assertion failure로 멈춘다.
- 실행 명령(합의 뒤): `cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar`.
  이 워크트리의 실제 루트에서는 아직 실행할 수 없다(settings/wrapper 없음).

### 2026-11-16 검증 결과 (통합 준비 후속)

- 실제 루트 명령 `cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar`:
  **실행 불가**, exit 127 — `./gradlew: No such file or directory`. 이 워크트리에는 Lane 1의
  `java/settings.gradle.kts`/wrapper가 없다. Lane 1이 include와 Paper repository를 고친 뒤
  같은 명령을 다시 실행해야 한다.
- 임시 harness `gradle -p /tmp/ferrite-nms-harness :ferrite-nms:test :fixtures:nms-plugin:jar
  --no-daemon --console=plain` → **BUILD SUCCESSFUL**, 기존 **34 tests passed**, 새
  `RealHostIntegrationTest` **1 skipped**(위 전제 누락), fixture jar 생성/UP-TO-DATE.
- `git diff --check` → 출력 없음(clean). 커밋/푸시 없음; 변경은 작업 트리에만 있다.
- 34개 기존 테스트는 삭제/약화/skip되지 않았다. 새 통합 skeleton만 전제가 없어 skip이며,
  전제가 채워지면 자동으로 실행된다.

## 구현: NmsBridge SPI + canonical weak handle cache (2026-11-16 후속)

> 상태: **코드/테스트 완료**, 커밋/푸시 없음. Lane 2 소스와 Lane 1 계약 파일은 수정하지 않았다.
> 계약 v2 state op 제안은 여전히 제안 상태이며 이 구현은 그 op들을 동결하지 않는다.

### 추가 파일

- `java/ferrite-nms/src/main/java/dev/ferrite/nms/bridge/NmsHandleProvider.java`
- `java/ferrite-nms/src/main/java/dev/ferrite/nms/bridge/NmsBridge.java`
- `java/ferrite-nms/src/main/java/dev/ferrite/nms/bridge/NmsBridgeException.java`
- `java/ferrite-nms/src/test/java/dev/ferrite/nms/bridge/NmsBridgeTest.java`

### 공개 SPI 시그니처

```java
package dev.ferrite.nms.bridge;

public interface NmsHandleProvider {
    ServerLevel level(org.bukkit.World world);
    ServerPlayer player(org.bukkit.entity.Player player);
    net.minecraft.world.entity.Entity entity(org.bukkit.entity.Entity entity);
}

public final class NmsBridge {
    public static void install(NmsHandleProvider provider);
    public static void uninstall(NmsHandleProvider expected);
    public static void close();
    public static boolean isInstalled();

    public static ServerLevel level(org.bukkit.World world);
    public static ServerPlayer player(org.bukkit.entity.Player player);
    public static net.minecraft.world.entity.Entity entity(org.bukkit.entity.Entity entity);
}

public final class NmsBridgeException extends RuntimeException {
    public String code();
}
```

`NmsBridgeException.code()` 토큰:

| code | 발생 지점 |
|---|---|
| `provider_not_installed` | Ferrite facade를 resolve했지만 provider가 없음. fallback 없음 |
| `provider_already_installed` | 다른 provider가 설치된 상태에서 `install` |
| `provider_owner_mismatch` | 다른 provider를 `uninstall(expected)` |
| `unknown_world` / `unknown_player` / `unknown_entity` | provider가 모르는 facade를 거부 |
| `unknown_handle` | provider가 null을 반환 |
| `handle_identity_mismatch` | provider가 요청 facade와 다른 UUID의 handle 반환 |
| `bridge_uninstalled` | resolve 도중 uninstall/close/replace로 generation이 바뀜 |

provider가 던지는 `NmsHostException`은 wrap하거나 다른 코드로 바꾸지 않고 그대로 전달한다.

### 캐시 semantics

- world/player/entity별 `WeakHashMap` 3개를 둔다. 키는 Bukkit facade 객체이고 **weak**이며,
  값은 facade를 참조하지 않는 detached snapshot handle이다. 따라서 bridge가 월드/플레이어를
  붙잡아 메모리에 남기지 않는다. provider도 handle 안에 facade를 넣으면 안 된다.
- 같은 facade 객체는 reachable한 동안 같은 handle 인스턴스로 resolve된다. 캐시 hit은 provider를
  다시 호출하지 않는다. `NmsBridge.entity(player)`는 player cache를 사용하므로 player/entity 두
  진입점이 서로 다른 player handle을 만들지 않는다.
- `WeakHashMap`은 다음 접근에서 stale entry를 expunge한다. `uninstall`/`close`는 즉시 세 캐시를
  비운다. 값이 strong인 것은 facade가 살아 있는 동안 canonical handle을 유지하기 위해서이며,
  facade에 대한 strong reference는 남지 않는다.
- 키 동등성은 facade의 `equals`/`hashCode`를 따른다. Ferrite facade는 수명 동안 안정적인
  UUID identity를 제공해야 한다. 같은 UUID를 나타내는 alias가 equals로 묶이면 같은 handle을
  공유할 수 있으며, bridge가 handle UUID를 검증하므로 다른 월드/플레이어 handle이 섞이지 않는다.
- 실제 `org.bukkit.craftbukkit.CraftWorld`/`CraftPlayer`/`CraftEntity`는 상속된 `getHandle()`을
  직접 사용하고 같은 캐시/epoch 규칙을 적용한다. Ferrite facade는 provider 경로를 사용한다.
  둘 다 아니면 `provider_not_installed`로 실패한다.

### 스레드 semantics

- provider/cache/generation 상태는 하나의 private lock으로 보호한다. provider 호출은 lock 밖에서
  수행하므로 느린 host snapshot 읽기가 다른 종류의 resolve나 install/uninstall을 막지 않는다.
- 같은 facade에 대한 concurrent miss는 provider를 두 번 이상 호출할 수 있다. 각 결과를 UUID
  검증한 뒤 lock 안에서 다시 cache를 확인하고, 먼저 들어간 하나만 canonical로 저장한다. 모든
  호출자는 그 canonical 인스턴스를 받는다. provider는 thread-safe여야 한다.
- `install`/`uninstall`/`close`는 generation을 증가시키고 cache를 비운다. resolve는 시작 시점의
  generation을 기억한다. uninstall/close와 겹친 resolve가 늦게 끝나면
  `bridge_uninstalled`로 실패하고 새 generation의 cache를 오염시키지 않는다. uninstall 전에
  cache write/return을 마친 resolve는 uninstall 이전에 linearize된 것으로 보고 handle을 반환할
  수 있다. 이미 반환된 handle은 detached snapshot이라 uninstall 후에도 읽을 수 있지만, 새 bridge
  lookup은 다시 install해야 한다.
- `install`은 한 provider만 허용한다. 같은 인스턴스를 다시 install하는 것은 idempotent no-op,
  다른 인스턴스는 `provider_already_installed`다. `uninstall(expected)`는 설치된 provider가
  없으면 no-op, 다른 provider면 `provider_owner_mismatch`로 실패하고 현재 provider를 유지한다.
  `close()`는 몇 번 호출해도 안전하며 provider-specific close hook은 호출하지 않는다(SPI에 없음).

### Lane 2가 구현할 것

1. `ferrite-api-impl`에 `ferrite-nms` 의존성을 추가한다. 방향은
   `ferrite-api-impl -> ferrite-nms` 단방향이며, `ferrite-nms`는 Lane 2 구현 클래스를 import하지
   않는다.
2. 공개 facade 타입에 대한 adapter에서 `NmsHandleProvider`의 세 메서드를 구현한다. world는
   `world.getUID()`, player/entity는 `getUniqueId()`로 UUID를 읽고 각각
   `ServerLevel.of(uuid)`, `ServerPlayer.of(uuid)`, `Entity.of(uuid)`를 반환한다. facade 종류가
   아니면 `NmsBridgeException("unknown_world"|"unknown_player"|"unknown_entity", ...)`로
   거부한다.
3. adapter는 `FerriteWorld`/`FerritePlayer`의 final 상속 계층을 바꾸지 않는다. 별도 클래스로
   구현하고, facade concrete type이 public이 아니면 bridge가 impl을 참조하지 않도록 public
   marker/accessor를 Lane 2가 제공한다.
4. provider는 thread-safe하고 stateless하게 유지한다. facade를 field에 저장하거나 handle에
   넣지 않는다. bridge가 canonical cache를 소유하므로 provider에 별도 strong cache를 두지 않는다.
5. `install`/`uninstall`을 resolve 메서드 안에서 호출하지 않는다.

### Lane 1이 할 것

1. `java/settings.gradle.kts`에 `:ferrite-nms`, `:fixtures:nms-plugin`,
   `:fixtures:typical-plugin`을 include하고 Paper repository
   (`https://repo.papermc.io/repository/maven-public/`)를 추가한다. host classpath에는
   `ferrite-nms`와 Lane 2의 provider 구현이 함께 있어야 한다.
2. host attach/facade 초기화 뒤, POSTWORLD 플러그인 `onEnable` 전에
   `NmsBridge.install(provider)`를 호출한다. provider가 없거나 설치에 실패하면 startup을
   실패시킨다. duplicate/wrong-owner 예외를 삼키지 않는다.
3. 모든 플러그인 `onDisable` 뒤, host 종료 전에 `finally`에서
   `NmsBridge.uninstall(provider)`를 호출한다. 이어서 `NmsBridge.close()`를 호출해도 된다
   (idempotent). resolve 도중 uninstall이 발생하면 해당 resolve는 `bridge_uninstalled`로
   실패하는 것이 정상이다.
4. embedded integration harness는 fixture를 enable하기 전에 provider를 설치하고, 종료 시
   uninstall한다. NMS fixture의 report가 `ok`가 아니면 integration test는 실패해야 하며,
   provider 부재를 skip으로 바꾸지 않는다.
5. `java/ferrite-nms/build.gradle.kts`의 test task는 이제 `:fixtures:nms-plugin:jar`에 의존한다.
   따라서 settings에 fixture include가 없으면 `:ferrite-nms:test` configuration부터 실패한다.
   이는 의도한 Lane 1 blocker다.

### fixture / integration 갱신

- `NmsFixturePlugin`은 `((CraftWorld) world).getHandle()` / `((CraftPlayer) player).getHandle()`
  cast를 제거하고 `NmsBridge.level(world)` / `NmsBridge.player(player)`를 사용한다. 시작 시
  `NmsBridge.isInstalled()`를 확인하므로 provider가 없으면 `onEnable`이 명시적으로 실패한다.
  player가 있으면 `NmsBridge.entity(player) == playerHandle`도 확인해 canonical player handle을
  검증한다.
- `RealHostIntegrationTest`는 Lane 1 host jar 또는 Lane 2 typical jar가 없을 때만 skip한다.
  NMS fixture jar 누락과 Lane 1 harness seam 누락은 assertion failure다. host가 provider를
  설치하지 않으면 NMS fixture report가 실패하므로 테스트도 실패한다.
- 여전히 실제 임베디드 JVM 통합은 실행되지 않았다. 이 워크트리에는 Lane 1 host jar/harness와
  Lane 2 typical jar가 없다.

### 아직 불가능한 것

- `(org.bukkit.craftbukkit.CraftWorld) ferriteWorld`와
  `(org.bukkit.craftbukkit.entity.CraftPlayer) ferritePlayer` cast는 여전히 실패한다. 이 bridge는
  선택된 NMS fixture/플러그인이 cast 없이 handle을 얻는 명시적 진입점이며, CraftBukkit 클래스
  계층을 흉내내지 않는다. 서드파티 플러그인의 cast 호환은 Lane 2의
  `UnsupportedWorldBase`/`UnsupportedPlayerBase` 계층을 바꾸는 별도 shared-bridge 단계가 필요하다.
- live entity/player/world 객체, Rust 포인터, `JNIEnv`, 전체 `net.minecraft` 필드는 노출하지 않는다.
- Lane 2 provider 구현과 real-host 통합 증거는 아직 없다.

### 검증 (2026-11-16)

- 임시 harness:
  `gradle -p /tmp/ferrite-nms-harness :ferrite-nms:test :fixtures:nms-plugin:jar --no-daemon --console=plain`
  → **BUILD SUCCESSFUL**. 결과 XML 집계: **tests=47, passed=46, skipped=1, failures=0, errors=0**.
  기존 34개 테스트는 모두 passed, 새 `NmsBridgeTest` 12개는 모두 passed, `RealHostIntegrationTest`
  1개만 Lane 1/2 산출물 부재로 skipped.
- fixture jar `java/fixtures/nms-plugin/build/libs/nms-plugin.jar` 생성/UP-TO-DATE 확인.
- 실제 루트: `cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar --no-daemon`
  → **exit 127**, `./gradlew: No such file or directory`. Lane 1의 `java/settings.gradle.kts`와
  wrapper가 이 워크트리에 없어 실행 불가; Lane 1 산출물이 들어온 뒤 같은 명령을 재실행해야 한다.
- `git diff --check` → clean (exit 0). 커밋/푸시 없음.
