//! The manager's JSON files: read leniently, written whole and atomically.

use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The file's contents, or `None` when it does not exist. A file that exists
/// but does not parse as `T` is `Some(T::default())`: the user's data is
/// never an error that stops the app from starting.
pub fn read_opt<T: DeserializeOwned + Default>(path: &Path) -> Option<T> {
    match fs::read(path) {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(_) => Some(T::default()),
    }
}

/// The file's contents; a missing or unreadable file is `T::default()`.
pub fn read<T: DeserializeOwned + Default>(path: &Path) -> T {
    read_opt(path).unwrap_or_default()
}

/// Write-then-rename, so a crash mid-write never leaves half a file.
/// Creates the parent directories.
pub fn write<T: Serialize>(path: &Path, data: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let json = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    type Map = BTreeMap<String, u32>;

    #[test]
    fn a_written_file_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/data.json");
        let data: Map = [("x".into(), 1)].into();
        write(&path, &data).unwrap();
        assert_eq!(read::<Map>(&path), data);
        assert!(!dir.path().join("a/b/data.json.tmp").exists());
    }

    #[test]
    fn a_missing_file_is_none_and_reads_as_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        assert_eq!(read_opt::<Map>(&path), None);
        assert_eq!(read::<Map>(&path), Map::new());
    }

    #[test]
    fn a_corrupt_file_reads_as_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(read_opt::<Map>(&path), Some(Map::new()));
    }
}
