//! Version constraint parsing and matching.
//!
//! This mirrors the grammar Quilt accepts in a dependency's `versions` field
//! (`org.quiltmc.loader.impl.metadata.qmj.VersionConstraintImpl` plus the
//! Fabric-compatible range parsing). A constraint is written either as a single
//! predicate string or as an array of predicate strings; the array is a union,
//! matching if any member matches. Each predicate is a conjunction of bounds:
//!
//! - `*` or the empty string matches any version.
//! - `1.2.3` (no operator, no wildcard) requires exact equality.
//! - `>=`, `>`, `<=`, `<`, `=` compare against a version.
//! - `^1.2.3` allows changes that do not modify the left-most non-zero component.
//! - `~1.2.3` allows patch-level changes.
//! - `1.x`, `1.X`, `1.*`, `1.2.x` fix a prefix and leave the rest free.
//!
//! Predicates that reference a version which is not semantic fall back to exact
//! string equality, exactly as Quilt does for opaque versions.

use crate::version::{SemanticVersion, Version};

/// A parsed `versions` value: a union of predicates.
#[derive(Clone, Debug)]
pub struct VersionSpec {
    predicates: Vec<Predicate>,
}

impl VersionSpec {
    /// A spec that matches every version.
    #[must_use]
    pub fn any() -> Self {
        Self {
            predicates: vec![Predicate::any()],
        }
    }

    /// Parse a single predicate string.
    ///
    /// # Errors
    /// Returns the offending fragment when the predicate is malformed.
    pub fn parse(input: &str) -> Result<Self, String> {
        Ok(Self {
            predicates: vec![Predicate::parse(input)?],
        })
    }

    /// Parse a union from several predicate strings.
    ///
    /// # Errors
    /// Returns the offending fragment when any predicate is malformed.
    pub fn parse_union<I, S>(inputs: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut predicates = Vec::new();
        for input in inputs {
            predicates.push(Predicate::parse(input.as_ref())?);
        }
        if predicates.is_empty() {
            predicates.push(Predicate::any());
        }
        Ok(Self { predicates })
    }

    /// Whether `version` satisfies this spec.
    #[must_use]
    pub fn matches(&self, version: &Version) -> bool {
        self.predicates
            .iter()
            .any(|predicate| predicate.matches(version))
    }
}

/// A conjunction of bounds, or an exact match against an opaque version.
#[derive(Clone, Debug)]
enum Predicate {
    Any,
    /// Exact match against a version that is not semantic.
    PlainEquals(String),
    /// All bounds must hold.
    Bounds(Vec<Bound>),
}

impl Predicate {
    fn any() -> Self {
        Self::Any
    }

    fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed == "*" {
            return Ok(Self::Any);
        }

        if let Some(rest) = strip_operator(trimmed) {
            let (operator, version_text) = rest;
            return match Version::parse(version_text) {
                Version::Semantic(version) => Ok(Self::Bounds(operator.bounds(&version))),
                Version::Plain(raw) => {
                    if operator == Operator::Exact {
                        Ok(Self::PlainEquals(raw))
                    } else {
                        Err(format!(
                            "operator '{}' cannot be applied to non-semantic version '{raw}'",
                            operator.symbol()
                        ))
                    }
                }
            };
        }

        // No operator: either a wildcard prefix or an exact version.
        if let Some(bounds) = parse_wildcard(trimmed)? {
            return Ok(Self::Bounds(bounds));
        }
        match Version::parse(trimmed) {
            Version::Semantic(version) => Ok(Self::Bounds(vec![Bound {
                comparator: Comparator::Equal,
                version,
            }])),
            Version::Plain(raw) => Ok(Self::PlainEquals(raw)),
        }
    }

    fn matches(&self, version: &Version) -> bool {
        match self {
            Self::Any => true,
            Self::PlainEquals(expected) => match version {
                Version::Plain(raw) => raw == expected,
                Version::Semantic(semantic) => semantic.to_string() == *expected,
            },
            Self::Bounds(bounds) => match version {
                Version::Semantic(semantic) => {
                    bounds.iter().all(|bound| bound.matches(semantic))
                }
                Version::Plain(_) => false,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comparator {
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Equal,
}

#[derive(Clone, Debug)]
struct Bound {
    comparator: Comparator,
    version: SemanticVersion,
}

impl Bound {
    fn matches(&self, version: &SemanticVersion) -> bool {
        let ordering = version.cmp(&self.version);
        match self.comparator {
            Comparator::Greater => ordering.is_gt(),
            Comparator::GreaterEqual => ordering.is_ge(),
            Comparator::Less => ordering.is_lt(),
            Comparator::LessEqual => ordering.is_le(),
            Comparator::Equal => ordering.is_eq(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Exact,
    Caret,
    Tilde,
}

impl Operator {
    fn symbol(self) -> &'static str {
        match self {
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Exact => "=",
            Self::Caret => "^",
            Self::Tilde => "~",
        }
    }

    /// Desugar an operator plus version into a set of concrete bounds.
    fn bounds(self, version: &SemanticVersion) -> Vec<Bound> {
        match self {
            Self::Greater => vec![bound(Comparator::Greater, version)],
            Self::GreaterEqual => vec![bound(Comparator::GreaterEqual, version)],
            Self::Less => vec![bound(Comparator::Less, version)],
            Self::LessEqual => vec![bound(Comparator::LessEqual, version)],
            Self::Exact => vec![bound(Comparator::Equal, version)],
            Self::Caret => caret_bounds(version),
            Self::Tilde => tilde_bounds(version),
        }
    }
}

fn bound(comparator: Comparator, version: &SemanticVersion) -> Bound {
    Bound {
        comparator,
        version: version.clone(),
    }
}

/// `^1.2.3` -> `>=1.2.3 <2.0.0`, honouring the left-most non-zero component so
/// `^0.2.3` -> `>=0.2.3 <0.3.0` and `^0.0.3` -> `>=0.0.3 <0.0.4`.
fn caret_bounds(version: &SemanticVersion) -> Vec<Bound> {
    let major = version.component(0);
    let minor = version.component(1);
    let patch = version.component(2);
    let upper = if major != 0 {
        exact_version(&[major + 1, 0, 0])
    } else if minor != 0 {
        exact_version(&[0, minor + 1, 0])
    } else {
        exact_version(&[0, 0, patch + 1])
    };
    vec![
        bound(Comparator::GreaterEqual, version),
        Bound {
            comparator: Comparator::Less,
            version: upper,
        },
    ]
}

/// `~1.2.3` -> `>=1.2.3 <1.3.0`; `~1.2` -> `>=1.2.0 <1.3.0`; `~1` -> `>=1.0.0 <2.0.0`.
fn tilde_bounds(version: &SemanticVersion) -> Vec<Bound> {
    let major = version.component(0);
    let upper = if version.component_count() >= 2 {
        exact_version(&[major, version.component(1) + 1, 0])
    } else {
        exact_version(&[major + 1, 0, 0])
    };
    vec![
        bound(Comparator::GreaterEqual, version),
        Bound {
            comparator: Comparator::Less,
            version: upper,
        },
    ]
}

/// Parse a wildcard prefix such as `1.x` or `1.2.*`. Returns `Ok(None)` when the
/// input contains no wildcard so the caller can treat it as an exact (possibly
/// non-semantic) version instead.
fn parse_wildcard(input: &str) -> Result<Option<Vec<Bound>>, String> {
    let parts: Vec<&str> = input.split('.').collect();
    let is_wildcard = |part: &str| matches!(part, "x" | "X" | "*");
    if !parts.iter().any(|part| is_wildcard(part)) {
        return Ok(None);
    }

    let mut prefix = Vec::new();
    let mut saw_wildcard = false;
    for part in parts {
        if is_wildcard(part) {
            saw_wildcard = true;
            continue;
        }
        if saw_wildcard {
            return Err(format!("fixed component after wildcard in '{input}'"));
        }
        let value = part
            .parse::<u64>()
            .map_err(|_| format!("invalid version component '{part}' in '{input}'"))?;
        prefix.push(value);
    }
    if prefix.is_empty() {
        // Bare `x` behaves like `*`.
        return Ok(Some(Vec::new()));
    }
    let lower = exact_version(&prefix);
    let mut upper_components = prefix.clone();
    let last = upper_components.len() - 1;
    upper_components[last] += 1;
    let upper = exact_version(&upper_components);
    Ok(Some(vec![
        bound(Comparator::GreaterEqual, &lower),
        Bound {
            comparator: Comparator::Less,
            version: upper,
        },
    ]))
}

fn exact_version(components: &[u64]) -> SemanticVersion {
    let joined: Vec<String> = components.iter().map(u64::to_string).collect();
    SemanticVersion::parse(&joined.join(".")).expect("generated version is valid")
}

fn strip_operator(input: &str) -> Option<(Operator, &str)> {
    for (symbol, operator) in [
        (">=", Operator::GreaterEqual),
        ("<=", Operator::LessEqual),
        (">", Operator::Greater),
        ("<", Operator::Less),
        ("=", Operator::Exact),
        ("^", Operator::Caret),
        ("~", Operator::Tilde),
    ] {
        if let Some(rest) = input.strip_prefix(symbol) {
            return Some((operator, rest.trim()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(spec: &str, version: &str) -> bool {
        VersionSpec::parse(spec)
            .expect("valid spec")
            .matches(&Version::parse(version))
    }

    #[test]
    fn any_matches_everything() {
        assert!(matches("*", "1.2.3"));
        assert!(matches("", "0.0.1"));
        assert!(VersionSpec::any().matches(&Version::parse("anything")));
    }

    #[test]
    fn exact_requires_equality_with_zero_padding() {
        assert!(matches("26.2", "26.2"));
        assert!(matches("26.2", "26.2.0"));
        assert!(!matches("26.2", "26.2.1"));
        assert!(!matches("26.2", "26.3"));
    }

    #[test]
    fn comparison_operators() {
        assert!(matches(">=0.30.0", "0.30.0"));
        assert!(matches(">=0.30.0", "0.31.5"));
        assert!(!matches(">=0.30.0", "0.29.9"));
        assert!(matches(">1.0.0", "1.0.1"));
        assert!(!matches(">1.0.0", "1.0.0"));
        assert!(matches("<=2.0.0", "2.0.0"));
        assert!(matches("<2.0.0", "1.9.9"));
    }

    #[test]
    fn caret_pins_left_most_non_zero_component() {
        assert!(matches("^1.2.3", "1.9.0"));
        assert!(!matches("^1.2.3", "2.0.0"));
        assert!(!matches("^1.2.3", "1.2.2"));
        assert!(matches("^0.2.3", "0.2.9"));
        assert!(!matches("^0.2.3", "0.3.0"));
        assert!(matches("^0.0.3", "0.0.3"));
        assert!(!matches("^0.0.3", "0.0.4"));
    }

    #[test]
    fn tilde_allows_patch_changes() {
        assert!(matches("~1.2.3", "1.2.9"));
        assert!(!matches("~1.2.3", "1.3.0"));
        assert!(matches("~1.2", "1.2.5"));
        assert!(!matches("~1.2", "1.3.0"));
        assert!(matches("~1", "1.9.9"));
        assert!(!matches("~1", "2.0.0"));
    }

    #[test]
    fn wildcards() {
        assert!(matches("1.x", "1.9.9"));
        assert!(!matches("1.x", "2.0.0"));
        assert!(matches("1.2.*", "1.2.7"));
        assert!(!matches("1.2.*", "1.3.0"));
        assert!(matches("x", "5.5.5"));
    }

    #[test]
    fn union_matches_any_member() {
        let spec = VersionSpec::parse_union(["1.x", ">=3.0.0"]).expect("valid union");
        assert!(spec.matches(&Version::parse("1.4.0")));
        assert!(spec.matches(&Version::parse("3.1.0")));
        assert!(!spec.matches(&Version::parse("2.0.0")));
    }

    #[test]
    fn operator_on_plain_version_is_rejected() {
        assert!(VersionSpec::parse(">=nightly").is_err());
        // Bare non-semantic strings still match exactly.
        assert!(matches("nightly-build", "nightly-build"));
        assert!(!matches("nightly-build", "release"));
    }
}
