use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "avi", "mkv", "mp4", "mov", "wmv", "flv", "webm", "m4v", "ts", "mts", "m2ts", "3gp", "divx", "vob"
];

pub fn scan_inputs<P: AsRef<Path>>(
    inputs: &[P],
    extensions: &[String],
    recursive: bool,
) -> Vec<PathBuf> {
    let valid_exts: HashSet<String> = extensions
        .iter()
        .map(|e| e.trim_start_matches('.').to_lowercase())
        .collect();

    let mut files = Vec::new();

    for input in inputs {
        let path = input.as_ref();
        if path.is_file() {
            // Direct file input (e.g., qBittorrent %F passing a single file path)
            if is_matching_extension(path, &valid_exts) || is_any_video_extension(path) {
                files.push(path.to_path_buf());
            }
        } else if path.is_dir() {
            // Directory input (e.g., qBittorrent %F passing a folder path)
            let walker = WalkDir::new(path).max_depth(if recursive { usize::MAX } else { 1 });
            for entry in walker.into_iter().filter_map(|e| e.ok()) {
                let entry_path = entry.path();
                if entry_path.is_file() && is_matching_extension(entry_path, &valid_exts) {
                    files.push(entry_path.to_path_buf());
                }
            }
        }
    }

    files.sort();
    files.dedup();
    files
}

fn is_matching_extension(path: &Path, valid_exts: &HashSet<String>) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        valid_exts.contains(&ext.to_lowercase())
    } else {
        false
    }
}

fn is_any_video_extension(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        DEFAULT_EXTENSIONS.contains(&ext_lower.as_str())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_is_matching_extension() {
        let mut exts = HashSet::new();
        exts.insert("mp4".to_string());
        exts.insert("mkv".to_string());

        assert!(is_matching_extension(Path::new("video.mp4"), &exts));
        assert!(is_matching_extension(Path::new("movie.MKV"), &exts));
        assert!(!is_matching_extension(Path::new("audio.mp3"), &exts));
        assert!(!is_matching_extension(Path::new("file_without_ext"), &exts));
    }

    #[test]
    fn test_scan_single_file() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("single_movie.mkv");
        File::create(&file1).unwrap();

        let exts = vec!["mp4".to_string()];
        let found = scan_inputs(&[&file1], &exts, true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], file1);
    }

    #[test]
    fn test_scan_directory() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("a.avi");
        let file2 = dir.path().join("b.mkv");
        let file3 = dir.path().join("c.txt");
        File::create(&file1).unwrap();
        File::create(&file2).unwrap();
        File::create(&file3).unwrap();

        let exts = vec!["avi".to_string(), "mkv".to_string()];
        let found = scan_inputs(&[dir.path()], &exts, true);

        assert_eq!(found.len(), 2);
        assert!(found.contains(&file1));
        assert!(found.contains(&file2));
        assert!(!found.contains(&file3));
    }
}
