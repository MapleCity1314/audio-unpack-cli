use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyInit};
use aes::Aes128;
use anyhow::{bail, Context, Result};
use base64::Engine;
use clap::Parser;
use ecb::Decryptor;
use id3::frame::Picture;
use id3::frame::PictureType;
use id3::TagLike;
use metaflac::block::PictureType as FlacPictureType;
use metaflac::Tag as FlacTag;
use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

const FLAG: [u8; 8] = [0x43, 0x54, 0x45, 0x4e, 0x46, 0x44, 0x41, 0x4d];
const CORE_BOX_KEY: [u8; 16] = [
    0x68, 0x7A, 0x48, 0x52, 0x41, 0x6D, 0x73, 0x6F, 0x35, 0x6B, 0x49, 0x6E, 0x62, 0x61, 0x78,
    0x57,
];
const MODIFY_BOX_KEY: [u8; 16] = [
    0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27,
    0x28,
];
const BUFFER_SIZE: usize = 0x8000;

#[derive(Parser, Debug)]
#[command(name = "audio-unpack-cli", version, about = "Fast cross-platform audio container decoder")]
struct Cli {
    input_dir: PathBuf,
    output_dir: PathBuf,

    #[arg(long, default_value_t = true)]
    recursive: bool,

    #[arg(long)]
    overwrite: bool,

    #[arg(long)]
    strict_metadata: bool,

    #[arg(long)]
    verbose: bool,

    #[arg(long)]
    jobs: Option<usize>,
}

#[derive(Default)]
struct Summary {
    success: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    warnings: AtomicUsize,
}

#[derive(Clone, Debug, Default)]
struct Metadata {
    format: String,
    title: Option<String>,
    album: Option<String>,
    artists: Vec<String>,
}

#[derive(Debug)]
struct WarningRecord {
    file: PathBuf,
    message: String,
}

#[derive(Debug)]
struct ErrorRecord {
    file: PathBuf,
    message: String,
}

struct DecodeOutcome {
    warnings: Vec<String>,
}

struct NcmDecoder {
    reader: BufReader<File>,
    key_box: [u8; 256],
    metadata: Metadata,
    cover: Vec<u8>,
    metadata_warnings: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(jobs) = cli.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs.max(1))
            .build_global()
            .context("failed to configure rayon thread pool")?;
    }

    if !cli.input_dir.is_dir() {
        bail!("input path is not a directory: {}", cli.input_dir.display());
    }

    fs::create_dir_all(&cli.output_dir).with_context(|| {
        format!(
            "failed to create output directory {}",
            cli.output_dir.display()
        )
    })?;

    let files = collect_input_files(&cli.input_dir, cli.recursive)?;
    if files.is_empty() {
        println!("No supported input files found in {}", cli.input_dir.display());
        return Ok(());
    }

    let summary = Arc::new(Summary::default());
    let warnings = Arc::new(Mutex::new(Vec::<WarningRecord>::new()));
    let errors = Arc::new(Mutex::new(Vec::<ErrorRecord>::new()));

    files.par_iter().for_each(|input_path| {
        let result = process_one(input_path, &cli);
        match result {
            Ok(outcome) => {
                summary.success.fetch_add(1, Ordering::Relaxed);
                if cli.verbose {
                    println!("OK {}", input_path.display());
                }
                for message in outcome.warnings {
                    summary.warnings.fetch_add(1, Ordering::Relaxed);
                    if cli.verbose {
                        println!("WARN {}: {}", input_path.display(), message);
                    }
                    warnings.lock().expect("warnings mutex poisoned").push(WarningRecord {
                        file: input_path.clone(),
                        message,
                    });
                }
            }
            Err(ProcessResult::Skipped(message)) => {
                summary.skipped.fetch_add(1, Ordering::Relaxed);
                if cli.verbose {
                    println!("SKIP {}: {}", input_path.display(), message);
                }
            }
            Err(ProcessResult::Failed(message)) => {
                summary.failed.fetch_add(1, Ordering::Relaxed);
                errors.lock().expect("errors mutex poisoned").push(ErrorRecord {
                    file: input_path.clone(),
                    message: message.clone(),
                });
                eprintln!("FAIL {}: {}", input_path.display(), message);
            }
        }
    });

    print_summary(&summary, &warnings, &errors);

    if summary.failed.load(Ordering::Relaxed) > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn print_summary(
    summary: &Summary,
    warnings: &Mutex<Vec<WarningRecord>>,
    errors: &Mutex<Vec<ErrorRecord>>,
) {
    println!();
    println!("Summary");
    println!(
        "  success: {}",
        summary.success.load(Ordering::Relaxed)
    );
    println!(
        "  skipped: {}",
        summary.skipped.load(Ordering::Relaxed)
    );
    println!(
        "  failed: {}",
        summary.failed.load(Ordering::Relaxed)
    );
    println!(
        "  warnings: {}",
        summary.warnings.load(Ordering::Relaxed)
    );

    let warning_records = warnings.lock().expect("warnings mutex poisoned");
    if !warning_records.is_empty() {
        println!();
        println!("Warnings");
        for warning in warning_records.iter() {
            println!("  {}: {}", warning.file.display(), warning.message);
        }
    }

    let error_records = errors.lock().expect("errors mutex poisoned");
    if !error_records.is_empty() {
        println!();
        println!("Failures");
        for error in error_records.iter() {
            println!("  {}: {}", error.file.display(), error.message);
        }
    }
}

fn collect_input_files(input_dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut walker = WalkDir::new(input_dir).follow_links(false);
    if !recursive {
        walker = walker.max_depth(1);
    }

    let mut files = Vec::new();
    for entry in walker {
        let entry = entry.with_context(|| {
            format!("failed while scanning {}", input_dir.display())
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let is_supported = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("ncm"))
            .unwrap_or(false);
        if is_supported {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

enum ProcessResult {
    Skipped(String),
    Failed(String),
}

fn process_one(input_path: &Path, cli: &Cli) -> std::result::Result<DecodeOutcome, ProcessResult> {
    let relative_path = input_path
        .strip_prefix(&cli.input_dir)
        .map_err(|err| ProcessResult::Failed(err.to_string()))?;

    let decoder = NcmDecoder::new(input_path, cli.strict_metadata)
        .map_err(|err| ProcessResult::Failed(format!("{err:#}")))?;
    let output_path = build_output_path(relative_path, &cli.output_dir, &decoder.metadata.format)
        .map_err(|err| ProcessResult::Failed(format!("{err:#}")))?;

    if output_path.exists() && !cli.overwrite {
        return Err(ProcessResult::Skipped(format!(
            "output already exists at {}",
            output_path.display()
        )));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ProcessResult::Failed(format!(
                "failed to create output directory {}: {}",
                parent.display(),
                err
            ))
        })?;
    }

    decoder
        .decode_to_file(&output_path, cli.strict_metadata)
        .map_err(|err| ProcessResult::Failed(format!("{err:#}")))
}

fn build_output_path(relative_input: &Path, output_root: &Path, format: &str) -> Result<PathBuf> {
    if format.trim().is_empty() {
        bail!("metadata format is missing");
    }

    let mut relative_output = relative_input.to_path_buf();
    relative_output.set_extension(format.trim().to_ascii_lowercase());
    Ok(output_root.join(relative_output))
}

impl NcmDecoder {
    fn new(path: &Path, strict_metadata: bool) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut reader = BufReader::new(file);

        let mut flag = [0_u8; 8];
        reader.read_exact(&mut flag)?;
        if flag != FLAG {
            bail!("not a valid supported container file");
        }

        discard_bytes(&mut reader, 2)?;

        let mut core_key_chunk = read_chunk(&mut reader)?;
        for byte in &mut core_key_chunk {
            *byte ^= 0x64;
        }
        let decrypted_core = aes_decrypt(&core_key_chunk, &CORE_BOX_KEY)
            .context("failed to decrypt core key chunk")?;
        if decrypted_core.len() <= 17 {
            bail!("core key chunk is too short");
        }

        let key_material = &decrypted_core[17..];
        let key_box = build_key_box(key_material);

        let mut metadata_chunk = read_chunk(&mut reader)?;
        let mut metadata_warnings = Vec::new();
        for byte in &mut metadata_chunk {
            *byte ^= 0x63;
        }

        let start_index = metadata_chunk
            .iter()
            .position(|&byte| byte == b':')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let metadata_base64 = String::from_utf8_lossy(&metadata_chunk[start_index..]).into_owned();
        let metadata_cipher = base64::engine::general_purpose::STANDARD
            .decode(metadata_base64.trim())
            .context("failed to decode metadata base64")?;
        let metadata_plain = aes_decrypt(&metadata_cipher, &MODIFY_BOX_KEY)
            .context("failed to decrypt metadata chunk")?;
        let metadata = parse_metadata(&metadata_plain, strict_metadata, &mut metadata_warnings)?;

        discard_bytes(&mut reader, 9)?;
        let cover = read_chunk(&mut reader).context("failed to read cover chunk")?;

        Ok(Self {
            reader,
            key_box,
            metadata,
            cover,
            metadata_warnings,
        })
    }

    fn decode_to_file(mut self, output_path: &Path, strict_metadata: bool) -> Result<DecodeOutcome> {
        let output_file = File::create(output_path)
            .with_context(|| format!("failed to create {}", output_path.display()))?;
        let mut writer = BufWriter::new(output_file);
        let mut buf = vec![0_u8; BUFFER_SIZE];

        loop {
            let read = self.reader.read(&mut buf)?;
            if read == 0 {
                break;
            }

            for (i, byte) in buf[..read].iter_mut().enumerate() {
                let j = ((i + 1) & 0xff) as usize;
                let index =
                    ((self.key_box[j] as usize + self.key_box[(self.key_box[j] as usize + j) & 0xff] as usize)
                        & 0xff) as usize;
                *byte ^= self.key_box[index];
            }

            writer.write_all(&buf[..read])?;
        }
        writer.flush()?;

        let mut warnings = self.metadata_warnings;
        if let Err(err) = write_tags(output_path, &self.metadata, &self.cover) {
            if strict_metadata {
                return Err(err).context("failed to write metadata");
            }
            warnings.push(format!("failed to write metadata: {err:#}"));
        }

        Ok(DecodeOutcome { warnings })
    }
}

fn discard_bytes<R: Read>(reader: &mut R, len: usize) -> Result<()> {
    let mut discard = vec![0_u8; len];
    reader.read_exact(&mut discard)?;
    Ok(())
}

fn read_chunk<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut raw_len = [0_u8; 4];
    reader.read_exact(&mut raw_len)?;
    let len = u32::from_le_bytes(raw_len) as usize;
    let mut chunk = vec![0_u8; len];
    reader.read_exact(&mut chunk)?;
    Ok(chunk)
}

fn aes_decrypt(data: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let mut buf = data.to_vec();
    let decryptor = Decryptor::<Aes128>::new_from_slice(key).expect("AES-128 key length is fixed");
    let plain = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| anyhow::anyhow!("invalid PKCS7 padding"))?;
    Ok(plain.to_vec())
}

fn build_key_box(key_material: &[u8]) -> [u8; 256] {
    let mut key_box = [0_u8; 256];
    for (idx, value) in key_box.iter_mut().enumerate() {
        *value = idx as u8;
    }

    let mut last_byte = 0_u8;
    let mut key_offset = 0_usize;
    for i in 0..256 {
        let swap = key_box[i];
        let c = ((swap as usize + last_byte as usize + key_material[key_offset] as usize) & 0xff) as u8;
        key_offset += 1;
        if key_offset >= key_material.len() {
            key_offset = 0;
        }
        key_box[i] = key_box[c as usize];
        key_box[c as usize] = swap;
        last_byte = c;
    }

    key_box
}

fn parse_metadata(
    metadata_plain: &[u8],
    strict_metadata: bool,
    warnings: &mut Vec<String>,
) -> Result<Metadata> {
    if metadata_plain.len() < 6 {
        bail!("metadata chunk is too short");
    }

    let payload = if metadata_plain.starts_with(b"music:") {
        &metadata_plain[6..]
    } else {
        metadata_plain
    };

    let trimmed = trim_ascii_control(payload);
    let value: Value = serde_json::from_slice(trimmed).context("failed to parse metadata JSON")?;
    let format = value
        .get("format")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let title = value
        .get("musicName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let album = value
        .get("album")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let artists = parse_artists(&value, warnings);

    let format = match format {
        Some(format) => format,
        None if strict_metadata => bail!("metadata is missing format"),
        None => {
            warnings.push("metadata is missing format, falling back to mp3".to_string());
            "mp3".to_string()
        }
    };

    Ok(Metadata {
        format,
        title,
        album,
        artists,
    })
}

fn parse_artists(value: &Value, warnings: &mut Vec<String>) -> Vec<String> {
    let Some(artist_value) = value.get("artist") else {
        return Vec::new();
    };

    let Some(items) = artist_value.as_array() else {
        warnings.push("artist field is not an array".to_string());
        return Vec::new();
    };

    let mut artists = Vec::new();
    for item in items {
        match item {
            Value::Array(parts) => {
                if let Some(name) = parts.first().and_then(Value::as_str) {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        artists.push(trimmed.to_string());
                    }
                }
            }
            Value::String(name) => {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    artists.push(trimmed.to_string());
                }
            }
            _ => warnings.push("artist item has unsupported shape".to_string()),
        }
    }

    artists
}

fn trim_ascii_control(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_control() || *byte == b'\n' || *byte == b'\r' || *byte == b'\t')
        .unwrap_or(0);
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_control() || *byte == b'\n' || *byte == b'\r' || *byte == b'\t')
        .map(|idx| idx + 1)
        .unwrap_or(bytes.len());
    &bytes[start..end]
}

fn write_tags(output_path: &Path, metadata: &Metadata, cover: &[u8]) -> Result<()> {
    match metadata.format.as_str() {
        "mp3" => write_mp3_tags(output_path, metadata, cover),
        "flac" => write_flac_tags(output_path, metadata, cover),
        other => bail!("tag writing is not implemented for format {other}"),
    }
}

fn write_mp3_tags(output_path: &Path, metadata: &Metadata, cover: &[u8]) -> Result<()> {
    let mut tag = id3::Tag::read_from_path(output_path).unwrap_or_else(|_| id3::Tag::new());

    if let Some(title) = metadata.title.as_deref() {
        tag.set_title(title);
    }
    if !metadata.artists.is_empty() {
        tag.set_artist(metadata.artists.join("/"));
    }
    if let Some(album) = metadata.album.as_deref() {
        tag.set_album(album);
    }
    if !cover.is_empty() {
        tag.remove_all_pictures();
        tag.add_frame(Picture {
            mime_type: detect_mime_type(cover).to_string(),
            picture_type: PictureType::CoverFront,
            description: String::new(),
            data: cover.to_vec(),
        });
    }

    tag.write_to_path(output_path, id3::Version::Id3v24)
        .with_context(|| format!("failed to write ID3 tag to {}", output_path.display()))
}

fn write_flac_tags(output_path: &Path, metadata: &Metadata, cover: &[u8]) -> Result<()> {
    let mut tag = FlacTag::read_from_path(output_path)
        .with_context(|| format!("failed to read FLAC metadata from {}", output_path.display()))?;

    {
        let comments = tag.vorbis_comments_mut();
        comments.remove("TITLE");
        comments.remove("ARTIST");
        comments.remove("ALBUM");

        if let Some(title) = metadata.title.as_deref() {
            comments.set_title(vec![title.to_string()]);
        }
        if !metadata.artists.is_empty() {
            comments.set_artist(metadata.artists.clone());
        }
        if let Some(album) = metadata.album.as_deref() {
            comments.set_album(vec![album.to_string()]);
        }
    }

    if !cover.is_empty() {
        tag.remove_picture_type(FlacPictureType::CoverFront);
        tag.add_picture(detect_mime_type(cover), FlacPictureType::CoverFront, cover.to_vec());
    }

    tag.save()
        .with_context(|| format!("failed to write FLAC tag to {}", output_path.display()))
}

fn detect_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}
