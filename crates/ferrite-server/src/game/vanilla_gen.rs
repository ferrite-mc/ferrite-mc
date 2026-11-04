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

    fn request(&self, op: u8, dimension: Dimension, x: i32, z: i32) -> anyhow::Result<Vec<u8>> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).max(1);
        let (reply, result) = channel();
        self.pending.lock().unwrap().insert(id, reply);
        let mut message = [0u8; 14];
        message[0..4].copy_from_slice(&id.to_be_bytes());
        message[4] = op;
        message[5] = match dimension {
            Dimension::Overworld => 0,
            Dimension::Nether => 1,
            Dimension::End => 2,
        };
        message[6..10].copy_from_slice(&x.to_be_bytes());
        message[10..14].copy_from_slice(&z.to_be_bytes());
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
        let bytes = self.request(OP_GENERATE, dimension, x, z)?;
        let (_, tag) = binary::read_named(&bytes)?;
        let (chunk, light) = column_from_tag(&tag, dimension, &self.registry)?;
        Ok(Generated { chunk, light })
    }

    fn spawn_height(&self, dimension: Dimension, x: i32, z: i32) -> anyhow::Result<i32> {
        let bytes = self.request(OP_SPAWN_HEIGHT, dimension, x, z)?;
        let bytes: [u8; 4] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("malformed spawn height reply"))?;
        Ok(i32::from_be_bytes(bytes))
    }

    fn locate_biome(
        &self,
        _target: &str,
        _x: i32,
        _z: i32,
        _radius: i32,
    ) -> anyhow::Result<Option<(i32, i32)>> {
        // Not offered by the host yet; reported as "not found".
        Ok(None)
    }
}

impl Drop for VanillaGenerator {
    fn drop(&mut self) {
        let mut message = [0u8; 14];
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
