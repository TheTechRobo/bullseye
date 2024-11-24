use crate::data::{File, Metadata, Status, UploadRow};
#[cfg(feature = "db")]
use crate::db::DbError;
use serde::{Deserialize, Serialize};

// Response payloads

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "status", content = "payload")]
#[serde(rename_all = "snake_case")]
pub enum ErrorablePayload<T> {
    Ok(T),
    NotFound,
    Err(String),
}

#[cfg(feature = "db")]
impl<T> From<DbError> for ErrorablePayload<T> {
    fn from(value: DbError) -> Self {
        match value {
            DbError::NotFound => Self::NotFound,
            DbError::WriteFailed => Self::Err("Write error".to_string()),
            DbError::WrongStatus => Self::Err("Wrong status".to_string()),
            DbError::Other => Self::Err("Database error".to_string()),
        }
    }
}

pub type SingleUploadResponse = UploadRow;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UploadInformation {
    pub id: String,
    pub base_url: String,
}

pub type NewUploadResponse = UploadInformation;

// Request payloads

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UploadInitialisationPayload {
    pub file: File,
    pub project: String,
    pub pipeline: String,
    pub metadata: Metadata,
}

pub type UploadChunkResponse = ();

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "payload")]
#[serde(rename_all = "snake_case")]
pub enum UploadEvent {
    StatusChange(Status),
}
