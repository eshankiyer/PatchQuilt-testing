use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub name: String,
    pub java_type: String,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Method {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: String,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Param {
    pub name: String,
    pub java_type: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
}

#[derive(Debug, Clone, Serialize)]
pub struct JavaType {
    pub package: String,
    pub qualified_name: String,
    pub kind: TypeKind,
    pub modifiers: Vec<String>,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub file: String,
}

#[derive(Debug, Serialize)]
pub struct Index {
    pub source_version: String,
    pub file_count: usize,
    pub types: Vec<JavaType>,
}
