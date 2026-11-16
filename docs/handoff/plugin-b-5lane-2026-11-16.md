# 플러그인 B — 5레인 배치 (2026-11-16)

이 문서는 3레인 지시서를 **대체하는 배치/소유권 인덱스**다. 3레인 문서는 상세 배경과 계약
설명으로 남지만, 레인 수와 소유 경계는 이 문서를 따른다.

## 레인

| # | 에이전트 | 브랜치 | 워크트리 | 범위 |
|---|---|---|---|---|
| 1 | Freebuff | `yxshadxw/plugin-b-host` | `plugin-b-host` | JVM 하나, 로더, 생명주기, 호스트 RPC, 서버 배선, 계약 |
| 2 | Freebuff | `yxshadxw/plugin-b-api` | `plugin-b-api` | Bukkit/Paper **코어 facade**: Server, World, Player, 이벤트, 명령, 스케줄러, 권한, plugin manager, config |
| 3 | Freebuff | `yxshadxw/plugin-b-nms` | `plugin-b-nms` | NMS **코어**: `CraftItemStack`, `CraftWorld`, `CraftMagicNumbers`, 버전 게이트 |
| 4 | Agy (`gemini-4-hydrogen-high`) | `yxshadxw/plugin-b-items` | `plugin-b-items` | `ItemStack`, `ItemMeta`, PDC, inventory, 아이템 컴포넌트, 아이템 fixture |
| 5 | Agy (`gemini-4-hydrogen-high`) | `yxshadxw/plugin-b-entity` | `plugin-b-entity` | `CraftPlayer`, `CraftEntity`, `ServerPlayer`, `EntitySnapshot`, NMS fixture, real-host 통합 |

모든 워크트리는 `/home/ubuntu/orca/workspaces/ferrite-mc/` 아래에 있다.

## 기준 스냅샷

Lane 4와 Lane 5는 기존 Lane 2/3의 현재 작업을 복사한 **조정용 스냅샷**에서 시작한다.
스냅샷은 로컬 ref이며 push/merge 대상이 아니다.

- Lane 4 기준: `lane/plugin-b-api-snapshot` = `ead8bfb1f59e65407d3cf8fa91e21eaee86861d2`
  (현재 `plugin-b-api` 작업트리의 2026-11-16 상태).
- Lane 5 기준: `lane/plugin-b-nms-snapshot` = `15beaac936f2bd641d8c7dd26eb37659bc0a41ca`
  (현재 `plugin-b-nms` 작업트리의 2026-11-16 상태).

새 레인은 이 스냅샷에 이미 있는 코어 코드를 **읽기 전용 기반**으로 사용한다. 자기 소유 파일만
수정하고, 코어 파일을 자기 레인에서 다시 구현하지 않는다. 최종 병합은 스냅샷 전체 merge가 아니라
Lane 2/3 브랜치 위에 Lane 4/5의 소유 파일 diff를 적용하는 방식으로 한다.

## 소유 경계

### Lane 1 — 변경 없음

`crates/ferrite-plugin-host/**`, `crates/ferrite-server/src/plugin_host/**`,
`crates/ferrite-server/src/main.rs`, `docs/plugin-b-host-contract.md`, `java/ferrite-host/**`,
`java/settings.gradle.kts`와 서버 배선은 Lane 1이 소유한다. 계약 v2는 동결 상태다.

### Lane 2 — API 코어로 축소

Lane 2는 다음을 계속 소유한다.

- `dev.ferrite.api.server.**`, `world.**`, `entity.FerritePlayer`, `entity.PlayerState`
- 이벤트 dispatch, 명령 dispatcher/console sender, scheduler, permission, plugin manager
- `java/fixtures/typical-plugin/**`의 **비아이템** 부분

Lane 2는 아래를 **더 이상 수정하지 않는다**. Lane 4가 소유한다.

- `dev.ferrite.api.item.**`
- `dev.ferrite.api.inventory.**`
- `dev.ferrite.api.entity.FerritePlayerInventory`
- `org/bukkit/inventory/ItemStack.java`
- `ItemStackTest`와 아이템/인벤토리/PDC 관련 테스트·fixture 부분

Lane 2의 현재 작업트리에는 이미 이 파일들이 있으므로, 새 변경만 Lane 4 경계로 넘긴다.
Lane 4 스냅샷은 그 현재 상태를 포함한다.

### Lane 3 — NMS 코어로 축소

Lane 3은 다음을 계속 소유한다.

- `org.bukkit.craftbukkit.inventory.CraftItemStack`
- `org.bukkit.craftbukkit.CraftWorld`
- `org.bukkit.craftbukkit.util.CraftMagicNumbers`
- `dev.ferrite.nms.FerriteNmsVersion`, `NmsUnsupported`, 버전/브리지 코어
- `CraftItemStackTest`, `CraftMagicNumbersTest`, `ServerLevelTest`, 버전/브리지 테스트

Lane 3은 아래를 **더 이상 수정하지 않는다**. Lane 5가 소유한다.

- `net.minecraft.server.level.ServerPlayer`
- `net.minecraft.world.entity.**`, `EntitySnapshotFactory`
- `org.bukkit.craftbukkit.entity.**` (`CraftPlayer`, `CraftEntity`)
- `java/fixtures/nms-plugin/**`
- `EntityTest`, `ServerPlayerTest`, `RealHostIntegrationTest`

Lane 3의 현재 작업트리에는 이미 이 파일들이 있으므로, 새 변경만 Lane 5 경계로 넘긴다.
Lane 5 스냅샷은 그 현재 상태를 포함한다.

## Lane 4/5의 코어 사용 규칙

- Lane 4는 `dev.ferrite.api`의 서버/월드/플레이어/이벤트/명령/스케줄러 구현을 **호출만** 한다.
  그 파일들을 수정하지 않는다. 인터페이스가 부족하면 코디네이터에게 보고하고, Lane 2 파일을
  직접 고치지 않는다.
- Lane 5는 `CraftItemStack`/`CraftWorld`/`CraftMagicNumbers`를 **호출만** 한다. 그 파일들을
  수정하지 않는다. 호스트 계약이나 Rust 코어를 수정하지 않는다.
- 두 레인 모두 `docs/plugin-b-host-contract.md`의 op/이벤트/스레드 규칙을 따른다. 계약 변경은
  Lane 1만 한다.
- `java/settings.gradle.kts`, 루트 Gradle, `game/mod.rs`, `game/region.rs`는 공유 파일이다.
  새 레인은 원칙적으로 수정하지 않는다. 불가피하면 이유와 hunk를 핸드오프에 적고 코디네이터
  승인을 받는다.

## 병합 순서

1. Lane 1 호스트/계약.
2. Lane 2 API 코어.
3. Lane 4가 Lane 2 위에 아이템/인벤토리 소유 파일을 적용.
4. Lane 3 NMS 코어.
5. Lane 5가 Lane 3 위에 엔티티/플레이어 NMS와 fixture를 적용.
6. 통합 브랜치에서 typical fixture와 NMS fixture를 **같은 임베디드 JVM**에 함께 로드한다.

Lane 4/5는 스냅샷에 Lane 2/3 코어 코드를 포함하므로, 코디네이터는 `git diff`에서 자기 소유
경로만 골라 적용한다. 스냅샷 브랜치를 통째로 merge하지 않는다.

## 공통 규칙

- 실제 Minecraft 클라이언트 검증은 하지 않는다. 봇/JVM 테스트 결과를 클라이언트 검증으로
  표현하지 않는다.
- Mojang 코드/에셋을 커밋하지 않는다. `vanilla/`는 gitignored다.
- 실패한 테스트를 skip/ignore/약화하지 않는다. 환경 의존 테스트는 이유와 선행 조건을 적고
  정직하게 skip한다.
- Rust 상태는 소유 리전 스레드만 만진다. Java 호출이 다른 리전을 동기 대기하게 만들지 않는다.
- 각 레인은 자기 소유 문서만 수정한다. 이 인덱스와 계약 문서는 코디네이터/Lane 1이 관리한다.
- 레인이 green이면 자기 브랜치에 커밋하고 `git push -u origin HEAD`로 원격 브랜치를 올린다.
  `main` 반영은 코디네이터가 merge 순서를 확인한 뒤 각 레인에 지시한다. 지시를 받으면
  `git pull --rebase origin main` 후 `git push origin HEAD:main`을 실행한다. force-push는 금지하고,
  거부/충돌 시 작업을 잃지 말고 즉시 보고한다. 완료 보고에는 브랜치, 커밋, 변경 파일,
  정확한 테스트 명령/결과, 남은 limitation을 적는다.

## 검증 게이트

각 레인은 자기 범위에서 아래를 통과한 뒤 핸드오프를 쓴다.

- Rust 변경이 있는 레인: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (또는 해당 crate/통합 테스트).
- Java 변경이 있는 레인: 해당 모듈의 `./gradlew test`와 fixture jar 빌드.
- `git diff --check`.
- Lane 4/5: 자기 소유 파일 밖의 diff가 없는지 확인한다.
- 실제 클라이언트 검증 없음.
