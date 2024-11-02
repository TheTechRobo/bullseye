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
    /** Current status of the upload
     * Not an enum because different pipelines will have different values for this.
     * The only meaningful values for the frontend are Creating, Uploading, and Finished.
     */
    pub(crate) status: String,

    pub(crate) file: File,
    /** The number of writes currently being done to the file.
     * NOT A LOCK; the behaviour of simultaneous overlapping writes is undefined.
     */
    pub(crate) writing: u16,
    /** The last time the server received data from the client; can be used to expire uploads */
    pub(crate) last_activity: u64,

    pub(crate) pipeline: String,
    pub(crate) project: String,

    pub(crate) metadata: Metadata,
}
