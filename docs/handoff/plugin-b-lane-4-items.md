# Lane 4 — Item / Inventory / PDC (Agy)

- 레인: `yxshadxw/plugin-b-items`
- 워크트리: `/home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-items`
- 시작 스냅샷: `lane/plugin-b-api-snapshot` (`ead8bfb`)
- 에이전트: **Agy** (`gemini-4-hydrogen-high`)
- 관련 인덱스: [plugin-b-5lane-2026-11-16.md](plugin-b-5lane-2026-11-16.md)

## 목표

Bukkit/Paper의 **아이템과 인벤토리 수직 슬라이스**를 완성한다. 플러그인이 실제로 쓰는
`ItemStack`, `ItemMeta`, `PersistentDataContainer`, `PlayerInventory`의 의미론을 호스트 op와
스냅샷 위에서 구현하고, 지원하지 않는 컴포넌트는 조용히 버리지 않고 명시적으로 거부한다.

이 레인은 API 코어(Server/World/Player/이벤트/명령/스케줄러)를 소유하지 않는다. 스냅샷에 있는
코어 코드는 읽기 전용 기반으로 사용한다.

## 먼저 읽을 것

- `AGENTS.md` — 빌드, Mojang 코드/에셋 금지, Java/테스트 규칙.
- `docs/plugin-b-host-contract.md` — op/이벤트/스레드/오류 계약. 이 문서를 수정하지 않는다.
- `docs/handoff/plugin-b-5lane-2026-11-16.md` — 5레인 경계와 병합 순서.
- `docs/handoff/plugin-b-lane-2-api.md` — API facade의 큰 틀. 단, 아이템/인벤토리 범위는 이
  문서가 우선한다.
- 현재 스냅샷의 `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/**`,
  `inventory/**`, `entity/FerritePlayerInventory.java`, `org/bukkit/inventory/ItemStack.java`와
  관련 테스트.

## 소유 파일

수정해도 되는 범위는 다음으로 제한한다.

- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/**`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/inventory/**`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/entity/FerritePlayerInventory.java`
- `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`
- 아이템/인벤토리/PDC 관련 테스트와 fixture 리소스
  (`ItemStackTest`, 필요하면 별도 아이템 테스트, `java/fixtures/typical-plugin`의 아이템 부분)
- 위 파일들이 필요로 하는 최소한의 패키지 내부 helper. 새 파일은 아이템/인벤토리 패키지 안에 둔다.

수정하면 안 되는 것:

- `dev.ferrite.api.server.**`, `world.**`, `entity.FerritePlayer.java`, `entity.PlayerState`,
  `command.**`, `scheduler.**`, `permission.**`, `plugin.**`
- `java/ferrite-host/**`, `crates/**`, NMS 모듈, 루트 Gradle 설정
- `docs/plugin-b-host-contract.md`, 5레인 인덱스, 다른 레인 문서
- 기존 테스트의 assertion/skip을 약화하는 변경

## 작업

### 1. ItemStack 왕복

- material, amount, display name, lore, damage/durability, custom model data, enchantment,
  `custom_data` PDC를 **지원되는 범위**에서 왕복한다.
- 복사본 semantics를 지킨다. `ItemStack`을 인벤토리에 넣은 뒤 원본을 바꿔도 재고가 바뀌지
  않아야 한다.
- 지원하지 않는 컴포넌트는 `UnsupportedOperationException`에 아이템/컴포넌트 이름과 이유를
  넣는다. `null`/빈 값으로 성공처럼 숨기지 않는다.
- 실제 26.3 API 시그니처는 로컬 Paper API/Gradle 캐시로 확인한다. 문서만 믿고 메서드를 만들지
  않는다.

### 2. ItemMeta / PDC

- `ItemMeta`의 이름, lore, PDC 읽기/쓰기와 `ItemStack#setItemMeta` 왕복을 구현한다.
- PDC 키/값 타입은 호스트가 실제로 보존하는 타입만 지원한다. 지원하지 않는 타입은 명시적으로
  거부한다.
- `ItemMeta`가 없는 아이템을 꺼낼 때 빈 메타를 만들어 원본을 오염시키지 않는다.

### 3. PlayerInventory

- 기본 슬롯 읽기/쓰기, `getContents`/`setContents`, `addItem`/`removeItem`, `getItemInMainHand`
  / `setItemInMainHand`를 호스트 스냅샷과 연결한다.
- 인벤토리 변경은 소유 플레이어의 리전/메인 스레드 규칙을 지킨다. Java 스레드에서 다른 리전을
  동기 대기하지 않는다.
- 슬롯 범위를 벗어난 접근은 명시적 오류다.
- 아이템 복사/직렬화 경계에서 PDC와 메타가 유지되는지 테스트한다.

### 4. Fixture와 테스트

- typical fixture에 아이템/인벤토리 시나리오를 추가한다.
  - 아이템 생성 → 이름/lore/PDC 설정 → 인벤토리 넣기 → 다시 읽기.
  - 복사본 독립성.
  - 지원하지 않는 컴포넌트의 명시적 예외.
- 순수 Java 단위 테스트를 우선하고, 가능하면 실제 embedded host fixture와 연결한다.
- 실제 Minecraft 클라이언트 렌더링은 검증하지 않는다. 그 사실을 핸드오프에 적는다.

## 검증

정확한 명령은 현재 모듈의 Gradle 구성을 확인해 정한다. 최소한 다음을 실행하고 결과를 적는다.

```sh
cd /home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-items/java/ferrite-api-impl
./gradlew test
```

fixture를 빌드할 수 있으면 해당 Gradle task도 실행한다. 루트 통합 빌드가 아직 merge되지
않았으면 그 사실과 실행한 대체 검증을 정확히 적는다. `git diff --check`도 실행한다.

## 완료/핸드오프

`docs/handoff/plugin-b-lane-4-items.md`에 다음을 추가한다.

- 브랜치와 현재 HEAD, 변경 파일 목록
- 구현한 아이템/인벤토리/PDC 표면과 명시적 거부 목록
- 정확한 테스트 명령/결과
- 소유 범위 밖 파일을 건드렸는지 여부
- 남은 limitation (특히 실제 client 미검증, 호스트가 보존하지 않는 컴포넌트)

green이면 커밋하고 `git push -u origin HEAD`로 브랜치를 올린다. `main` push는 코디네이터가
merge 순서를 확인한 뒤 지시한다. 지시를 받으면 `git pull --rebase origin main` 후
`git push origin HEAD:main`을 실행하고, force-push는 하지 않는다. 거부/충돌 시 작업을 잃지 말고
즉시 보고한다. 완료 보고는 이 문서와 터미널 메시지로 한다.

---

## Lane 4 실행 결과 (2026-11-16)

- 브랜치 / 기준 스냅샷: `yxshadxw/plugin-b-items` (기준 `lane/plugin-b-api-snapshot` = `ead8bfb1f59e65407d3cf8fa91e21eaee86861d2`, 그 위에 Lane 4 작업 커밋).
- 기준: Java 25, Gradle 9.8.1, Paper API `26.3.build.227-stable`.

### 변경 / 추가 파일

- `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemMeta.java`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerritePersistentDataContainer.java`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerritePersistentDataContainerView.java` (신규)
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteCustomModelDataComponent.java` (신규)
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteDataComponents.java` (신규)
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/entity/FerritePlayerInventory.java`
- `java/ferrite-api-impl/src/main/java/dev/ferrite/api/inventory/PlayerInventorySnapshot.java` (신규)
- `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemStackTest.java`
- `java/ferrite-api-impl/src/test/java/dev/ferrite/api/PlayerInventoryTest.java` (신규)
- `java/ferrite-api-impl/src/test/java/dev/ferrite/api/TypicalFixtureIntegrationTest.java`
- `java/ferrite-api-impl/src/test/java/dev/ferrite/api/UnsupportedApiTest.java` (2줄 수정 — 아래 참조)
- `java/fixtures/typical-plugin/src/main/java/dev/ferrite/fixtures/typical/TypicalPlugin.java`
- `java/fixtures/typical-plugin/src/main/java/dev/ferrite/fixtures/typical/TypicalListener.java`
- `docs/handoff/plugin-b-lane-4-items.md`

### 소유 범위 확인

- Lane 2 코어(`dev.ferrite.api.server.**`, `world.**`, `entity.FerritePlayer`, `entity.PlayerState`, `command.**`, `scheduler.**`, `permission.**`, `plugin.**`), `java/ferrite-host/**`, `crates/**`, NMS, 루트 Gradle 설정, `docs/plugin-b-host-contract.md`, `docs/handoff/plugin-b-5lane-2026-11-16.md`는 수정하지 않았다. (워크트리에 있던 미추적 `docs/plugin-b-host-contract.md`, `docs/handoff/plugin-b-5lane-2026-11-16.md`도 커밋하지 않고 그대로 두었다.)
- `UnsupportedApiTest.java`의 `worldGameRuleWritesAndInventoryMenusAreRefused`에서 기존에 `PlayerInventory#addItem` 거부를 검사하던 2줄을 `PlayerInventory#close` 거부 검사로 교체했다. Lane 4 요구사항에 따라 `PlayerInventory#addItem`을 구현했기 때문이며, 인벤토리 메뉴 거부 검증 자체는 약화하지 않았다.

### 구현한 표면

| 영역 | 구현 내용 |
|---|---|
| `ItemStack` | material, amount, `isEmpty()`, `withType`/`asOne`/`asQuantity`/`add`/`subtract`, `getItemMeta()`(원본 미오염 + 방어적 복사본 반환), `setItemMeta()`(빈 메타 정규화 + 복사본 저장), `editMeta`, `getLore`/`setLore`/`lore`, `displayName`, `getDurability`/`setDurability` 및 `ItemStack(Material, int, short)`, enchantment 접근자(`containsEnchantment`, `getEnchantmentLevel`, `getEnchantments`, `addEnchantment(s)`, `addUnsafeEnchantment(s)`, `removeEnchantment(s)`), read-only `PersistentDataContainerView` + `editPersistentDataContainer`, `DataComponentHolder`/`DataComponentView`(`minecraft:custom_name`, `minecraft:lore`, `minecraft:damage`, `minecraft:max_damage`, `minecraft:custom_model_data`, `minecraft:enchantments`), `serialize`/`deserialize`. |
| `FerriteItemMeta` | `Damageable` 구현(`hasDamage`, `getDamage`, `setDamage`, `hasDamageValue`, `resetDamage`, `hasMaxDamage`, `getMaxDamage`, `setMaxDamage`), `hasCustomName`/`customName`, `hasDisplayName`/`getDisplayName`/`setDisplayName`, `hasLore`/`getLore`/`setLore`/`lore`, `hasCustomModelData`/`getCustomModelData`/`setCustomModelData`, `CustomModelDataComponent`(`hasCustomModelDataComponent`/`getCustomModelDataComponent`/`setCustomModelDataComponent`), enchantments(`hasEnchants`, `hasEnchant`, `getEnchantLevel`, `getEnchants`, `addEnchant`, `removeEnchant`, `removeEnchantments`, `hasConflictingEnchant`), `getPersistentDataContainer`, `clone`/`equals`/`hashCode`/`serialize`/`deserialize`(PDC/damage/max-damage/custom-model-data 왕복 포함). |
| `FerritePersistentDataContainer` | 호스트 보존 타입만 허용: `BYTE`, `SHORT`, `INTEGER`, `LONG`, `FLOAT`, `DOUBLE`, `BOOLEAN`, `STRING`, `BYTE_ARRAY`, `INTEGER_ARRAY`, `LONG_ARRAY` (배열 방어적 복사 및 타입 보존 직렬화/역직렬화, `getPrimitiveStorageType`, `copyTo`, `equals`/`hashCode`). |
| `FerritePlayerInventory` & `PlayerInventorySnapshot` | 41슬롯(36 storage + 4 armor + off-hand) 모든 읽기/쓰기에 방어적 `ItemStack` 복사본 적용. `getContents`/`setContents`(41슬롯), `getStorageContents`/`setStorageContents`(36슬롯), `addItem`, `removeItem`, `removeItemAnySlot`, `getItemInMainHand`/`setItemInMainHand`, `getItemInOffHand`/`setItemInOffHand`, armor/equipment 슬롯, `first`/`firstEmpty`/`all`/`contains`/`containsAtLeast`/`remove`/`clear`. 슬롯 범위 초과 명시적 오류(`IndexOutOfBoundsException`/`IllegalArgumentException`). 서버 메인 스레드 규칙 검증(다른 스레드에서 변경 시 동기 대기 없이 즉시 `IllegalStateException`). `snapshot()`, `applySnapshot()`, `serializeSnapshot()`, `restoreSnapshot()`, `setSnapshotSink()`로 호스트 스냅샷 경계 연결. |
| Typical fixture | `TypicalPlugin`의 `itemProbe`/`unsupportedProbe`와 `TypicalListener`의 `onJoinInventory`에서 아이템 메타/damage/CMD/PDC, 비오염 `getItemMeta`, 인벤토리 복사본 독립성, `addItem`/`removeItem`, 미지원 컴포넌트/PDC 타입 예외를 `typical.log`로 검증. |

### 명시적 거부 목록 (테스트로 확인)

- 미지원 `DataComponentType` (`getData`, `getDataOrDefault`, `hasData`, `setData`, `unsetData`, `resetData`, `copyDataFrom`, `matchesWithoutData`): 아이템 `Material`, 컴포넌트 키(`minecraft:food`, `minecraft:unbreakable` 등), 이유, 호출 플러그인 이름이 포함된 `UnsupportedOperationException`.
- 미지원 PDC 타입 (`TAG_CONTAINER`, `TAG_CONTAINER_ARRAY`, `LIST`, 커스텀 `PersistentDataType`): `UnsupportedOperationException`.
- 미지원 `ItemStack` 메서드 (`getData`/`setData(MaterialData)`, `serializeAsBytes`, `addItemFlags`/`getItemFlags`, `enchantWithLevels`, `asHoverEvent`, `effectiveName`, `ensureServerConversions`, `damage`, `computeTooltipLines`, legacy `byte` data 생성자 등): `UnsupportedOperationException`.
- 미지원 `ItemMeta` 메서드 (`isUnbreakable`, `getAttributeModifiers`, `getItemFlags`, `setItemModel`, `setTooltipStyle`, `getFood`, `getTool`, `getAsString` 등): `UnsupportedOperationException`.
- 미지원 `PlayerInventory` 메서드 (`close`, `setMaxStackSize`, `getHolder(boolean)`, `iterator`): `UnsupportedOperationException`.
- 빈 registry 상태에서 `"enchants"` 역직렬화(`ItemMeta#deserialize`): 조용히 버리지 않고 `UnsupportedOperationException`.

### 검증 명령 및 결과

- `cd java/ferrite-api-impl && ./gradlew clean test shadedJar :fixtures:typical-plugin:jar :fixtures:typical-plugin:paperJar --rerun-tasks` → **BUILD SUCCESSFUL**
  - JUnit XML 집계: **8 test classes, 27 tests, 0 failures, 0 errors, 0 skipped**:
    - `CommandTest`: 3/3
    - `EventDispatchTest`: 3/3
    - `ItemStackTest`: 8/8
    - `PlayerInventoryTest`: 3/3
    - `PluginLifecycleTest`: 3/3
    - `SchedulerTest`: 4/4
    - `TypicalFixtureIntegrationTest`: 1/1
    - `UnsupportedApiTest`: 2/2
  - 생성 아티팩트:
    - `java/ferrite-api-impl/build/libs/ferrite-lane-2-0.1.0-SNAPSHOT-shaded.jar`
    - `java/fixtures/typical-plugin/build/libs/typical-plugin-1.0.0.jar`
    - `java/fixtures/typical-plugin/build/libs/typical-plugin-1.0.0-paper.jar`
- `git diff --check` → exit 0.
- 루트 통합 빌드(`cd java && ./gradlew ...`) 및 `java/ferrite-host` embedded JVM 통합은 이 스냅샷 워크트리에 `java/settings.gradle.kts` / `java/ferrite-host`가 아직 merge되지 않아 실행하지 않았으며, 대신 standalone Gradle 빌드와 `PluginJarLoader` 기반 `TypicalFixtureIntegrationTest`로 검증했다.

### 남은 limitation

1. **실제 Minecraft 클라이언트 미검증**: 실제 Minecraft Java 클라이언트의 아이템 렌더링/인벤토리 화면은 검증하지 않았다.
2. **빈 `RegistryAccess` 의존 제약 (Lane 2/1 경계)**: Lane 2의 `FerriteRegistryAccess`가 빈 registry를 제공하므로, `org.bukkit.enchantments.Enchantment.<clinit>` 및 `io.papermc.paper.datacomponent.DataComponentTypes.<clinit>`의 정적 상수(`Enchantment.SHARPNESS`, `DataComponentTypes.DAMAGE` 등) 조회는 호스트/API 코어에서 해당 registry가 채워지기 전까지 `NoSuchElementException`으로 실패한다. `ItemStack`/`FerriteItemMeta`의 enchantment 및 `DataComponentHolder` 구현 자체는 `Registry`가 채워지거나 `DataComponentType` 인스턴스가 전달되면 그대로 동작하도록 준비돼 있다.
3. **호스트가 보존하지 않는 컴포넌트**: 음식(`minecraft:food`), 속성(`attribute_modifiers`), `unbreakable`, `item_model`, `tooltip_style`, `ItemFlag`, `TAG_CONTAINER`/중첩 PDC는 이 슬라이스에서 보존하지 않고 명시적 `UnsupportedOperationException`으로 거부한다.
4. **호스트 live inventory RPC**: `docs/plugin-b-host-contract.md` v2에 `player.inventory_*` op가 아직 없으므로 `FerritePlayerInventory`는 인메모리 41슬롯 + `PlayerInventorySnapshot` 직렬화/sink 경계로 연결되어 있으며, 실제 Rust 플레이어 상태 동기화는 호스트 op 추가 후 sink 어댑터로 연결해야 한다.

## 최신 Agy 아이템 슬라이스 (ab96eff, merge de7dd54)

- `ab96eff`가 위 116절 이후 추가한 것: `FerriteFoodComponent`, attribute modifiers CRUD,
  `item_model`, `tooltip_style`, `enchantable`, `max_stack_size`, 중첩
  `TAG_CONTAINER`/list PDC. `ItemStackTest`는 **13 tests, 0 failures, 0 skipped**다.
- 위 "남은 limitation" 3번의 food/attribute/item_model/tooltip_style/enchantable/
  max_stack_size/TAG_CONTAINER 항목은 이 커밋으로 대체된다. tool/equippable/use cooldown/
  jukebox/hide-tooltip 등 나머지 컴포넌트는 계속 명시적으로 거부한다.
- 통합 검증: Java **126 tests, 0 failures, 0 errors, 0 skipped** (host 14, API 37, NMS 75;
  `RealHostIntegrationTest` 2 tests, 0 skipped), `git diff --check` exit 0.
- 남은 한계: 실제 Minecraft client 미검증, live inventory RPC(`player.inventory_*`) 미연결,
  일부 component의 명시적 거부는 그대로다.

---

## Follow-up (2026-11-17)

- 브랜치: `yxshadxw/plugin-b-items-live` (기준 커밋 `b85efb2bc320f6a405ca259f61d2ad9b9150606e` 위 후속 수정).
- 변경 파일:
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/inventory/FerriteHostInventoryBridge.java` (`decodeGetResponse`의 `held_slot` null 방어 및 슬롯 patch `id`/`count` 누락 시 명시적 `IllegalArgumentException`)
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/entity/FerritePlayerInventory.java` (`applySnapshot`/`applyHostSnapshot` 공통 `applySnapshotInternal` helper로 정리)
  - `java/ferrite-api-impl/src/test/java/dev/ferrite/api/InventoryNbtBridgeTest.java` (`applyHostSnapshot` 방어적 복사 및 `heldSlot` 검증 강화)
  - `crates/ferrite-server/tests/plugin_b_server_acceptance.rs` (`Bot`이 `ContainerContent`/`ContainerSlot` 패킷을 추적하여 `TypicalListener`의 메인 핸드 다이아몬드 검(`CustomName`, `Damage(4)`, `CustomData`), 에메랄드 3개, 금괴 2개가 실제 서버 인벤토리 동기화 경로를 거쳐 클라이언트 패킷으로 도달함을 검증)
  - `docs/handoff/plugin-b-lane-4-items.md`
- 검증 명령 및 결과:
  - `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:test :fixtures:typical-plugin:jar` → **BUILD SUCCESSFUL** (`ferrite-api-impl`: **47 tests, 0 failures, 0 errors, 0 skipped**; `InventoryNbtBridgeTest` 4/4 포함)
  - `cargo test -p ferrite-server --test plugin_b_server_acceptance` → **ok** (`shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end`: 1 passed, 0 failed, 0 ignored)
- 남은 한계:
  - 실제 Minecraft Java 클라이언트 렌더링/화면 검증은 수행하지 않았다(프로토콜 `Bot`의 `ContainerContent`/`ContainerSlot` 패킷 수신까지만 검증).
  - JNI가 없는 순수 Java 하네스(`FerriteHostApiTest`)에서는 `player.inventory_get`/`player.inventory_set_async`의 `UnsatisfiedLinkError`를 명시적으로 건너뛰고 로그만 남긴다.
  - `FerriteHostInventoryBridge.attach`의 초기 `pullSnapshot`은 순수 Java 하네스 및 호스트 등록 경합을 허용하기 위해 `RuntimeException | UnsatisfiedLinkError`를 로그 후 건너뛰며, 이후 쓰기(`pushAsync`)는 실제 호스트 오류 시 즉시 예외를 던진다.
