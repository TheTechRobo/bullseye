use std::{fs, io};

use base16ct::lower::encode_string;
use sha2::{Digest, Sha256};

#[cfg(feature = "db")]
pub mod db;
pub mod payloads;
pub mod data;

pub fn hash_file(mut file: fs::File) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let rv: [u8; 32] = hasher.finalize().into();
    Ok(encode_string(&rv))
}
