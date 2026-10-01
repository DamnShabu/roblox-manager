//! A downloaded AppImage's files, read straight out of it rather than by
//! running it. An AppImage is a small ELF runtime with a SquashFS image
//! appended; `--appimage-extract` has that runtime unpack itself, which fails
//! wherever running a downloaded program does -- NixOS hands every AppImage
//! to appimage-run through binfmt, and a sandboxed manager cannot see it
//! ("cannot execute: required file not found"). Reading the image needs
//! nothing from the host.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};

use backhand::{FilesystemReader, InnerNode};

use crate::cordial::CordialError;

/// SquashFS's magic, little-endian: every AppImage runtime appends that kind.
const SQUASHFS: &[u8; 4] = b"hsqs";

/// Unpack `appimage` into `into`, which must not exist yet.
pub fn unpack(appimage: &Path, into: &Path) -> Result<(), CordialError> {
    let mut file = File::open(appimage)
        .map_err(|e| CordialError::Io(format!("could not open {}: {e}", appimage.display())))?;
    let offset = image_offset(&mut file)?;
    let image = FilesystemReader::from_reader_with_offset(BufReader::new(file), offset)
        .map_err(|e| unpack_error(e.to_string()))?;
    fs::create_dir(into).map_err(|e| io_error("could not create", into, e))?;
    // Set last: a directory the image marks read-only still takes its files.
    let mut dirs = Vec::new();
    for node in image.files() {
        let Some(relative) = inside(&node.fullpath)? else { continue };
        let path = into.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error("could not create", parent, e))?;
        }
        let mode = u32::from(node.header.permissions) & 0o777;
        match &node.inner {
            InnerNode::Dir(_) => {
                fs::create_dir_all(&path).map_err(|e| io_error("could not create", &path, e))?;
                // Still the owner's to delete when a newer version replaces it.
                dirs.push((path, mode | 0o700));
            }
            InnerNode::File(contents) => {
                let mut out =
                    File::create(&path).map_err(|e| io_error("could not create", &path, e))?;
                io::copy(&mut image.file(contents).reader(), &mut out)
                    .map_err(|e| unpack_error(format!("{}: {e}", relative.display())))?;
                set_mode(&path, mode)?;
            }
            InnerNode::Symlink(link) => {
                symlink(&link.link, &path).map_err(|e| io_error("could not link", &path, e))?;
            }
            // Devices, pipes and sockets: nothing a program needs from its image.
            _ => {}
        }
    }
    dirs.iter().try_for_each(|(dir, mode)| set_mode(dir, *mode))
}

/// Where the image starts: right after the runtime, whose ELF header says
/// where its section headers -- its last part -- end.
fn image_offset(file: &mut File) -> Result<u64, CordialError> {
    let not_one = || unpack_error("not an AppImage".into());
    let mut header = [0u8; 64];
    file.read_exact(&mut header).map_err(|_| not_one())?;
    if &header[..4] != b"\x7fELF" {
        return Err(not_one());
    }
    let big = header[5] == 2;
    let field = |at: usize, len: usize| {
        header[at..at + len]
            .iter()
            .enumerate()
            .map(|(i, b)| u64::from(*b) << (8 * if big { len - 1 - i } else { i }))
            .sum::<u64>()
    };
    // (e_shoff, e_shentsize, e_shnum) for 32- and 64-bit ELF.
    let (shoff, entsize, count) = match header[4] {
        1 => (field(0x20, 4), field(0x2e, 2), field(0x30, 2)),
        2 => (field(0x28, 8), field(0x3a, 2), field(0x3c, 2)),
        _ => return Err(not_one()),
    };
    let offset = entsize.checked_mul(count).and_then(|len| shoff.checked_add(len));
    let offset = offset.ok_or_else(not_one)?;
    let mut magic = [0u8; 4];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut magic))
        .map_err(|_| not_one())?;
    if &magic != SQUASHFS {
        return Err(not_one());
    }
    Ok(offset)
}

/// The node's path under the image's root, `None` for the root itself. A
/// path that would climb out of it is refused.
fn inside(fullpath: &Path) -> Result<Option<PathBuf>, CordialError> {
    let mut relative = PathBuf::new();
    for part in fullpath.components() {
        match part {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => relative.push(name),
            Component::ParentDir | Component::Prefix(_) => {
                return Err(unpack_error(format!("it holds a path outside it, {fullpath:?}")));
            }
        }
    }
    Ok((!relative.as_os_str().is_empty()).then_some(relative))
}

fn set_mode(path: &Path, mode: u32) -> Result<(), CordialError> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| io_error("could not set the permissions of", path, e))
}

fn unpack_error(why: String) -> CordialError {
    CordialError::Stacked(format!("could not unpack the AppImage ({why})"))
}

fn io_error(what: &str, path: &Path, e: io::Error) -> CordialError {
    CordialError::Io(format!("{what} {}: {e}", path.display()))
}

#[cfg(test)]
pub(super) mod fake {
    //! AppImages for the tests: an ELF header whose section headers end at
    //! byte 128, and the SquashFS image there.

    use std::io::Cursor;

    use backhand::compression::Compressor;
    use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};

    /// Where the fake runtime ends.
    pub const RUNTIME_LEN: u64 = 128;

    /// An AppImage holding `files` (path, contents, mode) and `links`
    /// (path, target).
    pub fn appimage(files: &[(&str, &[u8], u16)], links: &[(&str, &str)]) -> Vec<u8> {
        let mut out = vec![0u8; RUNTIME_LEN as usize];
        out[..4].copy_from_slice(b"\x7fELF");
        out[4] = 2; // 64-bit
        out[5] = 1; // little-endian
        out[0x28] = 64; // e_shoff
        out[0x3a] = 64; // e_shentsize
        out[0x3c] = 1; // e_shnum
        let mut image = FilesystemWriter::default();
        image.set_compressor(FilesystemCompressor::new(Compressor::Zstd, None).unwrap());
        for (path, contents, mode) in files {
            let dir = std::path::Path::new(path).parent().unwrap();
            image.push_dir_all(dir, NodeHeader::new(0o755, 0, 0, 0)).unwrap();
            image
                .push_file(Cursor::new(contents.to_vec()), path, NodeHeader::new(*mode, 0, 0, 0))
                .unwrap();
        }
        for (path, target) in links {
            image.push_symlink(target, path, NodeHeader::new(0o777, 0, 0, 0)).unwrap();
        }
        let mut cursor = Cursor::new(out);
        image.write_with_offset(&mut cursor, RUNTIME_LEN).unwrap();
        cursor.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpacked(appimage: &[u8]) -> (tempfile::TempDir, Result<(), CordialError>) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.AppImage");
        fs::write(&file, appimage).unwrap();
        let got = unpack(&file, &dir.path().join("root"));
        (dir, got)
    }

    #[test]
    fn its_files_come_out_with_their_modes_and_links() {
        let image = fake::appimage(
            &[("usr/bin/cordial-run", b"#!/bin/sh\n", 0o755), ("usr/lib/libx.so.1", b"lib", 0o644)],
            &[("usr/lib/libx.so", "libx.so.1")],
        );
        let (dir, got) = unpacked(&image);
        got.unwrap();
        let root = dir.path().join("root");
        let engine = root.join("usr/bin/cordial-run");
        assert_eq!(fs::read(&engine).unwrap(), b"#!/bin/sh\n");
        assert_eq!(fs::metadata(&engine).unwrap().permissions().mode() & 0o777, 0o755);
        let lib = root.join("usr/lib/libx.so.1");
        assert_eq!(fs::metadata(&lib).unwrap().permissions().mode() & 0o777, 0o644);
        assert_eq!(fs::read_link(root.join("usr/lib/libx.so")).unwrap(), Path::new("libx.so.1"));
        assert_eq!(fs::read(root.join("usr/lib/libx.so")).unwrap(), b"lib");
    }

    #[test]
    fn what_is_no_appimage_says_so() {
        let not_one =
            CordialError::Stacked("could not unpack the AppImage (not an AppImage)".into());
        assert_eq!(unpacked(b"#!/bin/sh\nexit 1\n").1.unwrap_err(), not_one);
        // An ELF runtime with nothing after it.
        let mut bare = fake::appimage(&[], &[]);
        bare.truncate(fake::RUNTIME_LEN as usize);
        assert_eq!(unpacked(&bare).1.unwrap_err(), not_one);
    }

    #[test]
    fn a_path_out_of_the_image_is_refused() {
        assert!(inside(Path::new("/usr/../../etc")).is_err());
        assert_eq!(inside(Path::new("/")).unwrap(), None);
        assert_eq!(inside(Path::new("/usr/bin")).unwrap(), Some(PathBuf::from("usr/bin")));
    }
}
