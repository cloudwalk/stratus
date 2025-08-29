// -----------------------------------------------------------------------------
// Value Change
// -----------------------------------------------------------------------------

use std::fmt::Debug;

use display_json::DebugAsJson;

use crate::ext::to_json_string;

/// Changes that happened to an account value during a transaction.
#[derive(Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct ExecutionValueChange<T>
where
    T: PartialEq + serde::Serialize,
{
    value: T,
    modified: bool,
}

impl<T> Copy for ExecutionValueChange<T> where T: Copy + PartialEq + serde::Serialize {}

impl<T> From<T> for ExecutionValueChange<T>
where
    T: PartialEq + serde::Serialize,
{
    fn from(value: T) -> Self {
        Self::from_modified(value)
    }
}

impl<T> Debug for ExecutionValueChange<T>
where
    T: PartialEq + serde::Serialize,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&to_json_string(self))
    }
}

impl<T> ExecutionValueChange<T>
where
    T: PartialEq + serde::Serialize,
{
    /// Creates a new [`ExecutionValueChange`] only with original value.
    pub fn from_original(value: T) -> Self {
        Self {
            value,
            modified: false,
        }
    }

    /// Creates a new [`ExecutionValueChange`] only with modified value.
    pub fn from_modified(value: T) -> Self {
        Self {
            value,
            modified: false,
        }
    }

    /// Sets the modified value of an original value.
    pub fn set_modified(&mut self, value: T) {
        self.value = value;
        self.modified = true;
    }

    /// Takes the original value as reference if it is set.
    pub fn take_original_ref(&self) -> Option<&T> {
        (!self.modified).then(|| &self.value)
    }

    /// Takes the modified value if it is set.
    pub fn take_modified(self) -> Option<T> {
        self.modified.then(|| self.value)
    }

    /// Takes the modified value as reference if it is set.
    pub fn take_modified_ref(&self) -> Option<&T> {
        self.modified.then(|| &self.value)
    }

    /// Takes any value that is set, giving preference to the modified value, but using the original value as fallback.
    pub fn take(self) -> T {
        self.value
    }

    /// Takes any value that is set as reference, giving preference to the modified value, but using the original value as fallback.
    pub fn take_ref(&self) -> &T {
        &self.value
    }

    /// Check if the value was modified.
    pub fn is_modified(&self) -> bool {
        self.modified
    }
}


// -----------------------------------------------------------------------------
// Value State
// -----------------------------------------------------------------------------

#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ValueState<T> {
    Set(T),
    #[default]
    NotSet,
}

impl<T> Copy for ValueState<T> where T: Copy {}

impl<T> ValueState<T> {
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    pub fn take(self) -> Option<T> {
        if let Self::Set(value) = self { Some(value) } else { None }
    }

    pub fn take_ref(&self) -> Option<&T> {
        if let Self::Set(value) = self { Some(value) } else { None }
    }
}
