use crate::internal::fs::FsOps;
use std::{fs, path::Path};
use tracing::{debug, info};

pub struct FileSystem;

impl FileSystem {
    /// Recursively visits a directory to collect file paths and their contents.
    ///
    /// This function traverses the given directory and all its subdirectories, filtering out
    /// specific files or directories based on predefined rules (e.g., ignoring hidden files,
    /// certain directory names like `node_modules` or `target`). For each valid file encountered,
    /// it reads the file's content, computes its relative path from the root directory, and
    /// stores the data as a `(uri, content)` tuple in the `results` vector.
    ///
    /// # Arguments
    ///
    /// * `dir` - The directory to visit. This is the current directory being traversed.
    /// * `root` - The root directory used to compute relative paths for files.
    /// * `results` - A mutable vector to store the `(uri, content)` tuples found during traversal.
    ///
    /// # Panics
    ///
    /// This function does not explicitly panic but skips over any entries that
    /// cannot be read or processed (e.g., if `read_to_string` or `strip_prefix` fails).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::fs;
    /// use std::path::Path;
    /// use just_sync_client::adapters::fs::FileSystem;
    ///
    /// let root = Path::new("/my/project");
    /// let mut results = Vec::new();
    ///
    /// FileSystem::visit(root, root, &mut results);
    ///
    /// for (uri, content) in results {
    ///     println!("File: {} with content length {}", uri, content.len());
    /// }
    /// ```
    fn visit(dir: &Path, root: &Path, results: &mut Vec<(String, String)>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_dir = path.is_dir();

                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
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
                    Self::visit(&path, root, results);
                } else if let Ok(content) = fs::read_to_string(&path) {
                    let relative_path_cow = path
                        .strip_prefix(root)
                        .unwrap_or(&path) // Fallback to original path if strip fails
                        .to_string_lossy();

                    let relative_path = relative_path_cow.into_owned();

                    let uri = relative_path.replace('\\', "/");

                    info!("[Core] Found file {}", &uri);
                    results.push((uri, content));
                }
            }
        }
    }
}

impl FsOps for FileSystem {
    /// Recursively reads all files in a directory, returning a list of (Relative URI, Content).
    /// Skips hidden files (starting with .) and common build artifacts.
    fn scan_project_directory(&self, root: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        let root_path = Path::new(root);

        Self::visit(root_path, root_path, &mut results);
        results
    }

    /// Writes a list of files (path -> content) to the disk to CWD.
    fn write_project_files(&self, files: Vec<(String, String)>) -> anyhow::Result<()> {
        for (path_str, content) in files {
            if path_str.trim().is_empty() || path_str == "/" {
                debug!("[Core] Ignoring empty file path");
                continue;
            }

            info!("[Core] Found file: {}", path_str);

            // Ensure we are writing relatively to CWD
            let path = Path::new(&path_str);

            // Safety check: Prevent writing outside project (e.g. "../../../etc/passwd")
            if path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                debug!("[Core] Skipped unsafe path: {}", path_str);
                continue;
            }

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(path, content)?;
            info!("[Core] Wrote: {}", path_str);
        }
        Ok(())
    }
}
