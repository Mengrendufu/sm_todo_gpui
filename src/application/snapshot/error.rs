use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("标题不能为空")]
    EmptyTitle,
    #[error("无效的列状态")]
    InvalidStatus,
    #[error("无效的优先级")]
    InvalidPriority,
    #[error("卡片不存在")]
    CardNotFound,
    #[error("目标位置无效")]
    InvalidTarget,
    #[error("{0}")]
    Db(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
