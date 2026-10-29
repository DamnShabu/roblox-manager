//! The manager's JSON files: read leniently, written whole and atomically.

use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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
///
/// Only a file that does not exist reads as absent. Any other failure to read
/// it (permissions, an I/O error, running out of descriptors) is an error, as
/// is a broken file that could not be moved aside: reading either as empty
/// would let the next save replace the user's data with nothing.
pub fn read_owned<T: DeserializeOwned>(path: &Path) -> io::Result<Owned<T>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Owned { value: None, set_aside: None });
        }
        Err(e) => return Err(io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Owned { value: Some(value), set_aside: None }),
        Err(parse) => {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let mut aside = path.as_os_str().to_owned();
            aside.push(format!(".bad-{secs}"));
            let aside = std::path::PathBuf::from(aside);
            fs::rename(path, &aside).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "{} does not read ({parse}) and could not be moved aside: {e}",
                        path.display()
                    ),
                )
            })?;
            Ok(Owned { value: None, set_aside: Some(aside) })
        }
    }
}

/// Write-then-rename, so a crash mid-write never leaves half a file: the new
/// contents reach the disk before the rename publishes them, and the rename
/// reaches it before this returns. Without the syncs a power cut shortly
/// after a save can leave a zero-length file on filesystems that do not
/// order the two (XFS, btrfs), which then reads as "no accounts". The
/// temporary name is unique per write, so two writers never share one.
/// Creates the parent directories.
pub fn write<T: Serialize>(path: &Path, data: &T) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
    write_bytes(path, &json)
}

/// [`write`] for contents that are not JSON.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let tmp = std::path::PathBuf::from(tmp);
    let written = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        // Best-effort tidy-up; the write's own error is the one reported.
        let _ = fs::remove_file(&tmp);
    }
    written?;
    sync_dir(dir)
}

/// Make a rename in `dir` durable. A filesystem that cannot sync a
/// directory (some FUSE and network ones say so with EINVAL) has nothing
/// more to give; any other failure is reported.
fn sync_dir(dir: &Path) -> io::Result<()> {
    match fs::File::open(dir).and_then(|d| d.sync_all()) {
        Err(e) if matches!(e.kind(), io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported) => {
            Ok(())
        }
        other => other,
    }
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
        let left: Vec<_> = fs::read_dir(dir.path().join("a/b")).unwrap().collect();
        assert_eq!(left.len(), 1, "a temporary file was left behind");
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
        let got: Owned<Vec<u32>> = read_owned(&path).unwrap();
        assert!(got.value.is_none());
        let aside = got.set_aside.unwrap();
        assert_eq!(fs::read_to_string(aside).unwrap(), "[1,]");
        assert!(!path.exists());
        let missing: Owned<Vec<u32>> = read_owned(&path).unwrap();
        assert!(missing.value.is_none() && missing.set_aside.is_none());
    }

    #[test]
    fn a_corrupt_file_reads_as_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(read_opt::<Map>(&path), Some(Map::new()));
    }

    #[test]
    fn an_owned_file_that_cannot_be_read_is_an_error_not_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: reading fails, and not
        // with NotFound -- the same branch EACCES and EIO take.
        let path = dir.path().join("accounts.json");
        fs::create_dir(&path).unwrap();
        assert!(read_owned::<Vec<u32>>(&path).is_err());
        assert!(path.is_dir(), "an unreadable file must be left where it is");
    }

    #[test]
    fn a_broken_file_that_cannot_be_moved_aside_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine.json");
        fs::write(&path, "[1,]").unwrap();
        // A read-only directory refuses the rename.
        let mut perms = fs::metadata(dir.path()).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500);
        fs::set_permissions(dir.path(), perms.clone()).unwrap();
        let got = read_owned::<Vec<u32>>(&path);
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o700);
        fs::set_permissions(dir.path(), perms).unwrap();
        // Root ignores directory permissions and can still move it aside.
        if rustix::process::geteuid().is_root() {
            assert!(got.unwrap().set_aside.is_some());
        } else {
            assert!(got.is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), "[1,]");
        }
    }
}
