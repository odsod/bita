#[macro_use]
extern crate error_chain;
extern crate atty;
extern crate blake2;
extern crate clap;
extern crate curl;
extern crate lzma;
extern crate num_cpus;
extern crate protobuf;
extern crate threadpool;
extern crate zstd;

mod archive;
mod archive_reader;
mod buzhash;
mod chunk_dictionary;
mod chunker;
mod chunker_utils;
mod compress_cmd;
mod config;
mod errors;
mod file_archive_backend;
mod ordered_mpsc;
mod remote_archive_backend;
mod string_utils;
mod unpack_cmd;

use std::process;
use threadpool::ThreadPool;

use clap::{App, Arg, SubCommand};
use config::*;
use errors::*;
use std::path::Path;

pub const BUZHASH_SEED: u32 = 0x1032_4195;
pub const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");

fn parse_size(size_str: &str) -> usize {
    let size_val: String = size_str.chars().filter(|a| a.is_numeric()).collect();
    let size_val: usize = size_val.parse().expect("parse");
    let size_unit: String = size_str.chars().filter(|a| !a.is_numeric()).collect();
    if size_unit.is_empty() {
        return size_val;
    }
    match size_unit.as_str() {
        "TiB" => 1024 * 1024 * 1024 * 1024 * size_val,
        "GiB" => 1024 * 1024 * 1024 * size_val,
        "MiB" => 1024 * 1024 * size_val,
        "KiB" => 1024 * size_val,
        "B" => size_val,
        _ => panic!("Invalid size unit"),
    }
}

fn parse_opts() -> Result<Config> {
    let matches = App::new(PKG_NAME)
        .version(PKG_VERSION)
        .arg(
            Arg::with_name("force-create")
                .short("f")
                .long("force-create")
                .help("Overwrite output files if they exist.")
                .global(true),
        )
        .subcommand(
            SubCommand::with_name("compress")
                .about("Compress a file or stream.")
                .arg(
                    Arg::with_name("INPUT")
                        .short("i")
                        .long("input")
                        .value_name("FILE")
                        .help("Input file. If none is given then stdin will be used.")
                        .required(false),
                )
                .arg(
                    Arg::with_name("OUTPUT")
                        .value_name("OUTPUT")
                        .help("Output file.")
                        .required(true),
                )
                .arg(
                    Arg::with_name("avg-chunk-size")
                        .long("avg-chunk-size")
                        .value_name("SIZE")
                        .help("Indication of target chunk size [default: 64KiB]."),
                )
                .arg(
                    Arg::with_name("min-chunk-size")
                        .long("min-chunk-size")
                        .value_name("SIZE")
                        .help("Minimal size of chunks [default: 16KiB]."),
                )
                .arg(
                    Arg::with_name("max-chunk-size")
                        .long("max-chunk-size")
                        .value_name("SIZE")
                        .help("Maximal size of chunks [default: 16MiB]."),
                )
                .arg(
                    Arg::with_name("buzhash-window")
                        .long("buzhash-window")
                        .value_name("SIZE")
                        .help("Size of the buzhash window [default: 16B]."),
                )
                .arg(
                    Arg::with_name("hash-length")
                        .long("hash-length")
                        .value_name("LENGTH")
                        .help("Truncate the length of the stored strong hash [default: 64]."),
                )
                .arg(
                    Arg::with_name("compression-level")
                        .long("compression-level")
                        .value_name("LEVEL")
                        .help("Set the chunk data compression level (1-19) [default: 6]."),
                )
                .arg(
                    Arg::with_name("compression")
                        .long("compression")
                        .value_name("TYPE")
                        .help("Set the chunk data compression type (LZMA, ZSTD, NONE) [default: LZMA]."),
                )
                .arg(
                    Arg::with_name("chunk-dir")
                        .long("chunk-dir")
                        .conflicts_with("chunk-archive")
                        .value_name("DIRECTORY")
                        .help("Where chunk files should be stored [default: OUTPUT.store]."),
                )
                .arg(
                    Arg::with_name("chunk-archive")
                        .long("chunk-archive")
                        .conflicts_with("chunk-store")
                        .value_name("FILE")
                        .help("File where chunk data should be stored."),
                )
        )
        .subcommand(
            SubCommand::with_name("unpack")
                .about("Unpack a file.")
                .arg(
                    Arg::with_name("INPUT")
                        .value_name("INPUT")
                        .help("Input file. Can be a local cba file or a URL.")
                        .required(true),
                )
                .arg(
                    Arg::with_name("OUTPUT")
                        .value_name("OUTPUT")
                        .help("Output file.")
                        .required(true),
                )
                .arg(
                    Arg::with_name("seed")
                        .value_name("FILE")
                        .long("seed")
                        .help("File(s) to use as seed while unpacking.")
                        .multiple(true),
                ),
        )
        .get_matches();

    let base_config = BaseConfig {
        force_create: matches.is_present("force-create"),
    };

    if let Some(matches) = matches.subcommand_matches("compress") {
        let output = Path::new(matches.value_of("OUTPUT").unwrap());
        let input = if let Some(input) = matches.value_of("INPUT") {
            Some(Path::new(input).to_path_buf())
        } else {
            None
        };

        let chunk_store = if let Some(chunk_archive_path) = matches.value_of("chunk-archive") {
            ChunkStoreType::Archive(Path::new(chunk_archive_path).to_path_buf())
        } else {
            let chunk_dir_path = if let Some(path) = matches.value_of("chunk-dir") {
                Path::new(path).to_path_buf()
            } else {
                let output_file = output.file_stem().chain_err(|| "No output file!?")?;
                if let Some(output_dir) = output.parent() {
                    output_dir
                } else {
                    Path::new(".")
                }
                .join(output_file)
                .with_extension("store")
            };
            ChunkStoreType::Directory(chunk_dir_path.to_path_buf())
        };

        let avg_chunk_size = parse_size(matches.value_of("avg-chunk-size").unwrap_or("64KiB"));
        let min_chunk_size = parse_size(matches.value_of("min-chunk-size").unwrap_or("16KiB"));
        let max_chunk_size = parse_size(matches.value_of("max-chunk-size").unwrap_or("16MiB"));
        let hash_window_size = parse_size(matches.value_of("buzhash-window").unwrap_or("16B"));
        let hash_length = matches.value_of("hash-length").unwrap_or("64");
        let compression_level = matches
            .value_of("compression-level")
            .unwrap_or("6")
            .parse()
            .chain_err(|| "invalid compression level value")?;
        let compression = match matches.value_of("compression").unwrap_or("LZMA") {
            "LZMA" => chunk_dictionary::ChunkCompression_CompressionType::LZMA,
            "ZSTD" => chunk_dictionary::ChunkCompression_CompressionType::ZSTD,
            "NONE" => chunk_dictionary::ChunkCompression_CompressionType::NONE,
            _ => bail!("invalid compression"),
        };

        let chunk_filter_bits = avg_chunk_size.leading_zeros();
        if min_chunk_size > avg_chunk_size {
            bail!("min-chunk-size > avg-chunk-size");
        }
        if max_chunk_size < avg_chunk_size {
            bail!("max-chunk-size < avg-chunk-size");
        }
        if compression_level < 1 || compression_level > 19 {
            bail!("compression level not within range");
        }

        Ok(Config::Compress(CompressConfig {
            base: base_config,
            input,
            output: output.to_path_buf(),
            hash_length: hash_length
                .parse()
                .chain_err(|| "invalid hash length value")?,
            chunk_store,
            chunk_filter_bits,
            min_chunk_size,
            max_chunk_size,
            hash_window_size,
            compression_level,
            compression,
        }))
    } else if let Some(matches) = matches.subcommand_matches("unpack") {
        let input = matches.value_of("INPUT").unwrap();
        let output = matches.value_of("OUTPUT").unwrap_or("");
        let seed_files = matches
            .values_of("seed")
            .unwrap_or_default()
            .map(|s| s.to_string())
            .collect();
        Ok(Config::Unpack(UnpackConfig {
            base: base_config,
            input: input.to_string(),
            output: output.to_string(),
            seed_files,
            seed_stdin: false,
        }))
    } else {
        println!("Unknown command");
        process::exit(1);
    }
}

fn main() {
    let num_threads = num_cpus::get();
    let pool = ThreadPool::new(num_threads);

    let result = match parse_opts() {
        Ok(Config::Compress(config)) => compress_cmd::run(&config, &pool),
        Ok(Config::Unpack(config)) => unpack_cmd::run(&config, &pool),
        Err(e) => Err(e),
    };
    if let Err(ref e) = result {
        println!("error: {}", e);

        for e in e.iter().skip(1) {
            println!("Caused by: {}", e);
        }

        // Run with 'RUST_BACKTRACE=1' to get full backtrace
        if let Some(backtrace) = e.backtrace() {
            println!("backtrace: {:?}", backtrace);
        }

        ::std::process::exit(1);
    }
}
