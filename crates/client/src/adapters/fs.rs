use std::{fs, path::Path};

use crate::{internal::fs::FsOps, logger};

pub struct FileSystem;

impl FsOps for FileSystem {
    /// Recursively reads all files in a directory, returning (Relative URI, Content).
    /// Skips hidden files (starting with .) and common build artifacts.
    fn scan_project_directory(&self, root: &str) -> Vec<(String, String)> {
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

    fn write_project_files(&self, files: Vec<(String, String)>) -> anyhow::Result<()> {
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
}
