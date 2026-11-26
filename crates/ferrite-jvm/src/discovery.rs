//! Locating a Java 25+ installation and its JVM shared library.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{JvmError, Result};

/// Minimum supported Java feature version.
pub const MIN_JAVA_VERSION: u32 = 25;

/// A Java installation whose `release` file reports [`MIN_JAVA_VERSION`] or newer.
#[derive(Debug, Clone)]
pub struct JavaHome {
    root: PathBuf,
    version: u32,
}

impl JavaHome {
    /// Root of the installation: the directory containing `bin/`, `lib/` and `release`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Java feature version, for example `25` for `JAVA_VERSION="25.0.5"`.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Path of the JVM shared library (`jvm.dll`, `libjvm.so` or `libjvm.dylib`).
    ///
    /// The library is loaded at runtime; nothing links against it.
    pub fn jvm_library(&self) -> Result<PathBuf> {
        #[cfg(windows)]
        const RELATIVE_PATHS: &[&str] = &[
            "bin/server/jvm.dll",
            "bin/client/jvm.dll",
            "jre/bin/server/jvm.dll",
        ];
        #[cfg(target_os = "macos")]
        const RELATIVE_PATHS: &[&str] = &["lib/server/libjvm.dylib"];
        #[cfg(all(unix, not(target_os = "macos")))]
        const RELATIVE_PATHS: &[&str] = &[
            "lib/server/libjvm.so",
            "lib/amd64/server/libjvm.so",
            "jre/lib/server/libjvm.so",
        ];

        for relative in RELATIVE_PATHS {
            let candidate = self.root.join(relative);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(JvmError::JvmLibraryMissing {
            root: self.root.clone(),
        })
    }

    /// Path of the `java` launcher.
    pub fn java_binary(&self) -> PathBuf {
        self.binary("java")
    }

    /// Path of the `javac` compiler. Only JDKs ship one.
    pub fn javac(&self) -> PathBuf {
        self.binary("javac")
    }

    fn binary(&self, name: &str) -> PathBuf {
        #[cfg(windows)]
        {
            self.root.join("bin").join(format!("{name}.exe"))
        }
        #[cfg(not(windows))]
        {
            self.root.join("bin").join(name)
        }
    }

    /// Validates `root` as a Java [`MIN_JAVA_VERSION`]+ home.
    fn from_root(root: &Path) -> Result<Self> {
        let found = read_release_version(root);
        let version = found.as_deref().and_then(parse_major_version);
        match version {
            Some(version) if version >= MIN_JAVA_VERSION => Ok(Self {
                root: root.to_owned(),
                version,
            }),
            _ => Err(JvmError::UnsupportedJava {
                path: root.to_owned(),
                found,
            }),
        }
    }
}

/// Finds a Java [`MIN_JAVA_VERSION`]+ installation.
///
/// Search order: `explicit` (a hard error when it is unusable), `JAVA_HOME`, then the common
/// installation directories — on Windows `C:/Program Files/Eclipse Adoptium`,
/// `C:/Program Files/Java`, `C:/Program Files/Microsoft` and `C:/Program Files/Amazon Corretto`;
/// on Linux `/usr/lib/jvm`; on macOS `/Library/Java/JavaVirtualMachines/*/Contents/Home`.
/// Candidates are tried newest version first.
pub fn find_java_home(explicit: Option<&Path>) -> Result<JavaHome> {
    if let Some(root) = explicit {
        return JavaHome::from_root(root);
    }

    let mut tried = Vec::new();
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        let java_home = PathBuf::from(java_home);
        match JavaHome::from_root(&java_home) {
            Ok(java) => return Ok(java),
            Err(error) => {
                tracing::debug!(path = %java_home.display(), %error, "ignoring JAVA_HOME");
                tried.push(format!("JAVA_HOME={} ({error})", java_home.display()));
            }
        }
    }

    for root in common_java_homes() {
        match JavaHome::from_root(&root) {
            Ok(java) => {
                tracing::debug!(path = %root.display(), version = java.version(), "found Java");
                return Ok(java);
            }
            Err(error) => tried.push(format!("{} ({error})", root.display())),
        }
    }

    Err(JvmError::JavaNotFound { tried })
}

/// Candidate installation roots from the platform's usual locations, newest first.
fn common_java_homes() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(windows)]
    {
        const BASE_DIRS: &[&str] = &[
            "C:/Program Files/Eclipse Adoptium",
            "C:/Program Files/Java",
            "C:/Program Files/Microsoft",
            "C:/Program Files/Amazon Corretto",
        ];
        for base in BASE_DIRS {
            roots.extend(read_dir_paths(Path::new(base)));
        }
    }
    #[cfg(target_os = "macos")]
    {
        for entry in read_dir_paths(Path::new("/Library/Java/JavaVirtualMachines")) {
            roots.push(entry.join("Contents/Home"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        roots.extend(read_dir_paths(Path::new("/usr/lib/jvm")));
    }

    let mut candidates: Vec<(u32, String, PathBuf)> = roots
        .into_iter()
        .filter_map(|root| {
            let version = read_release_version(&root)?;
            let major = parse_major_version(&version)?;
            (major >= MIN_JAVA_VERSION).then_some((major, version, root))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    candidates.into_iter().map(|(_, _, root)| root).collect()
}

fn read_dir_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries.flatten().map(|entry| entry.path()).collect()
}

/// Reads `JAVA_VERSION` from a JDK's `release` file, for example `25.0.5`.
fn read_release_version(root: &Path) -> Option<String> {
    let release = fs::read_to_string(root.join("release")).ok()?;
    release.lines().find_map(|line| {
        line.strip_prefix("JAVA_VERSION=")
            .map(|value| value.trim().trim_matches('"').to_owned())
    })
}

/// Feature version of a `JAVA_VERSION` string: `"25.0.5"` becomes 25, `"1.8.0_392"` becomes 8.
fn parse_major_version(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_feature_versions() {
        assert_eq!(parse_major_version("25.0.5"), Some(25));
        assert_eq!(parse_major_version("1.8.0_392"), Some(8));
        assert_eq!(parse_major_version("17"), Some(17));
        assert_eq!(parse_major_version("23.0.1+11"), Some(23));
        assert_eq!(parse_major_version("not-a-version"), None);
        assert_eq!(parse_major_version(""), None);
    }

    #[test]
    fn accepts_a_supported_release_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("release"),
            "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"25.0.5\"\n",
        )
        .unwrap();
        let java = JavaHome::from_root(dir.path()).unwrap();
        assert_eq!(java.version(), 25);
        assert_eq!(java.root(), dir.path());
    }

    #[test]
    fn rejects_an_older_release_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("release"), "JAVA_VERSION=\"22.0.2\"\n").unwrap();
        let error = JavaHome::from_root(dir.path()).unwrap_err();
        assert!(matches!(
            error,
            JvmError::UnsupportedJava { found: Some(version), .. } if version == "22.0.2"
        ));
    }

    #[test]
    fn rejects_a_directory_without_a_release_file() {
        let dir = tempfile::tempdir().unwrap();
        let error = JavaHome::from_root(dir.path()).unwrap_err();
        assert!(matches!(
            error,
            JvmError::UnsupportedJava { found: None, .. }
        ));
    }
}
