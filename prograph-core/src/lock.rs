//! FS exclusive lock used by `prograph index` to prevent concurrent runs.
//! Backed by `fslock` for cross-platform behaviour (flock on Unix, LockFileEx on Windows).

use std::path::{Path, PathBuf};

use fslock::LockFile;

use crate::errors::{PrographError, Result};

/// RAII guard around an exclusive lock on `<path>`. Dropping it releases the lock.
pub struct IndexLockGuard {
    _file: LockFile,
    path: PathBuf,
}

impl std::fmt::Debug for IndexLockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexLockGuard")
            .field("path", &self.path)
            .finish()
    }
}

impl IndexLockGuard {
    /// Acquire the lock, or return `PrographError::Lock` if another process holds it.
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PrographError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let mut file = LockFile::open(path).map_err(|e| PrographError::Io {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        })?;

        let acquired = file.try_lock().map_err(|e| PrographError::Io {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        })?;

        if !acquired {
            return Err(PrographError::Lock {
                path: path.display().to_string(),
            });
        }

        Ok(Self {
            _file: file,
            path: path.to_path_buf(),
        })
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn acquire_succeeds_when_lock_free() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".prograph/index.lock");
        let g = IndexLockGuard::acquire(&path).unwrap();
        assert!(path.exists());
        assert_eq!(g.path(), &path);
    }

    #[test]
    fn second_acquire_fails_while_first_held() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("lock");

        let _g1 = IndexLockGuard::acquire(&path).unwrap();
        let result = IndexLockGuard::acquire(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PrographError::Lock { .. }));
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("lock");
        {
            let _g = IndexLockGuard::acquire(&path).unwrap();
        }
        // After drop, lock should be acquirable again.
        let _g2 = IndexLockGuard::acquire(&path).unwrap();
    }
}
