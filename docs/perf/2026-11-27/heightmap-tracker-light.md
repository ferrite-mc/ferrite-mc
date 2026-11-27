# 높이맵 · 트래커 · 조명 블록 변경 (2026-11-27)

기준 커밋 `6e6ffd64` (base)와 이 트리(head)를 같은 머신에서 번갈아 3회(트래커는 5회)
실행한 **best-of-N**입니다. 절대값은 공유 부하에 따라 흔들리므로 같은 표의 base↔head
차이만 읽으세요. 모든 변경은 동작(패킷 바이트, 조명 값, 높이맵 값)을 바꾸지 않습니다.

## 1. `Chunk::set_block` 높이맵 갱신

`crates/ferrite-world/src/chunk.rs`

- 상태 교체 전후의 높이맵 비트마스크를 한 번만 계산하고, 같으면 높이맵 작업을 통째로
  건너뜁니다. 돌↔흙, 흙↔잔디, 경작지 수분처럼 조명/높이맵 membership이 같은 변경이
  대부분입니다.
- 블록이 빠져 높이맵이 내려갈 때, 예전에는 높이맵 종류마다 컬럼을 처음부터 다시
  스캔했습니다(최대 6회, 각 스캔마다 `info.flags` 호출). 이제 내려가야 하는 맵들을
  모아 컬럼을 **한 번만** 스캔하면서 각 맵의 새 높이를 동시에 찾습니다.

```sh
cargo run --release -p ferrite-world --example heightmap_bench
```

| probe | base | head | 배율 |
|---|---:|---:|---:|
| 전체 블록 교체, 높이맵 동일 | 50.8 ns/set | 28.9 ns/set | **1.76x** |
| 공중 블록 제거+복원 (아래 364블록 스캔) | 7552 ns/pair | 1223 ns/pair | **6.17x** |

## 2. 엔티티 트래커 — 타입 테이블 + 중복 메타데이터 동기화 제거

`crates/ferrite-entity/src/tracker.rs`, `crates/ferrite-entity/src/metadata.rs`

- `TrackingInfo::of`가 엔티티마다 11개 `NO_DELTA_TYPES` 문자열 비교와 아이템 키 비교를
  했습니다. 엔티티 타입 수만큼 미리 계산한 `OnceLock` 테이블 조회로 바꿨습니다.
- `track_slice`가 틱마다 `sync_state_metadata`를 호출한 뒤 `send_changes_arc`가 같은
  함수를 한 번 더 호출했습니다. 두 번째 호출을 제거했습니다.
- `set_custom_name`은 이름이 그대로여도 매 틱 `String`을 새로 할당했습니다. 현재 값과
  먼저 비교해 같으면 할당 없이 반환합니다.

```sh
cargo run --release -p ferrite-entity --example tracker_bench
# 400 zombies x 8 viewers x 300 ticks
```

| 실행 | base | head |
|---|---:|---:|
| 1 | 430.3 ns/entity-tick | 342.5 ns/entity-tick |
| 2 | 435.9 | 332.2 |
| 3 | 453.0 | 365.3 |
| 4 | 432.1 | 337.5 |
| 5 | 427.0 | 332.8 |
| best | 427.0 | **332.2** (**1.29x**) |

두 바이너리 모두 같은 1,287,872 bytes를 전송했습니다(패킷 내용/수 동일).

```sh
cargo run --release -p ferrite-entity --example metadata_bench
```

| probe | base | head | 배율 |
|---|---:|---:|---:|
| 이름이 같은 `set_custom_name` | 16.48 ns/call | 8.38 ns/call | **1.97x** |

## 3. 조명 — 빛 속성이 같은 블록 변경은 relight 생략

`crates/ferrite-light/src/lib.rs`

`LightEngine::block_changed`는 블록 상태가 바뀌면 무조건 블록/하늘 조명 제거·재전파와
전 섹션 `compact`(Data 배열당 2 KiB 스캔)를 수행했습니다. 이제 이전/새 `LightProperties`
(방출, 감쇠, 면 가림)를 비교합니다.

- 완전히 같으면: 큐 작업과 `compact` 없이 반환.
- 방출만 다르면: 블록 조명만 relight.
- 감쇠/면만 다르면: 블록+하늘 조명 모두 relight.

돌↔흙처럼 빛 속성이 같은 변경은 조명 결과가 바뀔 수 없으므로 생략이 안전합니다.
회귀 테스트 `block_changed_skips_identical_properties_but_relights_a_removal`가
돌↔흙 스왑은 조명을 바꾸지 않고, 블록 제거는 하늘 컬럼을 다시 여는지 확인합니다.

```sh
cargo run --release -p ferrite-light --example block_change
```

| probe | base (median) | head (median) |
|---|---:|---:|
| 표면 컬럼 (stone↔air) | 7.1 us | 7.1 us |
| 빌드 리밋 근처 (stone↔air) | 69.9 us | 68.4 us |
| **빛 속성 동일 스왑 (stone↔dirt)** | **5.4 us** | **< 0.1 us** |

빛이 실제로 바뀌는 두 프로브는 그대로이고, 바뀌지 않는 스왑만 타이머 해상도 아래로
떨어졌습니다.

## 시도했지만 되돌린 변경

`ferrite-entity` 충돌 스위퍼의 `overlaps_on_other_axes`를 `Axis` match로 바꾸고, 검색
영역 내부 셀의 `intersects`를 생략하는 fast path를 넣어 측정했습니다. 4-shape
픽스처에서 수집 113→115 ns, 스윕 21.6→23.7 ns로 **일관되게 느려져** 전부 되돌렸습니다
(측정용 벤치도 함께 제거).

## 검증

```sh
cargo test -p ferrite-world -p ferrite-entity -p ferrite-light
cargo clippy -p ferrite-world -p ferrite-entity -p ferrite-light --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check --workspace --all-targets
```

`ferrite-world` 487개를 포함해 세 크레이트와 통합 테스트 전부 통과, clippy/fmt 클린,
워크스페이스 전체 체크 성공. 200-bot MSPT A/B는 실행하지 않았습니다(마이크로벤치만).
