use crate::document::ItemId;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("item {0:?} does not exist")]
    UnknownItem(ItemId),
    #[error("invalid geometry: {0}")]
    InvalidGeometry(&'static str),
    #[error("Via blocked: {0}")]
    ViaBlocked(String),
    #[error("unsupported file: {0}")]
    UnsupportedFile(String),
    #[error("malformed file: {0}")]
    Parse(#[from] serde_json::Error),
}
