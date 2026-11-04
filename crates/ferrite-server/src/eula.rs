//! The `eula.txt` gate.
//!
//! Ferrite never accepts the Mojang EULA on anyone's behalf: the file is created with
//! `eula=false` and the user changes it themselves. The world generation host runs the user's
//! own Minecraft server jar and reads the very same file, so there is a single acceptance.
use anyhow::Context;
use std::path::Path;

const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

/// Whether `contents` is an `eula.txt` with `eula=true` (comments and case are ignored).
pub fn is_accepted(contents: &str) -> bool {
    contents.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && line.split_once('=').is_some_and(|(key, value)| {
                key.trim().eq_ignore_ascii_case("eula") && value.trim().eq_ignore_ascii_case("true")
            })
    })
}

/// Returns `Ok` when `path` records acceptance. Otherwise creates the file with `eula=false`
/// (when it does not exist) and returns an error telling the user what to do.
pub fn ensure_accepted(path: &Path) -> anyhow::Result<()> {
    match std::fs::read_to_string(path) {
        Ok(contents) if is_accepted(&contents) => Ok(()),
        Ok(_) => anyhow::bail!(
            "you need to agree to the Minecraft EULA ({EULA_URL}) to run this server: set eula=true in {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(
                path,
                format!(
                    "#By changing the setting below to TRUE you are indicating your agreement to Mojang's EULA ({EULA_URL}).\neula=false\n"
                ),
            )
            .with_context(|| format!("creating {}", path.display()))?;
            anyhow::bail!(
                "you need to agree to the Minecraft EULA ({EULA_URL}) to run this server: set eula=true in {} and start again",
                path.display()
            )
        }
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_acceptance() {
        assert!(is_accepted("eula=true\n"));
        assert!(is_accepted("#comment\n EULA = TRUE \n"));
        assert!(!is_accepted("eula=false\n"));
        assert!(!is_accepted("#eula=true\n"));
        assert!(!is_accepted(""));
    }

    #[test]
    fn creates_a_declined_file_and_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eula.txt");
        assert!(ensure_accepted(&path).is_err());
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("eula=false"));
        assert!(
            ensure_accepted(&path).is_err(),
            "a declined file stays declined"
        );
        std::fs::write(&path, "eula=true\n").unwrap();
        ensure_accepted(&path).unwrap();
    }
}
