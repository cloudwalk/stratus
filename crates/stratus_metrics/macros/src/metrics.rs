use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::Ident;
use syn::LitStr;
use syn::Token;

syn::custom_keyword!(group);

/// Input of the `metrics!` macro.
struct MetricsInput {
    group: Ident,
    entries: Vec<MetricEntry>,
}

/// A single metric definition.
struct MetricEntry {
    description: LitStr,
    kind: MetricKind,
    name: Ident,
    labels: Vec<Ident>,
}

/// The kind of a metric, which defines how it is recorded.
enum MetricKind {
    Counter,
    HistogramCounter,
    HistogramDuration,
    Gauge,
}

impl MetricKind {
    fn parse(ident: &Ident) -> syn::Result<Self> {
        match ident.to_string().as_str() {
            "counter" => Ok(Self::Counter),
            "histogram_counter" => Ok(Self::HistogramCounter),
            "histogram_duration" => Ok(Self::HistogramDuration),
            "gauge" => Ok(Self::Gauge),
            _ => Err(syn::Error::new(
                ident.span(),
                "unknown metric kind; expected `counter`, `histogram_counter`, `histogram_duration`, or `gauge`",
            )),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::HistogramCounter => "histogram_counter",
            Self::HistogramDuration => "histogram_duration",
            Self::Gauge => "gauge",
        }
    }
}

impl Parse for MetricsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<group>()?;
        input.parse::<Token![:]>()?;
        let group = input.parse::<Ident>()?;
        input.parse::<Token![,]>()?;

        let mut entries = Vec::new();
        while !input.is_empty() {
            entries.push(input.parse()?);
            if input.is_empty() {
                break; // allow trailing comma
            }
            input.parse::<Token![,]>()?;
        }

        Ok(Self { group, entries })
    }
}

impl Parse for MetricEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let description = input.parse::<LitStr>()?;
        let kind = MetricKind::parse(&input.parse::<Ident>()?)?;
        let name = input.parse::<Ident>()?;

        let labels;
        syn::braced!(labels in input);
        let mut label_list = Vec::new();
        while !labels.is_empty() {
            label_list.push(labels.parse::<Ident>()?);
            if labels.is_empty() {
                break; // allow trailing comma
            }
            labels.parse::<Token![,]>()?;
        }

        Ok(Self {
            description,
            kind,
            name,
            labels: label_list,
        })
    }
}

impl MetricEntry {
    /// Full metric name, prefixed to avoid collisions with other applications.
    fn metric_name(&self) -> String {
        format!("stratus_{}", self.name)
    }

    /// Function parameters that convert each label value.
    fn label_parameters(&self) -> Vec<TokenStream> {
        self.labels.iter().map(|label| quote! { #label: impl Into<crate::MetricLabelValue> }).collect()
    }

    /// Label entries for the recorded metric.
    fn label_values(&self) -> Vec<TokenStream> {
        self.labels.iter().map(|label| quote! { (stringify!(#label), #label.into()) }).collect()
    }

    /// Recording functions for the metric, according to its kind.
    fn functions(&self, group: &Ident) -> TokenStream {
        let metric = self.metric_name();
        let group_label = group.to_string();
        let parameters = self.label_parameters();
        let label_values = self.label_values();
        let name = &self.name;

        let labels = quote! {
            crate::into_labels(vec![
                ("group", #group_label.into()),
                ("node_mode", crate::node_mode()),
                #(#label_values),*
            ])
        };

        match self.kind {
            MetricKind::Counter => {
                let inc_n = format_ident!("inc_n_{}", name);
                let inc = format_ident!("inc_{}", name);
                let doc_inc_n = format!("Add n to `{name}` counter.");
                let doc_inc = format!("Add 1 to `{name}` counter.");
                quote! {
                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc_inc_n]
                    pub fn #inc_n(n: u64, #(#parameters),*) {
                        let labels = #labels;
                        let counter = ::metrics::counter!(#metric, labels);
                        counter.increment(n);
                    }

                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc_inc]
                    pub fn #inc(#(#parameters),*) {
                        let labels = #labels;
                        let counter = ::metrics::counter!(#metric, labels);
                        counter.increment(1);
                    }
                }
            }
            MetricKind::HistogramCounter => {
                let inc = format_ident!("inc_{}", name);
                let doc = format!("Add N to `{name}` histogram.");
                quote! {
                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc]
                    pub fn #inc(n: usize, #(#parameters),*) {
                        let labels = #labels;
                        let hist = ::metrics::histogram!(#metric, labels);
                        hist.record(n as f64);
                    }
                }
            }
            MetricKind::HistogramDuration => {
                let inc = format_ident!("inc_{}", name);
                let doc = format!("Add operation duration to `{name}` histogram.");
                quote! {
                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc]
                    pub fn #inc(duration: std::time::Duration, #(#parameters),*) {
                        let labels = #labels;
                        let hist = ::metrics::histogram!(#metric, labels);
                        hist.record(duration);
                    }
                }
            }
            MetricKind::Gauge => {
                let set = format_ident!("set_{}", name);
                let inc = format_ident!("inc_{}", name);
                let dec = format_ident!("dec_{}", name);
                let doc_set = format!("Set `{name}` gauge.");
                let doc_inc = format!("Increment `{name}` gauge by `n` atomically.");
                let doc_dec = format!("Decrement `{name}` gauge by `n` atomically.");
                quote! {
                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc_set]
                    pub fn #set(n: u64, #(#parameters),*) {
                        let labels = #labels;
                        let gauge = ::metrics::gauge!(#metric, labels);
                        gauge.set(n as f64);
                    }

                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc_inc]
                    pub fn #inc(n: u64, #(#parameters),*) {
                        let labels = #labels;
                        let gauge = ::metrics::gauge!(#metric, labels);
                        gauge.increment(n as f64);
                    }

                    #[allow(clippy::too_many_arguments)]
                    #[doc = #doc_dec]
                    pub fn #dec(n: u64, #(#parameters),*) {
                        let labels = #labels;
                        let gauge = ::metrics::gauge!(#metric, labels);
                        gauge.decrement(n as f64);
                    }
                }
            }
        }
    }
}

pub(super) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: MetricsInput = syn::parse2(input)?;
    let group = &input.group;

    let constants = input.entries.iter().map(|entry| {
        let constant = format_ident!("METRIC_{}", entry.name.to_string().to_uppercase());
        let name = entry.metric_name();
        quote! { pub const #constant: &str = #name; }
    });

    let group_function = format_ident!("metrics_for_{}", group);
    let definitions = input.entries.iter().map(|entry| {
        let kind = entry.kind.as_str();
        let name = entry.metric_name();
        let description = &entry.description;
        quote! {
            crate::Metric {
                kind: #kind,
                name: #name,
                description: stringify!(#description),
            }
        }
    });

    let functions = input.entries.iter().map(|entry| entry.functions(group));

    Ok(quote! {
        #(#constants)*

        pub fn #group_function() -> Vec<crate::Metric> {
            vec![
                #(#definitions),*
            ]
        }

        #(#functions)*
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::expand;

    #[test]
    fn expands_all_metric_kinds() {
        let expanded = expand(quote! {
            group: test_group,

            "Number of things."
            counter things{kind},

            "Size of things."
            histogram_counter sizes{},

            "Duration of things."
            histogram_duration timings{scope},

            "Level of things."
            gauge levels{pool, depth},
        })
        .unwrap()
        .to_string();

        assert!(expanded.contains("METRIC_THINGS"));
        assert!(expanded.contains("stratus_things"));
        assert!(expanded.contains("metrics_for_test_group"));
        assert!(expanded.contains("inc_n_things"));
        assert!(expanded.contains("inc_things"));
        assert!(expanded.contains("inc_sizes"));
        assert!(expanded.contains("inc_timings"));
        assert!(expanded.contains("set_levels"));
        assert!(expanded.contains("inc_levels"));
        assert!(expanded.contains("dec_levels"));
        assert!(expanded.contains("crate :: node_mode"));
        assert!(expanded.contains("kind"));
    }

    #[test]
    fn rejects_unknown_metric_kind() {
        let error = expand(quote! {
            group: test_group,

            "Number of things."
            metric things{},
        })
        .unwrap_err();

        assert!(error.to_string().contains("unknown metric kind"));
    }

    #[test]
    fn rejects_missing_group() {
        let error = expand(quote! {
            "Number of things."
            counter things{},
        })
        .unwrap_err();

        assert!(error.to_string().contains("expected `group`"));
    }
}
