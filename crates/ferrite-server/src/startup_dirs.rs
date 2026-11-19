//! Default server directory layout created on startup.
//!
//! Ferrite creates only the directories it actually owns:
//!
//! - `<root>/config` — server configuration directory loaded by `ferrite-config`
//! - `<world>/` (resolved against `<root>` when relative, or used as-is when absolute)
//!   and its Overworld subdirectories:
//!   - `<world>/data` — world saved data (game rules, chunk tickets, saved maps)
//!   - `<world>/datapacks` — server datapacks and `/datapack` state
//!   - `<world>/dimensions/minecraft/overworld/region` — Overworld Anvil region files
//!   - `<world>/entities` — Overworld per-chunk entity sidecars
//!   - `<world>/playerdata` — player saves, advancements, recipes, and shoulder parrots
//! - `<root>/playerdata` instead of `<world>/playerdata` when running without an on-disk world
//! - `<worldgen>` (default `<root>/worldgen`) only when the vanilla worldgen host is enabled
//!
//! Plugin directories are intentionally never created (plugins are out of scope). Existing
//! directories and user files are never overwritten or removed.

use anyhow::Context;
use std::path::{Path, PathBuf};

use crate::game::Dimension;

/// Inputs that determine which directories Ferrite owns for a server startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupLayout<'a> {
    /// Server working directory (`.` in headless mode, executable directory in Windows GUI).
    pub root: &'a Path,
    /// Configured world directory (`None` for an in-memory world).
    pub world: Option<&'a Path>,
    /// Vanilla worldgen host working directory (`None` when `generator = "superflat"`).
    pub worldgen: Option<&'a Path>,
}

/// The resolved set of directories Ferrite ensures exist on startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupDirectories {
    pub config: PathBuf,
    pub world: Option<PathBuf>,
    pub playerdata: PathBuf,
    pub worldgen: Option<PathBuf>,
    directories: Vec<PathBuf>,
}

impl StartupDirectories {
    /// Compute the exact directories Ferrite owns for `layout` without touching disk.
    pub fn plan(layout: &StartupLayout<'_>) -> Self {
        let config = resolve(layout.root, Path::new("config"));
        let mut directories = vec![config.clone()];

        let (world, playerdata) = match layout.world {
            Some(world_path) => {
                let world = resolve(layout.root, world_path);
                let data = world.join("data");
                let datapacks = world.join("datapacks");
                let region = world.join(Dimension::Overworld.region_dir());
                let entities = world.join(Dimension::Overworld.entity_dir());
                let playerdata = world.join("playerdata");
                directories.push(world.clone());
                directories.push(data);
                directories.push(datapacks);
                directories.push(region);
                directories.push(entities);
                directories.push(playerdata.clone());
                (Some(world), playerdata)
            }
            None => {
                let playerdata = resolve(layout.root, Path::new("playerdata"));
                directories.push(playerdata.clone());
                (None, playerdata)
            }
        };

        let worldgen = layout.worldgen.map(|path| {
            let dir = resolve(layout.root, path);
            directories.push(dir.clone());
            dir
        });

        Self {
            config,
            world,
            playerdata,
            worldgen,
            directories,
        }
    }

    /// All directories to ensure exist, in creation order.
    pub fn all(&self) -> &[PathBuf] {
        &self.directories
    }

    /// Create every missing directory in this plan without overwriting existing files or
    /// directories. Returns the subset of directories that were newly created.
    pub fn ensure(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut created = Vec::new();
        for dir in &self.directories {
            if dir.is_dir() {
                continue;
            }
            if dir.exists() {
                anyhow::bail!(
                    "expected startup directory {} is an existing non-directory path",
                    dir.display()
                );
            }
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating startup directory {}", dir.display()))?;
            created.push(dir.clone());
        }
        Ok(created)
    }
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_creates_expected_directories_without_plugins() {
        let root = tempfile::tempdir().unwrap();
        let plan = StartupDirectories::plan(&StartupLayout {
            root: root.path(),
            world: Some(Path::new("world")),
            worldgen: Some(Path::new("worldgen")),
        });

        let created = plan.ensure().unwrap();
        assert_eq!(created, plan.all());

        for expected in [
            "config",
            "world",
            "world/data",
            "world/datapacks",
            "world/dimensions/minecraft/overworld/region",
            "world/entities",
            "world/playerdata",
            "worldgen",
        ] {
            assert!(
                root.path().join(expected).is_dir(),
                "expected {expected} to exist"
            );
        }

        for excluded in [
            "plugins",
            "logs",
            "debug",
            "vanilla",
            "world/DIM-1",
            "world/DIM1",
        ] {
            assert!(
                !root.path().join(excluded).exists(),
                "did not expect {excluded} to be created"
            );
        }

        // Running again is a no-op.
        let second = plan.ensure().unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn existing_install_creates_missing_optional_dirs_and_preserves_user_files() {
        let root = tempfile::tempdir().unwrap();
        let world = root.path().join("world");
        let playerdata = world.join("playerdata");
        let config = root.path().join("config");
        std::fs::create_dir_all(&playerdata).unwrap();
        std::fs::create_dir_all(&config).unwrap();

        let level_dat = world.join("level.dat");
        let player_file = playerdata.join("player.json");
        let config_file = config.join("paper-global.yml");
        std::fs::write(&level_dat, b"existing-level").unwrap();
        std::fs::write(&player_file, b"{\"name\":\"Alex\"}").unwrap();
        std::fs::write(&config_file, b"custom: true\n").unwrap();

        let plan = StartupDirectories::plan(&StartupLayout {
            root: root.path(),
            world: Some(Path::new("world")),
            worldgen: Some(Path::new("worldgen")),
        });
        let created = plan.ensure().unwrap();

        assert!(created.contains(&world.join("data")));
        assert!(created.contains(&world.join("datapacks")));
        assert!(created.contains(&world.join("dimensions/minecraft/overworld/region")));
        assert!(created.contains(&world.join("entities")));
        assert!(created.contains(&root.path().join("worldgen")));
        assert!(!created.contains(&config));
        assert!(!created.contains(&world));
        assert!(!created.contains(&playerdata));

        assert_eq!(std::fs::read(&level_dat).unwrap(), b"existing-level");
        assert_eq!(std::fs::read(&player_file).unwrap(), b"{\"name\":\"Alex\"}");
        assert_eq!(std::fs::read(&config_file).unwrap(), b"custom: true\n");
    }

    #[test]
    fn custom_relative_and_absolute_paths_are_honored() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();

        let relative_plan = StartupDirectories::plan(&StartupLayout {
            root: root.path(),
            world: Some(Path::new("worlds/survival")),
            worldgen: Some(Path::new("cache/worldgen")),
        });
        relative_plan.ensure().unwrap();

        assert!(root.path().join("config").is_dir());
        assert!(root.path().join("worlds/survival/datapacks").is_dir());
        assert!(root.path().join("worlds/survival/playerdata").is_dir());
        assert!(root.path().join("cache/worldgen").is_dir());
        assert!(!root.path().join("world").exists());
        assert!(!root.path().join("worldgen").exists());
        assert!(!root.path().join("plugins").exists());

        let abs_world = external.path().join("abs-world");
        let abs_worldgen = external.path().join("abs-worldgen");
        let absolute_plan = StartupDirectories::plan(&StartupLayout {
            root: root.path(),
            world: Some(&abs_world),
            worldgen: Some(&abs_worldgen),
        });
        absolute_plan.ensure().unwrap();

        assert!(abs_world.join("data").is_dir());
        assert!(abs_world.join("datapacks").is_dir());
        assert!(
            abs_world
                .join("dimensions/minecraft/overworld/region")
                .is_dir()
        );
        assert!(abs_world.join("entities").is_dir());
        assert!(abs_world.join("playerdata").is_dir());
        assert!(abs_worldgen.is_dir());
        assert!(!root.path().join("world").exists());
    }

    #[test]
    fn superflat_and_in_memory_modes_omit_unowned_directories() {
        let superflat_root = tempfile::tempdir().unwrap();
        let superflat = StartupDirectories::plan(&StartupLayout {
            root: superflat_root.path(),
            world: Some(Path::new("world")),
            worldgen: None,
        });
        superflat.ensure().unwrap();
        assert!(superflat_root.path().join("world/playerdata").is_dir());
        assert!(!superflat_root.path().join("worldgen").exists());
        assert!(!superflat_root.path().join("plugins").exists());

        let memory_root = tempfile::tempdir().unwrap();
        let memory = StartupDirectories::plan(&StartupLayout {
            root: memory_root.path(),
            world: None,
            worldgen: None,
        });
        memory.ensure().unwrap();
        assert!(memory_root.path().join("config").is_dir());
        assert!(memory_root.path().join("playerdata").is_dir());
        assert!(!memory_root.path().join("world").exists());
        assert!(!memory_root.path().join("worldgen").exists());
        assert!(!memory_root.path().join("plugins").exists());
    }

    #[test]
    fn existing_file_at_directory_path_fails_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let conflict = root.path().join("config");
        std::fs::write(&conflict, b"do-not-overwrite").unwrap();

        let plan = StartupDirectories::plan(&StartupLayout {
            root: root.path(),
            world: Some(Path::new("world")),
            worldgen: None,
        });
        let error = plan.ensure().unwrap_err().to_string();
        assert!(error.contains("non-directory"), "{error}");
        assert_eq!(std::fs::read(&conflict).unwrap(), b"do-not-overwrite");
    }

    #[test]
    fn simulated_gui_root_resolves_relative_paths_against_executable_directory() {
        let base = tempfile::tempdir().unwrap();
        let exe_dir = base.path().join("portable-install");
        std::fs::create_dir_all(&exe_dir).unwrap();

        let plan = StartupDirectories::plan(&StartupLayout {
            root: &exe_dir,
            world: Some(Path::new("world")),
            worldgen: Some(Path::new("worldgen")),
        });
        plan.ensure().unwrap();

        assert!(exe_dir.join("config").is_dir());
        assert!(exe_dir.join("world/datapacks").is_dir());
        assert!(exe_dir.join("worldgen").is_dir());
        assert!(!base.path().join("config").exists());
        assert!(!base.path().join("world").exists());
    }
}
