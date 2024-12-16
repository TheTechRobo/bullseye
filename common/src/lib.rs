use std::io;

use base16ct::lower::encode_string;
use sha2::{Digest, Sha256};

pub mod data;
#[cfg(feature = "db")]
pub mod db;
pub mod payloads;

pub fn hash_file<T: io::Read>(mut file: T) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let rv: [u8; 32] = hasher.finalize().into();
    Ok(encode_string(&rv))
}

#[cfg(test)]
mod tests {
    use crate::hash_file;

    #[test]
    fn test_sha256() {
        let b = "This is a STRING!\n".as_bytes();
        let expected = "9d7780a699c93822709b3aeac17615f8bb4d2de6f17fb832a510bdf8cb96f6b9";
        assert_eq!(
            expected,
            hash_file(b).unwrap(),
        )
    }
}

