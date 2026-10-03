use std::io::Read;
use std::path::Path;

use sha2::Digest;

fn read_hashed<D: Digest>(mut reader: impl Read, mut hasher: D) -> std::io::Result<String> {
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha512_reader(reader: impl Read) -> std::io::Result<String> {
    read_hashed(reader, sha2::Sha512::new())
}

pub fn sha1_reader(reader: impl Read) -> std::io::Result<String> {
    read_hashed(reader, sha1::Sha1::new())
}

pub fn sha512_hex(path: &Path) -> std::io::Result<String> {
    sha512_reader(std::fs::File::open(path)?)
}

pub fn sha1_hex(path: &Path) -> std::io::Result<String> {
    sha1_reader(std::fs::File::open(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha512_of_abc() {
        let hash = sha512_reader(std::io::Cursor::new(b"abc")).unwrap();
        assert_eq!(
            hash,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn test_sha1_of_abc() {
        let hash = sha1_reader(std::io::Cursor::new(b"abc")).unwrap();
        assert_eq!(hash, "a9993e364706816aba3e25717850c26c9cd0d89d");
    }
}
