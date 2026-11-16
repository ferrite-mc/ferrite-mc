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

## 계약 v2 제안 — state surface (Lane 3 → Lane 1)

상태: **Lane 1 v2 첫 수직 슬라이스 구현 완료**. `CONTRACT_VERSION`은 2이고 v1 op/이벤트는
하위호환으로 유지한다. 아래 초안의 `state.*` 이름은 Lane 1이 채택한 canonical 이름
(`world.block_get`, `world.block_set_async`, `world.chunk_loaded`, `player.state_get`,
`entity.state_get`)으로 대체되었다. 아직 `player.state_get`/`entity.state_get`와 write 완료
조회는 후속이다.

### 공통 규칙

- 전송/envelope/오류 코드는 v1의 `HostBridge.call`과 동일하다. 성공은
  `{"ok":true,"result":...}`, 실패는 `{"ok":false,"error":{"code","message"}}`다.
- UUID는 소문자 하이픈 문자열, world는 `minecraft:overworld` 같은 키 문자열이다. 정수는
  `i64`, 실수는 `f64`이며 64비트 범위를 넘는 값을 보내지 않는다.
- 반환 스냅샷은 불변/방어적 복사본이다. `ServerLevel`/`ServerPlayer`/JNI 포인터를 JSON에
  넣지 않는다.
- **읽기** op는 coordinator/owner region에서 스냅샷을 만든 뒤 Java 호출자에게 돌려준다.
  Java 호출자는 `call_timeout`까지만 기다리며, 리전 스레드가 Java를 기다리는 경로는 없다.
- **쓰기** op는 owner region 큐에 넣고 즉시 `{"queued":true,"request_id":N}`을 돌려준다.
  완료는 `state.request.status`로 확인한다. Java가 리전 tick을 동기 대기하지 않는다.
- `request_id`는 호스트 수명 동안 단조 증가하는 양의 정수다. 큐가 가득 차면 조용히 버리지
  않고 `queue_full`을 낸다. 완료/취소/TTL 만료 뒤의 상태 조회는 `unknown_request`다.
- v1의 `timeout` 한계(호출 취소 불가)는 state op에도 적용된다. 쓰기 요청은 타임아웃 뒤에도
  적용될 수 있으므로 `request_id`와 `stale_request`를 함께 기록한다.

### 1. World snapshot — `state.world.snapshot`

요청(정확히 하나의 식별자):

```json
{"world":"minecraft:overworld"}
```

응답:

```json
{
  "world": {
    "id": "minecraft:overworld",
    "uuid": "00000000-0000-0000-0000-000000000000",
    "environment": "normal",
    "min_y": -64,
    "height": 384,
    "seed": 0,
    "time": 12345,
    "spawn": {"x": 0, "y": 64, "z": 0},
    "loaded": true
  }
}
```

- 스레드: coordinator/소유 월드의 읽기 스냅샷. Java는 월드 tick을 기다리지 않는다.
- 오류: `invalid_request`, `unknown_world`, `not_loaded`, `timeout`, `shutdown`.
- `loaded=false`는 오류가 아니라 "현재 로드되지 않은 런타임 월드"를 뜻하며, 블록 읽기는 별도
  `chunk_not_loaded`로 거부한다.

### 2. Block read / async write

#### `state.block.read`

요청:

```json
{"world":"minecraft:overworld","x":0,"y":64,"z":0}
```

응답:

```json
{"state_id":1,"material":"minecraft:stone","properties":{},"loaded":true}
```

- 스레드: 해당 청크를 소유한 리전의 읽기 스냅샷(없으면 coordinator 경유).
- 오류: `invalid_request`, `unknown_world`, `chunk_not_loaded`, `timeout`, `shutdown`.
- `state_id`는 `state.registry.block_state`가 정의하는 전역 block-state id다. material/properties는
  디버깅과 NMS `BlockState` 표시용이며, 쓰기 권위는 `state_id`다.

#### `state.block.write`

요청:

```json
{
  "world":"minecraft:overworld",
  "x":0,"y":64,"z":0,
  "state_id":1,
  "expected_state_id":0
}
```

`expected_state_id`는 선택 사항인 compare-and-set이다.

응답:

```json
{"queued":true,"request_id":1}
```

- 스레드: owner region 큐에 넣고 **즉시** 반환한다. Java/플러그인 스레드가 리전을 기다리지
  않는다. 적용 결과는 `state.request.status`로 확인한다.
- 오류: `invalid_request`, `unknown_world`, `unknown_block_state`, `chunk_not_loaded`,
  `queue_full`, `stale_request`, `shutdown`.
- `expected_state_id`가 현재 값과 다르면 적용하지 않고 완료 상태를 `failed`/`stale`로 만든다.

### 3. Registry block-state id — `state.registry.block_state`

요청은 `id` 또는 `material`+`properties` 중 정확히 하나다.

```json
{"id":1}
```

```json
{"material":"minecraft:stone","properties":{"facing":"north"}}
```

응답:

```json
{
  "state_id": 1,
  "material": "minecraft:stone",
  "properties": {"facing": "north"},
  "data_version": 4771,
  "legacy": false
}
```

- 스레드: 어느 Java 호스트 스레드에서나 읽을 수 있는 불변 registry 스냅샷. 월드/리전 상태를
  만지지 않는다.
- 오류: `invalid_request`, `unknown_block_state`, `shutdown`.
- 26.3 데이터에 없는 material/property/id는 공기/빈 값으로 대체하지 않고 오류로 끝낸다.
  `data_version`은 26.3의 값을 쓰며, legacy 변환은 v2 첫 슬라이스에서 지원하지 않는다.

### 4. Player / entity snapshot

#### `state.player.snapshot`

```json
{"uuid":"00000000-0000-0000-0000-000000000001"}
```

```json
{
  "uuid":"00000000-0000-0000-0000-000000000001",
  "name":"Alice",
  "world":"minecraft:overworld",
  "x":0.5,"y":64.0,"z":0.5,"yaw":0.0,"pitch":0.0,
  "game_mode":"survival",
  "health":20.0,"food":20,"saturation":5.0,
  "online":true
}
```

- 스레드: coordinator/online 플레이어의 읽기 스냅샷.
- 오류: `invalid_request`, `player_not_found`, `timeout`, `shutdown`.

#### `state.entity.snapshot`

```json
{"uuid":"00000000-0000-0000-0000-000000000002"}
```

```json
{
  "uuid":"00000000-0000-0000-0000-000000000002",
  "type":"minecraft:zombie",
  "world":"minecraft:overworld",
  "x":0.5,"y":64.0,"z":0.5,"yaw":0.0,"pitch":0.0,
  "velocity":[0.0,0.0,0.0],
  "passengers":[],
  "vehicle":null
}
```

- 스레드: 엔티티를 소유한 리전의 읽기 스냅샷. 다른 리전의 라이브 상태를 직접 읽지 않는다.
- 오류: `invalid_request`, `entity_not_found`, `chunk_not_loaded`, `timeout`, `shutdown`.

### 5. Async player writes

세 op 모두 owner region 큐에 넣고 `{"queued":true,"request_id":N}`을 돌려준다. 완료는
`state.request.status`로 확인한다. Java/플러그인 스레드는 리전 tick을 기다리지 않는다.

| op | 요청 | 오류 |
|---|---|---|
| `state.player.teleport` | `{"uuid":"...","world":"...","x":0.5,"y":64.0,"z":0.5,"yaw":0.0,"pitch":0.0}` | `invalid_request`, `player_not_found`, `unknown_world`, `queue_full`, `stale_request`, `shutdown` |
| `state.player.game_mode` | `{"uuid":"...","game_mode":"creative"}` | `invalid_request`, `player_not_found`, `queue_full`, `stale_request`, `shutdown` |
| `state.player.health` | `{"uuid":"...","health":20.0,"food":20,"saturation":5.0}` (각 필드 선택) | `invalid_request`, `player_not_found`, `queue_full`, `stale_request`, `shutdown` |

- 허용 `game_mode`: `survival`, `creative`, `adventure`, `spectator`.
- `health`는 `0.0..=max_health`, `food`는 `0..=20`, `saturation`은 `0.0..=food` 범위로
  검증한다. 범위 밖 값은 clamp하지 않고 `invalid_request`다.
- 플레이어가 큐 처리 전에 로그아웃하면 완료 상태는 `failed`/`player_not_found`이며, 서버
  tick이나 다른 플레이어에게 영향을 주지 않는다.

### 6. Async completion — `state.request.status`

```json
{"request_id":1}
```

```json
{
  "state": "pending",
  "result": null,
  "error": null
}
```

`state`는 `pending`, `applied`, `failed`, `cancelled` 중 하나다. `failed`/`cancelled`에는
`error` 객체가 있고, `applied`에는 op별 결과(예: block write의 최종 `state_id`)가 있다.

- 스레드: 어느 Java 호스트 스레드에서나 읽을 수 있는 완료 큐 조회. 월드/리전 상태를 만지지
  않는다.
- 오류: `invalid_request`, `unknown_request`(TTL 만료/이미 회수), `shutdown`.
- 호스트는 완료된 request를 최소 5분 또는 다음 월드 저장까지 보관한다. TTL/상한은 Lane 1이
  정하고 로그에 남긴다. 완료를 `null`/성공으로 숨기지 않는다.

### 7. 오류/스레드 요약

| 코드 | 의미 |
|---|---|
| `unknown_world` | world 키/uuid를 모른다 |
| `unknown_block_state` | registry에 없는 block-state id/material/properties |
| `chunk_not_loaded` | 요청한 블록의 청크가 로드되지 않았다 |
| `player_not_found` | online 플레이어가 아니다 |
| `entity_not_found` | 엔티티가 없거나 아직 로드되지 않았다 |
| `queue_full` | owner region 쓰기 큐가 가득 찼다 |
| `stale_request` | compare-and-set/텔레포트 대상 상태가 바뀌었다 |
| `unknown_request` | 완료 큐에 없는 request id |
| `timeout` | 스냅샷/조회가 call timeout을 넘겼다 |
| `shutdown` | 호스트 종료 중이다 |

| 경로 | 실행 스레드 | Java 대기 |
|---|---|---|
| world/block/player/entity snapshot, registry | coordinator 또는 owner region의 스냅샷 | call timeout까지만 |
| block write, teleport, game mode, health | owner region 큐 | 없음(즉시 request id) |
| request status | 호스트 완료 큐 | 없음 |
| Java→Rust native 콜백 | Java 호출 스레드 | 재진입 없음 |

v2는 v1 op/이벤트를 제거하거나 의미를 바꾸지 않는다. 수용 시 `CONTRACT_VERSION=2`와 별도
버전 협상 필드를 추가하고, Lane 1이 Rust/Java 구현과 테스트를 소유한다.

## CraftWorld/CraftPlayer cast 블로커 — 최소 인터페이스 제안

Lane 2의 `CraftWorld#getHandle()`/`CraftPlayer#getHandle()`은 `ServerLevel`/`ServerPlayer`
래퍼를 돌려주고, NMS 쪽 `getWorld()`/`getBukkitEntity()`는 같은 facade 객체를 다시 가리켜야
한다. 호출마다 새 래퍼를 만들거나 Lane 2/3이 각자 캐시하면 `(CraftWorld) world`,
`(CraftPlayer) player`, identity 비교에서 `ClassCastException`/불일치가 난다.

### 제안: facade가 구현하는 `NmsHandleProvider` + canonical weak cache

`ferrite-nms`에 다음 최소 SPI를 둔다(타입은 26.3 NMS 래퍼, 실제 이름은 Lane 1/2 합의 후 확정).

```java
public interface NmsHandleProvider {
    ServerLevel serverLevel(org.bukkit.World world);
    ServerPlayer serverPlayer(org.bukkit.entity.Player player);
    Entity entity(org.bukkit.entity.Entity entity);
    void release(Object handle);
}
```

- Lane 2 facade가 이 인터페이스를 구현해 host 시작 시 `NmsHandles.install(provider)`로 등록한다.
  `CraftWorld#getHandle()`/`CraftPlayer#getHandle()`은 `NmsHandles.world(this)`/
  `NmsHandles.player(this)`만 호출한다.
- `NmsHandles`는 Bukkit 객체를 weak identity key로 캐시해 한 객체에 하나의 canonical 래퍼만
  돌려준다. 래퍼는 원래 Bukkit 객체를 참조해 역방향 cast/identity를 보존한다. plugin disable/
  host shutdown에서 `release`와 캐시 정리를 한다.
- 이 배치는 `ferrite-api-impl -> ferrite-nms -> ferrite-host + Paper API` 단방향 의존을 만든다.
  `ferrite-nms`는 `ferrite-api-impl`을 참조하지 않으므로 순환 의존이 없다. 공유 bridge에
  `Object` 반환 인터페이스를 두는 대안은 `getHandle()`의 컴파일 타임 반환형 때문에 cast/reflection이
  필요해 최소안에서 제외한다.
- **실제 인터페이스/등록 코드는 아직 만들지 않는다.** Lane 1이 host 수명/클래스로더 경계를,
  Lane 2가 facade 초기화 순서를 합의한 뒤 구현한다. 그 전까지 Lane 3은 이 SPI를 가정한 문서와
  통합 테스트 skeleton만 유지한다.

## Real-host integration skeleton

`crates/ferrite-server/tests/plugin_host_integration_skeleton.rs`에 typical + NMS fixture를
같은 `PluginHost`/JVM에 로드하는 skeleton을 추가했다. 현재는 fixture jar가 없으므로
`#[ignore]`이며, 다음 환경 변수를 제공한 뒤 `cargo test -p ferrite-server --test
plugin_host_integration_skeleton -- --ignored`로 실행한다.

- `FERRITE_HOST_JAR`, `FERRITE_API_IMPL_JAR`, `FERRITE_API_IMPL_CLASS`
- `FERRITE_TYPICAL_PLUGIN_JAR`, `FERRITE_NMS_PLUGIN_JAR`
- 선택: `FERRITE_TYPICAL_NAME`(기본 `Typical`), `FERRITE_NMS_NAME`(기본 `NMS`),
  `FERRITE_JAVA_HOME`

skeleton은 두 jar를 임시 plugins 디렉터리에 복사해 한 번에 스캔하고, `load_postworld` 뒤 두
이름이 모두 loaded인지, `player_join`과 `tick`이 예외 없이 도는지, shutdown이 정상인지
확인한다. 실제 fixture 구현이 들어오면 이 skeleton을 활성 테스트로 승격하고, 같은 JVM에서
`NoSuchMethodError`/`ClassCastException`이 없는지 assertion을 추가한다.

### 현재 워크트리 실행 결과 (2026-11-16, blocked)

이 워크트리에는 Lane 2/3 구현이 아직 없다. `java/ferrite-nms/`와 `java/ferrite-api-impl/`에는
`.gitkeep`만 있고, `java/fixtures/` 디렉터리도 없다. 따라서 다음 명령은 **실패**한다.

```sh
cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar --no-daemon
# FAILURE: Cannot locate tasks that match ':ferrite-nms:test'
# task 'test' not found in project ':ferrite-nms'
```

현재 확인 가능한 Java 테스트는 `ferrite-host`의 14개이며(5개 테스트 클래스), 요청된 34개
전체 테스트와 NMS/typical fixture 통합은 Lane 2/3 산출물이 이 워크트리에 들어온 뒤에만
검증할 수 있다. 이 워크트리에는 NMS 모듈이 없으므로 위 `:ferrite-nms:test` 실패는 그대로다.
Rust 쪽 v2 슬라이스 테스트는 통과했지만, NMS 통합을 통과로 보고하지 않는다.

## Lane 1 v2 harness seam (Lane 3이 reflection으로 호출할 계약)

클래스와 시그니처는 이제 고정이다.

```java
dev.ferrite.host.integration.HostIntegrationHarness
public static HostIntegrationHarness start(java.nio.file.Path pluginsDir,
                                            java.nio.file.Path dataDir) throws Exception
public java.util.List<java.util.Map<String, Object>> reports()
public void close()
```

- `start`는 Rust `PluginHost`가 아니라 Java `Bootstrap.start`를 호출하고, `pluginsDir`의 각
  `.jar`에서 top-level `plugin.yml`/`paper-plugin.yml`의 `name`, `version`, `main`을 읽어
  `Bootstrap.loadPlugin`로 로드한다. jar에는 이 세 필드가 필요하다.
- `reports()`의 각 map은 최소 `jar`, `name`, `version`, `main`, `enabled`를 가지며 실패 시
  `error` 문자열을 가진다. 반환 list/map은 읽기 전용이다.
- `close()`는 멱등이며 Java host를 shutdown한다. 테스트마다 새 `start` 전에 이전 harness를
  닫아야 한다(JVM의 host singleton 때문).
- reflection 예시:

```java
Class<?> type = Class.forName("dev.ferrite.host.integration.HostIntegrationHarness");
Object harness = type.getMethod("start", Path.class, Path.class).invoke(null, plugins, data);
try {
    List<Map<String, Object>> reports =
            (List<Map<String, Object>>) type.getMethod("reports").invoke(harness);
} finally {
    type.getMethod("close").invoke(harness);
}
```

- 이 harness는 실제 NMS/CraftBukkit surface나 Rust 월드 provider를 만들지 않는다. NMS fixture는
  Lane 2 facade와 같은 JVM에서 로드되는지만 검증하며, 실제 월드 read/write는 Rust
  `PluginHost` + `HostStateProvider` 경로에서 별도로 검증해야 한다.

### Canonical v2 state ops

| op | 구현 상태 |
|---|---|
| `world.block_get` | 구현됨: 실제 dimension source read, 없으면 `chunk_not_loaded` |
| `world.block_set_async` | 구현됨: 검증 후 큐 접수; 게임 루프 tick에서 적용 |
| `world.chunk_loaded` | 구현됨: resident chunk 조회 |
| `player.state_get` | 후보; 기본 provider `unsupported` |
| `entity.state_get` | 후보; 기본 provider `unsupported` |

`world.block_set_async`의 `queued:true`는 적용 완료가 아니다. 완료 상태 조회 op와 NMS 쓰기
future는 Lane 1 후속 작업/Lane 3 소비 구현에서 추가해야 한다. `host.info.capabilities.v2_ops`는
provider가 실제 광고한 op만 포함한다.

### Lane 3 후속 작업

1. `java/fixtures/typical-plugin`, `java/fixtures/nms-plugin`, `java/ferrite-nms`의 실제 build를
   이 워크트리에 추가하고 `:ferrite-nms:test :fixtures:nms-plugin:jar`를 실행한다.
2. harness reflection으로 typical+NMS jar를 한 JVM에 로드하고 `reports()`의 `enabled=true`를
   확인한다. 현재 NMS 모듈이 없어 이 테스트는 아직 skip 상태다.
3. 실제 world/block 왕복은 Rust `WorldStateProvider`와 `world.block_set_async`의 queue drain을
   사용한다. NMS `CraftWorld` 래퍼는 이 op만 호출하고 리전 tick을 동기 대기하지 않는다.
4. 완료 조회/텔레포트/player/entity는 아직 계약 후속이며, 구현 전에는 `unsupported`를 명시적
   예외로 노출한다.
