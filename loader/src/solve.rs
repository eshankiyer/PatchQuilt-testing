//! Multi-version dependency selection.
//!
//! [`crate::resolve`] validates a candidate set under the assumption that each
//! mod id appears once. Real mod folders violate that: two jars can ship the
//! same library id at different versions, and only one may load. Quilt Loader
//! feeds this choice to a Sat4j solver. This module performs the same job with a
//! complete backtracking search, which is exact for the small candidate sets a
//! server actually loads and needs no external solver.
//!
//! The search groups candidates by id, treats every distinct-version group as a
//! variable whose domain is its candidates ordered newest-first, and returns the
//! first fully-assigned selection in which every non-optional `depends` clause is
//! satisfied and no `breaks` clause matches. Newest-first ordering means the
//! first solution found is the highest-version one consistent with the graph,
//! matching Quilt's preference. When no selection works, it falls back to the
//! per-constraint diagnostics from [`crate::resolve`] so the user still learns
//! what is missing.

use std::collections::{BTreeMap, BTreeSet};

use crate::metadata::ModCandidate;
use crate::resolve::{BuiltinMod, Resolution, ResolutionError};
use crate::version::Version;

/// Select a loadable subset of `candidates`, picking one version per id.
#[must_use]
pub fn solve(candidates: Vec<ModCandidate>, builtins: &[BuiltinMod]) -> Resolution {
    // Two jars with the same id *and* version are indistinguishable duplicates,
    // which Quilt rejects outright rather than choosing between.
    if let Some(duplicate) = find_exact_duplicate(&candidates) {
        return Resolution {
            loaded: Vec::new(),
            errors: vec![ResolutionError::DuplicateId { id: duplicate }],
        };
    }

    // Group candidate indices by id, each group ordered newest version first.
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        groups.entry(candidate.id.as_str()).or_default().push(index);
    }
    for indices in groups.values_mut() {
        indices.sort_by(|&left, &right| newer_first(&candidates[left], &candidates[right]));
    }
    let domains: Vec<Vec<usize>> = groups.into_values().collect();

    let mut assignment = Vec::with_capacity(domains.len());
    if search(&domains, 0, &candidates, builtins, &mut assignment) {
        let mut chosen: Vec<usize> = assignment;
        chosen.sort_unstable();
        let loaded = chosen.into_iter().map(|index| candidates[index].clone()).collect();
        return Resolution {
            loaded,
            errors: Vec::new(),
        };
    }

    // No consistent selection exists: reuse the single-version validator over the
    // whole set to explain which clauses cannot be satisfied by anything present.
    let mut diagnostic = crate::resolve::resolve(candidates, builtins);
    if diagnostic.errors.is_empty() {
        // The set is individually consistent but has no simultaneous solution
        // (a genuine version conflict between two required mods).
        diagnostic.errors.push(ResolutionError::DuplicateId {
            id: "<conflicting versions>".to_owned(),
        });
    }
    diagnostic.loaded.clear();
    diagnostic
}

/// Depth-first assignment of one candidate per id group.
fn search(
    domains: &[Vec<usize>],
    depth: usize,
    candidates: &[ModCandidate],
    builtins: &[BuiltinMod],
    assignment: &mut Vec<usize>,
) -> bool {
    if depth == domains.len() {
        return is_consistent(assignment, candidates, builtins);
    }
    for &index in &domains[depth] {
        assignment.push(index);
        // Prune early on breakage against already-chosen mods; depends need the
        // full assignment and are checked at the leaf.
        if !breaks_conflict(assignment, candidates, builtins)
            && search(domains, depth + 1, candidates, builtins, assignment)
        {
            return true;
        }
        assignment.pop();
    }
    false
}

/// The full leaf test: every selected mod's non-optional depends are satisfied
/// and no breaks clause matches.
fn is_consistent(
    assignment: &[usize],
    candidates: &[ModCandidate],
    builtins: &[BuiltinMod],
) -> bool {
    let providers = provider_map(assignment, candidates, builtins);
    for &index in assignment {
        let candidate = &candidates[index];
        for dependency in &candidate.depends {
            if dependency.optional {
                continue;
            }
            let satisfied = providers
                .get(dependency.id.as_str())
                .is_some_and(|versions| versions.iter().any(|v| dependency.versions.matches(v)));
            if !satisfied {
                return false;
            }
        }
    }
    !breaks_conflict(assignment, candidates, builtins)
}

/// Whether any selected mod breaks a currently-present provider.
fn breaks_conflict(
    assignment: &[usize],
    candidates: &[ModCandidate],
    builtins: &[BuiltinMod],
) -> bool {
    let providers = provider_map(assignment, candidates, builtins);
    for &index in assignment {
        for breakage in &candidates[index].breaks {
            if let Some(versions) = providers.get(breakage.id.as_str())
                && versions.iter().any(|v| breakage.versions.matches(v))
            {
                return true;
            }
        }
    }
    false
}

/// Map every id offered by the selected mods and builtins to its versions.
fn provider_map<'a>(
    assignment: &[usize],
    candidates: &'a [ModCandidate],
    builtins: &'a [BuiltinMod],
) -> BTreeMap<&'a str, Vec<Version>> {
    let mut providers: BTreeMap<&str, Vec<Version>> = BTreeMap::new();
    for builtin in builtins {
        providers
            .entry(builtin.id.as_str())
            .or_default()
            .push(builtin.version.clone());
    }
    for &index in assignment {
        let candidate = &candidates[index];
        providers
            .entry(candidate.id.as_str())
            .or_default()
            .push(candidate.version.clone());
        for provided in &candidate.provides {
            let version = provided
                .version
                .clone()
                .unwrap_or_else(|| candidate.version.clone());
            providers
                .entry(provided.id.as_str())
                .or_default()
                .push(version);
        }
    }
    providers
}

fn find_exact_duplicate(candidates: &[ModCandidate]) -> Option<String> {
    let mut seen: BTreeSet<(&str, String)> = BTreeSet::new();
    for candidate in candidates {
        let key = (candidate.id.as_str(), candidate.version.to_string());
        if !seen.insert(key) {
            return Some(candidate.id.clone());
        }
    }
    None
}

/// Order two candidates so the newer version sorts first. Non-semantic versions
/// keep discovery order relative to each other.
fn newer_first(left: &ModCandidate, right: &ModCandidate) -> std::cmp::Ordering {
    match (left.version.as_semantic(), right.version.as_semantic()) {
        (Some(a), Some(b)) => b.cmp(a),
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::default_builtins;

    fn candidate(json: &[u8]) -> ModCandidate {
        ModCandidate::parse(json).expect("valid metadata")
    }

    fn library(version: &str) -> ModCandidate {
        candidate(
            format!(
                r#"{{"schema_version":1,"quilt_loader":{{"id":"shared_lib","version":"{version}"}}}}"#
            )
            .as_bytes(),
        )
    }

    fn consumer(spec: &str) -> ModCandidate {
        candidate(
            format!(
                r#"{{"schema_version":1,"quilt_loader":{{"id":"consumer","version":"1.0.0",
                   "depends":[{{"id":"shared_lib","versions":"{spec}"}}]}}}}"#
            )
            .as_bytes(),
        )
    }

    #[test]
    fn picks_the_newest_version_that_satisfies_dependents() {
        let solution = solve(
            vec![library("1.0.0"), library("2.0.0"), consumer("^1.0.0")],
            &default_builtins(),
        );
        assert!(solution.is_ok(), "errors: {:?}", solution.errors);
        // shared_lib 2.0.0 fails ^1.0.0, so the solver must keep 1.0.0.
        let chosen: Vec<String> = solution
            .loaded
            .iter()
            .filter(|c| c.id == "shared_lib")
            .map(|c| c.version.to_string())
            .collect();
        assert_eq!(chosen, vec!["1.0.0".to_owned()]);
        assert_eq!(solution.loaded.len(), 2);
    }

    #[test]
    fn prefers_the_highest_version_when_unconstrained() {
        let solution = solve(
            vec![library("1.0.0"), library("2.0.0"), consumer(">=1.0.0")],
            &default_builtins(),
        );
        assert!(solution.is_ok());
        let chosen: Vec<String> = solution
            .loaded
            .iter()
            .filter(|c| c.id == "shared_lib")
            .map(|c| c.version.to_string())
            .collect();
        assert_eq!(chosen, vec!["2.0.0".to_owned()]);
    }

    #[test]
    fn reports_when_no_version_can_satisfy() {
        let solution = solve(
            vec![library("1.0.0"), library("2.0.0"), consumer(">=3.0.0")],
            &default_builtins(),
        );
        assert!(!solution.is_ok());
        assert!(solution.loaded.is_empty());
    }

    #[test]
    fn identical_id_and_version_is_a_duplicate() {
        let solution = solve(vec![library("1.0.0"), library("1.0.0")], &default_builtins());
        assert!(matches!(
            solution.errors.as_slice(),
            [ResolutionError::DuplicateId { .. }]
        ));
    }

    #[test]
    fn honours_breaks_across_versions() {
        let breaker = candidate(
            br#"{"schema_version":1,"quilt_loader":{"id":"breaker","version":"1.0.0",
                "breaks":[{"id":"shared_lib","versions":"2.x"}]}}"#,
        );
        // Only shared_lib 2.0.0 is available, which the breaker forbids: no solution.
        let blocked = solve(vec![library("2.0.0"), breaker.clone()], &default_builtins());
        assert!(!blocked.is_ok());
        // With a 1.x also available, the solver picks it and both mods load.
        let solved = solve(
            vec![library("1.0.0"), library("2.0.0"), breaker],
            &default_builtins(),
        );
        assert!(solved.is_ok(), "errors: {:?}", solved.errors);
    }
}
