//! Compatibility classification of scanned Bukkit/Spigot/Paper/Folia/NMS API usage.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use ferrite_plugin_meta::PluginMeta;
use serde::Serialize;
use thiserror::Error;
use zip::ZipArchive;

use crate::classfile::{MethodRef, scan_class_bytes};

/// Errors produced while inspecting or converting a plugin jar.
#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid jar archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("plugin descriptor error: {0}")]
    Meta(#[from] ferrite_plugin_meta::PluginError),
}

/// Overall compatibility verdict for a scanned plugin jar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatVerdict {
    /// Only supported Paper/Spigot/Folia/NMS surfaces were detected.
    ReadyForJvmHost,
    /// Uses some unverified Bukkit/Paper classes, no hard Netty/reflection NMS blockers.
    NeedsVerification,
    /// Uses hard blockers (Netty pipeline, versioned CraftBukkit/NMS internals, etc.);
    /// porting to a Rust native Ferrite plugin is recommended.
    RecommendRustPort,
}

/// Structured compatibility report for a plugin `.jar`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatReport {
    pub plugin_name: String,
    pub plugin_version: String,
    pub main_class: String,
    pub folia_supported: bool,
    pub class_files_scanned: usize,
    pub commands: Vec<String>,
    pub detected_events: Vec<String>,
    pub supported_apis: Vec<String>,
    pub unverified_apis: Vec<String>,
    pub blockers: Vec<String>,
    pub verdict: CompatVerdict,
}

const SUPPORTED_CLASS_PREFIXES: &[&str] = &[
    "org.bukkit.Bukkit",
    "org.bukkit.Server",
    "org.bukkit.World",
    "org.bukkit.WorldBorder",
    "org.bukkit.Chunk",
    "org.bukkit.Location",
    "org.bukkit.Material",
    "org.bukkit.GameMode",
    "org.bukkit.NamespacedKey",
    "org.bukkit.Sound",
    "org.bukkit.Particle",
    "org.bukkit.block.Block",
    "org.bukkit.block.BlockFace",
    "org.bukkit.block.BlockState",
    "org.bukkit.block.data.BlockData",
    "org.bukkit.boss.BossBar",
    "org.bukkit.boss.BarColor",
    "org.bukkit.boss.BarStyle",
    "org.bukkit.boss.BarFlag",
    "org.bukkit.command.",
    "org.bukkit.configuration.",
    "org.bukkit.entity.Player",
    "org.bukkit.entity.Entity",
    "org.bukkit.entity.LivingEntity",
    "org.bukkit.entity.EntityType",
    "org.bukkit.event.Event",
    "org.bukkit.event.EventHandler",
    "org.bukkit.event.EventPriority",
    "org.bukkit.event.Listener",
    "org.bukkit.event.Cancellable",
    "org.bukkit.event.player.PlayerJoinEvent",
    "org.bukkit.event.player.PlayerQuitEvent",
    "org.bukkit.event.player.PlayerInteractEvent",
    "org.bukkit.event.player.PlayerMoveEvent",
    "org.bukkit.event.player.PlayerCommandPreprocessEvent",
    "org.bukkit.event.player.AsyncPlayerChatEvent",
    "org.bukkit.event.player.PlayerRespawnEvent",
    "org.bukkit.event.player.PlayerDeathEvent",
    "org.bukkit.event.block.BlockBreakEvent",
    "org.bukkit.event.block.BlockPlaceEvent",
    "org.bukkit.event.block.Action",
    "org.bukkit.event.entity.EntityDamageEvent",
    "org.bukkit.event.inventory.",
    "org.bukkit.inventory.",
    "org.bukkit.permissions.",
    "org.bukkit.persistence.",
    "org.bukkit.plugin.",
    "org.bukkit.scheduler.",
    "org.bukkit.scoreboard.",
    "org.bukkit.util.Vector",
    "org.bukkit.util.BoundingBox",
    "io.papermc.paper.threadedregions.scheduler.",
    "io.papermc.paper.datacomponent.",
    "io.papermc.paper.registry.",
    "net.kyori.adventure.",
    "org.bukkit.craftbukkit.CraftWorld",
    "org.bukkit.craftbukkit.entity.CraftEntity",
    "org.bukkit.craftbukkit.entity.CraftPlayer",
    "org.bukkit.craftbukkit.inventory.CraftItemStack",
    "org.bukkit.craftbukkit.util.CraftMagicNumbers",
    "net.minecraft.core.BlockPos",
    "net.minecraft.core.Direction",
    "net.minecraft.core.SectionPos",
    "net.minecraft.core.Vec3i",
    "net.minecraft.nbt.",
    "net.minecraft.network.chat.Component",
    "net.minecraft.resources.Identifier",
    "net.minecraft.server.level.ServerLevel",
    "net.minecraft.server.level.ServerPlayer",
    "net.minecraft.server.level.ServerChunkCache",
    "net.minecraft.util.UUIDUtil",
    "net.minecraft.world.entity.Entity",
    "net.minecraft.world.item.Item",
    "net.minecraft.world.item.ItemStack",
    "net.minecraft.world.level.block.Block",
    "net.minecraft.world.level.block.state.BlockState",
    "net.minecraft.world.phys.Vec3",
];

fn is_minecraft_ecosystem_class(class_name: &str) -> bool {
    class_name.starts_with("org.bukkit.")
        || class_name.starts_with("org.spigotmc.")
        || class_name.starts_with("io.papermc.")
        || class_name.starts_with("net.minecraft.")
        || class_name.starts_with("io.netty.")
        || class_name.starts_with("com.comphenix.protocol.")
}

fn is_supported_class(class_name: &str) -> bool {
    SUPPORTED_CLASS_PREFIXES.iter().any(|prefix| {
        if prefix.ends_with('.') {
            class_name.starts_with(prefix)
        } else {
            class_name == *prefix || class_name.starts_with(&format!("{prefix}$"))
        }
    })
}

fn is_hard_blocker_class(class_name: &str) -> bool {
    class_name.starts_with("io.netty.")
        || class_name.starts_with("org.bukkit.craftbukkit.v1_")
        || class_name.starts_with("net.minecraft.network.protocol.")
        || class_name.starts_with("net.minecraft.network.Connection")
        || class_name.starts_with("com.comphenix.protocol.")
        || class_name.starts_with("io.papermc.paper.plugin.lifecycle.")
}

fn is_hard_blocker_method(m: &MethodRef) -> Option<String> {
    match (m.owner.as_str(), m.name.as_str()) {
        ("org.bukkit.World", "setGameRuleValue") => Some("World#setGameRuleValue(...)".into()),
        ("org.bukkit.plugin.PluginManager", "loadPlugin") => {
            Some("PluginManager#loadPlugin(...)".into())
        }
        _ => None,
    }
}

/// Inspects a `.jar` file and produces a [`CompatReport`] along with the parsed [`PluginMeta`]
/// and optional raw `config.yml` text.
pub fn inspect_jar(
    path: &Path,
) -> Result<(PluginMeta, Option<String>, CompatReport), ConvertError> {
    let meta = ferrite_plugin_meta::read_plugin_meta(path)?;
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut class_files_scanned = 0usize;
    let mut config_yml = None;
    let mut all_classes = BTreeSet::new();
    let mut all_methods = BTreeSet::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.into_owned();
        if name == "config.yml" {
            let mut text = String::new();
            let _ = entry.read_to_string(&mut text);
            config_yml = Some(text);
        } else if name.ends_with(".class") {
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            if let Some(scan) = scan_class_bytes(&buf) {
                class_files_scanned += 1;
                all_classes.extend(scan.class_refs);
                all_methods.extend(scan.method_refs);
            }
        }
    }

    let mut detected_events = BTreeSet::new();
    let mut supported_apis = BTreeSet::new();
    let mut unverified_apis = BTreeSet::new();
    let mut blockers = BTreeSet::new();

    for cls in &all_classes {
        if (cls.starts_with("org.bukkit.event.") || cls.starts_with("io.papermc.paper.event."))
            && cls.ends_with("Event")
        {
            detected_events.insert(cls.clone());
        }
        if !is_minecraft_ecosystem_class(cls) {
            continue;
        }
        if is_hard_blocker_class(cls) {
            blockers.insert(cls.clone());
        } else if is_supported_class(cls) {
            supported_apis.insert(cls.clone());
        } else {
            unverified_apis.insert(cls.clone());
        }
    }

    for method in &all_methods {
        if let Some(blocker) = is_hard_blocker_method(method) {
            blockers.insert(blocker);
        }
    }

    let verdict = if !blockers.is_empty() {
        CompatVerdict::RecommendRustPort
    } else if !unverified_apis.is_empty() {
        CompatVerdict::NeedsVerification
    } else {
        CompatVerdict::ReadyForJvmHost
    };

    let commands = meta.commands.keys().cloned().collect();

    let report = CompatReport {
        plugin_name: meta.name.clone(),
        plugin_version: meta.version.clone(),
        main_class: meta.main.clone(),
        folia_supported: meta.folia_supported,
        class_files_scanned,
        commands,
        detected_events: detected_events.into_iter().collect(),
        supported_apis: supported_apis.into_iter().collect(),
        unverified_apis: unverified_apis.into_iter().collect(),
        blockers: blockers.into_iter().collect(),
        verdict,
    };

    Ok((meta, config_yml, report))
}
