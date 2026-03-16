//! Image handling — clipboard paste and file path detection.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tiff"];

/// Grab image from Wayland clipboard via wl-paste.
/// Saves to the given path and returns true if an image was found.
pub fn paste_clipboard_image(dest: &Path) -> Result<(), String> {
    // Check if clipboard has image data
    let types = Command::new("wl-paste")
        .arg("--list-types")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|_| "wl-paste not found. Install wl-clipboard: pacman -S wl-clipboard".to_string())?;

    let types_str = String::from_utf8_lossy(&types.stdout);
    let has_image = types_str.lines().any(|t| t.starts_with("image/"));

    if !has_image {
        return Err("No image in clipboard. Copy an image first.".to_string());
    }

    // Determine the best image type
    let mime = if types_str.lines().any(|t| t == "image/png") {
        "image/png"
    } else if types_str.lines().any(|t| t == "image/jpeg") {
        "image/jpeg"
    } else {
        // Take whatever image type is available
        types_str
            .lines()
            .find(|t| t.starts_with("image/"))
            .unwrap_or("image/png")
    };

    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let output = Command::new("wl-paste")
        .args(["--type", mime, "--no-newline"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("wl-paste failed: {e}"))?;

    if !output.status.success() || output.stdout.is_empty() {
        return Err("Failed to read image from clipboard".to_string());
    }

    std::fs::write(dest, &output.stdout)
        .map_err(|e| format!("Failed to save clipboard image: {e}"))?;

    Ok(())
}

/// Extract image file paths from user input text.
/// Returns (cleaned text without paths, list of image paths found).
pub fn extract_image_paths(input: &str) -> (String, Vec<PathBuf>) {
    let mut images = Vec::new();
    let mut text_parts = Vec::new();

    for word in input.split_whitespace() {
        // Expand ~ to home directory
        let expanded = if word.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(&word[2..]).to_string_lossy().to_string()
            } else {
                word.to_string()
            }
        } else {
            word.to_string()
        };

        let path = Path::new(&expanded);
        if path.exists() && is_image_path(path) {
            images.push(path.to_path_buf());
        } else if is_image_path(Path::new(word)) && Path::new(&expanded).exists() {
            images.push(PathBuf::from(&expanded));
        } else {
            text_parts.push(word);
        }
    }

    (text_parts.join(" "), images)
}

/// Check if a path looks like an image file (by extension).
fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_path() {
        assert!(is_image_path(Path::new("photo.png")));
        assert!(is_image_path(Path::new("photo.JPG")));
        assert!(is_image_path(Path::new("/tmp/sketch.jpeg")));
        assert!(!is_image_path(Path::new("model.stl")));
        assert!(!is_image_path(Path::new("code.py")));
    }

    #[test]
    fn test_extract_no_images() {
        let (text, images) = extract_image_paths("make a 10mm cube");
        assert_eq!(text, "make a 10mm cube");
        assert!(images.is_empty());
    }
}
