//! Version parsing and comparison matching Quilt Loader's version model.
//!
//! Quilt exposes two kinds of version through `Version.of`: a semantic version
//! (`org.quiltmc.loader.impl.metadata.qmj.SemanticVersionImpl`) and an opaque
//! string version used as a fallback when the value is not valid semver. This
//! module mirrors that split: [`Version::parse`] yields a [`Version::Semantic`]
//! when the input follows `SemVer` 2.0.0 (extended with an arbitrary number of
//! numeric components, exactly as Quilt allows), and a [`Version::Plain`]
//! otherwise. Only two plain versions compare, and only for equality, which is
//! the same contract Quilt's `StringVersion` offers.

use std::cmp::Ordering;
use std::fmt;

/// A parsed mod or game version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Version {
    /// A `SemVer` 2.0.0 version with an arbitrary number of release components.
    Semantic(SemanticVersion),
    /// A version string that is not valid semver; only equality is defined.
    Plain(String),
}

impl Version {
    /// Parse a version string, preferring the semantic representation.
    #[must_use]
    pub fn parse(input: &str) -> Self {
        SemanticVersion::parse(input).map_or_else(|| Self::Plain(input.to_owned()), Self::Semantic)
    }

    /// The semantic view of this version, if it has one.
    #[must_use]
    pub fn as_semantic(&self) -> Option<&SemanticVersion> {
        match self {
            Self::Semantic(version) => Some(version),
            Self::Plain(_) => None,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Semantic(version) => version.fmt(formatter),
            Self::Plain(raw) => formatter.write_str(raw),
        }
    }
}

/// A single dot-separated identifier inside a pre-release tag.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PreReleaseIdentifier {
    Numeric(u64),
    Alphanumeric(String),
}

impl Ord for PreReleaseIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Alphanumeric(left), Self::Alphanumeric(right)) => left.cmp(right),
            // `SemVer` 11.4.3: numeric identifiers always rank lower than alphanumeric ones.
            (Self::Numeric(_), Self::Alphanumeric(_)) => Ordering::Less,
            (Self::Alphanumeric(_), Self::Numeric(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for PreReleaseIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A semantic version: release components, an optional pre-release tag, and
/// optional build metadata that is ignored for ordering.
#[derive(Clone, Debug, Eq)]
pub struct SemanticVersion {
    components: Vec<u64>,
    pre_release: Vec<PreReleaseIdentifier>,
    build: Option<String>,
}

impl SemanticVersion {
    /// Parse a semantic version, returning `None` when the input is not valid.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        if input.is_empty() {
            return None;
        }
        let (without_build, build) = split_once_owned(input, '+');
        if matches!(&build, Some(metadata) if !is_valid_dotted(metadata)) {
            return None;
        }
        let (core, pre) = split_once_owned(&without_build, '-');

        let mut components = Vec::new();
        for raw in core.split('.') {
            components.push(parse_numeric_component(raw)?);
        }
        if components.is_empty() {
            return None;
        }

        let pre_release = match pre {
            None => Vec::new(),
            Some(tag) => parse_pre_release(&tag)?,
        };

        Some(Self {
            components,
            pre_release,
            build,
        })
    }

    /// The value of a release component, treating absent trailing components as
    /// zero so that `1.2` and `1.2.0` compare equal.
    #[must_use]
    pub fn component(&self, index: usize) -> u64 {
        self.components.get(index).copied().unwrap_or(0)
    }

    /// The number of explicit release components.
    #[must_use]
    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    /// Whether this version carries a pre-release tag.
    #[must_use]
    pub fn is_pre_release(&self) -> bool {
        !self.pre_release.is_empty()
    }
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let width = self.components.len().max(other.components.len());
        for index in 0..width {
            match self.component(index).cmp(&other.component(index)) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        compare_pre_release(&self.pre_release, &other.pre_release)
    }
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SemanticVersion {
    fn eq(&self, other: &Self) -> bool {
        // Build metadata is excluded from precedence and from equality.
        self.cmp(other) == Ordering::Equal
    }
}

impl fmt::Display for SemanticVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let joined: Vec<String> = self.components.iter().map(u64::to_string).collect();
        formatter.write_str(&joined.join("."))?;
        if !self.pre_release.is_empty() {
            formatter.write_str("-")?;
            let tags: Vec<String> = self
                .pre_release
                .iter()
                .map(|identifier| match identifier {
                    PreReleaseIdentifier::Numeric(value) => value.to_string(),
                    PreReleaseIdentifier::Alphanumeric(value) => value.clone(),
                })
                .collect();
            formatter.write_str(&tags.join("."))?;
        }
        if let Some(build) = &self.build {
            write!(formatter, "+{build}")?;
        }
        Ok(())
    }
}

/// Compare two pre-release tag lists per `SemVer` 11.4.
fn compare_pre_release(left: &[PreReleaseIdentifier], right: &[PreReleaseIdentifier]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        // A version with a pre-release tag has lower precedence than one without.
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (a, b) in left.iter().zip(right.iter()) {
        match a.cmp(b) {
            Ordering::Equal => {}
            unequal => return unequal,
        }
    }
    // A larger set of identifiers wins when all shared identifiers are equal.
    left.len().cmp(&right.len())
}

fn parse_numeric_component(raw: &str) -> Option<u64> {
    if raw.is_empty() || (raw.len() > 1 && raw.starts_with('0')) {
        // `SemVer` forbids leading zeros on numeric identifiers.
        return None;
    }
    raw.parse::<u64>().ok()
}

fn parse_pre_release(tag: &str) -> Option<Vec<PreReleaseIdentifier>> {
    if tag.is_empty() {
        return None;
    }
    let mut identifiers = Vec::new();
    for part in tag.split('.') {
        if part.is_empty() || !part.bytes().all(is_identifier_byte) {
            return None;
        }
        let numeric = !part.starts_with('0') || part == "0";
        if numeric && part.bytes().all(|byte| byte.is_ascii_digit()) {
            identifiers.push(PreReleaseIdentifier::Numeric(part.parse().ok()?));
        } else if part.bytes().all(|byte| byte.is_ascii_digit()) {
            // Numeric identifier with a leading zero is invalid.
            return None;
        } else {
            identifiers.push(PreReleaseIdentifier::Alphanumeric(part.to_owned()));
        }
    }
    Some(identifiers)
}

fn is_valid_dotted(value: &str) -> bool {
    !value.is_empty()
        && value
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(is_identifier_byte))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// Like `str::split_once` but returns owned halves, which keeps the parser
/// readable when a delimiter may appear in either half.
fn split_once_owned(input: &str, delimiter: char) -> (String, Option<String>) {
    input.split_once(delimiter).map_or_else(
        || (input.to_owned(), None),
        |(head, tail)| (head.to_owned(), Some(tail.to_owned())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn semantic(input: &str) -> SemanticVersion {
        SemanticVersion::parse(input).expect("valid semantic version")
    }

    #[test]
    fn parses_two_and_four_component_versions() {
        assert_eq!(semantic("26.2").component_count(), 2);
        assert_eq!(semantic("1.2.3.4").component(3), 4);
    }

    #[test]
    fn missing_trailing_components_are_zero() {
        assert_eq!(semantic("1.2"), semantic("1.2.0"));
        assert_eq!(semantic("1"), semantic("1.0.0"));
    }

    #[test]
    fn rejects_leading_zero_and_garbage() {
        assert!(SemanticVersion::parse("1.02.3").is_none());
        assert!(SemanticVersion::parse("1.x.0").is_none());
        assert!(SemanticVersion::parse("").is_none());
        assert_eq!(Version::parse("nightly-2026"), Version::Plain("nightly-2026".into()));
    }

    #[test]
    fn orders_release_components() {
        assert!(semantic("0.30.0") < semantic("0.30.1"));
        assert!(semantic("1.0.0") > semantic("0.99.99"));
        assert!(semantic("2.0") > semantic("1.9.9"));
    }

    #[test]
    fn pre_release_ranks_below_release() {
        assert!(semantic("1.0.0-alpha") < semantic("1.0.0"));
        assert!(semantic("1.0.0-alpha") < semantic("1.0.0-alpha.1"));
        assert!(semantic("1.0.0-alpha.1") < semantic("1.0.0-alpha.beta"));
        assert!(semantic("1.0.0-1") < semantic("1.0.0-alpha"));
    }

    #[test]
    fn build_metadata_is_ignored_for_precedence() {
        assert_eq!(semantic("1.0.0+a"), semantic("1.0.0+b"));
        assert_eq!(semantic("1.0.0+build.5").cmp(&semantic("1.0.0")), Ordering::Equal);
        assert!(SemanticVersion::parse("1.0.0+").is_none());
    }
}
