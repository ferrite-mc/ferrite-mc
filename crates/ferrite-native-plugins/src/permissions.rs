//! LuckPerms-style permissions engine.
//!
//! Supports weighted groups, multi-parent inheritance with cycle protection, per-player group
//! memberships and explicit permission overrides, hierarchical wildcard matching (`*`,
//! `ferrite.admin.*`), exact overrides beating wildcards, negation beating grants at equal
//! specificity and priority, OP fallback, and JSON persistence.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{PluginError, Result};
use crate::storage::{load_json, load_json_or_default, save_json_atomic};

/// A permission group with inheritance, weight, and permission nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGroup {
    /// Canonical lowercase group name.
    pub name: String,
    /// Priority weight (higher weight overrides lower weight).
    pub weight: i32,
    /// Parent group names inherited by this group.
    #[serde(default)]
    pub parents: BTreeSet<String>,
    /// Permission node grants (`true`) and negations (`false`).
    #[serde(default)]
    pub permissions: BTreeMap<String, bool>,
    /// Optional chat prefix for members of this group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Optional chat suffix for members of this group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

/// Per-player group memberships and explicit permission overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerPermissions {
    /// Groups this player directly belongs to.
    #[serde(default)]
    pub groups: BTreeSet<String>,
    /// Explicit per-node overrides (`true` = allow, `false` = deny).
    #[serde(default)]
    pub permissions: BTreeMap<String, bool>,
}

/// LuckPerms-style permissions store and evaluator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionsEngine {
    /// Named groups keyed by canonical lowercase name.
    #[serde(default)]
    pub groups: BTreeMap<String, PermissionGroup>,
    /// Per-player permission records keyed by player UUID.
    #[serde(default)]
    pub players: BTreeMap<Uuid, PlayerPermissions>,
}

/// Priority tier of a matching rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RuleTier {
    /// Player-direct overrides always beat group rules at equal specificity.
    is_player: bool,
    /// Group weight (or `i32::MAX` for player-direct).
    weight: i32,
    /// Negated BFS inheritance distance from the player (`0` for direct membership).
    neg_depth: i32,
}

impl PermissionsEngine {
    /// Creates an empty permissions engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Normalizes a group name or permission node; rejects empty or whitespace-containing names.
    fn normalize_identifier(raw: &str) -> Result<String> {
        let trimmed = raw.trim().to_ascii_lowercase();
        if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
            return Err(PluginError::InvalidIdentifier(raw.to_owned()));
        }
        Ok(trimmed)
    }

    /// Creates a new group with the given `weight`.
    pub fn create_group(&mut self, name: &str, weight: i32) -> Result<()> {
        let key = Self::normalize_identifier(name)?;
        if self.groups.contains_key(&key) {
            return Err(PluginError::GroupAlreadyExists(key));
        }
        self.groups.insert(
            key.clone(),
            PermissionGroup {
                name: key,
                weight,
                parents: BTreeSet::new(),
                permissions: BTreeMap::new(),
                prefix: None,
                suffix: None,
            },
        );
        Ok(())
    }

    /// Ensures a group exists, creating it with `weight` if missing or returning a mutable ref.
    pub fn get_or_create_group(&mut self, name: &str, weight: i32) -> Result<&mut PermissionGroup> {
        let key = Self::normalize_identifier(name)?;
        Ok(self
            .groups
            .entry(key.clone())
            .or_insert_with(|| PermissionGroup {
                name: key,
                weight,
                parents: BTreeSet::new(),
                permissions: BTreeMap::new(),
                prefix: None,
                suffix: None,
            }))
    }

    /// Sets or clears a group's chat prefix.
    pub fn set_group_prefix(&mut self, name: &str, prefix: Option<String>) -> Result<()> {
        let key = Self::normalize_identifier(name)?;
        let group = self
            .groups
            .get_mut(&key)
            .ok_or(PluginError::GroupNotFound(key))?;
        group.prefix = prefix;
        Ok(())
    }

    /// Sets or clears a group's chat suffix.
    pub fn set_group_suffix(&mut self, name: &str, suffix: Option<String>) -> Result<()> {
        let key = Self::normalize_identifier(name)?;
        let group = self
            .groups
            .get_mut(&key)
            .ok_or(PluginError::GroupNotFound(key))?;
        group.suffix = suffix;
        Ok(())
    }

    /// Resolves `(prefix, suffix)` from the player's highest-weight group(s).
    pub fn player_prefix_suffix(&self, player: Uuid) -> (Option<String>, Option<String>) {
        let Some(record) = self.players.get(&player) else {
            return (None, None);
        };
        let mut best_prefix: Option<(i32, &str)> = None;
        let mut best_suffix: Option<(i32, &str)> = None;
        for group_name in &record.groups {
            if let Some(group) = self.groups.get(group_name) {
                if let Some(prefix) = group.prefix.as_deref()
                    && best_prefix.is_none_or(|(w, _)| group.weight > w)
                {
                    best_prefix = Some((group.weight, prefix));
                }
                if let Some(suffix) = group.suffix.as_deref()
                    && best_suffix.is_none_or(|(w, _)| group.weight > w)
                {
                    best_suffix = Some((group.weight, suffix));
                }
            }
        }
        (
            best_prefix.map(|(_, s)| s.to_owned()),
            best_suffix.map(|(_, s)| s.to_owned()),
        )
    }

    /// Deletes a group and strips references to it from other groups and players.
    pub fn delete_group(&mut self, name: &str) -> Result<bool> {
        let key = Self::normalize_identifier(name)?;
        let existed = self.groups.remove(&key).is_some();
        if existed {
            for group in self.groups.values_mut() {
                group.parents.remove(&key);
            }
            for player in self.players.values_mut() {
                player.groups.remove(&key);
            }
        }
        Ok(existed)
    }

    /// Returns a group by name.
    pub fn get_group(&self, name: &str) -> Option<&PermissionGroup> {
        let key = Self::normalize_identifier(name).ok()?;
        self.groups.get(&key)
    }

    /// Updates a group's weight.
    pub fn set_group_weight(&mut self, name: &str, weight: i32) -> Result<()> {
        let key = Self::normalize_identifier(name)?;
        let group = self
            .groups
            .get_mut(&key)
            .ok_or_else(|| PluginError::GroupNotFound(key.clone()))?;
        group.weight = weight;
        Ok(())
    }

    /// Adds `parent` as an inherited parent of `group`, rejecting inheritance cycles.
    pub fn add_group_parent(&mut self, group: &str, parent: &str) -> Result<()> {
        let group_key = Self::normalize_identifier(group)?;
        let parent_key = Self::normalize_identifier(parent)?;

        if !self.groups.contains_key(&group_key) {
            return Err(PluginError::GroupNotFound(group_key));
        }
        if !self.groups.contains_key(&parent_key) {
            return Err(PluginError::GroupNotFound(parent_key));
        }
        if group_key == parent_key || self.group_inherits(&parent_key, &group_key) {
            return Err(PluginError::CyclicInheritance {
                group: group_key,
                parent: parent_key,
            });
        }

        if let Some(g) = self.groups.get_mut(&group_key) {
            g.parents.insert(parent_key);
        }
        Ok(())
    }

    /// Removes `parent` from `group`.
    pub fn remove_group_parent(&mut self, group: &str, parent: &str) -> Result<bool> {
        let group_key = Self::normalize_identifier(group)?;
        let parent_key = Self::normalize_identifier(parent)?;
        let g = self
            .groups
            .get_mut(&group_key)
            .ok_or(PluginError::GroupNotFound(group_key))?;
        Ok(g.parents.remove(&parent_key))
    }

    /// Sets or updates a permission node on a group (`true` = allow, `false` = deny).
    pub fn set_group_permission(&mut self, group: &str, node: &str, allow: bool) -> Result<()> {
        let group_key = Self::normalize_identifier(group)?;
        let node_key = Self::normalize_identifier(node)?;
        let g = self
            .groups
            .get_mut(&group_key)
            .ok_or(PluginError::GroupNotFound(group_key))?;
        g.permissions.insert(node_key, allow);
        Ok(())
    }

    /// Removes a permission node from a group.
    pub fn unset_group_permission(&mut self, group: &str, node: &str) -> Result<bool> {
        let group_key = Self::normalize_identifier(group)?;
        let node_key = Self::normalize_identifier(node)?;
        let g = self
            .groups
            .get_mut(&group_key)
            .ok_or(PluginError::GroupNotFound(group_key))?;
        Ok(g.permissions.remove(&node_key).is_some())
    }

    /// Adds a player to a group.
    pub fn add_player_group(&mut self, player: Uuid, group: &str) -> Result<()> {
        let group_key = Self::normalize_identifier(group)?;
        if !self.groups.contains_key(&group_key) {
            return Err(PluginError::GroupNotFound(group_key));
        }
        self.players
            .entry(player)
            .or_default()
            .groups
            .insert(group_key);
        Ok(())
    }

    /// Removes a player from a group.
    pub fn remove_player_group(&mut self, player: Uuid, group: &str) -> Result<bool> {
        let group_key = Self::normalize_identifier(group)?;
        Ok(self
            .players
            .get_mut(&player)
            .is_some_and(|p| p.groups.remove(&group_key)))
    }

    /// Sets an explicit per-player permission override (`true` = allow, `false` = deny).
    pub fn set_player_permission(&mut self, player: Uuid, node: &str, allow: bool) -> Result<()> {
        let node_key = Self::normalize_identifier(node)?;
        self.players
            .entry(player)
            .or_default()
            .permissions
            .insert(node_key, allow);
        Ok(())
    }

    /// Removes an explicit per-player permission override.
    pub fn unset_player_permission(&mut self, player: Uuid, node: &str) -> Result<bool> {
        let node_key = Self::normalize_identifier(node)?;
        Ok(self
            .players
            .get_mut(&player)
            .is_some_and(|p| p.permissions.remove(&node_key).is_some()))
    }

    /// Returns the direct groups assigned to `player`.
    pub fn player_groups(&self, player: Uuid) -> Vec<String> {
        self.players
            .get(&player)
            .map(|p| p.groups.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Checks whether `child` transitively inherits `target` (cycle-safe).
    fn group_inherits(&self, child: &str, target: &str) -> bool {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(child.to_owned());
        queue.push_back(child.to_owned());

        while let Some(curr) = queue.pop_front() {
            if let Some(group) = self.groups.get(&curr) {
                for parent in &group.parents {
                    if parent == target {
                        return true;
                    }
                    if visited.insert(parent.clone()) {
                        queue.push_back(parent.clone());
                    }
                }
            }
        }
        false
    }

    /// Resolves all reachable groups for `player` with cycle protection and shortest depth.
    fn resolve_player_groups(&self, player: Uuid) -> Vec<(&PermissionGroup, usize)> {
        let Some(record) = self.players.get(&player) else {
            return Vec::new();
        };

        let mut best_depth: HashMap<String, usize> = HashMap::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();

        for group_name in &record.groups {
            if self.groups.contains_key(group_name) && !best_depth.contains_key(group_name) {
                best_depth.insert(group_name.clone(), 0);
                queue.push_back((group_name.clone(), 0));
            }
        }

        // BFS guarantees shortest inheritance depth even if an external file introduced a cycle.
        while let Some((name, depth)) = queue.pop_front() {
            if let Some(group) = self.groups.get(&name) {
                for parent in &group.parents {
                    if self.groups.contains_key(parent) && !best_depth.contains_key(parent) {
                        best_depth.insert(parent.clone(), depth + 1);
                        queue.push_back((parent.clone(), depth + 1));
                    }
                }
            }
        }

        best_depth
            .into_iter()
            .filter_map(|(name, depth)| self.groups.get(&name).map(|g| (g, depth)))
            .collect()
    }

    /// Evaluates a permission node for `player`, returning `None` if neither player overrides
    /// nor inherited groups define a matching rule.
    ///
    /// Precedence order:
    /// 1. Higher specificity wins (`exact` > `a.b.*` > `a.*` > `*`).
    /// 2. At equal specificity, higher rule tier wins (`player override` > higher `weight` group >
    ///    shallower inheritance depth).
    /// 3. At equal specificity and equal tier, negation (`false`) beats grant (`true`).
    pub fn evaluate_permission(&self, player: Uuid, node: &str) -> Option<bool> {
        let player_record = self.players.get(&player)?;
        if player_record.permissions.is_empty() && player_record.groups.is_empty() {
            return None;
        }
        let query = Self::normalize_identifier(node).ok()?;
        let groups = if player_record.groups.is_empty() {
            Vec::new()
        } else {
            self.resolve_player_groups(player)
        };

        let eval_candidate = |candidate: &str| -> Option<bool> {
            let mut best: Option<(RuleTier, bool)> = None;
            if let Some(&val) = player_record.permissions.get(candidate) {
                let tier = RuleTier {
                    is_player: true,
                    weight: i32::MAX,
                    neg_depth: 0,
                };
                update_best(&mut best, tier, val);
            }
            for &(group, depth) in &groups {
                if let Some(&val) = group.permissions.get(candidate) {
                    let tier = RuleTier {
                        is_player: false,
                        weight: group.weight,
                        neg_depth: -(depth as i32),
                    };
                    update_best(&mut best, tier, val);
                }
            }
            best.map(|(_, allowed)| allowed)
        };

        // 1) Exact match (avoids any wildcard formatting allocation on the common path).
        if let Some(allowed) = eval_candidate(&query) {
            return Some(allowed);
        }

        // 2) `prefix.*` from longest to shortest, reusing a single buffer.
        let mut buf = String::with_capacity(query.len() + 2);
        let mut search_end = query.len();
        while let Some(dot_idx) = query[..search_end].rfind('.') {
            if dot_idx > 0 {
                buf.clear();
                buf.push_str(&query[..dot_idx]);
                buf.push_str(".*");
                if let Some(allowed) = eval_candidate(&buf) {
                    return Some(allowed);
                }
            }
            search_end = dot_idx;
        }

        // 3) Global wildcard `*`.
        if query != "*" {
            return eval_candidate("*");
        }

        None
    }

    /// Evaluates whether `player` has `node`, falling back to `is_op` when unset.
    pub fn has_permission(&self, player: Uuid, is_op: bool, node: &str) -> bool {
        self.evaluate_permission(player, node).unwrap_or(is_op)
    }

    /// Saves the permissions database atomically as pretty JSON.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        save_json_atomic(path.as_ref(), self)
    }

    /// Loads a permissions database from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        load_json(path.as_ref())
    }

    /// Loads a permissions database from a JSON file, or returns an empty one if missing.
    pub fn load_or_default(path: impl AsRef<Path>) -> Result<Self> {
        load_json_or_default(path.as_ref())
    }
}

fn update_best(best: &mut Option<(RuleTier, bool)>, tier: RuleTier, val: bool) {
    match best {
        None => *best = Some((tier, val)),
        Some((best_tier, best_val)) => {
            if tier > *best_tier || (tier == *best_tier && !val) {
                *best_tier = tier;
                *best_val = val;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn inheritance_weight_and_cycle_protection() {
        let mut perms = PermissionsEngine::new();
        perms.create_group("default", 0).unwrap();
        perms.create_group("mod", 50).unwrap();
        perms.create_group("admin", 100).unwrap();

        perms.add_group_parent("mod", "default").unwrap();
        perms.add_group_parent("admin", "mod").unwrap();

        // Direct self-cycle and indirect cycle are rejected.
        assert!(matches!(
            perms.add_group_parent("default", "default"),
            Err(PluginError::CyclicInheritance { .. })
        ));
        assert!(matches!(
            perms.add_group_parent("default", "admin"),
            Err(PluginError::CyclicInheritance { .. })
        ));

        perms
            .set_group_permission("default", "ferrite.home", true)
            .unwrap();
        perms
            .set_group_permission("default", "ferrite.kick", false)
            .unwrap();
        perms
            .set_group_permission("mod", "ferrite.kick", true)
            .unwrap();
        perms
            .set_group_permission("admin", "ferrite.admin.*", true)
            .unwrap();

        let p = Uuid::new_v4();
        perms.add_player_group(p, "admin").unwrap();

        assert!(perms.has_permission(p, false, "ferrite.home"));
        assert!(perms.has_permission(p, false, "ferrite.kick"));
        assert!(perms.has_permission(p, false, "ferrite.admin.ban"));
        assert!(!perms.has_permission(p, false, "ferrite.unknown"));
    }

    #[test]
    fn wildcard_exact_override_and_negation_tiebreak() {
        let mut perms = PermissionsEngine::new();
        perms.create_group("g1", 10).unwrap();
        perms.create_group("g2", 10).unwrap();

        perms
            .set_group_permission("g1", "ferrite.fly", true)
            .unwrap();
        perms
            .set_group_permission("g2", "ferrite.fly", false)
            .unwrap();
        perms
            .set_group_permission("g1", "ferrite.admin.*", true)
            .unwrap();
        perms
            .set_group_permission("g1", "ferrite.admin.stop", false)
            .unwrap();

        let p = Uuid::new_v4();
        perms.add_player_group(p, "g1").unwrap();
        perms.add_player_group(p, "g2").unwrap();

        // Equal weight & specificity: negation beats grant.
        assert!(!perms.has_permission(p, false, "ferrite.fly"));
        // Exact override beats wildcard.
        assert!(perms.has_permission(p, false, "ferrite.admin.ban"));
        assert!(!perms.has_permission(p, false, "ferrite.admin.stop"));

        // Player explicit override beats group exact negation.
        perms.set_player_permission(p, "ferrite.fly", true).unwrap();
        assert!(perms.has_permission(p, false, "ferrite.fly"));

        // Explicit negation beats wildcard at player level too.
        perms.set_player_permission(p, "*", true).unwrap();
        perms
            .set_player_permission(p, "ferrite.nuke", false)
            .unwrap();
        assert!(perms.has_permission(p, false, "anything.at.all"));
        assert!(!perms.has_permission(p, false, "ferrite.nuke"));
    }

    #[test]
    fn op_fallback_and_cycle_safe_loaded_json() {
        let mut perms = PermissionsEngine::new();
        let p = Uuid::new_v4();

        // Unset node falls back to is_op.
        assert!(perms.has_permission(p, true, "ferrite.op.cmd"));
        assert!(!perms.has_permission(p, false, "ferrite.op.cmd"));

        // Explicit deny overrides OP fallback.
        perms
            .set_player_permission(p, "ferrite.op.cmd", false)
            .unwrap();
        assert!(!perms.has_permission(p, true, "ferrite.op.cmd"));

        // Manually inject a cyclic parent relationship (simulating hand-edited JSON).
        perms.create_group("a", 5).unwrap();
        perms.create_group("b", 5).unwrap();
        perms
            .groups
            .get_mut("a")
            .unwrap()
            .parents
            .insert("b".into());
        perms
            .groups
            .get_mut("b")
            .unwrap()
            .parents
            .insert("a".into());
        perms
            .set_group_permission("b", "ferrite.cycle.ok", true)
            .unwrap();
        perms.add_player_group(p, "a").unwrap();
        assert!(perms.has_permission(p, false, "ferrite.cycle.ok"));
    }

    #[test]
    fn json_round_trip() {
        let mut perms = PermissionsEngine::new();
        perms.create_group("vip", 25).unwrap();
        perms
            .set_group_permission("vip", "ferrite.chat.*", true)
            .unwrap();
        let player = Uuid::new_v4();
        perms.add_player_group(player, "vip").unwrap();
        perms
            .set_player_permission(player, "ferrite.chat.color", false)
            .unwrap();

        let file = NamedTempFile::new().unwrap();
        perms.save_to_file(file.path()).unwrap();
        let loaded = PermissionsEngine::load_from_file(file.path()).unwrap();
        assert_eq!(perms, loaded);
        assert!(!loaded.has_permission(player, false, "ferrite.chat.color"));
        assert!(loaded.has_permission(player, false, "ferrite.chat.shout"));
    }

    #[test]
    fn multilevel_wildcards_delete_group_and_unset() {
        let mut perms = PermissionsEngine::new();
        perms.create_group("base", 1).unwrap();
        perms.create_group("staff", 10).unwrap();
        perms.add_group_parent("staff", "base").unwrap();

        perms
            .set_group_permission("base", "ferrite.*", false)
            .unwrap();
        perms
            .set_group_permission("staff", "ferrite.admin.*", true)
            .unwrap();

        let p = Uuid::new_v4();
        perms.add_player_group(p, "staff").unwrap();

        // More specific wildcard `ferrite.admin.*` beats `ferrite.*`.
        assert!(perms.has_permission(p, false, "ferrite.admin.ban"));
        assert!(!perms.has_permission(p, false, "ferrite.user.fly"));
        assert_eq!(perms.player_groups(p), vec!["staff".to_owned()]);

        // Unset removes explicit override and falls back cleanly.
        assert!(
            perms
                .unset_group_permission("staff", "ferrite.admin.*")
                .unwrap()
        );
        assert!(!perms.has_permission(p, false, "ferrite.admin.ban"));

        // Deleting `base` cleans up parent references on `staff`.
        assert!(perms.delete_group("base").unwrap());
        assert!(perms.get_group("staff").unwrap().parents.is_empty());
        assert!(perms.evaluate_permission(p, "ferrite.user.fly").is_none());
    }
}
