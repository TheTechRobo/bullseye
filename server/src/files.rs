use futures_util::StreamExt as _;
#[allow(deprecated)] // See the acquire_lock function for rationale.
use nix::{errno::Errno, fcntl::{flock, posix_fallocate}};
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

async fn acquire_lock(file: &mut File, exclusive: bool) -> io::Result<()> {
    let fd = file.as_raw_fd();
    let arg = match exclusive {
        true => nix::fcntl::FlockArg::LockExclusiveNonblock,
        false => nix::fcntl::FlockArg::LockSharedNonblock,
    };
    // We can't use the Flock struct because it requires an owned std::File or OwnedFd. I'm not
    // sure why it's so insistent on consuming the file. How does it expect you to *use* the file?
    // We could theoretically duplicate the file handle, but why would we when there's a
    // perfectly good deprecated function here?
    #[allow(deprecated)]
    let res = spawn_blocking(move || { flock(fd, arg) }).await?;
    match res {
        Ok(()) => Ok(()),
        Err(e) => {
            if e == Errno::EWOULDBLOCK { // The lock isn't available yet. Let the client retry.
                Err(io::Error::other("file is locked"))
            } else { // EWOULDBLOCK means the lock isn't available yet.
                Err(io::Error::other(e))
            }
        }
    }
}

async fn get_file(path: &str) -> io::Result<File> {
    let mut f = File::options()
        .read(true)
        .write(true)
        .open(path)
        .await?;
    acquire_lock(&mut f, false).await?;
    Ok(f)
}

async fn exclusive_lock(path: &str) -> io::Result<()> {
    let mut f = File::open(path).await?;
    acquire_lock(&mut f, true).await?;
    Ok(())
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
