use std::io;

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
    conn: web::Data<DatabaseHandle>,
    req: HttpRequest,
    pdetails: web::Json<UploadInitialisationPayload>,
) -> impl Responder {
    let id = uuidv7::create();
    let details = pdetails.clone();
    if let io::Result::Err(e) = files::new_file(&id, details.file.size).await {
        dbg!(e);
        return NewUploadResp::Err("I/O error".to_string()).to_response(HttpResponse::Created());
    }
    let res = UploadRow::new(
        &conn,
        id,
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
        Err(e) => NewUploadResp::from(e),
    }
    .to_response(HttpResponse::Created())
}

type GetUploadResp = ErrorablePayload<SingleUploadResponse>;

#[get("/upload/{uuid}")]
async fn get_upload(conn: web::Data<DatabaseHandle>, path: web::Path<String>) -> impl Responder {
    let uuid = path.into_inner();
    let upload = UploadRow::from_database(conn.get_ref(), uuid).await;
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
    conn: web::Data<DatabaseHandle>,
    path: web::Path<String>,
    qs: web::Query<UploadChunkQueryString>,
) -> impl Responder {
    let uuid = path.into_inner();
    let offset = qs.into_inner().offset;
    let row = UploadRow::from_database(&conn, uuid).await;
    let mut res = UploadChunkResp::Ok(());
    if let Ok(mut row) = row {
        if row.status() != status::UPLOADING {
            res = UploadChunkResp::Err("Item is not in the UPLOADING status".to_string());
        } else if offset > row.size() {
            res = UploadChunkResp::Err("Offset too large".to_string());
        } else if let Err(e) = row.enter(&conn).await {
            res = UploadChunkResp::from(e);
        } else {
            let r = files::write_to_file(row.id(), row.size(), offset, body).await;
            if let Err(e) = r {
                dbg!(e);
                res = UploadChunkResp::Err("I/O error".to_string());
            }
            let _ = row.exit(&conn).await; // not much we can do if this fails
        }
    }
    res.to_response(HttpResponse::Created())
}

#[get("/upload/{uuid}/events")]
async fn upload_subscribe(conn: web::Data<DatabaseHandle>, path: web::Path<String>) -> impl Responder {
    let uuid = path.into_inner();
    let conn = conn.into_inner();
    let row = UploadRow::from_database(&conn, uuid).await;
    if let Ok(mut row) = row {
        HttpResponse::Ok()
            .streaming(stream! {
                let iter = row.stream_status_changes(&conn);
                pin_mut!(iter);
                while let Some(change) = iter.next().await {
                    let event = UploadEvent::StatusChange(change);
                    if let Ok(mut serialized) = serde_json::to_vec(&event) {
                        serialized.push(0xA);
                        yield Ok(Bytes::from(serialized));
                    } else {
                        yield Err("JSON serialize error\n");
                    }
                }
            })
    } else {
        HttpResponse::InternalServerError().body("hi")
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    HttpServer::new(move || {
        let pool = DatabaseHandle::new().unwrap();
        App::new()
            .app_data(web::Data::new(pool))
            .service(slash)
            .service(get_upload)
            .service(new_upload)
            .service(put_upload_chunk)
            .service(upload_subscribe)
    })
    .bind(("127.0.0.1", 7000))?
    .run()
    .await
}
