//! Mod discovery: scan a directory of jars and read each mod's metadata.
//!
//! Quilt reads `quilt.mod.json` from the root of a jar, falling back to
//! `fabric.mod.json` for Fabric mods. This module reproduces that lookup order
//! over a mods directory and reports a per-jar result so one broken jar does not
//! hide the mods around it.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::metadata::ModCandidate;

/// The result of inspecting a single jar.
#[derive(Debug)]
pub struct DiscoveredMod {
    pub path: PathBuf,
    pub result: Result<ModCandidate, String>,
}

/// Discover every `.jar` in `dir`, sorted by path for deterministic ordering.
///
/// # Errors
/// Returns an error only when the directory itself cannot be read; individual
/// jar failures are captured in each [`DiscoveredMod::result`].
pub fn discover_dir(dir: &Path) -> Result<Vec<DiscoveredMod>, String> {
    let mut jar_paths = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("failed to read mods directory {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read directory entry: {error}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "jar") {
            jar_paths.push(path);
        }
    }
    jar_paths.sort();

    Ok(jar_paths
        .into_iter()
        .map(|path| {
            let result = read_jar_metadata(&path);
            DiscoveredMod { path, result }
        })
        .collect())
}

/// Read and parse the mod metadata from a single jar file.
///
/// # Errors
/// Returns a message when the jar cannot be opened or contains no recognised
/// metadata file.
pub fn read_jar_metadata(path: &Path) -> Result<ModCandidate, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("{} is not a valid jar: {error}", path.display()))?;

    for name in ["quilt.mod.json", "fabric.mod.json"] {
        if let Some(bytes) = read_entry(&mut archive, name)? {
            return ModCandidate::parse(&bytes)
                .map_err(|error| format!("{}: {error}", path.display()));
        }
    }
    Err(format!(
        "{} contains no quilt.mod.json or fabric.mod.json",
        path.display()
    ))
}

fn read_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<Option<Vec<u8>>, String> {
    match archive.by_name(name) {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| format!("failed to read {name}: {error}"))?;
            Ok(Some(bytes))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(format!("failed to open {name}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_jar(path: &Path, entry_name: &str, contents: &[u8]) {
        let file = std::fs::File::create(path).expect("create jar");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(entry_name, zip::write::SimpleFileOptions::default())
            .expect("start entry");
        writer.write_all(contents).expect("write entry");
        writer.finish().expect("finish jar");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("patchquilt-discovery-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn discovers_quilt_and_fabric_jars_and_reports_bad_ones() {
        let dir = temp_dir("mixed");
        write_jar(
            &dir.join("a-quilt.jar"),
            "quilt.mod.json",
            br#"{"schema_version":1,"quilt_loader":{"id":"quilt_mod","version":"1.0.0"}}"#,
        );
        write_jar(
            &dir.join("b-fabric.jar"),
            "fabric.mod.json",
            br#"{"schemaVersion":1,"id":"fabric_mod","version":"2.0.0"}"#,
        );
        write_jar(&dir.join("c-empty.jar"), "README.txt", b"nothing here");

        let discovered = discover_dir(&dir).expect("discovery succeeds");
        assert_eq!(discovered.len(), 3);
        assert_eq!(
            discovered[0].result.as_ref().map(|candidate| candidate.id.clone()),
            Ok("quilt_mod".to_owned())
        );
        assert_eq!(
            discovered[1].result.as_ref().map(|candidate| candidate.id.clone()),
            Ok("fabric_mod".to_owned())
        );
        assert!(discovered[2].result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
