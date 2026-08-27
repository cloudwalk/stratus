use std::future::Future;

/// Converts a domain cursor value into the generic cursor-pagination behavior.
///
/// Domains implement only this codec; the paginator behavior is shared.
pub trait CursorCodec: Sized {
    type Error;

    fn encode_cursor(&self, next_index: usize) -> String;
    fn decode_cursor(cursor: &str) -> Result<(Self, usize), Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPageInfo {
    /// Page budget applied by the server: item count for an explicit `limit`
    /// request, approximate serialized bytes otherwise.
    pub limit: usize,
    pub returned: usize,
    pub total: usize,
    pub next_cursor: Option<String>,
}

/// Reducer for client-side paginated fetches.
///
/// Every page is folded into an accumulator until the paginator reports
/// no next page, then the accumulator is finalized.
pub trait PageReducer<Page> {
    type Output;
    type NextPage;

    fn reduce(&mut self, page: Page) -> anyhow::Result<Option<Self::NextPage>>;
    fn finish_after_not_found(self) -> anyhow::Result<Option<Self::Output>>;
    fn finish(self) -> anyhow::Result<Option<Self::Output>>;
}

/// Client-side fetcher that repeatedly fetches pages and reduces them into a final output.
pub struct PaginatedPageFetcher<Reducer> {
    reducer: Reducer,
}

impl<Reducer> PaginatedPageFetcher<Reducer> {
    pub fn new(reducer: Reducer) -> Self {
        Self { reducer }
    }

    pub async fn collect<Page, FetchPage, FetchFuture>(mut self, mut fetch_page: FetchPage) -> anyhow::Result<Option<Reducer::Output>>
    where
        Reducer: PageReducer<Page>,
        FetchPage: FnMut(Option<Reducer::NextPage>) -> FetchFuture,
        FetchFuture: Future<Output = anyhow::Result<Option<Page>>>,
    {
        let mut next_page = None;

        loop {
            let Some(page) = fetch_page(next_page).await? else {
                return self.reducer.finish_after_not_found();
            };

            next_page = self.reducer.reduce(page)?;
            if next_page.is_none() {
                return self.reducer.finish();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PageReducer;
    use super::PaginatedPageFetcher;

    /// Stub page carrying data and the next cursor (None = last page).
    #[derive(Clone)]
    struct StubPage {
        data: Vec<u8>,
        next_cursor: Option<String>,
    }

    /// Stub reducer that accumulates `StubPage` data into a single concatenated vec.
    struct ConcatReducer {
        pages: Vec<Vec<u8>>,
    }

    impl PageReducer<StubPage> for ConcatReducer {
        type Output = Vec<u8>;
        type NextPage = String;

        fn reduce(&mut self, page: StubPage) -> anyhow::Result<Option<String>> {
            self.pages.push(page.data);
            Ok(page.next_cursor)
        }

        fn finish_after_not_found(self) -> anyhow::Result<Option<Self::Output>> {
            if self.pages.is_empty() {
                Ok(None)
            } else {
                Err(anyhow::anyhow!("block disappeared while fetching paginated pages"))
            }
        }

        fn finish(self) -> anyhow::Result<Option<Self::Output>> {
            let merged = self.pages.into_iter().flatten().collect();
            Ok(Some(merged))
        }
    }

    #[tokio::test]
    async fn collect_happy_path_two_pages() {
        let fetcher = PaginatedPageFetcher::new(ConcatReducer { pages: Vec::new() });
        let pages = [
            StubPage {
                data: vec![1, 2, 3],
                next_cursor: Some("page-2".to_string()),
            },
            StubPage {
                data: vec![4, 5],
                next_cursor: None,
            },
        ];
        let mut call_idx = 0;
        let result = fetcher
            .collect(|_cursor: Option<String>| {
                let idx = call_idx;
                call_idx += 1;
                let page = pages.get(idx).cloned();
                async move { Ok(page) }
            })
            .await
            .expect("ok");

        let merged = result.expect("some output");
        assert_eq!(merged, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn collect_not_found_immediately_returns_none() {
        let fetcher = PaginatedPageFetcher::new(ConcatReducer { pages: Vec::new() });
        let result = fetcher.collect(|_cursor: Option<String>| async { Ok(None) }).await.expect("ok");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn collect_not_found_after_partial_errors() {
        let fetcher = PaginatedPageFetcher::new(ConcatReducer { pages: Vec::new() });
        let mut call_idx = 0;
        let result = fetcher
            .collect(|_cursor: Option<String>| {
                let idx = call_idx;
                call_idx += 1;
                async move {
                    if idx == 0 {
                        Ok(Some(StubPage {
                            data: vec![1, 2, 3],
                            next_cursor: Some("page-2".to_string()),
                        }))
                    } else {
                        Ok(None)
                    }
                }
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn collect_error_propagates() {
        let fetcher = PaginatedPageFetcher::new(ConcatReducer { pages: Vec::new() });
        let result = fetcher.collect(|_cursor: Option<String>| async { Err(anyhow::anyhow!("network error")) }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn collect_single_page_no_cursor() {
        let fetcher = PaginatedPageFetcher::new(ConcatReducer { pages: Vec::new() });
        let result = fetcher
            .collect(|_cursor: Option<String>| async {
                Ok(Some(StubPage {
                    data: vec![42],
                    next_cursor: None,
                }))
            })
            .await
            .expect("ok");
        let merged = result.expect("some output");
        assert_eq!(merged, vec![42]);
    }
}
