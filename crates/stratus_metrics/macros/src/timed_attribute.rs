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
use syn::Stmt;
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
                    let #label_variable = ::stratus_metrics::ToMetricLabelValue::to_metric_label_value(&#parameter);
                });
            }
            LabelEntry::InputClosure(closure) => {
                let arguments = closure_arguments(closure, &parameters)?;
                let mut zero_argument_closure = closure.clone();
                zero_argument_closure.inputs.clear();
                before_body.push(quote! {
                    let #label_variable: ::stratus_metrics::MetricLabelValue = {
                        #(let #arguments = &#arguments;)*
                        (#zero_argument_closure)()
                    }
                    .into();
                });
            }
            LabelEntry::ResultExpression(expression) => {
                after_body.push(quote! {
                    let #label_variable: ::stratus_metrics::MetricLabelValue = {
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
    let timing_window = timing_window(body)?;
    let metric_function = Ident::new(&format!("inc_{}", args.metric), args.metric.span());
    let publish_body = quote! {
        #(#after_body)*
        ::stratus_metrics::#metric_function(
            __stratus_metrics_elapsed
            #(, #label_variables)*
        );
    };
    let is_async = function.sig.asyncness.is_some();
    let body = timing_window.record(is_async, &before_body, &publish_body);

    let attributes = &function.attrs;
    let visibility = &function.vis;
    let signature = &function.sig;

    Ok(quote! {
        #(#attributes)*
        #visibility #signature #body
    })
}

struct TimingWindow<'a> {
    statements: &'a [Stmt],
    start_marker: Option<usize>,
    end_marker: Option<usize>,
    duration_marker: Option<(usize, Expr)>,
}

impl TimingWindow<'_> {
    fn record(&self, is_async: bool, input_labels: &[TokenStream], publish_body: &TokenStream) -> TokenStream {
        let start_declaration = match self.start_marker {
            Some(_) => quote! { let mut __stratus_metrics_start = ::stratus_metrics::now(); },
            None => quote! { let __stratus_metrics_start = ::stratus_metrics::now(); },
        };
        let end_declaration = self.end_marker.map(|_| {
            quote! {
                ::stratus_metrics::__metrics_enabled! {
                    let mut __stratus_metrics_end = None;
                }
            }
        });
        let duration_declaration = self.duration_marker.as_ref().map(|_| {
            quote! {
                ::stratus_metrics::__metrics_enabled! {
                    let mut __stratus_metrics_duration = None;
                }
            }
        });
        let operation_body = self.statements.iter().enumerate().map(|(index, statement)| {
            if self.start_marker == Some(index) {
                quote! {
                    ::stratus_metrics::__metrics_enabled! {
                        __stratus_metrics_start = ::stratus_metrics::now();
                    }
                }
            } else if self.end_marker == Some(index) {
                quote! {
                    ::stratus_metrics::__metrics_enabled! {
                        __stratus_metrics_end = Some(::stratus_metrics::now());
                    }
                }
            } else if let Some((duration_index, duration)) = &self.duration_marker {
                if *duration_index == index {
                    quote! {
                        ::stratus_metrics::__metrics_enabled! {
                            __stratus_metrics_duration = Some(#duration);
                        }
                    }
                } else {
                    quote! { #statement }
                }
            } else {
                quote! { #statement }
            }
        });
        let operation = execute_block(quote! { #(#operation_body)* }, is_async);
        let end_resolution = match self.end_marker {
            Some(_) => quote! { __stratus_metrics_end.unwrap_or_else(::stratus_metrics::now) },
            None => quote! { ::stratus_metrics::now() },
        };
        let elapsed = if self.duration_marker.is_some() {
            quote! {
                match __stratus_metrics_duration {
                    Some(duration) => duration,
                    None => {
                        let end = #end_resolution;
                        end.duration_since(__stratus_metrics_start)
                    }
                }
            }
        } else {
            quote! {
                {
                    let end = #end_resolution;
                    end.duration_since(__stratus_metrics_start)
                }
            }
        };
        let publish_result = publish_result(publish_body);

        quote! {{
            ::stratus_metrics::__metrics_enabled! {
                #start_declaration
            }
            #(
                ::stratus_metrics::__metrics_enabled! {
                    #input_labels
                }
            )*
            #end_declaration
            #duration_declaration

            let __stratus_metrics_result = #operation;

            ::stratus_metrics::__metrics_enabled! {{
                let __stratus_metrics_elapsed = #elapsed;
                #publish_result
            }}

            __stratus_metrics_result
        }}
    }
}

fn execute_block(body: TokenStream, is_async: bool) -> TokenStream {
    match is_async {
        true => quote! { async { #body }.await },
        false => quote! { (|| { #body })() },
    }
}

fn publish_result(publish_body: &TokenStream) -> TokenStream {
    quote! {
        let __stratus_metrics_result = &__stratus_metrics_result;
        #publish_body
    }
}

/// Finds and validates the top-level timing markers in a function body.
fn timing_window(body: &syn::Block) -> syn::Result<TimingWindow<'_>> {
    let mut start_marker = None;
    let mut end_marker = None;
    let mut duration_marker = None;

    for (index, statement) in body.stmts.iter().enumerate() {
        let Stmt::Macro(statement_macro) = statement else {
            continue;
        };
        let Some(segment) = statement_macro.mac.path.segments.last() else {
            continue;
        };

        match segment.ident.to_string().as_str() {
            "timed_start" | "timed_end" => {
                if !statement_macro.mac.tokens.is_empty() {
                    return Err(syn::Error::new(
                        statement_macro.mac.tokens.span(),
                        format!("`{}!()` does not accept arguments", segment.ident),
                    ));
                }
                let marker = match segment.ident.to_string().as_str() {
                    "timed_start" => &mut start_marker,
                    _ => &mut end_marker,
                };
                if marker.replace(index).is_some() {
                    return Err(syn::Error::new(
                        statement_macro.span(),
                        format!("only one `{}!()` marker is allowed", segment.ident),
                    ));
                }
            }
            "timed_duration" => {
                let duration = syn::parse2(statement_macro.mac.tokens.clone())
                    .map_err(|_| syn::Error::new(statement_macro.mac.tokens.span(), "`timed_duration!()` requires one duration expression"))?;
                if duration_marker.replace((index, duration)).is_some() {
                    return Err(syn::Error::new(statement_macro.span(), "only one `timed_duration!()` marker is allowed"));
                }
            }
            _ => {}
        }
    }

    if let (Some(start), Some(end)) = (start_marker, end_marker) {
        if start >= end {
            return Err(syn::Error::new(body.stmts[start].span(), "`timed_start!()` must appear before `timed_end!()`"));
        }
    }

    Ok(TimingWindow {
        statements: &body.stmts,
        start_marker,
        end_marker,
        duration_marker,
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
    fn starts_before_input_labels_without_marker() {
        let expanded = expand(
            quote! { executor_inspect, labels(trace_type) },
            quote! {
                fn inspect(trace_type: String) {}
            },
        )
        .unwrap()
        .to_string();

        let timer_start = expanded.find("__stratus_metrics_start").unwrap();
        let input_label = expanded.find("to_metric_label_value").unwrap();
        assert!(timer_start < input_label);
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
    fn starts_timing_at_marker_without_duplicating_the_body() {
        let expanded = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    let _guard = acquire_guard();
                    stratus_metrics::timed_start!();
                    do_work();
                }
            },
        )
        .unwrap()
        .to_string();

        let acquire_guard = expanded.find("acquire_guard").unwrap();
        let timer_start = expanded.rfind("__stratus_metrics_start =").unwrap();
        let do_work = expanded.find("do_work").unwrap();
        assert!(acquire_guard < timer_start);
        assert!(timer_start < do_work);
        assert_eq!(expanded.matches("acquire_guard").count(), 1);
        assert_eq!(expanded.matches("do_work").count(), 1);
        assert!(!expanded.contains("cfg"));
        assert!(!expanded.contains("timed_start !"));
    }

    #[test]
    fn stops_timing_at_end_marker_without_duplicating_the_body() {
        let expanded = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    setup();
                    timed_start!();
                    timed_work();
                    timed_end!();
                    cleanup();
                }
            },
        )
        .unwrap()
        .to_string();

        let setup = expanded.find("setup").unwrap();
        let timed_work = expanded.find("timed_work").unwrap();
        let timer_end = expanded.find("__stratus_metrics_end = Some").unwrap();
        let cleanup = expanded.find("cleanup").unwrap();
        assert!(setup < timed_work);
        assert!(timed_work < timer_end);
        assert!(timer_end < cleanup);
        assert_eq!(expanded.matches("timed_work").count(), 1);
        assert_eq!(expanded.matches("cleanup").count(), 1);
        assert!(!expanded.contains("cfg"));
        assert!(!expanded.contains("timed_start !"));
        assert!(!expanded.contains("timed_end !"));
    }

    #[test]
    fn overrides_elapsed_at_duration_marker_and_keeps_clock_fallback() {
        let expanded = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    timed_start!();
                    let custom = std::time::Duration::from_millis(5);
                    timed_duration!(custom);
                    timed_end!();
                }
            },
        )
        .unwrap()
        .to_string();

        let start = expanded.rfind("__stratus_metrics_start =").unwrap();
        let duration = expanded.find("__stratus_metrics_duration = Some").unwrap();
        let end = expanded.find("__stratus_metrics_end = Some").unwrap();
        assert!(start < duration);
        assert!(duration < end);
        assert!(expanded.contains("match __stratus_metrics_duration"));
        assert!(expanded.contains("duration_since"));
        assert!(!expanded.contains("timed_duration !"));
    }

    #[test]
    fn rejects_multiple_duration_markers() {
        let error = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    timed_duration!(first);
                    timed_duration!(second);
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("only one `timed_duration!()` marker is allowed"));
    }

    #[test]
    fn rejects_missing_duration_expression() {
        let error = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    timed_duration!();
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("`timed_duration!()` requires one duration expression"));
    }

    #[test]
    fn rejects_multiple_timing_markers() {
        let error = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    timed_start!();
                    do_work();
                    timed_start!();
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("only one `timed_start!()` marker is allowed"));
    }

    #[test]
    fn rejects_end_before_start_marker() {
        let error = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    timed_end!();
                    do_work();
                    timed_start!();
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("`timed_start!()` must appear before `timed_end!()`"));
    }

    #[test]
    fn rejects_timing_marker_arguments() {
        let error = expand(
            quote! { storage_finish_pending_block },
            quote! {
                fn finish_pending_block() {
                    timed_start!(now);
                    do_work();
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("`timed_start!()` does not accept arguments"));
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
