use futures_util::StreamExt as _;
use nix::fcntl::posix_fallocate;
use std::{
    io,
    os::fd::{AsFd, AsRawFd},
    path::PathBuf,
};

use actix_web::web;
use tokio::{
    fs::{remove_file, File},
    io::{AsyncSeekExt, AsyncWriteExt},
    task::spawn_blocking,
};

const DATA_DIR: &str = "data";

fn get_path(id: &str) -> PathBuf {
    let path: PathBuf = [DATA_DIR, id].iter().collect();
    path
}

async fn get_file(id: &str) -> io::Result<File> {
    File::options()
        .read(true)
        .write(true)
        .open(get_path(id))
        .await
}

pub async fn new_file(id: &str, with_size: u64) -> io::Result<()> {
    let path = get_path(id);
    let file = File::create_new(&path).await?;
    let fd = file.as_fd().as_raw_fd();
    match spawn_blocking(move || posix_fallocate(fd, 0, with_size as i64)).await? {
        Ok(()) => io::Result::Ok(()),
        Err(e) => {
            remove_file(path).await?;
            io::Result::Err(io::Error::other(format!("{e}")))
        }
    }
}

pub async fn write_to_file(
    id: &str,
    size: u64,
    offset: u64,
    mut body: web::Payload,
) -> io::Result<()> {
    let mut file = get_file(id).await?;
    file.seek(io::SeekFrom::Start(offset)).await?;
    while let Some(chunk) = body.next().await {
        if let Ok(chunk) = chunk {
            if offset + chunk.len() as u64 > size {
                return io::Result::Err(io::Error::other("Exceeded file bounds"));
            }
            file.write_all(&chunk).await?;
            file.flush().await?;
            file.sync_all().await?;
        } else {
            dbg!(chunk.unwrap_err());
            return io::Result::Err(io::Error::other("Chunk read failed"));
        }
    }
    io::Result::Ok(())
}
