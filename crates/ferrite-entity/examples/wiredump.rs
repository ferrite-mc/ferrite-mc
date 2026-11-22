use ferrite_protocol::{Packet, VarInt, encode_packet, packets::play::*, types::LpVec3};
fn line<P: Packet>(name: &str, p: &P) {
    let mut out = Vec::new();
    encode_packet(p, &mut out);
    let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
    println!("play {name} {hex}");
}
fn main() {
    let item = ferrite_data::registries::id("minecraft:item", "minecraft:dirt").unwrap();
    for v in [
        [0.0, 0.0, 0.0],
        [0.05, 0.2, -0.07],
        [3.0, -2.5, 1.0],
        [0.0, 0.0, -12.3],
        [1000.0, 1.0, -0.5],
    ] {
        line(
            "add_entity",
            &AddEntity {
                id: VarInt(7),
                uuid: uuid::Uuid::from_u128(99),
                entity_type: VarInt(71),
                x: 0.5,
                y: 64.0,
                z: -3.0,
                velocity: LpVec3(v),
                pitch: 3,
                yaw: 200,
                head_yaw: 200,
                data: VarInt(0),
            },
        );
        line(
            "set_entity_motion",
            &SetEntityMotion {
                id: VarInt(7),
                velocity: LpVec3(v),
            },
        );
    }
    let values = vec![
        MetaEntry {
            index: 0,
            value: MetaValue::Byte(0x20),
        },
        MetaEntry {
            index: 1,
            value: MetaValue::Int(300),
        },
        MetaEntry {
            index: 4,
            value: MetaValue::Boolean(true),
        },
        MetaEntry {
            index: 6,
            value: MetaValue::Pose(1),
        },
        MetaEntry {
            index: 8,
            value: MetaValue::ItemStack(ItemStack {
                count: 3,
                item_id: item,
            }),
        },
        MetaEntry {
            index: 9,
            value: MetaValue::Float(0.5),
        },
        MetaEntry {
            index: 10,
            value: MetaValue::Long(-5),
        },
        MetaEntry {
            index: 11,
            value: MetaValue::String("hi".into()),
        },
    ];
    line(
        "set_entity_data",
        &SetEntityData {
            id: VarInt(7),
            values,
        },
    );
    line(
        "set_entity_data",
        &SetEntityData {
            id: VarInt(7),
            values: vec![],
        },
    );
    line(
        "take_item_entity",
        &TakeItemEntity {
            item: VarInt(7),
            collector: VarInt(1),
            amount: VarInt(3),
        },
    );
    line(
        "teleport_entity",
        &TeleportEntity {
            id: VarInt(7),
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
        },
    );
    line(
        "entity_position_sync",
        &EntityTeleport {
            id: VarInt(7),
            path_type: VarInt(0),
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 90.0,
            pitch: 0.0,
            on_ground: true,
        },
    );
    line(
        "move_entity_pos",
        &EntityMove {
            id: VarInt(7),
            properties: VarInt(1),
            dx: 2048,
            dy: -5,
            dz: 0,
        },
    );
    line(
        "move_entity_pos_rot",
        &EntityMoveRotation {
            id: VarInt(7),
            properties: VarInt(0),
            dx: 1,
            dy: 2,
            dz: 3,
            yaw: 4,
            pitch: 5,
        },
    );
    line(
        "move_entity_rot",
        &EntityRotation {
            id: VarInt(7),
            on_ground: true,
            yaw: 4,
            pitch: 5,
        },
    );
    line(
        "rotate_head",
        &EntityHeadRotation {
            id: VarInt(7),
            yaw: 4,
        },
    );
    line(
        "remove_entities",
        &RemoveEntities {
            ids: vec![VarInt(7), VarInt(8)],
        },
    );
    // Dump AddEntity + full SetEntityData for all 62 MobKind variants so Wirecheck verifies
    // every 26.3 SynchedEntityData index & serializer against Mojang's official codecs.
    let mut rng = ferrite_entity::Rng::new(77);
    for (idx, kind) in ferrite_entity::mob::MobKind::ALL_MOBS.iter().enumerate() {
        let eid = 100 + idx as i32;
        let record =
            ferrite_entity::mob::new_mob(*kind, ferrite_math::Vec3::new(0.5, 64.0, 0.5), &mut rng);
        line(
            "add_entity",
            &AddEntity {
                id: VarInt(eid),
                uuid: record.data.uuid,
                entity_type: VarInt(i32::from(record.data.kind)),
                x: 0.5,
                y: 64.0,
                z: 0.5,
                velocity: LpVec3([0.0, 0.0, 0.0]),
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
                data: VarInt(0),
            },
        );
        line(
            "set_entity_data",
            &SetEntityData {
                id: VarInt(eid),
                values: record.data.metadata.all_entries(),
            },
        );
    }
}
