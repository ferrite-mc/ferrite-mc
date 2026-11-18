# Plugin B — ItemMeta Components Slice (`glider`, `rarity`, `fire_resistant`)

- 브랜치: `yxshadxw/plugin-b-itemmeta-components` (기준 `origin/main` = `3d7eb846`)
- 소유 범위:
  - `crates/ferrite-item/src/component.rs`, `crates/ferrite-item/src/wire.rs`, `crates/ferrite-item/src/wire/tests.rs`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemMeta.java`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemNbt.java`
  - `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`
  - `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemMetaBreadthTest.java`
  - `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemStackTest.java`, `TypicalFixtureIntegrationTest.java`, `java/fixtures/typical-plugin/.../TypicalPlugin.java` (기존 `unsupportedProbe`의 미지원 예시 키를 `minecraft:glider`에서 `minecraft:creative_slot_lock`으로 교체)
  - `docs/handoff/plugin-b-itemmeta-components.md`
  - (`UnsupportedItemMetaBase.java`는 수정하지 않음)

## 26.3 컴포넌트 형태 및 구현/거부 내역

1. **`minecraft:glider` (구현 완료 — protocol_id 34)**:
   - 26.3 형태: unit presence component (NBT `TAG_Compound({})`, wire `0`바이트 `vec![]`).
   - Rust (`ferrite-item`): `Component::Glider` 추가, `to_nbt`/`from_nbt`, `wire` 인코딩/디코딩 및 왕복 테스트 추가.
   - Java: `FerriteItemMeta#isGlider()` / `setGlider(boolean)` (`false` 시 제거), `FerriteItemNbt` `minecraft:glider` 인코딩/디코딩, `ItemStack` `DataComponentHolder` (`DataComponentType.NonValued`) 지원.
2. **`minecraft:rarity` (구현 완료 — protocol_id 12)**:
   - 26.3 형태: enum string (`"common" | "uncommon" | "rare" | "epic"`) in NBT (`TAG_String`), `VarInt` index on wire (기존 `ferrite-item` `Component::Rarity(Rarity)` 모델 사용).
   - Java: `FerriteItemMeta#hasRarity()` / `getRarity()`(미설정 시 `IllegalStateException`) / `setRarity(org.bukkit.inventory.ItemRarity)` (`null` 시 제거), `FerriteItemNbt` `minecraft:rarity` 인코딩/디코딩, `ItemStack` `DataComponentHolder` (`DataComponentType.Valued<ItemRarity>`) 지원.
   - 참고: Paper 26.3 `ItemMeta`에는 `org.bukkit.inventory.ItemRarity` 오버로드만 존재한다(`io.papermc.paper.inventory.ItemRarity`는 `ItemStack#getRarity()`에만 있음).
3. **`minecraft:fire_resistant` (명시적 거부 유지 — 26.3 미존재)**:
   - `vanilla/26.3/generated/reports/registries.json`의 `minecraft:data_component_type`에 `minecraft:fire_resistant`가 존재하지 않는다(1.21.2+에서 `minecraft:damage_resistant {types: "#minecraft:is_fire"}`로 대체됨).
   - 임의의 wire 형태를 발명하지 않고 `damage_resistant`도 범위 밖으로 두는 규칙에 따라 `ItemMeta#isFireResistant` / `setFireResistant(boolean)`는 `UnsupportedItemMetaBase`의 `UnsupportedOperationException`을 그대로 유지한다.
4. **기타 미지원 유지**: `localizedName`, `damage_resistant`, `use_remainder` 등 나머지 메서드는 모두 `UnsupportedItemMetaBase`에서 `UnsupportedOperationException`을 던진다.

## 검증 명령 및 결과

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:test :ferrite-api-impl:jar :fixtures:typical-plugin:jar :ferrite-host:jar :ferrite-nms:jar` → **BUILD SUCCESSFUL** (`ferrite-api-impl`: **58 tests, 0 failures, 0 errors, 0 skipped**, `ItemMetaBreadthTest` 7/7, `ItemStackTest` 14/14)
- `cargo fmt --all -- --check` → exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test -p ferrite-item` → **ok** (`lib`: 46 passed, 0 failed, 1 ignored benchmark; `containers_books`: 9 passed; `java_vectors`: 1 passed)
- `cargo test -p ferrite-server --test plugin_b_server_acceptance` → **ok** (`shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end`: 1 passed, 0 failed, 0 ignored)
- `git diff --check` → exit 0

## 남은 한계

- 실제 Minecraft Java 클라이언트 렌더링/UI는 검증하지 않았다.
- `isFireResistant`/`setFireResistant`는 26.3에서 `minecraft:damage_resistant`로 표현되므로 `damage_resistant`가 구현되기 전까지 명시적으로 거부된다.
