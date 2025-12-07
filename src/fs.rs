use std::{fs, path::Path, vec};

use crate::logger;

// some change here too?
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
    let path_part = if uri.starts_with("file://") {
        &uri[7..] // strip file prefix
    } else {
        uri
    };

    Path::new(path_part)
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path_part.to_string())
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
use std::{fs, path::{Path}, vec};

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
    let path_part = if uri.starts_with("file://") {
        &uri[7..] // strip file prefix
    } else {
        uri
    };

    Path::new(path_part)
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path_part.to_string())
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
