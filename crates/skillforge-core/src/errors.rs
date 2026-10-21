//! Typed error types shared across the core library.
//!
//! Core/library code uses explicit, typed errors (thiserror). `anyhow` is only
//! used at the application boundary in `main.rs`.

use std::path::PathBuf;

use thiserror::Error;

/// Top-level error for operations that cross several layers.
#[derive(Debug, Error)]
pub enum Error {
    #[error("文件操作失败 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("路径不安全: {0}")]
    UnsafePath(#[from] crate::domain::path_safety::PathSafetyError),

    #[error("SKILL.md 处理失败: {0}")]
    SkillMd(#[from] crate::formats::skill_md::SkillMdError),

    #[error("工作区错误: {0}")]
    Workspace(String),

    #[error("安装错误: {0}")]
    Install(String),

    #[error("导入/导出错误: {0}")]
    ImportExport(String),

    #[error("设置错误: {0}")]
    Settings(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Build an I/O error that records the offending path.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
