use async_stream::stream;
use fix_hidden_lifetime_bug::fix_hidden_lifetime_bug;
use futures::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, time::SystemTime};
use unreql::{
    cmd::options::{ChangesOptions, UpdateOptions},
    r, rjson,
    types::{Change, WriteStatus},
};
use unreql_deadpool::{IntoPoolWrapper, PoolWrapper};

mod data;
pub use data::*;

pub mod status {
    pub const UPLOADING: &str = "UPLOADING";
    pub const PENDING: &str = "PENDING";
    pub const FINISHED: &str = "FINISHED";
    pub const FAILED_CHECKSUM: &str = "FAILED_CHECKSUM";
    pub const FAILED_VERIFY: &str = "FAILED_VERIFY";
    pub const FAILED_OTHER: &str = "FAILED_OTHER";
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DbError {
    NotFound,
    WriteFailed,
    WrongStatus,
    Other,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::NotFound => write!(f, "database row not found"),
            DbError::WriteFailed => write!(f, "database write failed"),
            DbError::WrongStatus => write!(f, "wrong status"),
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
        dir: String,
        id: String,
        file: File,
        pipeline: String,
        project: String,
        metadata: Metadata,
    ) -> Result<Self, DbError> {
        let s = Self {
            id,
            dir,
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

    pub fn dir(&self) -> &String {
        &self.dir
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

    pub async fn check_out(
        conn: &DatabaseHandle,
        status: String,
        new_status: String,
    ) -> Result<Option<Self>, DbError> {
        let s: unreql::Result<WriteStatus<Self>> = r
            .db("atuploads")
            .table("uploads")
            .get_all(r.with_opt(status, r.index("status")))
            .limit(1)
            .update(r.with_opt(
                rjson!({
                    "status": new_status,
                    "last_activity": Self::now()
                }),
                UpdateOptions {
                    return_changes: Some(true.into()),
                    ..Default::default()
                },
            ))
            .exec(&conn.pool)
            .await;

        match s {
            unreql::Result::Ok(ws) => {
                if ws.errors > 0 {
                    Err(DbError::WriteFailed)
                } else if ws.replaced > 0 {
                    let changes = ws.changes.unwrap();
                    assert_eq!(changes.len(), 1);
                    Ok(changes[0].new_val.clone())
                } else {
                    Ok(None)
                }
            }
            unreql::Result::Err(_) => Err(DbError::WriteFailed),
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

    pub async fn finish(&mut self, conn: &DatabaseHandle) -> Result<(), DbError> {
        if self.status != status::UPLOADING {
            return Err(DbError::WrongStatus);
        }
        let s: unreql::Result<WriteStatus> = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .update(rjson!({
                "status": status::PENDING
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
                    self.status = status::PENDING.to_string();
                    Ok(())
                }
            }
            unreql::Result::Err(_) => Err(DbError::WriteFailed),
        }
    }

    pub async fn enter(&mut self, conn: &DatabaseHandle) -> Result<(), DbError> {
        let now = Self::now();
        let s: unreql::Result<WriteStatus> = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .update(rjson!({
                "writing": r.row().g("writing").add(1),
                "last_activity": now
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
                    self.last_activity = now;
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

    pub fn file(&self) -> &File {
        &self.file
    }

    pub async fn change_status(
        &mut self,
        conn: &DatabaseHandle,
        new_status: String,
    ) -> Result<(), DbError> {
        let s: unreql::Result<WriteStatus> = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .update(rjson!({
                "status": new_status.clone()
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
                    self.status = new_status;
                    Ok(())
                }
            }
            unreql::Result::Err(_) => Err(DbError::WriteFailed),
        }
    }

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
