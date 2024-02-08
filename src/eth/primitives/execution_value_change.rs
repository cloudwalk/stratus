use crate::ext::not;

/// Changes that happened to an account value during a transaction.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct ExecutionValueChange<T>
where
    T: PartialEq,
{
    original: ValueState<T>,
    modified: ValueState<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueState<T> {
    Set(T),
    NotSet,
}

impl<T> ValueState<T> {
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    pub fn is_not_set(&self) -> bool {
        not(self.is_set())
    }

    pub fn take(self) -> Option<T> {
        if let Self::Set(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub fn take_ref(&self) -> Option<&T> {
        if let Self::Set(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

impl<T> ExecutionValueChange<T>
where
    T: PartialEq,
{
    /// Creates a new [`ExecutionValueChange`] only with original value.
    pub fn from_original(value: T) -> Self {
        Self {
            original: ValueState::Set(value),
            modified: ValueState::NotSet,
        }
    }

    /// Creates a new [`ExecutionValueChange`] only with modified value.
    pub fn from_modified(value: T) -> Self {
        Self {
            original: ValueState::NotSet,
            modified: ValueState::Set(value),
        }
    }

    /// Sets the modified value of an original value.
    pub fn set_modified(&mut self, value: T) {
        if self.original.is_not_set() {
            tracing::warn!("setting modified value without original value present.");
        }
        self.modified = ValueState::Set(value);
    }

    /// Takes the original value as reference if it is set.
    pub fn take_original_ref(&self) -> Option<&T> {
        self.original.take_ref()
    }

    /// Takes the original value if it is set.
    pub fn take_original(self) -> Option<T> {
        self.original.take()
    }

    /// Takes the modified value if it is set.
    pub fn take_modified(self) -> Option<T> {
        self.modified.take()
    }

    /// Takes the original and the modified value if they are set.
    pub fn take_both(self) -> (Option<T>, Option<T>) {
        (self.original.take(), self.modified.take())
    }

    /// Takes any value that is set giving preference to the modified value, but using the original value as fallback.
    pub fn take(self) -> Option<T> {
        self.modified.take().or_else(|| self.original.take())
    }

    /// Check if the value was modified.
    pub fn is_modified(&self) -> bool {
        self.modified.is_set() && (self.original != self.modified)
    }
}
