use std::future::Future;
use std::marker::PhantomData;
use std::ops::Range;

// TODO: test module

/// Immutable result produced by the pagination engine after a page is built.
pub struct PageOutcome<NextPage> {
    pub limit: usize,
    pub returned: usize,
    pub total: usize,
    pub next_page: Option<NextPage>,
}

/// Generic interface exposed by pagination engines.
pub trait Paginator {
    type Error;
    type NextPage;
    type PageInfo;

    fn take(&mut self, section_len: usize) -> Range<usize>;
    fn finish(self) -> Self::PageInfo;
}

/// Defines how a paginator validates requests and converts engine output into
/// a domain-specific page-info response.
pub trait PaginationPolicy {
    type Error;
    type NextPage;
    type PageInfo;

    fn invalid_start_error() -> Self::Error;
    fn next_page(&self, next_index: usize) -> Self::NextPage;
    fn page_info(&self, outcome: PageOutcome<Self::NextPage>) -> Self::PageInfo;
}

/// Generic pagination engine.
///
/// The engine only knows how to slice a logical stream. The policy decides how
/// invalid requests are reported and how the final page information is encoded
/// (cursor, limit/offset, page number, etc.).
pub struct PaginatorEngine<Policy> {
    policy: Policy,
    start: usize,
    limit: usize,
    total: usize,
    returned: usize,
    skipped: usize,
    remaining: usize,
}

impl<Policy> PaginatorEngine<Policy>
where
    Policy: PaginationPolicy,
{
    pub fn new(total: usize, start: usize, limit: usize, policy: Policy) -> Result<Self, Policy::Error> {
        if start > total || (start == total && total != 0) {
            return Err(Policy::invalid_start_error());
        }

        Ok(Self {
            policy,
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

impl<Policy> Paginator for PaginatorEngine<Policy>
where
    Policy: PaginationPolicy,
{
    type Error = Policy::Error;
    type NextPage = Policy::NextPage;
    type PageInfo = Policy::PageInfo;

    fn take(&mut self, section_len: usize) -> Range<usize> {
        let range_start = self.skipped.min(section_len);

        let range_end = if self.skipped < section_len {
            range_start.saturating_add(self.remaining).min(section_len)
        } else {
            range_start
        };

        let returned = range_end.saturating_sub(range_start);

        self.returned += returned;
        self.remaining = self.remaining.saturating_sub(returned);
        self.skipped = self.skipped.saturating_sub(section_len);

        range_start..range_end
    }

    fn finish(self) -> Self::PageInfo {
        let next_page = self.next_index().map(|next_index| self.policy.next_page(next_index));
        self.policy.page_info(PageOutcome {
            limit: self.limit,
            returned: self.returned,
            total: self.total,
            next_page,
        })
    }
}

/// Converts a domain cursor value into the generic cursor-pagination policy.
pub trait CursorCodec: Sized {
    type Error;

    fn invalid_start_error() -> Self::Error;
    fn encode_cursor(&self, next_index: usize) -> String;
    fn decode_cursor(cursor: &str) -> Result<(Self, usize), Self::Error>;
}

/// Reusable cursor pagination policy. Domains provide only the cursor payload
/// codec; the paginator behavior remains shared.
pub struct CursorPaginationPolicy<Cursor> {
    cursor: Cursor,
}

impl<Cursor> CursorPaginationPolicy<Cursor> {
    pub fn new(cursor: Cursor) -> Self {
        Self { cursor }
    }
}

impl<Cursor> PaginationPolicy for CursorPaginationPolicy<Cursor>
where
    Cursor: CursorCodec,
{
    type Error = Cursor::Error;
    type NextPage = String;
    type PageInfo = CursorPageInfo;

    fn invalid_start_error() -> Self::Error {
        Cursor::invalid_start_error()
    }

    fn next_page(&self, next_index: usize) -> Self::NextPage {
        self.cursor.encode_cursor(next_index)
    }

    fn page_info(&self, outcome: PageOutcome<Self::NextPage>) -> Self::PageInfo {
        CursorPageInfo {
            limit: outcome.limit,
            returned: outcome.returned,
            total: outcome.total,
            next_cursor: outcome.next_page,
        }
    }
}

/// Generic cursor paginator for every domain-specific cursor payload.
pub type CursorPaginator<Cursor> = PaginatorEngine<CursorPaginationPolicy<Cursor>>;

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
    type Paginator: Paginator;

    fn reduce(&mut self, page: Page) -> anyhow::Result<Option<<Self::Paginator as Paginator>::NextPage>>;
    fn finish_after_not_found(self) -> anyhow::Result<Option<Self::Output>>;
    fn finish(self) -> anyhow::Result<Option<Self::Output>>;
}

/// Client-side fetcher that repeatedly fetches pages and reduces them into a final output.
pub struct PaginatedPageFetcher<Reducer> {
    reducer: Reducer,
    _paginator: PhantomData<Reducer>,
}

impl<Reducer> PaginatedPageFetcher<Reducer> {
    pub fn new(reducer: Reducer) -> Self {
        Self {
            reducer,
            _paginator: PhantomData,
        }
    }

    pub async fn collect<Page, FetchPage, FetchFuture>(mut self, mut fetch_page: FetchPage) -> anyhow::Result<Option<Reducer::Output>>
    where
        Reducer: PageReducer<Page>,
        FetchPage: FnMut(Option<<Reducer::Paginator as Paginator>::NextPage>) -> FetchFuture,
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
