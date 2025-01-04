use std::{io, os::fd::RawFd};

use base16ct::lower::encode_string;
// See the acquire_lock function for rationale.
#[allow(deprecated)]
use nix::{errno::Errno, fcntl::flock};
use sha2::{Digest, Sha256};

pub mod data;
#[cfg(feature = "db")]
pub mod db;
pub mod payloads;
#[cfg(feature = "db")]
pub mod helpers;

pub fn hash_file<T: io::Read>(mut file: T) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let rv: [u8; 32] = hasher.finalize().into();
    Ok(encode_string(&rv))
}

pub fn acquire_lock(fd: RawFd, exclusive: bool) -> io::Result<()> {
    let arg = match exclusive {
        true => nix::fcntl::FlockArg::LockExclusiveNonblock,
        false => nix::fcntl::FlockArg::LockSharedNonblock,
    };
    // We can't use the Flock struct because it requires an owned std::File or OwnedFd. I'm not
    // sure why it's so insistent on consuming the file. How does it expect you to *use* the file?
    // We could theoretically duplicate the file handle, but why would we when there's a
    // perfectly good deprecated function here?
    #[allow(deprecated)]
    let res = flock(fd, arg);
    match res {
        Ok(()) => Ok(()),
        Err(e) => {
            if e == Errno::EWOULDBLOCK { // The lock isn't available yet. Let the client retry.
                Err(io::Error::other("file is locked"))
            } else {
                Err(io::Error::other(e))
            }
        }
    }
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

