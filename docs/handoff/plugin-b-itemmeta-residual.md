# Plugin B — ItemMeta Residual Closure (`can_break`, `can_place_on`, `Equippable.Builder`, `max_stack_size`)

- 브랜치: `yxshadxw/plugin-b-itemmeta-residual` (기준 `origin/main` = `acc7338f`)
- 소유 범위:
  - `crates/ferrite-item/src/component.rs`, `crates/ferrite-item/src/lib.rs`, `crates/ferrite-item/src/wire.rs`, `crates/ferrite-item/src/wire/tests.rs`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteDataComponents.java`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemMeta.java`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemNbt.java`
  - `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`
  - `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemMetaBreadthTest.java`
  - `docs/handoff/plugin-b-itemmeta-residual.md`
  - (`UnsupportedItemMetaBase.java`는 수정하지 않음)

## 26.3 컴포넌트 형태 및 구현 내역

1. **`minecraft:can_break` (`protocol_id: 14`) & `minecraft:can_place_on` (`protocol_id: 13`)**:
   - 26.3 형태: NBT `List<Compound>`(또는 단일 `Compound`), 각 요소는 `{blocks: "#tag" | "id" | ["id", ...]}`; wire는 `prefixed(MAX_LIST, block_predicate)` (`optional(HolderSet<Block>)` + `properties=None` + `nbt=None` + `components=0` + `partial_components=0`).
   - Rust (`ferrite-item`): `BlockPredicate { pub blocks: Option<HolderSet> }`, `Component::CanBreak(Vec<BlockPredicate>)`, `Component::CanPlaceOn(Vec<BlockPredicate>)` 추가; `registry_ids::block_id`/`block_name`을 이용해 NBT와 wire 왕복 구현(properties/nbt/component predicates가 포함된 경우 `Component::Raw`/`Opaque`로 무손실 보존).
   - Java: `FerriteItemMeta`에 `hasDestroyableKeys`, `getDestroyableKeys`, `setDestroyableKeys`, `getCanDestroy`, `setCanDestroy`, `hasPlaceableKeys`, `getPlaceableKeys`, `setPlaceableKeys`, `getCanPlaceOn`, `setCanPlaceOn` 구현(unmodifiable `Set` 반환, `null`/빈 컬렉션 시 제거); `FerriteItemNbt` 및 `ItemStack` `DataComponentHolder`(`ItemAdventurePredicate` / `BlockPredicate`) 지원.
2. **`Equippable#toBuilder()`**:
   - `FerriteDataComponents.FerriteEquippable#toBuilder()`를 `Equippable.Builder`로 구현해 모든 필드(`equipSound`, `assetId`, `cameraOverlay`, `allowedEntities`, `dispensable`, `swappable`, `damageOnHurt`, `equipOnInteract`, `canBeSheared`, `shearSound`)를 복사/수정 후 `build()` 가능하도록 연결.
3. **`minecraft:max_stack_size` (`FerriteItemNbt` 보완)**:
   - 기존 `FerriteItemMeta`와 `ferrite-item::Component::MaxStackSize`가 모델링하고 있던 `minecraft:max_stack_size`를 `FerriteItemNbt` 인코딩/디코딩에 연결해 `nbt_base64` 스냅샷에서도 보존.

## 명시적으로 거부(UnsupportedItemMetaBase 유지)

- `hasLocalizedName`/`getLocalizedName`/`setLocalizedName`: 26.3 아이템 컴포넌트 모델에 대응하는 저장 필드가 없으므로 명시적 `UnsupportedOperationException` 유지.
- `getCustomTagContainer`: 레거시 API(대신 `getPersistentDataContainer` 지원)로 `UnsupportedOperationException` 유지.
- `getAsString` / `getAsComponentString` / `setVersion`: 직렬화 문자열/내부 버전 API로 `UnsupportedOperationException` 유지.

## 검증 명령 및 결과

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:jar :ferrite-api-impl:test :ferrite-host:test :ferrite-host:jar :ferrite-nms:jar :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar` → **BUILD SUCCESSFUL**
  - `ferrite-api-impl`: **65 tests, 0 failures, 0 errors, 0 skipped** (`ItemMetaBreadthTest` 9/9 포함)
  - `ferrite-host`: **14 tests, 0 failures, 0 errors, 0 skipped**
- `cargo test -p ferrite-item` → **ok** (`lib`: 46 passed, 0 failed, 1 ignored benchmark; `containers_books`: 9 passed; `java_vectors`: 1 passed)
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo fmt --all -- --check` → exit 0
- `cargo test -p ferrite-server --test plugin_b_server_acceptance` → **ok** (`1 passed, 0 failed, 0 ignored`)
- `cargo test --workspace` → **ok** (0 failures)
- `git diff --check` → exit 0

## 남은 한계

- 실제 Minecraft Java 클라이언트 렌더링/UI는 검증하지 않았다.
- `can_break`/`can_place_on`의 `BlockPredicate`는 `blocks` 필드만 Java facade로 노출하며, `properties`/`nbt`/component predicate가 포함된 원시 데이터는 Rust `Component::Raw`/`Opaque`로 보존한다.
