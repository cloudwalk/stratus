/// Derive `From` implementation for [New Type](https://doc.rust-lang.org/rust-by-example/generics/new_types.html) using the wrapped type `From` implementation.
#[macro_export]
macro_rules! derive_newtype_from {
    (self = $type:ty, other = $($source:ty),+) => {
        $(
            impl From<$source> for $type {
                fn from(value: $source) -> Self {
                    Self(value.into())
                }
            }
        )+
    };
}
