//! A pure-Rust implementation of Quilt Loader's front end: version semantics,
//! version-constraint matching, `quilt.mod.json` / `fabric.mod.json` parsing,
//! jar discovery, and dependency resolution. None of this needs a JVM.
//!
//! This is the JVM-free half of Quilt Loader. Everything up to and including the
//! decision of *which* mods to load and *why* one is rejected is metadata logic
//! that can be reproduced faithfully in Rust; only the steps that execute mod
//! bytecode (class loading, Mixin application, entrypoint invocation) still need
//! a bytecode runtime and stay outside this crate. Keeping this boundary sharp
//! lets `PatchQuilt` replace the Java loader front end incrementally while reusing
//! the existing host only for the parts that genuinely require the JVM.

pub mod constraint;
pub mod discovery;
pub mod metadata;
pub mod resolve;
pub mod solve;
pub mod version;

use std::path::Path;

pub use metadata::{ModCandidate, ModFormat};
pub use resolve::{BuiltinMod, Resolution, ResolutionError, default_builtins};
pub use version::Version;

/// A discovered jar that could not be parsed, kept alongside the resolution so
/// callers can surface both classes of problem together.
#[derive(Debug)]
pub struct MalformedMod {
    pub path: std::path::PathBuf,
    pub error: String,
}

/// The full outcome of planning a mods directory.
#[derive(Debug)]
pub struct LoadPlan {
    /// Jars that produced valid metadata but may still fail resolution.
    pub resolution: Resolution,
    /// Jars that could not be read or parsed at all.
    pub malformed: Vec<MalformedMod>,
}

impl LoadPlan {
    /// Whether every jar parsed and the resolved graph is internally consistent.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.malformed.is_empty() && self.resolution.is_ok()
    }

    /// All problems as user-facing lines, malformed jars first.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .malformed
            .iter()
            .map(|entry| format!("{}: {}", entry.path.display(), entry.error))
            .collect();
        lines.extend(self.resolution.errors.iter().map(ResolutionError::message));
        lines
    }
}

/// Discover, parse, and resolve every mod in `mods_dir` against `builtins`.
///
/// This is the end-to-end entry point equivalent to the discovery and solve
/// phases of Quilt Loader, minus the class-loading it hands off to the JVM.
/// Resolution uses the multi-version [`solve`] search, so several jars sharing a
/// mod id resolve to a single chosen version rather than being rejected.
///
/// # Errors
/// Returns an error only when `mods_dir` cannot be read; malformed jars and
/// resolution failures are reported inside the returned [`LoadPlan`].
pub fn plan_mods(mods_dir: &Path, builtins: &[BuiltinMod]) -> Result<LoadPlan, String> {
    let discovered = discovery::discover_dir(mods_dir)?;
    let mut candidates = Vec::new();
    let mut malformed = Vec::new();
    for entry in discovered {
        match entry.result {
            Ok(candidate) => candidates.push(candidate),
            Err(error) => malformed.push(MalformedMod {
                path: entry.path,
                error,
            }),
        }
    }
    let resolution = solve::solve(candidates, builtins);
    Ok(LoadPlan {
        resolution,
        malformed,
    })
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

    #[test]
    fn plans_a_directory_end_to_end() {
        let dir = std::env::temp_dir().join("patchquilt-plan-end-to-end");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        write_jar(
            &dir.join("lifecycle.jar"),
            "quilt.mod.json",
            br#"{
                "schema_version": 1,
                "quilt_loader": {
                    "id": "lifecycle_test",
                    "version": "1.0.0",
                    "depends": [
                        { "id": "quilt_loader", "versions": ">=0.30.0" },
                        { "id": "minecraft", "versions": "26.2" }
                    ]
                }
            }"#,
        );
        write_jar(&dir.join("broken.jar"), "note.txt", b"not a mod");

        let plan = plan_mods(&dir, &default_builtins()).expect("planning succeeds");
        assert_eq!(plan.resolution.loaded.len(), 1);
        assert_eq!(plan.malformed.len(), 1);
        assert!(!plan.is_ok());
        assert!(plan.problems().iter().any(|line| line.contains("broken.jar")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
