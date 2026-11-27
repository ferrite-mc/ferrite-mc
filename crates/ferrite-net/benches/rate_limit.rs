//! Per-IP connect limiter cost on the accept path.
//!
//! `check` runs once per accepted TCP connection. Two numbers matter: the hit path for an
//! address that is already tracked, and a miss while the tracked set is full, which has to
//! evict something before inserting. The miss case is the one an IP-rotation flood drives, so
//! it must not scan the whole map per connection.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Instant;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use ferrite_net::rate_limit::{BucketConfig, IpConnectionLimiter};
use std::hint::black_box;

/// Addresses are spread over 10.0.0.0/8 so hashing is realistic.
fn ip(i: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + i))
}

fn limiter(tracked: u32, now: Instant) -> IpConnectionLimiter {
    let mut limiter = IpConnectionLimiter::new(
        BucketConfig {
            burst: 64,
            per_second: 8.0,
        },
        false,
    );
    for i in 0..tracked {
        assert!(limiter.check(ip(i), now));
    }
    limiter
}

fn bench_ip_limiter(c: &mut Criterion) {
    // Fixed timestamps: no bucket ages out mid-measurement, so the fill stays at the cap.
    let now = Instant::now();
    let cap = 4096;

    let mut group = c.benchmark_group("ip_limiter");
    group.throughput(Throughput::Elements(1));

    group.bench_function("already_tracked", |b| {
        let mut limiter = limiter(cap, now);
        b.iter(|| black_box(limiter.check(black_box(ip(1234)), now)));
    });

    group.bench_function("miss_at_capacity", |b| {
        let mut limiter = limiter(cap, now);
        let mut next = cap;
        b.iter(|| {
            next += 1;
            black_box(limiter.check(black_box(ip(next)), now))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_ip_limiter);
criterion_main!(benches);
