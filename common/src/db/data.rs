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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UploadRow {
    /** The primary key of the upload */
    pub(crate) id: String,
    /** The directory the file is in */
    pub(crate) dir: String,
    /** Current status of the upload
     * Not an enum because different pipelines will have different values for this.
     * The only meaningful values for the frontend are Creating, Uploading, and Finished.
     */
    pub(crate) status: String,

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
