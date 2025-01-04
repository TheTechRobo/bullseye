use serde::{Deserialize, Serialize};

use crate::db::UploadRow;

#[derive(Serialize, Deserialize)]
pub enum MegawarcLocation {
    Warc,
}

#[derive(Serialize, Deserialize)]
pub struct MegawarcTarget {
    pub container: MegawarcLocation,
    pub offset: u64,
    pub size: u64,
}

#[derive(Serialize, Deserialize)]
pub struct MegawarcMetadata {
    pub target: MegawarcTarget,
    upload_details: Option<UploadRow>,
}
