use bita::chunker_utils::HashBuf;
use bita::compression::Compression;
use std::path::PathBuf;

#[derive(Debug)]
pub struct CompressConfig {
    pub force_create: bool,

    // Use stdin if input not given
    pub input: Option<PathBuf>,
    pub output: PathBuf,
    pub temp_file: PathBuf,
    pub hash_length: usize,
    pub chunk_filter_bits: u32,
    pub min_chunk_size: usize,
    pub max_chunk_size: usize,
    pub hash_window_size: usize,
    pub compression_level: u32,
    pub compression: Compression,
}

#[derive(Debug)]
pub struct CloneConfig {
    pub force_create: bool,
    pub input: String,
    pub output: PathBuf,
    pub seed_stdin: bool,
    pub seed_files: Vec<PathBuf>,
    pub header_checksum: Option<HashBuf>,
}

#[derive(Debug)]
pub struct InfoConfig {
    pub input: String,
}

#[derive(Debug)]
pub enum Config {
    Compress(CompressConfig),
    Clone(CloneConfig),
    Info(InfoConfig),
}
