use std::{io, path::{Path, PathBuf}};

use actix_web::{get, post, put, web::{self, Bytes}, App, HttpRequest, HttpResponse, HttpServer, Responder};

use async_stream::stream;
use serde::Deserialize;
use futures::{pin_mut, StreamExt};

use common::db::*;
mod payloads;
use payloads::*;
mod files;

#[get("/")]
async fn slash() -> impl Responder {
    HttpResponse::Ok().body("the archivists are coming for you")
}

type NewUploadResp = ErrorablePayload<NewUploadResponse>;

#[post("/upload")]
async fn new_upload(
    conn: web::Data<SharedCtx>,
    req: HttpRequest,
    pdetails: web::Json<UploadInitialisationPayload>,
) -> impl Responder {
    let id = uuidv7::create();
    let mut details = pdetails.clone();
    details.file.name = Path::new(&details.file.name).file_name().unwrap().to_str().unwrap().to_string();
    if let io::Result::Err(e) = files::new_file(conn.cwd.clone(), &id, details.file.size).await {
        dbg!(e);
        return NewUploadResp::Err("I/O error".to_string()).to_response(HttpResponse::Created());
    }
    let res = UploadRow::new(
        &conn.pool,
        conn.cwd.to_str().unwrap().to_string(),
        id.clone(),
        details.file,
        details.pipeline,
        details.project,
        details.metadata,
    )
    .await;

    match res {
        Ok(entry) => {
            NewUploadResp::Ok(UploadInformation {
                id: entry.id().clone(),
                // I would like to fix this abomination
                base_url: req
                    .url_for("get_upload", [entry.id()])
                    .unwrap()
                    .as_str()
                    .to_string(),
            })
        }
        Err(e) => {
            let _ = files::delete_file(conn.cwd.clone(), &id).await;
            NewUploadResp::from(e)
        }
    }
    .to_response(HttpResponse::Created())
}

type GetUploadResp = ErrorablePayload<SingleUploadResponse>;

#[get("/upload/{uuid}")]
async fn get_upload(conn: web::Data<SharedCtx>, path: web::Path<String>) -> impl Responder {
    let uuid = path.into_inner();
    let upload = UploadRow::from_database(&conn.pool, uuid).await;
    match upload {
        Ok(payload) => GetUploadResp::Ok(payload),
        Err(e) => GetUploadResp::from(e),
    }
    .to_response(HttpResponse::Ok())
}

type UploadChunkResp = ErrorablePayload<UploadChunkResponse>;

#[derive(Deserialize)]
struct UploadChunkQueryString {
    offset: u64,
}

#[put("/upload/{uuid}/data")]
async fn put_upload_chunk(
    body: web::Payload,
    conn: web::Data<SharedCtx>,
    path: web::Path<String>,
    qs: web::Query<UploadChunkQueryString>,
) -> impl Responder {
    let uuid = path.into_inner();
    let offset = qs.into_inner().offset;
    let row = UploadRow::from_database(&conn.pool, uuid).await;
    let mut res = UploadChunkResp::Ok(());
    if let Ok(mut row) = row {
        if row.status() != status::UPLOADING {
            res = UploadChunkResp::Err("Item is not in the UPLOADING status".to_string());
        } else if offset > row.size() {
            res = UploadChunkResp::Err("Offset too large".to_string());
        } else if let Err(e) = row.enter(&conn.pool).await {
            res = UploadChunkResp::from(e);
        } else {
            let r = files::write_to_file(conn.cwd.clone(), row.id(), row.size(), offset, body).await;
            if let Err(e) = r {
                dbg!(e);
                res = UploadChunkResp::Err("I/O error".to_string());
            }
        }
    }
    res.to_response(HttpResponse::Created())
}

#[get("/upload/{uuid}/events")]
async fn upload_subscribe(conn: web::Data<SharedCtx>, path: web::Path<String>) -> impl Responder {
    let uuid = path.into_inner();
    let conn = conn.into_inner();
    let row = UploadRow::from_database(&conn.pool, uuid).await;
    match row {
        Ok(mut row) => {
            HttpResponse::Ok()
                .streaming(stream! {
                    let iter = row.stream_status_changes(&conn.pool);
                    pin_mut!(iter);
                    while let Some(change) = iter.next().await {
                        let event = UploadEvent::StatusChange(change);
                        if let Ok(mut serialized) = serde_json::to_vec(&event) {
                            serialized.push(0xA); // add newline to make this JSONL
                            yield Ok(Bytes::from(serialized));
                        } else {
                            yield Err("JSON serialize error\n");
                        }
                    }
                })
        },
        Err(e) => {
            let e: ErrorablePayload<()> = e.into();
            e.to_response(HttpResponse::InternalServerError())
        }
    }
}

#[post("/upload/{uuid}/finish")]
async fn upload_finish(conn: web::Data<SharedCtx>, path: web::Path<String>) -> impl Responder {
    let uuid = path.into_inner();
    let conn = conn.into_inner();
    let resp: ErrorablePayload<()> = match UploadRow::from_database(&conn.pool, uuid).await {
        Ok(mut row) => {
            let lock = files::exclusive_lock(conn.cwd.clone(), row.id()).await;
            if lock.is_err() {
                ErrorablePayload::Err("Failed to lock file".to_string())
            } else {
                match row.finish(&conn.pool).await {
                    Ok(()) => ErrorablePayload::Ok(()),
                    Err(e) => e.into(),
                }
            }
        },
        Err(e) => e.into(),
    };
    resp.to_response(HttpResponse::Accepted())
}

struct SharedCtx {
    pool: DatabaseHandle,
    cwd: PathBuf,
}

const DATA_DIR: &str = "data";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    let mut cwd = std::env::current_dir()?;
    cwd.push(DATA_DIR);
    env_logger::init();
    HttpServer::new(move || {
        let pool = SharedCtx {
            pool: DatabaseHandle::new().unwrap(),
            cwd: cwd.clone(),
        };
        App::new()
            .app_data(web::Data::new(pool))
            .service(slash)
            .service(get_upload)
            .service(new_upload)
            .service(put_upload_chunk)
            .service(upload_subscribe)
            .service(upload_finish)
    })
    .bind(("127.0.0.1", 7000))?
    .run()
    .await
}
