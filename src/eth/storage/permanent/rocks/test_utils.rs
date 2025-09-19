use std::env;
use std::fmt::Debug;
use std::fs;
use std::marker::PhantomData;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use serde::Deserialize;
use serde::Serialize;
use strum::VariantNames;

use crate::ext::not;
use crate::ext::type_basename;
use crate::utils::test_utils::glob_to_string_paths;

const SNAPSHOT_FOLDER: &str = "tests/fixtures/";

pub trait FakeEnum {
    fn fake(arm: &str) -> Self;
}

pub trait ToFileName {
    const FILE_NAME: &'static str;
}

#[macro_export]
macro_rules! impl_to_file_name {
    ($type:ident, $cf_name:expr) => {
        impl ToFileName for $type {
            const FILE_NAME: &'static str = $cf_name;
        }
    };
}

/// A drop bomb that guarantees that all variants of an enum have been tested.
pub struct EnumCoverageDropBombChecker<CfValue>
where
    CfValue: VariantNames,
{
    confirmations: Vec<TestRunConfirmation<CfValue>>,
}

impl<CfValue> EnumCoverageDropBombChecker<CfValue>
where
    CfValue: VariantNames,
{
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { confirmations: Vec::new() }
    }

    pub fn add(&mut self, rhs: TestRunConfirmation<CfValue>) {
        self.confirmations.push(rhs);
    }
}

impl<CfValue> Drop for EnumCoverageDropBombChecker<CfValue>
where
    CfValue: VariantNames,
{
    fn drop(&mut self) {
        // check for missing confirmations
        for variant_name in CfValue::VARIANTS {
            let found = self.confirmations.iter().find(|confirmation| confirmation.variant_name == *variant_name);

            if found.is_none() {
                panic!(
                    "TestRunDropBombChecker<{enum_typename}> panic on drop: cf {}: missing test for variant '{}' of enum {enum_typename}",
                    type_basename::<CfValue>(),
                    variant_name,
                    enum_typename = type_basename::<CfValue>(),
                );
            }
        }
    }
}

/// A confirmation that a test was run for a specific variant of an enum, used by the drop bomb.
pub struct TestRunConfirmation<CfValue> {
    variant_name: &'static str,
    _marker: PhantomData<CfValue>,
}

impl<CfValue> TestRunConfirmation<CfValue> {
    pub fn new(variant_name: &'static str) -> Self {
        Self {
            variant_name,
            _marker: PhantomData,
        }
    }
}

fn get_all_bincode_snapshots_from_folder(folder: &Path) -> anyhow::Result<Vec<String>> {
    let pattern = format!("{}/*.bincode", folder.display());
    glob_to_string_paths(pattern).context("failed to get all bincode snapshots from folder")
}

fn load_or_generate_json_fixture<CfValue>(json_parent_path: &Path, cf_name: &str, variant_name: &str) -> Result<CfValue>
where
    CfValue: for<'de> Deserialize<'de> + Serialize + fake::Dummy<fake::Faker> + bincode::Encode + bincode::Decode<()> + FakeEnum,
{
    let json_path = json_parent_path.join(format!("{variant_name}.json"));
    // Try to load existing fixture first
    if json_path.exists() {
        let json_content = fs::read_to_string(json_path.as_path()).with_context(|| format!("failed to read JSON fixture at {json_path:?}"))?;
        return serde_json::from_str(&json_content).with_context(|| format!("failed to deserialize CfValue from JSON fixture at {json_path:?}"));
    }
    // Generate fixture if it doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is set
    if env::var("DANGEROUS_UPDATE_SNAPSHOTS").is_ok() {
        let generated_fixture = CfValue::fake(variant_name);
        let json_content = serde_json::to_string_pretty(&generated_fixture).with_context(|| format!("failed to serialize generated fixture for {cf_name}"))?;

        fs::create_dir_all(json_parent_path).with_context(|| format!("failed to create directory {json_parent_path:?}"))?;
        fs::write(json_path.as_path(), &json_content).with_context(|| format!("failed to write JSON fixture to {json_path:?}"))?;

        return Ok(generated_fixture);
    }
    bail!("JSON fixture at '{json_path:?}' doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is not set");
}

pub fn verify_fixtures<CfValue>(checker: &mut EnumCoverageDropBombChecker<CfValue>, group: &str) -> anyhow::Result<()>
where
    CfValue: for<'de> Deserialize<'de>
        + Debug
        + ToFileName
        + PartialEq
        + Serialize
        + fake::Dummy<fake::Faker>
        + bincode::Encode
        + bincode::Decode<()>
        + FakeEnum
        + VariantNames,
{
    use crate::rocks_bincode_config;
    let cf_name = CfValue::FILE_NAME;
    let parent_path = Path::new(SNAPSHOT_FOLDER).join(group).join(cf_name);
    for variant_name in CfValue::VARIANTS {
        let expected: CfValue = load_or_generate_json_fixture(parent_path.as_path(), cf_name, variant_name)?;
        let expected_snapshot_path = parent_path.join(format!("{variant_name}.bincode"));
        // create snapshot if it doesn't exist
        if not(expected_snapshot_path.exists()) {
            // -> CAREFUL WHEN UPDATING SNAPSHOTS <-
            // the snapshots are supposed to prevent you from breaking the DB accidentally
            // the DB must be able to deserialize older versions, and those versions can't change
            // don't reorder variants, remove older variants or modify the data inside existing ones
            // adding a new snapshot for a new variant is safe as long as you don't mess up in the points above
            // -> CAREFUL WHEN UPDATING SNAPSHOTS <-
            if env::var("DANGEROUS_UPDATE_SNAPSHOTS").is_ok() {
                use crate::rocks_bincode_config;
                let serialized = bincode::encode_to_vec(&expected, rocks_bincode_config())?;
                fs::create_dir_all(parent_path.as_path()).with_context(|| format!("failed to create directory {parent_path:?}"))?;
                fs::write(expected_snapshot_path.as_path(), serialized)?;
            } else {
                bail!("snapshot file at '{expected_snapshot_path:?}' doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is not set");
            }
        }

        let (deserialized, _) = bincode::decode_from_slice(&fs::read(expected_snapshot_path.as_path())?, rocks_bincode_config())?;
        ensure!(
            expected == deserialized,
            "deserialized value doesn't match expected\n deserialized = {deserialized:?}\n expected = {expected:?}",
        );
        checker.add(TestRunConfirmation::new(variant_name));
    }
    let snapshots = get_all_bincode_snapshots_from_folder(parent_path.as_path())?;
    if snapshots.len() != CfValue::VARIANTS.len() {
        bail!("expected {} snapshots, found {}: {snapshots:?}", CfValue::VARIANTS.len(), snapshots.len());
    };

    Ok(())
}
