//! Minecraft Java Edition wire protocol: primitive codecs, framing, and packet definitions.
//!
//! The core speaks exactly one protocol version ([`PROTOCOL_VERSION`]); other versions are
//! handled by a translation layer in front of it.

pub mod codec;
mod compress;
pub mod frame;
pub mod packets;
pub mod types;

pub use codec::{Decode, DecodeError, Encode, RestBytes, VarInt, VarLong};
pub use frame::{FrameDecoder, FrameEncoder};

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Minecraft Java 26.3.
pub const PROTOCOL_VERSION: i32 = 777;
pub const GAME_VERSION: &str = "26.3";

static VANILLA_GENERATED_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Registers the resolved `vanilla/<GAME_VERSION>/generated` directory for this process.
///
/// Subsequent calls to [`vanilla_generated_dir`] prefer this path whenever `FERRITE_VANILLA_ROOT`
/// is not explicitly set.
pub fn set_vanilla_generated_dir(path: PathBuf) {
    let _ = VANILLA_GENERATED_OVERRIDE.set(path);
}

fn is_valid_generated_dir(path: &Path) -> bool {
    path.join("reports/registries.json").is_file()
}

fn resolve_from_base(candidate: PathBuf) -> Option<PathBuf> {
    if is_valid_generated_dir(&candidate) {
        return Some(candidate);
    }
    let nested_gen = candidate.join("generated");
    if is_valid_generated_dir(&nested_gen) {
        return Some(nested_gen);
    }
    let nested_full = candidate
        .join("vanilla")
        .join(GAME_VERSION)
        .join("generated");
    if is_valid_generated_dir(&nested_full) {
        return Some(nested_full);
    }
    None
}

/// Pure resolver for `vanilla/<GAME_VERSION>/generated` across explicit env, runtime override,
/// CWD/executable ancestors, and compile-time fallback.
pub fn resolve_vanilla_generated_dir(
    env_override: Option<&Path>,
    runtime_override: Option<&Path>,
    cwd: Option<&Path>,
    exe_dir: Option<&Path>,
    manifest_dir: Option<&Path>,
) -> PathBuf {
    let rel = Path::new("vanilla").join(GAME_VERSION).join("generated");
    if let Some(env_path) = env_override {
        if let Some(found) = resolve_from_base(env_path.to_path_buf()) {
            return found;
        }
        return env_path.to_path_buf();
    }
    if let Some(rt) = runtime_override
        && is_valid_generated_dir(rt)
    {
        return rt.to_path_buf();
    }
    for anchor in [cwd, exe_dir].into_iter().flatten() {
        for ancestor in anchor.ancestors() {
            let candidate = ancestor.join(&rel);
            if is_valid_generated_dir(&candidate) {
                return candidate;
            }
        }
    }
    if let Some(manifest) = manifest_dir {
        let candidate = manifest.join("../..").join(&rel);
        if is_valid_generated_dir(&candidate) || candidate.is_dir() {
            return candidate;
        }
    }
    rel
}

/// Locates `vanilla/26.3/generated` for both portable release binaries and workspace builds.
pub fn vanilla_generated_dir(manifest_dir: Option<&Path>) -> PathBuf {
    let env_override = std::env::var_os("FERRITE_VANILLA_ROOT").map(PathBuf::from);
    let cwd = std::env::current_dir().ok();
    let exe_path = std::env::current_exe().ok();
    let exe_dir = exe_path.as_deref().and_then(Path::parent);
    resolve_vanilla_generated_dir(
        env_override.as_deref(),
        VANILLA_GENERATED_OVERRIDE.get().map(PathBuf::as_path),
        cwd.as_deref(),
        exe_dir,
        manifest_dir,
    )
}

/// A named Minecraft Java protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolVersion {
    pub protocol: i32,
    pub name: &'static str,
}

/// Protocol versions backed by authoritative local packet reports (`vanilla/26.3/generated`).
/// Older versions are refused explicitly at login until their packet reports are present.
pub const SUPPORTED_PROTOCOLS: &[ProtocolVersion] = &[ProtocolVersion {
    protocol: PROTOCOL_VERSION,
    name: GAME_VERSION,
}];

/// Result of negotiating a client's handshake protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiationOutcome {
    Supported(ProtocolVersion),
    OutdatedClient {
        client_protocol: i32,
        expected: ProtocolVersion,
    },
    OutdatedServer {
        client_protocol: i32,
        expected: ProtocolVersion,
    },
}

/// Negotiate a client handshake `protocol_version` against [`SUPPORTED_PROTOCOLS`].
/// Never guesses or silently accepts mismatched versions.
pub fn negotiate_protocol(client_protocol: i32) -> NegotiationOutcome {
    let expected = SUPPORTED_PROTOCOLS[0];
    if let Some(&matched) = SUPPORTED_PROTOCOLS
        .iter()
        .find(|v| v.protocol == client_protocol)
    {
        NegotiationOutcome::Supported(matched)
    } else if client_protocol < expected.protocol {
        NegotiationOutcome::OutdatedClient {
            client_protocol,
            expected,
        }
    } else {
        NegotiationOutcome::OutdatedServer {
            client_protocol,
            expected,
        }
    }
}

/// Connection state, which selects the packet ID namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Handshaking,
    Status,
    Login,
    Configuration,
    Play,
}

/// Direction a packet travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bound {
    Serverbound,
    Clientbound,
}

/// A packet with a fixed ID in a fixed state and direction.
pub trait Packet: Encode + Decode {
    const ID: i32;
    const STATE: State;
    const BOUND: Bound;
}

/// Encode a packet body (ID + fields) into `out`, without the length prefix.
pub fn encode_packet<P: Packet>(packet: &P, out: &mut Vec<u8>) {
    VarInt(P::ID).encode(out);
    packet.encode(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_negotiation_accepts_26_3_and_refuses_mismatches() {
        assert_eq!(
            negotiate_protocol(PROTOCOL_VERSION),
            NegotiationOutcome::Supported(ProtocolVersion {
                protocol: 777,
                name: "26.3"
            })
        );
        assert_eq!(
            negotiate_protocol(776),
            NegotiationOutcome::OutdatedClient {
                client_protocol: 776,
                expected: SUPPORTED_PROTOCOLS[0],
            }
        );
        assert_eq!(
            negotiate_protocol(778),
            NegotiationOutcome::OutdatedServer {
                client_protocol: 778,
                expected: SUPPORTED_PROTOCOLS[0],
            }
        );
    }

    #[test]
    fn resolves_vanilla_dir_from_exe_and_env_before_stale_manifest() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ferrite_proto_vanilla_{nanos}"));
        let exe_dir = root.join("portable");
        let generated = exe_dir.join("vanilla").join(GAME_VERSION).join("generated");
        std::fs::create_dir_all(generated.join("reports")).unwrap();
        std::fs::write(generated.join("reports/registries.json"), "{}").unwrap();

        let stale_manifest = Path::new("/nonexistent/ci/runner/crates/ferrite-server");
        let resolved = resolve_vanilla_generated_dir(
            None,
            None,
            Some(root.join("other").as_path()),
            Some(&exe_dir),
            Some(stale_manifest),
        );
        assert_eq!(resolved, generated);

        let env_base = root.join("explicit_base");
        let env_gen = env_base.join("generated");
        std::fs::create_dir_all(env_gen.join("reports")).unwrap();
        std::fs::write(env_gen.join("reports/registries.json"), "{}").unwrap();
        let resolved_env = resolve_vanilla_generated_dir(
            Some(&env_base),
            Some(&generated),
            None,
            None,
            Some(stale_manifest),
        );
        assert_eq!(resolved_env, env_gen);

        let _ = std::fs::remove_dir_all(root);
    }
}
