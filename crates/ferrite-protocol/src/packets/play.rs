use super::login::Property;
use crate::{
    Bound, Decode, DecodeError, Encode, Packet, State, VarInt, VarLong,
    types::{LpVec3, PlainText},
};
use uuid::Uuid;
macro_rules! fields { ($name:ident {$($f:ident:$t:ty),* $(,)?})=>{ #[derive(Debug,Clone,PartialEq)] pub struct $name {$(pub $f:$t),*} impl Encode for $name {fn encode(&self,out:&mut Vec<u8>){$(self.$f.encode(out);)*}} impl Decode for $name {fn decode(buf:&mut &[u8])->Result<Self,DecodeError>{Ok(Self{$($f:<$t>::decode(buf)?),*})}} }; }
macro_rules! packet {($name:ident,$id:expr,$bound:ident,{$($f:ident:$t:ty),* $(,)?})=>{ fields!($name {$($f:$t),*}); impl Packet for $name {const ID:i32=$id;const STATE:State=State::Play;const BOUND:Bound=Bound::$bound;} };}
fields!(DeathLocation {
    dimension: String,
    position: i64
});
packet!(PlayLogin,50,Clientbound,{entity_id:i32,hardcore:bool,dimensions:Vec<String>,max_players:VarInt,view_distance:VarInt,simulation_distance:VarInt,reduced_debug:bool,respawn_screen:bool,limited_crafting:bool,dimension_type:VarInt,dimension:String,seed:i64,game_mode:VarInt,previous_game_mode:VarInt,debug:bool,flat:bool,death_location:Option<DeathLocation>,portal_cooldown:VarInt,sea_level:VarInt,online_mode:bool,secure_chat:bool});
packet!(PositionSync,73,Clientbound,{teleport_id:VarInt,x:f64,y:f64,z:f64,velocity_x:f64,velocity_y:f64,velocity_z:f64,yaw:f32,pitch:f32,flags:i32});
packet!(GameEvent,39,Clientbound,{event:u8,value:f32});
packet!(CustomChatCompletions,23,Clientbound,{action:VarInt,entries:Vec<String>});
packet!(BlockDestruction,5,Clientbound,{entity_id:VarInt,position:i64,progress:i8});
packet!(ChunkBatchFinished,11,Clientbound,{size:VarInt});
packet!(CenterChunk,96,Clientbound,{x:VarInt,z:VarInt});
packet!(DefaultSpawn,99,Clientbound,{dimension:String,position:i64,yaw:f32,pitch:f32});
packet!(PlayKeepAlive,45,Clientbound,{id:i64});
packet!(ClientKeepAlive,28,Serverbound,{id:i64});
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkBatchStart;
impl Encode for ChunkBatchStart {
    fn encode(&self, _: &mut Vec<u8>) {}
}
impl Decode for ChunkBatchStart {
    fn decode(_: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self)
    }
}
impl Packet for ChunkBatchStart {
    const ID: i32 = 12;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
fields!(Heightmap {kind:VarInt,data:Vec<i64>});
fields!(AttributeModifier {
    id: String,
    amount: f64,
    operation: VarInt
});
fields!(AttributeSnapshot {attribute:VarInt,base:f64,modifiers:Vec<AttributeModifier>});
// Vanilla `ClientboundUpdateAttributesPacket`: the attribute values the client computes with
// (the health bar's size comes from `max_health`).
packet!(UpdateAttributes,134,Clientbound,{entity_id:VarInt,attributes:Vec<AttributeSnapshot>});
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkData {
    pub x: i32,
    pub z: i32,
    pub heightmaps: Vec<Heightmap>,
    pub data: Vec<u8>,
    pub sky_mask: Vec<i64>,
    pub block_mask: Vec<i64>,
    pub empty_sky_mask: Vec<i64>,
    pub empty_block_mask: Vec<i64>,
    pub sky_arrays: Vec<Vec<u8>>,
    pub block_arrays: Vec<Vec<u8>>,
}
/// Light masks are `java.util.BitSet`s. In 26.3 the client reads them as a VarInt-prefixed *byte* array
/// (`BitSet.toByteArray()`: little-endian, no trailing zero bytes), not as the long array older versions
/// and the wiki describe. Verified against the real client codec with `tools/wirecheck`.
fn encode_bitset(longs: &[i64], out: &mut Vec<u8>) {
    let mut bytes: Vec<u8> = longs.iter().flat_map(|l| l.to_le_bytes()).collect();
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    VarInt(bytes.len() as i32).encode(out);
    out.extend_from_slice(&bytes);
}
fn decode_bitset(buf: &mut &[u8]) -> Result<Vec<i64>, DecodeError> {
    let mut bytes = Vec::<u8>::decode(buf)?;
    bytes.resize(bytes.len().div_ceil(8) * 8, 0);
    Ok(bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| i64::from_le_bytes(*c))
        .collect())
}
impl Encode for ChunkData {
    fn encode(&self, out: &mut Vec<u8>) {
        self.x.encode(out);
        self.z.encode(out);
        self.heightmaps.encode(out);
        self.data.encode(out);
        VarInt(0).encode(out);
        encode_bitset(&self.sky_mask, out);
        encode_bitset(&self.block_mask, out);
        encode_bitset(&self.empty_sky_mask, out);
        encode_bitset(&self.empty_block_mask, out);
        self.sky_arrays.encode(out);
        self.block_arrays.encode(out);
    }
}
impl Decode for ChunkData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let x = i32::decode(buf)?;
        let z = i32::decode(buf)?;
        let heightmaps = Vec::<Heightmap>::decode(buf)?;
        let data = Vec::<u8>::decode(buf)?;
        if VarInt::decode(buf)?.0 != 0 {
            return Err(DecodeError::Invalid("block entities unsupported"));
        }
        Ok(Self {
            x,
            z,
            heightmaps,
            data,
            sky_mask: decode_bitset(buf)?,
            block_mask: decode_bitset(buf)?,
            empty_sky_mask: decode_bitset(buf)?,
            empty_block_mask: decode_bitset(buf)?,
            sky_arrays: Vec::decode(buf)?,
            block_arrays: Vec::decode(buf)?,
        })
    }
}
impl Packet for ChunkData {
    const ID: i32 = 46;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
/// `ADD_PLAYER | INITIALIZE_CHAT | UPDATE_GAME_MODE | UPDATE_LISTED`: the actions every player
/// needs for the tab list and for validating chat signatures.
const PLAYER_INFO_ACTIONS: u8 = 1 | 2 | 4 | 8;
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInfo {
    pub uuid: Uuid,
    pub name: String,
    pub properties: Vec<Property>,
    pub chat_session: Option<RemoteChatSessionData>,
    pub game_mode: VarInt,
}
impl Encode for PlayerInfo {
    fn encode(&self, out: &mut Vec<u8>) {
        PLAYER_INFO_ACTIONS.encode(out);
        VarInt(1).encode(out);
        self.uuid.encode(out);
        self.name.encode(out);
        self.properties.encode(out);
        self.chat_session.encode(out);
        self.game_mode.encode(out);
        true.encode(out);
    }
}
impl Decode for PlayerInfo {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if u8::decode(buf)? != PLAYER_INFO_ACTIONS || VarInt::decode(buf)?.0 != 1 {
            return Err(DecodeError::Invalid("player info actions"));
        }
        let uuid = Uuid::decode(buf)?;
        let name = String::decode(buf)?;
        let properties = Vec::decode(buf)?;
        let chat_session = Option::decode(buf)?;
        let game_mode = VarInt::decode(buf)?;
        if !bool::decode(buf)? {
            return Err(DecodeError::Invalid("unlisted player"));
        }
        Ok(Self {
            uuid,
            name,
            properties,
            chat_session,
            game_mode,
        })
    }
}
impl Packet for PlayerInfo {
    const ID: i32 = 71;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
packet!(ConfirmTeleport,0,Serverbound,{id:VarInt,x:f64,y:f64,z:f64,yaw:f32,pitch:f32});
packet!(MovePosition,30,Serverbound,{x:f64,y:f64,z:f64,flags:u8});
packet!(MovePositionRotation,31,Serverbound,{x:f64,y:f64,z:f64,yaw:f32,pitch:f32,flags:u8});
// Vanilla `ServerboundMoveVehiclePacket`: where the client moved the vehicle it steers.
packet!(ServerboundMoveVehicle,34,Serverbound,{x:f64,y:f64,z:f64,yaw:f32,pitch:f32,on_ground:bool});
packet!(MoveRotation,32,Serverbound,{yaw:f32,pitch:f32,flags:u8});
packet!(MoveFlags,33,Serverbound,{flags:u8});
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLoaded;
impl Encode for PlayerLoaded {
    fn encode(&self, _: &mut Vec<u8>) {}
}
impl Decode for PlayerLoaded {
    fn decode(_: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self)
    }
}
impl Packet for PlayerLoaded {
    const ID: i32 = 44;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}
packet!(PlayDisconnect,32,Clientbound,{reason:crate::types::PlainText});
packet!(ForgetChunk,38,Clientbound,{z:i32,x:i32});
packet!(EntityMove,54,Clientbound,{id:VarInt,properties:VarInt,dx:i16,dy:i16,dz:i16});
packet!(EntityMoveRotation,55,Clientbound,{id:VarInt,properties:VarInt,dx:i16,dy:i16,dz:i16,yaw:u8,pitch:u8});
packet!(EntityRotation,57,Clientbound,{id:VarInt,on_ground:bool,yaw:u8,pitch:u8});
packet!(EntityHeadRotation,85,Clientbound,{id:VarInt,yaw:u8});
packet!(EntityTeleport,35,Clientbound,{id:VarInt,path_type:VarInt,x:f64,y:f64,z:f64,yaw:f32,pitch:f32,on_ground:bool});
packet!(RemoveEntities,78,Clientbound,{ids:Vec<VarInt>});
packet!(SetPassengers,109,Clientbound,{vehicle:VarInt,passengers:Vec<VarInt>});
packet!(PlayerInfoRemove,70,Clientbound,{uuids:Vec<Uuid>});
packet!(SystemChat,124,Clientbound,{content:crate::types::PlainText,overlay:bool});
packet!(ChunkBatchReceived,11,Serverbound,{chunks_per_tick:f32});
#[derive(Debug, Clone, PartialEq)]
pub struct AddEntity {
    pub id: VarInt,
    pub uuid: Uuid,
    pub entity_type: VarInt,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub velocity: LpVec3,
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
    pub data: VarInt,
}
impl Encode for AddEntity {
    fn encode(&self, out: &mut Vec<u8>) {
        self.id.encode(out);
        self.uuid.encode(out);
        self.entity_type.encode(out);
        self.x.encode(out);
        self.y.encode(out);
        self.z.encode(out);
        self.velocity.encode(out);
        self.pitch.encode(out);
        self.yaw.encode(out);
        self.head_yaw.encode(out);
        self.data.encode(out);
    }
}
impl Decode for AddEntity {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let id = VarInt::decode(buf)?;
        let uuid = Uuid::decode(buf)?;
        let entity_type = VarInt::decode(buf)?;
        let x = f64::decode(buf)?;
        let y = f64::decode(buf)?;
        let z = f64::decode(buf)?;
        Ok(Self {
            id,
            uuid,
            entity_type,
            x,
            y,
            z,
            velocity: LpVec3::decode(buf)?,
            pitch: u8::decode(buf)?,
            yaw: u8::decode(buf)?,
            head_yaw: u8::decode(buf)?,
            data: VarInt::decode(buf)?,
        })
    }
}
impl Packet for AddEntity {
    const ID: i32 = 1;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// What the locator bar should draw for one waypoint.
#[derive(Debug, Clone, PartialEq)]
pub enum WaypointData {
    Empty,
    Position([i32; 3]),
    Chunk([i32; 2]),
    Azimuth(f32),
}

/// `waypoint` (locator bar). `operation`: 0 track, 1 untrack, 2 update.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackedWaypoint {
    pub operation: VarInt,
    pub uuid: Uuid,
    pub style: String,
    pub color: Option<u32>,
    pub data: WaypointData,
}
impl Encode for TrackedWaypoint {
    fn encode(&self, out: &mut Vec<u8>) {
        self.operation.encode(out);
        true.encode(out);
        self.uuid.encode(out);
        self.style.encode(out);
        match self.color {
            Some(rgb) => {
                true.encode(out);
                out.extend_from_slice(&rgb.to_be_bytes()[1..]);
            }
            None => false.encode(out),
        }
        match &self.data {
            WaypointData::Empty => VarInt(0).encode(out),
            WaypointData::Position(p) => {
                VarInt(1).encode(out);
                for v in p {
                    VarInt(*v).encode(out);
                }
            }
            WaypointData::Chunk(c) => {
                VarInt(2).encode(out);
                for v in c {
                    VarInt(*v).encode(out);
                }
            }
            WaypointData::Azimuth(angle) => {
                VarInt(3).encode(out);
                angle.encode(out);
            }
        }
    }
}
impl Decode for TrackedWaypoint {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let operation = VarInt::decode(buf)?;
        if !bool::decode(buf)? {
            return Err(DecodeError::Invalid("string waypoint id"));
        }
        let uuid = Uuid::decode(buf)?;
        let style = String::decode(buf)?;
        let color = if bool::decode(buf)? {
            if buf.len() < 3 {
                return Err(DecodeError::Invalid("waypoint color"));
            }
            let rgb = u32::from_be_bytes([0, buf[0], buf[1], buf[2]]);
            *buf = &buf[3..];
            Some(rgb)
        } else {
            None
        };
        let data = match VarInt::decode(buf)?.0 {
            0 => WaypointData::Empty,
            1 => WaypointData::Position([
                VarInt::decode(buf)?.0,
                VarInt::decode(buf)?.0,
                VarInt::decode(buf)?.0,
            ]),
            2 => WaypointData::Chunk([VarInt::decode(buf)?.0, VarInt::decode(buf)?.0]),
            3 => WaypointData::Azimuth(f32::decode(buf)?),
            _ => return Err(DecodeError::Invalid("waypoint type")),
        };
        Ok(Self {
            operation,
            uuid,
            style,
            color,
            data,
        })
    }
}
impl Packet for TrackedWaypoint {
    const ID: i32 = 141;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerGameMode {
    pub uuid: Uuid,
    pub game_mode: VarInt,
}
impl Encode for PlayerGameMode {
    fn encode(&self, out: &mut Vec<u8>) {
        4u8.encode(out);
        VarInt(1).encode(out);
        self.uuid.encode(out);
        self.game_mode.encode(out);
    }
}
impl Decode for PlayerGameMode {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if u8::decode(buf)? != 4 || VarInt::decode(buf)?.0 != 1 {
            return Err(DecodeError::Invalid("game mode update"));
        }
        Ok(Self {
            uuid: Uuid::decode(buf)?,
            game_mode: VarInt::decode(buf)?,
        })
    }
}
impl Packet for PlayerGameMode {
    const ID: i32 = 71;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
packet!(SetEntityMotion,103,Clientbound,{id:VarInt,velocity:LpVec3});
packet!(TakeItemEntity,127,Clientbound,{item:VarInt,collector:VarInt,amount:VarInt});
// `teleport_entity`: an explicit teleport (`PositionMoveRotation`, relative flags as an int bit
// set, on-ground). Movement updates use `entity_position_sync` (`EntityTeleport`).
packet!(TeleportEntity,128,Clientbound,{id:VarInt,x:f64,y:f64,z:f64,velocity_x:f64,velocity_y:f64,velocity_z:f64,yaw:f32,pitch:f32,relatives:i32,on_ground:bool});

/// One synched entity data value, tagged with its `EntityDataSerializers` id on the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    Byte(i8),
    Int(i32),
    Long(i64),
    Float(f32),
    String(String),
    ItemStack(ItemStack),
    Boolean(bool),
    Pose(i32),
    /// Vanilla `VillagerData`: type and profession registry ids and the trade level.
    VillagerData {
        kind: i32,
        profession: i32,
        level: i32,
    },
    /// Vanilla `OPTIONAL_LIVING_ENTITY_REFERENCE`: a tamed animal's owner.
    OptionalUuid(Option<uuid::Uuid>),
    /// Vanilla `OPTIONAL_BLOCK_POS`: an End crystal's beam target.
    OptionalBlockPos(Option<i64>),
    /// Vanilla `PARTICLE`: a particle type registry id, plus the ARGB colour that tinted
    /// particles (`entity_effect`, protocol id 28) carry as their only option. Particles with
    /// other options (dust, block, item ...) are not modelled.
    Particle {
        id: i32,
        argb: Option<i32>,
    },
}
/// `minecraft:entity_effect` in the particle type registry (26.3): its option is one ARGB int.
const PARTICLE_ENTITY_EFFECT: i32 = 28;
impl MetaValue {
    /// Serializer id in `EntityDataSerializers` registration order (26.3).
    pub fn serializer(&self) -> i32 {
        match self {
            Self::Byte(_) => 0,
            Self::Int(_) => 1,
            Self::Long(_) => 2,
            Self::Float(_) => 3,
            Self::String(_) => 4,
            Self::ItemStack(_) => 7,
            Self::Boolean(_) => 8,
            Self::OptionalBlockPos(_) => 11,
            Self::OptionalUuid(_) => 13,
            Self::Particle { .. } => 16,
            Self::VillagerData { .. } => 18,
            Self::Pose(_) => 20,
        }
    }
}
/// A data value at its accessor index.
#[derive(Debug, Clone, PartialEq)]
pub struct MetaEntry {
    pub index: u8,
    pub value: MetaValue,
}
impl Encode for MetaEntry {
    fn encode(&self, out: &mut Vec<u8>) {
        self.index.encode(out);
        VarInt(self.value.serializer()).encode(out);
        match &self.value {
            MetaValue::Byte(v) => v.encode(out),
            MetaValue::Int(v) | MetaValue::Pose(v) => VarInt(*v).encode(out),
            MetaValue::Long(v) => VarLong(*v).encode(out),
            MetaValue::Float(v) => v.encode(out),
            MetaValue::String(v) => v.encode(out),
            MetaValue::ItemStack(v) => v.encode(out),
            MetaValue::Boolean(v) => v.encode(out),
            MetaValue::VillagerData {
                kind,
                profession,
                level,
            } => {
                VarInt(*kind).encode(out);
                VarInt(*profession).encode(out);
                VarInt(*level).encode(out);
            }
            MetaValue::OptionalUuid(owner) => {
                owner.is_some().encode(out);
                if let Some(owner) = owner {
                    owner.encode(out);
                }
            }
            MetaValue::OptionalBlockPos(pos) => {
                pos.is_some().encode(out);
                if let Some(pos) = pos {
                    pos.encode(out);
                }
            }
            MetaValue::Particle { id, argb } => {
                VarInt(*id).encode(out);
                if let Some(argb) = argb {
                    argb.encode(out);
                }
            }
        }
    }
}
impl MetaEntry {
    fn decode_at(index: u8, buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let value = match VarInt::decode(buf)?.0 {
            0 => MetaValue::Byte(i8::decode(buf)?),
            1 => MetaValue::Int(VarInt::decode(buf)?.0),
            2 => MetaValue::Long(VarLong::decode(buf)?.0),
            3 => MetaValue::Float(f32::decode(buf)?),
            4 => MetaValue::String(String::decode(buf)?),
            7 => MetaValue::ItemStack(ItemStack::decode(buf)?),
            8 => MetaValue::Boolean(bool::decode(buf)?),
            11 => MetaValue::OptionalBlockPos(if bool::decode(buf)? {
                Some(i64::decode(buf)?)
            } else {
                None
            }),
            13 => MetaValue::OptionalUuid(if bool::decode(buf)? {
                Some(uuid::Uuid::decode(buf)?)
            } else {
                None
            }),
            16 => {
                let id = VarInt::decode(buf)?.0;
                let argb = if id == PARTICLE_ENTITY_EFFECT {
                    Some(i32::decode(buf)?)
                } else {
                    None
                };
                MetaValue::Particle { id, argb }
            }
            18 => MetaValue::VillagerData {
                kind: VarInt::decode(buf)?.0,
                profession: VarInt::decode(buf)?.0,
                level: VarInt::decode(buf)?.0,
            },
            20 => MetaValue::Pose(VarInt::decode(buf)?.0),
            _ => return Err(DecodeError::Invalid("unsupported entity data serializer")),
        };
        Ok(Self { index, value })
    }
}
/// `set_entity_data`: changed data values, terminated by index 255.
#[derive(Debug, Clone, PartialEq)]
pub struct SetEntityData {
    pub id: VarInt,
    pub values: Vec<MetaEntry>,
}
impl Encode for SetEntityData {
    fn encode(&self, out: &mut Vec<u8>) {
        self.id.encode(out);
        for value in &self.values {
            value.encode(out);
        }
        out.push(0xFF);
    }
}
impl Decode for SetEntityData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let id = VarInt::decode(buf)?;
        let mut values = Vec::new();
        loop {
            match u8::decode(buf)? {
                0xFF => return Ok(Self { id, values }),
                index => values.push(MetaEntry::decode_at(index, buf)?),
            }
        }
    }
}
impl Packet for SetEntityData {
    const ID: i32 = 101;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
packet!(PlayerAction,41,Serverbound,{status:VarInt,location:i64,face:i8,sequence:VarInt});
packet!(UseItemOn,66,Serverbound,{hand:VarInt,location:i64,face:VarInt,cursor_x:f32,cursor_y:f32,cursor_z:f32,inside:bool,world_border_hit:bool,sequence:VarInt});
packet!(BlockUpdate,8,Clientbound,{location:i64,block_state:VarInt});
/// Vanilla `ClientboundBlockEntityDataPacket`: the block position, the block entity type's
/// registry id and the block entity's NBT. The tag is the last field and is sent without the
/// chunk-local `x`/`y`/`z` keys the save format stores.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntityData {
    pub location: i64,
    pub block_type: VarInt,
    pub nbt: ferrite_nbt::Compound,
}
impl Encode for BlockEntityData {
    fn encode(&self, out: &mut Vec<u8>) {
        self.location.encode(out);
        self.block_type.encode(out);
        let tag = ferrite_nbt::Tag::Compound(self.nbt.clone());
        match ferrite_nbt::binary::write_network(&tag) {
            Ok(bytes) => out.extend_from_slice(&bytes),
            // `write_network` only fails on a tag deeper or larger than its bounds, which a
            // block entity never is; TAG_End keeps the packet structurally decodable.
            Err(_) => out.push(0),
        }
    }
}
impl Decode for BlockEntityData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let location = i64::decode(buf)?;
        let block_type = VarInt::decode(buf)?;
        let tag = ferrite_nbt::binary::read_network(buf)
            .map_err(|error| DecodeError::Nbt(error.to_string()))?;
        *buf = &[];
        match tag {
            ferrite_nbt::Tag::Compound(nbt) => Ok(Self {
                location,
                block_type,
                nbt,
            }),
            _ => Err(DecodeError::Invalid("block entity nbt")),
        }
    }
}
impl Packet for BlockEntityData {
    const ID: i32 = 6;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
/// `show_dialog` with an inline dialog: holder id 0, then the dialog as network NBT.
#[derive(Debug, Clone, PartialEq)]
pub struct ShowDialogInline {
    pub dialog: ferrite_nbt::Compound,
}
impl Encode for ShowDialogInline {
    fn encode(&self, out: &mut Vec<u8>) {
        VarInt(0).encode(out);
        match ferrite_nbt::binary::write_network(&ferrite_nbt::Tag::Compound(self.dialog.clone())) {
            Ok(bytes) => out.extend_from_slice(&bytes),
            Err(_) => out.push(0),
        }
    }
}
impl Decode for ShowDialogInline {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if VarInt::decode(buf)?.0 != 0 {
            return Err(DecodeError::Invalid("registered dialog"));
        }
        let tag = ferrite_nbt::binary::read_network(buf)
            .map_err(|error| DecodeError::Nbt(error.to_string()))?;
        *buf = &[];
        match tag {
            ferrite_nbt::Tag::Compound(dialog) => Ok(Self { dialog }),
            _ => Err(DecodeError::Invalid("dialog nbt")),
        }
    }
}
impl Packet for ShowDialogInline {
    const ID: i32 = 143;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
// Vanilla `ServerboundSetCommandBlockPacket`: the command, the command block mode
// (0 = impulse, 1 = chain, 2 = repeat) and the three editor toggles.
packet!(SetCommandBlock,55,Serverbound,{location:i64,command:String,mode:VarInt,track_output:bool,conditional:bool,automatic:bool});
packet!(AcknowledgeBlockChange,4,Clientbound,{sequence:VarInt});
packet!(OpenBook,59,Clientbound,{hand:VarInt});
/// One 26.3 `MapDecoration`: a registry decoration type, signed map-relative position, a
/// four-bit rotation and an optional plain-text caption (network NBT `TAG_String`).
#[derive(Debug, Clone, PartialEq)]
pub struct MapDecoration {
    pub kind: VarInt,
    pub x: i8,
    pub y: i8,
    pub direction: i8,
    pub caption: Option<PlainText>,
}
impl Encode for MapDecoration {
    fn encode(&self, out: &mut Vec<u8>) {
        self.kind.encode(out);
        self.x.encode(out);
        self.y.encode(out);
        self.direction.encode(out);
        self.caption.encode(out);
    }
}
impl Decode for MapDecoration {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: VarInt::decode(buf)?,
            x: i8::decode(buf)?,
            y: i8::decode(buf)?,
            direction: i8::decode(buf)?,
            caption: Option::decode(buf)?,
        })
    }
}
/// `minecraft:map_item_data`: optional decorations followed by an optional full or partial
/// colour patch. The patch is encoded as `width`, and only when `width` is non-zero, `height`,
/// `startX`, `startY` and the colour byte array (vanilla `MapPatch.STREAM_CODEC`).
#[derive(Debug, Clone, PartialEq)]
pub struct MapItemData {
    pub map_id: VarInt,
    pub scale: u8,
    pub locked: bool,
    pub decorations: Option<Vec<MapDecoration>>,
    pub width: u8,
    pub height: u8,
    pub x: u8,
    pub z: u8,
    pub colors: Vec<u8>,
}
impl Encode for MapItemData {
    fn encode(&self, out: &mut Vec<u8>) {
        self.map_id.encode(out);
        self.scale.encode(out);
        self.locked.encode(out);
        self.decorations.encode(out);
        self.width.encode(out);
        if self.width != 0 {
            self.height.encode(out);
            self.x.encode(out);
            self.z.encode(out);
            self.colors.encode(out);
        }
    }
}
impl Decode for MapItemData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let map_id = VarInt::decode(buf)?;
        let scale = u8::decode(buf)?;
        let locked = bool::decode(buf)?;
        let decorations = Option::<Vec<MapDecoration>>::decode(buf)?;
        let width = u8::decode(buf)?;
        if width == 0 {
            return Ok(Self {
                map_id,
                scale,
                locked,
                decorations,
                width,
                height: 0,
                x: 0,
                z: 0,
                colors: Vec::new(),
            });
        }
        Ok(Self {
            map_id,
            scale,
            locked,
            decorations,
            width,
            height: u8::decode(buf)?,
            x: u8::decode(buf)?,
            z: u8::decode(buf)?,
            colors: Vec::decode(buf)?,
        })
    }
}
impl Packet for MapItemData {
    const ID: i32 = 52;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
packet!(SetHeldItem,54,Serverbound,{slot:i16});
// Vanilla `ServerboundAttackPacket`: left-click on an entity, by network id.
packet!(ServerboundAttack,1,Serverbound,{entity_id:VarInt});
// Vanilla `ServerboundInteractPacket`: use or attack an entity at a point in its box.
packet!(ServerboundInteract,26,Serverbound,{entity_id:VarInt,hand:VarInt,x:f64,y:f64,z:f64,using_secondary_action:bool});
// Vanilla `ClientboundHurtAnimationPacket`: `yaw` is the hurt direction in degrees.
packet!(HurtAnimation,43,Clientbound,{id:VarInt,yaw:f32});
// Vanilla `ClientboundAnimatePacket`: 4 = critical hit, 5 = magic critical hit.
packet!(Animate,2,Clientbound,{id:VarInt,action:u8});
packet!(EntityEvent,34,Clientbound,{entity_id:i32,status:u8});
packet!(LevelEvent,47,Clientbound,{event:i32,location:i64,data:i32,global:bool});

/// Packets carrying item stacks are generic over the stack type: this crate cannot depend on
/// `ferrite-item`, whose full `ItemStack` (and `HashedSlot`) implement `Encode`/`Decode` behind
/// its `protocol` feature. The default is the minimal [`ItemStack`].
macro_rules! item_packet {($name:ident<$g:ident $(= $d:ty)?>,$id:expr,$bound:ident,{$($f:ident:$t:ty),* $(,)?})=>{
    #[derive(Debug,Clone,PartialEq)] pub struct $name<$g $(= $d)?> {$(pub $f:$t),*}
    impl<$g: Encode> Encode for $name<$g> {fn encode(&self,out:&mut Vec<u8>){$(self.$f.encode(out);)*}}
    impl<$g: Decode> Decode for $name<$g> {fn decode(buf:&mut &[u8])->Result<Self,DecodeError>{Ok(Self{$($f:<$t>::decode(buf)?),*})}}
    impl<$g: Encode + Decode> Packet for $name<$g> {const ID:i32=$id;const STATE:State=State::Play;const BOUND:Bound=Bound::$bound;}
};}
item_packet!(CreativeSlot<I = ItemStack>,57,Serverbound,{slot:i16,item:I});
item_packet!(ContainerContent<I = ItemStack>,18,Clientbound,{window:VarInt,state:VarInt,slots:Vec<I>,carried:I});
item_packet!(ContainerSlot<I = ItemStack>,20,Clientbound,{window:VarInt,state:VarInt,slot:i16,item:I});
/// Vanilla `ClientboundSetEquipmentPacket`: the entity id, then `(slot, item)` pairs where every slot
/// byte but the last has bit 0x80 set. Slots: 0 main hand, 1 off hand, 2 feet, 3 legs, 4 chest, 5 head.
#[derive(Debug, Clone, PartialEq)]
pub struct SetEquipment<I = ItemStack> {
    pub entity_id: VarInt,
    pub slots: Vec<(u8, I)>,
}
impl<I: Encode> Encode for SetEquipment<I> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.entity_id.encode(out);
        for (i, (slot, item)) in self.slots.iter().enumerate() {
            let more = if i + 1 < self.slots.len() { 0x80 } else { 0 };
            (slot & 0x7f | more).encode(out);
            item.encode(out);
        }
    }
}
impl<I: Decode> Decode for SetEquipment<I> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let entity_id = VarInt::decode(buf)?;
        let mut slots = Vec::new();
        loop {
            let byte = u8::decode(buf)?;
            slots.push((byte & 0x7f, I::decode(buf)?));
            if byte & 0x80 == 0 {
                break;
            }
        }
        Ok(Self { entity_id, slots })
    }
}
impl<I: Encode + Decode> Packet for SetEquipment<I> {
    const ID: i32 = 104;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
/// One link of `ClientboundServerLinksPacket`: a known type (its index in vanilla `KnownLinkType`,
/// 0 bug report .. 9 announcements) or a plain-text label, then the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLinkEntry {
    pub known: Option<i32>,
    pub label: String,
    pub url: String,
}
impl Encode for ServerLinkEntry {
    fn encode(&self, out: &mut Vec<u8>) {
        match self.known {
            Some(kind) => {
                true.encode(out);
                VarInt(kind).encode(out);
            }
            None => {
                false.encode(out);
                crate::types::PlainText(self.label.clone()).encode(out);
            }
        }
        self.url.encode(out);
    }
}
impl Decode for ServerLinkEntry {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let (known, label) = if bool::decode(buf)? {
            (Some(VarInt::decode(buf)?.0), String::new())
        } else {
            (None, crate::types::PlainText::decode(buf)?.0)
        };
        Ok(Self {
            known,
            label,
            url: String::decode(buf)?,
        })
    }
}
packet!(ServerLinks,140,Clientbound,{links:Vec<ServerLinkEntry>});
packet!(CustomPayload,24,Clientbound,{channel:String,data:crate::codec::RestBytes});
packet!(Transfer,132,Clientbound,{host:String,port:VarInt});
packet!(SetCamera,95,Clientbound,{entity:VarInt});
packet!(PostEffects,83,Clientbound,{effects:Vec<String>});
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearDialog;
impl Encode for ClearDialog {
    fn encode(&self, _: &mut Vec<u8>) {}
}
impl Decode for ClearDialog {
    fn decode(_: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self)
    }
}
impl Packet for ClearDialog {
    const ID: i32 = 142;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}
// A dialog sent as a registry reference: the holder is the entry's registry index plus one.
packet!(ShowDialog,143,Clientbound,{holder:VarInt});
packet!(ResourcePackPush,82,Clientbound,{id:Uuid,url:String,hash:String,required:bool,prompt:Option<Vec<u8>>});
packet!(ResourcePackPop,81,Clientbound,{id:Option<Uuid>});
// Vanilla `ClientboundStoreCookiePacket`: the client keeps `payload` (at most 5120 bytes) under `key`.
packet!(StoreCookie,122,Clientbound,{key:String,payload:Vec<u8>});
// Vanilla `ClientboundCookieRequestPacket`: asks the client for the cookie stored under `key`.
packet!(CookieRequest,21,Clientbound,{key:String});
// Vanilla `ServerboundCookieResponsePacket`: the cookie the client holds, if any.
packet!(CookieResponse,21,Serverbound,{key:String,payload:Option<Vec<u8>>});
packet!(ItemCooldown,22,Clientbound,{group:String,ticks:VarInt});
packet!(SetExperience,105,Clientbound,{progress:f32,level:VarInt,total:VarInt});
// Vanilla `ClientboundPlayerAbilitiesPacket`: flags (1 invulnerable, 2 flying, 4 may fly, 8 instant build),
// flying speed and the walking speed (field-of-view modifier).
packet!(PlayerAbilities,65,Clientbound,{flags:u8,flying_speed:f32,walking_speed:f32});
// Vanilla `ClientboundSetHealthPacket`: health, food level and saturation.
packet!(SetHealth,106,Clientbound,{health:f32,food:VarInt,saturation:f32});
// Vanilla `ClientboundPlayerCombatKillPacket`: the death screen (`player_id`, death message).
packet!(PlayerCombatKill,69,Clientbound,{player_id:VarInt,message:crate::types::PlainText});
// Vanilla `ClientboundRespawnPacket`: `CommonPlayerSpawnInfo` followed by the
// `dataToKeep` bit set (1 = attributes, 2 = entity data, 3 = both).
packet!(Respawn,84,Clientbound,{dimension_type:VarInt,dimension:String,seed:i64,game_mode:VarInt,previous_game_mode:VarInt,debug:bool,flat:bool,death_location:Option<DeathLocation>,portal_cooldown:VarInt,sea_level:VarInt,data_to_keep:u8});
// Vanilla `ServerboundClientCommandPacket`: 0 = perform respawn, 1 = request stats.
packet!(ServerboundClientCommand,12,Serverbound,{action:VarInt});
// Vanilla `ServerboundUseItemPacket`: right-click in the air (eating uses this).
packet!(ServerboundUseItem,67,Serverbound,{hand:VarInt,sequence:VarInt,yaw:f32,pitch:f32});
// Vanilla `ServerboundPlayerCommandPacket`: 3/4 = start/stop sprinting.
packet!(ServerboundPlayerCommand,42,Serverbound,{entity_id:VarInt,action:VarInt,data:VarInt});
// Vanilla `ServerboundPlayerInputPacket`: `Input` flags (1 forward, 2 backward, 4 left, 8 right,
// 16 jump, 32 shift, 64 sprint), sent whenever they change.
packet!(PlayerInput,43,Serverbound,{flags:u8});
// Vanilla `ServerboundResourcePackPacket`: the client's progress on a pack the server pushed.
// `action` is `Action.ordinal()`: 0 loaded, 1 declined, 2 failed download, 3 accepted,
// 4 downloaded, 5 invalid url, 6 failed reload, 7 discarded.
packet!(ResourcePackResponse,50,Serverbound,{id:Uuid,action:VarInt});
item_packet!(SetCursorItem<I = ItemStack>,98,Clientbound,{item:I});
item_packet!(SetPlayerInventory<I = ItemStack>,110,Clientbound,{slot:VarInt,item:I});
packet!(ServerboundContainerClose,19,Serverbound,{window:VarInt});
packet!(ClientboundContainerClose,17,Clientbound,{window:VarInt});
// Vanilla `ClientboundBlockEventPacket`: a block entity's `triggerEvent` (a beacon's beam
// height, a chest lid). `block` is the block registry id, then the two event bytes.
packet!(BlockEvent,7,Clientbound,{pos:crate::types::BlockPos,block:VarInt,param_a:u8,param_b:u8});
packet!(SetHeldSlot,107,Clientbound,{slot:VarInt});
packet!(OpenScreen,60,Clientbound,{window:VarInt,menu:VarInt,title:crate::types::Translatable});
// `mount_screen_open`: the horse inventory screen for entity `entity` with `columns` chest columns.
packet!(MountScreenOpen,42,Clientbound,{window:VarInt,columns:VarInt,entity:i32});
// `set_entity_link`: a lead from `attached` to `holder`; holder 0 takes it off.
packet!(SetEntityLink,102,Clientbound,{attached:i32,holder:i32});
// Vanilla `ClientboundContainerSetDataPacket`: one menu property (furnace lit/progress, anvil
// cost, enchanting costs and seed, stonecutter selection).
packet!(ContainerSetData,19,Clientbound,{window:VarInt,id:i16,value:i16});
// Vanilla `ServerboundContainerButtonClickPacket`: enchant choice, stonecutter recipe, ...
packet!(ContainerButtonClick,17,Serverbound,{window:VarInt,button:VarInt});
// Vanilla `ServerboundSetBeaconPacket`: the two effects the player picked (empty = none).
packet!(SetBeacon,53,Serverbound,{primary:Option<VarInt>,secondary:Option<VarInt>});
// Vanilla `ServerboundRenameItemPacket` (anvil): the name field is capped at 50 by the server.
packet!(RenameItem,49,Serverbound,{name:String});
// Vanilla `ServerboundPlaceRecipePacket`: a click on a recipe in the recipe book.
packet!(PlaceRecipe,39,Serverbound,{window:VarInt,display_id:VarInt,use_max:bool});

/// Vanilla `ItemCost`: an item, the count of it the trade wants and the data-component predicate
/// the stack must match. Offers generated from the vanilla trade data never carry a predicate, so
/// decoding a non-empty one is rejected rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemCost {
    pub item: VarInt,
    pub count: VarInt,
}
impl Encode for ItemCost {
    fn encode(&self, out: &mut Vec<u8>) {
        self.item.encode(out);
        self.count.encode(out);
        // `DataComponentExactPredicate.STREAM_CODEC`: a list length, then typed components.
        VarInt(0).encode(out);
    }
}
impl Decode for ItemCost {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let item = VarInt::decode(buf)?;
        let count = VarInt::decode(buf)?;
        if VarInt::decode(buf)?.0 != 0 {
            return Err(DecodeError::Invalid("item cost component predicate"));
        }
        Ok(Self { item, count })
    }
}

/// One vanilla `MerchantOffer`, as encoded in `merchant_offers`.
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantOffer<I = ItemStack> {
    pub wants: ItemCost,
    pub gives: I,
    /// Vanilla `costB`, shown as a second input slot.
    pub second_wants: Option<ItemCost>,
    /// Vanilla `isOutOfStock()`: `uses >= max_uses`.
    pub out_of_stock: bool,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub special_price: i32,
    pub price_multiplier: f32,
    pub demand: i32,
}
impl<I: Encode> Encode for MerchantOffer<I> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.wants.encode(out);
        self.gives.encode(out);
        self.second_wants.encode(out);
        self.out_of_stock.encode(out);
        self.uses.encode(out);
        self.max_uses.encode(out);
        self.xp.encode(out);
        self.special_price.encode(out);
        self.price_multiplier.encode(out);
        self.demand.encode(out);
    }
}
impl<I: Decode> Decode for MerchantOffer<I> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            wants: ItemCost::decode(buf)?,
            gives: I::decode(buf)?,
            second_wants: Option::<ItemCost>::decode(buf)?,
            out_of_stock: bool::decode(buf)?,
            uses: i32::decode(buf)?,
            max_uses: i32::decode(buf)?,
            xp: i32::decode(buf)?,
            special_price: i32::decode(buf)?,
            price_multiplier: f32::decode(buf)?,
            demand: i32::decode(buf)?,
        })
    }
}

/// Vanilla `ClientboundMerchantOffersPacket.MAX_OFFERS`.
pub const MAX_MERCHANT_OFFERS: usize = 128;

/// `merchant_offers`: the merchant menu's offers plus the villager's level, trade XP and restock
/// state.
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantOffers<I = ItemStack> {
    pub window: VarInt,
    pub offers: Vec<MerchantOffer<I>>,
    pub level: VarInt,
    pub xp: VarInt,
    pub show_progress: bool,
    pub can_restock: bool,
}
impl<I: Encode> Encode for MerchantOffers<I> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.window.encode(out);
        VarInt(self.offers.len() as i32).encode(out);
        for offer in &self.offers {
            offer.encode(out);
        }
        self.level.encode(out);
        self.xp.encode(out);
        self.show_progress.encode(out);
        self.can_restock.encode(out);
    }
}
impl<I: Decode> Decode for MerchantOffers<I> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let window = VarInt::decode(buf)?;
        let len = usize::try_from(VarInt::decode(buf)?.0)
            .ok()
            .filter(|&len| len <= MAX_MERCHANT_OFFERS)
            .ok_or(DecodeError::Invalid("too many merchant offers"))?;
        let mut offers = Vec::with_capacity(len);
        for _ in 0..len {
            offers.push(MerchantOffer::<I>::decode(buf)?);
        }
        Ok(Self {
            window,
            offers,
            level: VarInt::decode(buf)?,
            xp: VarInt::decode(buf)?,
            show_progress: bool::decode(buf)?,
            can_restock: bool::decode(buf)?,
        })
    }
}
impl<I: Encode + Decode> Packet for MerchantOffers<I> {
    const ID: i32 = 53;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

// Vanilla `ServerboundSelectTradePacket`: the index of the offer the client selected.
packet!(SelectTrade,52,Serverbound,{slot:VarInt});

/// Vanilla `ServerboundContainerClickPacket.MAX_SLOT_COUNT`.
pub const CONTAINER_CLICK_MAX_SLOTS: usize = 128;

/// `container_click`. `H` is the hashed stack (`ferrite_item::HashedSlot`); `changed` holds at
/// most [`CONTAINER_CLICK_MAX_SLOTS`] `(slot, predicted stack)` pairs, `None` being empty.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerClick<H> {
    pub window: VarInt,
    pub state: VarInt,
    pub slot: i16,
    pub button: i8,
    pub input: VarInt,
    pub changed: Vec<(i16, Option<H>)>,
    pub carried: Option<H>,
}
impl<H: Encode> Encode for ContainerClick<H> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.window.encode(out);
        self.state.encode(out);
        self.slot.encode(out);
        self.button.encode(out);
        self.input.encode(out);
        VarInt(self.changed.len() as i32).encode(out);
        for (slot, item) in &self.changed {
            slot.encode(out);
            item.encode(out);
        }
        self.carried.encode(out);
    }
}
impl<H: Decode> Decode for ContainerClick<H> {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let window = VarInt::decode(buf)?;
        let state = VarInt::decode(buf)?;
        let slot = i16::decode(buf)?;
        let button = i8::decode(buf)?;
        let input = VarInt::decode(buf)?;
        let len = usize::try_from(VarInt::decode(buf)?.0)
            .ok()
            .filter(|&len| len <= CONTAINER_CLICK_MAX_SLOTS)
            .ok_or(DecodeError::Invalid("too many changed slots"))?;
        let mut changed = Vec::with_capacity(len);
        for _ in 0..len {
            changed.push((i16::decode(buf)?, Option::<H>::decode(buf)?));
        }
        let carried = Option::<H>::decode(buf)?;
        Ok(Self {
            window,
            state,
            slot,
            button,
            input,
            changed,
            carried,
        })
    }
}
impl<H: Encode + Decode> Packet for ContainerClick<H> {
    const ID: i32 = 18;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

/// Minimal item stack; component patches are deliberately rejected until supported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemStack {
    pub count: i32,
    pub item_id: i32,
}
impl Encode for ItemStack {
    fn encode(&self, out: &mut Vec<u8>) {
        VarInt(self.count).encode(out);
        if self.count > 0 {
            VarInt(self.item_id).encode(out);
            VarInt(0).encode(out);
            VarInt(0).encode(out);
        }
    }
}
impl Decode for ItemStack {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let count = VarInt::decode(buf)?.0;
        if count == 0 {
            return Ok(Self::default());
        }
        if !(1..=99).contains(&count) {
            return Err(DecodeError::Invalid("invalid item count"));
        }
        let item_id = VarInt::decode(buf)?.0;
        if item_id < 0 {
            return Err(DecodeError::Invalid("invalid item id"));
        }
        if VarInt::decode(buf)?.0 != 0 || VarInt::decode(buf)?.0 != 0 {
            return Err(DecodeError::Invalid("item components unsupported"));
        }
        Ok(Self { count, item_id })
    }
}

/// A 256-byte `SHA256withRSA` chat signature (`MessageSignature` in vanilla). ED25519 was never
/// used for Java chat: profile keys are 2048-bit RSA keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageSignature(pub [u8; 256]);
impl Encode for MessageSignature {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0);
    }
}
impl Decode for MessageSignature {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let bytes = crate::codec::take(buf, 256)?;
        Ok(Self(bytes.try_into().expect("exactly 256 bytes")))
    }
}

/// `LastSeenMessages.Update`: offset into the acknowledged queue, a 20-bit bitset (three bytes,
/// little-endian) of acknowledged entries, and the checksum (zero means unchecked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSeenUpdate {
    pub offset: VarInt,
    pub acknowledged: [u8; 3],
    pub checksum: i8,
}
impl LastSeenUpdate {
    pub fn is_acknowledged(&self, index: usize) -> bool {
        index < 20 && self.acknowledged[index / 8] & (1 << (index % 8)) != 0
    }
    pub fn new(offset: i32, acknowledged: &[usize], checksum: i8) -> Self {
        let mut bits = [0u8; 3];
        for &index in acknowledged {
            bits[index / 8] |= 1 << (index % 8);
        }
        Self {
            offset: VarInt(offset),
            acknowledged: bits,
            checksum,
        }
    }
}
impl Encode for LastSeenUpdate {
    fn encode(&self, out: &mut Vec<u8>) {
        self.offset.encode(out);
        out.extend_from_slice(&self.acknowledged);
        self.checksum.encode(out);
    }
}
impl Decode for LastSeenUpdate {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            offset: VarInt::decode(buf)?,
            acknowledged: crate::codec::take(buf, 3)?
                .try_into()
                .expect("exactly 3 bytes"),
            checksum: i8::decode(buf)?,
        })
    }
}

/// `ProfilePublicKey.Data`: expiry (epoch millis), X.509 RSA key, and Mojang's signature over both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileKeyData {
    pub expires_at: i64,
    pub key: Vec<u8>,
    pub key_signature: Vec<u8>,
}
impl Encode for ProfileKeyData {
    fn encode(&self, out: &mut Vec<u8>) {
        self.expires_at.encode(out);
        self.key.encode(out);
        self.key_signature.encode(out);
    }
}
impl Decode for ProfileKeyData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let expires_at = i64::decode(buf)?;
        let key = decode_byte_array(buf, 512)?;
        let key_signature = decode_byte_array(buf, 4096)?;
        Ok(Self {
            expires_at,
            key,
            key_signature,
        })
    }
}
fn decode_byte_array(buf: &mut &[u8], max: usize) -> Result<Vec<u8>, DecodeError> {
    let len = VarInt::decode(buf)?.0;
    let len = usize::try_from(len).map_err(|_| DecodeError::Invalid("negative array length"))?;
    if len > max {
        return Err(DecodeError::Invalid("byte array too long"));
    }
    Ok(crate::codec::take(buf, len)?.to_vec())
}

/// `RemoteChatSession.Data`: the session UUID plus the profile public key. Sent by the client in
/// `chat_session_update` and echoed to every client in `player_info` so it can verify signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChatSessionData {
    pub session_id: Uuid,
    pub profile_key: ProfileKeyData,
}
impl Encode for RemoteChatSessionData {
    fn encode(&self, out: &mut Vec<u8>) {
        self.session_id.encode(out);
        self.profile_key.encode(out);
    }
}
impl Decode for RemoteChatSessionData {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            session_id: Uuid::decode(buf)?,
            profile_key: ProfileKeyData::decode(buf)?,
        })
    }
}

/// `ClientboundPlayerInfoUpdatePacket` carrying only `INITIALIZE_CHAT`, broadcast when a player's
/// chat session changes after joining.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInfoChatSession {
    pub uuid: Uuid,
    pub chat_session: Option<RemoteChatSessionData>,
}
impl Encode for PlayerInfoChatSession {
    fn encode(&self, out: &mut Vec<u8>) {
        2u8.encode(out);
        VarInt(1).encode(out);
        self.uuid.encode(out);
        self.chat_session.encode(out);
    }
}
impl Decode for PlayerInfoChatSession {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if u8::decode(buf)? != 2 || VarInt::decode(buf)?.0 != 1 {
            return Err(DecodeError::Invalid("chat session update actions"));
        }
        Ok(Self {
            uuid: Uuid::decode(buf)?,
            chat_session: Option::decode(buf)?,
        })
    }
}
impl Packet for PlayerInfoChatSession {
    const ID: i32 = 71;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// Argument signature used by signed chat commands (`ArgumentSignatures.Entry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentSignature {
    pub name: String,
    pub signature: MessageSignature,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerboundChat {
    pub message: String,
    /// Epoch milliseconds.
    pub timestamp: i64,
    pub salt: i64,
    pub signature: Option<MessageSignature>,
    pub last_seen: LastSeenUpdate,
}
impl Encode for ServerboundChat {
    fn encode(&self, out: &mut Vec<u8>) {
        self.message.encode(out);
        self.timestamp.encode(out);
        self.salt.encode(out);
        self.signature.encode(out);
        self.last_seen.encode(out);
    }
}
impl Decode for ServerboundChat {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            message: crate::codec::decode_string(buf, 256)?,
            timestamp: i64::decode(buf)?,
            salt: i64::decode(buf)?,
            signature: Option::decode(buf)?,
            last_seen: LastSeenUpdate::decode(buf)?,
        })
    }
}
impl Packet for ServerboundChat {
    const ID: i32 = 9;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerboundChatAck {
    pub offset: VarInt,
}
impl Encode for ServerboundChatAck {
    fn encode(&self, out: &mut Vec<u8>) {
        self.offset.encode(out);
    }
}
impl Decode for ServerboundChatAck {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            offset: VarInt::decode(buf)?,
        })
    }
}
impl Packet for ServerboundChatAck {
    const ID: i32 = 6;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerboundChatCommand {
    pub command: String,
}
impl Encode for ServerboundChatCommand {
    fn encode(&self, out: &mut Vec<u8>) {
        self.command.encode(out);
    }
}
impl Decode for ServerboundChatCommand {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            command: String::decode(buf)?,
        })
    }
}
impl Packet for ServerboundChatCommand {
    const ID: i32 = 7;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerboundChatCommandSigned {
    pub command: String,
    pub timestamp: i64,
    pub salt: i64,
    pub argument_signatures: Vec<ArgumentSignature>,
    pub last_seen: LastSeenUpdate,
}
impl Encode for ServerboundChatCommandSigned {
    fn encode(&self, out: &mut Vec<u8>) {
        self.command.encode(out);
        self.timestamp.encode(out);
        self.salt.encode(out);
        VarInt(self.argument_signatures.len() as i32).encode(out);
        for entry in &self.argument_signatures {
            entry.name.encode(out);
            entry.signature.encode(out);
        }
        self.last_seen.encode(out);
    }
}
impl Decode for ServerboundChatCommandSigned {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let command = String::decode(buf)?;
        let timestamp = i64::decode(buf)?;
        let salt = i64::decode(buf)?;
        let len = VarInt::decode(buf)?.0;
        let len =
            usize::try_from(len).map_err(|_| DecodeError::Invalid("negative array length"))?;
        if len > 8 {
            return Err(DecodeError::Invalid("too many argument signatures"));
        }
        let mut argument_signatures = Vec::with_capacity(len);
        for _ in 0..len {
            argument_signatures.push(ArgumentSignature {
                name: crate::codec::decode_string(buf, 16)?,
                signature: MessageSignature::decode(buf)?,
            });
        }
        Ok(Self {
            command,
            timestamp,
            salt,
            argument_signatures,
            last_seen: LastSeenUpdate::decode(buf)?,
        })
    }
}
impl Packet for ServerboundChatCommandSigned {
    const ID: i32 = 8;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerboundChatSessionUpdate {
    pub chat_session: RemoteChatSessionData,
}
impl Encode for ServerboundChatSessionUpdate {
    fn encode(&self, out: &mut Vec<u8>) {
        self.chat_session.encode(out);
    }
}
impl Decode for ServerboundChatSessionUpdate {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            chat_session: RemoteChatSessionData::decode(buf)?,
        })
    }
}
impl Packet for ServerboundChatSessionUpdate {
    const ID: i32 = 10;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

/// The chat-type registry holder (`VarInt` id + 1, 0 meaning an inline definition), the sender
/// name shown by the chat decoration, and an optional recipient name for private messages.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatTypeBound {
    pub chat_type: VarInt,
    pub name: crate::types::PlainText,
    pub target_name: Option<crate::types::PlainText>,
}
impl Encode for ChatTypeBound {
    fn encode(&self, out: &mut Vec<u8>) {
        VarInt(self.chat_type.0 + 1).encode(out);
        self.name.encode(out);
        self.target_name.encode(out);
    }
}
impl Decode for ChatTypeBound {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let id = VarInt::decode(buf)?.0;
        if id == 0 {
            return Err(DecodeError::Invalid("inline chat types unsupported"));
        }
        Ok(Self {
            chat_type: VarInt(id - 1),
            name: crate::types::PlainText::decode(buf)?,
            target_name: Option::decode(buf)?,
        })
    }
}

/// `SignedMessageBody.Packed`: content, epoch-millisecond timestamp, salt, and the signature
/// chain's last-seen entries (encoded as full signatures).
#[derive(Debug, Clone, PartialEq)]
pub struct SignedMessageBody {
    pub content: String,
    pub timestamp: i64,
    pub salt: i64,
    pub last_seen: Vec<MessageSignature>,
}
impl Encode for SignedMessageBody {
    fn encode(&self, out: &mut Vec<u8>) {
        self.content.encode(out);
        self.timestamp.encode(out);
        self.salt.encode(out);
        VarInt(self.last_seen.len() as i32).encode(out);
        for signature in &self.last_seen {
            // `MessageSignature.Packed` id -1: the full 256-byte signature follows.
            VarInt(0).encode(out);
            signature.encode(out);
        }
    }
}
impl Decode for SignedMessageBody {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let content = crate::codec::decode_string(buf, 256)?;
        let timestamp = i64::decode(buf)?;
        let salt = i64::decode(buf)?;
        let len = VarInt::decode(buf)?.0;
        let len =
            usize::try_from(len).map_err(|_| DecodeError::Invalid("negative array length"))?;
        if len > 20 {
            return Err(DecodeError::Invalid("too many last-seen signatures"));
        }
        let mut last_seen = Vec::with_capacity(len);
        for _ in 0..len {
            if VarInt::decode(buf)?.0 != 0 {
                return Err(DecodeError::Invalid("packed signature cache unsupported"));
            }
            last_seen.push(MessageSignature::decode(buf)?);
        }
        Ok(Self {
            content,
            timestamp,
            salt,
            last_seen,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundPlayerChat {
    /// Per-recipient chat index (the client's acknowledgement counter).
    pub global_index: VarInt,
    pub sender: Uuid,
    /// The sender's signature-chain index.
    pub index: VarInt,
    pub signature: Option<MessageSignature>,
    pub body: SignedMessageBody,
    pub unsigned_content: Option<crate::types::PlainText>,
    /// `FilterMask` type id; 0 is pass-through.
    pub filter_mask: VarInt,
    pub chat_type: ChatTypeBound,
}
impl Encode for ClientboundPlayerChat {
    fn encode(&self, out: &mut Vec<u8>) {
        self.global_index.encode(out);
        self.sender.encode(out);
        self.index.encode(out);
        self.signature.encode(out);
        self.body.encode(out);
        self.unsigned_content.encode(out);
        self.filter_mask.encode(out);
        self.chat_type.encode(out);
    }
}
impl Decode for ClientboundPlayerChat {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            global_index: VarInt::decode(buf)?,
            sender: Uuid::decode(buf)?,
            index: VarInt::decode(buf)?,
            signature: Option::decode(buf)?,
            body: SignedMessageBody::decode(buf)?,
            unsigned_content: Option::decode(buf)?,
            filter_mask: VarInt::decode(buf)?,
            chat_type: ChatTypeBound::decode(buf)?,
        })
    }
}
impl Packet for ClientboundPlayerChat {
    const ID: i32 = 66;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundDisguisedChat {
    pub message: crate::types::PlainText,
    pub chat_type: ChatTypeBound,
}
impl Encode for ClientboundDisguisedChat {
    fn encode(&self, out: &mut Vec<u8>) {
        self.message.encode(out);
        self.chat_type.encode(out);
    }
}
impl Decode for ClientboundDisguisedChat {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            message: crate::types::PlainText::decode(buf)?,
            chat_type: ChatTypeBound::decode(buf)?,
        })
    }
}
impl Packet for ClientboundDisguisedChat {
    const ID: i32 = 33;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

#[cfg(test)]
mod chat_tests {
    use super::*;
    fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: T) {
        let mut bytes = Vec::new();
        value.encode(&mut bytes);
        let mut data = bytes.as_slice();
        assert_eq!(T::decode(&mut data).unwrap(), value);
        assert!(data.is_empty());
    }
    fn signature(seed: u8) -> MessageSignature {
        MessageSignature([seed; 256])
    }
    fn profile_key() -> ProfileKeyData {
        ProfileKeyData {
            expires_at: 1_700_000_000_000,
            key: vec![1, 2, 3],
            key_signature: vec![4, 5],
        }
    }
    fn session() -> RemoteChatSessionData {
        RemoteChatSessionData {
            session_id: Uuid::from_u128(7),
            profile_key: profile_key(),
        }
    }
    fn chat_type() -> ChatTypeBound {
        ChatTypeBound {
            chat_type: VarInt(0),
            name: crate::types::PlainText("Steve".into()),
            target_name: None,
        }
    }
    #[test]
    fn chat_packets_roundtrip() {
        roundtrip(ServerboundChat {
            message: "hello".into(),
            timestamp: 1_700_000_000_123,
            salt: 42,
            signature: Some(signature(3)),
            last_seen: LastSeenUpdate::new(2, &[0, 4], 9),
        });
        roundtrip(ServerboundChatAck { offset: VarInt(3) });
        roundtrip(ServerboundChatCommand {
            command: "gamemode creative".into(),
        });
        roundtrip(ServerboundChatCommandSigned {
            command: "say hi".into(),
            timestamp: 1,
            salt: 2,
            argument_signatures: vec![ArgumentSignature {
                name: "message".into(),
                signature: signature(4),
            }],
            last_seen: LastSeenUpdate::new(0, &[], 1),
        });
        roundtrip(ServerboundChatSessionUpdate {
            chat_session: session(),
        });
        roundtrip(PlayerInfoChatSession {
            uuid: Uuid::from_u128(9),
            chat_session: Some(session()),
        });
        roundtrip(PlayerInfoChatSession {
            uuid: Uuid::from_u128(9),
            chat_session: None,
        });
        roundtrip(ClientboundPlayerChat {
            global_index: VarInt(5),
            sender: Uuid::from_u128(9),
            index: VarInt(2),
            signature: Some(signature(8)),
            body: SignedMessageBody {
                content: "hello".into(),
                timestamp: 1_700_000_000_123,
                salt: 42,
                last_seen: vec![signature(1), signature(2)],
            },
            unsigned_content: None,
            filter_mask: VarInt(0),
            chat_type: chat_type(),
        });
        roundtrip(ClientboundDisguisedChat {
            message: crate::types::PlainText("hi".into()),
            chat_type: chat_type(),
        });
    }
    #[test]
    fn chat_type_uses_holder_id_plus_one() {
        let mut bytes = Vec::new();
        chat_type().encode(&mut bytes);
        assert_eq!(bytes[0], 1);
    }
    #[test]
    fn last_seen_bitset_is_three_bytes() {
        let update = LastSeenUpdate::new(0, &[0, 7, 19], 0);
        assert!(update.is_acknowledged(0));
        assert!(update.is_acknowledged(7));
        assert!(update.is_acknowledged(19));
        assert!(!update.is_acknowledged(1));
        let mut bytes = Vec::new();
        LastSeenUpdate::new(0, &[], 0).encode(&mut bytes);
        assert_eq!(bytes, [0, 0, 0, 0, 0]);
    }
    #[test]
    fn rejects_oversized_profile_key() {
        let mut bytes = Vec::new();
        Uuid::nil().encode(&mut bytes);
        0i64.encode(&mut bytes);
        VarInt(513).encode(&mut bytes);
        bytes.extend(std::iter::repeat_n(0u8, 513));
        assert!(RemoteChatSessionData::decode(&mut bytes.as_slice()).is_err());
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: T) {
        let mut bytes = Vec::new();
        value.encode(&mut bytes);
        let mut data = bytes.as_slice();
        assert_eq!(T::decode(&mut data).unwrap(), value);
        assert!(data.is_empty());
    }
    #[test]
    fn camera_dialog_and_post_effect_packets_roundtrip_with_vanilla_ids() {
        assert_eq!(
            (
                SetCamera::ID,
                PostEffects::ID,
                ClearDialog::ID,
                ShowDialog::ID
            ),
            (95, 83, 142, 143)
        );
        roundtrip(SetCamera {
            entity: VarInt(300),
        });
        roundtrip(PostEffects {
            effects: vec!["minecraft:blur".into(), "pack:glow".into()],
        });
        roundtrip(PostEffects { effects: vec![] });
        roundtrip(ClearDialog);
        roundtrip(ShowDialog { holder: VarInt(4) });
        let mut dialog = ferrite_nbt::Compound::new();
        dialog.insert(
            "type".into(),
            ferrite_nbt::Tag::String("minecraft:notice".into()),
        );
        dialog.insert("title".into(), ferrite_nbt::Tag::String("Hi".into()));
        roundtrip(ShowDialogInline { dialog });
        let mut bytes = Vec::new();
        ClearDialog.encode(&mut bytes);
        assert!(bytes.is_empty());
    }

    #[test]
    fn tracked_waypoint_matches_the_bytes_the_vanilla_codec_writes() {
        let hex = |s: &str| -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        };
        let uuid = "0102030405060708090a0b0c0d0e0f10";
        let style = "116d696e6563726166743a64656661756c74";
        let uuid = Uuid::decode(&mut hex(uuid).as_slice()).unwrap();
        let cases = [
            (
                0,
                None,
                WaypointData::Position([1, -2, 300]),
                format!(
                    "0001{}{style}000101feffffff0fac02",
                    "0102030405060708090a0b0c0d0e0f10"
                ),
            ),
            (
                2,
                Some(0x123456),
                WaypointData::Position([1, -2, 300]),
                format!(
                    "0201{}{style}011234560101feffffff0fac02",
                    "0102030405060708090a0b0c0d0e0f10"
                ),
            ),
            (
                0,
                None,
                WaypointData::Chunk([5, -7]),
                format!(
                    "0001{}{style}000205f9ffffff0f",
                    "0102030405060708090a0b0c0d0e0f10"
                ),
            ),
            (
                0,
                None,
                WaypointData::Azimuth(1.5),
                format!(
                    "0001{}{style}00033fc00000",
                    "0102030405060708090a0b0c0d0e0f10"
                ),
            ),
            (
                1,
                None,
                WaypointData::Empty,
                format!("0101{}{style}0000", "0102030405060708090a0b0c0d0e0f10"),
            ),
        ];
        for (op, color, data, bytes) in cases {
            let packet = TrackedWaypoint {
                operation: VarInt(op),
                uuid,
                style: "minecraft:default".into(),
                color,
                data,
            };
            let mut out = Vec::new();
            packet.encode(&mut out);
            assert_eq!(out, hex(&bytes));
            roundtrip(packet);
        }
        assert_eq!(TrackedWaypoint::ID, 141);
    }

    #[test]
    fn set_passengers_roundtrips_with_the_vanilla_id() {
        assert_eq!(SetPassengers::ID, 109);
        roundtrip(SetPassengers {
            vehicle: VarInt(12),
            passengers: vec![VarInt(3), VarInt(400)],
        });
        roundtrip(SetPassengers {
            vehicle: VarInt(12),
            passengers: vec![],
        });
    }

    #[test]
    fn plugin_packets_roundtrip_and_have_the_vanilla_ids() {
        assert_eq!(
            (
                CustomPayload::ID,
                Transfer::ID,
                ResourcePackPush::ID,
                ResourcePackPop::ID
            ),
            (24, 132, 82, 81)
        );
        roundtrip(CustomPayload {
            channel: "bungeecord:main".into(),
            data: crate::codec::RestBytes(vec![1, 2, 3]),
        });
        roundtrip(Transfer {
            host: "play.example.com".into(),
            port: VarInt(25566),
        });
        roundtrip(ResourcePackPush {
            id: Uuid::from_u128(7),
            url: "https://example.com/pack.zip".into(),
            hash: "ab".repeat(20),
            required: true,
            prompt: None,
        });
        roundtrip(CookieRequest {
            key: "my:key".into(),
        });
        roundtrip(CookieResponse {
            key: "my:key".into(),
            payload: Some(vec![1, 2, 3]),
        });
        roundtrip(CookieResponse {
            key: "my:key".into(),
            payload: None,
        });
        roundtrip(StoreCookie {
            key: "plugin:session".into(),
            payload: vec![1, 2, 3],
        });
        roundtrip(ResourcePackPop { id: None });
        roundtrip(ResourcePackPop {
            id: Some(Uuid::from_u128(9)),
        });
    }
    #[test]
    fn item_cooldown_roundtrips_and_has_the_vanilla_id() {
        assert_eq!(ItemCooldown::ID, 22);
        roundtrip(ItemCooldown {
            group: "minecraft:ender_pearl".into(),
            ticks: VarInt(300),
        });
    }
    #[test]
    fn container_packets_roundtrip() {
        let stone = ItemStack {
            count: 3,
            item_id: 1,
        };
        roundtrip(ContainerClick::<ItemStack> {
            window: VarInt(1),
            state: VarInt(7),
            slot: 36,
            button: 1,
            input: VarInt(5),
            changed: vec![(36, None), (9, Some(stone))],
            carried: Some(stone),
        });
        roundtrip(SetCursorItem { item: stone });
        roundtrip(SetPlayerInventory {
            slot: VarInt(40),
            item: stone,
        });
        roundtrip(ServerboundContainerClose { window: VarInt(3) });
        roundtrip(ClientboundContainerClose { window: VarInt(3) });
        roundtrip(SetHeldSlot { slot: VarInt(4) });
        roundtrip(OpenScreen {
            window: VarInt(1),
            menu: VarInt(12),
            title: crate::types::Translatable("container.crafting".into()),
        });
        roundtrip(SetEntityLink {
            attached: 4,
            holder: 9,
        });
        roundtrip(MountScreenOpen {
            window: VarInt(1),
            columns: VarInt(5),
            entity: 77,
        });
        roundtrip(ContainerSetData {
            window: VarInt(1),
            id: 4,
            value: 200,
        });
        roundtrip(ContainerButtonClick {
            window: VarInt(1),
            button: VarInt(7),
        });
        roundtrip(RenameItem {
            name: "Excalibur".into(),
        });
        roundtrip(SetBeacon {
            primary: Some(VarInt(1)),
            secondary: None,
        });
        roundtrip(SetBeacon {
            primary: Some(VarInt(1)),
            secondary: Some(VarInt(10)),
        });
        roundtrip(BlockEvent {
            pos: crate::types::BlockPos::new(4, 70, 4),
            block: VarInt(7),
            param_a: 1,
            param_b: 4,
        });
        // More than 128 changed slots is rejected like vanilla.
        let mut bytes = Vec::new();
        ContainerClick::<ItemStack> {
            window: VarInt(0),
            state: VarInt(0),
            slot: 0,
            button: 0,
            input: VarInt(0),
            changed: vec![(0, None); 129],
            carried: None,
        }
        .encode(&mut bytes);
        assert!(ContainerClick::<ItemStack>::decode(&mut bytes.as_slice()).is_err());
    }
    #[test]
    fn interaction_packets_roundtrip() {
        roundtrip(PlayerGameMode {
            uuid: Uuid::nil(),
            game_mode: VarInt(1),
        });
        roundtrip(PlayerAction {
            status: VarInt(0),
            location: 15,
            face: 1,
            sequence: VarInt(42),
        });
        roundtrip(UseItemOn {
            hand: VarInt(0),
            location: 15,
            face: VarInt(1),
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside: false,
            world_border_hit: false,
            sequence: VarInt(3),
        });
        roundtrip(CustomChatCompletions {
            action: VarInt(2),
            entries: vec!["alpha".into(), "".into()],
        });
        roundtrip(BlockDestruction {
            entity_id: VarInt(99),
            position: crate::types::BlockPos::new(1, 64, -2).pack(),
            progress: 5,
        });
        roundtrip(BlockUpdate {
            location: 15,
            block_state: VarInt(9),
        });
        assert_eq!(MapItemData::ID, 52);
        roundtrip(MapItemData {
            map_id: VarInt(3),
            scale: 2,
            locked: true,
            decorations: Some(vec![
                MapDecoration {
                    kind: VarInt(0),
                    x: -128,
                    y: 127,
                    direction: 15,
                    caption: Some(crate::types::PlainText("Alex".into())),
                },
                MapDecoration {
                    kind: VarInt(26),
                    x: 127,
                    y: -128,
                    direction: 4,
                    caption: None,
                },
            ]),
            width: 128,
            height: 128,
            x: 0,
            z: 0,
            colors: vec![4; 128 * 128],
        });
        roundtrip(MapItemData {
            map_id: VarInt(4),
            scale: 0,
            locked: false,
            decorations: Some(Vec::new()),
            width: 2,
            height: 1,
            x: 5,
            z: 6,
            colors: vec![44, 45],
        });
        let mut encoded = Vec::new();
        MapItemData {
            map_id: VarInt(3),
            scale: 2,
            locked: true,
            decorations: Some(vec![MapDecoration {
                kind: VarInt(0),
                x: -128,
                y: 127,
                direction: 15,
                caption: None,
            }]),
            width: 1,
            height: 1,
            x: 5,
            z: 6,
            colors: vec![44],
        }
        .encode(&mut encoded);
        assert_eq!(
            encoded,
            vec![3, 2, 1, 1, 1, 0, 128, 127, 15, 0, 1, 1, 5, 6, 1, 44]
        );
        assert_eq!(OpenBook::ID, 59);
        roundtrip(OpenBook { hand: VarInt(1) });
        roundtrip(AcknowledgeBlockChange {
            sequence: VarInt(3),
        });
        roundtrip(SetHeldItem { slot: 2 });
        roundtrip(ServerboundAttack {
            entity_id: VarInt(9),
        });
        roundtrip(ServerboundInteract {
            entity_id: VarInt(9),
            hand: VarInt(0),
            x: 0.25,
            y: 1.5,
            z: -0.5,
            using_secondary_action: true,
        });
        roundtrip(PlaceRecipe {
            window: VarInt(3),
            display_id: VarInt(412),
            use_max: true,
        });
        roundtrip(HurtAnimation {
            id: VarInt(9),
            yaw: 45.0,
        });
        roundtrip(Animate {
            id: VarInt(9),
            action: 4,
        });
        roundtrip(EntityEvent {
            entity_id: 77,
            status: 26,
        });
        roundtrip(LevelEvent {
            event: 1000,
            location: 123,
            data: 0,
            global: false,
        });
        roundtrip(CreativeSlot {
            slot: 36,
            item: ItemStack {
                count: 3,
                item_id: 7,
            },
        });
        roundtrip(ContainerContent {
            window: VarInt(0),
            state: VarInt(0),
            slots: vec![
                ItemStack::default(),
                ItemStack {
                    count: 1,
                    item_id: 2,
                },
            ],
            carried: ItemStack::default(),
        });
        roundtrip(ContainerSlot {
            window: VarInt(0),
            state: VarInt(3),
            slot: 36,
            item: ItemStack::default(),
        });
        roundtrip(SetExperience {
            progress: 0.5,
            level: VarInt(12),
            total: VarInt(456),
        });
        roundtrip(PlayerAbilities {
            flags: 0b0110,
            flying_speed: 0.05,
            walking_speed: 0.1,
        });
        roundtrip(SetHealth {
            health: 17.5,
            food: VarInt(14),
            saturation: 3.25,
        });
        roundtrip(PlayerCombatKill {
            player_id: VarInt(5),
            message: crate::types::PlainText("Steve fell from a high place".into()),
        });
        roundtrip(Respawn {
            dimension_type: VarInt(0),
            dimension: "minecraft:overworld".into(),
            seed: 42,
            game_mode: VarInt(0),
            previous_game_mode: VarInt(0),
            debug: false,
            flat: true,
            death_location: Some(DeathLocation {
                dimension: "minecraft:overworld".into(),
                position: 15,
            }),
            portal_cooldown: VarInt(0),
            sea_level: VarInt(63),
            data_to_keep: 3,
        });
        roundtrip(ServerboundClientCommand { action: VarInt(0) });
        roundtrip(ServerboundUseItem {
            hand: VarInt(0),
            sequence: VarInt(9),
            yaw: 90.0,
            pitch: 10.0,
        });
        roundtrip(ServerboundPlayerCommand {
            entity_id: VarInt(5),
            action: VarInt(3),
            data: VarInt(0),
        });
        roundtrip(PlayerInput { flags: 0b0110_0001 });
        assert_eq!(ServerboundMoveVehicle::ID, 34);
        roundtrip(ServerboundMoveVehicle {
            x: 1.5,
            y: 64.0,
            z: -3.25,
            yaw: 90.0,
            pitch: 5.0,
            on_ground: true,
        });
        roundtrip(UpdateAttributes {
            entity_id: VarInt(3),
            attributes: vec![AttributeSnapshot {
                attribute: VarInt(23),
                base: 40.0,
                modifiers: vec![AttributeModifier {
                    id: "minecraft:effect.health_boost".into(),
                    amount: 8.0,
                    operation: VarInt(0),
                }],
            }],
        });
        roundtrip(ResourcePackResponse {
            id: Uuid::from_u128(5),
            action: VarInt(3),
        });
    }
    #[test]
    fn entity_packets_roundtrip() {
        roundtrip(AddEntity {
            id: VarInt(5),
            uuid: Uuid::from_u128(7),
            entity_type: VarInt(71),
            x: 0.5,
            y: 64.0,
            z: -3.25,
            velocity: LpVec3::ZERO,
            pitch: 0,
            yaw: 200,
            head_yaw: 200,
            data: VarInt(0),
        });
        roundtrip(SetEntityMotion {
            id: VarInt(5),
            velocity: LpVec3::ZERO,
        });
        roundtrip(TakeItemEntity {
            item: VarInt(5),
            collector: VarInt(1),
            amount: VarInt(3),
        });
        roundtrip(TeleportEntity {
            id: VarInt(5),
            x: 1.0,
            y: 2.0,
            z: 3.0,
            velocity_x: 0.0,
            velocity_y: -0.1,
            velocity_z: 0.0,
            yaw: 90.0,
            pitch: 0.0,
            relatives: 0,
            on_ground: true,
        });
        let values = [
            (0, MetaValue::Byte(0x20)),
            (1, MetaValue::Int(300)),
            (
                8,
                MetaValue::ItemStack(ItemStack {
                    count: 2,
                    item_id: 28,
                }),
            ),
            (6, MetaValue::Pose(1)),
            (9, MetaValue::Long(-5)),
            (10, MetaValue::String("x".into())),
            (11, MetaValue::Boolean(true)),
            (12, MetaValue::Float(0.5)),
        ];
        roundtrip(SetEntityData {
            id: VarInt(5),
            values: values
                .into_iter()
                .map(|(index, value)| MetaEntry { index, value })
                .collect(),
        });
        // The villager's synched data: index 19, serializer 18 (`VILLAGER_DATA`), then the
        // villager type, profession and level as registry ids.
        let mut bytes = Vec::new();
        MetaEntry {
            index: 19,
            value: MetaValue::VillagerData {
                kind: 2,
                profession: 5,
                level: 1,
            },
        }
        .encode(&mut bytes);
        assert_eq!(bytes[..3], [19, 18, 2]);
        assert_eq!(
            MetaEntry::decode_at(19, &mut &bytes[1..]).unwrap().value,
            MetaValue::VillagerData {
                kind: 2,
                profession: 5,
                level: 1
            }
        );
        // A wolf's owner: serializer 13, a presence flag and the UUID.
        for owner in [None, Some(uuid::Uuid::from_u128(5))] {
            let mut bytes = Vec::new();
            MetaEntry {
                index: 19,
                value: MetaValue::OptionalUuid(owner),
            }
            .encode(&mut bytes);
            assert_eq!(bytes[..2], [19, 13]);
            assert_eq!(
                MetaEntry::decode_at(19, &mut &bytes[1..]).unwrap().value,
                MetaValue::OptionalUuid(owner)
            );
        }
        // An End crystal's beam target: serializer 11, a presence flag and the packed position.
        for target in [None, Some(crate::types::BlockPos::new(0, 128, 0).pack())] {
            let mut bytes = Vec::new();
            MetaEntry {
                index: 8,
                value: MetaValue::OptionalBlockPos(target),
            }
            .encode(&mut bytes);
            assert_eq!(bytes[..2], [8, 11]);
            assert_eq!(
                MetaEntry::decode_at(8, &mut &bytes[1..]).unwrap().value,
                MetaValue::OptionalBlockPos(target)
            );
        }
        // A cloud's particle: serializer 16, the type id, and an ARGB int for entity_effect.
        for (id, argb) in [(28, Some(0x80FF_0000_u32 as i32)), (15, None)] {
            let mut bytes = Vec::new();
            MetaEntry {
                index: 10,
                value: MetaValue::Particle { id, argb },
            }
            .encode(&mut bytes);
            assert_eq!(bytes[..3], [10, 16, id as u8]);
            assert_eq!(bytes.len(), 3 + if argb.is_some() { 4 } else { 0 });
            assert_eq!(
                MetaEntry::decode_at(10, &mut &bytes[1..]).unwrap().value,
                MetaValue::Particle { id, argb }
            );
        }
        // A non-zero velocity survives within LpVec3 precision.
        let mut bytes = Vec::new();
        SetEntityMotion {
            id: VarInt(1),
            velocity: LpVec3([0.1, 0.2, -0.05]),
        }
        .encode(&mut bytes);
        let back = SetEntityMotion::decode(&mut bytes.as_slice()).unwrap();
        assert!((back.velocity.0[1] - 0.2).abs() < 1e-4);
    }

    #[test]
    fn merchant_packets_roundtrip() {
        roundtrip(SelectTrade { slot: VarInt(2) });
        roundtrip(MerchantOffers {
            window: VarInt(4),
            offers: vec![
                MerchantOffer {
                    wants: ItemCost {
                        item: VarInt(1),
                        count: VarInt(12),
                    },
                    gives: ItemStack {
                        count: 6,
                        item_id: 32,
                    },
                    second_wants: None,
                    out_of_stock: false,
                    uses: 3,
                    max_uses: 16,
                    xp: 2,
                    special_price: -1,
                    price_multiplier: 0.05,
                    demand: 4,
                },
                MerchantOffer {
                    wants: ItemCost {
                        item: VarInt(3),
                        count: VarInt(1),
                    },
                    gives: ItemStack {
                        count: 1,
                        item_id: 7,
                    },
                    second_wants: Some(ItemCost {
                        item: VarInt(9),
                        count: VarInt(2),
                    }),
                    out_of_stock: true,
                    uses: 12,
                    max_uses: 12,
                    xp: 5,
                    special_price: 0,
                    price_multiplier: 0.2,
                    demand: 0,
                },
            ],
            level: VarInt(2),
            xp: VarInt(30),
            show_progress: true,
            can_restock: false,
        });
    }

    #[test]
    fn merchant_offers_reject_an_oversized_list() {
        let mut bytes = Vec::new();
        VarInt(1).encode(&mut bytes);
        VarInt(MAX_MERCHANT_OFFERS as i32 + 1).encode(&mut bytes);
        assert!(MerchantOffers::<ItemStack>::decode(&mut bytes.as_slice()).is_err());
    }
}

// ---------------------------------------------------------------------------------------------
// Command feedback packets (26.3 protocol ids from `reports/packets.json`).
// ---------------------------------------------------------------------------------------------

packet!(UpdateMobEffect,135,Clientbound,{entity_id:VarInt,effect:VarInt,amplifier:VarInt,duration:VarInt,flags:u8});
packet!(RemoveMobEffect,79,Clientbound,{entity_id:VarInt,effect:VarInt});
packet!(SetTitleText,116,Clientbound,{text:PlainText});
packet!(SetSubtitleText,114,Clientbound,{text:PlainText});
packet!(SetActionBarText,89,Clientbound,{text:PlainText});
packet!(SetTitlesAnimation,117,Clientbound,{fade_in:i32,stay:i32,fade_out:i32});
packet!(ClearTitles,14,Clientbound,{reset:bool});
// `minecraft:tab_list`: the player list header and footer; an empty text hides either.
packet!(TabList,125,Clientbound,{header:PlainText,footer:PlainText});
// `minecraft:sound`: a registry holder, the source category, fixed-point position, volume,
// pitch and the random seed used to pick a sound variant.
packet!(SoundEffect,119,Clientbound,{sound:VarInt,source:VarInt,x:i32,y:i32,z:i32,volume:f32,pitch:f32,seed:i64});
// `minecraft:level_particles` for particle options without extra type data.
packet!(LevelParticles,48,Clientbound,{particle:VarInt,long_distance:bool,always_show:bool,x:f64,y:f64,z:f64,velocity_x:f32,velocity_y:f32,velocity_z:f32,speed:f32,count:i32});
packet!(InitializeBorder,44,Clientbound,{x:f64,z:f64,old_diameter:f64,new_diameter:f64,speed:VarLong,portal_teleport_boundary:VarInt,warning_blocks:VarInt,warning_time:VarInt});
packet!(SetBorderCenter,90,Clientbound,{x:f64,z:f64});
packet!(SetBorderLerpSize,91,Clientbound,{old_diameter:f64,new_diameter:f64,speed:VarLong});
packet!(SetBorderSize,92,Clientbound,{diameter:f64});
packet!(SetBorderWarningDelay,93,Clientbound,{warning_delay:VarInt});
packet!(SetBorderWarningDistance,94,Clientbound,{warning_distance:VarInt});
packet!(SetDisplayObjective,100,Clientbound,{slot:VarInt,objective:String});

/// `minecraft:stop_sound`: a flags byte says whether a source category and/or a sound id
/// follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopSound {
    pub source: Option<VarInt>,
    pub sound: Option<String>,
}

impl Encode for StopSound {
    fn encode(&self, out: &mut Vec<u8>) {
        let mut flags = 0u8;
        if self.source.is_some() {
            flags |= 1;
        }
        if self.sound.is_some() {
            flags |= 2;
        }
        flags.encode(out);
        if let Some(source) = self.source {
            source.encode(out);
        }
        if let Some(sound) = &self.sound {
            sound.encode(out);
        }
    }
}

impl Decode for StopSound {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let flags = u8::decode(buf)?;
        if flags & !3 != 0 {
            return Err(DecodeError::Invalid("stop sound flags"));
        }
        Ok(Self {
            source: (flags & 1 != 0).then(|| VarInt::decode(buf)).transpose()?,
            sound: (flags & 2 != 0).then(|| String::decode(buf)).transpose()?,
        })
    }
}

impl Packet for StopSound {
    const ID: i32 = 121;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// `minecraft:set_objective`: add (0), remove (1) or change (2). Add and change carry the
/// display component, render type and an optional number format; Ferrite always writes the
/// "no number format" marker.
#[derive(Debug, Clone, PartialEq)]
pub struct SetObjective {
    pub name: String,
    pub mode: u8,
    pub display_name: Option<PlainText>,
    pub render_type: Option<VarInt>,
}

impl Encode for SetObjective {
    fn encode(&self, out: &mut Vec<u8>) {
        self.name.encode(out);
        self.mode.encode(out);
        if self.mode != 1 {
            self.display_name
                .clone()
                .unwrap_or_else(|| PlainText(String::new()))
                .encode(out);
            self.render_type.unwrap_or(VarInt(0)).encode(out);
            false.encode(out);
        }
    }
}

impl Decode for SetObjective {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let name = String::decode(buf)?;
        let mode = u8::decode(buf)?;
        if mode == 1 {
            return Ok(Self {
                name,
                mode,
                display_name: None,
                render_type: None,
            });
        }
        let display_name = Some(PlainText::decode(buf)?);
        let render_type = Some(VarInt::decode(buf)?);
        if bool::decode(buf)? {
            return Err(DecodeError::Invalid("objective number format unsupported"));
        }
        Ok(Self {
            name,
            mode,
            display_name,
            render_type,
        })
    }
}

impl Packet for SetObjective {
    const ID: i32 = 108;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// `minecraft:set_score`: the owner, objective and value, with optional display name and number
/// format (never written by Ferrite).
#[derive(Debug, Clone, PartialEq)]
pub struct SetScore {
    pub entity: String,
    pub objective: String,
    pub value: VarInt,
    pub display_name: Option<PlainText>,
}

impl Encode for SetScore {
    fn encode(&self, out: &mut Vec<u8>) {
        self.entity.encode(out);
        self.objective.encode(out);
        self.value.encode(out);
        self.display_name.encode(out);
        false.encode(out);
    }
}

impl Decode for SetScore {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let entity = String::decode(buf)?;
        let objective = String::decode(buf)?;
        let value = VarInt::decode(buf)?;
        let display_name = Option::<PlainText>::decode(buf)?;
        if bool::decode(buf)? {
            return Err(DecodeError::Invalid("score number format unsupported"));
        }
        Ok(Self {
            entity,
            objective,
            value,
            display_name,
        })
    }
}

impl Packet for SetScore {
    const ID: i32 = 112;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

// `minecraft:reset_score`: an owner and an optional objective (`None` removes all scores).
fields!(ResetScore {
    entity: String,
    objective: Option<String>
});

impl Packet for ResetScore {
    const ID: i32 = 80;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// `minecraft:boss_event`: 26.3 merged every boss bar action into one packet.
#[derive(Debug, Clone, PartialEq)]
pub enum BossBarAction {
    Add {
        name: PlainText,
        progress: f32,
        color: VarInt,
        overlay: u8,
        flags: u8,
    },
    Remove,
    UpdateProgress(f32),
    UpdateName(PlainText),
    UpdateStyle {
        color: VarInt,
        overlay: u8,
    },
    UpdateProperties(u8),
}

impl BossBarAction {
    fn operation(&self) -> VarInt {
        VarInt(match self {
            Self::Add { .. } => 0,
            Self::Remove => 1,
            Self::UpdateProgress(_) => 2,
            Self::UpdateName(_) => 3,
            Self::UpdateStyle { .. } => 4,
            Self::UpdateProperties(_) => 5,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BossBar {
    pub uuid: Uuid,
    pub action: BossBarAction,
}

impl Encode for BossBar {
    fn encode(&self, out: &mut Vec<u8>) {
        self.uuid.encode(out);
        self.action.operation().encode(out);
        match &self.action {
            BossBarAction::Add {
                name,
                progress,
                color,
                overlay,
                flags,
            } => {
                name.encode(out);
                progress.encode(out);
                color.encode(out);
                overlay.encode(out);
                flags.encode(out);
            }
            BossBarAction::Remove => {}
            BossBarAction::UpdateProgress(progress) => progress.encode(out),
            BossBarAction::UpdateName(name) => name.encode(out),
            BossBarAction::UpdateStyle { color, overlay } => {
                color.encode(out);
                overlay.encode(out);
            }
            BossBarAction::UpdateProperties(flags) => flags.encode(out),
        }
    }
}

impl Decode for BossBar {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let uuid = Uuid::decode(buf)?;
        let operation = VarInt::decode(buf)?.0;
        let action = match operation {
            0 => BossBarAction::Add {
                name: PlainText::decode(buf)?,
                progress: f32::decode(buf)?,
                color: VarInt::decode(buf)?,
                overlay: u8::decode(buf)?,
                flags: u8::decode(buf)?,
            },
            1 => BossBarAction::Remove,
            2 => BossBarAction::UpdateProgress(f32::decode(buf)?),
            3 => BossBarAction::UpdateName(PlainText::decode(buf)?),
            4 => BossBarAction::UpdateStyle {
                color: VarInt::decode(buf)?,
                overlay: u8::decode(buf)?,
            },
            5 => BossBarAction::UpdateProperties(u8::decode(buf)?),
            _ => return Err(DecodeError::Invalid("unknown boss bar operation")),
        };
        Ok(Self { uuid, action })
    }
}

impl Packet for BossBar {
    const ID: i32 = 9;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// The settings of a team in `minecraft:set_player_team` (read from the 26.3 server's
/// `ClientboundSetPlayerTeamPacket.Parameters` codec). Visibility and collision rule are ids
/// (always 0, never 1, hide-for-other-teams / push-other-teams 2, own-team 3); the color is a
/// team color id 0..=15 or absent; `options` has bit 0 friendly fire and bit 1 see invisibles.
#[derive(Debug, Clone, PartialEq)]
pub struct TeamParams {
    pub display_name: PlainText,
    pub prefix: PlainText,
    pub suffix: PlainText,
    pub nametag_visibility: VarInt,
    pub collision_rule: VarInt,
    pub color: Option<VarInt>,
    pub options: u8,
}

impl Encode for TeamParams {
    fn encode(&self, out: &mut Vec<u8>) {
        self.display_name.encode(out);
        self.prefix.encode(out);
        self.suffix.encode(out);
        self.nametag_visibility.encode(out);
        self.collision_rule.encode(out);
        self.color.encode(out);
        self.options.encode(out);
    }
}

impl Decode for TeamParams {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            display_name: PlainText::decode(buf)?,
            prefix: PlainText::decode(buf)?,
            suffix: PlainText::decode(buf)?,
            nametag_visibility: VarInt::decode(buf)?,
            collision_rule: VarInt::decode(buf)?,
            color: Option::decode(buf)?,
            options: u8::decode(buf)?,
        })
    }
}

/// `minecraft:set_player_team`: method 0 adds a team (with its settings and members), 1 removes
/// it, 2 changes its settings, 3 adds members and 4 removes them.
#[derive(Debug, Clone, PartialEq)]
pub enum TeamAction {
    Add {
        params: TeamParams,
        players: Vec<String>,
    },
    Remove,
    Change(TeamParams),
    Join(Vec<String>),
    Leave(Vec<String>),
}

impl TeamAction {
    fn method(&self) -> u8 {
        match self {
            Self::Add { .. } => 0,
            Self::Remove => 1,
            Self::Change(_) => 2,
            Self::Join(_) => 3,
            Self::Leave(_) => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerTeam {
    pub name: String,
    pub action: TeamAction,
}

impl Encode for PlayerTeam {
    fn encode(&self, out: &mut Vec<u8>) {
        self.name.encode(out);
        self.action.method().encode(out);
        match &self.action {
            TeamAction::Add { params, players } => {
                params.encode(out);
                players.encode(out);
            }
            TeamAction::Remove => {}
            TeamAction::Change(params) => params.encode(out),
            TeamAction::Join(players) | TeamAction::Leave(players) => players.encode(out),
        }
    }
}

impl Decode for PlayerTeam {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let name = String::decode(buf)?;
        let method = u8::decode(buf)?;
        let action = match method {
            0 => TeamAction::Add {
                params: TeamParams::decode(buf)?,
                players: Vec::decode(buf)?,
            },
            1 => TeamAction::Remove,
            2 => TeamAction::Change(TeamParams::decode(buf)?),
            3 => TeamAction::Join(Vec::decode(buf)?),
            4 => TeamAction::Leave(Vec::decode(buf)?),
            _ => return Err(DecodeError::Invalid("unknown team method")),
        };
        Ok(Self { name, action })
    }
}

impl Packet for PlayerTeam {
    const ID: i32 = 111;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Clientbound;
}

/// Vanilla `ServerboundSignUpdatePacket`: the four lines of one side of a sign.
#[derive(Debug, Clone, PartialEq)]
pub struct SignUpdate {
    pub location: i64,
    pub is_front_text: bool,
    pub lines: [String; 4],
}
impl Encode for SignUpdate {
    fn encode(&self, out: &mut Vec<u8>) {
        self.location.encode(out);
        self.is_front_text.encode(out);
        for line in &self.lines {
            line.encode(out);
        }
    }
}
impl Decode for SignUpdate {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        Ok(Self {
            location: i64::decode(buf)?,
            is_front_text: bool::decode(buf)?,
            lines: [
                String::decode(buf)?,
                String::decode(buf)?,
                String::decode(buf)?,
                String::decode(buf)?,
            ],
        })
    }
}
impl Packet for SignUpdate {
    const ID: i32 = 62;
    const STATE: State = State::Play;
    const BOUND: Bound = Bound::Serverbound;
}

// Vanilla `ClientboundOpenSignEditorPacket`.
packet!(OpenSignEditor,61,Clientbound,{location:i64,is_front_text:bool});

#[cfg(test)]
mod sign_packet_tests {
    use super::*;
    use crate::types::BlockPos;

    fn rt<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: T) {
        let mut bytes = Vec::new();
        value.encode(&mut bytes);
        let mut input = bytes.as_slice();
        assert_eq!(T::decode(&mut input).unwrap(), value);
        assert!(input.is_empty());
    }

    #[test]
    fn player_command_sign_update_and_editor_roundtrip() {
        rt(SignUpdate {
            location: i64::from(BlockPos::new(1, 2, 3).pack() as i32),
            is_front_text: true,
            lines: ["one".into(), "two".into(), "".into(), "four".into()],
        });
        rt(OpenSignEditor {
            location: i64::from(BlockPos::new(1, 2, 3).pack() as i32),
            is_front_text: false,
        });
    }
}

#[cfg(test)]
mod command_feedback_tests {
    use super::*;
    use crate::{Decode, Encode};

    fn rt<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: T) {
        let mut bytes = Vec::new();
        value.encode(&mut bytes);
        let mut input = bytes.as_slice();
        assert_eq!(T::decode(&mut input).unwrap(), value);
        assert!(input.is_empty());
    }

    #[test]
    fn equipment_packet_marks_all_but_the_last_slot() {
        let item = ItemStack::default();
        let packet = SetEquipment {
            entity_id: VarInt(5),
            slots: vec![(5, item), (0, item)],
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        assert_eq!(bytes[..2], [5, 0x85]);
        rt(packet);
    }

    #[test]
    fn server_links_roundtrip_known_and_custom_entries() {
        rt(ServerLinks {
            links: vec![
                ServerLinkEntry {
                    known: Some(6),
                    label: String::new(),
                    url: "https://example.org".into(),
                },
                ServerLinkEntry {
                    known: None,
                    label: "Wiki".into(),
                    url: "https://wiki.example.org".into(),
                },
            ],
        });
    }

    #[test]
    fn effect_and_title_packets_roundtrip() {
        rt(UpdateMobEffect {
            entity_id: VarInt(7),
            effect: VarInt(1),
            amplifier: VarInt(2),
            duration: VarInt(100),
            flags: 0b110,
        });
        rt(RemoveMobEffect {
            entity_id: VarInt(7),
            effect: VarInt(1),
        });
        rt(SetTitleText {
            text: PlainText("Hello".into()),
        });
        rt(SetSubtitleText {
            text: PlainText("Sub".into()),
        });
        rt(SetActionBarText {
            text: PlainText("Bar".into()),
        });
        rt(SetTitlesAnimation {
            fade_in: 10,
            stay: 70,
            fade_out: 20,
        });
        rt(ClearTitles { reset: true });
        rt(TabList {
            header: PlainText("Head".into()),
            footer: PlainText(String::new()),
        });
    }

    #[test]
    fn sound_and_particle_packets_roundtrip() {
        rt(SoundEffect {
            sound: VarInt(4),
            source: VarInt(0),
            x: 8,
            y: 64,
            z: -8,
            volume: 1.0,
            pitch: 1.0,
            seed: 42,
        });
        rt(StopSound {
            source: Some(VarInt(0)),
            sound: None,
        });
        rt(StopSound {
            source: None,
            sound: Some("minecraft:block.note_block.harp".into()),
        });
        rt(StopSound {
            source: None,
            sound: None,
        });
        rt(LevelParticles {
            particle: VarInt(9),
            long_distance: false,
            always_show: false,
            x: 0.5,
            y: 64.0,
            z: 0.5,
            velocity_x: 0.0,
            velocity_y: 0.1,
            velocity_z: 0.0,
            speed: 0.2,
            count: 12,
        });
    }

    #[test]
    fn world_border_packets_roundtrip() {
        rt(InitializeBorder {
            x: 0.0,
            z: 0.0,
            old_diameter: 100.0,
            new_diameter: 120.0,
            speed: VarLong(20),
            portal_teleport_boundary: VarInt(29_999_984),
            warning_blocks: VarInt(5),
            warning_time: VarInt(15),
        });
        rt(SetBorderCenter { x: 1.0, z: -2.0 });
        rt(SetBorderLerpSize {
            old_diameter: 10.0,
            new_diameter: 20.0,
            speed: VarLong(2),
        });
        rt(SetBorderSize { diameter: 42.0 });
        rt(SetBorderWarningDelay {
            warning_delay: VarInt(15),
        });
        rt(SetBorderWarningDistance {
            warning_distance: VarInt(5),
        });
    }

    #[test]
    fn scoreboard_packets_roundtrip() {
        rt(SetObjective {
            name: "objective".into(),
            mode: 0,
            display_name: Some(PlainText("Objective".into())),
            render_type: Some(VarInt(0)),
        });
        rt(SetObjective {
            name: "objective".into(),
            mode: 1,
            display_name: None,
            render_type: None,
        });
        rt(SetScore {
            entity: "Alice".into(),
            objective: "objective".into(),
            value: VarInt(5),
            display_name: None,
        });
        rt(ResetScore {
            entity: "Alice".into(),
            objective: None,
        });
        rt(SetDisplayObjective {
            slot: VarInt(1),
            objective: "objective".into(),
        });
    }

    #[test]
    fn boss_bar_and_team_packets_roundtrip() {
        for action in [
            BossBarAction::Add {
                name: PlainText("Boss".into()),
                progress: 0.5,
                color: VarInt(1),
                overlay: 0,
                flags: 0b101,
            },
            BossBarAction::Remove,
            BossBarAction::UpdateProgress(0.75),
            BossBarAction::UpdateName(PlainText("Renamed".into())),
            BossBarAction::UpdateStyle {
                color: VarInt(2),
                overlay: 1,
            },
            BossBarAction::UpdateProperties(0),
        ] {
            rt(BossBar {
                uuid: Uuid::from_u128(9),
                action,
            });
        }
        let params = TeamParams {
            display_name: PlainText("Team".into()),
            prefix: PlainText("[".into()),
            suffix: PlainText("]".into()),
            nametag_visibility: VarInt(2),
            collision_rule: VarInt(3),
            color: Some(VarInt(10)),
            options: 3,
        };
        for action in [
            TeamAction::Add {
                params: params.clone(),
                players: vec!["Alice".into()],
            },
            TeamAction::Add {
                params: TeamParams {
                    color: None,
                    ..params.clone()
                },
                players: vec![],
            },
            TeamAction::Remove,
            TeamAction::Change(params),
            TeamAction::Join(vec!["Alice".into()]),
            TeamAction::Leave(vec!["Alice".into()]),
        ] {
            rt(PlayerTeam {
                name: "team".into(),
                action,
            });
        }
    }

    #[test]
    fn block_entity_packets_roundtrip() {
        let mut nbt = ferrite_nbt::Compound::new();
        nbt.insert(
            "id".into(),
            ferrite_nbt::Tag::String("minecraft:command_block".into()),
        );
        nbt.insert("Command".into(), ferrite_nbt::Tag::String("say hi".into()));
        nbt.insert("auto".into(), ferrite_nbt::Tag::Byte(1));
        nbt.insert("LastExecution".into(), ferrite_nbt::Tag::Long(-7));
        nbt.insert(
            "LastOutput".into(),
            ferrite_nbt::Tag::String("{\"text\":\"hi\"}".into()),
        );
        rt(BlockEntityData {
            location: crate::types::BlockPos::new(3, 64, -8).pack(),
            block_type: VarInt(23),
            nbt,
        });
        rt(SetCommandBlock {
            location: crate::types::BlockPos::new(3, 64, -8).pack(),
            command: "setblock ~ ~ ~ stone".into(),
            mode: VarInt(2),
            track_output: true,
            conditional: false,
            automatic: true,
        });
    }
}
