use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

use glob::glob;
use nom::bytes::complete::tag;
use nom::character::complete::hex_digit1;
use nom::combinator::rest;
use nom::sequence::separated_pair;
use nom::IResult;
use vergen::EmitBuilder;

fn main() {
    print_build_directives();
    generate_build_info();
    generate_proto_structs();
    generate_signatures_structs();
}

// -----------------------------------------------------------------------------
// Directives
// -----------------------------------------------------------------------------
fn print_build_directives() {
    // any code change
    println!("cargo:rerun-if-changed=src/");
    // used in signatures codegen
    println!("cargo:rerun-if-changed=static/");
    // fixture files that are "inserted" into test code
    println!("cargo:rerun-if-changed=tests/");
    // retrigger database compile-time checks
    println!("cargo:rerun-if-changed=.sqlx/");
}

// -----------------------------------------------------------------------------
// Code generation: Proto files
// -----------------------------------------------------------------------------
fn generate_proto_structs() {
    tonic_build::configure().protoc_arg("--experimental_allow_proto3_optional").compile(&["static/proto/append_entry.proto"], &["static/proto"]).unwrap();
}

// -----------------------------------------------------------------------------
// Code generation: Build Info
// -----------------------------------------------------------------------------
fn generate_build_info() {
    if let Err(e) = EmitBuilder::builder()
        .build_timestamp()
        .git_branch()
        .git_describe(false, true, None)
        .git_sha(true)
        .git_commit_timestamp()
        .git_commit_message()
        .git_commit_author_name()
        .cargo_debug()
        .cargo_features()
        .rustc_semver()
        .rustc_channel()
        .rustc_host_triple()
        .emit()
    {
        panic!("failed to emit build information | reason={e:?}");
    };
}

// -----------------------------------------------------------------------------
// Code generation: Solidity signatures
// -----------------------------------------------------------------------------

fn generate_signatures_structs() {
    let signatures_file_content = generate_signature_maps_file();
    let signature_file_path = out_dir().join("signatures.rs");
    if let Err(e) = fs::write(&signature_file_path, signatures_file_content) {
        panic!("failed to write to file {signature_file_path:?} | reason={e:?}");
    }
}

const SIGNATURES_DIR: &str = "static/contracts/*.signatures";

/// Generates the `signatures.rs` file containing a static PHF map with Solidity hashes and their description.
fn generate_signature_maps_file() -> String {
    // list all signature files to be parsed
    let signature_files: Vec<PathBuf> = glob(SIGNATURES_DIR)
        .expect("Listing signature files should not fail")
        .map(|x| x.expect("Listing signature file should not fail"))
        .collect();

    if signature_files.is_empty() {
        panic!("No signature files found in \"{}\"", SIGNATURES_DIR);
    }

    // iterate contract signature files populating 4 bytes and 32 bytes signatures
    let mut seen = HashSet::<Vec<u8>>::new();
    let mut signatures_4_bytes = phf_codegen::Map::<[u8; 4]>::new();
    let mut signatures_32_bytes = phf_codegen::Map::<[u8; 32]>::new();
    for signature_file in signature_files {
        let prefix = signature_file.file_name().unwrap().to_str().unwrap().split('.').next().unwrap();
        let signatures_content = fs::read_to_string(&signature_file).expect("Reading signature file shoult not fail");
        populate_signature_maps(&signatures_content, &mut seen, &mut signatures_4_bytes, &mut signatures_32_bytes, prefix);
    }

    format!(
        "// WARNING: This file was auto-generated in `build.rs`, you're not supposed to manually edit it

        /// Mapping between 4 byte signatures and Solidity function and error signatures.
        pub static SIGNATURES_4_BYTES: phf::Map<[u8; 4], &'static str> = {};

        /// Mapping between 32 byte signatures and Solidity events signatures.
        pub static SIGNATURES_32_BYTES: phf::Map<[u8; 32], &'static str> = {};
        ",
        signatures_4_bytes.build(),
        signatures_32_bytes.build()
    )
}

fn populate_signature_maps(
    input: &str,
    seen: &mut HashSet<Vec<u8>>,
    signatures_4_bytes: &mut phf_codegen::Map<[u8; 4]>,
    signatures_32_bytes: &mut phf_codegen::Map<[u8; 32]>,
    prefix: &str,
) {
    for line in input.lines() {
        if let Ok((_, (id, signature))) = parse_signature(line) {
            if seen.contains(&id) {
                continue;
            }
            seen.insert(id.clone());

            let signature = format!("\"{}::{}\"", prefix, signature);
            match id.len() {
                4 => {
                    signatures_4_bytes.entry(id.try_into().unwrap(), &signature);
                }
                32 => {
                    signatures_32_bytes.entry(id.try_into().unwrap(), &signature);
                }
                _ => continue,
            };
        }
    }
}

fn parse_signature(input: &str) -> IResult<&str, (Vec<u8>, &str)> {
    let (remaining, (id, signature)) = separated_pair(hex_digit1, tag(": "), rest)(input)?;
    let id = const_hex::decode(id).unwrap();
    Ok((remaining, (id, signature)))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Read compiler ENV var OUT_DIR
fn out_dir() -> PathBuf {
    env::var_os("OUT_DIR").map(PathBuf::from).expect("Compiler should set OUT_DIR")
}
