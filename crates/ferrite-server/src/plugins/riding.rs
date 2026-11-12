//! `Entity#getVehicle` / `getPassengers` / `addPassenger` / `leaveVehicle` / `eject` and friends,
//! backed by the same ride table `/ride` uses. The table is global, so the answer is exact; the
//! game loop is only told which vehicle's passenger list changed so it can resend it.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall, PlayerCommand};
use uuid::Uuid;

use crate::game::ride;

/// The reply to `call` and, for a change, the command that tells clients about it.
pub fn handle(call: &HostCall) -> Option<(String, Option<PlayerCommand>)> {
    if call.owner != "Player" && call.owner != "Entity" {
        return None;
    }
    let subject = Uuid::parse_str(&call.subject).ok()?;
    let changed = |vehicle| Some(PlayerCommand::RideChanged { vehicle });
    let arg = || Uuid::parse_str(call.args.first()?).ok();
    Some(match call.method.as_str() {
        "isInsideVehicle()" => (ride::vehicle_of(subject).is_some().to_string(), None),
        "getVehicle()" => (
            ride::vehicle_of(subject)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            None,
        ),
        "getPassengers()" => (
            ride::passengers_of(subject)
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(&HOST_ITEM_SEPARATOR.to_string()),
            None,
        ),
        "isEmpty()" => (ride::passengers_of(subject).is_empty().to_string(), None),
        "leaveVehicle()" => match ride::dismount(subject) {
            Some(vehicle) => ("true".to_owned(), changed(vehicle)),
            None => ("false".to_owned(), None),
        },
        "eject()" => {
            let riders = ride::passengers_of(subject);
            for rider in &riders {
                ride::dismount(*rider);
            }
            (
                (!riders.is_empty()).to_string(),
                (!riders.is_empty()).then(|| changed(subject)).flatten(),
            )
        }
        "addPassenger(Entity)" => match ride::mount_in(
            arg()?,
            subject,
            false,
            super::facts::fact(subject).map(|f| f.kind),
        ) {
            Ok(()) => ("true".to_owned(), changed(subject)),
            Err(_) => ("false".to_owned(), None),
        },
        "removePassenger(Entity)" => {
            let rider = arg()?;
            if ride::vehicle_of(rider) == Some(subject) {
                ride::dismount(rider);
                ("true".to_owned(), changed(subject))
            } else {
                ("false".to_owned(), None)
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(owner: &str, subject: Uuid, method: &str, args: &[Uuid]) -> HostCall {
        HostCall {
            owner: owner.into(),
            subject: subject.to_string(),
            method: method.into(),
            args: args.iter().map(Uuid::to_string).collect(),
        }
    }

    #[test]
    fn riding_follows_the_shared_ride_table() {
        let _turn = crate::game::ride::test_turn();
        let (vehicle, rider) = (Uuid::from_u128(0x71de1), Uuid::from_u128(0x71de2));
        let ask = |owner, subject, method: &str, args: &[Uuid]| {
            handle(&call(owner, subject, method, args)).unwrap()
        };
        assert_eq!(ask("Player", rider, "isInsideVehicle()", &[]).0, "false");
        let (reply, command) = ask("Entity", vehicle, "addPassenger(Entity)", &[rider]);
        assert_eq!(reply, "true");
        assert_eq!(command, Some(PlayerCommand::RideChanged { vehicle }));
        assert_eq!(
            ask("Player", rider, "getVehicle()", &[]).0,
            vehicle.to_string()
        );
        assert_eq!(
            ask("Entity", vehicle, "getPassengers()", &[]).0,
            rider.to_string()
        );
        assert_eq!(ask("Entity", vehicle, "isEmpty()", &[]).0, "false");
        assert_eq!(
            ask("Entity", vehicle, "addPassenger(Entity)", &[rider]).0,
            "false"
        );
        let (reply, command) = ask("Player", rider, "leaveVehicle()", &[]);
        assert_eq!(
            (reply.as_str(), command),
            ("true", Some(PlayerCommand::RideChanged { vehicle }))
        );
        assert_eq!(ask("Player", rider, "leaveVehicle()", &[]).0, "false");
        ask("Entity", vehicle, "addPassenger(Entity)", &[rider]);
        assert_eq!(ask("Entity", vehicle, "eject()", &[]).0, "true");
        assert_eq!(ask("Entity", vehicle, "eject()", &[]).0, "false");
        assert_eq!(
            ask("Entity", vehicle, "removePassenger(Entity)", &[rider]).0,
            "false"
        );
    }
}
