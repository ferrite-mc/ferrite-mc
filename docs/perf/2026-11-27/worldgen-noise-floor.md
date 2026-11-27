# 월드젠 노이즈: `f64::floor` 인라인화 (2026-11-27)

`crates/ferrite-worldgen/src/noise.rs`

지형 생성은 코너(4x8x4 셀)마다 `BlendedNoise`(구 `old_blended_noise`, 옥타브 8+16+16개)와
`NormalNoise`(동굴·기후·누들)를 평가합니다. 프로파일에서 `floor`가 전체의 5.1%를 차지했고,
심볼이 따로 잡힌다는 것은 인라인 `roundsd`가 아니라 **libm 호출**이라는 뜻입니다. 기본
x86-64 타깃에는 SSE4.1이 없어 `f64::floor`가 아웃오브라인 호출로 남습니다.

## 변경

노이즈 좌표는 `PerlinNoise::wrap`이 `±16,777,216`로 제한하므로 항상 `i32`에 들어갑니다.
그래서 절단 캐스트 + 보정으로 바꿨습니다.

```rust
#[inline(always)]
fn floor_split(v: f64) -> (i32, f64) {
    if v.abs() < 2_147_483_648.0 {
        let mut i = v as i32;          // trunc
        let mut f = v - f64::from(i);
        if f < 0.0 { i -= 1; f += 1.0; }
        (i, f)
    } else {
        let f = v.floor();             // 범위 밖은 기존 그대로
        (f as i32, v - f)
    }
}
```

`ImprovedNoise::sample_2d_xz` / `prep_xz` / `sample_prepared_y` / `sample_inner`,
`SimplexNoise::sample_2d`, 그리고 스미어 항의 `(...).floor()`에 적용했습니다. `|v| ≥ 2^31`은
폴백이라 `sample()`을 직접 호출하는 테스트나 특수 좌표에서도 동작이 같습니다.

## 측정

```sh
cargo run --release -p ferrite-worldgen --example terrain_bench
# 지형+동굴+바이옴+표면 패스만 (구조물/장식/조명/섹션 패킹 제외)
# 시드 12345, 8x8=64청크, best-of-5, 3회 실행
```

| 실행 | base | head |
|---|---:|---:|
| 1 | 2565.0 µs/chunk | 2336.1 µs/chunk |
| 2 | 2550.2 | 2283.9 |
| 3 | 2538.8 | 2312.2 |
| best | 2538.8 | **2283.9** (**1.11x**) |

64청크 블록 상태 FNV-1a 해시는 base/head 모두 `9e3129d9e68c7b08`로 **동일**합니다.

`perf record` (cycles, `--no-children`, 변경 후):

| 심볼 | base | head |
|---|---:|---:|
| `PerlinNoise::get_value` | 32.42% | 33.47% |
| `DensityFunction::compute` (블렌디드 노이즈 포함) | 29.52% | 31.60% |
| `DensityFunction::compute_cached` | 9.17% | 9.29% |
| `RTreeNode::search` (바이옴) | 8.37% | 8.40% |
| `generate_chunk_into_u16` | 7.36% | 7.85% |
| `floor` | 5.13% | **사라짐** |

남은 시간의 대부분은 노이즈 수식 자체(`BlendedNoise` 40옥타브 + `NormalNoise`)라, 바닐라와
같은 알고리즘을 그대로 두는 한 더 줄이기 어렵습니다.

## 시도했다가 되돌린 것 (측정 결과 무효)

| 실험 | 결과 |
|---|---|
| 표면/네더 재질 패스를 y-우선(행 단위) 순회로 전치 | 2542–2558 µs, 변화 없음 |
| 2차 함수 필요 여부를 3x3x3 이웃 스캔 대신 분리형 dilation 3패스로 계산 | 2532–2548 µs, 변화 없음 |
| 바이옴 R-tree에서 `best <= 0` 조기 종료 | 2303–2342 µs, 변화 없음 |
| `BlendedNoise` 평가를 0으로 치환 | 1695 µs지만 지형 자체가 바뀌어 비중 측정으로만 사용 |

## 검증

- `cargo test -p ferrite-worldgen` — 단위 9개 + `tests/vanilla_parity.rs` 3개(바닐라 픽스처
  청크 비교 포함) 모두 통과.
- `cargo clippy -p ferrite-worldgen --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `cargo check --workspace --all-targets` 클린.

## 수신(네트워킹) 경로 조사 결과

이번 라운드에서 클라이언트 수신 경로도 같이 봤지만, 측정 가능한 낭비가 없어 **변경하지
않았습니다**. 이미 (1) 프레임을 복사 없이 빌려 디코드하고(`FrameDecoder::next_frame_ref`),
(2) 압축 해제는 재사용 스크래치에, (3) 브로드캐스트는 한 번 압축한 `SharedFrame`을 공유하며,
(4) 4 KiB 이상 배치는 압축·암호화를 `spawn_blocking`으로 넘깁니다. 남은 비용은 소켓
I/O와 zlib 자체입니다.

## 다음 후보

1. `BlendedNoise` 옥타브별 y-의존 값(`yi`, `dy_grad`, `smoothstep`) 메모 — y 값이 청크마다
   반복되므로 표로 캐시할 수 있으나, 표 접근 비용이 절감분을 넘을 수 있어 측정 필요.
2. 바이옴 R-tree 자식 정렬 — 가지치기는 좋아지지만 **동점 처리 순서가 바뀌어** 바닐라
   패리티가 깨질 수 있음(정렬 전 별도 검증 필요).
3. `NormalNoise` SIMD — 결과 비트가 달라지므로 패리티 전략을 먼저 정해야 함.
