use std::{fs, io};

use base16ct::lower::encode_string;
use sha2::{Digest, Sha256};
use tokio::task::spawn_blocking;

pub mod db;
pub mod payloads;

pub async fn hash_file(mut file: fs::File) -> io::Result<String> {
    let hash = spawn_blocking(move || -> io::Result<[u8; 32]> {
        let mut hasher = Sha256::new();
        io::copy(&mut file, &mut hasher)?;
        let rv: [u8; 32] = hasher.finalize().into();
        Ok(rv)
    })
    .await??;
    Ok(encode_string(&hash))
}
