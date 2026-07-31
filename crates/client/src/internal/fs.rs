use std::path::Path;

pub trait FsOps {
    /// Scans a project directory for its content.
    ///
    /// # Arguments
    ///
    /// * `root` - The path to the root of the project.
    ///
    /// # Returns
    ///
    /// A list of tuples (uri -> content) representing scanned documents.
    fn scan_project_directory(&self, root: &str) -> Vec<(String, String)>;

    /// Writes a list of project files to local disk
    ///
    /// # Arguments
    ///
    /// * `files` - The list of files to write to disk
    fn write_project_files(&self, files: Vec<(String, String)>) -> anyhow::Result<()>;
}

/// Converts a given path to a valid path that is relative to the given root.
///
/// # Arguments
///
/// * `uri` - The path to make relative.
/// * `root` - The root marker.
///
/// # Returns
///
/// The relative part of the given uri, based on the root.
#[must_use]
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
