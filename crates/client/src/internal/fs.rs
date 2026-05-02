use std::path::Path;

pub trait FsOps {
    fn scan_project_directory(&self, root: &str) -> Vec<(String, String)>;
    fn write_project_files(&self, files: Vec<(String, String)>) -> anyhow::Result<()>;
}

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
