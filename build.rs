#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    // retrigger database compile-time checks
    println!("cargo:rerun-if-changed=.sqlx/");
}

// -----------------------------------------------------------------------------
// Code generation: Build Info
// -----------------------------------------------------------------------------
fn generate_build_info() {
    // Capture the hostname of the machine where the binary is being built
    let build_hostname = hostname::get().unwrap_or_default().to_string_lossy().into_owned();

    // Export BUILD_HOSTNAME as a compile-time environment variable
    println!("cargo:rustc-env=BUILD_HOSTNAME={}", build_hostname);

    // Capture OpenSSL version
    let openssl_version = Command::new("openssl")
        .arg("version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());

    // Capture glibc version (Linux only)
    let glibc_version = if cfg!(target_os = "linux") {
        Command::new("ldd")
            .arg("--version")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|s| s.lines().next().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        "not applicable".to_string()
    };

    // Export as compile-time environment variables
    println!("cargo:rustc-env=BUILD_OPENSSL_VERSION={}", openssl_version.trim());
    println!("cargo:rustc-env=BUILD_GLIBC_VERSION={}", glibc_version.trim());

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
        panic!("Failed to emit build information | reason={e:?}");
    };
}

// -----------------------------------------------------------------------------
// Code generation: Contract addresses
// -----------------------------------------------------------------------------
type ContractAddress = [u8; 20];
type ContractName = str;

fn generate_contracts_structs() {
    write_module("contracts.rs", generate_contracts_module_content());
}

fn generate_contracts_module_content() -> String {
    let deployment_files = list_files("static/contracts-deployments/*.csv");

    let mut seen = HashSet::<ContractAddress>::new();
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

fn populate_contracts_map(file_content: &str, seen: &mut HashSet<ContractAddress>, contracts: &mut phf_codegen::Map<[u8; 20]>) {
    for line in file_content.lines() {
        let (address, name) = parse_contract(line);

        if seen.contains(&address) {
            continue;
        }
        seen.insert(address);

        let name = format!("\"{name}\"");
        contracts.entry(address, &name);
    }
}

fn parse_contract(input: &str) -> (ContractAddress, &ContractName) {
    fn parse(input: &str) -> IResult<&str, (&str, &str)> {
        separated_pair(preceded(tag("0x"), hex_digit1), tag(","), rest)(input)
    }

    let (_, (address, name)) = parse(input).expect("Contract deployment line should match the expected pattern | pattern=[0x<hex_address>,<name>]\n");

    let address = match const_hex::decode(address) {
        Ok(address) => address,
        Err(e) => panic!("Failed to parse contract address as hexadecimal | value={} reason={:?}", address, e),
    };

    let address: [u8; 20] = match address.try_into() {
        Ok(address) => address,
        Err(address) => panic!(
            "Contract address should have 20 bytes | value={}, len={}",
            const_hex::encode_prefixed(&address),
            address.len()
        ),
    };

    (address, name)
}

// -----------------------------------------------------------------------------
// Code generation: Solidity signatures
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum SolidityId {
    FunctionOrError(SolidityFunctionOrErrorId),
    Event(SolidityEventId),
}
type SolidityFunctionOrErrorId = [u8; 4];
type SolidityEventId = [u8; 32];
type SoliditySignature = str;

fn generate_signatures_structs() {
    write_module("signatures.rs", generate_signature_module_content());
}

/// Generates the `signatures.rs` file containing a static PHF map with Solidity hashes and their description.
fn generate_signature_module_content() -> String {
    let signature_files = list_files("static/contracts-signatures/*.signatures");

    let mut seen = HashSet::<SolidityId>::new();
    let mut signatures_4_bytes = phf_codegen::Map::<[u8; 4]>::new();
    let mut signatures_32_bytes = phf_codegen::Map::<[u8; 32]>::new();
    for signature_file in signature_files {
        populate_signature_maps(&signature_file.content, &mut seen, &mut signatures_4_bytes, &mut signatures_32_bytes);
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
    seen: &mut HashSet<SolidityId>,
    signatures_4_bytes: &mut phf_codegen::Map<[u8; 4]>,
    signatures_32_bytes: &mut phf_codegen::Map<[u8; 32]>,
) {
    for line in file_content.lines() {
        // skip empty lines and header lines
        if line.is_empty() || line.contains("signatures") {
            continue;
        }

        // parse
        let (id, signature) = parse_signature(line);

        // check tracked
        if seen.contains(&id) {
            continue;
        }

        // track
        seen.insert(id);
        let signature = format!("\"{}\"", signature);
        match id {
            SolidityId::FunctionOrError(id) => {
                signatures_4_bytes.entry(id, &signature);
            }
            SolidityId::Event(id) => {
                signatures_32_bytes.entry(id, &signature);
            }
        }
    }
}

fn parse_signature(input: &str) -> (SolidityId, &SoliditySignature) {
    fn parse(input: &str) -> IResult<&str, (&str, &str)> {
        separated_pair(hex_digit1, tag(": "), rest)(input)
    }
    let (_, (id, signature)) = parse(input).expect("Solidity signature line should match the expected pattern | pattern=[0x<hex_id>: <signature>]\n");
    let id = match const_hex::decode(id) {
        Ok(id) => id,
        Err(e) => panic!("Failed to parse Solidity ID as hexadecimal | value={} reason={:?}", id, e),
    };

    // try to parse 32 bytes
    let id_result: Result<SolidityEventId, Vec<u8>> = id.clone().try_into();
    if let Ok(id) = id_result {
        return (SolidityId::Event(id), signature);
    }

    // try to parse 4 bytes
    let id_result: Result<SolidityFunctionOrErrorId, Vec<u8>> = id.clone().try_into();
    if let Ok(id) = id_result {
        return (SolidityId::FunctionOrError(id), signature);
    }

    // failure
    panic!(
        "Failed to parse Solidity ID as function, error or event | id={} len={}",
        const_hex::encode_prefixed(&id),
        id.len()
    );
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

struct InputFile {
    _filename: PathBuf,
    content: String,
}

/// Lists files in a path and ensure at least one file exists.
fn list_files(pattern: &'static str) -> Vec<InputFile> {
    // list files
    let filenames: Vec<PathBuf> = glob(pattern)
        .expect("Listing files should not fail")
        .map(|x| x.expect("Listing files should not fail"))
        .collect();

    // ensure at least one exists
    if filenames.is_empty() {
        panic!("No files found in \"{}\"", pattern);
    }

    // read file contents
    let mut files = Vec::with_capacity(filenames.len());
    for filename in filenames {
        files.push(InputFile {
            content: fs::read_to_string(&filename).expect("Reading file should not fail"),
            _filename: filename,
        });
    }

    files
}

/// Writes generated module content to a file in the OUT_DIR.
fn write_module(file_basename: &'static str, file_content: String) {
    let out_dir = env::var_os("OUT_DIR").map(PathBuf::from).expect("Compiler should set OUT_DIR");
    let module_path = out_dir.join(file_basename);
    if let Err(e) = fs::write(&module_path, file_content) {
        panic!("Failed to write to file {module_path:?} | reason={e:?}");
    }
}
