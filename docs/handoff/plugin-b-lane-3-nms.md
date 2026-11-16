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
