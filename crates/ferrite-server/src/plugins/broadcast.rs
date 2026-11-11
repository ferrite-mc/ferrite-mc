//! `Player#broadcast*` calls that show an event of the subject's entity to a chosen set of viewers.
use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall, PlayerCommand, PluginPacket};
use uuid::Uuid;

/// Vanilla `LivingEntity.getEquipmentBreakEvent`: the entity event that plays the item-break
/// effect for each equipment slot.
fn slot_break_status(slot: &str) -> Option<u8> {
    Some(match slot {
        "HAND" => 47,
        "OFF_HAND" => 48,
        "HEAD" => 49,
        "CHEST" => 50,
        "LEGS" => 51,
        "FEET" => 52,
        _ => return None,
    })
}

/// One packet per viewer. A viewer list that cannot be read completely yields nothing, so a
/// malformed call never sends to a partial audience.
pub fn broadcast_commands(call: &HostCall) -> Vec<PlayerCommand> {
    broadcast(call).unwrap_or_default()
}

fn broadcast(call: &HostCall) -> Option<Vec<PlayerCommand>> {
    if call.owner != "Player" || call.method != "broadcastSlotBreak(EquipmentSlot,Collection)" {
        return None;
    }
    let subject = Uuid::parse_str(&call.subject).ok()?;
    let status = slot_break_status(call.args.first()?)?;
    let entity_id = super::facts::entity_id(subject)?;
    let viewers = call.args.get(1)?;
    if viewers.is_empty() {
        return Some(Vec::new());
    }
    let viewers = viewers
        .split(HOST_ITEM_SEPARATOR)
        .map(|viewer| Uuid::parse_str(viewer).ok())
        .collect::<Option<Vec<_>>>()?;
    if viewers.len() > 1024 {
        return None;
    }
    Some(
        viewers
            .into_iter()
            .map(|player| PlayerCommand::Packet {
                player,
                packet: PluginPacket::EntityEvent { entity_id, status },
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::facts::{PlayerFacts, forget, record};

    fn call(subject: u128, slot: &str, viewers: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: Uuid::from_u128(subject).to_string(),
            method: "broadcastSlotBreak(EquipmentSlot,Collection)".into(),
            args: vec![slot.into(), viewers.into()],
        }
    }

    fn facts(entity_id: i32) -> PlayerFacts {
        PlayerFacts {
            entity_id,
            joined: std::time::SystemTime::now(),
            address: None,
        }
    }

    #[test]
    fn slot_break_reaches_each_listed_viewer_with_the_subjects_entity() {
        let subject = Uuid::from_u128(0xb10);
        record(subject, facts(4242));
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let viewers = format!("{a}{HOST_ITEM_SEPARATOR}{b}");
        let commands = broadcast_commands(&call(0xb10, "CHEST", &viewers));
        let event = |player| PlayerCommand::Packet {
            player,
            packet: PluginPacket::EntityEvent {
                entity_id: 4242,
                status: 50,
            },
        };
        assert_eq!(commands, vec![event(a), event(b)]);
        for (slot, status) in [
            ("HAND", 47),
            ("OFF_HAND", 48),
            ("HEAD", 49),
            ("LEGS", 51),
            ("FEET", 52),
        ] {
            let commands = broadcast_commands(&call(0xb10, slot, &a.to_string()));
            assert_eq!(
                commands,
                vec![PlayerCommand::Packet {
                    player: a,
                    packet: PluginPacket::EntityEvent {
                        entity_id: 4242,
                        status
                    }
                }]
            );
        }
        forget(subject);
    }

    #[test]
    fn unknown_slots_bad_viewers_and_unknown_subjects_send_nothing() {
        let subject = Uuid::from_u128(0xb11);
        let a = Uuid::from_u128(1);
        // The subject has no recorded entity id yet.
        assert!(broadcast_commands(&call(0xb11, "HEAD", &a.to_string())).is_empty());
        record(subject, facts(7));
        assert!(broadcast_commands(&call(0xb11, "SADDLE", &a.to_string())).is_empty());
        let mixed = format!("{a}{HOST_ITEM_SEPARATOR}not-a-uuid");
        assert!(broadcast_commands(&call(0xb11, "HEAD", &mixed)).is_empty());
        assert!(broadcast_commands(&call(0xb11, "HEAD", "")).is_empty());
        forget(subject);
    }
}
