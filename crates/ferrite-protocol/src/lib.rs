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

/// Minecraft Java 26.3.
pub const PROTOCOL_VERSION: i32 = 777;
pub const GAME_VERSION: &str = "26.3";

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
}
