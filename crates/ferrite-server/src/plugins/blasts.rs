//! `World#createExplosion`: an ordinary vanilla explosion at a point of the default world.
//!
//! The blast damages and pushes players and mobs and breaks blocks unless the plugin asked it not to.

use ferrite_plugin::{HostCall, PlayerCommand};
use uuid::Uuid;

/// The largest power vanilla ever uses is a charged creeper's 6 and a bed's 5; Bukkit allows
/// more, so this only guards against absurd values.
const MAX_POWER: f32 = 100.0;

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && v.abs() < 3.0e7)
}

/// `World#strikeLightning` / `strikeLightningEffect` on the default world.
pub fn lightning_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "World"
        || !matches!(
            call.method.as_str(),
            "strikeLightning(Location)" | "strikeLightningEffect(Location)"
        )
    {
        return None;
    }
    if Uuid::parse_str(&call.subject).ok()? != ferrite_plugin::default_world_id() {
        return None;
    }
    let mut parts = call
        .args
        .first()?
        .split(ferrite_plugin::HOST_ITEM_SEPARATOR);
    parts.next()?;
    let center = [
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    ];
    Some(PlayerCommand::Lightning { center })
}

pub fn explosion_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "World" || !call.method.starts_with("createExplosion(") {
        return None;
    }
    if Uuid::parse_str(&call.subject).ok()? != ferrite_plugin::default_world_id() {
        return None;
    }
    // `(Location, power ...)` or `(x, y, z, power ...)`; then optional `setFire`, `breakBlocks`.
    let (center, rest) = if call.method.starts_with("createExplosion(Location,") {
        let mut parts = call
            .args
            .first()?
            .split(ferrite_plugin::HOST_ITEM_SEPARATOR);
        parts.next()?;
        let center = [
            number(parts.next()?)?,
            number(parts.next()?)?,
            number(parts.next()?)?,
        ];
        (center, call.args.get(1..)?)
    } else if call
        .method
        .starts_with("createExplosion(double,double,double,")
    {
        let center = [
            number(call.args.first()?)?,
            number(call.args.get(1)?)?,
            number(call.args.get(2)?)?,
        ];
        (center, call.args.get(3..)?)
    } else {
        return None;
    };
    let power: f32 = rest
        .first()?
        .parse()
        .ok()
        .filter(|p: &f32| p.is_finite() && (0.0..=MAX_POWER).contains(p))?;
    let flag = |index: usize, default: bool| match rest.get(index).map(String::as_str) {
        None => Some(default),
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some(_) => None,
    };
    let fire = flag(1, false)?;
    let breaks = flag(2, true)?;
    Some(PlayerCommand::Explosion {
        center,
        power,
        fire,
        breaks,
    })
}

/// `World#spawnEntity` and `spawnFallingBlock`: an entity of a kind the game models.
/// An operation a plugin asks of an entity it holds a handle to.
pub fn entity_command(call: &HostCall) -> Option<PlayerCommand> {
    use ferrite_plugin::EntityOperation as Op;
    // `Player#attack(Entity)`: a bare-handed hit of one health point.
    if call.owner == "Player" && call.method == "attack(String)" {
        return Some(PlayerCommand::EntityOp {
            uuid: Uuid::parse_str(call.args.first()?).ok()?,
            op: Op::Damage(1.0),
        });
    }
    if call.owner != "Entity" {
        return None;
    }
    let uuid = Uuid::parse_str(&call.subject).ok()?;
    let triple = |text: &str| {
        let mut parts = text.split(ferrite_plugin::HOST_ITEM_SEPARATOR);
        Some([
            number(parts.next()?)?,
            number(parts.next()?)?,
            number(parts.next()?)?,
        ])
    };
    let op = match call.method.as_str() {
        "remove()" => Op::Remove,
        "setVelocity(Vector)" => Op::SetVelocity(triple(call.args.first()?)?),
        "teleportTo(double,double,double)" => Op::Teleport([
            number(call.args.first()?)?,
            number(call.args.get(1)?)?,
            number(call.args.get(2)?)?,
        ]),
        "damage(double)" => Op::Damage(
            call.args
                .first()?
                .parse::<f32>()
                .ok()
                .filter(|a| a.is_finite() && *a > 0.0 && *a <= 1.0e6)?,
        ),
        _ => return None,
    };
    Some(PlayerCommand::EntityOp { uuid, op })
}

pub fn spawn_command(call: &HostCall) -> Option<PlayerCommand> {
    // A player shoots from their own eyes (`Player#launchProjectile`); everything else is a
    // `World` call on the default world.
    let from_player = call.owner == "Player"
        && matches!(
            call.method.as_str(),
            "spawnArrow(Location,double,double,double,String)"
                | "launchThrown(Location,EntityType,double,double,double,String,String)"
        );
    if !from_player
        && (call.owner != "World"
            || Uuid::parse_str(&call.subject).ok()? != ferrite_plugin::default_world_id())
    {
        return None;
    }
    let mut parts = call
        .args
        .first()?
        .split(ferrite_plugin::HOST_ITEM_SEPARATOR);
    parts.next()?;
    let center = [
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    ];
    let mut velocity = [0.0; 3];
    let uuid = Uuid::parse_str(call.args.last()?).ok()?;
    let mut owner = None;
    let (kind, block) = match call.method.as_str() {
        "launchThrown(Location,EntityType,double,double,double,String,String)" => {
            let kind = format!("minecraft:{}", call.args.get(1)?.to_ascii_lowercase());
            ferrite_entity::arrow::Kind::thrown_from_name(&kind)?;
            velocity = [
                number(call.args.get(2)?)?,
                number(call.args.get(3)?)?,
                number(call.args.get(4)?)?,
            ];
            if velocity.iter().map(|v| v * v).sum::<f64>() > 10.0 * 10.0 {
                return None;
            }
            owner = Uuid::parse_str(call.args.get(5)?).ok();
            (kind, None)
        }
        "spawnArrow(Location,double,double,double,String)" => {
            velocity = [
                number(call.args.get(1)?)?,
                number(call.args.get(2)?)?,
                number(call.args.get(3)?)?,
            ];
            // Vanilla caps what a bow gives an arrow at 3 blocks per tick; allow a little more.
            if velocity.iter().map(|v| v * v).sum::<f64>() > 10.0 * 10.0 {
                return None;
            }
            ("minecraft:arrow".to_owned(), None)
        }
        "spawnEntity(Location,EntityType,String)" => {
            let kind = format!("minecraft:{}", call.args.get(1)?.to_ascii_lowercase());
            let modelled = matches!(kind.as_str(), "minecraft:tnt")
                || ferrite_entity::arrow::Kind::thrown_from_name(&kind).is_some()
                || ferrite_entity::mob::MobKind::from_name(&kind).is_some();
            if !modelled {
                return None;
            }
            (kind, None)
        }
        "spawnFallingBlock(Location,Material,String)" => {
            let block = format!("minecraft:{}", call.args.get(1)?.to_ascii_lowercase());
            ferrite_data::blocks::default_state(&block)?;
            ("minecraft:falling_block".to_owned(), Some(block))
        }
        _ => return None,
    };
    Some(PlayerCommand::SpawnEntity {
        owner,
        center,
        kind,
        block,
        velocity,
        uuid,
    })
}

/// `World#dropItem(Location, ItemStack)` and `dropItemNaturally`: an item entity at the spot.
pub fn drop_command(call: &HostCall) -> Option<PlayerCommand> {
    let natural = match call.method.as_str() {
        "dropItem(Location,ItemStack,String)" => false,
        "dropItemNaturally(Location,ItemStack,String)" => true,
        _ => return None,
    };
    if call.owner != "World"
        || Uuid::parse_str(&call.subject).ok()? != ferrite_plugin::default_world_id()
    {
        return None;
    }
    let mut parts = call
        .args
        .first()?
        .split(ferrite_plugin::HOST_ITEM_SEPARATOR);
    parts.next()?;
    let center = [
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    ];
    let (material, amount) = call
        .args
        .get(1)?
        .split_once(ferrite_plugin::HOST_ITEM_SEPARATOR)?;
    let amount: u8 = amount.parse().ok().filter(|a| (1..=99).contains(a))?;
    let item = format!("minecraft:{}", material.to_ascii_lowercase());
    ferrite_item::item_id(&item)?;
    Some(PlayerCommand::DropItem {
        center,
        item,
        amount,
        natural,
        uuid: Uuid::parse_str(call.args.get(2)?).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_operations_become_commands() {
        let uuid = Uuid::from_u128(9);
        let on_entity = |method: &str, args: &[&str]| {
            let mut c = call(method, args);
            c.owner = "Entity".into();
            c.subject = uuid.to_string();
            entity_command(&c)
        };
        assert_eq!(
            on_entity("remove()", &[]),
            Some(PlayerCommand::EntityOp {
                uuid,
                op: ferrite_plugin::EntityOperation::Remove
            })
        );
        let push = format!("0{0}1.5{0}0", ferrite_plugin::HOST_ITEM_SEPARATOR);
        assert_eq!(
            on_entity("setVelocity(Vector)", &[&push]),
            Some(PlayerCommand::EntityOp {
                uuid,
                op: ferrite_plugin::EntityOperation::SetVelocity([0.0, 1.5, 0.0])
            })
        );
        assert_eq!(on_entity("damage(double)", &["-1"]), None);
        let mut attack = call("attack(String)", &["00000000-0000-0000-0000-000000000009"]);
        attack.owner = "Player".into();
        assert!(entity_command(&attack).is_some());
    }

    #[test]
    fn spawns_are_limited_to_modelled_entities() {
        let at = format!(
            "w{0}1{0}70{0}2{0}0{0}0",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        let spawn = |method: &str, what: &str| {
            spawn_command(&call(
                method,
                &[&at, what, "00000000-0000-0000-0000-000000000009"],
            ))
        };
        assert_eq!(
            spawn("spawnEntity(Location,EntityType,String)", "COW"),
            Some(PlayerCommand::SpawnEntity {
                owner: None,
                center: [1.0, 70.0, 2.0],
                kind: "minecraft:cow".into(),
                block: None,
                velocity: [0.0; 3],
                uuid: Uuid::from_u128(9)
            })
        );
        assert!(spawn("spawnEntity(Location,EntityType,String)", "TNT").is_some());
        assert_eq!(
            spawn("spawnEntity(Location,EntityType,String)", "GHAST"),
            None
        );
        assert_eq!(
            spawn("spawnFallingBlock(Location,Material,String)", "SAND"),
            Some(PlayerCommand::SpawnEntity {
                owner: None,
                center: [1.0, 70.0, 2.0],
                kind: "minecraft:falling_block".into(),
                block: Some("minecraft:sand".into()),
                velocity: [0.0; 3],
                uuid: Uuid::from_u128(9)
            })
        );
        let shot = spawn_command(&call(
            "spawnArrow(Location,double,double,double,String)",
            &[
                &at,
                "0",
                "0.5",
                "2.5",
                "00000000-0000-0000-0000-000000000009",
            ],
        ));
        assert_eq!(
            shot,
            Some(PlayerCommand::SpawnEntity {
                owner: None,
                center: [1.0, 70.0, 2.0],
                kind: "minecraft:arrow".into(),
                block: None,
                velocity: [0.0, 0.5, 2.5],
                uuid: Uuid::from_u128(9)
            })
        );
        assert!(matches!(
            spawn_command(&call(
                "launchThrown(Location,EntityType,double,double,double,String,String)",
                &[
                    &at,
                    "ENDER_PEARL",
                    "0",
                    "0.5",
                    "1.5",
                    "00000000-0000-0000-0000-0000000000aa",
                    "00000000-0000-0000-0000-000000000009",
                ],
            )),
            Some(PlayerCommand::SpawnEntity {
                owner: Some(_),
                velocity: [0.0, 0.5, 1.5],
                ..
            })
        ));
        assert_eq!(
            spawn_command(&call(
                "launchThrown(Location,EntityType,double,double,double,String,String)",
                &[
                    &at,
                    "SMALL_FIREBALL",
                    "0",
                    "0.5",
                    "1.5",
                    "00000000-0000-0000-0000-0000000000aa",
                    "00000000-0000-0000-0000-000000000009",
                ],
            )),
            Some(PlayerCommand::SpawnEntity {
                owner: Some(Uuid::from_u128(0xaa)),
                center: [1.0, 70.0, 2.0],
                kind: "minecraft:small_fireball".into(),
                block: None,
                velocity: [0.0, 0.5, 1.5],
                uuid: Uuid::from_u128(9),
            })
        );
        assert_eq!(
            spawn_command(&call(
                "launchThrown(Location,EntityType,double,double,double,String,String)",
                &[
                    &at,
                    "DRAGON_FIREBALL",
                    "0",
                    "0.5",
                    "1.5",
                    "00000000-0000-0000-0000-0000000000aa",
                    "00000000-0000-0000-0000-000000000009",
                ],
            )),
            None
        );
        assert_eq!(
            spawn_command(&call(
                "launchThrown(Location,EntityType,double,double,double,String,String)",
                &[
                    &at,
                    "SMALL_FIREBALL",
                    "0",
                    "0",
                    "99",
                    "00000000-0000-0000-0000-0000000000aa",
                    "00000000-0000-0000-0000-000000000009",
                ],
            )),
            None
        );
        assert_eq!(
            spawn_command(&call(
                "spawnArrow(Location,double,double,double,String)",
                &[&at, "0", "0", "99", "00000000-0000-0000-0000-000000000009"]
            )),
            None
        );
        assert_eq!(
            spawn("spawnFallingBlock(Location,Material,String)", "NOPE"),
            None
        );
    }

    #[test]
    fn dropped_items_become_commands() {
        let at = format!(
            "w{0}1.5{0}70{0}-2{0}0{0}0",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        let stack = format!("DIAMOND{}3", ferrite_plugin::HOST_ITEM_SEPARATOR);
        assert_eq!(
            drop_command(&call(
                "dropItemNaturally(Location,ItemStack,String)",
                &[&at, &stack, "00000000-0000-0000-0000-000000000009"]
            )),
            Some(PlayerCommand::DropItem {
                center: [1.5, 70.0, -2.0],
                item: "minecraft:diamond".into(),
                amount: 3,
                natural: true,
                uuid: Uuid::from_u128(9),
            })
        );
        let bad = format!("NOT_AN_ITEM{}1", ferrite_plugin::HOST_ITEM_SEPARATOR);
        assert_eq!(
            drop_command(&call(
                "dropItem(Location,ItemStack,String)",
                &[&at, &bad, "00000000-0000-0000-0000-000000000009"]
            )),
            None
        );
    }

    #[test]
    fn lightning_calls_become_a_bolt_at_the_location() {
        let at = format!(
            "world{s}1.5{s}64{s}-2{s}0{s}0",
            s = ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        for method in [
            "strikeLightning(Location)",
            "strikeLightningEffect(Location)",
        ] {
            assert_eq!(
                lightning_command(&call(method, &[&at])),
                Some(PlayerCommand::Lightning {
                    center: [1.5, 64.0, -2.0]
                })
            );
        }
        assert_eq!(
            lightning_command(&call("strikeLightning(Location)", &["bad"])),
            None
        );
        assert_eq!(lightning_command(&call("getName()", &[&at])), None);
    }

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: ferrite_plugin::default_world_id().to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn at() -> String {
        format!(
            "w{0}10.5{0}64{0}-3{0}0{0}0",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        )
    }

    #[test]
    fn location_and_coordinate_forms_become_one_command() {
        let expected = Some(PlayerCommand::Explosion {
            center: [10.5, 64.0, -3.0],
            power: 4.0,
            fire: true,
            breaks: true,
        });
        assert_eq!(
            explosion_command(&call(
                "createExplosion(Location,float,boolean,boolean)",
                &[&at(), "4", "true", "true"]
            )),
            expected
        );
        assert_eq!(
            explosion_command(&call(
                "createExplosion(double,double,double,float,boolean)",
                &["10.5", "64", "-3", "4", "true"]
            )),
            expected
        );
    }

    #[test]
    fn defaults_and_refusals() {
        assert_eq!(
            explosion_command(&call("createExplosion(Location,float)", &[&at(), "2"])),
            Some(PlayerCommand::Explosion {
                center: [10.5, 64.0, -3.0],
                power: 2.0,
                fire: false,
                breaks: true
            })
        );
        // Block damage can be switched off.
        assert_eq!(
            explosion_command(&call(
                "createExplosion(Location,float,boolean,boolean)",
                &[&at(), "2", "false", "false"]
            )),
            Some(PlayerCommand::Explosion {
                center: [10.5, 64.0, -3.0],
                power: 2.0,
                fire: false,
                breaks: false
            })
        );
        assert_eq!(
            explosion_command(&call("createExplosion(Location,float)", &[&at(), "NaN"])),
            None
        );
        assert_eq!(
            explosion_command(&call("createExplosion(Location,float)", &[&at(), "1000"])),
            None
        );
    }
}
