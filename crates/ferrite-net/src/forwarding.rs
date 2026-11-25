//! Player information forwarding from a Velocity or BungeeCord proxy.
//!
//! This mirrors Paper's implementation in
//! `paper-server/src/main/java/com/destroystokyo/paper/proxy/VelocityProxy.java`
//! (modern forwarding) and the Spigot legacy-forwarding patch in
//! `paper-server/patches/sources/net/minecraft/server/network/ServerHandshakePacketListenerImpl.java.patch`.
//! The login-side handling follows
//! `paper-server/patches/sources/net/minecraft/server/network/ServerLoginPacketListenerImpl.java.patch`.

use std::net::IpAddr;

use anyhow::{Context, Result, anyhow, bail, ensure};
use ferrite_protocol::{
    Decode, Encode, Packet, RestBytes, VarInt, codec::decode_string, packets::login::Property,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

/// Channel Velocity uses for the modern forwarding login query.
pub const VELOCITY_PLAYER_INFO_CHANNEL: &str = "velocity:player_info";

/// Highest forwarding version advertised to Velocity.
///
/// Matches `VelocityProxy.MAX_SUPPORTED_FORWARDING_VERSION` in Paper. Velocity picks
/// version 1 for pre-1.19.3 clients and version 4 (lazy session) for newer clients;
/// both start with the address/profile fields we consume.
pub const VELOCITY_MAX_FORWARDING_VERSION: i32 = 4;

/// Sanity cap for the property list in a forwarding payload.
const MAX_PROPERTIES: usize = 1024;

/// How player information reaches this server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Forwarding {
    /// Clients connect directly and authenticate with Mojang (or run offline).
    #[default]
    None,
    /// Velocity modern forwarding. `secret` is the shared `forwarding.secret` value.
    Velocity { secret: String },
    /// BungeeCord legacy forwarding in the handshake host field.
    Bungee,
}

impl Forwarding {
    /// Proxy forwarding supplies the authenticated profile, so Mojang session
    /// authentication must not run again for the forwarded connection.
    pub fn is_proxy(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Optional Mojang profile public key forwarded by Velocity versions 2 and 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VelocityChatKey {
    pub expiry_epoch_millis: i64,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub key_holder: Option<Uuid>,
}

/// Profile data forwarded by Velocity modern forwarding.
#[derive(Debug, Clone, PartialEq)]
pub struct VelocityProfile {
    pub address: IpAddr,
    pub uuid: Uuid,
    pub name: String,
    pub properties: Vec<Property>,
    pub chat_signing_key: Option<VelocityChatKey>,
}

/// Data parsed from a BungeeCord legacy handshake host.
///
/// The legacy format is `<host>\0<address>\0<undashed uuid>[\0<properties json>]`.
/// It does not carry the player name; that still comes from `LoginStart`.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyForwarding {
    pub host: String,
    pub address: IpAddr,
    pub uuid: Uuid,
    pub properties: Vec<Property>,
}

/// Clientbound login `minecraft:custom_query` (packet id 4 in the 26.3
/// `reports/packets.json`) used to request Velocity's forwarding payload.
///
/// The protocol crate only models the serverbound answer; the query is local to
/// the login flow so it lives here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CustomQuery {
    pub message_id: VarInt,
    pub channel: String,
    pub data: RestBytes,
}

impl Encode for CustomQuery {
    fn encode(&self, out: &mut Vec<u8>) {
        self.message_id.encode(out);
        self.channel.encode(out);
        self.data.encode(out);
    }
}

impl Decode for CustomQuery {
    fn decode(buf: &mut &[u8]) -> Result<Self, ferrite_protocol::DecodeError> {
        Ok(Self {
            message_id: VarInt::decode(buf)?,
            channel: String::decode(buf)?,
            data: RestBytes::decode(buf)?,
        })
    }
}

impl Packet for CustomQuery {
    const ID: i32 = 4;
    const STATE: ferrite_protocol::State = ferrite_protocol::State::Login;
    const BOUND: ferrite_protocol::Bound = ferrite_protocol::Bound::Clientbound;
}

/// Verify and parse a Velocity modern-forwarding response.
///
/// The payload starts with a 32-byte HMAC-SHA256 over every following byte, then
/// the forwarding version, address, UUID, name and property list. This is the same
/// layout read by `VelocityProxy.checkIntegrity`/`readAddress`/`createProfile`.
pub fn parse_velocity_payload(secret: &[u8], payload: &[u8]) -> Result<VelocityProfile> {
    let (signature, data) = payload
        .split_at_checked(32)
        .context("truncated Velocity forwarding signature")?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|_| anyhow!("invalid Velocity forwarding secret"))?;
    mac.update(data);
    mac.verify_slice(signature)
        .map_err(|_| anyhow!("invalid Velocity forwarding signature"))?;

    let mut buf = data;
    let version = VarInt::decode(&mut buf)
        .context("Velocity forwarding version")?
        .0;
    ensure!(
        (1..=VELOCITY_MAX_FORWARDING_VERSION).contains(&version),
        "unsupported Velocity forwarding version {version}"
    );
    let address = parse_address(&decode_string(&mut buf, 32767)?)?;
    let uuid = Uuid::decode(&mut buf).context("Velocity forwarding UUID")?;
    let name = decode_string(&mut buf, 16).context("Velocity forwarding name")?;
    ensure!(
        crate::valid_username(&name),
        "invalid Velocity forwarding name"
    );
    let count = VarInt::decode(&mut buf)
        .context("Velocity forwarding property count")?
        .0;
    let count = usize::try_from(count).context("negative property count")?;
    ensure!(count <= MAX_PROPERTIES, "too many forwarded properties");
    let mut properties = Vec::with_capacity(count.min(buf.len()));
    for _ in 0..count {
        properties.push(Property {
            name: decode_string(&mut buf, 32767)?,
            value: decode_string(&mut buf, 32767)?,
            signature: Option::<String>::decode(&mut buf)?,
        });
    }
    let chat_signing_key = if (2..=3).contains(&version) && !buf.is_empty() {
        parse_velocity_chat_key(&mut buf, version)
    } else {
        None
    };
    Ok(VelocityProfile {
        address,
        uuid,
        name,
        properties,
        chat_signing_key,
    })
}

fn parse_velocity_chat_key(buf: &mut &[u8], version: i32) -> Option<VelocityChatKey> {
    let expiry_epoch_millis = i64::decode(buf).ok()?;
    let key_len = usize::try_from(VarInt::decode(buf).ok()?.0).ok()?;
    if key_len > 4096 || buf.len() < key_len {
        return None;
    }
    let public_key = buf[..key_len].to_vec();
    *buf = &buf[key_len..];
    let sig_len = usize::try_from(VarInt::decode(buf).ok()?.0).ok()?;
    if sig_len > 4096 || buf.len() < sig_len {
        return None;
    }
    let signature = buf[..sig_len].to_vec();
    *buf = &buf[sig_len..];
    let key_holder = if version == 3 && !buf.is_empty() {
        Option::<Uuid>::decode(buf).ok().flatten()
    } else {
        None
    };
    Some(VelocityChatKey {
        expiry_epoch_millis,
        public_key,
        signature,
        key_holder,
    })
}

/// Split and parse a BungeeCord legacy handshake host.
///
/// Returns `None` when the host has no NUL-separated forwarding data, so callers
/// can distinguish "proxy mode configured but plain client connected" from a
/// malformed payload.
pub fn parse_legacy_host(host: &str) -> Result<Option<LegacyForwarding>> {
    let mut parts = host.split('\0');
    let server_host = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        return Ok(None);
    }
    ensure!(
        rest.len() == 2 || rest.len() == 3,
        "malformed BungeeCord forwarding data"
    );
    let address = parse_address(rest[0])?;
    let uuid = Uuid::parse_str(rest[1]).context("invalid BungeeCord forwarding UUID")?;
    let properties = match rest.get(2) {
        Some(json) if !json.is_empty() => parse_legacy_properties(json)?,
        _ => Vec::new(),
    };
    Ok(Some(LegacyForwarding {
        host: server_host.to_owned(),
        address,
        uuid,
        properties,
    }))
}

fn parse_address(address: &str) -> Result<IpAddr> {
    // Velocity sends the unbracketed host string; tolerate brackets from other
    // proxies the way Paper's HOST_PATTERN does.
    let address = address
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(address);
    address
        .parse()
        .with_context(|| format!("invalid forwarded address: {address}"))
}

fn parse_legacy_properties(json: &str) -> Result<Vec<Property>> {
    let value: serde_json::Value =
        serde_json::from_str(json).context("invalid BungeeCord forwarding properties")?;
    let array = value
        .as_array()
        .context("BungeeCord forwarding properties must be an array")?;
    ensure!(
        array.len() <= MAX_PROPERTIES,
        "too many BungeeCord forwarding properties"
    );
    array
        .iter()
        .map(|property| {
            let name = property
                .get("name")
                .and_then(serde_json::Value::as_str)
                .context("missing forwarding property name")?;
            ensure!(
                !name.is_empty()
                    && name.len() <= 16
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "invalid forwarding property name"
            );
            let value = property
                .get("value")
                .and_then(serde_json::Value::as_str)
                .context("missing forwarding property value")?;
            let signature = match property.get("signature") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(signature)) => Some(signature.clone()),
                Some(_) => bail!("invalid forwarding property signature"),
            };
            Ok(Property {
                name: name.to_owned(),
                value: value.to_owned(),
                signature,
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn signed_velocity_payload(
    secret: &[u8],
    version: i32,
    address: &str,
    uuid: Uuid,
    name: &str,
    properties: &[Property],
) -> Vec<u8> {
    let mut data = Vec::new();
    VarInt(version).encode(&mut data);
    address.encode(&mut data);
    uuid.encode(&mut data);
    name.encode(&mut data);
    VarInt(properties.len() as i32).encode(&mut data);
    for property in properties {
        property.encode(&mut data);
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(&data);
    let mut payload = mac.finalize().into_bytes().to_vec();
    payload.extend_from_slice(&data);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"hunter2";
    const UUID: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";

    #[test]
    fn velocity_payload_with_valid_hmac_is_accepted() {
        let uuid = Uuid::parse_str(UUID).unwrap();
        let properties = vec![Property {
            name: "textures".into(),
            value: "dGV4dHVyZQ==".into(),
            signature: Some("c2lnbmF0dXJl".into()),
        }];
        let payload = signed_velocity_payload(SECRET, 4, "203.0.113.7", uuid, "Notch", &properties);

        let parsed = parse_velocity_payload(SECRET, &payload).unwrap();
        assert_eq!(parsed.address, "203.0.113.7".parse::<IpAddr>().unwrap());
        assert_eq!(parsed.uuid, uuid);
        assert_eq!(parsed.name, "Notch");
        assert_eq!(parsed.properties, properties);
    }

    #[test]
    fn velocity_payload_with_wrong_secret_is_rejected() {
        let payload = signed_velocity_payload(
            SECRET,
            4,
            "203.0.113.7",
            Uuid::parse_str(UUID).unwrap(),
            "Notch",
            &[],
        );
        let error = parse_velocity_payload(b"wrong-secret", &payload).unwrap_err();
        assert!(error.to_string().contains("signature"));
    }

    #[test]
    fn velocity_payload_tampering_is_rejected() {
        let mut payload = signed_velocity_payload(
            SECRET,
            4,
            "203.0.113.7",
            Uuid::parse_str(UUID).unwrap(),
            "Notch",
            &[],
        );
        // Flip a byte in the signed body (the address string is right after the
        // version byte and 32-byte signature).
        payload[33] ^= 1;
        assert!(parse_velocity_payload(SECRET, &payload).is_err());
    }

    #[test]
    fn velocity_version_one_payload_is_accepted() {
        let uuid = Uuid::parse_str(UUID).unwrap();
        let payload = signed_velocity_payload(SECRET, 1, "2001:db8::1", uuid, "Notch", &[]);
        let parsed = parse_velocity_payload(SECRET, &payload).unwrap();
        assert_eq!(parsed.address, "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(parsed.uuid, uuid);
    }

    #[test]
    fn legacy_host_split_reads_address_uuid_and_properties() {
        let host = [
            "play.example.com",
            "203.0.113.7",
            "069a79f444e94726a5befca90e38aaf5",
            r#"[{"name":"textures","value":"dGV4dHVyZQ==","signature":"c2lnbmF0dXJl"}]"#,
        ]
        .join("\0");

        let forwarded = parse_legacy_host(&host).unwrap().unwrap();
        assert_eq!(forwarded.host, "play.example.com");
        assert_eq!(forwarded.address, "203.0.113.7".parse::<IpAddr>().unwrap());
        assert_eq!(forwarded.uuid, Uuid::parse_str(UUID).unwrap());
        assert_eq!(
            forwarded.properties,
            vec![Property {
                name: "textures".into(),
                value: "dGV4dHVyZQ==".into(),
                signature: Some("c2lnbmF0dXJl".into()),
            }]
        );
    }

    #[test]
    fn legacy_host_without_forwarding_data_is_none() {
        assert!(parse_legacy_host("play.example.com").unwrap().is_none());
    }

    #[test]
    fn legacy_host_without_properties_is_accepted() {
        let host = [
            "play.example.com",
            "203.0.113.7",
            "069a79f444e94726a5befca90e38aaf5",
        ]
        .join("\0");
        let forwarded = parse_legacy_host(&host).unwrap().unwrap();
        assert!(forwarded.properties.is_empty());
    }

    #[test]
    fn legacy_host_with_invalid_address_is_rejected() {
        let host = [
            "play.example.com",
            "not-an-ip",
            "069a79f444e94726a5befca90e38aaf5",
        ]
        .join("\0");
        assert!(parse_legacy_host(&host).is_err());
    }

    #[test]
    fn velocity_v3_parses_chat_signing_key() {
        let secret = b"my-secret";
        let uuid = Uuid::from_u128(0x0102030405060708090a0b0c0d0e0f10);
        let holder = Uuid::from_u128(0x1112131415161718191a1b1c1d1e1f20);
        let mut data = Vec::new();
        VarInt(3).encode(&mut data);
        "10.0.0.9".to_string().encode(&mut data);
        uuid.encode(&mut data);
        "Signer".to_string().encode(&mut data);
        VarInt(0).encode(&mut data);
        1_700_000_000_000i64.encode(&mut data);
        VarInt(4).encode(&mut data);
        data.extend_from_slice(&[1, 2, 3, 4]);
        VarInt(3).encode(&mut data);
        data.extend_from_slice(&[9, 8, 7]);
        Some(holder).encode(&mut data);

        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(&data);
        let sig = mac.finalize().into_bytes();
        let mut payload = Vec::with_capacity(32 + data.len());
        payload.extend_from_slice(&sig);
        payload.extend_from_slice(&data);

        let parsed = parse_velocity_payload(secret, &payload).unwrap();
        let key = parsed
            .chat_signing_key
            .expect("expected parsed v3 chat key");
        assert_eq!(key.expiry_epoch_millis, 1_700_000_000_000);
        assert_eq!(key.public_key, vec![1, 2, 3, 4]);
        assert_eq!(key.signature, vec![9, 8, 7]);
        assert_eq!(key.key_holder, Some(holder));
    }
}
