//! Derives `apply_cli_overrides` for config structs.
//!
//! Config structs are dual-natured: they are deserialized from the TOML config file and
//! also parsed by clap for CLI arguments. This derive generates the method that merges
//! the two layers: values from arguments explicitly provided in the command line
//! override the values loaded from the config file.
//!
//! The method is generated from the struct fields alone, so a new field is covered
//! automatically:
//!
//! - Plain fields are copied from the CLI struct when their argument id (the field name)
//!   was explicitly provided in the command line.
//! - Fields flattened for CLI parsing (`#[clap(flatten)]`) recurse into the child struct,
//!   because their arguments belong to the child. Optional flattened sections are merged
//!   when both layers have them, or taken from the CLI when only the CLI has them.
//! - Fields skipped by serde (`#[serde(skip)]`) are not part of the config file, so their
//!   only possible source is the CLI and the CLI value is always applied.
//! - `#[cfg(...)]` attributes on fields are propagated to the generated code.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::DeriveInput;

/// Expands the `CliOverrides` derive.
pub fn expand(input: TokenStream) -> TokenStream {
    match expand_impl(syn::parse_macro_input!(input as DeriveInput)) {
        Ok(expanded) => expanded.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates the merge method for a named-field struct.
fn expand_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let struct_name = &input.ident;
    let fields = named_fields(&input)?;
    let statements: Vec<TokenStream2> = fields.named.iter().filter_map(merge_statement).collect();
    Ok(method_impl(struct_name, statements))
}

/// Returns the named fields of the derive input, rejecting other data kinds.
fn named_fields(input: &DeriveInput) -> syn::Result<&syn::FieldsNamed> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "CliOverrides can only be derived for structs"));
    };
    let syn::Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "CliOverrides can only be derived for structs with named fields",
        ));
    };
    Ok(fields)
}

/// Generates the merge statement for one field, or `None` for unnamed fields.
fn merge_statement(field: &syn::Field) -> Option<TokenStream2> {
    let field_name = field.ident.as_ref()?;
    let cfg_attributes = cfg_attributes(field);

    if has_attribute(&field.attrs, "serde", "skip") {
        return Some(cli_only_statement(field_name, &cfg_attributes));
    }
    if is_flattened(&field.attrs) {
        return Some(flattened_statement(field, field_name, &cfg_attributes));
    }
    Some(plain_statement(field_name, &cfg_attributes))
}

/// Returns the `#[cfg]` attributes that gate the field, so the generated code follows them.
fn cfg_attributes(field: &syn::Field) -> Vec<&syn::Attribute> {
    field.attrs.iter().filter(|attr| attr.path().is_ident("cfg")).collect()
}

/// Fields skipped by serde never come from the config file, so the CLI is their only source.
fn cli_only_statement(field_name: &syn::Ident, cfg_attributes: &[&syn::Attribute]) -> TokenStream2 {
    quote! {
        #(#cfg_attributes)*
        self.#field_name = ::std::clone::Clone::clone(&cli.#field_name);
    }
}

/// Flattened sections merge recursively, so the child's arguments override the child's values.
fn flattened_statement(field: &syn::Field, field_name: &syn::Ident, cfg_attributes: &[&syn::Attribute]) -> TokenStream2 {
    if option_inner(&field.ty).is_some() {
        optional_section_statement(field_name, cfg_attributes)
    } else {
        quote! {
            #(#cfg_attributes)*
            self.#field_name.apply_cli_overrides(&cli.#field_name, explicit);
        }
    }
}

/// Optional sections merge when both layers have them, otherwise the CLI's value is taken.
fn optional_section_statement(field_name: &syn::Ident, cfg_attributes: &[&syn::Attribute]) -> TokenStream2 {
    quote! {
        #(#cfg_attributes)*
        match (&mut self.#field_name, &cli.#field_name) {
            (::std::option::Option::Some(file_section), ::std::option::Option::Some(cli_section)) => {
                file_section.apply_cli_overrides(cli_section, explicit);
            }
            (::std::option::Option::None, ::std::option::Option::Some(cli_section)) => {
                self.#field_name = ::std::option::Option::Some(::std::clone::Clone::clone(cli_section));
            }
            _ => {}
        }
    }
}

/// Plain fields override when the argument id was explicitly provided in the command line.
fn plain_statement(field_name: &syn::Ident, cfg_attributes: &[&syn::Attribute]) -> TokenStream2 {
    let field_id = field_name.to_string();
    quote! {
        #(#cfg_attributes)*
        if explicit.contains(#field_id) {
            self.#field_name = ::std::clone::Clone::clone(&cli.#field_name);
        }
    }
}

/// Wraps the merge statements in the `apply_cli_overrides` method.
fn method_impl(struct_name: &syn::Ident, statements: Vec<TokenStream2>) -> TokenStream2 {
    // structs without overridable fields still need the method so parents can recurse into them
    let (lint_allow, body) = if statements.is_empty() {
        (quote! { #[allow(clippy::unused_self)] }, quote! {})
    } else {
        (quote! {}, quote! { #(#statements)* })
    };

    quote! {
        #[automatically_derived]
        impl #struct_name {
            #lint_allow
            pub(crate) fn apply_cli_overrides(&mut self, cli: &Self, explicit: &::std::collections::HashSet<::std::string::String>) {
                #body
            }
        }
    }
}

/// Checks whether the attributes contain `#[<tool>(...)]` with the given bare marker, e.g. `#[clap(flatten)]`.
fn has_attribute(attrs: &[syn::Attribute], tool: &str, marker: &str) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident(tool)
            && attr
                .parse_args_with(syn::punctuated::Punctuated::<syn::Ident, syn::Token![,]>::parse_terminated)
                .is_ok_and(|markers| markers.iter().any(|ident| ident == marker))
    })
}

/// Checks whether the field is flattened into its parent for CLI parsing, e.g. `#[clap(flatten)]`.
fn is_flattened(attrs: &[syn::Attribute]) -> bool {
    ["clap", "arg", "command"].iter().any(|tool| has_attribute(attrs, tool, "flatten"))
}

/// Returns the inner type when the type is an `Option<T>`.
fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(type_path) = ty else { return None };
    if type_path.qself.is_some() {
        return None;
    }
    if type_path.path.segments.len() != 1 {
        return None;
    }
    let segment = &type_path.path.segments[0];
    if segment.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }
    let syn::GenericArgument::Type(inner) = &arguments.args[0] else { return None };
    Some(inner)
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;

    use crate::cli_overrides::expand_impl;

    fn derive(input: TokenStream) -> String {
        let input = syn::parse2(input).unwrap();
        expand_impl(input).unwrap().to_string()
    }

    #[test]
    fn test_plain_fields() {
        let output = derive(quote! {
            #[derive(CliOverrides)]
            pub struct ExampleConfig {
                #[arg(long = "value")]
                pub value: u64,

                #[arg(long = "flag")]
                pub flag: bool,
            }
        });
        let expected = quote! {
            #[automatically_derived]
            impl ExampleConfig {
                pub(crate) fn apply_cli_overrides(&mut self, cli: &Self, explicit: &::std::collections::HashSet<::std::string::String>) {
                    if explicit.contains("value") {
                        self.value = ::std::clone::Clone::clone(&cli.value);
                    }
                    if explicit.contains("flag") {
                        self.flag = ::std::clone::Clone::clone(&cli.flag);
                    }
                }
            }
        };
        assert_eq!(output, expected.to_string());
    }

    #[test]
    fn test_flattened_fields() {
        let output = derive(quote! {
            #[derive(CliOverrides)]
            pub struct ParentConfig {
                #[clap(flatten)]
                pub child: ChildConfig,

                #[clap(flatten)]
                pub optional: Option<ChildConfig>,
            }
        });
        let expected = quote! {
            #[automatically_derived]
            impl ParentConfig {
                pub(crate) fn apply_cli_overrides(&mut self, cli: &Self, explicit: &::std::collections::HashSet<::std::string::String>) {
                    self.child.apply_cli_overrides(&cli.child, explicit);
                    match (&mut self.optional, &cli.optional) {
                        (::std::option::Option::Some(file_section), ::std::option::Option::Some(cli_section)) => {
                            file_section.apply_cli_overrides(cli_section, explicit);
                        }
                        (::std::option::Option::None, ::std::option::Option::Some(cli_section)) => {
                            self.optional = ::std::option::Option::Some(::std::clone::Clone::clone(cli_section));
                        }
                        _ => {}
                    }
                }
            }
        };
        assert_eq!(output, expected.to_string());
    }

    #[test]
    fn test_skipped_and_cfg_fields() {
        let output = derive(quote! {
            #[derive(CliOverrides)]
            pub struct ExampleConfig {
                #[serde(skip)]
                pub skipped: bool,

                #[clap(flatten)]
                #[cfg(feature = "dev")]
                pub child: ChildConfig,
            }
        });
        let expected = quote! {
            #[automatically_derived]
            impl ExampleConfig {
                pub(crate) fn apply_cli_overrides(&mut self, cli: &Self, explicit: &::std::collections::HashSet<::std::string::String>) {
                    self.skipped = ::std::clone::Clone::clone(&cli.skipped);
                    #[cfg(feature = "dev")]
                    self.child.apply_cli_overrides(&cli.child, explicit);
                }
            }
        };
        assert_eq!(output, expected.to_string());
    }

    #[test]
    fn test_no_overridable_fields() {
        let output = derive(quote! {
            #[derive(CliOverrides)]
            pub struct EmptyConfig {
                #[serde(skip)]
                pub skipped: bool,
            }
        });
        let expected = quote! {
            #[automatically_derived]
            impl EmptyConfig {
                pub(crate) fn apply_cli_overrides(&mut self, cli: &Self, explicit: &::std::collections::HashSet<::std::string::String>) {
                    self.skipped = ::std::clone::Clone::clone(&cli.skipped);
                }
            }
        };
        assert_eq!(output, expected.to_string());
    }

    #[test]
    fn test_unsupported_input() {
        use syn::DeriveInput;

        let input = syn::parse2::<DeriveInput>(quote! {
            #[derive(CliOverrides)]
            pub enum ExampleEnum {
                Variant,
            }
        })
        .unwrap();
        let error = expand_impl(input).unwrap_err();
        assert_eq!(error.to_string(), "CliOverrides can only be derived for structs");
    }
}
