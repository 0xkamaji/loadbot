//! Stable cross-process leases and same-directory file replacement.
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Debug)]
pub struct Busy {
    pub resource: PathBuf,
}
impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is busy or changed while awaiting a decision; retry the operation",
            self.resource.display()
        )
    }
}
impl std::error::Error for Busy {}

#[derive(Debug)]
pub struct DurabilityUncertain {
    pub path: PathBuf,
    source: std::io::Error,
}
impl std::fmt::Display for DurabilityUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} was replaced, but directory synchronization failed: {}",
            self.path.display(),
            self.source
        )
    }
}
impl std::error::Error for DurabilityUncertain {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Never unlink this sidecar: its inode/Windows file identity is the lock target.
/// Locks are nonblocking, advisory, and released by the OS when the handle closes.
pub struct Lease {
    file: File,
    resource: PathBuf,
    generation: u64,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Release at the operation boundary, even when a concurrent Unix fork
        // briefly inherits this file description before exec closes it.
        // Closing the handle still supplies OS cleanup on failure or crash.
        let _ = self.file.unlock();
    }
}

impl Lease {
    pub fn acquire(resource: &Path) -> Result<Self> {
        let parent = resource.parent().context("resource has no parent")?;
        fs::create_dir_all(parent)?;
        let name = resource.file_name().context("resource has no file name")?;
        let mut lock_name = std::ffi::OsString::from(".");
        lock_name.push(name);
        lock_name.push(".loadbot-lock");
        let lock_path = parent.join(lock_name);
        if fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("refusing symlink lock {}", lock_path.display());
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        let mut lease = Self {
            file,
            resource: resource.to_owned(),
            generation: 0,
        };
        lease.lock()?;
        lease.generation = lease
            .read_generation()?
            .checked_add(1)
            .context("lock generation exhausted")?;
        lease.file.seek(SeekFrom::Start(0))?;
        lease.file.write_all(&lease.generation.to_le_bytes())?;
        lease.file.set_len(8)?;
        lease.file.sync_data()?;
        Ok(lease)
    }
    fn lock(&self) -> Result<()> {
        match self.file.try_lock() {
            Ok(()) => Ok(()),
            Err(TryLockError::WouldBlock) => Err(Busy {
                resource: self.resource.clone(),
            }
            .into()),
            Err(TryLockError::Error(error)) => Err(error).context("could not lock resource"),
        }
    }
    fn read_generation(&mut self) -> Result<u64> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        (&self.file).take(9).read_to_end(&mut bytes)?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let bytes: [u8; 8] = bytes.try_into().map_err(|_| {
            anyhow::anyhow!(
                "invalid lock generation; inspect {}",
                self.resource.display()
            )
        })?;
        Ok(u64::from_le_bytes(bytes))
    }
    pub(crate) fn suspend(&self) -> Result<()> {
        self.file.unlock().map_err(Into::into)
    }
    pub(crate) fn resume(&mut self) -> Result<()> {
        self.lock()?;
        if self.read_generation()? != self.generation {
            return Err(Busy {
                resource: self.resource.clone(),
            }
            .into());
        }
        Ok(())
    }
}

/// No missing-destination window, including Windows (tempfile uses MoveFileExW).
/// An abandoned unique temporary file is never interpreted as committed data.
pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_toml_with(path, value, |temporary, path| {
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)
    })
}

fn write_toml_with<T: Serialize>(
    path: &Path,
    value: &T,
    replace: impl FnOnce(tempfile::NamedTempFile, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let contents = toml::to_string_pretty(value).context("could not serialize configuration")?;
    let parent = path.parent().context("configuration path has no parent")?;
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("refusing symlink configuration {}", path.display());
    }
    // A Phase 1 backup may be the only valid copy; never overwrite it implicitly.
    ensure_no_legacy_recovery(path)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".loadbot-write-")
        .tempfile_in(parent)?;
    if let Ok(metadata) = fs::metadata(path) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    replace(temporary, path).with_context(|| {
        format!(
            "could not replace {}; existing data was retained",
            path.display()
        )
    })?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| DurabilityUncertain {
            path: path.to_owned(),
            source,
        })?;
    Ok(())
}

fn ensure_no_legacy_recovery(path: &Path) -> Result<()> {
    let backup = path.with_extension("toml.loadbot-backup");
    if backup.try_exists()? {
        bail!(
            "recovery required: inspect {} and {}; no files were changed",
            path.display(),
            backup.display()
        );
    }
    Ok(())
}

/// Read-only recovery detection: absence is not silently accepted beside a backup.
pub fn read_optional(path: &Path) -> Result<Option<String>> {
    ensure_no_legacy_recovery(path)?;
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn lease_release_does_not_wait_for_an_inherited_file_description() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("repository");
        let lease = Lease::acquire(&path).unwrap();
        // dup shares the same open file description, just as a concurrent fork
        // does until exec closes CLOEXEC descriptors in the child.
        let inherited = lease.file.try_clone().unwrap();
        drop(lease);
        let next = Lease::acquire(&path);
        assert!(next.is_ok(), "the completed operation still holds its lease");
        drop(inherited);
    }

    #[test]
    fn replacement_failure_keeps_old_file_and_removes_only_our_temporary() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        fs::write(&path, "version = 1\n").unwrap();
        let orphan = root.path().join(".loadbot-write-other-operation");
        fs::write(&orphan, "owned elsewhere").unwrap();
        let error = write_toml_with(
            &path,
            &crate::config::LocalConfig::default(),
            |temporary, _| {
                assert!(!fs::read(temporary.path()).unwrap().is_empty());
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected replacement failure",
                ))
            },
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("injected replacement failure"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "version = 1\n");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        assert!(orphan.exists());
    }
}
