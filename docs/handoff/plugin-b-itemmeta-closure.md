# Plugin B — ItemMeta & Item Component Closure

- 브랜치: `yxshadxw/plugin-b-itemmeta-closure` (기준 `origin/main` = `873d173e`)
- 소유 범위:
  - `crates/ferrite-item/src/component.rs`, `crates/ferrite-item/src/lib.rs`, `crates/ferrite-item/src/wire.rs`, `crates/ferrite-item/src/wire/tests.rs`
  - `crates/ferrite-server/src/game/region.rs` (`USE_REMAINDER`가 `Component::Opaque`에서 `Component::UseRemainder`로 승격됨에 따라 물약 섭취 후 유리병 반환 매칭 갱신)
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteHolderSet.java` (신규)
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteDataComponents.java`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemMeta.java`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemNbt.java`
  - `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`
  - `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemMetaBreadthTest.java`
  - `docs/handoff/plugin-b-itemmeta-closure.md`
  - (`UnsupportedItemMetaBase.java`는 수정하지 않음)

## 26.3 컴포넌트 형태 및 구현 내역

1. **`minecraft:damage_resistant` (`protocol_id: 27`) + `isFireResistant`/`setFireResistant`**:
   - 26.3 형태: NBT `{types: "#namespace:tag" | ["id", ...]}` (`HolderSet`), wire `VarInt(0) + tag_id` (또는 `VarInt(n+1) + VarInt*n`).
   - `crates/ferrite-item`: `HolderSet::{Tag, Ids}` 및 `Component::DamageResistant(HolderSet)` 추가; 태그 형태는 NBT/wire 모두 왕복, 정적 프로토콜 ID가 없는 `damage_type` ID 목록은 NBT 보존 및 wire 인코딩 시 명시적 `WireError::UnknownRegistry("damage_type")` 반환.
   - Java: `FerriteItemMeta#isFireResistant()` / `setFireResistant(boolean)`은 `vanilla/26.3/generated/data/minecraft/tags/damage_type/is_fire.json`의 `#minecraft:is_fire` (8개 화염 피해 타입)와 정확히 매핑. `hasDamageResistant`, `getDamageResistant`, `setDamageResistant`, `getDamageResistantTypes`, `setDamageResistantTypes`, `ItemStack` `DataComponentHolder`, `FerriteItemNbt` 모두 지원.
2. **`minecraft:use_remainder` (`protocol_id: 25`)**:
   - 26.3 형태: NBT 아이템 스택 compound `{id, count, components?}`, wire `item_stack_template`.
   - `crates/ferrite-item`: `Component::UseRemainder(Box<ItemStack>)` 추가 (`encode_template_into` / `decode_template` 왕복).
   - `crates/ferrite-server/src/game/region.rs`: 물약/소모품 사용 완료 시 `Component::UseRemainder`를 직접 읽어 잔여 아이템(`minecraft:glass_bottle` 등)을 지급하도록 갱신.
   - Java: `FerriteItemMeta#hasUseRemainder` / `getUseRemainder` / `setUseRemainder` (방어적 복사 및 `null`/`AIR`/빈 수량 정규화), `FerriteItemNbt`, `ItemStack` `DataComponentHolder`(`UseRemainder`) 지원.
3. **`minecraft:repairable` (`protocol_id: 33`)**:
   - 26.3 형태: NBT `{items: "#namespace:tag" | ["minecraft:...", ...]}`, wire `HolderSet<Item>` (`0 + tag` 또는 `(len + 1) + item_id*len`).
   - `crates/ferrite-item`: `Component::Repairable(HolderSet)` 추가, NBT 및 wire 왕복.
   - Java: `FerriteItemNbt` `minecraft:repairable` 인코딩/디코딩 및 `ItemStack` `DataComponentHolder`(`io.papermc.paper.datacomponent.item.Repairable`) 지원.

## 명시적으로 거부(UnsupportedItemMetaBase 유지)

- `hasLocalizedName`/`getLocalizedName`/`setLocalizedName`, `getDestroyableKeys`/`setDestroyableKeys`/`hasDestroyableKeys`, `getPlaceableKeys`/`setPlaceableKeys`/`hasPlaceableKeys`, `Equippable#toBuilder` 등 모델링되지 않은 메서드는 모두 `UnsupportedOperationException`을 유지한다.

## 검증 명령 및 결과

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:jar :ferrite-api-impl:test :ferrite-host:test :ferrite-host:jar :ferrite-nms:jar :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar` → **BUILD SUCCESSFUL**
  - `ferrite-api-impl`: **61 tests, 0 failures, 0 errors, 0 skipped** (`ItemMetaBreadthTest` 8/8 포함)
  - `ferrite-host`: **14 tests, 0 failures, 0 errors, 0 skipped**
- `cargo test -p ferrite-item` → **ok** (`lib`: 46 passed, 0 failed, 1 ignored benchmark; `containers_books`: 9 passed; `java_vectors`: 1 passed)
- `cargo test -p ferrite-server --test plugin_b_server_acceptance` → **ok** (`shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end`: 1 passed, 0 failed, 0 ignored)
- `cargo test --workspace` → **ok** (0 failures; `ferrite-server` `potion_drinking_applies_vanilla_effects_with_bot` 포함 전체 통과)
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo fmt --all -- --check` → exit 0
- `git diff --check` → exit 0

## 남은 한계

- 실제 Minecraft Java 클라이언트 렌더링/UI는 검증하지 않았다.
- `damage_type`은 데이터팩 레지스트리이므로 `#minecraft:is_fire` 등 태그 참조는 wire 왕복이 가능하지만, 개별 `damage_type` ID 목록의 wire 직렬화는 정적 테이블 부재로 명시적 `WireError::UnknownRegistry`를 반환한다.
