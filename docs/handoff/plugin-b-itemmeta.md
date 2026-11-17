# Plugin B — ItemMeta Breadth Slice

- 브랜치: `yxshadxw/plugin-b-itemmeta` (기준 `origin/main` = `f58a210`)
- 소유 범위: `java/ferrite-api-impl/src/main/java/dev/ferrite/api/item/**`, `java/ferrite-api-impl/src/main/java/org/bukkit/inventory/ItemStack.java`, `java/ferrite-api-impl/src/test/java/dev/ferrite/api/ItemMetaBreadthTest.java`, `docs/handoff/plugin-b-itemmeta.md` (`UnsupportedItemMetaBase.java`는 수정하지 않음)

## 구현/연결한 ItemMeta 및 브리지 표면

- **Display name / Item name**:
  - 기존 `hasCustomName`, `customName()`, `customName(Component)`, `hasDisplayName`, `getDisplayName`, `setDisplayName(String)` 유지
  - 신규 `hasItemName`, `itemName()`, `itemName(Component)`, `getItemName`, `setItemName(String)` 구현 (`FerriteItemMeta`의 `clone`, `isEmpty`, `serialize`, `deserialize`, `equals`, `hashCode`, `toString` 포함)
  - `FerriteItemNbt`에 `minecraft:item_name` 인코딩/디코딩 추가 및 `ItemStack` `DataComponentHolder`에 `minecraft:item_name` 지원 추가
- **Lore**: `hasLore`, `lore()`, `getLore`, `setLore(List<String>)`, `lore(List<? extends Component>)` (방어적 복사 및 `null`/빈 리스트 정규화)
- **Custom model data**: `hasCustomModelData`, `getCustomModelData`, `setCustomModelData(Integer)` 및 같은 `FerriteCustomModelDataComponent`와 연결된 `hasCustomModelDataComponent`, `getCustomModelDataComponent`, `setCustomModelDataComponent`
- **Enchantments**: `hasEnchants`, `hasEnchant`, `getEnchantLevel`, `getEnchants`, `addEnchant`, `removeEnchant`, `removeEnchantments`, `hasConflictingEnchant`
- **Unbreakable & Damage/MaxDamage**: `isUnbreakable`, `setUnbreakable`, `hasDamage`, `getDamage`, `setDamage`, `hasDamageValue`, `resetDamage`, `hasMaxDamage`, `getMaxDamage`, `setMaxDamage`, `FerriteItemNbt`에 `minecraft:max_damage` 인코딩/디코딩 추가 (`minecraft:damage`, `minecraft:unbreakable` 왕복 포함)

## 명시적으로 거부(UnsupportedItemMetaBase 유지)

- `hasEnchantmentGlintOverride`, `getEnchantmentGlintOverride`, `setEnchantmentGlintOverride` (`ferrite-item::Component`에 타입으로 모델링되지 않아 조용히 버리지 않도록 `UnsupportedOperationException` 유지)
- `hasLocalizedName` / `getLocalizedName` / `setLocalizedName`, `isGlider` / `setGlider`, `hasRarity` / `getRarity` / `setRarity`, `hasFireResistant` / `setFireResistant`, `hasDamageResistant`, `hasUseRemainder`, `getDisplayNameComponent` 등 나머지 미지원 `ItemMeta` 메서드는 모두 `UnsupportedItemMetaBase`의 `UnsupportedOperationException` 유지

## 검증 명령 및 결과

- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:test :fixtures:typical-plugin:jar` → **BUILD SUCCESSFUL** (`ferrite-api-impl`: **51 tests, 0 failures, 0 errors, 0 skipped**, `ItemMetaBreadthTest` 4/4 포함)
- `cd java && JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 gradle --no-daemon --console=plain :ferrite-api-impl:jar :ferrite-host:jar :ferrite-nms:jar :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar` → **BUILD SUCCESSFUL**
- `cargo test -p ferrite-server --test plugin_b_server_acceptance` → **ok** (`shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end`: 1 passed, 0 failed, 0 ignored)
- `git diff --check` → exit 0

## 남은 한계

- 실제 Minecraft Java 클라이언트 렌더링/UI는 검증하지 않았다(프로토콜 `Bot` 및 JUnit 단위 테스트까지만 검증).
- `enchantment_glint_override` 등 `ferrite-item::Component`에 아직 모델링되지 않은 컴포넌트는 `UnsupportedOperationException`으로 거부한다.
