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

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // =========================================================================
    //  to_relative_path
    // =========================================================================

    #[test]
    fn test_rel_basic_unix() {
        let root = "file:///home/user/project";
        let uri = "file:///home/user/project/src/main.rs";
        assert_eq!(to_relative_path(uri, root), "src/main.rs");
    }

    #[test]
    fn test_rel_windows_backslashes_normalization() {
        // Robustness Check: Even if OS provides backslashes,
        let root = "file://C:\\Users\\Dev\\Project";
        let uri = "file://C:\\Users\\Dev\\Project\\src\\main.rs";

        // Internal logic:
        // 1. Strips file:// -> "C:\Users\..."
        // 2. Replaces \ with / -> "C:/Users/..."
        // 3. Strips prefix
        assert_eq!(to_relative_path(uri, root), "src/main.rs");
    }

    #[test]
    fn test_rel_mixed_slashes() {
        // LSP clients are messy. Sometimes they send mixed slashes.
        let root = "file:///C:/Users/Dev/Project";
        let uri = "file:///C:\\Users\\Dev\\Project/src/main.rs";

        assert_eq!(to_relative_path(uri, root), "src/main.rs");
    }

    #[test]
    fn test_rel_directory_prefix_collision() {
        let root = "file:///var/www/site";
        let uri = "file:///var/www/site_admin/config.toml";

        let result = to_relative_path(uri, root);

        assert_eq!(result, "/var/www/site_admin/config.toml");
    }

    #[test]
    fn test_rel_encoding_spaces() {
        let root = "file:///My%20Project";
        let uri = "file:///My%20Project/file.txt";
        assert_eq!(to_relative_path(uri, root), "file.txt");
    }

    #[test]
    #[cfg(windows)] // Guarded by cfg(windows), so only test it on windows
    fn test_rel_windows_case_insensitivity() {
        // SCENARIO: VS Code sends lowercase 'c:', Root has uppercase 'C:'
        let root = "file:///C:/Project";
        let uri = "file:///c:/Project/src/lib.rs";

        assert_eq!(to_relative_path(uri, root), "src/lib.rs");
    }

    // =========================================================================
    //  to_absolute_uri
    // =========================================================================

    #[test]
    fn test_abs_join_clean() {
        let root = "file:///home/user";
        let rel = "docs/readme.md";
        assert_eq!(
            to_absolute_uri(rel, root),
            "file:///home/user/docs/readme.md"
        );
    }

    #[test]
    fn test_abs_prevents_double_scheme() {
        let root = "file:///home/user";
        let rel = "src/main.rs";

        let result = to_absolute_uri(rel, root);
        assert_eq!(result, "file:///home/user/src/main.rs");
    }

    #[test]
    fn test_abs_handles_already_absolute_unix() {
        let root = "file:///home/user";
        let rel = "/usr/local/bin/config";

        let result = to_absolute_uri(rel, root);
        assert_eq!(result, "file:///usr/local/bin/config");
    }

    #[test]
    fn test_abs_handles_already_absolute_windows_style() {
        let root = "file:///C:/Users";
        let rel = "C:\\Windows\\System32\\driver.sys";

        let result = to_absolute_uri(rel, root);
        assert_eq!(result, "file:///C:/Windows/System32/driver.sys");
    }

    #[test]
    fn test_abs_idempotency() {
        let root = "file:///home/user";
        let rel = "file:///home/user/src/main.rs";

        let result = to_absolute_uri(rel, root);
        assert_eq!(result, "file:///home/user/src/main.rs");
    }

    #[test]
    fn test_abs_windows_normalization() {
        // Input has backslashes -> Output must have forward slashes
        let root = "file:///C:/Users";
        let rel = "src\\modules\\logic.rs";

        let result = to_absolute_uri(rel, root);
        assert_eq!(result, "file:///C:/Users/src/modules/logic.rs");
    }

    // =========================================================================
    //  scan_project_directory
    // =========================================================================

    /// Helper to create a file with content inside a temp dir
    fn create_file(dir: &TempDir, path: &str, content: &str) {
        let file_path = dir.path().join(path);
        // Ensure parent directories exist
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        let mut file = File::create(file_path).expect("Failed to create file");
        writeln!(file, "{:?}", content).expect("Failed to write content");
    }

    #[test]
    fn test_scan_simple_structure() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create: root/main.rs
        create_file(&temp_dir, "main.rs", "fn main() {}");
        // Create: root/README.md
        create_file(&temp_dir, "README.md", "# Docs");

        // Action: Scan the directory
        let root_str = temp_dir.path().to_str().unwrap();
        let results = scan_project_directory(root_str);

        // Assert
        assert_eq!(results.len(), 2);

        // We convert to HashSet to ignore order, as file system order is not guaranteed
        let found_files: std::collections::HashSet<_> =
            results.into_iter().map(|(path, _)| path).collect();
        assert!(found_files.contains("main.rs"));
        assert!(found_files.contains("README.md"));
    }

    #[test]
    fn test_scan_recursive_nested() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create: root/src/utils/helper.rs
        create_file(&temp_dir, "src/utils/helper.rs", "pub fn help() {}");

        let root_str = temp_dir.path().to_str().unwrap();
        let results = scan_project_directory(root_str);

        assert_eq!(results.len(), 1);
        let (path, content) = &results[0];

        // Ensure URI uses forward slashes (even on Windows)
        assert_eq!(path, "src/utils/helper.rs");
        // Note: writeln! adds a newline, so we trim for comparison or check contains
        assert!(content.trim().contains("pub fn help() {}"));
    }

    #[test]
    fn test_ignores_hidden_files_and_dot_directories() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Should be ignored (starts with .)
        create_file(&temp_dir, ".env", "SECRET=123");
        // Should be ignored (inside .git directory)
        create_file(&temp_dir, ".git/HEAD", "ref: refs/heads/main");
        // Should be found
        create_file(&temp_dir, "visible.txt", "I am seen");

        let root_str = temp_dir.path().to_str().unwrap();
        let results = scan_project_directory(root_str);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "visible.txt");
    }

    #[test]
    fn test_ignores_build_artifacts() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Folder names that are explicitly blocked in the code
        create_file(&temp_dir, "target/debug/app.exe", "binary blob");
        create_file(&temp_dir, "node_modules/react/index.js", "library code");
        create_file(&temp_dir, "dist/bundle.js", "minified code");
        create_file(&temp_dir, "_build/out", "build output");

        // Valid file
        create_file(&temp_dir, "src/main.rs", "code");

        let root_str = temp_dir.path().to_str().unwrap();
        let results = scan_project_directory(root_str);

        // Should only find src/main.rs
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "src/main.rs");
    }

    #[test]
    fn test_handles_binary_files_gracefully() {
        // fs::read_to_string returns an Error if the file is not valid UTF-8.
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let file_path = temp_dir.path().join("image.png");
        let mut file = File::create(file_path).unwrap();
        // Write invalid UTF-8 bytes
        file.write_all(&[0xFF, 0xFE, 0xFD]).unwrap();

        let root_str = temp_dir.path().to_str().unwrap();
        let results = scan_project_directory(root_str);

        // Should be empty because read_to_string failed
        assert_eq!(results.len(), 0);
    }

    // =========================================================================
    //  write_project_files
    // =========================================================================

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    // This function runs a closure inside a temporary directory.
    // It guarantees we don't accidentally write to your real hard drive.
    fn run_in_temp_dir<F>(test_fn: F)
    where
        F: FnOnce(),
    {
        // Acquire lock on testing cwd
        let _guard = CWD_LOCK.lock().unwrap();

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let original_dir = std::env::current_dir().expect("Failed to get current dir");

        // Switch to temp dir
        std::env::set_current_dir(&temp_dir).expect("Failed to change CWD");

        // Run the test
        // Using catch_unwind to ensure we switch back even if the test panics
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test_fn));

        // Switch back to original dir
        std::env::set_current_dir(&original_dir).expect("Failed to restore CWD");

        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn test_write_simple_files() {
        run_in_temp_dir(|| {
            let files = vec![
                ("main.rs".to_string(), "fn main() {}".to_string()),
                ("Cargo.toml".to_string(), "[package]".to_string()),
            ];

            let result = write_project_files(files);
            assert!(result.is_ok());

            // Verify files exist in the (temp) CWD
            assert!(Path::new("main.rs").exists());
            assert!(Path::new("Cargo.toml").exists());

            let content = fs::read_to_string("main.rs").unwrap();
            assert_eq!(content, "fn main() {}");
        });
    }

    #[test]
    fn test_create_nested_directories() {
        run_in_temp_dir(|| {
            let files = vec![(
                "src/utils/math.rs".to_string(),
                "pub fn add() {}".to_string(),
            )];

            write_project_files(files).unwrap();

            // Verify directory structure was created
            assert!(Path::new("src").is_dir());
            assert!(Path::new("src/utils").is_dir());
            assert!(Path::new("src/utils/math.rs").exists());
        });
    }

    #[test]
    fn test_security_prevents_directory_traversal() {
        run_in_temp_dir(|| {
            // SCENARIO: Malicious actor tries to write outside the project root
            let files = vec![
                ("../evil.txt".to_string(), "hacked".to_string()),
                ("src/../../oops.txt".to_string(), "hacked".to_string()),
            ];

            let result = write_project_files(files);
            assert!(result.is_ok()); // Function returns Ok, but skips unsafe files

            // Verify files were NOT written
            assert!(!Path::new("../evil.txt").exists());
        });
    }

    #[test]
    fn test_security_allows_safe_dots() {
        run_in_temp_dir(|| {
            // SCENARIO: Normal hidden files or dot-relative paths
            let files = vec![
                (".gitignore".to_string(), "/target".to_string()),
                ("./src/lib.rs".to_string(), "// code".to_string()),
            ];

            write_project_files(files).unwrap();

            assert!(Path::new(".gitignore").exists());
            assert!(Path::new("src/lib.rs").exists());
        });
    }

    #[test]
    fn test_ignores_empty_paths() {
        run_in_temp_dir(|| {
            let files = vec![
                ("".to_string(), "ignore me".to_string()),
                ("   ".to_string(), "ignore me too".to_string()),
                ("/".to_string(), "ignore root".to_string()),
            ];

            let result = write_project_files(files);
            assert!(result.is_ok());

            // Ensure nothing weird was created
            let count = fs::read_dir(".").unwrap().count();
            assert_eq!(count, 0, "Should not have created any files");
        });
    }

    #[test]
    fn test_overwrites_existing_files() {
        run_in_temp_dir(|| {
            // Create file initially
            fs::write("config.json", "{}").unwrap();

            // Overwrite it
            let files = vec![(
                "config.json".to_string(),
                "{ \"updated\": true }".to_string(),
            )];
            write_project_files(files).unwrap();

            // Verify new content
            let content = fs::read_to_string("config.json").unwrap();
            assert_eq!(content, "{ \"updated\": true }");
        });
    }
}
