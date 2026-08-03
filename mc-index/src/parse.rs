use crate::model::{Field, JavaType, Method, Param, TypeKind};
use crate::strip::strip_comments_and_literals;
use regex::Regex;
use std::sync::LazyLock;

static PACKAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*package\s+([\w.]+)\s*;").expect("valid regex"));

// These regexes run only after `strip_leading_annotations_and_modifiers` has already consumed
// every modifier keyword and annotation from the front of the chunk. Folding modifier-matching
// into the regex itself (an earlier version of this file did) is ambiguous for constructors:
// `public Zombie(...)` has only one bare word before the parens, so a modifier-then-return-type
// alternation can misparse "public" itself as the return type instead of backtracking to treat
// it as a modifier.
static TYPE_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)^\b(class|interface|enum|record)\s+(\w+)(?:<.*?>)?(?:\s*\(([^)]*)\))?(?:\s+extends\s+(.+?))?(?:\s+implements\s+(.+?))?(?:\s+permits\s+.+?)?\s*$",
    )
    .expect("valid regex")
});

static METHOD_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)^(?:<.*?>\s+)?([\w.\[\]?]+(?:<.*?>)?(?:\[\])*)\s+(\w+)\s*\(([^)]*)\)\s*(?:throws\s+[\w.,\s<>]+)?$",
    )
    .expect("valid regex")
});

static CONSTRUCTOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^(\w+)\s*\(([^)]*)\)\s*(?:throws\s+[\w.,\s<>]+)?$").expect("valid regex")
});

static FIELD_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^([\w.?]+(?:<.*?>)?(?:\[\])*)\s+(.+)$").expect("valid regex")
});

const MODIFIER_WORDS: &[&str] = &[
    "public",
    "private",
    "protected",
    "static",
    "final",
    "abstract",
    "sealed",
    "non-sealed",
    "strictfp",
    "synchronized",
    "native",
    "default",
    "transient",
    "volatile",
];

/// Repeatedly strips leading `@Annotation`/`@Annotation(...)` markers and known modifier
/// keywords from `chunk`, returning the collected modifiers and the untouched remainder.
fn strip_leading_annotations_and_modifiers(chunk: &str) -> (Vec<String>, &str) {
    let mut rest = chunk.trim_start();
    let mut modifiers = Vec::new();
    loop {
        if let Some(after_at) = rest.strip_prefix('@') {
            let ident_end = after_at
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(after_at.len());
            let mut after = after_at[ident_end..].trim_start();
            if let Some(after_paren) = after.strip_prefix('(') {
                after = after_paren.find(')').map_or("", |i| &after_paren[i + 1..]);
            }
            rest = after.trim_start();
            continue;
        }
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..word_end];
        if MODIFIER_WORDS.contains(&word) {
            modifiers.push(word.to_string());
            rest = rest[word_end..].trim_start();
            continue;
        }
        break;
    }
    (modifiers, rest)
}

/// Splits on commas that are not nested inside `<...>`, `(...)`, or `[...]`.
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = s[start..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }
    parts
}

fn parse_params(raw: &str) -> Vec<Param> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    split_top_level_commas(raw)
        .into_iter()
        .filter_map(|p| {
            let p = p.trim().trim_start_matches("final").trim();
            let p = p.strip_prefix('@').map_or(p, |_| p);
            let mut words: Vec<&str> = p.split_whitespace().collect();
            let name = words.pop()?.trim_start_matches("...").to_string();
            let java_type = words.join(" ");
            if java_type.is_empty() {
                None
            } else {
                Some(Param { name, java_type })
            }
        })
        .collect()
}

/// Splits a Java multi-declarator field statement (`int a, b = 1, c[];`, body already stripped
/// of the trailing `;`) into individual (name, type) pairs sharing the same base type.
fn split_declarators(base_type: &str, decl_list: &str) -> Vec<(String, String)> {
    split_top_level_commas(decl_list)
        .into_iter()
        .filter_map(|d| {
            let d = d.split('=').next().unwrap_or(&d).trim();
            let mut extra_brackets = 0usize;
            let mut name = d.to_string();
            while let Some(stripped) = name.strip_suffix("[]") {
                extra_brackets += 1;
                name = stripped.trim_end().to_string();
            }
            if name.is_empty() || !name.chars().next().is_some_and(char::is_alphabetic) {
                return None;
            }
            let java_type = format!("{base_type}{}", "[]".repeat(extra_brackets));
            Some((name, java_type))
        })
        .collect()
}

struct TypeDecl {
    kind: TypeKind,
    name: String,
    modifiers: Vec<String>,
    extends: Option<String>,
    implements: Vec<String>,
    record_fields: Vec<Field>,
}

fn try_parse_type_decl(chunk: &str) -> Option<TypeDecl> {
    let (modifiers, rest) = strip_leading_annotations_and_modifiers(chunk);
    let caps = TYPE_DECL_RE.captures(rest)?;
    let kind = match &caps[1] {
        "class" => TypeKind::Class,
        "interface" => TypeKind::Interface,
        "enum" => TypeKind::Enum,
        "record" => TypeKind::Record,
        _ => return None,
    };
    let name = caps[2].to_string();
    let extends = caps.get(4).map(|m| m.as_str().trim().to_string());
    let implements = caps
        .get(5)
        .map(|m| split_top_level_commas(m.as_str().trim()))
        .unwrap_or_default();
    let record_fields = caps
        .get(3)
        .map(|m| {
            parse_params(m.as_str())
                .into_iter()
                .map(|p| Field {
                    name: p.name,
                    java_type: p.java_type,
                    modifiers: vec!["private".to_string(), "final".to_string()],
                })
                .collect()
        })
        .unwrap_or_default();
    Some(TypeDecl {
        kind,
        name,
        modifiers,
        extends,
        implements,
        record_fields,
    })
}

fn try_parse_method_decl(chunk: &str, enclosing_simple_name: Option<&str>) -> Option<Method> {
    let (modifiers, rest) = strip_leading_annotations_and_modifiers(chunk);
    if let Some(caps) = METHOD_DECL_RE.captures(rest) {
        let return_type = caps[1].trim().to_string();
        let name = caps[2].to_string();
        let params = parse_params(&caps[3]);
        return Some(Method {
            name,
            params,
            return_type,
            modifiers,
        });
    }
    let caps = CONSTRUCTOR_RE.captures(rest)?;
    let name = caps[1].to_string();
    if Some(name.as_str()) != enclosing_simple_name {
        return None;
    }
    let params = parse_params(&caps[2]);
    Some(Method {
        name: "<init>".to_string(),
        params,
        return_type: name,
        modifiers,
    })
}

fn try_parse_field_decl(chunk: &str) -> Vec<Field> {
    let (modifiers, rest) = strip_leading_annotations_and_modifiers(chunk);
    let Some(caps) = FIELD_DECL_RE.captures(rest) else {
        return Vec::new();
    };
    let base_type = caps[1].trim().to_string();
    let decl_list = caps[2].trim();
    split_declarators(&base_type, decl_list)
        .into_iter()
        .map(|(name, java_type)| Field {
            name,
            java_type,
            modifiers: modifiers.clone(),
        })
        .collect()
}

struct OpenType {
    qualified_name: String,
    simple_name: String,
    kind: TypeKind,
    modifiers: Vec<String>,
    extends: Option<String>,
    implements: Vec<String>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    open_depth: i32,
}

/// Parses one Java source file into its top-level and nested type declarations.
///
/// This is a best-effort brace-depth scanner, not a full Java parser. Known gaps, all safe
/// (they only cause under-counting, never a crash or a corrupted depth count): array-initializer
/// braces (`int[] a = {1, 2}`), anonymous-class field initializers, and enum constant lists are
/// not captured as fields/methods.
#[must_use]
pub fn parse_file(relative_path: &str, source: &str) -> Vec<JavaType> {
    let package = PACKAGE_RE
        .captures(source)
        .map_or_else(String::new, |c| c[1].to_string());
    let stripped = strip_comments_and_literals(source);
    let bytes = stripped.as_bytes();

    let mut depth = 0i32;
    let mut chunk_start = 0usize;
    let mut stack: Vec<OpenType> = Vec::new();
    let mut output = Vec::new();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => {
                let member_level = stack
                    .last()
                    .map_or(depth == 0, |top| depth == top.open_depth + 1);
                if member_level {
                    let chunk = &stripped[chunk_start..i];
                    if let Some(decl) = try_parse_type_decl(chunk) {
                        let qualified_name = stack.last().map_or_else(
                            || decl.name.clone(),
                            |top| format!("{}.{}", top.qualified_name, decl.name),
                        );
                        stack.push(OpenType {
                            qualified_name,
                            simple_name: decl.name,
                            kind: decl.kind,
                            modifiers: decl.modifiers,
                            extends: decl.extends,
                            implements: decl.implements,
                            fields: decl.record_fields,
                            methods: Vec::new(),
                            open_depth: depth,
                        });
                    } else if let Some(m) =
                        try_parse_method_decl(chunk, stack.last().map(|t| t.simple_name.as_str()))
                        && let Some(top) = stack.last_mut()
                    {
                        top.methods.push(m);
                    }
                }
                depth += 1;
                chunk_start = i + 1;
            }
            b'}' => {
                depth -= 1;
                if let Some(finished) = stack.pop_if(|top| depth == top.open_depth) {
                    output.push(JavaType {
                        package: package.clone(),
                        qualified_name: finished.qualified_name,
                        kind: finished.kind,
                        modifiers: finished.modifiers,
                        extends: finished.extends,
                        implements: finished.implements,
                        fields: finished.fields,
                        methods: finished.methods,
                        file: relative_path.to_string(),
                    });
                }
                chunk_start = i + 1;
            }
            b';' => {
                let member_level = stack
                    .last()
                    .map_or(depth == 0, |top| depth == top.open_depth + 1);
                if member_level {
                    let chunk = &stripped[chunk_start..i];
                    if let Some(m) =
                        try_parse_method_decl(chunk, stack.last().map(|t| t.simple_name.as_str()))
                    {
                        if let Some(top) = stack.last_mut() {
                            top.methods.push(m);
                        }
                    } else {
                        let fields = try_parse_field_decl(chunk);
                        if let Some(top) = stack.last_mut() {
                            top.fields.extend(fields);
                        }
                    }
                }
                chunk_start = i + 1;
            }
            _ => {}
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_class_with_field_and_method() {
        let src = r"
            package net.minecraft.world.entity.animal.frog;

            public class Frog extends Animal {
                private static final int MAX_AGE = 20;

                protected void registerGoals() {
                    this.goalSelector.addGoal(0, new FloatGoal(this));
                }
            }
        ";
        let types = parse_file("net/minecraft/world/entity/animal/frog/Frog.java", src);
        assert_eq!(types.len(), 1);
        let frog = &types[0];
        assert_eq!(frog.qualified_name, "Frog");
        assert_eq!(frog.package, "net.minecraft.world.entity.animal.frog");
        assert_eq!(frog.kind as u8, TypeKind::Class as u8);
        assert_eq!(frog.extends.as_deref(), Some("Animal"));
        assert_eq!(frog.fields.len(), 1);
        assert_eq!(frog.fields[0].name, "MAX_AGE");
        assert_eq!(frog.fields[0].java_type, "int");
        assert_eq!(frog.methods.len(), 1);
        assert_eq!(frog.methods[0].name, "registerGoals");
        assert_eq!(frog.methods[0].return_type, "void");
    }

    #[test]
    fn parses_nested_class_with_qualified_name() {
        let src = r"
            package net.minecraft.world.entity.monster.illager;

            public abstract class SpellcasterIllager extends AbstractIllager {
                protected enum IllagerSpell {
                    NONE, FANGS;
                }
            }
        ";
        let types = parse_file(
            "net/minecraft/world/entity/monster/illager/SpellcasterIllager.java",
            src,
        );
        assert_eq!(types.len(), 2);
        assert!(
            types
                .iter()
                .any(|t| t.qualified_name == "SpellcasterIllager")
        );
        assert!(
            types
                .iter()
                .any(|t| t.qualified_name == "SpellcasterIllager.IllagerSpell")
        );
    }

    #[test]
    fn parses_interface_with_implements_list() {
        let src = r"
            package net.minecraft.world.entity;

            public class Foo implements Nameable, EntityAccess {
                public int bar(int a, String b) {
                    return a;
                }
            }
        ";
        let types = parse_file("net/minecraft/world/entity/Foo.java", src);
        assert_eq!(types.len(), 1);
        assert_eq!(
            types[0].implements,
            vec!["Nameable".to_string(), "EntityAccess".to_string()]
        );
        assert_eq!(types[0].methods[0].params.len(), 2);
        assert_eq!(types[0].methods[0].params[0].java_type, "int");
        assert_eq!(types[0].methods[0].params[1].java_type, "String");
    }

    #[test]
    fn parses_record_components_as_fields() {
        let src = r"
            package net.minecraft.core;

            public record BlockPos(int x, int y, int z) {
            }
        ";
        let types = parse_file("net/minecraft/core/BlockPos.java", src);
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].kind as u8, TypeKind::Record as u8);
        assert_eq!(types[0].fields.len(), 3);
        assert_eq!(types[0].fields[0].name, "x");
    }

    #[test]
    fn parses_constructor_distinct_from_return_typed_method() {
        let src = r"
            package net.minecraft.world.entity;

            public class Zombie {
                public Zombie(EntityType<?> type, Level level) {
                }

                public boolean isAlive() {
                    return true;
                }
            }
        ";
        let types = parse_file("net/minecraft/world/entity/Zombie.java", src);
        assert_eq!(types[0].methods.len(), 2);
        assert!(types[0].methods.iter().any(|m| m.name == "<init>"));
        assert!(types[0].methods.iter().any(|m| m.name == "isAlive"));
    }

    #[test]
    fn does_not_confuse_method_body_locals_with_fields() {
        let src = r#"
            package net.minecraft.world.entity;

            public class Foo {
                public void bar() {
                    int localVariable = 5;
                    if (localVariable > 0) {
                        String another = "x";
                    }
                }
            }
        "#;
        let types = parse_file("net/minecraft/world/entity/Foo.java", src);
        assert_eq!(types[0].fields.len(), 0);
        assert_eq!(types[0].methods.len(), 1);
    }

    #[test]
    fn split_top_level_commas_ignores_generics() {
        let parts = split_top_level_commas("Comparable<Foo, Bar>, Iterable<Baz>");
        assert_eq!(parts, vec!["Comparable<Foo, Bar>", "Iterable<Baz>"]);
    }

    #[test]
    fn multi_declarator_field_shares_base_type() {
        let fields = try_parse_field_decl("private int a, b = 5, c");
        assert_eq!(fields.len(), 3);
        assert!(fields.iter().all(|f| f.java_type == "int"));
    }
}
