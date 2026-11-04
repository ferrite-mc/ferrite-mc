use ferrite_net::{ServerConfig, serve};
use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        configuration::{
            AcknowledgeFinishConfiguration, ClientboundKnownPacks, ServerboundKnownPacks,
        },
        handshake::{Intent, Intention},
        login::{LoginAcknowledged, LoginStart},
        play::{ChunkBatchReceived, ConfirmTeleport, MovePosition, PositionSync},
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
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                if let Some(frame) = self.decoder.next_frame().unwrap() {
                    return frame;
                }
                let mut bytes = [0u8; 8192];
                let n = self.stream.read(&mut bytes).await.unwrap();
                assert_ne!(n, 0);
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
        match id {
            73 => {
                let mut b = rest.as_slice();
                let p = PositionSync::decode(&mut b).unwrap();
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
#[cfg(not(windows))]
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
    let start = tokio::time::Instant::now();
    let (_, cpu_start) = process_usage();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(n + 1));
    let mut tasks = Vec::new();
    for i in 0..n {
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
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
    world.shutdown().await.unwrap();
    server.abort();
}
#[tokio::main]
async fn main() {
    let n = std::env::args()
        .nth(1)
        .map(|s| s.parse().unwrap())
        .unwrap_or(200);
    run_world(None, n).await;
    run_world(Some("vanilla/26.3/fixture-world/world".into()), n).await;
}
