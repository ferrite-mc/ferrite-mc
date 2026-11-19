//! End-to-end chunk holder batching benchmark: compares `set_batch_chunk_holders(true)`
//! (`Signal::HoldBatch` -> `add_chunks` / `remove_chunks` / `schedule_at_chunks`, one scheduler
//! topology lock and wake per packet or command batch) against `false` (one `Signal::Hold`,
//! `add_chunk`, `remove_chunk`, and `schedule_at_chunk` per chunk).
//!
//! Run with:
//! `cargo run --release -p ferrite-server --example holders_e2e`
//!
//! Optional env vars:
//! - `HOLDERS_RUNS=2`: number of interleaved batched/unbatched pairs
//! - `FORCELOAD_CYCLES=40`: 256-chunk add+remove cycles per run
//! - `STREAM_BOTS=32`: spread bots (`view_distance=8`, 289 chunks each, disjoint regions)
//! - `STREAM_WAVES=4`: disjoint `/tp` waves across the spread bots
//! - `HOLDERS_STREAM=1`: also run the player chunk-stream suites. Off by default because the
//!   stream harness has not been validated end to end and can stall after joining.
//! - `HOLDERS_UNPACED=1`: also run the `/tick rate 10000` variants. Off by default: at that rate
//!   the coordinator's event branch can starve behind the always-ready clock, so even the
//!   rate-change command may never complete. The 20 TPS `/forceload` suite is the supported
//!   measurement.

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
            ChunkBatchFinished, ChunkBatchReceived, ChunkData, ClientKeepAlive, ConfirmTeleport,
            PlayKeepAlive, PositionSync,
        },
    },
};
use ferrite_region::ChunkPos;
use ferrite_server::game::{COORDINATOR_MS, Dimension, RECORD_COORDINATOR, World};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn square_chunks(base_chunk_x: i32, base_chunk_z: i32, side: i32) -> Vec<ChunkPos> {
    (base_chunk_x..base_chunk_x + side)
        .flat_map(|x| (base_chunk_z..base_chunk_z + side).map(move |z| ChunkPos(x, z)))
        .collect()
}

fn window_chunks(center_x: i32, center_z: i32, view: i32) -> Vec<ChunkPos> {
    (center_x - view..=center_x + view)
        .flat_map(|x| (center_z - view..=center_z + view).map(move |z| ChunkPos(x, z)))
        .collect()
}

fn stats(mut v: Vec<f64>) -> (f64, f64, f64, f64) {
    v.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let p50 = v[(v.len() - 1) / 2];
    let p99 = v[((v.len() - 1) as f64 * 0.99) as usize];
    let max = v[v.len() - 1];
    (mean, p50, p99, max)
}

fn reset_metrics(world: &World) {
    RECORD_COORDINATOR.store(true, std::sync::atomic::Ordering::Relaxed);
    COORDINATOR_MS
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(Vec::clear);
    world.tick_samples.lock().unwrap().clear();
}

fn coordinator_signals_summary() -> (usize, f64, f64) {
    let arms = COORDINATOR_MS.lock().unwrap();
    let runs = &arms[1];
    let total_ms: f64 = runs.iter().sum();
    let mean_ms = if runs.is_empty() {
        0.0
    } else {
        total_ms / runs.len() as f64
    };
    (runs.len(), total_ms, mean_ms)
}

struct ForceloadResult {
    batched: bool,
    total_s: f64,
    add_mean_ms: f64,
    add_p50_ms: f64,
    add_p99_ms: f64,
    remove_mean_ms: f64,
    remove_p50_ms: f64,
    remove_p99_ms: f64,
    cmd_add_mean_ms: f64,
    cmd_remove_mean_ms: f64,
    chunks_per_sec: f64,
    sig_runs: usize,
    sig_total_ms: f64,
}

async fn bench_forceload(batched: bool, cycles: usize, tick_rate: Option<u32>) -> ForceloadResult {
    let world = World::start();
    world.set_batch_chunk_holders(batched);
    if let Some(rate) = tick_rate {
        world
            .console_command(format!("tick rate {rate}"))
            .await
            .unwrap();
    }

    // Warmup so scheduler worker threads and allocator state are live.
    for cycle in 0..2 {
        let base_chunk = cycle * 16;
        let base_block = base_chunk * 16;
        let chunks = square_chunks(base_chunk, 0, 16);
        world
            .console_command(format!(
                "forceload add {base_block} 0 {} 255",
                base_block + 255
            ))
            .await
            .unwrap();
        world
            .sync_chunk_holds(Dimension::Overworld, chunks.clone(), true)
            .await
            .unwrap();
        world
            .console_command("forceload remove all".into())
            .await
            .unwrap();
        world
            .sync_chunk_holds(Dimension::Overworld, chunks, false)
            .await
            .unwrap();
    }

    reset_metrics(&world);
    let mut add_e2e = Vec::with_capacity(cycles);
    let mut remove_e2e = Vec::with_capacity(cycles);
    let mut add_cmd = Vec::with_capacity(cycles);
    let mut remove_cmd = Vec::with_capacity(cycles);

    let total_start = Instant::now();
    for cycle in 0..cycles {
        let base_chunk = ((cycle % 16) as i32) * 16;
        let base_block = base_chunk * 16;
        let chunks = square_chunks(base_chunk, 0, 16);

        let t0 = Instant::now();
        world
            .console_command(format!(
                "forceload add {base_block} 0 {} 255",
                base_block + 255
            ))
            .await
            .unwrap();
        add_cmd.push(t0.elapsed().as_secs_f64() * 1000.0);
        world
            .sync_chunk_holds(Dimension::Overworld, chunks.clone(), true)
            .await
            .unwrap();
        add_e2e.push(t0.elapsed().as_secs_f64() * 1000.0);

        let t1 = Instant::now();
        world
            .console_command("forceload remove all".into())
            .await
            .unwrap();
        remove_cmd.push(t1.elapsed().as_secs_f64() * 1000.0);
        world
            .sync_chunk_holds(Dimension::Overworld, chunks, false)
            .await
            .unwrap();
        remove_e2e.push(t1.elapsed().as_secs_f64() * 1000.0);
    }
    let total_s = total_start.elapsed().as_secs_f64();
    let (sig_runs, sig_total_ms, _) = coordinator_signals_summary();
    world.shutdown().await.unwrap();

    let (add_mean_ms, add_p50_ms, add_p99_ms, _) = stats(add_e2e);
    let (remove_mean_ms, remove_p50_ms, remove_p99_ms, _) = stats(remove_e2e);
    let cmd_add_mean_ms = add_cmd.iter().sum::<f64>() / add_cmd.len() as f64;
    let cmd_remove_mean_ms = remove_cmd.iter().sum::<f64>() / remove_cmd.len() as f64;
    let total_changes = (cycles * 256 * 2) as f64;

    ForceloadResult {
        batched,
        total_s,
        add_mean_ms,
        add_p50_ms,
        add_p99_ms,
        remove_mean_ms,
        remove_p50_ms,
        remove_p99_ms,
        cmd_add_mean_ms,
        cmd_remove_mean_ms,
        chunks_per_sec: total_changes / total_s,
        sig_runs,
        sig_total_ms,
    }
}

struct Bot {
    stream: TcpStream,
    encoder: FrameEncoder,
    decoder: FrameDecoder,
    batch_open: bool,
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
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                if let Some(frame) = self.decoder.next_frame().unwrap() {
                    return frame;
                }
                let mut bytes = [0u8; 8192];
                let n = self.stream.read(&mut bytes).await.unwrap();
                assert_ne!(n, 0, "server closed connection unexpectedly");
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
            PositionSync::ID => {
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
            PlayKeepAlive::ID => {
                let p = PlayKeepAlive::decode(&mut rest.as_slice()).unwrap();
                self.send(&ClientKeepAlive { id: p.id }).await;
            }
            ChunkData::ID => {
                self.batch_open = true;
            }
            ChunkBatchFinished::ID => {
                self.batch_open = false;
                self.send(&ChunkBatchReceived {
                    chunks_per_tick: 64.0,
                })
                .await;
            }
            _ => {}
        }
        (id, rest)
    }

    async fn join(addr: std::net::SocketAddr, name: &str, view: i8) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        let mut b = Self {
            stream,
            encoder: FrameEncoder::default(),
            decoder: FrameDecoder::default(),
            batch_open: false,
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
        b.send(&ClientInformation {
            locale: "en_us".into(),
            view_distance: view,
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
        loop {
            let (id, _) = b.next_play().await;
            if id == PositionSync::ID {
                break;
            }
        }
        b
    }

    async fn wait_window(&mut self, cx: i32, cz: i32, view: i32) {
        let expected: HashSet<(i32, i32)> = window_chunks(cx, cz, view)
            .into_iter()
            .map(|c| (c.0, c.1))
            .collect();
        let mut seen = HashSet::with_capacity(expected.len());
        tokio::time::timeout(Duration::from_secs(60), async {
            while seen.len() < expected.len() || self.batch_open {
                let (id, body) = self.next_play().await;
                if id == ChunkData::ID {
                    let mut slice = body.as_slice();
                    let chunk = ChunkData::decode(&mut slice).unwrap();
                    if expected.contains(&(chunk.x, chunk.z)) {
                        seen.insert((chunk.x, chunk.z));
                    }
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "timed out waiting for chunk window ({cx}, {cz}): saw {}/{} chunks",
                seen.len(),
                expected.len()
            )
        });
    }
}

struct StreamResult {
    batched: bool,
    bots: usize,
    waves: usize,
    total_s: f64,
    wave_mean_ms: f64,
    wave_p50_ms: f64,
    wave_max_ms: f64,
    chunks_per_sec: f64,
    avg_mspt: f64,
    p99_mspt: f64,
    sig_runs: usize,
    sig_total_ms: f64,
}

async fn bench_player_stream(
    batched: bool,
    num_bots: usize,
    waves: usize,
    tick_rate: Option<u32>,
) -> StreamResult {
    const VIEW: i32 = 8;
    let world = World::start();
    world.set_batch_chunk_holders(batched);
    if let Some(rate) = tick_rate {
        world
            .console_command(format!("tick rate {rate}"))
            .await
            .unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        ServerConfig {
            online_mode: false,
            max_players: num_bots as u32 + 2,
            view_distance: VIEW as u8,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));

    let mut join_set = tokio::task::JoinSet::new();
    for i in 0..num_bots {
        join_set.spawn(async move {
            tokio::time::sleep(Duration::from_millis(i as u64 * 2)).await;
            let mut bot = Bot::join(addr, &format!("StreamBot{i}"), VIEW as i8).await;
            bot.wait_window(0, 0, VIEW).await;
            (i, bot)
        });
    }
    let mut joined: Vec<Option<Bot>> = (0..num_bots).map(|_| None).collect();
    while let Some(res) = join_set.join_next().await {
        let (idx, bot) = res.unwrap();
        joined[idx] = Some(bot);
    }
    let mut bots: Vec<Bot> = joined.into_iter().map(Option::unwrap).collect();

    // Move bots onto disjoint initial regions ((i + 1) * 32 chunks = 512 blocks apart in X)
    // before timing so every bot leaves chunk (0, 0) and holds its own 289-chunk window.
    for i in 0..num_bots {
        let x = (i as i32 + 1) * 512;
        world
            .console_command(format!("tp StreamBot{i} {x} 65 0"))
            .await
            .unwrap();
    }
    let targets: Vec<_> = (0..num_bots).map(|i| ((i as i32 + 1) * 32, 0)).collect();
    bots = wait_all_windows(bots, targets, VIEW).await;

    reset_metrics(&world);
    let mut wave_ms = Vec::with_capacity(waves);
    let total_start = Instant::now();

    for wave in 1..=waves {
        let z_block = wave as i32 * 512;
        let z_chunk = wave as i32 * 32;
        let prev_z_chunk = (wave as i32 - 1) * 32;

        let t = Instant::now();
        for i in 0..num_bots {
            let x = (i as i32 + 1) * 512;
            world
                .console_command(format!("tp StreamBot{i} {x} 65 {z_block}"))
                .await
                .unwrap();
        }
        let mut prev_chunks = Vec::with_capacity(num_bots * 289);
        let mut next_chunks = Vec::with_capacity(num_bots * 289);
        let mut targets = Vec::with_capacity(num_bots);
        for i in 0..num_bots {
            let cx = (i as i32 + 1) * 32;
            prev_chunks.extend(window_chunks(cx, prev_z_chunk, VIEW));
            next_chunks.extend(window_chunks(cx, z_chunk, VIEW));
            targets.push((cx, z_chunk));
        }

        bots = wait_all_windows(bots, targets, VIEW).await;

        world
            .sync_chunk_holds(Dimension::Overworld, prev_chunks, false)
            .await
            .unwrap();
        world
            .sync_chunk_holds(Dimension::Overworld, next_chunks, true)
            .await
            .unwrap();
        wave_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    drop(bots);

    let total_s = total_start.elapsed().as_secs_f64();
    let (sig_runs, sig_total_ms, _) = coordinator_signals_summary();
    let samples = world.tick_samples.lock().unwrap().clone();
    let (avg_mspt, _, p99_mspt, _) = if samples.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        stats(samples)
    };

    server.abort();
    world.shutdown().await.unwrap();

    let (wave_mean_ms, wave_p50_ms, _, wave_max_ms) = stats(wave_ms);
    let total_adds = num_bots * waves * 289;
    let total_removes = num_bots * waves * 289;
    let total_changes = (total_adds + total_removes) as f64;

    StreamResult {
        batched,
        bots: num_bots,
        waves,
        total_s,
        wave_mean_ms,
        wave_p50_ms,
        wave_max_ms,
        chunks_per_sec: total_changes / total_s,
        avg_mspt,
        p99_mspt,
        sig_runs,
        sig_total_ms,
    }
}

async fn wait_all_windows(bots: Vec<Bot>, targets: Vec<(i32, i32)>, view: i32) -> Vec<Bot> {
    let n = bots.len();
    let mut set = tokio::task::JoinSet::new();
    for (idx, (mut bot, (cx, cz))) in bots.into_iter().zip(targets).enumerate() {
        set.spawn(async move {
            bot.wait_window(cx, cz, view).await;
            (idx, bot)
        });
    }
    let mut out: Vec<Option<Bot>> = (0..n).map(|_| None).collect();
    while let Some(res) = set.join_next().await {
        let (idx, bot) = res.unwrap();
        out[idx] = Some(bot);
    }
    out.into_iter().map(Option::unwrap).collect()
}

fn run_forceload_suite(
    rt: &tokio::runtime::Runtime,
    label: &str,
    runs: usize,
    cycles: usize,
    tick_rate: Option<u32>,
) {
    println!("=== {label} ({cycles} cycles x 256 chunks add + remove all, region-settled) ===");
    println!(
        "| run | mode | total_s | add_mean_ms | add_p50_ms | add_p99_ms | rem_mean_ms | rem_p50_ms | rem_p99_ms | cmd_add_ms | cmd_rem_ms | holder_changes/s | sig_runs | sig_total_ms |"
    );
    println!("|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");

    let mut fl_batched = Vec::new();
    let mut fl_unbatched = Vec::new();
    for run in 1..=runs {
        for batched in [true, false] {
            let r = rt.block_on(bench_forceload(batched, cycles, tick_rate));
            println!(
                "| {run} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.0} | {} | {:.1} |",
                if r.batched { "batched" } else { "unbatched" },
                r.total_s,
                r.add_mean_ms,
                r.add_p50_ms,
                r.add_p99_ms,
                r.remove_mean_ms,
                r.remove_p50_ms,
                r.remove_p99_ms,
                r.cmd_add_mean_ms,
                r.cmd_remove_mean_ms,
                r.chunks_per_sec,
                r.sig_runs,
                r.sig_total_ms,
            );
            if batched {
                fl_batched.push(r);
            } else {
                fl_unbatched.push(r);
            }
        }
    }
    let fl_b_mean =
        fl_batched.iter().map(|r| r.chunks_per_sec).sum::<f64>() / fl_batched.len() as f64;
    let fl_u_mean =
        fl_unbatched.iter().map(|r| r.chunks_per_sec).sum::<f64>() / fl_unbatched.len() as f64;
    let fl_b_sig = fl_batched.iter().map(|r| r.sig_total_ms).sum::<f64>() / fl_batched.len() as f64;
    let fl_u_sig =
        fl_unbatched.iter().map(|r| r.sig_total_ms).sum::<f64>() / fl_unbatched.len() as f64;
    println!(
        "summary ({cycles} cycles x 512 changes = {} changes/run): batched={fl_b_mean:.0} changes/s (sig={fl_b_sig:.1} ms), unbatched={fl_u_mean:.0} changes/s (sig={fl_u_sig:.1} ms), throughput_ratio={:.2}x, sig_time_ratio={:.2}x",
        cycles * 512,
        fl_b_mean / fl_u_mean,
        fl_u_sig / fl_b_sig.max(0.001),
    );
    println!();
}

fn run_stream_suite(
    rt: &tokio::runtime::Runtime,
    label: &str,
    runs: usize,
    stream_bots: usize,
    stream_waves: usize,
    tick_rate: Option<u32>,
) {
    println!(
        "=== {label} ({stream_bots} spread bots, view=8 / 289 chunks each, {stream_waves} waves) ==="
    );
    println!(
        "| run | mode | total_s | wave_mean_ms | wave_p50_ms | wave_max_ms | holder_changes/s | avg_mspt | p99_mspt | sig_runs | sig_total_ms |"
    );
    println!("|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");

    let mut st_batched = Vec::new();
    let mut st_unbatched = Vec::new();
    for run in 1..=runs {
        for batched in [true, false] {
            let r = rt.block_on(bench_player_stream(
                batched,
                stream_bots,
                stream_waves,
                tick_rate,
            ));
            println!(
                "| {run} | {} | {:.3} | {:.1} | {:.1} | {:.1} | {:.0} | {:.3} | {:.3} | {} | {:.1} |",
                if r.batched { "batched" } else { "unbatched" },
                r.total_s,
                r.wave_mean_ms,
                r.wave_p50_ms,
                r.wave_max_ms,
                r.chunks_per_sec,
                r.avg_mspt,
                r.p99_mspt,
                r.sig_runs,
                r.sig_total_ms,
            );
            if batched {
                st_batched.push(r);
            } else {
                st_unbatched.push(r);
            }
        }
    }
    let st_b_mean =
        st_batched.iter().map(|r| r.chunks_per_sec).sum::<f64>() / st_batched.len() as f64;
    let st_u_mean =
        st_unbatched.iter().map(|r| r.chunks_per_sec).sum::<f64>() / st_unbatched.len() as f64;
    let st_b_sig = st_batched.iter().map(|r| r.sig_total_ms).sum::<f64>() / st_batched.len() as f64;
    let st_u_sig =
        st_unbatched.iter().map(|r| r.sig_total_ms).sum::<f64>() / st_unbatched.len() as f64;
    println!(
        "summary ({} bots, {} waves, {} changes/run): batched={st_b_mean:.0} changes/s (sig={st_b_sig:.1} ms), unbatched={st_u_mean:.0} changes/s (sig={st_u_sig:.1} ms), throughput_ratio={:.2}x, sig_time_ratio={:.2}x",
        st_batched[0].bots,
        st_batched[0].waves,
        st_batched[0].bots * st_batched[0].waves * 289 * 2,
        st_b_mean / st_u_mean,
        st_u_sig / st_b_sig.max(0.001),
    );
    println!();
}

fn main() {
    let runs = env_usize("HOLDERS_RUNS", 2);
    let cycles = env_usize("FORCELOAD_CYCLES", 40);
    let stream_bots = env_usize("STREAM_BOTS", 32);
    let stream_waves = env_usize("STREAM_WAVES", 4);
    let unpaced = std::env::var("HOLDERS_UNPACED").ok().as_deref() == Some("1");
    let stream = std::env::var("HOLDERS_STREAM").ok().as_deref() == Some("1");

    println!(
        "cmd: HOLDERS_RUNS={runs} FORCELOAD_CYCLES={cycles} STREAM_BOTS={stream_bots} STREAM_WAVES={stream_waves} stream={stream} unpaced={unpaced}"
    );
    if !stream {
        println!(
            "player-stream suites skipped: experimental and not part of the default measurement; set HOLDERS_STREAM=1 to opt in"
        );
    }
    if !unpaced {
        println!(
            "unpaced suites skipped: /tick rate 10000 can starve the coordinator event branch; set HOLDERS_UNPACED=1 to opt in"
        );
    }
    let rt = tokio::runtime::Runtime::new().unwrap();

    run_forceload_suite(&rt, "Workload 1a: /forceload at 20 TPS", runs, cycles, None);
    if unpaced {
        run_forceload_suite(
            &rt,
            "Workload 1b: /forceload unpaced (/tick rate 10000)",
            runs,
            cycles,
            Some(10000),
        );
    }
    if stream {
        run_stream_suite(
            &rt,
            "Workload 2a: Player chunk stream at 20 TPS",
            runs,
            stream_bots,
            stream_waves,
            None,
        );
        if unpaced {
            run_stream_suite(
                &rt,
                "Workload 2b: Player chunk stream unpaced (/tick rate 10000)",
                runs,
                stream_bots,
                stream_waves,
                Some(10000),
            );
        }
    }
}
