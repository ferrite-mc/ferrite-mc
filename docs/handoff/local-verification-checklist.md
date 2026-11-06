# 로컬에서 직접 확인해야 하는 것 (클라우드 세션에서는 못 한 것)

환경: `export RUSTUP_TOOLCHAIN=stable`은 Linux 클라우드용. 로컬(Windows)은 `source tools/env.sh`.

## 1. 실제 jar 월드젠 (eula=true 필요 — 사용자가 직접 기입)
- [ ] `cargo test -p ferrite-server --test vanilla_host`
- [ ] 실서버를 `[worldgen]` 기본값(패처 켜짐)으로 실행 → 청크 생성, 구조물 존재 확인
- [ ] `patch = false`와 비교(성능, 결과 동일 여부), 패처 실패 시 원본 jar fallback 로그
- [ ] `World#getStructures` (host op 10) 실제 구조물 좌표가 맞는지
- [x] 호스트 스레드 8개 정지: 실제 jar에서 재현되지 않음(8, 16스레드 모두 완료). 576청크 측정에서 4/8/16 스레드가 모두 ~42/s로 정체했던 원인은 `getChunk`를 요청 스레드에서 부르면 서버 메인 스레드가 `managedBlock`으로 중첩 대기하는 것. `OP_GENERATE`를 티켓 + `getChunkFuture`로 바꿔 ~42/s에서 ~50-55/s로 개선(2026-11-06, 20스레드 PC, 패치 jar).
- [ ] 위 이후에도 8코어 안팎만 쓰고 정체함(힙 1/2/4 GB 모두 ~55/s). 남은 상한은 바닐라 월드젠 내부 병렬도로 추정, 미확인

## 2. 실제 클라이언트 (26.3 / protocol 777)
- [ ] 로케이터 바: 다른 플레이어 아이콘, 방향, 스타일/색(`/waypoint modify`), 관전자 제외
- [ ] 발전과제: 탭/토스트 표시, 저장, 달성(인벤토리/킬/섭취/차원/블록 설치), 채팅 공지, 레시피·XP 보상
- [ ] 레시피북: 열림/필터 설정 유지(재접속 후), 레시피 해금/제거
- [ ] 클라이언트 디코드 실패로 접속이 끊기지 않는지 (위 세 패킷이 위험 지점)
- [ ] `createExplorerMap`: 지금은 Java MapMeta에만 링크가 있어 클라이언트에는 일반 지도로 보임 → map_id 컴포넌트와 지도 데이터 필요

## 3. 부하/동시성
- [ ] 200봇 부하 테스트 재실행 (로케이터/발전과제/레시피를 별도 스레드로 뺀 뒤 처음)
- [ ] 코디네이터에 아직 남은 직렬 작업 측정: join/leave, `Signal::Update`(플레이어당 매 틱), `command_state` 뮤텍스

## 4. 클라우드에서 돌릴 수 없는 통합 테스트
- [ ] `cargo test --workspace` 전체 (픽스처 월드 `vanilla/26.3/fixture-world` 필요한 `game::integration::*` 9개. CI에서는 통과 중, run #206)
- [ ] `tools/make_fixture_world.py`는 `eula=true`를 쓰므로 EULA에 동의한 뒤 직접 실행

## 5. 구현은 했지만 기본 동작만 단위 테스트로 확인한 것
- [ ] `/summon ... {NoAI:1b}` 와 NoAI 몹 저장/로드(월드 재로드 후에도 유지되는지)
- [ ] 피스톤이 서 있는 몹/플레이어를 같이 옮기는 동작(몹은 CI 통합 테스트 통과, 플레이어는 실제 클라이언트로 확인 필요)

## 아직 구현 안 된 것 (참고)
- `/test`, `/jfr`(의도적으로 실패), 발전과제 트리거 (`recipe_crafted`는 작업대와 대장간 제작(장식 포함)만 구현: 재료 조건은 아직) `bred_animals`/`tame_animal`/`item_used_on_block` (`location`은 바이옴 조건만 구현, 구조물/좌표 조건은 아직)
- loot_table/function 보상, `seen_recipe`, 웨이포인트 invisible 필터 (전송/수신 범위와 호박·머리 숨김은 구현)
- Bukkit `call:` 훅, 월드젠 `process`/`remote` 모드
