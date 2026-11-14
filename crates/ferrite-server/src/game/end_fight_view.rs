//! What plugins see of, and ask of, the End fight (see `plugins::dragon`).

use super::end_fight::RespawnStage;

/// The fight as the End region last saw it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub started: bool,
    pub killed: bool,
    pub gateways: u32,
    pub dragon_alive: bool,
    pub stage: Option<RespawnStage>,
    pub portal_level: Option<i32>,
}

/// A change a plugin asked for, applied by the End region on its next tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    SetKilled(bool),
    /// Builds the exit portal podium (`true`: with the active portal).
    GeneratePortal(bool),
    InitiateRespawn,
    SpawnGateway,
    /// Opens a gateway at an arbitrary spot (`DragonBattle#spawnNewGateway(Position)`); the
    /// vanilla count and ring are untouched, as in vanilla.
    SpawnGatewayAt([i32; 3]),
    /// Removes the spike crystals and lets the placement pass rebuild all of them.
    ResetCrystals,
    /// `None` aborts the respawn sequence.
    SetPhase(Option<RespawnStage>),
}

/// The stage a serialized name stands for.
pub fn stage(name: &str) -> Option<RespawnStage> {
    RespawnStage::from_name(name)
}

impl super::end_fight::FightState {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            started: self.started,
            killed: self.killed,
            gateways: self.gateways,
            dragon_alive: self.dragon_alive,
            stage: self.respawn_stage,
            portal_level: self.exit_portal_level,
        }
    }

    /// Applies one plugin request to the state; the caller places any blocks the request needs.
    /// Returns the portal podium to build, if the request asked for one.
    pub fn apply(&mut self, request: Request, floor: Option<i32>) -> Option<(i32, bool)> {
        match request {
            Request::SetKilled(killed) => self.killed = killed,
            Request::GeneratePortal(active) => {
                let level = self.exit_portal_level.or(floor)?;
                self.exit_portal_level = Some(level);
                return Some((level, active));
            }
            Request::InitiateRespawn => {
                if let Some(level) = self.exit_portal_level.or(floor) {
                    self.begin_respawn(level);
                }
            }
            Request::SpawnGateway => {
                self.open_next_gateway();
            }
            // The region places the blocks; the fight's counters do not change.
            Request::SpawnGatewayAt(_) => {}
            Request::ResetCrystals => self.crystals.clear(),
            Request::SetPhase(None) => self.abort_respawn(),
            Request::SetPhase(Some(stage)) => {
                self.respawn_stage = Some(stage);
                self.respawn_ticks = 0;
                if self.exit_portal_level.is_none() {
                    self.exit_portal_level = floor;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::end_fight::FightState;

    #[test]
    fn requests_change_the_fight_and_a_respawn_needs_a_kill() {
        let mut fight = FightState::default();
        assert_eq!(fight.apply(Request::InitiateRespawn, Some(60)), None);
        assert_eq!(fight.respawn_stage, None, "no dragon has died yet");
        fight.apply(Request::SetKilled(true), None);
        fight.apply(Request::InitiateRespawn, Some(60));
        assert_eq!(fight.respawn_stage, Some(RespawnStage::Start));
        assert_eq!(fight.snapshot().portal_level, Some(60));
        fight.apply(Request::SetPhase(None), None);
        assert_eq!(fight.respawn_stage, None);
        fight.apply(
            Request::SetPhase(Some(RespawnStage::SummoningPillars)),
            None,
        );
        assert_eq!(fight.respawn_stage, Some(RespawnStage::SummoningPillars));
        fight.apply(Request::SpawnGateway, None);
        assert_eq!(fight.gateways, 1);
        fight.apply(Request::SpawnGatewayAt([5, 70, 5]), None);
        assert_eq!(
            fight.gateways, 1,
            "a gateway at a chosen spot is not part of the ring"
        );
        fight.crystals.extend([1, 2]);
        fight.apply(Request::ResetCrystals, None);
        assert!(
            fight.crystals.is_empty(),
            "every spike crystal is placed again"
        );
        assert_eq!(
            fight.apply(Request::GeneratePortal(true), None),
            Some((60, true))
        );
    }
}
