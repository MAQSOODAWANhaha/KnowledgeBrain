//! Local object I/O anchored to directory descriptors. Path checks alone are
//! insufficient: an intermediate symlink must never redirect a later open.
use crate::DeploymentNamespaceV1;
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
};

pub(crate) fn object_name(value: &str) -> io::Result<&str> {
    // Existing derived Markdown objects use a .md suffix. Neither source nor
    // derived object names may contain a path, URI delimiter or encoded path.
    if value.is_empty()
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid object filename",
        ));
    }
    Ok(value)
}

fn c_name(value: &std::ffi::OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path component"))
}

fn open_at(parent: &File, name: &CStr, flags: i32, mode: libc::mode_t) -> io::Result<File> {
    // SAFETY: parent owns a live descriptor; name is NUL terminated; a returned
    // descriptor is owned exactly once by File. No pointer escapes this call.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn directory_at(parent: &File, name: &CStr, create: bool) -> io::Result<File> {
    match open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
        Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
            // SAFETY: live parent descriptor and NUL-terminated name.
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
            open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
        }
        result => result,
    }
}

fn regular_file(parent: &File, name: &CStr) -> io::Result<File> {
    let file = open_at(parent, name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "object must be a regular, singly linked file",
        ));
    }
    Ok(file)
}

pub(crate) struct LocalObjectStore {
    root: PathBuf,
    pub(crate) namespace: DeploymentNamespaceV1,
}

impl LocalObjectStore {
    pub(crate) fn new(root: PathBuf, namespace: DeploymentNamespaceV1) -> io::Result<Self> {
        if root.as_os_str().is_empty()
            || root.components().any(|p| {
                matches!(
                    p,
                    Component::ParentDir | Component::CurDir | Component::Prefix(_)
                )
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object root contains an invalid component",
            ));
        }
        Ok(Self { root, namespace })
    }

    pub(crate) fn from_environment() -> io::Result<Self> {
        let root = std::env::var_os("OBJECT_DIR")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "OBJECT_DIR is required"))?;
        let namespace = DeploymentNamespaceV1::from_environment().map_err(io::Error::other)?;
        Self::new(root.into(), namespace)
    }

    pub(crate) fn path(&self, name: &str) -> io::Result<PathBuf> {
        Ok(self
            .root
            .join(self.namespace.storage_label())
            .join("objects")
            .join(object_name(name)?))
    }

    fn root_directory(&self, create: bool) -> io::Result<File> {
        let mut parent = File::open(if self.root.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        })?;
        for component in self.root.components() {
            if let Component::Normal(name) = component {
                parent = directory_at(&parent, &c_name(name)?, create)?;
            }
        }
        Ok(parent)
    }

    fn directory(&self, create: bool) -> io::Result<File> {
        let mut parent = self.root_directory(create)?;
        parent = directory_at(
            &parent,
            &CString::new(self.namespace.storage_label()).unwrap(),
            create,
        )?;
        directory_at(&parent, c"objects", create)
    }

    /// Observe existing directories using the same no-follow descriptor walk as
    /// object I/O. A missing configured root is an error; a missing namespace or
    /// objects child is an explicit observation, never a reason to create it.
    pub(crate) fn reset_directories(&self) -> io::Result<(File, Option<File>, Option<File>)> {
        let root = self.root_directory(false)?;
        let namespace = match directory_at(
            &root,
            &CString::new(self.namespace.storage_label()).unwrap(),
            false,
        ) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((root, None, None)),
            Err(error) => return Err(error),
        };
        let objects = match directory_at(&namespace, c"objects", false) {
            Ok(directory) => Some(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok((root, Some(namespace), objects))
    }

    pub(crate) fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let name = CString::new(object_name(name)?).unwrap();
        let directory = self.directory(false)?;
        let mut bytes = Vec::new();
        regular_file(&directory, &name)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn exists(&self, name: &str) -> io::Result<bool> {
        let name = CString::new(object_name(name)?).unwrap();
        let result = self
            .directory(false)
            .and_then(|directory| regular_file(&directory, &name));
        match result {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn write(&self, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.path(name)?;
        let name = CString::new(name).unwrap();
        let directory = self.directory(true)?;
        match regular_file(&directory, &name) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let temporary = CString::new(format!(".{}.tmp", uuid::Uuid::new_v4())).unwrap();
        let mut file = open_at(
            &directory,
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            // SAFETY: both filenames and the shared parent descriptor are live.
            if unsafe {
                libc::renameat(
                    directory.as_raw_fd(),
                    temporary.as_ptr(),
                    directory.as_raw_fd(),
                    name.as_ptr(),
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
            }
            directory.sync_all()?;
            Ok(path)
        })();
        if result.is_err() {
            // SAFETY: unlink only this operation's random temporary filename.
            unsafe {
                libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0);
            }
        }
        result
    }

    pub(crate) fn remove(&self, name: &str) -> io::Result<()> {
        let name = CString::new(object_name(name)?).unwrap();
        let directory = self.directory(false)?;
        regular_file(&directory, &name)?;
        // SAFETY: unlink is relative to the opened namespace directory and
        // does not follow the final component if it is concurrently replaced.
        if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        directory.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("kb-object-scope-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn store(&self) -> LocalObjectStore {
            LocalObjectStore::new(
                self.0.join("root"),
                uuid::Uuid::new_v4().to_string().parse().unwrap(),
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn namespaces_isolate_source_and_derived_object_reads_writes_and_removal() {
        let fixture = Fixture::new();
        let first = fixture.store();
        let second = fixture.store();
        for name in ["shared", "shared.md"] {
            let path = first.write(name, b"first").unwrap();
            assert!(
                path.starts_with(
                    fixture
                        .0
                        .join("root")
                        .join(first.namespace.storage_label())
                        .join("objects")
                )
            );
            assert_eq!(
                second.read(name).unwrap_err().kind(),
                io::ErrorKind::NotFound
            );
            second.write(name, b"second").unwrap();
            first.write(name, b"updated first").unwrap();
            assert_eq!(first.read(name).unwrap(), b"updated first");
            assert_eq!(second.read(name).unwrap(), b"second");
            first.remove(name).unwrap();
            assert!(!first.exists(name).unwrap());
            assert!(second.exists(name).unwrap());
            assert_eq!(second.read(name).unwrap(), b"second");
        }
        assert_eq!(
            fs::read_dir(first.path("shared").unwrap().parent().unwrap())
                .unwrap()
                .count(),
            0,
            "atomic writes must not leak temporary files"
        );
    }

    #[test]
    fn object_paths_reject_traversal_and_do_not_create_directories_on_read() {
        let fixture = Fixture::new();
        let store = fixture.store();
        for name in [
            "",
            ".",
            "..",
            "../outside",
            "/outside",
            "a/b",
            "a\\b",
            "%2e%2e",
            "a?x",
            "a#x",
            "a\0b",
        ] {
            assert!(store.path(name).is_err());
            assert!(store.read(name).is_err());
            assert!(store.write(name, b"invalid").is_err());
            assert!(store.remove(name).is_err());
        }
        assert_eq!(
            store.read("absent").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!fixture.0.join("root").exists());
        assert!(LocalObjectStore::new(fixture.0.join("../outside"), store.namespace).is_err());
    }

    #[test]
    fn symlinks_at_every_level_and_hard_linked_objects_cannot_escape_scope() {
        for level in 0..4 {
            let fixture = Fixture::new();
            let store = fixture.store();
            let object = store.write("object", b"inside").unwrap();
            let outside = fixture.0.join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("object"), b"outside").unwrap();
            let attacked = match level {
                0 => fixture.0.join("root"),
                1 => fixture.0.join("root").join(store.namespace.storage_label()),
                2 => object.parent().unwrap().to_path_buf(),
                _ => object,
            };
            if level == 3 {
                fs::remove_file(&attacked).unwrap();
                symlink(outside.join("object"), &attacked).unwrap();
            } else {
                fs::remove_dir_all(&attacked).unwrap();
                symlink(&outside, &attacked).unwrap();
            }
            assert!(store.read("object").is_err());
            assert!(store.write("object", b"must not escape").is_err());
            assert!(store.remove("object").is_err());
            assert_eq!(fs::read(outside.join("object")).unwrap(), b"outside");
            assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        }
        let fixture = Fixture::new();
        let store = fixture.store();
        let object = store.write("object", b"protected").unwrap();
        let linked = fixture.0.join("outside-link");
        fs::hard_link(&object, &linked).unwrap();
        assert!(store.read("object").is_err());
        assert!(store.write("object", b"changed").is_err());
        assert!(store.remove("object").is_err());
        assert_eq!(fs::read(linked).unwrap(), b"protected");
    }
}
