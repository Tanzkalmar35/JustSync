use std::{fs, path::Path};

use crate::logger;

pub fn to_relative_path(uri: &str, root: &str) -> String {
    let clean_uri = uri.replace("%20", " ");
    let clean_root = root.replace("%20", " ");

    let path_str = clean_uri.strip_prefix("file://").unwrap_or(&clean_uri);
    let root_str = clean_root.strip_prefix("file://").unwrap_or(&clean_root);

    let path_norm = path_str.replace('\\', "/");
    let root_norm = root_str.replace('\\', "/");

    let path = Path::new(&path_norm);
    let root = Path::new(&root_norm);

    // Try standard path stripping
    if let Ok(relative) = path.strip_prefix(root) {
        return relative.to_string_lossy().replace('\\', "/");
    }

    // Windows Fallback (Case Insensitivity)
    let p_lower = path_norm.to_lowercase();
    let r_lower = root_norm.to_lowercase();

    if p_lower.starts_with(&r_lower) {
        // Check if the next character is a separator or if the string ends there.
        let match_len = r_lower.len();
        let boundary_char = p_lower.chars().nth(match_len);

        if boundary_char.is_none() || boundary_char == Some('/') {
            let rel = &path_norm[match_len..];
            return rel.trim_start_matches('/').to_string();
        }
    }

    path_norm
}

pub fn to_absolute_uri(rel_path: &str, root: &str) -> String {
    // Already a URI
    if rel_path.starts_with("file://") {
        return rel_path.replace('\\', "/");
    }

    // Windows Absolute Path (C:\...)
    if rel_path.len() > 1 && rel_path.chars().nth(1) == Some(':') {
        // FIX: Windows URIs need 3 slashes: file:///C:/...
        return format!("file:///{}", rel_path.replace('\\', "/"));
    }

    // Unix Absolute Path (/usr/...)
    if rel_path.starts_with('/') || rel_path.starts_with('\\') {
        return format!("file://{}", rel_path.replace('\\', "/"));
    }

    // Relative Path -> Join with Root
    let clean_root = root.trim_start_matches("file://");
    let root_norm = clean_root.replace('\\', "/");
    let rel_norm = rel_path.replace('\\', "/");

    let path = Path::new(&root_norm).join(&rel_norm);
    let full_path = path.to_string_lossy().replace('\\', "/");

    format!("file://{}", full_path)
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
                    || file_name.starts_with("oil://")
                    || file_name == "target"
                    || file_name == "node_modules"
                    || file_name == "dist"
                    || file_name == "_build"
                {
                    continue;
                }

                if is_dir {
                    visit(&path, root, results);
                } else if let Ok(content) = fs::read_to_string(&path) {
                    // Safely attempt to strip the prefix.
                    // If it fails (e.g. root is "." and path is "src/main.rs"),
                    // we likely just want the path as is.
                    let relative_path_cow = path
                        .strip_prefix(root)
                        .unwrap_or(&path) // Fallback to original path if strip fails
                        .to_string_lossy();

                    // Convert Cow<str> to String
                    let relative_path = relative_path_cow.into_owned();

                    let uri = relative_path.replace("\\", "/");

                    logger::log(&format!("Found file {}", &uri));
                    results.push((uri, content));
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
