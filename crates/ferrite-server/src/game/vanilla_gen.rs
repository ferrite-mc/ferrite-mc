//! Chunks from Mojang's own generator, running in a separate host JVM.
//!
//! Ferrite contains no world generation. [`VanillaGenerator`] starts the host (see
//! `docs/worldgen-host.md`) on the user's own Minecraft server jar, sends it chunk requests over
//! a loopback socket and reads back the Anvil NBT the jar's serializer produced.
use super::{ChunkGenerator, Dimension, Generated, anvil::column_from_tag};
use anyhow::Context;
use ferrite_nbt::binary;
use ferrite_world::RuntimeRegistry;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc::{Sender, channel},
    },
    time::{Duration, Instant},
};

const OP_GENERATE: u8 = 1;
const OP_SPAWN_HEIGHT: u8 = 2;
const OP_SHUTDOWN: u8 = 3;
const OP_BIOME_AT: u8 = 4;
const OP_LOCATE_BIOME: u8 = 5;
const OP_LOCATE_STRUCTURE: u8 = 6;
const OP_STRUCTURE_AT: u8 = 7;
const OP_PLACE: u8 = 8;
const OP_LOCATE_POI: u8 = 9;
const START_TIMEOUT: Duration = Duration::from_secs(180);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

type Reply = Result<Vec<u8>, String>;
type Pending = Arc<Mutex<HashMap<u32, Sender<Reply>>>>;

/// Everything needed to start the host.
#[derive(Clone, Debug)]
pub struct HostSettings {
    /// The user's `server-26.3.jar`.
    pub server_jar: PathBuf,
    /// The jar's `libraries` directory.
    pub libraries: PathBuf,
    /// Our compiled host (`tools/build_worldgen_host.py`).
    pub host_jar: PathBuf,
    /// Rule file for the bytecode patcher. `None` runs the user's jar unmodified; with a file, a
    /// patched copy is built on this machine under `work_dir/patched` and used instead.
    pub patch_rules: Option<PathBuf>,
    /// The host's private working directory (world, properties, log).
    pub work_dir: PathBuf,
    /// Ferrite's `eula.txt`; its acceptance is mirrored into `work_dir`.
    pub eula: PathBuf,
    pub java_home: Option<PathBuf>,
    pub heap_mb: u32,
    pub threads: u32,
    pub jvm_args: Vec<String>,
    pub seed: String,
}

impl HostSettings {
    /// Default layout: the jar under `vanilla/26.3/lib`, the host under `target/worldgen-host`.
    pub fn standard(root: &std::path::Path) -> Self {
        Self {
            server_jar: root.join("vanilla/26.3/lib/server-26.3.jar"),
            libraries: root.join("vanilla/26.3/lib/libraries"),
            host_jar: root.join("target/worldgen-host/ferrite-worldgen-host.jar"),
            patch_rules: None,
            work_dir: root.join("worldgen"),
            eula: root.join("eula.txt"),
            java_home: None,
            heap_mb: 1024,
            threads: 4,
            jvm_args: Vec::new(),
            seed: "0".into(),
        }
    }
}

/// A running host and the connection to it.
pub struct VanillaGenerator {
    writer: Mutex<TcpStream>,
    pending: Pending,
    next_id: AtomicU32,
    registry: Arc<RuntimeRegistry>,
    child: Mutex<Child>,
}

fn jar_paths(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            jar_paths(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "jar") {
            out.push(path);
        }
    }
}

/// Builds (or reuses) the patched copy of the user's jar. It is rebuilt whenever the original
/// jar, the rule file or the host (which contains the patcher) is newer than the copy.
fn patched_jar(
    settings: &HostSettings,
    rules: &std::path::Path,
    java_bin: &std::path::Path,
) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        rules.exists(),
        "the patch rules are missing: {}",
        rules.display()
    );
    let out = settings.work_dir.join("patched").join("server.jar");
    let modified = |path: &std::path::Path| std::fs::metadata(path).and_then(|m| m.modified());
    let newest_input = [
        &settings.server_jar,
        &settings.host_jar,
        &rules.to_path_buf(),
    ]
    .iter()
    .filter_map(|path| modified(path).ok())
    .max();
    let fresh = match (modified(&out), newest_input) {
        (Ok(built), Some(input)) => built >= input,
        _ => false,
    };
    if fresh {
        return Ok(out);
    }
    let output = Command::new(java_bin)
        .arg("-cp")
        .arg(&settings.host_jar)
        .arg("dev.ferrite.worldgen.Patcher")
        .arg("--in")
        .arg(&settings.server_jar)
        .arg("--rules")
        .arg(rules)
        .arg("--out")
        .arg(&out)
        .output()
        .context("running the patcher")?;
    anyhow::ensure!(
        output.status.success(),
        "the patcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(out)
}

/// The host generates on many threads and does not need short pauses, so the throughput
/// collector is the default (measured about 8% faster than G1 at the same memory). A collector
/// the user names in `jvm-args` wins; two collectors on one command line make the JVM refuse to
/// start.
fn default_gc(jvm_args: &[String]) -> Option<&'static str> {
    let chosen = jvm_args
        .iter()
        .any(|arg| arg.starts_with("-XX:+Use") && arg.ends_with("GC"));
    (!chosen).then_some("-XX:+UseParallelGC")
}

impl VanillaGenerator {
    /// The host JVM's process id (for measuring it from outside).
    pub fn process_id(&self) -> u32 {
        self.child.lock().unwrap().id()
    }

    /// Starts the host and waits until the jar's generator is ready. Requires the user's
    /// `eula=true`.
    pub fn spawn(settings: &HostSettings, registry: Arc<RuntimeRegistry>) -> anyhow::Result<Self> {
        crate::eula::ensure_accepted(&settings.eula)?;
        for (what, path) in [
            ("server jar", &settings.server_jar),
            ("world generation host", &settings.host_jar),
        ] {
            anyhow::ensure!(
                path.exists(),
                "the {what} is missing: {} (python tools/fetch_vanilla.py, then python tools/build_worldgen_host.py)",
                path.display()
            );
        }
        std::fs::create_dir_all(&settings.work_dir).context("creating the worldgen directory")?;
        // The host runs the user's own server jar, which reads this file: one acceptance.
        std::fs::write(settings.work_dir.join("eula.txt"), "eula=true\n")?;

        let java = ferrite_jvm::find_java_home(settings.java_home.as_deref())
            .context("the world generation host needs Java 25 or newer")?;
        let java_bin =
            java.root()
                .join("bin")
                .join(if cfg!(windows) { "java.exe" } else { "java" });

        let server_jar = match &settings.patch_rules {
            Some(rules) => patched_jar(settings, rules, &java_bin)?,
            None => settings.server_jar.clone(),
        };
        let mut classpath = vec![settings.host_jar.clone(), server_jar];
        jar_paths(&settings.libraries, &mut classpath);
        let classpath = classpath
            .iter()
            .map(|path| std::path::absolute(path).unwrap_or_else(|_| path.clone()))
            .collect::<Vec<_>>();
        let classpath = std::env::join_paths(classpath)?;

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        // The jar still opens its own game port; give it a free loopback one so it can
        // neither clash with Ferrite nor be reached from outside.
        let game_port = {
            let probe = TcpListener::bind(("127.0.0.1", 0))?;
            probe.local_addr()?.port()
        };
        let log = std::fs::File::create(settings.work_dir.join("host.log"))?;
        let mut child = Command::new(java_bin)
            .current_dir(&settings.work_dir)
            .arg(format!("-Xmx{}m", settings.heap_mb))
            .args([
                "--add-opens",
                "java.base/java.lang=ALL-UNNAMED",
                "--enable-native-access=ALL-UNNAMED",
            ])
            .args(default_gc(&settings.jvm_args))
            .args(&settings.jvm_args)
            .arg("-cp")
            .arg(classpath)
            .arg("dev.ferrite.worldgen.Host")
            .args(["--port", &port.to_string()])
            .args(["--game-port", &game_port.to_string()])
            .args(["--seed", &settings.seed])
            .args(["--threads", &settings.threads.to_string()])
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .context("starting the world generation host")?;

        listener.set_nonblocking(true)?;
        let deadline = Instant::now() + START_TIMEOUT;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(status) = child.try_wait()? {
                        anyhow::bail!(
                            "the world generation host exited early ({status}); see {}",
                            settings.work_dir.join("host.log").display()
                        );
                    }
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        anyhow::bail!("the world generation host did not start in time");
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        // The host announces readiness with an empty response to id 0.
        let mut ready = [0u8; 9];
        stream.read_exact(&mut ready)?;

        let pending: Pending = Arc::default();
        let mut reader = stream.try_clone()?;
        let waiting = Arc::clone(&pending);
        std::thread::Builder::new()
            .name("ferrite-worldgen-client".into())
            .spawn(move || read_replies(&mut reader, &waiting))?;
        Ok(Self {
            writer: Mutex::new(stream),
            pending,
            next_id: AtomicU32::new(1),
            registry,
            child: Mutex::new(child),
        })
    }

    fn request(
        &self,
        op: u8,
        dimension: Dimension,
        x: i32,
        z: i32,
        argument: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let argument = argument.as_bytes();
        let length = u16::try_from(argument.len()).context("host request argument is too long")?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).max(1);
        let (reply, result) = channel();
        self.pending.lock().unwrap().insert(id, reply);
        let mut message = vec![0u8; 16 + argument.len()];
        message[0..4].copy_from_slice(&id.to_be_bytes());
        message[4] = op;
        message[5] = match dimension {
            Dimension::Overworld => 0,
            Dimension::Nether => 1,
            Dimension::End => 2,
        };
        message[6..10].copy_from_slice(&x.to_be_bytes());
        message[10..14].copy_from_slice(&z.to_be_bytes());
        message[14..16].copy_from_slice(&length.to_be_bytes());
        message[16..].copy_from_slice(argument);
        if let Err(error) = self.writer.lock().unwrap().write_all(&message) {
            self.pending.lock().unwrap().remove(&id);
            return Err(error).context("sending a request to the world generation host");
        }
        match result.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(message)) => anyhow::bail!("world generation host: {message}"),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                anyhow::bail!("the world generation host did not answer in time")
            }
        }
    }
}

fn read_replies(stream: &mut TcpStream, pending: &Pending) {
    loop {
        let mut header = [0u8; 9];
        if stream.read_exact(&mut header).is_err() {
            break;
        }
        let id = u32::from_be_bytes(header[0..4].try_into().unwrap());
        let failed = header[4] != 0;
        let length = u32::from_be_bytes(header[5..9].try_into().unwrap()) as usize;
        let mut payload = vec![0u8; length];
        if stream.read_exact(&mut payload).is_err() {
            break;
        }
        let reply = if failed {
            Err(String::from_utf8_lossy(&payload).into_owned())
        } else {
            Ok(payload)
        };
        if let Some(sender) = pending.lock().unwrap().remove(&id) {
            let _ = sender.send(reply);
        }
    }
    for (_, sender) in pending.lock().unwrap().drain() {
        let _ = sender.send(Err("the connection to the host closed".into()));
    }
}

impl ChunkGenerator for VanillaGenerator {
    fn generate(&self, dimension: Dimension, x: i32, z: i32) -> anyhow::Result<Generated> {
        let bytes = self.request(OP_GENERATE, dimension, x, z, "")?;
        let (_, tag) = binary::read_named(&bytes)?;
        let (chunk, light) = column_from_tag(&tag, dimension, &self.registry)?;
        Ok(Generated { chunk, light })
    }

    fn spawn_height(&self, dimension: Dimension, x: i32, z: i32) -> anyhow::Result<i32> {
        let bytes = self.request(OP_SPAWN_HEIGHT, dimension, x, z, "")?;
        let bytes: [u8; 4] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("malformed spawn height reply"))?;
        Ok(i32::from_be_bytes(bytes))
    }

    fn locate_biome(
        &self,
        dimension: Dimension,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        let argument = format!("{target};{radius};{x};64;{z}");
        let reply = self.request(OP_LOCATE_BIOME, dimension, 0, 0, &argument)?;
        parse_position(&reply)
    }

    fn biome_at(&self, dimension: Dimension, x: i32, y: i32, z: i32) -> anyhow::Result<String> {
        let reply = self.request(OP_BIOME_AT, dimension, 0, 0, &format!("{x} {y} {z}"))?;
        Ok(String::from_utf8(reply)?)
    }

    fn locate_structure(
        &self,
        dimension: Dimension,
        target: &str,
        x: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        let argument = format!("{target};{radius};{x};{z}");
        parse_position(&self.request(OP_LOCATE_STRUCTURE, dimension, 0, 0, &argument)?)
    }

    fn place(&self, dimension: Dimension, argument: &str) -> anyhow::Result<Vec<([i32; 3], u16)>> {
        let reply = self.request(OP_PLACE, dimension, 0, 0, argument)?;
        parse_placed(&reply)
    }

    fn locate_poi(
        &self,
        dimension: Dimension,
        target: &str,
        x: i32,
        y: i32,
        z: i32,
        radius: i32,
    ) -> anyhow::Result<Option<(i32, i32, i32)>> {
        let argument = format!("{target};{radius};{x};{y};{z}");
        parse_position(&self.request(OP_LOCATE_POI, dimension, 0, 0, &argument)?)
    }

    fn structure_at(
        &self,
        dimension: Dimension,
        id: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> anyhow::Result<bool> {
        let reply = self.request(
            OP_STRUCTURE_AT,
            dimension,
            0,
            0,
            &format!("{id} {x} {y} {z}"),
        )?;
        Ok(reply == b"1")
    }
}

/// Parses the host reply of a placement: `ok` then `x y z block[props]` lines, or `fail <text>`.
fn parse_placed(reply: &[u8]) -> anyhow::Result<Vec<([i32; 3], u16)>> {
    let text = std::str::from_utf8(reply)?;
    let mut lines = text.lines();
    match lines.next() {
        Some("ok") => {}
        Some(first) => anyhow::bail!("{}", first.strip_prefix("fail ").unwrap_or(first)),
        None => anyhow::bail!("the host returned nothing"),
    }
    let mut blocks = Vec::new();
    for line in lines {
        let mut fields = line.splitn(4, ' ');
        let (Some(x), Some(y), Some(z), Some(state)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(x), Ok(y), Ok(z)) = (x.parse(), y.parse(), z.parse()) else {
            continue;
        };
        let (name, properties) = match state.split_once('[') {
            Some((name, rest)) => (name, rest.trim_end_matches(']')),
            None => (state, ""),
        };
        let props: Vec<(&str, &str)> = properties
            .split(',')
            .filter(|pair| !pair.is_empty())
            .filter_map(|pair| pair.split_once('='))
            .collect();
        if let Some(id) = ferrite_data::blocks::state_id(name, &props) {
            blocks.push(([x, y, z], id));
        }
    }
    Ok(blocks)
}

/// Parses a host reply of `x y z[ text]`; an empty reply means nothing was found.
fn parse_position(reply: &[u8]) -> anyhow::Result<Option<(i32, i32, i32)>> {
    if reply.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(reply)?;
    let mut parts = text.split(' ');
    let mut next = || -> anyhow::Result<i32> {
        parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("malformed host position reply"))?
            .parse::<i32>()
            .map_err(Into::into)
    };
    Ok(Some((next()?, next()?, next()?)))
}

impl Drop for VanillaGenerator {
    fn drop(&mut self) {
        let mut message = [0u8; 16];
        message[4] = OP_SHUTDOWN;
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(&message);
        }
        if let Ok(mut child) = self.child.lock() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn placed_blocks_are_parsed_and_failures_are_reported() {
        let reply = b"ok\n10 64 -3 minecraft:stone\n11 64 -3 minecraft:oak_log[axis=y]\n12 64 -3 minecraft:not_a_block\n";
        let blocks = super::parse_placed(reply).unwrap();
        assert_eq!(blocks.len(), 2, "unknown blocks are skipped");
        assert_eq!(blocks[0].0, [10, 64, -3]);
        assert_eq!(
            Some(blocks[1].1),
            ferrite_data::blocks::state_id("minecraft:oak_log", &[("axis", "y")])
        );
        let error = super::parse_placed(b"fail Could not place\n").unwrap_err();
        assert_eq!(error.to_string(), "Could not place");
        assert!(super::parse_placed(b"").is_err());
    }

    use super::default_gc;

    #[test]
    fn parallel_gc_is_the_default_but_a_named_collector_wins() {
        assert_eq!(default_gc(&[]), Some("-XX:+UseParallelGC"));
        assert_eq!(
            default_gc(&["-Dsomething=1".into()]),
            Some("-XX:+UseParallelGC")
        );
        assert_eq!(default_gc(&["-XX:+UseG1GC".into()]), None);
        assert_eq!(default_gc(&["-XX:+UseZGC".into()]), None);
    }
}
