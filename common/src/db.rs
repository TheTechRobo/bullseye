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

pub use crate::data::*;

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
            status: Status::Uploading,
            last_activity: Self::now(),
            processing: false,
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

    pub async fn check_out(conn: &DatabaseHandle, status: Status) -> Result<Option<Self>, DbError> {
        let s: unreql::Result<WriteStatus<Self>> = r
            .db("atuploads")
            .table("uploads")
            .get_all(r.with_opt(status, r.index("status")))
            .get_all(r.with_opt(false, r.index("processing")))
            .limit(1)
            .update(r.with_opt(
                r.branch(
                    r.row().g("processing").eq(false),
                    rjson!({
                        "processing": true,
                        "last_activity": Self::now()
                    }),
                    rjson!({}),
                ),
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
                    let mut changes = ws.changes.unwrap();
                    assert_eq!(changes.len(), 1);
                    let v = changes.remove(0).new_val;
                    Ok(v)
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

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub async fn finish(&mut self, conn: &DatabaseHandle) -> Result<(), DbError> {
        if self.status != Status::Uploading {
            return Err(DbError::WrongStatus);
        }
        let s: unreql::Result<WriteStatus> = r
            .db("atuploads")
            .table("uploads")
            .get(self.id.clone())
            .update(rjson!({
                "status": Status::Verifying
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
                    self.status = Status::Verifying;
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
                    self.last_activity = now;
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
        new_status: Status,
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
    pub fn stream_status_changes(&mut self, conn: &DatabaseHandle) -> impl Stream<Item = Status> {
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
                    let res: Result<Self, _> = serde_json::from_value(new_val);
                    if let Ok(status) = res {
                        self.status = status.status;
                        yield self.status.clone();
                    } /* else {
                        dbg!(&res);
                    } */
                }
            }
        }
    }
}

pub struct DatabaseHandle {
    pub(crate) pool: PoolWrapper,
}

impl DatabaseHandle {
    pub fn new() -> Result<Self, String> {
        let cfg = unreql::cmd::connect::Options::default();
        let manager = unreql_deadpool::SessionManager::new(cfg);
        let pool = deadpool::managed::Pool::builder(manager)
            .max_size(4)
            .build();
        match pool {
            Ok(pool) => Ok(Self {
                pool: pool.wrapper(),
            }),
            Err(e) => Err(e.to_string()),
        }
    }
}
