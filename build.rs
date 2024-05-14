use std::collections::HashSet;
use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use glob::glob;
use nom::bytes::complete::tag;
use nom::character::complete::hex_digit1;
use nom::combinator::rest;
use nom::sequence::separated_pair;
use nom::IResult;

fn main() {
    generate_signature_maps();
}

// -----------------------------------------------------------------------------
// Solidity signatures
// -----------------------------------------------------------------------------
const SIGNATURES_DIR: &str = "static/contracts/*.signatures";

/// Generates the `signatures.rs` file containing a static PHF map with Solidity hashes and their description.
fn generate_signature_maps() {
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

    // write signatures.rs file
    write!(
        &mut create_file("signatures.rs"),
        "
        /// Mapping between 4 byte signatures and Solidity function and error signatures.
        pub static SIGNATURES_4_BYTES: phf::Map<[u8; 4], &'static str> = {};

        /// Mapping between 32 byte signatures and Solidity events signatures.
        pub static SIGNATURES_32_BYTES: phf::Map<[u8; 32], &'static str> = {};
        ",
        signatures_4_bytes.build(),
        signatures_32_bytes.build()
    )
    .expect("File writing should not fail");
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

/// Creates a file in the OUT_DIR directory with the provided name.
fn create_file(filename: &'static str) -> File {
    let mut path = PathBuf::new();
    path.push(env::var("OUT_DIR").expect("Rust compiler shoult set OUT_DIR"));
    path.push(filename);

    File::create(path).expect("File creation should not fail")
}
