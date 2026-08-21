use std::future::Future;
use std::ops::Range;

/// Generic interface exposed by pagination engines.
///
/// Kept as a trait so alternative slicing strategies (limit/offset, page number, etc.)
/// can be slotted in later without touching call sites.
pub trait Paginator {
    type PageInfo;

    fn take(&mut self, section_len: usize) -> Range<usize>;
    fn finish(self) -> Self::PageInfo;
}

/// Converts a domain cursor value into the generic cursor-pagination behavior.
///
/// Domains implement only this codec; the paginator behavior is shared.
pub trait CursorCodec: Sized {
    type Error;

    fn encode_cursor(&self, next_index: usize) -> String;
    fn decode_cursor(cursor: &str) -> Result<(Self, usize), Self::Error>;
}

/// Cursor-based paginator.
///
/// Knows only how to slice a logical stream. The [`CursorCodec`] decides how
/// cursors are encoded/decoded; the paginator reports the final page info via
/// [`CursorPageInfo`].
pub struct CursorPaginator<C> {
    codec: C,
    start: usize,
    limit: usize,
    total: usize,
    returned: usize,
    skipped: usize,
    remaining: usize,
}

impl<C> CursorPaginator<C>
where
    C: CursorCodec,
{
    pub fn new(total: usize, start: usize, limit: usize, codec: C) -> Option<Self> {
        if start > total || (start == total && total != 0) {
            return None;
        }

        Some(Self {
            codec,
            start,
            limit,
            total,
            returned: 0,
            skipped: start,
            remaining: limit,
        })
    }

    fn next_index(&self) -> Option<usize> {
        let next_index = self.start.saturating_add(self.returned);
        (next_index < self.total).then_some(next_index)
    }
}

impl<C> Paginator for CursorPaginator<C>
where
    C: CursorCodec,
{
    type PageInfo = CursorPageInfo;

    fn take(&mut self, section_len: usize) -> Range<usize> {
        // skip an entire section
        if self.skipped >= section_len {
            self.skipped -= section_len;
            return section_len..section_len;
        }

        let start = self.skipped;
        self.skipped = 0;

        let end = (start + self.remaining).min(section_len);
        let returned = end - start;

        self.returned += returned;
        self.remaining -= returned;

        start..end
    }

    fn finish(self) -> Self::PageInfo {
        let next_cursor = self.next_index().map(|next_index| self.codec.encode_cursor(next_index));
        CursorPageInfo {
            limit: self.limit,
            returned: self.returned,
            total: self.total,
            next_cursor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPageInfo {
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
    use std::ops::Range;

    use super::CursorCodec;
    use super::CursorPageInfo;
    use super::CursorPaginator;
    use super::Paginator;

    // A minimal codec for testing the paginator independently of any domain type.
    struct IndexCodec;

    impl CursorCodec for IndexCodec {
        type Error = &'static str;

        fn encode_cursor(&self, next_index: usize) -> String {
            next_index.to_string()
        }

        fn decode_cursor(cursor: &str) -> Result<(Self, usize), Self::Error> {
            Ok((IndexCodec, cursor.parse().map_err(|_| "bad cursor")?))
        }
    }

    fn paginator(total: usize, start: usize, limit: usize) -> CursorPaginator<IndexCodec> {
        CursorPaginator::new(total, start, limit, IndexCodec).expect("valid paginator")
    }

    fn take_all(mut p: CursorPaginator<IndexCodec>, sections: &[usize]) -> (Vec<Range<usize>>, CursorPageInfo) {
        let mut ranges = Vec::new();
        for &section_len in sections {
            ranges.push(p.take(section_len));
        }
        let info = p.finish();
        (ranges, info)
    }

    #[test]
    fn empty_total_is_valid() {
        let p = paginator(0, 0, 10);
        let (_, info) = take_all(p, &[]);
        assert_eq!(
            info,
            CursorPageInfo {
                limit: 10,
                returned: 0,
                total: 0,
                next_cursor: None
            }
        );
    }

    #[test]
    fn start_equal_to_total_is_invalid() {
        assert!(CursorPaginator::new(5, 5, 10, IndexCodec).is_none());
    }

    #[test]
    fn start_greater_than_total_is_invalid() {
        assert!(CursorPaginator::new(5, 6, 10, IndexCodec).is_none());
    }

    #[test]
    fn limit_smaller_than_one_section() {
        let (ranges, info) = take_all(paginator(100, 0, 3), &[10]);
        assert_eq!(ranges, vec![0..3]);
        assert_eq!(info.returned, 3);
        assert_eq!(info.next_cursor, Some("3".to_string()));
    }

    #[test]
    fn limit_spanning_multiple_sections() {
        let (ranges, info) = take_all(paginator(100, 0, 7), &[3, 3, 3]);
        // first section fully taken (3), second fully taken (3), third partial (1)
        assert_eq!(ranges, vec![0..3, 0..3, 0..1]);
        assert_eq!(info.returned, 7);
        assert_eq!(info.next_cursor, Some("7".to_string()));
    }

    #[test]
    fn skip_across_section_boundary() {
        // start=5, sections of length 3, 3, 3 -> skip 3 (section 1), skip 2 (section 2),
        // then take 1 from section 2 and the remaining 3 from section 3.
        let (ranges, info) = take_all(paginator(100, 5, 4), &[3, 3, 3]);
        assert_eq!(ranges, vec![3..3, 2..3, 0..3]);
        assert_eq!(info.returned, 4);
        assert_eq!(info.next_cursor, Some("9".to_string()));
    }

    #[test]
    fn fully_consumed_has_no_cursor() {
        let (ranges, info) = take_all(paginator(5, 0, 5), &[3, 2]);
        assert_eq!(ranges, vec![0..3, 0..2]);
        assert_eq!(info.returned, 5);
        assert_eq!(info.next_cursor, None);
    }

    #[test]
    fn limit_larger_than_total_clamps_to_total() {
        let (ranges, info) = take_all(paginator(4, 0, 100), &[2, 2]);
        assert_eq!(ranges, vec![0..2, 0..2]);
        assert_eq!(info.returned, 4);
        assert_eq!(info.next_cursor, None);
        assert_eq!(info.total, 4);
    }

    #[test]
    fn section_larger_than_remaining_returns_partial() {
        let (ranges, info) = take_all(paginator(100, 0, 2), &[10]);
        assert_eq!(ranges, vec![0..2]);
        assert_eq!(info.returned, 2);
        assert_eq!(info.next_cursor, Some("2".to_string()));
    }

    #[test]
    fn extra_sections_after_limit_are_empty() {
        let (ranges, info) = take_all(paginator(100, 0, 2), &[2, 2, 2]);
        assert_eq!(ranges, vec![0..2, 0..0, 0..0]);
        assert_eq!(info.returned, 2);
        assert_eq!(info.next_cursor, Some("2".to_string()));
    }

    // PaginatedPageFetcher::collect tests

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
