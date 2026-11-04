//! `World#createExplosion`: an ordinary vanilla explosion at a point of the default world.
//!
//! The blast breaks blocks, hurts and pushes players and mobs like a bed explosion; a plugin
//! asking for no block damage is left unhandled because the region's explosion always breaks
//! blocks.

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
    if !flag(2, true)? {
        return None;
    }
    Some(PlayerCommand::Explosion {
        center,
        power,
        fire,
    })
}

/// `World#dropItem(Location, ItemStack)` and `dropItemNaturally`: an item entity at the spot.
pub fn drop_command(call: &HostCall) -> Option<PlayerCommand> {
    let natural = match call.method.as_str() {
        "dropItem(Location,ItemStack)" => false,
        "dropItemNaturally(Location,ItemStack)" => true,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_items_become_commands() {
        let at = format!(
            "w{0}1.5{0}70{0}-2{0}0{0}0",
            ferrite_plugin::HOST_ITEM_SEPARATOR
        );
        let stack = format!("DIAMOND{}3", ferrite_plugin::HOST_ITEM_SEPARATOR);
        assert_eq!(
            drop_command(&call(
                "dropItemNaturally(Location,ItemStack)",
                &[&at, &stack]
            )),
            Some(PlayerCommand::DropItem {
                center: [1.5, 70.0, -2.0],
                item: "minecraft:diamond".into(),
                amount: 3,
                natural: true,
            })
        );
        let bad = format!("NOT_AN_ITEM{}1", ferrite_plugin::HOST_ITEM_SEPARATOR);
        assert_eq!(
            drop_command(&call("dropItem(Location,ItemStack)", &[&at, &bad])),
            None
        );
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
                fire: false
            })
        );
        // Block damage cannot be switched off.
        assert_eq!(
            explosion_command(&call(
                "createExplosion(Location,float,boolean,boolean)",
                &[&at(), "2", "false", "false"]
            )),
            None
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
