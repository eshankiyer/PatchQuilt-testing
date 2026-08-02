//! Dependency resolution over a set of discovered candidates plus the builtin
//! mods the runtime always provides (`minecraft`, `quilt_loader`, `java`).
//!
//! Quilt Loader ultimately hands its constraint graph to a Sat4j solver so it
//! can pick between several available versions of the same mod. `PatchQuilt` does
//! not yet ship a SAT solver, so this resolver assumes a single version per id
//! and validates the resulting graph directly: every non-optional `depends`
//! clause must be satisfied by a loaded or builtin provider, no `breaks` clause
//! may match a present mod, and no id may be defined twice. That covers the
//! common single-candidate case exactly; multi-version selection is tracked as
//! explicit follow-up rather than silently approximated.

use std::collections::HashMap;

use crate::constraint::VersionSpec;
use crate::metadata::{Dependency, ModCandidate};
use crate::version::Version;

/// A mod the runtime provides without a jar, such as the game itself.
#[derive(Clone, Debug)]
pub struct BuiltinMod {
    pub id: String,
    pub version: Version,
}

impl BuiltinMod {
    #[must_use]
    pub fn new(id: &str, version: &str) -> Self {
        Self {
            id: id.to_owned(),
            version: Version::parse(version),
        }
    }
}

/// The builtins that mirror the `PatchQuilt` host environment: Pumpkin as the
/// `minecraft` game provider, Quilt Loader, and the running JVM version.
#[must_use]
pub fn default_builtins() -> Vec<BuiltinMod> {
    vec![
        BuiltinMod::new("minecraft", "26.2"),
        BuiltinMod::new("quilt_loader", "0.30.0"),
        BuiltinMod::new("java", "25"),
    ]
}

/// A single resolution failure with enough context to report to a user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionError {
    /// Two mods (or a mod and a builtin) claim the same id.
    DuplicateId { id: String },
    /// A required dependency is absent entirely.
    MissingDependency {
        mod_id: String,
        dependency: String,
        reason: Option<String>,
    },
    /// A dependency exists but no available version satisfies the constraint.
    UnsatisfiedVersion {
        mod_id: String,
        dependency: String,
        available: String,
    },
    /// A `breaks` clause matches a mod that is present.
    Breakage {
        mod_id: String,
        broken: String,
        version: String,
    },
}

impl ResolutionError {
    /// A one-line, user-facing description.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::DuplicateId { id } => format!("mod id '{id}' is defined more than once"),
            Self::MissingDependency {
                mod_id,
                dependency,
                reason,
            } => {
                let base = format!("'{mod_id}' requires '{dependency}', which is not installed");
                reason
                    .as_ref()
                    .map_or(base.clone(), |why| format!("{base} ({why})"))
            }
            Self::UnsatisfiedVersion {
                mod_id,
                dependency,
                available,
            } => format!(
                "'{mod_id}' requires a different version of '{dependency}' \
                 (installed: {available})"
            ),
            Self::Breakage {
                mod_id,
                broken,
                version,
            } => format!("'{mod_id}' is incompatible with '{broken}' {version}"),
        }
    }
}

/// The outcome of resolving a candidate set.
#[derive(Debug)]
pub struct Resolution {
    /// Candidates that participate in a valid graph, in input order.
    pub loaded: Vec<ModCandidate>,
    /// Every problem discovered; a non-empty list means resolution failed.
    pub errors: Vec<ResolutionError>,
}

impl Resolution {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Resolve `candidates` against the supplied builtins.
#[must_use]
pub fn resolve(candidates: Vec<ModCandidate>, builtins: &[BuiltinMod]) -> Resolution {
    let mut providers: HashMap<String, Vec<Version>> = HashMap::new();
    let mut errors = Vec::new();

    for builtin in builtins {
        providers
            .entry(builtin.id.clone())
            .or_default()
            .push(builtin.version.clone());
    }

    // Record every id a candidate defines or provides, detecting real duplicates.
    let mut seen_primary: HashMap<String, usize> = HashMap::new();
    for candidate in &candidates {
        let count = seen_primary.entry(candidate.id.clone()).or_insert(0);
        *count += 1;
        if *count == 2 {
            errors.push(ResolutionError::DuplicateId {
                id: candidate.id.clone(),
            });
        }
        providers
            .entry(candidate.id.clone())
            .or_default()
            .push(candidate.version.clone());
        for provided in &candidate.provides {
            let version = provided
                .version
                .clone()
                .unwrap_or_else(|| candidate.version.clone());
            providers
                .entry(provided.id.clone())
                .or_default()
                .push(version);
        }
    }

    for candidate in &candidates {
        for dependency in &candidate.depends {
            check_dependency(candidate, dependency, &providers, &mut errors);
        }
        for breakage in &candidate.breaks {
            check_breakage(candidate, breakage, &providers, &mut errors);
        }
    }

    let loaded = if errors.is_empty() {
        candidates
    } else {
        Vec::new()
    };
    Resolution { loaded, errors }
}

fn check_dependency(
    candidate: &ModCandidate,
    dependency: &Dependency,
    providers: &HashMap<String, Vec<Version>>,
    errors: &mut Vec<ResolutionError>,
) {
    if dependency.optional {
        return;
    }
    match providers.get(&dependency.id) {
        None => errors.push(ResolutionError::MissingDependency {
            mod_id: candidate.id.clone(),
            dependency: dependency.id.clone(),
            reason: dependency.reason.clone(),
        }),
        Some(versions) => {
            if !any_version_matches(&dependency.versions, versions) {
                errors.push(ResolutionError::UnsatisfiedVersion {
                    mod_id: candidate.id.clone(),
                    dependency: dependency.id.clone(),
                    available: join_versions(versions),
                });
            }
        }
    }
}

fn check_breakage(
    candidate: &ModCandidate,
    breakage: &Dependency,
    providers: &HashMap<String, Vec<Version>>,
    errors: &mut Vec<ResolutionError>,
) {
    if let Some(versions) = providers.get(&breakage.id) {
        for version in versions {
            if breakage.versions.matches(version) {
                errors.push(ResolutionError::Breakage {
                    mod_id: candidate.id.clone(),
                    broken: breakage.id.clone(),
                    version: version.to_string(),
                });
                break;
            }
        }
    }
}

fn any_version_matches(spec: &VersionSpec, versions: &[Version]) -> bool {
    versions.iter().any(|version| spec.matches(version))
}

fn join_versions(versions: &[Version]) -> String {
    versions
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(json: &[u8]) -> ModCandidate {
        ModCandidate::parse(json).expect("valid metadata")
    }

    fn lifecycle_mod() -> ModCandidate {
        candidate(
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
        )
    }

    #[test]
    fn reference_mod_resolves_against_builtins() {
        let resolution = resolve(vec![lifecycle_mod()], &default_builtins());
        assert!(resolution.is_ok(), "errors: {:?}", resolution.errors);
        assert_eq!(resolution.loaded.len(), 1);
    }

    #[test]
    fn missing_dependency_is_reported() {
        let mod_json = candidate(
            br#"{
                "schema_version": 1,
                "quilt_loader": {
                    "id": "needs_owo",
                    "version": "1.0.0",
                    "depends": [{ "id": "owo_lib", "reason": "menus" }]
                }
            }"#,
        );
        let resolution = resolve(vec![mod_json], &default_builtins());
        assert_eq!(
            resolution.errors,
            vec![ResolutionError::MissingDependency {
                mod_id: "needs_owo".into(),
                dependency: "owo_lib".into(),
                reason: Some("menus".into()),
            }]
        );
    }

    #[test]
    fn wrong_game_version_is_unsatisfied() {
        let mod_json = candidate(
            br#"{
                "schema_version": 1,
                "quilt_loader": {
                    "id": "old_mod",
                    "version": "1.0.0",
                    "depends": [{ "id": "minecraft", "versions": "1.21" }]
                }
            }"#,
        );
        let resolution = resolve(vec![mod_json], &default_builtins());
        assert!(matches!(
            resolution.errors.as_slice(),
            [ResolutionError::UnsatisfiedVersion { .. }]
        ));
    }

    #[test]
    fn provides_satisfies_a_dependency() {
        let library = candidate(
            br#"{
                "schema_version": 1,
                "quilt_loader": {
                    "id": "real_lib",
                    "version": "3.1.0",
                    "provides": [{ "id": "owo_lib", "version": "3.1.0" }]
                }
            }"#,
        );
        let consumer = candidate(
            br#"{
                "schema_version": 1,
                "quilt_loader": {
                    "id": "consumer",
                    "version": "1.0.0",
                    "depends": [{ "id": "owo_lib", "versions": "^3.0.0" }]
                }
            }"#,
        );
        let resolution = resolve(vec![library, consumer], &default_builtins());
        assert!(resolution.is_ok(), "errors: {:?}", resolution.errors);
    }

    #[test]
    fn breakage_and_duplicates_are_reported() {
        let breaker = candidate(
            br#"{
                "schema_version": 1,
                "quilt_loader": {
                    "id": "breaker",
                    "version": "1.0.0",
                    "breaks": [{ "id": "minecraft", "versions": "26.2" }]
                }
            }"#,
        );
        let resolution = resolve(vec![breaker], &default_builtins());
        assert!(matches!(
            resolution.errors.as_slice(),
            [ResolutionError::Breakage { .. }]
        ));

        let duplicate_a = lifecycle_mod();
        let duplicate_b = lifecycle_mod();
        let dupes = resolve(vec![duplicate_a, duplicate_b], &default_builtins());
        assert!(
            dupes
                .errors
                .iter()
                .any(|error| matches!(error, ResolutionError::DuplicateId { .. }))
        );
    }
}
