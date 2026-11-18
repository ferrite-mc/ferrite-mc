# Plugin B — ItemMeta Compat Slice (Bungee Views & `enchantment_glint_override`)

- 브랜치: `yxshadxw/plugin-b-itemmeta-compat` (기준 `origin/main` = `ee4d7cf`)
- 소유 범위:
  - `crates/ferrite-item/src/component.rs`, `crates/ferrite-item/src/wire.rs`, `crates/ferrite-item/src/wire/tests.rs`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemMeta.java`
  - `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/FerriteItemNbt.java`
  - `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`
  - `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemMetaBreadthTest.java`
  - `docs/handoff/plugin-b-itemmeta-compat.md`
  - (`UnsupportedItemMetaBase.java`는 수정하지 않음)

## 구현/연결한 표면

1. **Bungee 호환 뷰 (`FerriteItemMeta`)**:
   - `getDisplayNameComponent()`, `setDisplayNameComponent(BaseComponent[])`, `getLoreComponents()`, `setLoreComponents(List<BaseComponent[]>)`
   - `GsonComponentSerializer.gson()` ↔ `ComponentSerializer`로 Adventure `customName()`/`lore()`와 단일 소스를 공유하며, `null`/빈 입력 정규화와 방어적 복사를 유지한다.
2. **`enchantment_glint_override` 종단간(end-to-end)**:
   - `crates/ferrite-item`: `Component::EnchantmentGlintOverride(bool)` 추가, NBT(`TAG_BYTE`) 및 네트워크 wire(`bool`) 인코딩/디코딩과 왕복 단위 테스트 추가.
   - `FerriteItemMeta`: `hasEnchantmentGlintOverride()`, `getEnchantmentGlintOverride()`(미설정 시 `IllegalStateException`), `setEnchantmentGlintOverride(Boolean)` 구현; `clone`/`isEmpty`/`serialize`/`deserialize`/`equals`/`hashCode`/`toString` 반영. `absent` / `true` / `false` 3상태를 구분한다.
   - `FerriteItemNbt`: `minecraft:enchantment_glint_override` (`TAG_BYTE`) 인코딩/디코딩.
   - `ItemStack`: `DataComponentHolder`에 `minecraft:enchantment_glint_override` (`Boolean`) 추가.

## 명시적으로 거부(UnsupportedItemMetaBase 유지)

- `hasLocalizedName`/`getLocalizedName`/`setLocalizedName`, `isGlider`/`setGlider`, `hasRarity`/`getRarity`/`setRarity`, `isFireResistant`/`setFireResistant`, `hasDamageResistant`, `hasUseRemainder` 등 나머지 미지원 `ItemMeta` 메서드는 모두 `UnsupportedItemMetaBase`의 `UnsupportedOperationException`을 유지한다.

## 검증 명령 및 결과

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:test :fixtures:typical-plugin:jar :ferrite-host:jar :ferrite-nms:jar` → **BUILD SUCCESSFUL** (`ferrite-api-impl`: **55 tests, 0 failures, 0 errors, 0 skipped**, `ItemMetaBreadthTest` 6/6 포함)
- `cargo fmt --all -- --check` → exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test -p ferrite-item` → **ok** (`lib`: 46 passed, 0 failed, 1 ignored benchmark; `containers_books`: 9 passed; `java_vectors`: 1 passed)
- `cargo test -p ferrite-server --test plugin_b_server_acceptance` → **ok** (`shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end`: 1 passed, 0 failed, 0 ignored)
- `git diff --check` → exit 0

## 남은 한계

- 실제 Minecraft Java 클라이언트 렌더링/UI는 검증하지 않았다.
- `localizedName`, `glider`, `rarity`, `fireResistant`, `damageResistant`, `useRemainder` 등 미지원 `ItemMeta` 메서드는 계속 `UnsupportedOperationException`으로 거부한다.
