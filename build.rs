use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

use glob::glob;
use nom::bytes::complete::tag;
use nom::character::complete::hex_digit1;
use nom::combinator::rest;
use nom::sequence::preceded;
use nom::sequence::separated_pair;
use nom::IResult;
use vergen::EmitBuilder;

fn main() {
    print_build_directives();
    generate_build_info();
    generate_proto_structs();
    generate_contracts_structs();
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
    // fixture files that are "inserted" into stratus code
    println!("cargo:rerun-if-changed=tests/fixtures/snapshots/");
    println!("cargo:rerun-if-changed=tests/fixtures/blocks/");
    // retrigger database compile-time checks
    println!("cargo:rerun-if-changed=.sqlx/");
}

// -----------------------------------------------------------------------------
// Code generation: Proto files
// -----------------------------------------------------------------------------
fn generate_proto_structs() {
    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(&["static/proto/append_entry.proto"], &["static/proto"])
        .unwrap();
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
// Code generation: Contract addresses
// -----------------------------------------------------------------------------
fn generate_contracts_structs() {
    write_module("contracts.rs", generate_contracts_module_content());
}

fn generate_contracts_module_content() -> String {
    let deployment_files = list_files("static/contracts-deployments/*.csv");

    let mut seen = HashSet::<Vec<u8>>::new();
    let mut contracts = phf_codegen::Map::<[u8; 20]>::new();
    for deployment_file in deployment_files {
        populate_contracts_map(&deployment_file.content, &mut seen, &mut contracts);
    }

    format!(
        "
        /// Mapping between 20 byte address and contract names.
        pub static CONTRACTS: phf::Map<[u8; 20], &'static str> = {};
        ",
        contracts.build()
    )
}

fn populate_contracts_map(file_content: &str, seen: &mut HashSet<Vec<u8>>, contracts: &mut phf_codegen::Map<[u8; 20]>) {
    for line in file_content.lines() {
        let (_, (address, name)) = parse_contract(line).unwrap();

        if seen.contains(&address) {
            continue;
        }
        seen.insert(address.clone());

        let name = format!("\"{name}\"");
        contracts.entry(address.try_into().unwrap(), &name);
    }
}

type ContractAddress = Vec<u8>;
type ContractName = str;
fn parse_contract(input: &str) -> IResult<&str, (ContractAddress, &ContractName)> {
    let (remaining, (address, name)) = separated_pair(preceded(tag("0x"), hex_digit1), tag(","), rest)(input)?;
    let address = const_hex::decode(address).unwrap();
    Ok((remaining, (address, name)))
}

// -----------------------------------------------------------------------------
// Code generation: Solidity signatures
// -----------------------------------------------------------------------------

fn generate_signatures_structs() {
    write_module("signatures.rs", generate_signature_module_content());
}

/// Generates the `signatures.rs` file containing a static PHF map with Solidity hashes and their description.
fn generate_signature_module_content() -> String {
    let signature_files = list_files("static/contracts/*.signatures");

    let mut seen = HashSet::<Vec<u8>>::new();
    let mut signatures_4_bytes = phf_codegen::Map::<[u8; 4]>::new();
    let mut signatures_32_bytes = phf_codegen::Map::<[u8; 32]>::new();
    for signature_file in signature_files {
        let prefix = signature_file.filename.file_name().unwrap().to_str().unwrap().split('.').next().unwrap();
        populate_signature_maps(&signature_file.content, &mut seen, &mut signatures_4_bytes, &mut signatures_32_bytes, prefix);
    }

    format!(
        "
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
    file_content: &str,
    seen: &mut HashSet<Vec<u8>>,
    signatures_4_bytes: &mut phf_codegen::Map<[u8; 4]>,
    signatures_32_bytes: &mut phf_codegen::Map<[u8; 32]>,
    prefix: &str,
) {
    for line in file_content.lines() {
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

type SolidityId = Vec<u8>;
type SoliditySignature = str;
fn parse_signature(input: &str) -> IResult<&str, (SolidityId, &SoliditySignature)> {
    let (remaining, (id, signature)) = separated_pair(hex_digit1, tag(": "), rest)(input)?;
    let id = const_hex::decode(id).unwrap();
    Ok((remaining, (id, signature)))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

struct InputFile {
    filename: PathBuf,
    content: String,
}

/// Lists files in a path and ensure at least one file exists.
fn list_files(pattern: &'static str) -> Vec<InputFile> {
    // list files
    let filenames: Vec<PathBuf> = glob(pattern)
        .expect("Listing files should not fail")
        .map(|x| x.expect("Listing file should not fail"))
        .collect();

    // ensure at least one exists
    if filenames.is_empty() {
        panic!("No signature files found in \"{}\"", pattern);
    }

    // read file contents
    let mut files = Vec::with_capacity(filenames.len());
    for filename in filenames {
        files.push(InputFile {
            content: fs::read_to_string(&filename).expect("Reading file should not fail"),
            filename,
        });
    }

    files
}

/// Writes generated module content to a file in the OUT_DIR.
fn write_module(file_basename: &'static str, file_content: String) {
    let out_dir = env::var_os("OUT_DIR").map(PathBuf::from).expect("Compiler should set OUT_DIR");
    let module_path = out_dir.join(file_basename);
    if let Err(e) = fs::write(&module_path, file_content) {
        panic!("failed to write to file {module_path:?} | reason={e:?}");
    }
}
