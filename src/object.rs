use anyhow::{Context, bail};
use flate2::bufread::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use std::collections::BTreeMap;
use std::default;
use std::fs::{self, File, read};
use std::io::{BufReader, prelude::*};
use std::panic::resume_unwind;
use std::path::{Components, Path, PathBuf};
use std::ffi::{OsStr, OsString};

use std::str::FromStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use anyhow::Result;

use crate::{index, object};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    ExecRegularFile,    // 100755
    NExecRegularFile,   // 100644
    SymbolicLink,       // 120000
    Gitlink,            // 160000
    Directory,          // 40000(注意，长度与其他的不同)
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub enum Object {
    Blob(BlobObject),
    Tree(TreeObject),
    Commit, // TODO
    Tag,    // TODO
}

/// 从 zlib 解压流上读取 object 类型名（`blob` / `tree` / …），类似 `git cat-file -t`
/// 必须与同一 `BufReader<&mut ZlibDecoder<_>>` 上后续的 `read_tree` 等解析共用，勿另包一层 `BufReader`。
pub fn read_object_type<R: BufRead>(
    reader: &mut BufReader<&mut ZlibDecoder<R>>,
) -> anyhow::Result<String> {
    let mut buf = Vec::new();
    reader.read_until(b' ', &mut buf)?;
    buf.pop(); // 去掉分隔空格
    Ok(String::from_utf8(buf)?)
}

/// 跳过 `git cat-file` 头部里类型之后的 `<ascii-size>\0`（与 `read_object_type` 连用）。
pub fn skip_git_object_size_nul<R: BufRead>(reader: &mut R) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    reader.read_until(b'\0', &mut buf)?;
    Ok(())
}

pub struct BlobObject {
    object_name: ObjectSha,
}

impl BlobObject {
    pub fn object_name(&self) -> &ObjectSha {
        &self.object_name
    }
}

pub struct TreeEntry {
    pub file_mode: FileMode,
    pub object_name: ObjectSha,
}

impl TreeEntry {
    pub fn read_entry<R: BufRead>(
        reader: &mut BufReader<&mut ZlibDecoder<R>>,
        is_sha1: bool
    ) -> Result<Option<(OsString, Self)> > {
        let mut mode_buf: Vec<u8> = Vec::new();
        let n = reader.read_until(b' ', &mut mode_buf)?;
        if n == 0 { 
            return Ok( None ) 
        }

        let mode_word = str::from_utf8(&mode_buf).unwrap().trim(); // 注意: read_until会读入分隔符
        let file_mode = match mode_word {
            "100755" => FileMode::ExecRegularFile,
            "100644" => FileMode::NExecRegularFile,
            "120000" => FileMode::SymbolicLink,
            "160000" => FileMode::Gitlink,
            "40000"  => FileMode::Directory,
            _ => bail!("invalid mode_word")
        };
        
        let mut file_path_buf: Vec<u8> = Vec::new();
        reader.read_until(b'\0', &mut file_path_buf)?;
        file_path_buf.pop();
        let file_path = OsString::from_vec(file_path_buf);

        let object_name = if is_sha1 {
            let mut sha1 = [0u8; 20];
            reader.read_exact(&mut sha1)?;
            ObjectSha::SHA1(sha1)
        } else {
            let mut sha2 = [0u8; 32];
            reader.read_exact(&mut sha2)?;
            ObjectSha::SHA256(sha2)
        };

        Ok( Some( (file_path, TreeEntry{file_mode, object_name}) ) )
    }

    pub fn to_binary(&self, file_name: &OsString) -> Vec<u8> {
        let mode_bin = match self.file_mode {
            FileMode::ExecRegularFile  => b"100755".to_vec(),        
            FileMode::NExecRegularFile => b"100644".to_vec(),         
            FileMode::SymbolicLink     => b"120000".to_vec(),     
            FileMode::Gitlink          => b"160000".to_vec(),
            FileMode::Directory        => b"40000".to_vec(),  
        };

        let file_name_bin = file_name.as_bytes();
        let object_name_bin = self.object_name.as_bytes().to_vec();

        let total_len = mode_bin.len() + 1 + file_name_bin.len() + 1 + object_name_bin.len();
        let mut result = Vec::with_capacity(total_len);     // 知识点: 用with_capacity预先算出需要分配堆的大小，减少堆分配次数
        result.extend_from_slice(&mode_bin);
        result.push(b' ');
        result.extend_from_slice(&file_name_bin);
        result.push(b'\0');
        result.extend_from_slice(&object_name_bin);
        result
    }
}

pub struct TreeObject {
    object_name: ObjectSha,
    entries: BTreeMap<OsString, TreeEntry>
}

impl TreeObject {
    pub fn object_name(&self) -> &ObjectSha {
        &self.object_name
    }

    pub fn entries(&self) -> &BTreeMap<OsString, TreeEntry> {
        &self.entries
    }

    pub fn read_tree<R: BufRead>(
        object_name: ObjectSha,
        reader: &mut BufReader<&mut ZlibDecoder<R>>,
        is_sha1: bool
    ) -> Result<TreeObject> {
        let mut entries: BTreeMap<OsString, TreeEntry> = BTreeMap::default();
        loop {
            match TreeEntry::read_entry(reader, is_sha1)? {
                None => return Ok(TreeObject { object_name, entries }),
                Some((file_path, entry)) => {
                    entries.insert(file_path, entry);
                }
            }
        }
    }

    pub fn entries_to_binary(entries: BTreeMap<OsString, TreeEntry>, _is_sha1: bool) -> Vec<u8> {
        let mut payload: Vec<u8> = Vec::new();
        for (file_name, entry) in entries {
            payload.extend_from_slice(&entry.to_binary(&file_name));
        }
        let header = format!("tree {}\0", payload.len()).into_bytes();
        let mut content = Vec::with_capacity(header.len() + payload.len());
        content.extend_from_slice(&header);
        content.extend_from_slice(&payload);
        content
    }

    pub fn read_loose_tree(repo: &Path, hex_oid: &str) -> TreeObject {
        let loose = repo
            .join("objects")
            .join(&hex_oid[0..2])
            .join(&hex_oid[2..]);
        assert!(
            loose.is_file(),
            "loose object missing: {}",
            loose.display()
        );

        let f = File::open(&loose).expect("open loose object");
        let raw = BufReader::new(f);
        let mut zlib = ZlibDecoder::new(raw);
        let mut br = BufReader::new(&mut zlib);

        let kind = object::read_object_type(&mut br).expect("read type");
        assert_eq!(kind, "tree", "cat-file -t should be tree");

        object::skip_git_object_size_nul(&mut br).expect("skip size\\0");

        let oid_bytes: [u8; 20] = hex::decode(hex_oid)
            .expect("hex oid")
            .try_into()
            .expect("oid len");
        let object_name = ObjectSha::SHA1(oid_bytes);

        TreeObject::read_tree(object_name, &mut br, true).expect("read_tree")
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
        let link_buf = link_path.as_os_str().as_bytes();
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
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(content)?;
    let compressed = encoder.finish()?;
    object_file.write_all(&compressed)?;
    Ok(())
}