use ferrite_net::{ServerConfig, serve};
use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        configuration::{
            AcknowledgeFinishConfiguration, ClientInformation, ClientboundKnownPacks,
            ServerboundKnownPacks,
        },
        handshake::{Intent, Intention},
        login::{LoginAcknowledged, LoginStart},
        play::{
            ChunkBatchReceived, ClientKeepAlive, ConfirmTeleport, MovePosition, PlayKeepAlive,
            PositionSync,
        },
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
struct Bot {
    stream: TcpStream,
    encoder: FrameEncoder,
    decoder: FrameDecoder,
    /// The last packet ids received, reported when the server closes the connection.
    recent: std::collections::VecDeque<i32>,
    /// Where the server last placed this bot.
    placed: [f64; 3],
}
impl Bot {
    async fn send<P: Packet>(&mut self, p: &P) {
        let mut body = Vec::new();
        encode_packet(p, &mut body);
        let mut wire = Vec::new();
        self.encoder.encode(&body, &mut wire);
        self.stream.write_all(&wire).await.unwrap();
    }
    async fn read(&mut self) -> Vec<u8> {
        tokio::time::timeout(std::time::Duration::from_secs(120), async {
            loop {
                if let Some(frame) = self.decoder.next_frame().unwrap() {
                    return frame;
                }
                let mut bytes = [0u8; 8192];
                let n = self.stream.read(&mut bytes).await.unwrap();
                assert_ne!(
                    n, 0,
                    "server closed the connection; last packet ids {:?}",
                    self.recent
                );
                self.decoder.feed(&bytes[..n]);
            }
        })
        .await
        .unwrap()
    }
    async fn next_play(&mut self) -> (i32, Vec<u8>) {
        let bytes = self.read().await;
        let mut body = bytes.as_slice();
        let id = VarInt::decode(&mut body).unwrap().0;
        let rest = body.to_vec();
        if self.recent.len() == 12 {
            self.recent.pop_front();
        }
        self.recent.push_back(id);
        match id {
            73 => {
                let mut b = rest.as_slice();
                let p = PositionSync::decode(&mut b).unwrap();
                self.placed = [p.x, p.y, p.z];
                self.send(&ConfirmTeleport {
                    id: p.teleport_id,
                    x: p.x,
                    y: p.y,
                    z: p.z,
                    yaw: p.yaw,
                    pitch: p.pitch,
                })
                .await;
            }
            // Other servers kick a client that never answers (Ferrite does not ask).
            45 => {
                let p = PlayKeepAlive::decode(&mut rest.as_slice()).unwrap();
                self.send(&ClientKeepAlive { id: p.id }).await;
            }
            11 => {
                self.send(&ChunkBatchReceived {
                    chunks_per_tick: 64.0,
                })
                .await
            }
            _ => {}
        }
        (id, rest)
    }
    async fn until(&mut self, id: i32) -> Vec<u8> {
        loop {
            let (got, body) = self.next_play().await;
            if got == id {
                return body;
            }
        }
    }
    async fn join(addr: std::net::SocketAddr, name: &str) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        let mut b = Self {
            stream,
            encoder: FrameEncoder::default(),
            decoder: FrameDecoder::default(),
            recent: Default::default(),
            placed: [0.0; 3],
        };
        b.send(&Intention {
            protocol_version: 777,
            server_address: "localhost".into(),
            server_port: addr.port(),
            intent: Intent::Login,
        })
        .await;
        b.send(&LoginStart {
            name: name.into(),
            uuid: uuid::Uuid::nil(),
        })
        .await;
        let first = b.read().await;
        assert_eq!(VarInt::decode(&mut first.as_slice()).unwrap().0, 3);
        b.encoder.set_compression(Some(256), 6);
        b.decoder.set_compression(Some(256));
        let login = b.read().await;
        assert_eq!(VarInt::decode(&mut login.as_slice()).unwrap().0, 2);
        b.send(&LoginAcknowledged).await;
        // Other servers send only as far as the client says it can see.
        b.send(&ClientInformation {
            locale: "en_us".into(),
            view_distance: std::env::var("BOTS_VIEW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8),
            chat_mode: VarInt(0),
            chat_colors: true,
            skin_parts: 0x7f,
            main_hand: VarInt(1),
            text_filtering: false,
            server_listing: true,
            particle_status: VarInt(0),
        })
        .await;
        loop {
            let frame = b.read().await;
            let mut s = frame.as_slice();
            let id = VarInt::decode(&mut s).unwrap().0;
            if id == ClientboundKnownPacks::ID {
                let p = ClientboundKnownPacks::decode(&mut s).unwrap();
                b.send(&ServerboundKnownPacks { packs: p.packs }).await;
            }
            if id == 3 {
                break;
            }
        }
        b.send(&AcknowledgeFinishConfiguration).await;
        b.until(73).await;
        b
    }
}

#[cfg(windows)]
fn process_usage() -> (u64, u64) {
    #[repr(C)]
    struct Memory {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            p: *mut std::ffi::c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(p: *mut std::ffi::c_void, info: *mut Memory, size: u32) -> i32;
    }
    let mut mem: Memory = unsafe { std::mem::zeroed() };
    mem.cb = std::mem::size_of::<Memory>() as u32;
    let (mut created, mut exited, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    unsafe {
        let p = GetCurrentProcess();
        GetProcessMemoryInfo(p, &mut mem, std::mem::size_of::<Memory>() as u32);
        GetProcessTimes(p, &mut created, &mut exited, &mut kernel, &mut user);
    }
    let ticks = |t: FileTime| (u64::from(t.high) << 32) | u64::from(t.low);
    (mem.working_set as u64, ticks(kernel) + ticks(user))
}
/// Resident set size in bytes and CPU time in 100 ns units, read from `/proc/self`.
#[cfg(target_os = "linux")]
fn process_usage() -> (u64, u64) {
    let rss_pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0);
    // Fields 14 and 15 (utime, stime) in clock ticks; the command name may contain spaces, so
    // count from the closing parenthesis.
    let cpu_ticks = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            let rest = s.rsplit_once(')')?.1.to_owned();
            let f: Vec<&str> = rest.split_whitespace().collect();
            Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
        })
        .unwrap_or(0);
    // _SC_CLK_TCK is 100 on Linux, so one tick is 10 ms = 100_000 units of 100 ns.
    (rss_pages * 4096, cpu_ticks * 100_000)
}
#[cfg(not(any(windows, target_os = "linux")))]
fn process_usage() -> (u64, u64) {
    (0, 0)
}

async fn run_world(path: Option<std::path::PathBuf>, n: usize) {
    let label = if path.is_some() {
        "fixture"
    } else {
        "superflat"
    };
    let mut initial = [0.5, 65.0, 0.5];
    let world = if let Some(path) = path {
        let registry = ferrite_world::RuntimeRegistry::load("vanilla/26.3/generated").unwrap();
        let spawn = ferrite_server::game::load_spawn(&path).unwrap();
        initial = [
            spawn[0] as f64 + 0.5,
            spawn[1] as f64,
            spawn[2] as f64 + 0.5,
        ];
        ferrite_server::game::World::start_with(
            Some(ferrite_server::game::new_anvil_source(path, registry)),
            spawn,
        )
    } else {
        ferrite_server::game::World::start()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        ServerConfig {
            online_mode: false,
            max_players: n as u32 + 1,
            view_distance: 2,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
    ferrite_server::game::RECORD_COORDINATOR.store(true, std::sync::atomic::Ordering::Relaxed);
    ferrite_server::game::COORDINATOR_MS
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(Vec::clear);
    let start = tokio::time::Instant::now();
    let (_, cpu_start) = process_usage();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(n + 1));
    let mut tasks = Vec::new();
    for i in 0..n {
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            // `BOTS_STAGGER_MS` spreads the connects: 800 sockets opened in one instant overflow
            // the accept backlog and are reset before the server ever sees them.
            let stagger: u64 = std::env::var("BOTS_STAGGER_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            tokio::time::sleep(std::time::Duration::from_millis(i as u64 * stagger)).await;
            let mut bot = Bot::join(addr, &format!("Bot{i}")).await;
            barrier.wait().await;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
            let mut x = initial[0]; let mut z = initial[2]; let mut rng = i as u64 + 1;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => break,
                    _ = interval.tick() => { rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1); x += ((rng >> 32) % 3) as f64 - 1.0; z += ((rng >> 40) % 3) as f64 - 1.0; bot.send(&MovePosition {x,y:initial[1],z,flags:1}).await; },
                    _ = bot.next_play() => {},
                }
            }
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(120), barrier.wait())
        .await
        .expect("bots did not all join");
    // Join storm figures first, then restart the samples so the walk is measured on its own.
    {
        let mut samples = world.tick_samples.lock().unwrap();
        let mut joined = samples.clone();
        joined.sort_by(f64::total_cmp);
        if let Some(max) = joined.last() {
            println!(
                "{label} join phase: region ticks={}, p99={:.3} ms, max={max:.2} ms",
                joined.len(),
                joined[((joined.len() - 1) as f64 * 0.99) as usize]
            );
        }
        samples.clear();
    }
    // `BOTS_FORCELOAD=<cycles>` drives the batched holder path end to end while the bots walk:
    // `/forceload add` of 256 chunks (the command maximum) and `remove all` through the
    // console, timing each command from dispatch to its reply.
    if let Some(cycles) = std::env::var("BOTS_FORCELOAD")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        let mut add_ms = Vec::new();
        let mut remove_ms = Vec::new();
        for cycle in 0..cycles {
            // A distinct 16x16-chunk square per cycle keeps every add a real change.
            let base = (cycle as i32 % 8) * 256;
            let t = std::time::Instant::now();
            world
                .console_command(format!("forceload add {base} 0 {} 255", base + 255))
                .await
                .unwrap();
            add_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = std::time::Instant::now();
            world
                .console_command("forceload remove all".into())
                .await
                .unwrap();
            remove_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        for (name, mut v) in [("add 256 chunks", add_ms), ("remove all", remove_ms)] {
            v.sort_by(f64::total_cmp);
            println!(
                "{label} forceload {name}: cycles={}, mean={:.3} ms, p99={:.3} ms, max={:.3} ms",
                v.len(),
                v.iter().sum::<f64>() / v.len() as f64,
                v[((v.len() - 1) as f64 * 0.99) as usize],
                v[v.len() - 1]
            );
        }
    }
    for task in tasks {
        task.await.unwrap();
    }
    let elapsed = start.elapsed().as_secs_f64();
    let (rss, cpu_end) = process_usage();
    let mut samples = world.tick_samples.lock().unwrap().clone();
    samples.sort_by(f64::total_cmp);
    let avg = samples.iter().sum::<f64>() / samples.len() as f64;
    let p99 = samples[((samples.len() - 1) as f64 * 0.99) as usize];
    let cache = world.cache_stats();
    println!(
        "{label}: bots={n}, avg_mspt={avg:.3}, p99_mspt={p99:.3}, rss_mb={:.1}, cpu_percent={:.1}, samples={}, cache_hit_rate={:.1}%, encodes={}, lookups={}",
        rss as f64 / 1048576.0,
        (cpu_end - cpu_start) as f64 / 10_000_000.0 / elapsed * 100.0,
        samples.len(),
        cache.hit_rate() * 100.0,
        cache.encodes,
        cache.lookups
    );
    {
        let arms = ferrite_server::game::COORDINATOR_MS.lock().unwrap();
        for (name, runs) in [
            "events",
            "signals",
            "clock",
            "  head (before game.tick)",
            "  locator",
            "  advancements",
            "  recipes",
            "  scan",
            "  ride",
            "  rest",
        ]
        .iter()
        .zip(arms.iter())
        {
            let mut runs = runs.clone();
            runs.sort_by(f64::total_cmp);
            let total: f64 = runs.iter().sum();
            if let Some(max) = runs.last() {
                println!(
                    "  coordinator {name}: runs={}, total={total:.0} ms ({:.1}% of the run), mean={:.4} ms, p99={:.3} ms, max={max:.2} ms",
                    runs.len(),
                    total / 10.0 / elapsed,
                    total / runs.len() as f64,
                    runs[((runs.len() - 1) as f64 * 0.99) as usize],
                );
            }
        }
    }
    println!("{}", world.profiler().timings().report_all(50.0));
    world.shutdown().await.unwrap();
    server.abort();
}
/// Walks `n` bots on an already running server (`BOTS_ADDR=host:port`)
/// for 40 s so another server can be compared under the same load. The server's own tick figures
/// are read from that server; this prints `WALKING` once everyone has joined.
async fn run_external(addr: std::net::SocketAddr, n: usize) {
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(n + 1));
    let mut tasks = Vec::new();
    for i in 0..n {
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            // Paper's accept backlog resets a burst of 800 connects at once; spread them.
            tokio::time::sleep(std::time::Duration::from_millis(i as u64 * 5)).await;
            let mut bot = Bot::join(addr, &format!("Bot{i}")).await;
            // Let the server settle (others lag right after a join burst, and kick a client whose
            // two moves land in one lagging tick) while answering its packets.
            let settle = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(settle) => break,
                    _ = bot.next_play() => {},
                }
            }
            barrier.wait().await;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(40);
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(1000));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Other servers kick a client that sends two position packets in one tick, which a
            // burst of missed interval ticks (after the join wait) would do.
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let [mut x, y, mut z] = bot.placed;
            let mut rng = i as u64 + 1;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => break,
                    _ = interval.tick() => { rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1); x += ((rng >> 32) % 3) as f64 - 1.0; z += ((rng >> 40) % 3) as f64 - 1.0; bot.send(&MovePosition {x,y,z,flags:1}).await; },
                    _ = bot.next_play() => {},
                }
            }
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(300), barrier.wait())
        .await
        .expect("bots did not all join");
    println!("WALKING");
    for task in tasks {
        task.await.unwrap();
    }
    println!("DONE");
}
/// One bot joins a running server (`BOTS_ADDR`, `BOTS_CHUNKS=1`) and counts the chunks it
/// receives until none arrives for 8 s: how fast the server makes and sends the area around the
/// spawn. Prints `CHUNKS <count> <seconds to the last one> <chunks per second>`.
async fn run_chunks(addr: std::net::SocketAddr) {
    let mut bot = Bot::join(addr, "ChunkBot").await;
    let start = std::time::Instant::now();
    let mut count = 0u32;
    let mut last = start;
    // Other packets (time, entities) keep arriving, so the 8 s of quiet run from the last chunk
    // and are checked on every packet, not only when the socket is idle.
    while last.elapsed() < std::time::Duration::from_secs(8) {
        let Ok((id, _)) =
            tokio::time::timeout(std::time::Duration::from_secs(8), bot.next_play()).await
        else {
            break;
        };
        if id == 46 {
            count += 1;
            last = std::time::Instant::now();
        }
    }
    let seconds = (last - start).as_secs_f64();
    println!(
        "CHUNKS {count} {seconds:.1} {:.1}",
        f64::from(count) / seconds.max(0.001)
    );
}
#[tokio::main]
async fn main() {
    let n = std::env::args()
        .nth(1)
        .map(|s| s.parse().unwrap())
        .unwrap_or(200);
    if let Ok(addr) = std::env::var("BOTS_ADDR") {
        if std::env::var("BOTS_CHUNKS").is_ok() {
            run_chunks(addr.parse().unwrap()).await;
            return;
        }
        let n = std::env::args()
            .nth(1)
            .map(|s| s.parse().unwrap())
            .unwrap_or(200);
        run_external(addr.parse().unwrap(), n).await;
        return;
    }
    run_world(None, n).await;
    run_world(Some("vanilla/26.3/fixture-world/world".into()), n).await;
}
