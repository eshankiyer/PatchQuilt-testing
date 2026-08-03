pub mod model;
pub mod parse;
pub mod strip;

use model::Index;
use std::path::Path;

/// Walks `source_root` for `.java` files and builds a structured index of every class,
/// interface, enum, and record declaration found, including nested types.
///
/// # Errors
///
/// Returns an error if `source_root` cannot be walked or a file cannot be read as UTF-8.
pub fn build_index(source_root: &Path, source_version: &str) -> std::io::Result<Index> {
    let mut types = Vec::new();
    let mut file_count = 0usize;
    let mut stack = vec![source_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("java") {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(source_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            file_count += 1;
            types.extend(parse::parse_file(&relative, &source));
        }
    }
    types.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    Ok(Index {
        source_version: source_version.to_string(),
        file_count,
        types,
    })
}
