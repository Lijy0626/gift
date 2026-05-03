use anyhow::{Context, bail};
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::{Path};

use anyhow::Result;

#[derive(Debug, Clone)]
pub enum ObjectSha {
    SHA1([u8; 20]),
    SHA256([u8; 32]), // TODO: support SHA256
}

impl ObjectSha {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            ObjectSha::SHA1(h) => h.as_slice(),
            ObjectSha::SHA256(h) => h.as_slice(),
        }
    }
}

pub fn hash_object(path: impl AsRef<Path>) -> Result<(ObjectSha, Vec<u8>)> {
    let path = path.as_ref();
    let metadata = fs::symlink_metadata(path)
    .with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.is_file(){
        let mut f = File::open(path)?;
        let mut buf: Vec<u8> = vec![];
        let _ = f.read_to_end(&mut buf)?;
        let mut content = format!("blob {}\0", buf.len()).as_bytes().to_vec();
        content.extend(buf);
        let hash: [u8; 20] = Sha1::digest(&content).try_into().unwrap();
        return Ok((ObjectSha::SHA1(hash), content))
    } else if metadata.is_symlink() {
        let link_path = fs::read_link(path)?;
        let link_buf = link_path.to_str().unwrap().as_bytes();
        let mut content = format!("blob {}\0", link_buf.len()).as_bytes().to_vec();
        content.extend(link_buf);
        let hash: [u8; 20] = Sha1::digest(&content).try_into().unwrap();
        return Ok((ObjectSha::SHA1(hash), content))
    } else {
        bail!(format!("unable to hash {}", path.display()));
    }
}

pub fn write_hash_object(
    root: impl AsRef<Path>,
    hash: &ObjectSha,
    content: &[u8],
) -> Result<(), anyhow::Error> {

    let hash = hex::encode(hash.as_bytes());
    let object_dir_path = root.as_ref().join("objects").join(&hash[0..2]);
    fs::create_dir_all(&object_dir_path)?;
    let object_file_path = object_dir_path.join(&hash[2..]);
    let mut object_file = File::create(object_file_path)?;
    object_file.write_all(content)?;
    Ok(())
}