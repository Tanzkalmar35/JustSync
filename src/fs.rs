use std::{fs, path::Path, vec};

use crate::logger;

pub fn get_project_files() -> Vec<(String, String)> {
    let ignored = vec![".git", "target", "node_modules"];
    let mut files = Vec::new();

    for entry in walkdir::WalkDir::new(".")
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip dirs
        if path.is_dir() {
            continue;
        }

        // Skip ignored modules (binaries, etc...)
        if path.components().any(|c| {
            let s = c.as_os_str().to_str().unwrap_or("");
            ignored.contains(&s.into())
        }) {
            continue;
        }

        let relative_path = path
            .strip_prefix(".")
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        if let Ok(content) = fs::read_to_string(path) {
            files.push((relative_path, content));
        }
    }

    files
}

pub fn write_project_files(files: Vec<(String, String)>) -> anyhow::Result<()> {
    for (path_str, content) in files {
        let path = Path::new(&path_str);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, content)?;
        logger::log(&format!(">> [FS] Wrote: {}", path_str));
    }
    Ok(())
}

pub fn to_relative_path(uri: &str, root: &str) -> String {
    // Simple decoding (replace %20 with space if needed)
    let clean_uri = uri.replace("%20", " ");
    let clean_root = root.replace("%20", " ");

    // Strip "file://" prefix if present
    let path = clean_uri.trim_start_matches("file://");
    let root_path = clean_root.trim_start_matches("file://");

    // Get absolute path string
    let path_str = path.to_string();

    // Try to strip root
    if path_str.starts_with(root_path) {
        let rel = &path_str[root_path.len()..];
        // Strip leading slash
        let rel = rel.trim_start_matches('/');
        rel.to_string()
    } else {
        path_str
    }
}

pub fn to_absolute_uri(rel_path: &str, root: &str) -> String {
    // If it's already absolute (external lib), leave it
    if rel_path.starts_with("/") {
        return format!("file://{}", rel_path);
    }

    // otherwise join
    let path = Path::new(root).join(rel_path);
    format!("file://{}", path.to_string_lossy())
}
