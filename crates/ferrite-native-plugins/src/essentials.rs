//! EssentialsX-style core utility engine.
//!
//! Provides per-player homes, global warps, global spawn, TPA teleport requests with configurable
//! TTL and expiry cleanup, an atomic non-negative economy ledger, chat mute tracking, and JSON
//! persistence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{PluginError, Result};
use crate::storage::{load_json, load_json_or_default, save_json_atomic};

/// Default TPA request lifetime in milliseconds (60 seconds).
pub const DEFAULT_TPA_TTL_MS: u64 = 60_000;

/// Full 3D world location with orientation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationSpec {
    /// World name / dimension identifier.
    pub world: String,
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
    /// Horizontal look yaw in degrees.
    pub yaw: f32,
    /// Vertical look pitch in degrees.
    pub pitch: f32,
}

impl LocationSpec {
    /// Creates a location with explicit coordinates and orientation.
    pub fn new(world: impl Into<String>, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Self {
        Self {
            world: world.into(),
            x,
            y,
            z,
            yaw,
            pitch,
        }
    }

    /// Creates a location from `[x, y, z]` coordinates with zero yaw/pitch.
    pub fn from_pos(world: impl Into<String>, pos: [f64; 3]) -> Self {
        Self {
            world: world.into(),
            x: pos[0],
            y: pos[1],
            z: pos[2],
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// Pending teleport-ask (`/tpa`) request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TpaRequest {
    /// Player who wants to teleport.
    pub requester: Uuid,
    /// Player being asked for permission.
    pub target: Uuid,
    /// Creation timestamp in Unix epoch milliseconds.
    pub created_at_ms: u64,
    /// Expiration timestamp in Unix epoch milliseconds (`now >= expires_at_ms` is expired).
    pub expires_at_ms: u64,
}

impl TpaRequest {
    /// Returns `true` if this request has expired at `now_ms`.
    #[inline]
    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

/// Chat mute metadata for a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MuteRecord {
    /// Optional human-readable mute reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Mute timestamp in Unix epoch milliseconds.
    pub muted_at_ms: u64,
    /// Optional expiration timestamp in Unix epoch milliseconds (`None` = permanent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

impl MuteRecord {
    /// Returns `true` if the mute is active at `now_ms`.
    #[inline]
    pub fn is_active_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms.is_none_or(|exp| now_ms < exp)
    }
}

/// EssentialsX-style homes, warps, spawn, TPA, economy, and mute engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EssentialsEngine {
    /// Per-player named homes (`player -> home_name -> location`).
    #[serde(default)]
    pub homes: BTreeMap<Uuid, BTreeMap<String, LocationSpec>>,
    /// Global named warps (`warp_name -> location`).
    #[serde(default)]
    pub warps: BTreeMap<String, LocationSpec>,
    /// Global server spawn location.
    #[serde(default)]
    pub spawn: Option<LocationSpec>,
    /// Pending TPA requests keyed by target player UUID (ordered oldest to newest).
    #[serde(default)]
    pub pending_tpas: BTreeMap<Uuid, Vec<TpaRequest>>,
    /// Configured TPA request time-to-live in milliseconds.
    #[serde(default = "default_tpa_ttl_ms")]
    pub tpa_ttl_ms: u64,
    /// Account balances keyed by player UUID.
    #[serde(default)]
    pub balances: BTreeMap<Uuid, f64>,
    /// Starting balance for accounts without a ledger entry.
    #[serde(default)]
    pub starting_balance: f64,
    /// Active chat mutes keyed by player UUID.
    #[serde(default)]
    pub mutes: BTreeMap<Uuid, MuteRecord>,
    /// Last recorded location prior to teleport/death for `/back`.
    #[serde(default)]
    pub last_locations: BTreeMap<Uuid, LocationSpec>,
    /// Players with invulnerable god mode enabled (`/god`).
    #[serde(default)]
    pub god_mode: BTreeSet<Uuid>,
    /// Players with flight enabled (`/fly`).
    #[serde(default)]
    pub fly_mode: BTreeSet<Uuid>,
}

const fn default_tpa_ttl_ms() -> u64 {
    DEFAULT_TPA_TTL_MS
}

impl Default for EssentialsEngine {
    fn default() -> Self {
        Self {
            homes: BTreeMap::new(),
            warps: BTreeMap::new(),
            spawn: None,
            pending_tpas: BTreeMap::new(),
            tpa_ttl_ms: DEFAULT_TPA_TTL_MS,
            balances: BTreeMap::new(),
            starting_balance: 0.0,
            mutes: BTreeMap::new(),
            last_locations: BTreeMap::new(),
            god_mode: BTreeSet::new(),
            fly_mode: BTreeSet::new(),
        }
    }
}

impl EssentialsEngine {
    /// Creates a new essentials engine with default settings (`tpa_ttl = 60s`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an essentials engine with a custom TPA TTL.
    pub fn with_tpa_ttl(tpa_ttl: Duration) -> Self {
        let mut engine = Self::default();
        engine.set_tpa_ttl(tpa_ttl);
        engine
    }

    /// Returns the current system clock in Unix epoch milliseconds.
    pub fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    fn normalize_name(raw: &str) -> Result<String> {
        let trimmed = raw.trim().to_ascii_lowercase();
        if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
            return Err(PluginError::InvalidIdentifier(raw.to_owned()));
        }
        Ok(trimmed)
    }

    // ── Homes, Warps, Spawn ─────────────────────────────────────────────────────────────

    /// Sets or updates a named home for `player`.
    pub fn set_home(&mut self, player: Uuid, name: &str, location: LocationSpec) -> Result<()> {
        let key = Self::normalize_name(name)?;
        self.homes.entry(player).or_default().insert(key, location);
        Ok(())
    }

    /// Retrieves a player's named home.
    pub fn home(&self, player: Uuid, name: &str) -> Option<&LocationSpec> {
        let key = Self::normalize_name(name).ok()?;
        self.homes.get(&player)?.get(&key)
    }

    /// Deletes a player's named home, returning `true` if it existed.
    pub fn del_home(&mut self, player: Uuid, name: &str) -> Result<bool> {
        let key = Self::normalize_name(name)?;
        let removed = self
            .homes
            .get_mut(&player)
            .is_some_and(|m| m.remove(&key).is_some());
        if self.homes.get(&player).is_some_and(BTreeMap::is_empty) {
            self.homes.remove(&player);
        }
        Ok(removed)
    }

    /// Lists all home names for `player` in alphabetical order.
    pub fn list_homes(&self, player: Uuid) -> Vec<String> {
        self.homes
            .get(&player)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Sets or updates a global warp.
    pub fn set_warp(&mut self, name: &str, location: LocationSpec) -> Result<()> {
        let key = Self::normalize_name(name)?;
        self.warps.insert(key, location);
        Ok(())
    }

    /// Retrieves a global warp by name.
    pub fn warp(&self, name: &str) -> Option<&LocationSpec> {
        let key = Self::normalize_name(name).ok()?;
        self.warps.get(&key)
    }

    /// Deletes a global warp, returning `true` if it existed.
    pub fn del_warp(&mut self, name: &str) -> Result<bool> {
        let key = Self::normalize_name(name)?;
        Ok(self.warps.remove(&key).is_some())
    }

    /// Lists all global warp names in alphabetical order.
    pub fn list_warps(&self) -> Vec<String> {
        self.warps.keys().cloned().collect()
    }

    /// Sets the global spawn location.
    pub fn set_spawn(&mut self, location: LocationSpec) {
        self.spawn = Some(location);
    }

    /// Returns the global spawn location, if configured.
    pub fn spawn(&self) -> Option<&LocationSpec> {
        self.spawn.as_ref()
    }

    // ── TPA Requests ────────────────────────────────────────────────────────────────────

    /// Returns the configured TPA request TTL.
    pub fn tpa_ttl(&self) -> Duration {
        Duration::from_millis(self.tpa_ttl_ms)
    }

    /// Updates the TPA request TTL.
    pub fn set_tpa_ttl(&mut self, ttl: Duration) {
        self.tpa_ttl_ms = u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX);
    }

    /// Sends a TPA request from `requester` to `target` using the system clock.
    pub fn request_tpa(&mut self, requester: Uuid, target: Uuid) -> Result<TpaRequest> {
        self.request_tpa_at(requester, target, Self::now_unix_ms())
    }

    /// Sends a TPA request at explicit timestamp `now_ms`.
    pub fn request_tpa_at(
        &mut self,
        requester: Uuid,
        target: Uuid,
        now_ms: u64,
    ) -> Result<TpaRequest> {
        if requester == target {
            return Err(PluginError::CannotTpaSelf);
        }

        self.cleanup_expired_tpas_at(now_ms);
        let req = TpaRequest {
            requester,
            target,
            created_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(self.tpa_ttl_ms),
        };

        let queue = self.pending_tpas.entry(target).or_default();
        queue.retain(|existing| existing.requester != requester);
        queue.push(req.clone());
        Ok(req)
    }

    /// Accepts the latest pending TPA request for `target` using the system clock.
    pub fn accept_tpa(&mut self, target: Uuid) -> Result<TpaRequest> {
        self.accept_tpa_at(target, None, Self::now_unix_ms())
    }

    /// Accepts a pending TPA request for `target` (optionally filtered to `requester`) at `now_ms`.
    pub fn accept_tpa_at(
        &mut self,
        target: Uuid,
        requester: Option<Uuid>,
        now_ms: u64,
    ) -> Result<TpaRequest> {
        self.resolve_tpa_at(target, requester, now_ms)
    }

    /// Denies the latest pending TPA request for `target` using the system clock.
    pub fn deny_tpa(&mut self, target: Uuid) -> Result<TpaRequest> {
        self.deny_tpa_at(target, None, Self::now_unix_ms())
    }

    /// Denies a pending TPA request for `target` (optionally filtered to `requester`) at `now_ms`.
    pub fn deny_tpa_at(
        &mut self,
        target: Uuid,
        requester: Option<Uuid>,
        now_ms: u64,
    ) -> Result<TpaRequest> {
        self.resolve_tpa_at(target, requester, now_ms)
    }

    fn resolve_tpa_at(
        &mut self,
        target: Uuid,
        requester: Option<Uuid>,
        now_ms: u64,
    ) -> Result<TpaRequest> {
        let Some(queue) = self.pending_tpas.get_mut(&target) else {
            return Err(PluginError::NoPendingTpa);
        };

        let had_matching = match requester {
            Some(req_id) => queue.iter().any(|r| r.requester == req_id),
            None => !queue.is_empty(),
        };
        if !had_matching {
            return Err(PluginError::NoPendingTpa);
        }

        queue.retain(|r| !r.is_expired_at(now_ms));

        let pos = match requester {
            Some(req_id) => queue.iter().rposition(|r| r.requester == req_id),
            None => queue.len().checked_sub(1),
        };

        let result = match pos {
            Some(idx) => Ok(queue.remove(idx)),
            None => Err(PluginError::TpaExpired),
        };

        if queue.is_empty() {
            self.pending_tpas.remove(&target);
        }
        result
    }

    /// Removes all expired TPA requests using the system clock, returning the count removed.
    pub fn cleanup_expired_tpas(&mut self) -> usize {
        self.cleanup_expired_tpas_at(Self::now_unix_ms())
    }

    /// Removes all expired TPA requests at timestamp `now_ms`, returning the count removed.
    pub fn cleanup_expired_tpas_at(&mut self, now_ms: u64) -> usize {
        let mut removed = 0usize;
        self.pending_tpas.retain(|_, queue| {
            let before = queue.len();
            queue.retain(|req| !req.is_expired_at(now_ms));
            removed += before - queue.len();
            !queue.is_empty()
        });
        removed
    }

    // ── Economy Ledger ──────────────────────────────────────────────────────────────────

    fn validate_positive_amount(amount: f64) -> Result<f64> {
        if !amount.is_finite() || amount <= 0.0 {
            return Err(PluginError::InvalidAmount(amount));
        }
        Ok(amount)
    }

    fn normalize_balance(val: f64) -> f64 {
        if !val.is_finite() || val <= 1e-12 {
            0.0
        } else {
            val
        }
    }

    /// Returns `player`'s current balance (or `starting_balance` if unset).
    pub fn balance(&self, player: Uuid) -> f64 {
        self.balances
            .get(&player)
            .copied()
            .unwrap_or(self.starting_balance)
    }

    /// Deposits `amount` into `player`'s account and returns the updated balance.
    pub fn deposit(&mut self, player: Uuid, amount: f64) -> Result<f64> {
        let amount = Self::validate_positive_amount(amount)?;
        let current = self.balance(player);
        let next = Self::normalize_balance(current + amount);
        if !next.is_finite() {
            return Err(PluginError::InvalidAmount(amount));
        }
        self.balances.insert(player, next);
        Ok(next)
    }

    /// Withdraws `amount` from `player`'s account if sufficient funds exist.
    pub fn withdraw(&mut self, player: Uuid, amount: f64) -> Result<f64> {
        let amount = Self::validate_positive_amount(amount)?;
        let current = self.balance(player);
        if current + 1e-12 < amount {
            return Err(PluginError::InsufficientFunds {
                balance: current,
                required: amount,
            });
        }
        let next = Self::normalize_balance(current - amount);
        self.balances.insert(player, next);
        Ok(next)
    }

    /// Atomically transfers `amount` from `from` to `to`, guaranteeing non-negative balances.
    ///
    /// Returns `(new_from_balance, new_to_balance)`. If any validation fails, neither balance is
    /// modified.
    pub fn pay(&mut self, from: Uuid, to: Uuid, amount: f64) -> Result<(f64, f64)> {
        if from == to {
            return Err(PluginError::CannotPaySelf);
        }
        let amount = Self::validate_positive_amount(amount)?;
        let from_bal = self.balance(from);
        if from_bal + 1e-12 < amount {
            return Err(PluginError::InsufficientFunds {
                balance: from_bal,
                required: amount,
            });
        }
        let to_bal = self.balance(to);
        let next_from = Self::normalize_balance(from_bal - amount);
        let next_to = Self::normalize_balance(to_bal + amount);
        if !next_to.is_finite() {
            return Err(PluginError::InvalidAmount(amount));
        }
        self.balances.insert(from, next_from);
        self.balances.insert(to, next_to);
        Ok((next_from, next_to))
    }

    // ── Chat Mute Tracking ──────────────────────────────────────────────────────────────

    /// Mutes `player` permanently or for `duration`, using the system clock.
    pub fn mute(&mut self, player: Uuid, reason: Option<String>, duration: Option<Duration>) {
        self.mute_at(player, reason, duration, Self::now_unix_ms());
    }

    /// Mutes `player` at explicit timestamp `now_ms`.
    pub fn mute_at(
        &mut self,
        player: Uuid,
        reason: Option<String>,
        duration: Option<Duration>,
        now_ms: u64,
    ) {
        let expires_at_ms = duration.map(|d| {
            let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
            now_ms.saturating_add(ms)
        });
        self.mutes.insert(
            player,
            MuteRecord {
                reason,
                muted_at_ms: now_ms,
                expires_at_ms,
            },
        );
    }

    /// Unmutes `player`, returning `true` if they had a mute record.
    pub fn unmute(&mut self, player: Uuid) -> bool {
        self.mutes.remove(&player).is_some()
    }

    /// Returns `true` if `player` is currently muted using the system clock.
    pub fn is_muted(&self, player: Uuid) -> bool {
        self.is_muted_at(player, Self::now_unix_ms())
    }

    /// Returns `true` if `player` is muted at timestamp `now_ms`.
    pub fn is_muted_at(&self, player: Uuid, now_ms: u64) -> bool {
        self.mutes
            .get(&player)
            .is_some_and(|m| m.is_active_at(now_ms))
    }

    /// Returns active mute details for `player` at `now_ms`.
    pub fn mute_info_at(&self, player: Uuid, now_ms: u64) -> Option<&MuteRecord> {
        self.mutes.get(&player).filter(|m| m.is_active_at(now_ms))
    }

    /// Removes expired mutes using the system clock and returns how many were removed.
    pub fn cleanup_expired_mutes(&mut self) -> usize {
        self.cleanup_expired_mutes_at(Self::now_unix_ms())
    }

    /// Removes expired mutes at `now_ms` and returns how many were removed.
    pub fn cleanup_expired_mutes_at(&mut self, now_ms: u64) -> usize {
        let before = self.mutes.len();
        self.mutes.retain(|_, m| m.is_active_at(now_ms));
        before - self.mutes.len()
    }

    // ── Back / God / Fly ────────────────────────────────────────────────────────────────

    /// Records `player`'s previous location for `/back`.
    pub fn set_last_location(&mut self, player: Uuid, location: LocationSpec) {
        self.last_locations.insert(player, location);
    }

    /// Returns `player`'s last recorded location for `/back`, if any.
    pub fn last_location(&self, player: Uuid) -> Option<&LocationSpec> {
        self.last_locations.get(&player)
    }

    /// Toggles invulnerable god mode for `player`, returning the new state (`true` = enabled).
    pub fn toggle_god(&mut self, player: Uuid) -> bool {
        if self.god_mode.contains(&player) {
            self.god_mode.remove(&player);
            false
        } else {
            self.god_mode.insert(player);
            true
        }
    }

    /// Returns `true` if `player` currently has god mode enabled.
    #[inline]
    pub fn is_god(&self, player: Uuid) -> bool {
        self.god_mode.contains(&player)
    }

    /// Toggles flight mode for `player`, returning the new state (`true` = enabled).
    pub fn toggle_fly(&mut self, player: Uuid) -> bool {
        if self.fly_mode.contains(&player) {
            self.fly_mode.remove(&player);
            false
        } else {
            self.fly_mode.insert(player);
            true
        }
    }

    /// Returns `true` if `player` currently has flight mode enabled.
    #[inline]
    pub fn is_fly(&self, player: Uuid) -> bool {
        self.fly_mode.contains(&player)
    }

    // ── Persistence ─────────────────────────────────────────────────────────────────────

    /// Saves essentials state atomically as pretty JSON.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        save_json_atomic(path.as_ref(), self)
    }

    /// Loads essentials state from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        load_json(path.as_ref())
    }

    /// Loads essentials state from a JSON file, or returns defaults if missing.
    pub fn load_or_default(path: impl AsRef<Path>) -> Result<Self> {
        load_json_or_default(path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn homes_warps_and_spawn() {
        let mut ess = EssentialsEngine::new();
        let p = Uuid::new_v4();
        let base = LocationSpec::new("world", 10.5, 64.0, -20.25, 90.0, 0.0);
        let nether = LocationSpec::new("world_nether", 0.0, 70.0, 0.0, 0.0, 0.0);

        ess.set_home(p, "Base", base.clone()).unwrap();
        ess.set_home(p, "nether", nether.clone()).unwrap();
        assert_eq!(ess.home(p, "base"), Some(&base));
        assert_eq!(
            ess.list_homes(p),
            vec!["base".to_owned(), "nether".to_owned()]
        );
        assert!(ess.del_home(p, "base").unwrap());
        assert_eq!(ess.home(p, "base"), None);

        ess.set_warp("Shop", base.clone()).unwrap();
        assert_eq!(ess.warp("shop"), Some(&base));
        assert_eq!(ess.list_warps(), vec!["shop".to_owned()]);
        assert!(ess.del_warp("shop").unwrap());
        assert_eq!(ess.warp("shop"), None);

        assert_eq!(ess.spawn(), None);
        ess.set_spawn(nether.clone());
        assert_eq!(ess.spawn(), Some(&nether));
    }

    #[test]
    fn tpa_ttl_and_expiry_cleanup() {
        let mut ess = EssentialsEngine::with_tpa_ttl(Duration::from_secs(30));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        assert!(matches!(
            ess.request_tpa_at(a, a, 1_000),
            Err(PluginError::CannotTpaSelf)
        ));

        ess.request_tpa_at(a, b, 1_000).unwrap();
        // Still valid before 31_000 ms.
        let accepted = ess.accept_tpa_at(b, None, 30_999).unwrap();
        assert_eq!(accepted.requester, a);
        assert_eq!(accepted.target, b);

        // Expired request returns TpaExpired and cleans up.
        ess.request_tpa_at(a, b, 1_000).unwrap();
        assert!(matches!(
            ess.accept_tpa_at(b, None, 31_000),
            Err(PluginError::TpaExpired)
        ));
        assert!(matches!(
            ess.accept_tpa_at(b, None, 31_001),
            Err(PluginError::NoPendingTpa)
        ));

        // Explicit cleanup removes stale requests.
        ess.request_tpa_at(a, b, 5_000).unwrap();
        assert_eq!(ess.cleanup_expired_tpas_at(20_000), 0);
        assert_eq!(ess.cleanup_expired_tpas_at(35_000), 1);
    }

    #[test]
    fn economy_atomic_checks_and_mutes() {
        let mut ess = EssentialsEngine::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        assert!((ess.balance(alice) - 0.0).abs() < 1e-9);
        assert!(matches!(
            ess.deposit(alice, -5.0),
            Err(PluginError::InvalidAmount(_))
        ));
        assert!(matches!(
            ess.deposit(alice, f64::NAN),
            Err(PluginError::InvalidAmount(_))
        ));

        ess.deposit(alice, 100.0).unwrap();
        assert!((ess.balance(alice) - 100.0).abs() < 1e-9);

        // Atomic pay failure leaves both balances unchanged.
        assert!(matches!(
            ess.pay(alice, bob, 150.0),
            Err(PluginError::InsufficientFunds { .. })
        ));
        assert!((ess.balance(alice) - 100.0).abs() < 1e-9);
        assert!((ess.balance(bob) - 0.0).abs() < 1e-9);

        assert!(matches!(
            ess.pay(alice, alice, 10.0),
            Err(PluginError::CannotPaySelf)
        ));

        let (a_bal, b_bal) = ess.pay(alice, bob, 40.0).unwrap();
        assert!((a_bal - 60.0).abs() < 1e-9);
        assert!((b_bal - 40.0).abs() < 1e-9);

        let after_withdraw = ess.withdraw(alice, 60.0).unwrap();
        assert!((after_withdraw - 0.0).abs() < 1e-9);

        // Mute tracking with expiry.
        ess.mute_at(
            alice,
            Some("spam".into()),
            Some(Duration::from_secs(10)),
            1_000,
        );
        assert!(ess.is_muted_at(alice, 5_000));
        assert!(!ess.is_muted_at(alice, 11_000));
        assert_eq!(ess.cleanup_expired_mutes_at(11_000), 1);
        assert!(ess.mute_info_at(alice, 5_000).is_none());

        ess.mute_at(bob, None, None, 2_000);
        assert!(ess.is_muted_at(bob, 999_999_999));
        assert!(ess.unmute(bob));
        assert!(!ess.is_muted_at(bob, 999_999_999));
    }

    #[test]
    fn json_round_trip() {
        let mut ess = EssentialsEngine::with_tpa_ttl(Duration::from_secs(45));
        let p = Uuid::new_v4();
        ess.set_spawn(LocationSpec::from_pos("world", [0.0, 64.0, 0.0]));
        ess.set_home(
            p,
            "home",
            LocationSpec::from_pos("world", [10.0, 70.0, 10.0]),
        )
        .unwrap();
        ess.set_warp("hub", LocationSpec::from_pos("world", [1.0, 65.0, 1.0]))
            .unwrap();
        ess.deposit(p, 250.5).unwrap();
        ess.mute_at(p, Some("timeout".into()), None, 500);

        let file = NamedTempFile::new().unwrap();
        ess.save_to_file(file.path()).unwrap();
        let loaded = EssentialsEngine::load_from_file(file.path()).unwrap();
        assert_eq!(ess, loaded);

        let missing_dir = tempfile::tempdir().unwrap();
        let empty =
            EssentialsEngine::load_or_default(missing_dir.path().join("missing.json")).unwrap();
        assert_eq!(empty, EssentialsEngine::default());
    }

    #[test]
    fn tpa_deny_and_specific_requester() {
        let mut ess = EssentialsEngine::with_tpa_ttl(Duration::from_secs(60));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let target = Uuid::new_v4();

        ess.request_tpa_at(a, target, 1_000).unwrap();
        ess.request_tpa_at(b, target, 2_000).unwrap();

        let denied = ess.deny_tpa_at(target, Some(a), 3_000).unwrap();
        assert_eq!(denied.requester, a);

        // `b`'s request remains pending and can be accepted.
        let accepted = ess.accept_tpa_at(target, None, 4_000).unwrap();
        assert_eq!(accepted.requester, b);
        assert!(matches!(
            ess.accept_tpa_at(target, None, 5_000),
            Err(PluginError::NoPendingTpa)
        ));
    }
}
