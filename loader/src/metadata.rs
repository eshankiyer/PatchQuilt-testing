//! Mod metadata parsing for `quilt.mod.json` (QMJ schema 1) and the
//! Fabric-compatible `fabric.mod.json`.
//!
//! Both formats are reduced to a single [`ModCandidate`], the same way Quilt
//! Loader normalises Fabric metadata into its internal model before resolution.
//! Only the fields that affect discovery and dependency resolution are modelled;
//! presentation-only fields (contributors, contact, icon) are accepted and
//! ignored rather than rejected, matching Quilt's lenient parsing.

use serde_json::Value;

use crate::constraint::VersionSpec;
use crate::version::Version;

/// The origin format of a candidate, retained for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModFormat {
    Quilt,
    Fabric,
}

/// A dependency or breakage clause.
#[derive(Clone, Debug)]
pub struct Dependency {
    pub id: String,
    pub versions: VersionSpec,
    pub optional: bool,
    pub reason: Option<String>,
}

/// An identity a mod claims to provide in addition to its own id.
#[derive(Clone, Debug)]
pub struct ProvidedMod {
    pub id: String,
    pub version: Option<Version>,
}

/// A normalised mod definition ready for resolution.
#[derive(Clone, Debug)]
pub struct ModCandidate {
    pub format: ModFormat,
    pub group: Option<String>,
    pub id: String,
    pub version: Version,
    pub name: Option<String>,
    pub provides: Vec<ProvidedMod>,
    pub depends: Vec<Dependency>,
    pub breaks: Vec<Dependency>,
    pub entrypoints: Vec<String>,
}

impl ModCandidate {
    /// Parse metadata from raw JSON bytes, choosing the format automatically.
    ///
    /// # Errors
    /// Returns a human-readable message when the JSON is invalid or a required
    /// field is missing or malformed.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let value: Value =
            serde_json::from_slice(bytes).map_err(|error| format!("invalid JSON: {error}"))?;
        if value.get("quilt_loader").is_some() {
            parse_quilt(&value)
        } else if value.get("schemaVersion").is_some() || value.get("id").is_some() {
            parse_fabric(&value)
        } else {
            Err("metadata is neither quilt.mod.json nor fabric.mod.json".to_owned())
        }
    }
}

fn parse_quilt(root: &Value) -> Result<ModCandidate, String> {
    let schema = root
        .get("schema_version")
        .and_then(Value::as_i64)
        .ok_or("quilt.mod.json is missing schema_version")?;
    if schema != 1 {
        return Err(format!("unsupported quilt.mod.json schema_version {schema}"));
    }
    let loader = root
        .get("quilt_loader")
        .and_then(Value::as_object)
        .ok_or("quilt.mod.json is missing the quilt_loader object")?;

    let id = required_str(loader, "id", "quilt_loader.id")?;
    validate_mod_id(&id)?;
    let group = optional_str(loader, "group");
    let version = Version::parse(&required_str(loader, "version", "quilt_loader.version")?);
    let name = loader
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let depends = parse_dependency_list(loader.get("depends"))?;
    let breaks = parse_dependency_list(loader.get("breaks"))?;
    let provides = parse_provides(loader.get("provides"))?;
    let entrypoints = parse_quilt_entrypoints(loader.get("entrypoints"));

    Ok(ModCandidate {
        format: ModFormat::Quilt,
        group,
        id,
        version,
        name,
        provides,
        depends,
        breaks,
        entrypoints,
    })
}

fn parse_fabric(root: &Value) -> Result<ModCandidate, String> {
    let id = root
        .get("id")
        .and_then(Value::as_str)
        .ok_or("fabric.mod.json is missing id")?
        .to_owned();
    validate_mod_id(&id)?;
    let version = Version::parse(
        root.get("version")
            .and_then(Value::as_str)
            .ok_or("fabric.mod.json is missing version")?,
    );
    let name = root.get("name").and_then(Value::as_str).map(str::to_owned);

    let depends = parse_fabric_dependency_map(root.get("depends"), false)?;
    let breaks = parse_fabric_dependency_map(root.get("breaks"), false)?;
    // Fabric `recommends`/`suggests` are advisory; model them as optional depends.
    let mut all_depends = depends;
    all_depends.extend(parse_fabric_dependency_map(root.get("recommends"), true)?);

    let provides = root
        .get("provides")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|id| ProvidedMod {
                    id: id.to_owned(),
                    version: None,
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(ModCandidate {
        format: ModFormat::Fabric,
        group: None,
        id,
        version,
        name,
        provides,
        depends: all_depends,
        breaks,
        entrypoints: Vec::new(),
    })
}

/// Quilt allows `depends` to be a single object, a single id string, or an array
/// mixing both. Each entry is `{ id, versions?, optional?, reason? }` or a bare
/// id string that implies any version.
fn parse_dependency_list(value: Option<&Value>) -> Result<Vec<Dependency>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::Array(items) => items.iter().map(parse_dependency_entry).collect(),
        other => Ok(vec![parse_dependency_entry(other)?]),
    }
}

fn parse_dependency_entry(value: &Value) -> Result<Dependency, String> {
    match value {
        Value::String(id) => Ok(Dependency {
            id: id.clone(),
            versions: VersionSpec::any(),
            optional: false,
            reason: None,
        }),
        Value::Object(object) => {
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .ok_or("dependency is missing id")?
                .to_owned();
            let versions = parse_versions_field(object.get("versions"))?;
            let optional = object
                .get("optional")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let reason = object
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok(Dependency {
                id,
                versions,
                optional,
                reason,
            })
        }
        _ => Err("dependency must be a string or object".to_owned()),
    }
}

fn parse_versions_field(value: Option<&Value>) -> Result<VersionSpec, String> {
    match value {
        None | Some(Value::Null) => Ok(VersionSpec::any()),
        Some(Value::String(single)) => VersionSpec::parse(single),
        Some(Value::Array(items)) => {
            let strings: Result<Vec<&str>, String> = items
                .iter()
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| "version array entries must be strings".to_owned())
                })
                .collect();
            VersionSpec::parse_union(strings?)
        }
        Some(_) => Err("versions must be a string or an array of strings".to_owned()),
    }
}

/// Fabric encodes dependencies as an object mapping id to a version range or an
/// array of ranges.
fn parse_fabric_dependency_map(
    value: Option<&Value>,
    optional: bool,
) -> Result<Vec<Dependency>, String> {
    let Some(Value::Object(map)) = value else {
        return Ok(Vec::new());
    };
    let mut dependencies = Vec::new();
    for (id, range) in map {
        dependencies.push(Dependency {
            id: id.clone(),
            versions: parse_versions_field(Some(range))?,
            optional,
            reason: None,
        });
    }
    Ok(dependencies)
}

fn parse_provides(value: Option<&Value>) -> Result<Vec<ProvidedMod>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or("provides must be an array")?
        .iter();
    let mut provided = Vec::new();
    for item in items {
        match item {
            Value::String(id) => provided.push(ProvidedMod {
                id: id.clone(),
                version: None,
            }),
            Value::Object(object) => {
                let id = object
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("provides entry is missing id")?
                    .to_owned();
                let version = object
                    .get("version")
                    .and_then(Value::as_str)
                    .map(Version::parse);
                provided.push(ProvidedMod { id, version });
            }
            _ => return Err("provides entry must be a string or object".to_owned()),
        }
    }
    Ok(provided)
}

/// Flatten the Quilt entrypoints map into a list of implementation class names.
/// Each value is a string, an object with a `value` key, or an array of those.
fn parse_quilt_entrypoints(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    let mut classes = Vec::new();
    for entry in map.values() {
        collect_entrypoint_values(entry, &mut classes);
    }
    classes
}

fn collect_entrypoint_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(class) => out.push(class.clone()),
        Value::Object(object) => {
            if let Some(class) = object.get("value").and_then(Value::as_str) {
                out.push(class.to_owned());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_entrypoint_values(item, out);
            }
        }
        _ => {}
    }
}

/// Quilt mod ids must match `^[a-z][a-z0-9-_]{1,63}$`.
fn validate_mod_id(id: &str) -> Result<(), String> {
    let length = id.len();
    if !(2..=64).contains(&length) {
        return Err(format!("mod id '{id}' must be between 2 and 64 characters"));
    }
    let mut bytes = id.bytes();
    let first = bytes.next().expect("length checked above");
    if !first.is_ascii_lowercase() {
        return Err(format!("mod id '{id}' must start with a lowercase letter"));
    }
    if !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')) {
        return Err(format!(
            "mod id '{id}' may only contain lowercase letters, digits, '-', and '_'"
        ));
    }
    Ok(())
}

fn required_str(
    object: &serde_json::Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<String, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing required field {label}"))
}

fn optional_str(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIFECYCLE_MOD: &[u8] = br#"{
        "schema_version": 1,
        "quilt_loader": {
            "group": "org.patchquilt",
            "id": "patchquilt_lifecycle_test",
            "version": "1.0.0",
            "metadata": { "name": "PatchQuilt Lifecycle Test" },
            "intermediate_mappings": "net.fabricmc:intermediary",
            "entrypoints": { "init": "org.patchquilt.testmod.LifecycleTestMod" },
            "depends": [
                { "id": "quilt_loader", "versions": ">=0.30.0" },
                { "id": "minecraft", "versions": "26.2" }
            ]
        },
        "mixin": ["patchquilt.mixins.json"]
    }"#;

    #[test]
    fn parses_the_reference_quilt_mod() {
        let candidate = ModCandidate::parse(LIFECYCLE_MOD).expect("valid metadata");
        assert_eq!(candidate.format, ModFormat::Quilt);
        assert_eq!(candidate.id, "patchquilt_lifecycle_test");
        assert_eq!(candidate.group.as_deref(), Some("org.patchquilt"));
        assert_eq!(candidate.name.as_deref(), Some("PatchQuilt Lifecycle Test"));
        assert_eq!(candidate.depends.len(), 2);
        assert_eq!(candidate.entrypoints, vec!["org.patchquilt.testmod.LifecycleTestMod"]);
        assert_eq!(candidate.version, Version::parse("1.0.0"));
    }

    #[test]
    fn accepts_string_and_array_dependencies() {
        let json = br#"{
            "schema_version": 1,
            "quilt_loader": {
                "id": "example_mod",
                "version": "2.0.0",
                "depends": ["quilt_base", { "id": "owo", "versions": ["1.x", ">=3.0.0"] }]
            }
        }"#;
        let candidate = ModCandidate::parse(json).expect("valid metadata");
        assert_eq!(candidate.depends[0].id, "quilt_base");
        assert!(candidate.depends[0].versions.matches(&Version::parse("9.9.9")));
        assert!(candidate.depends[1].versions.matches(&Version::parse("3.4.0")));
        assert!(!candidate.depends[1].versions.matches(&Version::parse("2.0.0")));
    }

    #[test]
    fn parses_fabric_metadata() {
        let json = br#"{
            "schemaVersion": 1,
            "id": "fabric_example",
            "version": "1.4.2",
            "depends": { "fabricloader": ">=0.15.0", "minecraft": ["26.2"] }
        }"#;
        let candidate = ModCandidate::parse(json).expect("valid metadata");
        assert_eq!(candidate.format, ModFormat::Fabric);
        assert_eq!(candidate.depends.len(), 2);
        assert!(
            candidate
                .depends
                .iter()
                .any(|dependency| dependency.id == "fabricloader")
        );
    }

    #[test]
    fn rejects_bad_schema_and_ids() {
        assert!(ModCandidate::parse(br#"{"schema_version": 2, "quilt_loader": {}}"#).is_err());
        let bad_id = br#"{"schema_version":1,"quilt_loader":{"id":"Bad-ID","version":"1.0.0"}}"#;
        assert!(ModCandidate::parse(bad_id).is_err());
    }
}
