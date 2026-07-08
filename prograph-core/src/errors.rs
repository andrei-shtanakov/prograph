//! Error types for prograph-core. All errors convert to Python exceptions at the FFI boundary.

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PrographError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("project discovery failed at {root}: {reason}")]
    Discovery { root: String, reason: String },

    #[error("parse error in {path}: {reason}")]
    Parse { path: String, reason: String },

    #[error("index lock at {path} is held by another process")]
    Lock { path: String },
}

impl From<PrographError> for PyErr {
    fn from(err: PrographError) -> PyErr {
        match err {
            PrographError::Io { .. } => PyIOError::new_err(err.to_string()),
            PrographError::Sqlite(_) => PyRuntimeError::new_err(err.to_string()),
            PrographError::Config(_) => PyValueError::new_err(err.to_string()),
            PrographError::Discovery { .. } => PyRuntimeError::new_err(err.to_string()),
            PrographError::Parse { .. } => PyValueError::new_err(err.to_string()),
            PrographError::Lock { .. } => PyRuntimeError::new_err(err.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, PrographError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_displays_message() {
        let err = PrographError::Config("missing root".into());
        assert_eq!(err.to_string(), "invalid configuration: missing root");
    }

    #[test]
    fn pyerr_conversion_picks_value_error_for_config() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let err: PyErr = PrographError::Config("x".into()).into();
            assert!(err.is_instance_of::<PyValueError>(py));
        });
    }

    #[test]
    fn parse_error_displays_path_and_reason() {
        let err = PrographError::Parse {
            path: "pyproject.toml".into(),
            reason: "missing [project] table".into(),
        };
        assert_eq!(
            err.to_string(),
            "parse error in pyproject.toml: missing [project] table"
        );
    }

    #[test]
    fn lock_error_maps_to_runtime_error() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let err: PyErr = PrographError::Lock {
                path: ".prograph/index.lock".into(),
            }
            .into();
            assert!(err.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
        });
    }
}
