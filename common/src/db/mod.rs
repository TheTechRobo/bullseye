use async_stream::stream;
use fix_hidden_lifetime_bug::fix_hidden_lifetime_bug;
use futures::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, time::SystemTime};
use unreql::{
    cmd::options::ChangesOptions,
    r, rjson,
    types::{Change, WriteStatus},
};
use unreql_deadpool::{IntoPoolWrapper, PoolWrapper};

mod data;
pub use data::*;

pub mod status {
    pub const UPLOADING: &str = "UPLOADING";
    pub const FINISHED: &str = "FINISHED";
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DbError {
    NotFound,
    WriteFailed,
    Other,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::NotFound => write!(f, "database row not found"),
            DbError::WriteFailed => write!(f, "database write failed"),
            DbError::Other => write!(f, "unknown database error"),
        }
    }
}

impl Error for DbError {}

impl UploadRow {
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub async fn new(
        conn: &DatabaseHandle,
        id: String,
        file: File,
        pipeline: String,
        project: String,
        metadata: Metadata,
    ) -> Result<Self, DbError> {
        let s = Self {
            id,
            file,
            pipeline,
            project,
            status: "UPLOADING".to_string(),
            writing: 0,
            last_activity: Self::now(),
            metadata,
        };
        let result: Result<WriteStatus, _> = r
            .db("atuploads")
            .table("uploads")
            .insert(s.clone())
            .exec(&conn.pool)
            .await;
        match result {
            Ok(a) => {
                if a.inserted != 1 {
                    Err(DbError::WriteFailed)
                } else {
                    Ok(s)
                }
            }
            Err(_) => Err(DbError::Other),
        }
    }

    pub async fn from_database(conn: &DatabaseHandle, uuid: String) -> Result<UploadRow, DbError> {
        let result: Result<Vec<UploadRow>, _> = r
            .db("atuploads")
            .table("uploads")
            .get_all(uuid)
            .exec_to_vec(&conn.pool)
            .await;
        if let Ok(mut v) = result {
            match v.len() {
                0 => Err(DbError::NotFound),
                1 => Ok(v.remove(0)),
                _ => unreachable!(),
            }
        } else {
            println!("warning: Unknown database error occured, see: {result:?}");
            Err(DbError::Other)
        }
    }

    pub fn id(&self) -> &String {
        &self.id
    }

    pub fn size(&self) -> u64 {
        self.file.size
    }

    pub fn status(&self) -> &String {
        &self.status
    }

    pub async fn enter(&mut self, conn: &DatabaseHandle) -> Result<(), DbError> {
        let s: unreql::Result<WriteStatus> = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .update(rjson!({
                "writing": r.row().g("writing").add(1)
            }))
            .exec(&conn.pool)
            .await;
        match s {
            unreql::Result::Ok(ws) => {
                if ws.errors > 0 {
                    Err(DbError::WriteFailed)
                } else if ws.skipped > 0 {
                    Err(DbError::NotFound)
                } else {
                    self.writing += 1;
                    Ok(())
                }
            }
            unreql::Result::Err(_) => Err(DbError::WriteFailed),
        }
    }

    // TODO: Do this in a destructor
    pub async fn exit(&mut self, conn: &DatabaseHandle) -> Result<(), DbError> {
        let s: unreql::Result<WriteStatus> = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .update(rjson!({
                "writing": r.row().g("writing").sub(1)
            }))
            .exec(&conn.pool)
            .await;
        match s {
            unreql::Result::Ok(ws) => {
                if ws.errors > 0 {
                    Err(DbError::WriteFailed)
                } else if ws.skipped > 0 {
                    Err(DbError::NotFound)
                } else {
                    self.writing -= 1;
                    Ok(())
                }
            }
            unreql::Result::Err(_) => Err(DbError::WriteFailed),
        }
    }

    /*pub async fn change_status(conn: &PoolWrapper, new_status: String) -> Result<(), DbError> {

    }*/

    #[fix_hidden_lifetime_bug] // what the fuck
    pub fn stream_status_changes(&mut self, conn: &DatabaseHandle) -> impl Stream<Item = String> {
        let opts = ChangesOptions::new()
            .include_initial(true)
            .include_states(false);

        let mut q = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .changes(opts)
            .run::<_, Change>(&conn.pool);

        stream! {
            while let Ok(Some(changed)) = q.try_next().await {
                if let Some(new_val) = changed.new_val {
                    self.status = match &new_val["status"] {
                        serde_json::Value::String(s) => s.clone(),
                        _ => unreachable!("{}", new_val),
                    };
                    yield self.status.clone();
                }
            }
        }
    }
}

pub struct DatabaseHandle {
    pub(crate) pool: PoolWrapper,
}

impl DatabaseHandle {
    pub fn new() -> Result<Self, ()> {
        let cfg = unreql::cmd::connect::Options::default();
        let manager = unreql_deadpool::SessionManager::new(cfg);
        let pool = deadpool::managed::Pool::builder(manager)
            .max_size(4)
            .build();
        if let Ok(pool) = pool {
            Ok(Self {
                pool: pool.wrapper(),
            })
        } else {
            Err(())
        }
    }
}
