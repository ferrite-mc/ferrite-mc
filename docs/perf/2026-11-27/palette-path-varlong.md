# 팔레트 레인 / A* / VarLong (2026-11-27)

세 가지 핫패스 변경과 그 측정입니다. 기준 커밋은 `6e6ffd64` (base), 변경 트리는
`yxshadxw/agent-desk-oyv89-1` (head)입니다. 같은 머신(12 vCPU Xeon Platinum 8358,
Linux, 공유 부하)에서 base와 head 바이너리를 번갈아 3회씩 실행하고 **best-of-3**을
기록했습니다. 절대값은 부하에 따라 흔들리므로 같은 표 안의 base↔head 차이만 읽으세요.

## 1. `PalettedContainer::get` / `set` — 레인 나눗셈 제거

`crates/ferrite-world/src/lib.rs`

기존 `get`/`write`는 엔트리마다 `index / per`(per = 64 / bits)를 계산했습니다. bits가
5·6·7이면 `per`가 12·10·9라 정수가 아닌 나눗셈이 남고, 블록 읽기 한 번마다 이 비용을
치렀습니다. 이제 비트폭을 const generic(`bits_read::<BITS>` / `bits_write::<BITS>`)으로
매칭해 `index / per`와 레인 시프트가 곱셈-시프트로 접힙니다. 9·17~31 같은 희귀 폭은
기존 런타임 경로로 폴백합니다. 팔레트 레이아웃과 네트워크 바이트는 바뀌지 않습니다
(`from_states_u16` → `get`/`set`/`encode_network` 왕복 테스트 통과).

```sh
cargo run --release -p ferrite-world --example palette_bench
```

| bits | get base (ns/op) | get head (ns/op) | 배율 | set base (ns/op) | set head (ns/op) | 배율 |
|---:|---:|---:|---:|---:|---:|---:|
| 4 | 4.51 | 3.31 | **1.36x** | 23.74 | 19.25 | **1.23x** |
| 5 | 4.43 | 3.67 | **1.21x** | 29.46 | 24.87 | **1.18x** |
| 6 | 4.40 | 3.66 | **1.20x** | 37.21 | 34.28 | 1.09x |
| 7 | 4.42 | 3.60 | **1.23x** | 56.64 | 54.02 | 1.05x |
| 8 | 4.44 | 3.29 | **1.35x** | 81.23 | 78.39 | 1.04x |

bits 8의 get이 1.35x인 이유는 팔레트 조회가 아니라 레인 산술 비중이 커서입니다.
`set`은 `counts` 이진 탐색과 Vec 삽입이 지배해 배율이 낮습니다.

## 2. A* — 이완당 해시 프로브 1회

`crates/ferrite-entity/src/path.rs`

`try_relax`가 `nodes`를 두 번 조회했습니다(`closed` 확인 → `g` 확인). 한 번의
`match nodes.get(..)`로 합쳤고, 탐색 시작 시 `nodes`/`eval_cache`/`open`을
`node_budget` 기반으로 선할당해 확장 중 재해시를 없앴습니다. `clear()`가 할당을
유지하므로 실제 할당은 첫 탐색(또는 더 큰 예산의 탐색)에서만 일어납니다.
경로 결과는 그대로입니다 — 기존 path 테스트 487개와 벽 우회/계단/낙하 케이스가 모두
통과합니다.

```sh
cargo run --release -p ferrite-entity --example path_bench
```

| 실행 | base (us/path) | head (us/path) |
|---|---:|---:|
| 1 | 583.4 | 470.2 |
| 2 | 589.8 | 474.3 |
| 3 | 586.6 | 465.3 |
| best | 586.6 | **465.3** (**1.26x**) |

픽스처는 `x=8`에 3블록 높이 벽이 `z=-12..=12`를 막는 평지에서 `(0,64,0)` →
`(12,64,0)` 경로를 400회 찾습니다(웨이포인트 27개).

## 3. `VarLong::decode` 1~2바이트 fast path

`crates/ferrite-protocol/src/codec.rs`

블록 위치와 엔티티 id는 대부분 1~2바이트 VarLong입니다. `VarInt`와 같은 조기 반환을
넣어 지배적인 경우에 10회 루프를 건너뜁니다. 소비 바이트 수는 그대로이며,
`varlong_decode_fast_path_consumes_exactly_one_or_two_bytes`가 뒤따르는 꼬리 바이트가
남는지 확인합니다. 기존 VarLong 라운드트립/오버런 테스트도 그대로 통과합니다.

## 검증

```sh
cargo test -p ferrite-world -p ferrite-protocol -p ferrite-entity
cargo clippy -p ferrite-world -p ferrite-protocol -p ferrite-entity --all-targets -- -D warnings
cargo fmt --all -- --check
```

`ferrite-world` 487, `ferrite-protocol` 73, `ferrite-entity` 및 워크스페이스 통합
테스트 전부 통과, clippy/fmt 클린.
