use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Metadata {
    pub uploader: String,
    pub items: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct File {
    pub hash: String,
    pub name: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum UploadError {
    /// The checksum did not match. The client should try uploading again.
    #[serde(rename = "FAILED_CHECKSUM")]
    Checksum,
    /// The file was uploaded successfully, but its contents are invalid or otherwise unacceptable.
    /// The file the client was told to upload is invalid, and it should not try again.
    #[serde(rename = "FAILED_VERIFY")]
    Verify,
    /// An unknown error occured when uploading.
    #[serde(rename = "FAILED_OTHER")]
    Other,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    /// The file is currently being uploaded. The file has been fully allocated but its
    /// contents might not be there yet.
    Uploading,
    /// The file is currently being verified.
    Verifying,
    /// Only used on some pipelines. The file is being processed in some way.
    Deriving,
    /// The file is currently being packed or otherwise queued for uploading.
    Packing,
    /// The file has been safely readied for uploading. The client's job is done.
    Finished,
    /// The upload was abandoned by the client and the file has been removed.
    Abandoned,
    /// Something went wrong with the upload.
    #[serde(untagged)]
    Error(UploadError),
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UploadRow {
    /** The primary key of the upload */
    pub(crate) id: String,
    /** The directory the file is in */
    pub(crate) dir: String,

    pub(crate) status: Status,

    pub(crate) file: File,
    /** The last time the server received data from the client; can be used to expire uploads */
    pub(crate) last_activity: u64,

    pub(crate) pipeline: String,
    pub(crate) project: String,

    /** If true, the upload is actively being processed.
     * A processor can die without setting this to false,
     * so a separate task should be run which occasionally
     * checks for unlocked files and sets their `processing`
     * flag to false.
     **/
    pub(crate) processing: bool,

    pub(crate) metadata: Metadata,
}

#[cfg(test)]
mod tests {
    use super::{Status, UploadError};

    #[test]
    fn status_serialization() {
        let tests = [
            (Status::Verifying, "VERIFYING"),
            (Status::Uploading, "UPLOADING"),
            (Status::Error(UploadError::Verify), "FAILED_VERIFY"),
        ];
        for (src, expected) in tests {
            assert_eq!(
                serde_json::from_str::<Status>(&serde_json::to_string(&src).unwrap()).unwrap(),
                src
            );
            assert_eq!(format!("{}", &src), expected);
            assert_eq!(
                serde_json::to_value(src.clone()).unwrap().as_str().unwrap(),
                expected
            );
        }
    }
}
