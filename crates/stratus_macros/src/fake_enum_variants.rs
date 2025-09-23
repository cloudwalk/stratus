use proc_macro::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::Expr;
use syn::Fields;
use syn::ItemEnum;
use syn::Lit;
use syn::Path;

struct MacroArgs {
    func_name: Path,
}

impl Parse for MacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name_value: syn::MetaNameValue = input.parse()?;
        if name_value.path.is_ident("generate") {
            let Expr::Lit(lit_expr) = name_value.value else {
                return Err(input.error("Expected a string literal for `func`"));
            };
            if let Lit::Str(lit_str) = lit_expr.lit {
                let func_name = lit_str.parse()?;
                Ok(MacroArgs { func_name })
            } else {
                Err(input.error("Expected a string literal for `func`"))
            }
        } else {
            Err(input.error("Expected `func = \"...\"`"))
        }
    }
}

pub(crate) fn derive_fake_enum_impl(input: ItemEnum) -> TokenStream {
    let attr = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("fake_enum"))
        .expect("Expected `fake_enum( generate = \"...\"`");
    let fake_enum_attr = attr.parse_args::<MacroArgs>().expect("Expected `fake_enum( generate = \"...\"`");

    let func_name = &fake_enum_attr.func_name;

    let enum_name = &input.ident;
    let variants = &input.variants;
    let mut match_arms = Vec::new();

    for variant in variants.iter() {
        let variant_name = &variant.ident;
        let variant_string = variant_name.to_string();

        let arm = match &variant.fields {
            Fields::Named(fields) =>
                if fields.named.is_empty() {
                    quote! {
                        #variant_string => #enum_name::#variant_name{},
                    }
                } else {
                    let field_assignments = fields
                        .named
                        .iter()
                        .map(|f| {
                            let field_name = &f.ident;
                            let field_type = &f.ty;
                            quote! { #field_name: #func_name::<#field_type>() }
                        })
                        .collect::<Vec<_>>();
                    quote! {
                        #variant_string => #enum_name::#variant_name{
                            #(#field_assignments),*
                        },
                    }
                },
            Fields::Unnamed(fields) =>
                if fields.unnamed.is_empty() {
                    quote! {
                        #variant_string => #enum_name::#variant_name(),
                    }
                } else {
                    let inner_type = fields.unnamed.iter().next().map(|f| &f.ty);
                    quote! {
                        #variant_string => #enum_name::#variant_name(#func_name::<#inner_type>()),
                    }
                },
            Fields::Unit => quote! {
                #variant_string => #enum_name::#variant_name,
            },
        };

        match_arms.push(arm);
    }

    let expanded = quote! {
        #[cfg(test)]
        impl FakeEnum for #enum_name {
            fn fake(arm_name: &str) -> Self {
                match arm_name {
                    #(#match_arms)*
                    _ => panic!("Warning: unknown arm '{}'", arm_name),
                }
            }
        }
    };
    TokenStream::from(expanded)
}
