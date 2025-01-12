use futures_util::StreamExt as _;
use nix::{sys::statvfs::statvfs, fcntl::posix_fallocate};
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

pub const DATA_DIR: &str = "data";

async fn acquire_lock(file: &mut File, exclusive: bool) -> io::Result<()> {
    let fd = file.as_raw_fd();
    spawn_blocking(move || common::acquire_lock(fd, exclusive)).await?
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

pub async fn exclusive_lock(mut path: PathBuf, id: &str) -> io::Result<File> {
    path.push(id);
    let mut f = File::open(&path).await?;
    acquire_lock(&mut f, true).await?;
    Ok(f)
}

pub async fn new_file(mut path: PathBuf, id: &str, with_size: u64) -> io::Result<()> {
    let with_size: i64 = match with_size.try_into() {
        Ok(s) => s,
        Err(_) => return Err(io::Error::other("File too large")),
    };
    path.push(id);
    let file = File::create_new(&path).await?;
    let fd = file.as_fd().as_raw_fd();
    if with_size > 0 {
        match spawn_blocking(move || posix_fallocate(fd, 0, with_size)).await? {
            Ok(()) => io::Result::Ok(()),
            Err(e) => {
                remove_file(path).await?;
                io::Result::Err(io::Error::other(format!("{e}")))
            }
        }
    } else {
        // posix_fallocate doesn't accept len <= 0, but that space is already guaranteed anyway
        io::Result::Ok(())
    }
}

pub async fn delete_file(mut path: PathBuf, id: &str) -> io::Result<()> {
    path.push(id);
    remove_file(path).await?;
    Ok(())
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

// TODO: Tests are run in parallel, so how do I test this?
// Other tests may have started when we check free space.
async fn get_free_space(path: PathBuf) -> io::Result<u64> {
    let stats = spawn_blocking(move || statvfs(&path)).await??;
    let fragment_size = stats.fragment_size();
    let available_blocks = stats.blocks_available();
    Ok(fragment_size * available_blocks)
}

#[cfg(test)]
mod tests {
    use std::{mem, path::PathBuf};

    use actix_web::{test::{self, TestRequest}, App};
    use tokio::fs::{self, File, OpenOptions};

    use crate::files::{self, new_file};
    use super::{get_free_space, DATA_DIR};

    /// Ensures that file creation and deletion works as expected.
    #[actix_web::test]
    async fn test_create_delete() {
        const NAME: &str = "Unit-test-NewFile";
        let mut dir = std::env::current_dir().unwrap();
        dir.push(DATA_DIR);
        files::new_file(dir.clone(), NAME, 20).await.unwrap();
        let mut file = dir.clone();
        file.push(NAME);
        let m = fs::metadata(file.clone()).await.unwrap();
        assert_eq!(m.len(), 20);
        files::delete_file(dir, NAME).await.unwrap();
        fs::metadata(file).await.unwrap_err();
    }

    /// Ensures that locks work as expected.
    #[actix_web::test]
    async fn test_locks() {
        const NAME: &str = "Unit-test-Locks";
        let mut dir = std::env::current_dir().unwrap();
        dir.push(DATA_DIR);
        let mut path = dir.clone();
        path.push(NAME);
        let mut file = OpenOptions::new().create(true).write(true).open(&path).await.unwrap();
        let mut file2 = File::open(&path).await.unwrap();
        let mut file3 = File::open(&path).await.unwrap();
        let mut file4 = File::open(&path).await.unwrap();
        // Shared lock. Succeeds.
        files::acquire_lock(&mut file, false).await.unwrap();
        // Exclusive lock. Fails due to the preexisting shared lock.
        files::acquire_lock(&mut file2, true).await.unwrap_err();
        // Shared lock. Succeeds because the only other lock is shared.
        files::acquire_lock(&mut file3, false).await.unwrap();
        // Exclusive lock. Fails due to the preexisting shared lock.
        files::exclusive_lock(dir, NAME).await.unwrap_err();
        // Close shared locks
        mem::drop(file);
        mem::drop(file3);
        // Exclusive lock. Succeeds; other locks have been closed.
        files::acquire_lock(&mut file2, true).await.unwrap();
        // Shared lock. Fails due to exclusive lock.
        files::acquire_lock(&mut file4, false).await.unwrap_err();
    }

    /// Ensures that new_file does not overwrite existing files.
    #[actix_web::test]
    async fn test_file_exclusivity() {
        const NAME: &str = "Unit-test-Exclusivity";
        let mut dir = std::env::current_dir().unwrap();
        dir.push(DATA_DIR);
        new_file(dir.clone(), NAME, 20).await.unwrap();
        new_file(dir.clone(), NAME, 25).await.unwrap_err();
        dir.push(NAME);
        assert_eq!(fs::metadata(dir.clone()).await.unwrap().len(), 20);
        fs::remove_file(dir).await.unwrap();
    }

    /// Regression test. Ensures that zero-size files work.
    #[actix_web::test]
    async fn test_zero_size_file() {
        const NAME: &str = "Unit-test-ZeroSize";
        let mut dir = std::env::current_dir().unwrap();
        dir.push(DATA_DIR);
        new_file(dir.clone(), NAME, 0).await.unwrap();
        dir.push(NAME);
        assert_eq!(fs::metadata(dir.clone()).await.unwrap().len(), 0);
        fs::remove_file(dir).await.unwrap();
    }

    #[actix_web::test]
    async fn test_free_space_works() {
        let pb: PathBuf = [DATA_DIR].iter().collect();
        get_free_space(pb).await.unwrap();
    }
}
