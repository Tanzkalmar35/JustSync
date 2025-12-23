use std::{
    fs,
    path::{self, Path},
    vec,
};

use tokio::runtime::Runtime;

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

pub fn to_relative_path(uri: &str, root: &str) -> String {
    // Simple decoding (replace %20 with space if needed)
    let clean_uri = uri.replace("%20", " ");
    let clean_root = root.replace("%20", " ");

    // Strip "file://" prefix if present
    let path = clean_uri.trim_start_matches("file://");
    let root_path = clean_root.trim_start_matches("file://");

    // Get absolute path string
    let path_str = path.to_string();

    logger::log(&format!("Clean URI: {}, Root: {}", path_str, root_path));

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

/// Recursively reads all files in a directory, returning (Relative URI, Content).
/// Skips hidden files (starting with .) and common build artifacts.
pub fn scan_project_directory(root: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let root_path = Path::new(root);

    fn visit(dir: &Path, root: &Path, results: &mut Vec<(String, String)>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                let is_dir = path.is_dir();
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };

                if file_name.starts_with('.')
                    || file_name == "target"
                    || file_name == "node_modules"
                    || file_name == "dist"
                    || file_name == "_build"
                {
                    continue;
                }

                if is_dir {
                    visit(&path, root, results);
                } else {
                    if let Ok(content) = fs::read_to_string(&path) {
                        // --- FIX STARTS HERE ---
                        // Safely attempt to strip the prefix.
                        // If it fails (e.g. root is "." and path is "src/main.rs"),
                        // we likely just want the path as is.
                        let relative_path_cow = path
                            .strip_prefix(root)
                            .unwrap_or(&path) // Fallback to original path if strip fails
                            .to_string_lossy();

                        // Convert Cow<str> to String
                        let relative_path = relative_path_cow.into_owned();
                        // --- FIX ENDS HERE ---

                        let uri = relative_path.replace("\\", "/");

                        logger::log(&format!("Found file {}", &uri));
                        results.push((uri, content));
                    }
                }
            }
        }
    }

    visit(root_path, root_path, &mut results);
    results
}

pub fn write_project_files(files: Vec<(String, String)>) -> anyhow::Result<()> {
    for (path_str, content) in files {
        if path_str.trim().is_empty() || path_str == "/" {
            logger::log("Ignoring empty file path");
            continue;
        } else {
            logger::log(&format!(">> [FS DEBUG] Found file: {}", path_str));
        }

        // Ensure we are writing relatively to CWD
        let path = Path::new(&path_str);

        // Safety check: Prevent writing outside project (e.g. "../../../etc/passwd")
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            crate::logger::log(&format!("!! [FS] Skipped unsafe path: {}", path_str));
            continue;
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, content)?;
        crate::logger::log(&format!(">> [FS] Wrote: {}", path_str));
    }
    Ok(())
}
