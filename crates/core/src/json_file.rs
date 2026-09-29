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

/// One of the manager's own files, as loaded.
#[derive(Debug)]
pub struct Owned<T> {
    /// None when the file does not exist -- or did not parse.
    pub value: Option<T>,
    /// Where a file that did not parse was moved, so the next save cannot
    /// overwrite what the user wrote by hand.
    pub set_aside: Option<std::path::PathBuf>,
}

/// Read a file only the manager writes. One that exists but does not parse
/// (a hand edit with a stray comma) is renamed to `<name>.bad-<unix time>`
/// and reads as absent: starting empty is recoverable, overwriting is not.
pub fn read_owned<T: DeserializeOwned>(path: &Path) -> Owned<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Owned { value: None, set_aside: None },
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Owned { value: Some(value), set_aside: None },
        Err(_) => {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let mut aside = path.as_os_str().to_owned();
            aside.push(format!(".bad-{secs}"));
            let aside = std::path::PathBuf::from(aside);
            let set_aside = fs::rename(path, &aside).is_ok().then_some(aside);
            Owned { value: None, set_aside }
        }
    }
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
    fn an_owned_file_that_does_not_parse_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine.json");
        fs::write(&path, "[1,]").unwrap();
        let got: Owned<Vec<u32>> = read_owned(&path);
        assert!(got.value.is_none());
        let aside = got.set_aside.unwrap();
        assert_eq!(fs::read_to_string(aside).unwrap(), "[1,]");
        assert!(!path.exists());
        let missing: Owned<Vec<u32>> = read_owned(&path);
        assert!(missing.value.is_none() && missing.set_aside.is_none());
    }

    #[test]
    fn a_corrupt_file_reads_as_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(read_opt::<Map>(&path), Some(Map::new()));
    }
}
