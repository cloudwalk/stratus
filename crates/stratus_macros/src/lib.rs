use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;
use syn::Data;
use syn::DeriveInput;
use syn::Expr;
use syn::ExprLit;
use syn::Fields;
use syn::Lit;
use syn::Meta;

#[proc_macro_derive(ErrorCode, attributes(error_code))]
pub fn derive_error_code(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_error_code_impl(input).into()
}

fn derive_error_code_impl(input: DeriveInput) -> proc_macro2::TokenStream {
    let name = &input.ident;

    let Data::Enum(data_enum) = &input.data else {
        panic!("ErrorCode can only be derived for enums");
    };

    let mut match_arms = Vec::new();
    let mut reverse_match_arms = Vec::new();

    // Process each variant
    for variant in &data_enum.variants {
        let variant_name = &variant.ident;

        // Find the error_code attribute
        let error_code = variant
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("error_code"))
            .and_then(|attr| {
                if let Meta::NameValue(meta) = &attr.meta {
                    if let Expr::Lit(ExprLit { lit: Lit::Int(lit_int), .. }) = &meta.value {
                        lit_int.base10_parse::<i32>().ok()
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .expect(&format!("Missing error_code attribute for variant {}", variant_name));

        // Handle different field types
        match &variant.fields {
            Fields::Named(_) => {
                match_arms.push(quote! {
                    #name::#variant_name { .. } => #error_code
                });
            }
            Fields::Unnamed(_) => {
                match_arms.push(quote! {
                    #name::#variant_name(..) => #error_code
                });
            }
            Fields::Unit => {
                match_arms.push(quote! {
                    #name::#variant_name => #error_code
                });
            }
        }

        // Add reverse mapping
        reverse_match_arms.push(quote! {
            #error_code => stringify!(#variant_name)
        });
    }

    let expanded = quote! {
        impl ErrorCode for #name {
            fn error_code(&self) -> i32 {
                match self {
                    #(#match_arms),*
                }
            }

            fn str_repr_from_err_code(code: i32) -> &'static str {
                match code {
                    #(#reverse_match_arms),*,
                    _ => "Unknown"
                }
            }
        }
    };

    expanded
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;

    use crate::derive_error_code_impl;

    #[test]
    fn test_derive_error_code() {
        let input = TokenStream::from(quote! {
            #[derive(ErrorCode)]
            pub enum TestError {
                #[error_code = 3]
                First,
                #[error_code = 4]
                Second { field: String },
                #[error_code = 0]
                Third(u32),
            }
        });
        let input = syn::parse2(input).unwrap();

        let out = derive_error_code_impl(input);
        let expected = quote! {
            impl ErrorCode for TestError {
                fn error_code(&self) -> i32 {
                    match self {
                        TestError::First => 3i32,
                        TestError::Second { .. } => 4i32,
                        TestError::Third(..) => 0i32
                    }
                }

                fn str_repr_from_err_code(code: i32) -> &'static str {
                    match code {
                        3i32 => stringify ! (First),
                        4i32 => stringify ! (Second),
                        0i32 => stringify ! (Third),
                        _ => "Unknown"
                    }
                }
            }
        };
        assert_eq!(out.to_string(), expected.to_string());
    }

    #[test]
    #[should_panic(expected = "Missing error_code attribute")]
    fn test_missing_error_code() {
        let input = TokenStream::from(quote! {
            #[derive(ErrorCode)]
            pub enum TestError {
                First,
            }
        });
        let input = syn::parse2(input).unwrap();

        let _out = derive_error_code_impl(input);
    }

    #[test]
    #[should_panic(expected = "ErrorCode can only be derived for enums")]
    fn test_non_enum() {
        let input = TokenStream::from(quote! {
            #[derive(ErrorCode)]
            pub struct TestError {
                field: String,
            }
        });
        let input = syn::parse2(input).unwrap();
        let _out = derive_error_code_impl(input);
    }
}
