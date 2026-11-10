//! Epic's build formats, written from their structure:
//!
//! - **Manifest** (binary, or JSON in older builds): the files of a build, each
//!   made of parts of chunks, plus start data and custom fields.
//! - **Chunk**: a header with GUID, rolling hash and SHA-1, then the data
//!   (usually zlib-compressed, at most 1 MiB unpacked).
//!
//! Cloud saves use the same formats, so this module can also write both.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Write};

use sha1::{Digest, Sha1};

use crate::error::{AppError, AppResult};

const MANIFEST_MAGIC: u32 = 0x44BE_C00C;
const CHUNK_MAGIC: u32 = 0xB1FE_3AA2;
/// Bit of `stored_as`: the data is zlib-compressed.
const COMPRESSED: u8 = 0x01;
/// Bit of a chunk's `stored_as`: encrypted (not supported, never seen in games).
const ENCRYPTED: u8 = 0x02;
/// Manifest version Cove writes (cloud saves).
const WRITE_VERSION: u32 = 18;
/// Largest manifest or chunk Cove unpacks (protects against broken data).
const MAX_UNPACKED: usize = 512 << 20;

/// File flag: the file is executable on Linux/macOS.
pub const FLAG_EXECUTABLE: u8 = 0x04;

pub type Guid = [u32; 4];

fn broken(what: &str) -> AppError {
    eprintln!("[cove] Epic-Daten beschädigt: {what}");
    AppError::Download(tr!(
        "Epic hat beschädigte Daten geliefert. Cove versucht es beim Fortsetzen erneut.",
        "Epic sent damaged data. Cove tries again when you resume."
    ))
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    Sha1::digest(data).into()
}

pub fn guid_hex(guid: &Guid) -> String {
    guid.iter()
        .fold(String::with_capacity(32), |mut out, part| {
            let _ = write!(out, "{part:08X}");
            out
        })
}

fn parse_guid_hex(text: &str) -> Option<Guid> {
    if text.len() != 32 {
        return None;
    }
    let mut guid = [0u32; 4];
    for (i, part) in guid.iter_mut().enumerate() {
        *part = u32::from_str_radix(text.get(i * 8..i * 8 + 8)?, 16).ok()?;
    }
    Some(guid)
}

// ───────────── reading ─────────────

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> AppResult<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.data.len());
        let end = end.ok_or_else(|| broken("too short"))?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> AppResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> AppResult<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> AppResult<u64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    fn sha(&mut self) -> AppResult<[u8; 20]> {
        let mut sha = [0u8; 20];
        sha.copy_from_slice(self.take(20)?);
        Ok(sha)
    }

    fn guid(&mut self) -> AppResult<Guid> {
        Ok([self.u32()?, self.u32()?, self.u32()?, self.u32()?])
    }

    fn count(&mut self) -> AppResult<usize> {
        let n = usize::try_from(self.u32()?).map_err(|_| broken("count"))?;
        // Every element needs at least one byte: a larger count is garbage.
        if n > self.data.len() - self.pos {
            return Err(broken("count"));
        }
        Ok(n)
    }

    /// Unreal `FString`: length (with the trailing NUL); negative = UTF-16.
    fn string(&mut self) -> AppResult<String> {
        let length = i32::from_le_bytes(self.u32()?.to_le_bytes());
        if length == 0 {
            return Ok(String::new());
        }
        let text = if length > 0 {
            let bytes = self.take(usize::try_from(length).map_err(|_| broken("string"))?)?;
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            let units = usize::try_from(length.unsigned_abs()).map_err(|_| broken("string"))?;
            let bytes = self.take(units.checked_mul(2).ok_or_else(|| broken("string"))?)?;
            let wide: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16_lossy(&wide)
        };
        Ok(text.trim_end_matches('\0').to_owned())
    }

    fn strings(&mut self) -> AppResult<Vec<String>> {
        let n = self.count()?;
        (0..n).map(|_| self.string()).collect()
    }

    /// Start of a section (size, version); returns where it ends.
    fn section(&mut self) -> AppResult<(usize, u8)> {
        let start = self.pos;
        let size = usize::try_from(self.u32()?).map_err(|_| broken("section"))?;
        let version = self.u8()?;
        let end = start.checked_add(size).filter(|e| *e <= self.data.len());
        Ok((end.ok_or_else(|| broken("section"))?, version))
    }
}

fn inflate(data: &[u8], expected: usize) -> AppResult<Vec<u8>> {
    if expected > MAX_UNPACKED {
        return Err(broken("too large"));
    }
    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(data)
        .take(u64::try_from(MAX_UNPACKED).unwrap_or(u64::MAX))
        .read_to_end(&mut out)
        .map_err(|_| broken("zlib"))?;
    Ok(out)
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing into a Vec can't fail.
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

// ───────────── manifest ─────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkInfo {
    pub guid: Guid,
    pub hash: u64,
    pub sha: [u8; 20],
    pub group: u8,
    /// Unpacked size.
    pub window_size: u32,
    /// Size of the `.chunk` file (what is downloaded).
    pub file_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkPart {
    pub guid: Guid,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub sha: [u8; 20],
    pub flags: u8,
    pub parts: Vec<ChunkPart>,
}

impl FileEntry {
    pub fn size(&self) -> u64 {
        self.parts.iter().map(|p| u64::from(p.size)).sum()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    pub app_name: String,
    pub build_version: String,
    pub launch_exe: String,
    pub launch_command: String,
    pub chunks: HashMap<Guid, ChunkInfo>,
    pub files: Vec<FileEntry>,
    pub custom: HashMap<String, String>,
}

impl Manifest {
    /// Folder of the chunks below the CDN base URL (depends on the version).
    pub fn chunk_dir(&self) -> &'static str {
        match self.version {
            15.. => "ChunksV4",
            6.. => "ChunksV3",
            3.. => "ChunksV2",
            _ => "Chunks",
        }
    }

    /// `ChunksV4/12/0123456789ABCDEF_<GUID>.chunk`
    pub fn chunk_path(&self, chunk: &ChunkInfo) -> String {
        format!(
            "{}/{:02}/{:016X}_{}.chunk",
            self.chunk_dir(),
            chunk.group,
            chunk.hash,
            guid_hex(&chunk.guid)
        )
    }

    /// Bytes downloaded for the given chunks.
    pub fn download_size<'a>(&self, guids: impl IntoIterator<Item = &'a Guid>) -> u64 {
        guids
            .into_iter()
            .filter_map(|g| self.chunks.get(g))
            .map(|c| c.file_size)
            .sum()
    }

    pub fn install_size(&self) -> u64 {
        self.files.iter().map(FileEntry::size).sum()
    }

    pub fn parse(data: &[u8]) -> AppResult<Self> {
        if data.first() == Some(&b'{') {
            return parse_json(data);
        }
        let mut header = Reader::new(data);
        if header.u32()? != MANIFEST_MAGIC {
            return Err(broken("manifest magic"));
        }
        let header_size = usize::try_from(header.u32()?).map_err(|_| broken("header"))?;
        let size_unpacked = usize::try_from(header.u32()?).map_err(|_| broken("header"))?;
        let size_packed = usize::try_from(header.u32()?).map_err(|_| broken("header"))?;
        let sha = header.sha()?;
        let stored_as = header.u8()?;
        let version = header.u32()?;
        let body = data
            .get(header_size..)
            .ok_or_else(|| broken("manifest body"))?;
        let body = if stored_as & COMPRESSED != 0 {
            inflate(
                body.get(..size_packed)
                    .ok_or_else(|| broken("manifest size"))?,
                size_unpacked,
            )?
        } else {
            body.to_vec()
        };
        if self::sha1(&body) != sha {
            return Err(broken("manifest sha"));
        }
        let mut manifest = parse_body(&body)?;
        manifest.version = version;
        Ok(manifest)
    }
}

fn parse_body(body: &[u8]) -> AppResult<Manifest> {
    let mut r = Reader::new(body);
    let mut manifest = Manifest::default();

    // Meta
    let (end, _) = r.section()?;
    let _feature_level = r.u32()?;
    let _is_file_data = r.u8()?;
    let _app_id = r.u32()?;
    manifest.app_name = r.string()?;
    manifest.build_version = r.string()?;
    manifest.launch_exe = r.string()?;
    manifest.launch_command = r.string()?;
    r.pos = end;

    // Chunk data list: column by column.
    let (end, _) = r.section()?;
    let n = r.count()?;
    let guids: Vec<Guid> = (0..n).map(|_| r.guid()).collect::<AppResult<_>>()?;
    let hashes: Vec<u64> = (0..n).map(|_| r.u64()).collect::<AppResult<_>>()?;
    let shas: Vec<[u8; 20]> = (0..n).map(|_| r.sha()).collect::<AppResult<_>>()?;
    let groups: Vec<u8> = (0..n).map(|_| r.u8()).collect::<AppResult<_>>()?;
    let windows: Vec<u32> = (0..n).map(|_| r.u32()).collect::<AppResult<_>>()?;
    let sizes: Vec<u64> = (0..n).map(|_| r.u64()).collect::<AppResult<_>>()?;
    for i in 0..n {
        manifest.chunks.insert(
            guids[i],
            ChunkInfo {
                guid: guids[i],
                hash: hashes[i],
                sha: shas[i],
                group: groups[i],
                window_size: windows[i],
                file_size: sizes[i],
            },
        );
    }
    r.pos = end;

    // File manifest list.
    let (end, _) = r.section()?;
    let n = r.count()?;
    let names: Vec<String> = (0..n).map(|_| r.string()).collect::<AppResult<_>>()?;
    let _symlinks: Vec<String> = (0..n).map(|_| r.string()).collect::<AppResult<_>>()?;
    let shas: Vec<[u8; 20]> = (0..n).map(|_| r.sha()).collect::<AppResult<_>>()?;
    let flags: Vec<u8> = (0..n).map(|_| r.u8()).collect::<AppResult<_>>()?;
    for _ in 0..n {
        let _install_tags = r.strings()?;
    }
    for i in 0..n {
        let count = r.count()?;
        let mut parts = Vec::with_capacity(count);
        for _ in 0..count {
            let start = r.pos;
            let part_size = usize::try_from(r.u32()?).map_err(|_| broken("part"))?;
            parts.push(ChunkPart {
                guid: r.guid()?,
                offset: r.u32()?,
                size: r.u32()?,
            });
            r.pos = start.checked_add(part_size).ok_or_else(|| broken("part"))?;
        }
        manifest.files.push(FileEntry {
            name: names[i].clone(),
            sha: shas[i],
            flags: flags[i],
            parts,
        });
    }
    r.pos = end;

    // Custom fields (optional in very old builds).
    if r.pos < body.len() {
        let (end, _) = r.section()?;
        let n = r.count()?;
        let keys: Vec<String> = (0..n).map(|_| r.string()).collect::<AppResult<_>>()?;
        let values: Vec<String> = (0..n).map(|_| r.string()).collect::<AppResult<_>>()?;
        manifest.custom = keys.into_iter().zip(values).collect();
        if r.pos > end {
            return Err(broken("custom fields"));
        }
    }
    for file in &manifest.files {
        if file
            .parts
            .iter()
            .any(|p| !manifest.chunks.contains_key(&p.guid))
        {
            return Err(broken("unknown chunk"));
        }
    }
    Ok(manifest)
}

/// Older JSON manifests store numbers as "blobs": three decimal digits per byte, little-endian.
fn blob(text: &str) -> AppResult<Vec<u8>> {
    if text.len() % 3 != 0 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(broken("blob"));
    }
    text.as_bytes()
        .chunks(3)
        .map(|d| {
            std::str::from_utf8(d)
                .ok()
                .and_then(|s| s.parse::<u8>().ok())
                .ok_or_else(|| broken("blob"))
        })
        .collect()
}

fn blob_number(value: &serde_json::Value) -> AppResult<u64> {
    let bytes = blob(value.as_str().ok_or_else(|| broken("blob"))?)?;
    Ok(bytes
        .iter()
        .take(8)
        .enumerate()
        .fold(0u64, |n, (i, b)| n | (u64::from(*b) << (8 * i))))
}

fn parse_json(data: &[u8]) -> AppResult<Manifest> {
    let json: serde_json::Value = serde_json::from_slice(data).map_err(|_| broken("json"))?;
    let text = |key: &str| json[key].as_str().unwrap_or_default().to_owned();
    let mut manifest = Manifest {
        version: u32::try_from(blob_number(&json["ManifestFileVersion"]).unwrap_or(0)).unwrap_or(0),
        app_name: text("AppNameString"),
        build_version: text("BuildVersionString"),
        launch_exe: text("LaunchExeString"),
        launch_command: text("LaunchCommand"),
        ..Manifest::default()
    };
    let empty = serde_json::Map::new();
    let hashes = json["ChunkHashList"].as_object().unwrap_or(&empty);
    for (key, hash) in hashes {
        let guid = parse_guid_hex(key).ok_or_else(|| broken("guid"))?;
        let sha = json["ChunkShaList"][key]
            .as_str()
            .and_then(hex20)
            .unwrap_or([0; 20]);
        manifest.chunks.insert(
            guid,
            ChunkInfo {
                guid,
                hash: blob_number(hash)?,
                sha,
                group: u8::try_from(blob_number(&json["DataGroupList"][key]).unwrap_or(0) % 100)
                    .unwrap_or(0),
                window_size: 1 << 20,
                file_size: blob_number(&json["ChunkFilesizeList"][key]).unwrap_or(0),
            },
        );
    }
    for file in json["FileManifestList"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let mut sha = [0u8; 20];
        let hash = blob(file["FileHash"].as_str().unwrap_or_default())?;
        if hash.len() == 20 {
            sha.copy_from_slice(&hash);
        }
        let parts = file["FileChunkParts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|p| {
                Ok(ChunkPart {
                    guid: parse_guid_hex(p["Guid"].as_str().unwrap_or_default())
                        .ok_or_else(|| broken("guid"))?,
                    offset: u32::try_from(blob_number(&p["Offset"])?)
                        .map_err(|_| broken("offset"))?,
                    size: u32::try_from(blob_number(&p["Size"])?).map_err(|_| broken("size"))?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        manifest.files.push(FileEntry {
            name: file["Filename"].as_str().unwrap_or_default().to_owned(),
            sha,
            flags: if file["bIsUnixExecutable"].as_bool() == Some(true) {
                FLAG_EXECUTABLE
            } else {
                0
            },
            parts,
        });
    }
    if let Some(custom) = json["CustomFields"].as_object() {
        manifest.custom = custom
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect();
    }
    Ok(manifest)
}

fn hex20(text: &str) -> Option<[u8; 20]> {
    if text.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

// ───────────── chunks ─────────────

/// Unpacks a downloaded `.chunk` file and checks it against the manifest.
pub fn read_chunk(data: &[u8], expected: &ChunkInfo) -> AppResult<Vec<u8>> {
    let mut r = Reader::new(data);
    if r.u32()? != CHUNK_MAGIC {
        return Err(broken("chunk magic"));
    }
    let header_version = r.u32()?;
    let header_size = usize::try_from(r.u32()?).map_err(|_| broken("chunk header"))?;
    let stored_size = usize::try_from(r.u32()?).map_err(|_| broken("chunk header"))?;
    let guid = r.guid()?;
    let _rolling = r.u64()?;
    let stored_as = r.u8()?;
    let header_sha = if header_version >= 2 {
        Some(r.sha()?)
    } else {
        None
    };
    if guid != expected.guid {
        return Err(broken("chunk guid"));
    }
    if stored_as & ENCRYPTED != 0 {
        return Err(broken("encrypted chunk"));
    }
    let stored = data
        .get(header_size..header_size.saturating_add(stored_size))
        .ok_or_else(|| broken("chunk size"))?;
    let unpacked = if stored_as & COMPRESSED != 0 {
        inflate(stored, usize::try_from(expected.window_size).unwrap_or(0))?
    } else {
        stored.to_vec()
    };
    let sha = if expected.sha == [0; 20] {
        header_sha
    } else {
        Some(expected.sha)
    };
    if sha.is_some_and(|s| s != [0; 20] && s != self::sha1(&unpacked)) {
        return Err(broken("chunk sha"));
    }
    Ok(unpacked)
}

/// Epic's rolling hash (64 bit, CRC-64/ECMA table, rotate by one per byte).
pub fn rolling_hash(data: &[u8]) -> u64 {
    const POLY: u64 = 0xC96C_5795_D787_0F42;
    static TABLE: std::sync::OnceLock<[u64; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u64; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut value = i as u64;
            for _ in 0..8 {
                value = if value & 1 == 1 {
                    (value >> 1) ^ POLY
                } else {
                    value >> 1
                };
            }
            *entry = value;
        }
        table
    });
    data.iter().fold(0u64, |hash, byte| {
        hash.rotate_left(1) ^ table[usize::from(*byte)]
    })
}

/// A new chunk (cloud saves): compressed, with rolling hash and SHA-1.
pub fn write_chunk(guid: Guid, data: &[u8]) -> (ChunkInfo, Vec<u8>) {
    let packed = deflate(data);
    let hash = rolling_hash(data);
    let sha = self::sha1(data);
    let header_size: u32 = 66;
    let len = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(packed.len() + 66);
    out.extend(CHUNK_MAGIC.to_le_bytes());
    out.extend(3u32.to_le_bytes()); // header version
    out.extend(header_size.to_le_bytes());
    out.extend(len(packed.len()).to_le_bytes());
    for part in guid {
        out.extend(part.to_le_bytes());
    }
    out.extend(hash.to_le_bytes());
    out.push(COMPRESSED);
    out.extend(sha);
    out.push(3); // hash type: rolling + SHA-1
    out.extend(len(data.len()).to_le_bytes());
    out.extend(&packed);
    let info = ChunkInfo {
        guid,
        hash,
        sha,
        group: u8::try_from(crc32fast_like(&guid) % 100).unwrap_or(0),
        window_size: len(data.len()),
        file_size: out.len() as u64,
    };
    (info, out)
}

/// Data group of a chunk: any stable number below 100 (Epic derives it from the GUID).
fn crc32fast_like(guid: &Guid) -> u32 {
    guid.iter()
        .fold(0u32, |acc, part| acc.wrapping_mul(31).wrapping_add(*part))
}

// ───────────── writing (cloud saves) ─────────────

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend(v.to_le_bytes());
    }

    fn count(&mut self, n: usize) {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }

    /// ASCII when possible, else UTF-16 (negative length).
    fn string(&mut self, text: &str) {
        if text.is_empty() {
            self.u32(0);
        } else if text.is_ascii() {
            self.count(text.len() + 1);
            self.0.extend(text.as_bytes());
            self.0.push(0);
        } else {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let length = -i32::try_from(wide.len()).unwrap_or(i32::MAX);
            self.0.extend(length.to_le_bytes());
            for unit in wide {
                self.0.extend(unit.to_le_bytes());
            }
        }
    }

    /// Writes a section; its size is filled in afterwards.
    fn section(&mut self, version: u8, body: impl FnOnce(&mut Self)) {
        let start = self.0.len();
        self.u32(0);
        self.u8(version);
        body(self);
        let size = u32::try_from(self.0.len() - start).unwrap_or(u32::MAX);
        self.0[start..start + 4].copy_from_slice(&size.to_le_bytes());
    }
}

impl Manifest {
    /// Binary manifest, compressed (cloud saves).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.section(0, |w| {
            w.u32(WRITE_VERSION);
            w.u8(1); // file data
            w.u32(0);
            w.string(&self.app_name);
            w.string(&self.build_version);
            w.string(&self.launch_exe);
            w.string(&self.launch_command);
            w.count(0); // prerequisites
            w.string("");
            w.string("");
            w.string("");
        });
        let mut chunks: Vec<&ChunkInfo> = self.chunks.values().collect();
        chunks.sort_by_key(|c| c.guid);
        w.section(0, |w| {
            w.count(chunks.len());
            for c in &chunks {
                for part in c.guid {
                    w.u32(part);
                }
            }
            for c in &chunks {
                w.u64(c.hash);
            }
            for c in &chunks {
                w.0.extend(c.sha);
            }
            for c in &chunks {
                w.u8(c.group);
            }
            for c in &chunks {
                w.u32(c.window_size);
            }
            for c in &chunks {
                w.u64(c.file_size);
            }
        });
        w.section(0, |w| {
            w.count(self.files.len());
            for f in &self.files {
                w.string(&f.name);
            }
            for _ in &self.files {
                w.string("");
            }
            for f in &self.files {
                w.0.extend(f.sha);
            }
            for f in &self.files {
                w.u8(f.flags);
            }
            for _ in &self.files {
                w.count(0); // install tags
            }
            for f in &self.files {
                w.count(f.parts.len());
                for p in &f.parts {
                    w.u32(28);
                    for part in p.guid {
                        w.u32(part);
                    }
                    w.u32(p.offset);
                    w.u32(p.size);
                }
            }
        });
        let mut custom: Vec<(&String, &String)> = self.custom.iter().collect();
        custom.sort();
        w.section(0, |w| {
            w.count(custom.len());
            for (k, _) in &custom {
                w.string(k);
            }
            for (_, v) in &custom {
                w.string(v);
            }
        });
        let body = w.0;
        let packed = deflate(&body);
        let len = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let mut out = Vec::with_capacity(packed.len() + 41);
        out.extend(MANIFEST_MAGIC.to_le_bytes());
        out.extend(41u32.to_le_bytes());
        out.extend(len(body.len()).to_le_bytes());
        out.extend(len(packed.len()).to_le_bytes());
        out.extend(self::sha1(&body));
        out.push(COMPRESSED);
        out.extend(WRITE_VERSION.to_le_bytes());
        out.extend(packed);
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Manifest, chunk files by GUID, and the expected files.
    pub(crate) type Sample = (Manifest, HashMap<Guid, Vec<u8>>, Vec<(String, Vec<u8>)>);

    /// A manifest with two files over two chunks, and the chunk files.
    pub(crate) fn sample() -> Sample {
        let a =
            b"Hello from chunk A, which holds the start of game.exe and all of readme.txt".to_vec();
        let b = b"Chunk B: the rest of game.exe".to_vec();
        let (ca, fa) = write_chunk([1, 2, 3, 4], &a);
        let (cb, fb) = write_chunk([5, 6, 7, 8], &b);
        let exe: Vec<u8> = [&a[..30], &b[..]].concat();
        let readme = a[30..].to_vec();
        let manifest = Manifest {
            version: WRITE_VERSION,
            app_name: "Sugar".into(),
            build_version: "1.0".into(),
            launch_exe: "Binaries/Win64/Sugar.exe".into(),
            launch_command: "-nosplash".into(),
            chunks: [(ca.guid, ca.clone()), (cb.guid, cb.clone())]
                .into_iter()
                .collect(),
            files: vec![
                FileEntry {
                    name: "Binaries/Win64/Sugar.exe".into(),
                    sha: sha1(&exe),
                    flags: FLAG_EXECUTABLE,
                    parts: vec![
                        ChunkPart {
                            guid: ca.guid,
                            offset: 0,
                            size: 30,
                        },
                        ChunkPart {
                            guid: cb.guid,
                            offset: 0,
                            size: u32::try_from(b.len()).unwrap(),
                        },
                    ],
                },
                FileEntry {
                    name: "readme.txt".into(),
                    sha: sha1(&readme),
                    flags: 0,
                    parts: vec![ChunkPart {
                        guid: ca.guid,
                        offset: 30,
                        size: u32::try_from(readme.len()).unwrap(),
                    }],
                },
            ],
            custom: [("CloudSaveFolder".to_owned(), "{AppData}/Sugar".to_owned())]
                .into_iter()
                .collect(),
        };
        (
            manifest,
            [(ca.guid, fa), (cb.guid, fb)].into_iter().collect(),
            vec![
                ("Binaries/Win64/Sugar.exe".into(), exe),
                ("readme.txt".into(), readme),
            ],
        )
    }

    #[test]
    fn manifests_survive_a_roundtrip() {
        let (manifest, _, files) = sample();
        let bytes = manifest.to_bytes();
        assert_eq!(Manifest::parse(&bytes).unwrap(), manifest);
        let mut broken = bytes.clone();
        let last = broken.len() - 1;
        broken[last] ^= 0xFF;
        assert!(Manifest::parse(&broken).is_err());
        assert!(Manifest::parse(b"nonsense").is_err());
        let expected: usize = files.iter().map(|(_, data)| data.len()).sum();
        assert_eq!(manifest.install_size(), expected as u64);
    }

    #[test]
    fn chunks_are_checked() {
        let (manifest, files, _) = sample();
        let info = &manifest.chunks[&[1, 2, 3, 4]];
        let data = read_chunk(&files[&[1, 2, 3, 4]], info).unwrap();
        assert!(data.starts_with(b"Hello from chunk A"));
        let mut damaged = files[&[1, 2, 3, 4]].clone();
        let last = damaged.len() - 3;
        damaged[last] ^= 0x10;
        assert!(read_chunk(&damaged, info).is_err());
        let other = &manifest.chunks[&[5, 6, 7, 8]];
        assert!(
            read_chunk(&files[&[1, 2, 3, 4]], other).is_err(),
            "wrong guid"
        );
        assert_eq!(
            manifest.chunk_path(info),
            format!(
                "ChunksV4/{:02}/{:016X}_00000001000000020000000300000004.chunk",
                info.group, info.hash
            )
        );
    }

    #[test]
    fn strings_in_utf16_and_rolling_hash() {
        let mut w = Writer(Vec::new());
        w.string("Spielstände");
        w.string("plain");
        let mut r = Reader::new(&w.0);
        assert_eq!(r.string().unwrap(), "Spielstände");
        assert_eq!(r.string().unwrap(), "plain");
        assert_eq!(rolling_hash(b""), 0);
        assert_ne!(rolling_hash(b"a"), rolling_hash(b"b"));
        // Rotation makes the order matter.
        assert_ne!(rolling_hash(b"ab"), rolling_hash(b"ba"));
    }

    #[test]
    fn reads_old_json_manifests() {
        let json = br#"{
            "ManifestFileVersion": "013000000000",
            "AppNameString": "Old", "BuildVersionString": "1", "LaunchExeString": "Old.exe", "LaunchCommand": "",
            "FileManifestList": [ { "Filename": "Old.exe", "FileHash": "000000000000000000000000000000000000000000000000000000000000",
                "FileChunkParts": [ { "Guid": "00000001000000020000000300000004", "Offset": "000000000000", "Size": "016000000000" } ] } ],
            "ChunkHashList": { "00000001000000020000000300000004": "255000000000000000000000" },
            "DataGroupList": { "00000001000000020000000300000004": "007" },
            "ChunkFilesizeList": { "00000001000000020000000300000004": "100002000000000000000000" }
        }"#;
        let manifest = Manifest::parse(json).unwrap();
        assert_eq!(manifest.version, 13);
        assert_eq!(manifest.chunk_dir(), "ChunksV3");
        assert_eq!(manifest.files[0].parts[0].size, 16);
        let chunk = &manifest.chunks[&[1, 2, 3, 4]];
        assert_eq!(
            (chunk.hash, chunk.group, chunk.file_size),
            (255, 7, 100 + 2 * 256)
        );
    }
}
