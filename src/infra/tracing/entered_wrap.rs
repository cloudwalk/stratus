use std::marker::PhantomData;

use tracing::span::Entered;

/// A wrapper around `tracing::span::Entered` that is not `Send`.
///
/// This is useful for ensuring that a span is not sent to another thread,
/// specially to prevent most cases of spans being helf accross await points.
pub struct EnteredWrap<'a> {
    // we only care about its destructor
    _entered: Entered<'a>,
    _not_send: NotSendMarker,
}

impl<'a> EnteredWrap<'a> {
    pub fn new(entered: Entered<'a>) -> Self {
        Self {
            _entered: entered,
            _not_send: PhantomData,
        }
    }
}

// workaround for making a type !Send while negative trait bounds is an unstable feature
type NotSendMarker = PhantomData<*mut ()>;
