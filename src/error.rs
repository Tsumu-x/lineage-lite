use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, LineageError>;

#[derive(Debug, thiserror::Error)]
pub enum LineageError {
    #[error("IO 錯誤: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQL 解析錯誤於 {file}: {message}")]
    SqlParse { file: PathBuf, message: String },

    #[error("儲存錯誤: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("找不到節點: {0}")]
    NodeNotFound(String),

    #[error("重複的節點: {0}")]
    DuplicateNode(String),
}
