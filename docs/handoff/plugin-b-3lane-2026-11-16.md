# 플러그인 B 구조 — 3레인 지시서 (2026-11-16)

이 문서는 **지시서 인덱스**다. 실제 작업 범위는 아래 세 레인 문서가 소유한다.

- [Lane 1 — JVM 호스트·로더·생명주기](plugin-b-lane-1-host.md)
- [Lane 2 — Bukkit/Paper API facade](plugin-b-lane-2-api.md)
- [Lane 3 — 선별 NMS 어댑터](plugin-b-lane-3-nms.md)

## 0. 기준과 전제

- 기준 브랜치: `lane/plugins-z`의 **플러그인 제거 변경을 포함한 베이스**. 현재 변경은
  uncommitted이므로, 레인을 시작하기 전에 coordinator가 베이스를 커밋/확정한다.
- 목표 구조는 B다: **Rust 서버 코어 + 같은 프로세스의 JVM 하나 + Java 호스트/로더 +
  Bukkit/Paper facade + 선별 NMS 어댑터**.
- 기존 플러그인 구현은 이미 제거됐다. `java/ferrite-api-impl`, 서버 `plugins/` 모듈,
  `crates/ferrite-plugin`, Java 테스트 플러그인을 git에서 통째로 복원하지 않는다.
  과거 구현은 동작·API 버전·테스트 아이디어를 확인하는 참고 자료로만 쓰고, 새 구조에
  맞춰 다시 작성한다.
- `crates/ferrite-jvm`과 `crates/ferrite-plugin-meta`는 유지한다. 전자는 임베디드 JVM,
  후자는 `plugin.yml`/`paper-plugin.yml` 검증과 로드 순서를 이미 소유한다.
- `ferrite-server`의 바닐라 worldgen은 **외부 Java 프로세스**를 띄운다. 따라서 플러그인
  호스트가 이 프로세스의 유일한 임베디드 JVM이 된다. `Jvm::start`를 두 곳에서 호출하거나
  두 번째 JVM을 만들지 않는다. 나중에 worldgen을 임베디드 JVM으로 옮기더라도 JVM 시작
  소유자는 Lane 1 하나다.
- `docs/plugin-api-stubs.md`, `docs/plugin-stubs-status.md`,
  `docs/plugin-api-completion-directive.md`, `docs/handoff/local-plugin-api-queue.md`는
  **삭제된 구현의 역사 기록**이다. 새 레인의 완료 조건이나 현재 코드 상태로 인용하지 않는다.
  새 계약은 Lane 1이 만드는 `docs/plugin-b-host-contract.md`가 소유한다.

## 1. 레인 경계

| | Lane 1 | Lane 2 | Lane 3 |
|---|---|---|---|
| 이름 | JVM 호스트·로더·생명주기 | Bukkit/Paper API facade | 선별 NMS 어댑터 |
| 브랜치 | `lane/plugin-b-host` | `lane/plugin-b-api` | `lane/plugin-b-nms` |
| 핵심 산출물 | `crates/ferrite-plugin-host`, `java/ferrite-host`, 서버 배선 | `java/ferrite-api-impl`, typical fixture plugin | `java/ferrite-nms`, NMS fixture plugin |
| 직접 소유 | JVM 시작, 클래스로더, `plugin.yml` 로드, `onLoad`/`onEnable`/`onDisable`, 호스트 RPC, 이벤트 소스 | `Server`/`World`/`Player`/`ItemStack`/이벤트/명령/스케줄러/설정 facade | `CraftItemStack`, `CraftWorld#getHandle`, `CraftMagicNumbers`, 선별 엔티티/월드 핸들 |
| 직접 소유하지 않음 | Bukkit 메서드 의미론, NMS 래퍼 | JVM 시작/클래스로더/RPC 전송, NMS 내부 | Bukkit API 일반 동작, 호스트 RPC 의미론 |
| 선행 | 없음 (계약과 스캐폴드 먼저) | Lane 1 계약 동결 + Gradle 스캐폴드 | Lane 1 계약 + Lane 2 모듈 스캐폴드 |

### 공유 파일과 충돌 규칙

- `crates/ferrite-server/src/main.rs`, `crates/ferrite-server/src/plugin_host/**`,
  `crates/ferrite-plugin-host/**`, `crates/ferrite-jvm/**`, `java/settings.gradle.kts`,
  `java/ferrite-host/**`는 **Lane 1만** 수정한다.
- Lane 2는 `java/ferrite-api-impl/**`와 `java/fixtures/typical-plugin/**`만 수정한다.
  `java/settings.gradle.kts`의 모듈 include가 필요하면 Lane 1에 요청한다.
- Lane 3은 `java/ferrite-nms/**`와 `java/fixtures/nms-plugin/**`만 수정한다.
  호스트나 API facade의 계약을 바꾸지 않는다.
- `game/mod.rs`, `game/region.rs`는 공유 코어다. 플러그인 이벤트를 내보내기 위한 최소
  콜백은 Lane 1이 추가하고, Lane 2/3은 이 파일들을 직접 수정하지 않는다. 불가피하면
  해당 hunk와 이유를 핸드오프에 적고 coordinator 승인을 받는다.
- 각 레인은 자기 문서(`docs/handoff/plugin-b-lane-N-*.md`)만 수정한다. 이 인덱스와
  `docs/plugin-b-host-contract.md`는 coordinator/Lane 1이 관리한다.

## 2. 계약 동결과 병합 순서

1. **Lane 1, 계약 + 스캐폴드**: `docs/plugin-b-host-contract.md`에 RPC op 이름, 요청/응답
   스키마, 오류 코드, 버전, 스레드 규칙, 이벤트 이름을 적고, `java/settings.gradle.kts`와
   세 Java 모듈의 빈 스캐폴드를 만든다. 이 단계가 끝나기 전에는 Lane 2/3이 호스트 op를
   확정하지 않는다.
2. **Lane 1, 호스트 smoke**: JVM 하나로 `java/ferrite-host`를 띄우고, Java 쪽 테스트 클래스의
   static 호출과 native 콜백을 왕복한다. 이 시점에 Lane 2/3이 병렬로 시작할 수 있다.
3. **Lane 2와 Lane 3 병렬**: 각자 fake host로 단위 테스트를 만들고, 가능한 범위에서 Lane 1의
   실제 호스트와 통합한다.
4. **통합**: Lane 1 → Lane 2 → Lane 3 순서로 coordinator가 병합한다. Lane 3 병합 뒤
   typical fixture와 NMS fixture를 **같은 임베디드 JVM**에서 함께 로드하는 통합 테스트를
   한 번 더 실행한다.
5. 계약 변경은 금지가 아니라 **명시적 변경**이다. op나 payload를 바꾸면 계약 문서의 버전을
   올리고 세 레인 핸드오프에 영향 범위를 적는다. 구버전 op를 조용히 재해석하지 않는다.

## 3. 모든 레인에 적용되는 규칙

- **스레드 소유권**: Rust 월드/엔티티 상태는 소유 리전 스레드만 만진다. 플러그인 호출은
  호스트 큐를 거쳐 소유 리전으로 라우팅하고, 리전 스레드에서 다른 리전을 동기 대기하지
  않는다. Bukkit 이벤트/명령/동기 스케줄러는 서버 메인 스레드 규칙을 지킨다. async
  스케줄러만 별도 풀을 쓴다.
- **명시적 실패**: 지원하지 않는 메서드는 `null`, 빈 컬렉션, 성공처럼 보이는 no-op으로
  숨기지 않는다. `UnsupportedOperationException`에 플러그인 이름과 메서드 이름을 넣고,
  대체 Bukkit 경로가 있으면 문서화한다.
- **정직한 버전**: `paper-api:26.3.build.227-stable`와 Java 25를 기준으로 한다. 다른
  API/서버 버전을 지원하는 척하지 않는다. NMS는 26.3의 선별 surface만 약속한다.
- **Mojang 코드/에셋 금지**: 디컴파일 코드, Mojang jar에서 복사한 클래스, 바닐라 에셋을
  커밋하지 않는다. 로컬 `vanilla/`는 gitignored이며 시그니처 확인에만 쓴다.
- **테스트 삭제/약화 금지**: 실패한 테스트를 skip/ignore로 바꾸거나 assertion을 낮춰
  통과시키지 않는다. 환경 의존 테스트는 이유와 선행 조건을 출력하고 정직하게 skip한다.
- **실제 클라이언트 검증 없음**: 봇/JVM 테스트는 실제 Minecraft 화면·클라이언트 렌더링을
  대체하지 않는다. 검증하지 않은 것은 핸드오프에 limitation으로 적는다.
- **빌드 자원**: 공유 호스트에서 동시에 무거운 Gradle/cargo 작업을 여러 개 돌리지 않는다.
  다른 워커의 프로세스를 죽이지 않는다.

## 4. 공통 검증 게이트

각 레인은 자기 범위에서 아래를 통과한 뒤 핸드오프를 쓴다.

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` (Lane 1/2/3 변경이 코어에 닿는 경우)
- `cd java && ./gradlew test` (Lane 2/3; Lane 1은 host 모듈 테스트)
- `git diff --check`
- 호스트 통합: 픽스처 플러그인 jar를 실제 임베디드 JVM에 로드해 `onEnable` → 이벤트/명령 →
  스케줄러 → `onDisable`을 확인한다.
- 실제 클라이언트 검증은 별도이며, 이 3레인의 완료 조건이 아니다.

## 5. 공통 완료 정의

- 각 레인의 P0 항목이 구현되고 테스트로 고정됐다.
- 지원/거부 표면이 문서화됐고, 거부는 테스트로 확인된다.
- 호스트 op/이벤트 이름이 계약 문서와 일치한다.
- 통합 브랜치에서 세 레인을 함께 로드했을 때 서버가 기동하고 정상 종료한다.
- 핸드오프에 브랜치, 커밋, 변경 파일, 공유 파일 hunk, 정확한 테스트 명령/결과, 남은
  limitation, 계약 버전을 적는다.
