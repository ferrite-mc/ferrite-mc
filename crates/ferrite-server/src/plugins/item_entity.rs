//! Bukkit's `Item` entity surface: the stack, pickup delay, owner and the lifetime flags.
//!
//! Reads are answered from the entity facts a region published this tick. Writes become
//! [`ferrite_plugin::EntityOperation`]s, which the region owning the item applies to the live
//! [`ferrite_entity::item::ItemState`].

use ferrite_plugin::{EntityOperation, HOST_ITEM_SEPARATOR, HOST_NULL, HostCall, PlayerCommand};
use uuid::Uuid;

/// The answer to an `Item` getter, or `None` when the entity is gone or the method is unknown.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "Item" || !call.args.is_empty() {
        return None;
    }
    let uuid = Uuid::parse_str(&call.subject).ok()?;
    let item = super::facts::fact(uuid)?.item?;
    Some(match call.method.as_str() {
        "getItemStack()" => format!("{}{HOST_ITEM_SEPARATOR}{}", item.material, item.amount),
        "getPickupDelay()" => item.pickup_delay.to_string(),
        "getOwner()" => item
            .owner
            .map_or_else(|| HOST_NULL.to_owned(), |owner| owner.to_string()),
        "getThrower()" => item
            .thrower
            .map_or_else(|| HOST_NULL.to_owned(), |thrower| thrower.to_string()),
        "isUnlimitedLifetime()" => item.unlimited_lifetime.to_string(),
        "canMobPickup()" => item.can_mob_pickup.to_string(),
        "canPlayerPickup()" => item.can_player_pickup.to_string(),
        "willAge()" => item.will_age.to_string(),
        "getHealth()" => item.health.to_string(),
        _ => return None,
    })
}

/// The operation an `Item` setter asks for.
pub fn command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Item" {
        return None;
    }
    let uuid = Uuid::parse_str(&call.subject).ok()?;
    let flag = || match call.args.first().map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let number = || call.args.first()?.parse::<i32>().ok();
    let reference = || match call.args.first().map(String::as_str) {
        Some(HOST_NULL) | None => Some(None),
        Some(text) => Uuid::parse_str(text).ok().map(Some),
    };
    let op = match call.method.as_str() {
        "setItemStack(ItemStack)" => {
            let (material, amount) = call.args.first()?.split_once(HOST_ITEM_SEPARATOR)?;
            let amount: u8 = amount.parse().ok().filter(|amount| *amount > 0)?;
            let material = material.to_ascii_lowercase();
            let material = if material.contains(':') {
                material
            } else {
                format!("minecraft:{material}")
            };
            ferrite_item::item_id(&material)?;
            EntityOperation::SetItem { material, amount }
        }
        "setPickupDelay(int)" => EntityOperation::SetPickupDelay(number()?),
        "setOwner(UUID)" => EntityOperation::SetOwner(reference()?),
        "setThrower(UUID)" => EntityOperation::SetThrower(reference()?),
        "setUnlimitedLifetime(boolean)" => EntityOperation::SetUnlimitedLifetime(flag()?),
        "setCanMobPickup(boolean)" => EntityOperation::SetCanMobPickup(flag()?),
        "setCanPlayerPickup(boolean)" => EntityOperation::SetCanPlayerPickup(flag()?),
        "setWillAge(boolean)" => EntityOperation::SetWillAge(flag()?),
        "setHealth(int)" => EntityOperation::SetItemHealth(number()?.max(0)),
        _ => return None,
    };
    Some(PlayerCommand::EntityOp { uuid, op })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::{EntityData, EntityRecord, EntityStore, Flags};
    use ferrite_math::Vec3;

    fn item_entity() -> (Uuid, EntityStore) {
        let uuid = Uuid::from_u128(0x17e5);
        let mut store = EntityStore::new(3);
        let stack = ferrite_protocol::packets::play::ItemStack {
            count: 4,
            item_id: i32::from(ferrite_item::item_id("minecraft:diamond").unwrap()),
        };
        let mut data = EntityData::new(
            ferrite_entity::allocate_id(),
            uuid,
            ferrite_entity::item::item_type(),
            0.25,
            0.25,
        );
        data.metadata.define(
            ferrite_entity::metadata::item::ITEM,
            ferrite_protocol::packets::play::MetaValue::ItemStack(Default::default()),
        );
        ferrite_entity::item::set_stack(&mut data, stack);
        data.item = Some(ferrite_entity::item::ItemState {
            pickup_delay: 40,
            target: Some(Uuid::from_u128(0xabcd)),
            unlimited_lifetime: true,
            ..Default::default()
        });
        store.insert(EntityRecord {
            pos: Vec3::new(1.0, 64.0, 2.0),
            vel: Vec3::ZERO,
            flags: Flags::default(),
            data,
        });
        (uuid, store)
    }

    fn ask(uuid: Uuid, method: &str) -> Option<String> {
        answer(&HostCall {
            owner: "Item".into(),
            subject: uuid.to_string(),
            method: method.into(),
            args: vec![],
        })
    }

    fn call(method: &str, args: &[&str]) -> Option<PlayerCommand> {
        command(&HostCall {
            owner: "Item".into(),
            subject: Uuid::from_u128(0x17e5).to_string(),
            method: method.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        })
    }

    #[test]
    fn item_queries_read_the_published_state() {
        let (uuid, store) = item_entity();
        let mut owned = std::collections::HashSet::new();
        super::super::facts::publish_entities(
            &mut owned,
            &store,
            crate::game::Dimension::Overworld,
        );
        assert_eq!(
            ask(uuid, "getItemStack()").as_deref(),
            Some("minecraft:diamond\u{1d}4")
        );
        assert_eq!(ask(uuid, "getPickupDelay()").as_deref(), Some("40"));
        assert_eq!(
            ask(uuid, "getOwner()").as_deref(),
            Some("00000000-0000-0000-0000-00000000abcd")
        );
        assert_eq!(ask(uuid, "isUnlimitedLifetime()").as_deref(), Some("true"));
        assert_eq!(ask(uuid, "canPlayerPickup()").as_deref(), Some("true"));
        assert_eq!(ask(uuid, "getHealth()").as_deref(), Some("5"));
        assert_eq!(ask(Uuid::from_u128(0xdead), "getHealth()"), None);
    }

    #[test]
    fn item_setters_become_operations() {
        assert_eq!(
            call("setItemStack(ItemStack)", &["minecraft:stone\u{1d}2"]),
            Some(PlayerCommand::EntityOp {
                uuid: Uuid::from_u128(0x17e5),
                op: EntityOperation::SetItem {
                    material: "minecraft:stone".into(),
                    amount: 2,
                },
            })
        );
        assert_eq!(
            call("setPickupDelay(int)", &["7"]),
            Some(PlayerCommand::EntityOp {
                uuid: Uuid::from_u128(0x17e5),
                op: EntityOperation::SetPickupDelay(7),
            })
        );
        assert_eq!(
            call("setOwner(UUID)", &[HOST_NULL]),
            Some(PlayerCommand::EntityOp {
                uuid: Uuid::from_u128(0x17e5),
                op: EntityOperation::SetOwner(None),
            })
        );
        assert_eq!(
            call("setUnlimitedLifetime(boolean)", &["true"]),
            Some(PlayerCommand::EntityOp {
                uuid: Uuid::from_u128(0x17e5),
                op: EntityOperation::SetUnlimitedLifetime(true),
            })
        );
        assert_eq!(
            call("setItemStack(ItemStack)", &["minecraft:air\u{1d}0"]),
            None
        );
        assert_eq!(call("setPickupDelay(int)", &["not a number"]), None);
        assert_eq!(call("setPickupDelay(int)", &[]), None);
    }
}
