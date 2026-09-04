use proc_macro2::Span;
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::Expr;
use syn::ExprClosure;
use syn::FnArg;
use syn::Ident;
use syn::ItemFn;
use syn::Pat;
use syn::Token;

syn::custom_keyword!(labels);

/// Arguments accepted by `#[timed(...)]`.
struct MetricsArgs {
    metric: Ident,
    labels: Vec<LabelEntry>,
}

impl Parse for MetricsArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let metric = input.parse()?;
        let mut parsed_labels = Vec::new();

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;

            if !input.is_empty() {
                input.parse::<labels>()?;
                let content;
                syn::parenthesized!(content in input);

                while !content.is_empty() {
                    parsed_labels.push(content.parse()?);
                    if content.peek(Token![,]) {
                        content.parse::<Token![,]>()?;
                    } else if !content.is_empty() {
                        return Err(content.error("expected `,` between labels"));
                    }
                }
            }
        }

        if !input.is_empty() {
            return Err(input.error("unexpected metrics argument; expected `labels(...)`"));
        }

        Ok(Self { metric, labels: parsed_labels })
    }
}

/// Label values are positional. Optional names improve readability but do not
/// affect the generated call.
enum LabelEntry {
    /// A bare function parameter, converted before executing the function body.
    Parameter(Ident),
    /// A closure over function parameters, evaluated before the function body.
    InputClosure(ExprClosure),
    /// An expression evaluated after the function body, with `result` in scope.
    ResultExpression(Expr),
}

impl Parse for LabelEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Named label: `label_name = expression`. The name documents the
        // positional argument and the expression determines evaluation timing.
        if input.peek(Ident) && input.peek2(Token![=]) {
            input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            return Ok(Self::from_expression(input.parse()?));
        }

        let expression = input.parse()?;
        if let Expr::Path(path) = &expression {
            if let Some(parameter) = path.path.get_ident() {
                return Ok(Self::Parameter(parameter.clone()));
            }
        }
        Ok(Self::from_expression(expression))
    }
}

impl LabelEntry {
    fn from_expression(expression: Expr) -> Self {
        match expression {
            Expr::Closure(closure) => Self::InputClosure(closure),
            expression => Self::ResultExpression(expression),
        }
    }
}

pub(super) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args: MetricsArgs = syn::parse2(args)?;
    let function: ItemFn = syn::parse2(item).map_err(|_| syn::Error::new(Span::call_site(), "`#[timed]` can only be applied to functions with a body"))?;

    if let Some(constness) = function.sig.constness {
        return Err(syn::Error::new(constness.span(), "`#[timed]` cannot be applied to a `const fn`"));
    }
    if let Some(unsafety) = function.sig.unsafety {
        return Err(syn::Error::new(unsafety.span(), "`#[timed]` cannot be applied to an `unsafe fn`"));
    }

    let parameters = function
        .sig
        .inputs
        .iter()
        .filter_map(|argument| match argument {
            FnArg::Typed(argument) => match argument.pat.as_ref() {
                Pat::Ident(parameter) => Some(parameter.ident.clone()),
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect::<Vec<_>>();

    let mut before_body = Vec::new();
    let mut after_body = Vec::new();
    let mut label_variables = Vec::new();

    for (index, label) in args.labels.iter().enumerate() {
        let label_variable = Ident::new(&format!("__stratus_metrics_label_{index}"), Span::call_site());
        label_variables.push(label_variable.clone());

        match label {
            LabelEntry::Parameter(parameter) => {
                ensure_parameter_exists(parameter, &parameters)?;
                before_body.push(quote! {
                    let #label_variable = crate::infra::metrics::ToMetricLabelValue::to_metric_label_value(&#parameter);
                });
            }
            LabelEntry::InputClosure(closure) => {
                let arguments = closure_arguments(closure, &parameters)?;
                let mut zero_argument_closure = closure.clone();
                zero_argument_closure.inputs.clear();
                before_body.push(quote! {
                    let #label_variable: crate::infra::metrics::MetricLabelValue = {
                        #(let #arguments = &#arguments;)*
                        (#zero_argument_closure)()
                    }
                    .into();
                });
            }
            LabelEntry::ResultExpression(expression) => {
                after_body.push(quote! {
                    let #label_variable: crate::infra::metrics::MetricLabelValue = {
                        #[allow(unused_variables)]
                        let result = __stratus_metrics_result;
                        #expression
                    }
                    .into();
                });
            }
        }
    }

    let body = &function.block;
    let metric_function = Ident::new(&format!("inc_{}", args.metric), args.metric.span());
    let publish = quote! {
        |__stratus_metrics_elapsed, __stratus_metrics_result| {
            #(#after_body)*
            crate::infra::metrics::#metric_function(
                __stratus_metrics_elapsed
                #(, #label_variables)*
            );
        }
    };
    let record = if function.sig.asyncness.is_some() {
        quote! {
            crate::infra::metrics::record_async(|| async #body, #publish).await
        }
    } else {
        quote! {
            crate::infra::metrics::record(|| #body, #publish)
        }
    };

    let attributes = &function.attrs;
    let visibility = &function.vis;
    let signature = &function.sig;

    Ok(quote! {
        #(#attributes)*
        #visibility #signature {
            #[cfg(feature = "metrics")]
            {
                #(#before_body)*
                #record
            }

            #[cfg(not(feature = "metrics"))]
            #body
        }
    })
}

fn ensure_parameter_exists(parameter: &Ident, parameters: &[Ident]) -> syn::Result<()> {
    if parameters.iter().any(|candidate| candidate == parameter) {
        return Ok(());
    }

    Err(syn::Error::new(
        parameter.span(),
        format!("`{parameter}` is not a plain function parameter; use `{parameter} = <expression>` for a derived label"),
    ))
}

fn closure_arguments(closure: &ExprClosure, parameters: &[Ident]) -> syn::Result<Vec<Ident>> {
    if let Some(asyncness) = closure.asyncness {
        return Err(syn::Error::new(asyncness.span(), "metrics label closures cannot be async"));
    }

    closure
        .inputs
        .iter()
        .map(|input| match input {
            Pat::Ident(parameter)
                if parameter.attrs.is_empty() && parameter.by_ref.is_none() && parameter.mutability.is_none() && parameter.subpat.is_none() =>
            {
                ensure_parameter_exists(&parameter.ident, parameters)?;
                Ok(parameter.ident.clone())
            }
            _ => Err(syn::Error::new(
                input.span(),
                "metrics label closure parameters must be plain identifiers matching function parameters",
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::expand;

    #[test]
    fn expands_sync_function_with_all_label_sources() {
        let expanded = expand(
            quote! {
                storage_read_block,
                labels(storage = "permanent", success = result.is_ok(), filter = |filter| filter.to_string())
            },
            quote! {
                fn read_block(filter: u64) -> Result<u64, String> {
                    Ok(filter)
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(expanded.contains("inc_storage_read_block"));
        assert!(!expanded.contains("async"));
        assert!(expanded.contains("& filter"));
        assert!(expanded.contains("result . is_ok"));
    }

    #[test]
    fn expands_async_function() {
        let expanded = expand(
            quote! { storage_finish_pending_block },
            quote! {
                async fn finish_pending_block() {
                    do_work().await;
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(expanded.contains("async"));
        assert!(expanded.contains(". await"));
        assert!(expanded.contains("inc_storage_finish_pending_block"));
    }

    #[test]
    fn rejects_unknown_parameter() {
        let error = expand(
            quote! { executor_inspect, labels(trace_type) },
            quote! {
                fn inspect(kind: String) {}
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("`trace_type` is not a plain function parameter"));
    }
}
