use std::{fs, path::Path, vec};

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
        println!(">> [FS] Wrote: {}", path_str);
    }
    Ok(())
}
