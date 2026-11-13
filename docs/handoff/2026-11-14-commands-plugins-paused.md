# Commands / plugins 인수인계 — 2026-11-14

## 중단 이유와 이번 마무리 범위

이전 Claude 세션은 사용량 제한으로 2026-11-13 16:40 UTC에 중단됐다.
사용자는 이번 세션에서 구현 재개 대신 인수인계 문서 작성·커밋·푸시만 요청했다.
이번 마무리에서는 코드 수정, 후속 브랜치 병합, 워커 재개, 원격 Orca 재시작을 하지 않았다.
이 문서는 구현 완료 선언이 아니다.

## 이어서 볼 위치

- 작업 트리: `C:/Users/taegy/orca/workspaces/ferrite-mc-ref/plan1104/.worktrees/commands-3lanes`
- 브랜치: `integrate/commands-3lanes`, upstream: `origin/integrate/commands-3lanes`
- 문서 작성 직전 로컬/원격 HEAD: `3f281503618ecf1f6f2edc205d7a37471ee762c4`
- 원격: `https://github.com/ferrite-mc/ferrite-mc.git`
- 해당 작업 트리는 문서 작성 전 clean이었다. 상위 `plan1104`는 별도 `fix/main-merge`
  브랜치이며 untracked `artifacts/`가 있다. 두 작업 트리를 혼동하지 말 것.
- 이전 세션 기록(읽기 전용):
  `C:/Users/taegy/.claude/projects/C--Users-taegy-orca-workspaces-ferrite-mc-ref-plan1104/4b1f2d20-0d42-4f41-9baa-949b96582d8a.jsonl`
- 이전 Orca Run: `run_08635b12c05a`.
  이번 세션의 `orca orchestration check --run run_08635b12c05a --peek --json`은
  `consumer_fenced: This coordinator terminal is no longer bound to Run run_08635b12c05a.`로
  거부됐다. 메시지 확인·ack 또는 실행 권한 인수는 하지 않았다.
  워커의 현재 생존 여부, 미커밋 파일, 최종 완료 보고는 확인되지 않았다.

## Git에서 확인된 저장 상태

2026-11-14에 `git ls-remote --heads`와 `git fetch origin`으로 확인했다.
아래 SHA는 체크포인트이며 재개할 때 원격을 다시 확인해야 한다.

| 브랜치 | 원격 HEAD | 확인된 내용 |
|---|---|---|
| `lane/commands-a` | `7efc7443` | 초기 명령어 lane A |
| `lane/commands-b` | `a5f0a597` | 초기 명령어 lane B |
| `lane/commands-c` | `4b2a99ed` | 초기 명령어 lane C |
| `lane/plugins-a` | `f148efb9` | 기존 플러그인 작업 상태 정리 |
| `lane/blocked-x` | `13406d7c` | loot 함수/호스트, item modifier, loot 피드백, fall_variants 후속 |
| `lane/blocked-y` | `d51165df` | advancement 이벤트, 변종, compute health, place support 후속 |
| `lane/blocked-z` | `b5f02a5a` | 차원 간 clone, scoreboard wire format, compute, predicate, test runner |
| `lane/plugins-z` | `d39ec3cd` | banner/skull, biome API, Windows Gradle, 실제 jar host 테스트 추가 |

통합 HEAD의 최근 병합은 `1b7c65d8`(plugins-a), `6fce795d`(commands B),
`3f281503`(commands C)이다. `blocked-x/y/z`와 `plugins-z` 후속 커밋은 아직 통합되지 않았다.
`blocked-z` HEAD는 `plugins-z` HEAD의 조상이 아니다(`merge-base --is-ancestor` exit 1).
두 브랜치의 공통 이력과 변경 중복을 확인하고, plugins-z만 병합해 Z 전체가 들어왔다고 가정하지 말 것.
`lane/plugins-x`, `lane/plugins-y`는 이번 원격 heads 목록에 없었다.
이 사실만으로 원격 작업 트리의 작업 유무를 판단할 수 없다.

## 작업자별 남은 범위

기존 command-a/b/c 세션에 각각 X/Y/Z 후속과 plugins-x/y/z를 순차 배정한 상태였다.
아래는 과거 배정 내용 및 저장된 커밋의 요약이며, 통합 검증 결과가 아니다.

- **X → plugins-x:** X 커밋은 원격에 저장돼 있다. `/item` slot-source 및 fall_variants의
  정확한 잔여 제한은 `13406d7c`의 문서 diff부터 확인한다. 후속은 playEffect 데이터 타입,
  GameRule/MemoryKey registry, Enchantment/BlockData/MapCanvas 미지원 API,
  Java LootTable과 Rust loot 동작 정합성, Paper lifecycle events, 비-NMS UnsafeValues다.
  plugins-x 완료 증거는 확보하지 못했다.
- **Y → plugins-y:** Y의 마지막 문서에는 추가 advancement 이벤트와 cat/wolf 변종,
  bred_animals, `/compute` mob health, `/place` support 처리가 기록돼 있다.
  변종은 클라이언트 전송 미구현, container loot의 seed/luck 미사용,
  place는 fence/wall/pane 연결 미구현, 일부 trigger는 기반 gameplay 부재로 남아 있다.
  후속은 non-player entity queries, 기본 3차원 entity NBT/PDC 및 structure capture,
  shoulder parrot, passenger/vehicle 연결, ChangeDimension 즉시 snapshot 회귀 테스트다.
  plugins-y 완료 증거는 확보하지 못했다.
- **Z → plugins-z:** Z 커밋과 plugins-z 일부 커밋이 원격에 저장돼 있다.
  남은 점검은 stub probe 목록 재생성 및 host 응답/실제 미지원 분류,
  현실적인 plugin config/commands/listeners/scheduler/PDC/inventory GUI/scoreboard
  패턴을 실제 JVM+bot으로 확인하는 smoke 테스트다. 최신 plugins-z HEAD에는 실제 jar의
  locate/structure API 테스트 코드가 추가돼 있으나 이번 세션에서는 실행하지 않았다.
  `/execute`의 최초 요청 전체가 구현됐다고 추정하지 말고 현 코드와 상태 문서를 대조할 것.
  `/jfr`의 정직한 미지원은 유지 대상이다.

## 재개 순서와 검증

1. 사용자가 재개를 요청하면 원격 heads와 실제 워커 작업 트리부터 확인한다.
   기존 provider 세션은 읽기 전용 자료이며 자동 resume/수정하지 않는다.
2. 각 후속 브랜치의 diff와 `docs/COMMANDS-STATUS.md`를 읽고 실제 미완료 항목을 재확인한다.
   상태 문서의 오래된 섹션과 후속 섹션이 충돌할 수 있다.
   `blocked-z`의 tree parity 기록은 82 exact roots, 12 differing roots, 186 differences /
   24 allow-list entries이며, 통합 후 수치는 다시 측정해야 한다.
3. 승인된 재개 범위에서 브랜치들을 검토·통합한다. 여러 lane이 commands_world,
   protocol/play, 상태 문서를 수정하므로 충돌 해결 뒤 의미 검증이 필요하다.
4. 관련 회귀 테스트, command tree parity, Rust workspace tests/clippy/fmt,
   Java API tests 및 실제 JVM+bot/vanilla-host 검증을 실행한다.
   실제 클라이언트 검증과 Paper/NMS 전체 호환 완료는 별도 증거가 필요하다.
5. 과거 사용자는 모든 워커 작업 종료 후 SSH로 원격 Orca를 재시작하여 모바일 pairing
   코드를 발급해 달라고 요청했다. 아직 완료하지 않았고 이번 마무리 범위에도 포함하지 않았다.
   워커 종료와 환경을 확인한 뒤 사용자 의도를 재확인하여 다룰 후속 항목이다.

이번 세션의 검증은 Git 상태/원격 refs/커밋 기록 확인과 문서 diff 검사에 한정된다.
제품 코드 변경이 없어 빌드·Rust/Java 테스트를 실행하지 않았으며 이전 테스트의 통과를
현재 통합 브랜치의 통과로 간주하지 않는다.
