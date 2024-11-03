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

async fn get_file(path: &str) -> io::Result<File> {
    File::options()
        .read(true)
        .write(true)
        .open(path)
        .await
}

pub async fn new_file(mut path: PathBuf, id: &str, with_size: u64) -> io::Result<()> {
    let with_size: i64 = match with_size.try_into() {
        Ok(s) => s,
        Err(_) => return Err(io::Error::other("File too large")),
    };
    path.push(id);
    let file = File::create_new(&path).await?;
    let fd = file.as_fd().as_raw_fd();
    match spawn_blocking(move || posix_fallocate(fd, 0, with_size)).await? {
        Ok(()) => io::Result::Ok(()),
        Err(e) => {
            remove_file(path).await?;
            io::Result::Err(io::Error::other(format!("{e}")))
        }
    }
}

pub async fn write_to_file(
    mut dir: PathBuf,
    id: &str,
    size: u64,
    offset: u64,
    mut body: web::Payload,
) -> io::Result<()> {
    dir.push(id);
    let mut file = get_file(dir.to_str().unwrap()).await?;
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
