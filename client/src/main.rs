use anyhow::{anyhow, bail, Result};
use async_stream::stream;
use bytes::{Bytes, BytesMut};
use clap::Parser;
use common::{
    db::{status, File, Metadata},
    hash_file,
    payloads::*,
};
use futures_util::{pin_mut, Stream, StreamExt};
use kdam::{
    term::{self, Colorizer},
    tqdm, BarExt, Column, RichProgress, Spinner,
};
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    error::Error,
    fmt, fs,
    io::{self, stderr, IsTerminal},
    path::Path,
    time::Duration,
};
use tokio::{fs::metadata, io::{AsyncBufReadExt, AsyncReadExt}, select, spawn, sync::watch, time::sleep};
use tokio_util::{io::StreamReader, sync::CancellationToken};
use url::Url;

#[allow(dead_code)] // the inner values are only there for Debug
#[derive(Clone, Debug)]
enum UploadError {
    ReqwestError(String),
    BadStatusCode(u16),
    JsonDecodeError(String),
    BadResponse(String),
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReqwestError(s) => write!(f, "reqwest error: {s}"),
            Self::BadStatusCode(s) => write!(f, "bad status code {s}"),
            Self::JsonDecodeError(s) => write!(f, "json decode error: {s}"),
            Self::BadResponse(s) => write!(f, "bad response: {s}"),
        }
    }
}

impl Error for UploadError {}

impl From<reqwest::Error> for UploadError {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(format!("{}", value))
    }
}

impl From<serde_json::Error> for UploadError {
    fn from(value: serde_json::Error) -> Self {
        Self::JsonDecodeError(value.to_string())
    }
}

#[derive(Debug)]
struct Upload {
    base_url: String,
    id: String,
}

/// Runs a function returning Result in a loop with exponentional backoff.
/// Returns a successful response. Otherwise, bail!s.
macro_rules! try_something {
    ($a:expr) => {
        const MAX_TRIES: u8 = 7;
        for i in 0..MAX_TRIES {
            let e = $a;
            if let Ok(resp) = e {
                return Ok(resp);
            }
            let to_sleep = 1 << i;
            eprintln!("try {i} failed, sleeping {to_sleep}s: {:?}", e.unwrap_err());
            sleep(Duration::from_secs(to_sleep)).await;
        }
        eprintln!("max tries reached; returning error");
        bail!("max tries reached");
    };
}

impl Upload {
    /// Processes a response from the server.
    /// This involves checking the status code, decoding the body, etc.
    async fn process_response<Resp: DeserializeOwned + fmt::Debug>(
        input: reqwest::Result<reqwest::Response>,
        expected_status: u16,
    ) -> Result<Resp> {
        let res = input?;
        let status_code = res.status().as_u16();
        if status_code != expected_status {
            dbg!(res.text().await?);
            bail!(UploadError::BadStatusCode(status_code));
        }
        let text = res.text().await?;
        let response: ErrorablePayload<Resp> = serde_json::from_str(&text)?;
        match response {
            ErrorablePayload::Ok(response_payload) => Ok(response_payload),
            _ => Err(anyhow!(UploadError::BadResponse(format!("{response:?}")))),
        }
    }

    async fn post<Req: Serialize, Resp: DeserializeOwned + fmt::Debug>(
        client: &Client,
        url: &String,
        payload: &Req,
        expected_status: u16,
    ) -> Result<Resp> {
        let res = client.post(url).json(&payload).send().await;
        Self::process_response(res, expected_status).await
    }

    async fn try_post<Req: Serialize, Resp: DeserializeOwned + fmt::Debug>(
        client: &Client,
        url: String,
        payload: Req,
        expected_status: u16,
    ) -> Result<Resp> {
        try_something!(Self::post(client, &url, &payload, expected_status).await);
    }

    async fn put<Req: Into<reqwest::Body>, Resp: DeserializeOwned + fmt::Debug>(
        client: &Client,
        url: &String,
        payload: Req,
        expected_status: u16,
    ) -> Result<Resp> {
        let res = client.put(url).body(payload).send().await;
        Self::process_response(res, expected_status).await
    }

    async fn try_put<Resp: DeserializeOwned + fmt::Debug>(
        client: &Client,
        url: String,
        payload: Bytes,
        expected_status: u16,
    ) -> Result<Resp> {
        try_something!(Self::put(client, &url, payload.clone(), expected_status).await);
    }

    pub async fn new(
        client: &Client,
        upload_endpoint: String,
        file: File,
        project: String,
        pipeline: String,
        metadata: Metadata,
    ) -> Result<Self> {
        let payload = UploadInitialisationPayload {
            file,
            project,
            pipeline,
            metadata,
        };
        let response: UploadInformation =
            Self::try_post(client, upload_endpoint, payload, 201).await?;
        Ok(Self {
            base_url: response.base_url,
            id: response.id,
        })
    }

    pub async fn upload_part(&self, client: &Client, offset: u64, part_data: Bytes) -> Result<()> {
        let nl = self.base_url.clone() + "/data";
        let url = Url::parse_with_params(&nl, &[("offset", offset.to_string())]).unwrap();
        let _: () = Self::try_put(client, url.to_string(), part_data, 201).await?;
        Ok(())
    }

    pub async fn finish(&self, client: &Client) -> Result<()> {
        let nl = self.base_url.clone() + "/finish";
        let _: () = Self::try_post(client, nl.to_string(), "", 202).await?;
        Ok(())
    }

    pub async fn subscribe(&self, client: &Client) -> Result<impl Stream<Item = io::Result<UploadEvent>>> {
        let nl = self.base_url.clone() + "/events";
        let r = client.get(nl)
            .send()
            .await?;
        let status = r.status();
        if status != 200 {
            bail!("bad status code {status}");
        }
        let stream = r.bytes_stream().map(|result| result.map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::Other, err)
        }));
        let mut reader = StreamReader::new(stream);
        let mut s = String::new();
        Ok(stream! {
            loop {
                s.clear();
                if let Ok(len) = reader.read_line(&mut s).await {
                    if len == 0 {
                        // EOF
                        break;
                    }
                    let v: UploadEvent = serde_json::from_str(&s)?;
                    yield Ok(v);
                } else {
                    yield Err(io::Error::other("couldn't read line"));
                    break;
                }
            }
        })
    }
}

async fn get_file_metadata(fp: &Path) -> Result<File> {
    let metadata = metadata(fp).await?;
    let f = fs::File::open(fp)?;
    let hash = hash_file(f).await?;
    Ok(File {
        name: fp.file_name().unwrap().to_str().unwrap().to_string(), // Why
        hash,
        size: metadata.len(),
    })
}

const CHUNK_SIZE: usize = 16 * 1024 * 1024;

async fn read_chunk(file: &mut tokio::fs::File) -> Result<Bytes> {
    let mut buf = BytesMut::with_capacity(CHUNK_SIZE);
    file.read_buf(&mut buf).await?;
    Ok(buf.freeze())
}

async fn refresh_bar(mut bar: Option<RichProgress>, token: CancellationToken, status: watch::Receiver<String>) -> Option<RichProgress> {
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut prev = String::new();
    loop {
        select! {
            _ = timer.tick() => {
                let s = status.borrow();
                if let Some(&mut ref mut bar) = bar.as_mut() { // Go home, Rust, you're drunk.
                    bar.columns.truncate(3);
                    bar.columns.push(Column::Text(s.clone().colorize("green")));
                    let _ = bar.refresh();
                } else if s.to_string() != prev {
                    eprintln!("Item entered status {}.", *s);
                    prev = s.clone();
                }
            }
            _ = token.cancelled() => {
                return bar;
            }
        }
    }
}

// Outside: Ok if upload OK, Err if any error.
// Inside: Ok if upload OK, Err if hash verification failed.
async fn iter_file(
    client: &Client,
    upload: Upload,
    file: &mut tokio::fs::File,
    size: u64,
    tty: bool,
) -> Result<Result<(), ()>> {
    let mut bytes_remaining = size;
    let mut offset: u64 = 0;
    let mut bar: Option<RichProgress> = None;
    eprintln!("Uploading {} bytes.", size);
    if tty {
        bar = Some(RichProgress::new(
            tqdm!(
                total = size.try_into()?,
                unit_scale = true,
                unit_divisor = 1024,
                unit = "iB"
            ),
            vec![
                Column::Spinner(Spinner::new(
                    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
                    80.0,
                    1.0,
                )),
                Column::Text("[bold blue]?".to_owned()),
                Column::Animation,
                Column::Percentage(1),
                Column::Text("•".to_owned()),
                Column::CountTotal,
                Column::Text("•".to_owned()),
                Column::Rate,
                Column::Text("•".to_owned()),
                Column::RemainingTime,
            ],
        ));
    }
    while bytes_remaining > 0 {
        let chunk = read_chunk(file).await?;
        let l = chunk.len() as u64;
        upload.upload_part(client, offset, chunk).await?;
        offset += l;
        bytes_remaining -= l;
        if let Some(&mut ref mut bar) = bar.as_mut() {
            let _ = bar.update(l as usize);
        }
    }
    if let Some(&mut ref mut bar) = bar.as_mut() {
        let _ = bar.update_to(0); // to get the little animation
        bar.write("Finalizing upload...".colorize("bold blue"))?;
    } else {
        eprintln!("Finalizing upload...");
    }
    upload.finish(client).await?;
    let token = CancellationToken::new();
    let (sender, receiver) = watch::channel("Making request...".to_string());
    let f = spawn(refresh_bar(bar, token.clone(), receiver));

    let mut current_status = String::new();
    let mut tries = 0;
    while current_status != status::FINISHED {
        let stream = match upload.subscribe(client).await {
            Ok(s) => s,
            Err(e) => {
                dbg!(&e);
                sleep(Duration::from_secs(1 << tries)).await;
                tries += 1;
                if tries > 12 {
                    Err(e)?;
                }
                continue;
            }
        };
        pin_mut!(stream);
        while let Some(Ok(i)) = stream.next().await {
            match i {
                UploadEvent::StatusChange(s) => {
                    current_status = s.clone();
                    if s == status::FINISHED {
                        break;
                    }
                    if s == status::FAILED_CHECKSUM ||
                        s == status::FAILED_OTHER {
                        bail!("bad status: {}", s);
                    }
                    if s == status::FAILED_VERIFY {
                        return Ok(Err(()));
                    }
                    sender.send(s)?;
                },
            }
        }
    }

    token.cancel();
    if let Some(mut bar) = f.await? {
        bar.clear()?;
    }

    Ok(Ok(()))
}

async fn upload_file(client: &Client, args: Args, tty: bool) -> Result<Result<(), ()>> {
    let fp = Path::new(&args.file);
    let file = get_file_metadata(fp).await?;
    let upload = Upload::new(
        client,
        args.base_url,
        file.clone(),
        args.project,
        args.pipeline,
        Metadata {
            uploader: args.uploader,
            items: args.items,
        },
    )
    .await?;
    eprintln!("Upload ID: {}", &upload.id);
    let mut fh = tokio::fs::File::open(fp).await?;
    fh.set_max_buf_size(CHUNK_SIZE);
    iter_file(client, upload, &mut fh, file.size, tty).await
}

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    pub file: String,
    pub items: Vec<String>,

    #[arg(long)]
    pub project: String,

    #[arg(long)]
    pub pipeline: String,

    #[arg(long)]
    pub uploader: String,

    #[arg(short, long)]
    pub base_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let is_tty = stderr().is_terminal();
    term::init(is_tty);
    let args = Args::parse();
    if args.items.is_empty() {
        bail!("Must have one or more items");
    }

    let client = Client::builder()
        .user_agent("UploadPacker/0.1 (proof-of-concept)")
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .build()
        .unwrap();

    for i in 0..5 {
        match upload_file(&client, args.clone(), is_tty).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(())) => eprintln!("hash verification failed, retrying"),
            Err(e) => eprintln!("other failure ({e:?}), retrying"),
        };
        sleep(Duration::from_secs(1 << i)).await;
    }
    bail!("upload failure")
}
