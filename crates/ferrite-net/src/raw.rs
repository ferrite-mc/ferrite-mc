use ferrite_protocol::{Decode, DecodeError, Encode, Packet, VarInt};

/// One packet as it is framed on the wire: its ID and its body (fields only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPacket {
    pub id: i32,
    pub body: Vec<u8>,
}

impl RawPacket {
    pub fn new(id: i32, body: Vec<u8>) -> Self {
        Self { id, body }
    }

    /// Split a decoded frame (`VarInt id | body`, as yielded by `FrameDecoder`).
    pub fn parse(frame: &[u8]) -> Result<Self, DecodeError> {
        let mut buf = frame;
        let id = VarInt::decode(&mut buf)?.0;
        Ok(Self {
            id,
            body: buf.to_vec(),
        })
    }

    /// Append `VarInt id | body` (the input of `FrameEncoder::encode`).
    pub fn write(&self, out: &mut Vec<u8>) {
        VarInt(self.id).encode(out);
        out.extend_from_slice(&self.body);
    }

    pub fn to_frame(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.body.len() + 5);
        self.write(&mut out);
        out
    }

    /// Encode a core (777) packet.
    pub fn of<P: Packet>(packet: &P) -> Self {
        let mut body = Vec::new();
        packet.encode(&mut body);
        Self { id: P::ID, body }
    }
}
