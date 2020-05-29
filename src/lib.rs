//! Allows to easily generate streams with async/await.

#[cfg(test)]
mod tests;

use futures::{channel::mpsc, sink::SinkExt, stream, stream::StreamExt, Future, Stream};

/// A handle that's used to produce (yield) values of the stream.
pub struct Yielder<T>(mpsc::Sender<T>);

impl<T> Yielder<T> {
    /// Yields `value` from the stream created with `generate_stream`
    /// or `generate_try_stream`.
    ///
    /// Note that `value` will be dropped if the corresponding stream is dropped before it could
    /// produce that value.
    pub async fn send(&mut self, value: T) {
        let _ = self.0.send(value).await;
    }
}

impl<T> Clone for Yielder<T> {
    fn clone(&self) -> Self {
        Yielder(self.0.clone())
    }
}

/// Creates a stream from `generator`.
///
/// `generator` will receive a yielder object as an argument and should return a future
/// (usually an async block) that will produce the stream's values using the yielder.
/// If the future finishes, the stream will end after producing all yielded values. If the
/// future never finishes, the stream will also never finish.
/// ```
/// use futures::{stream::StreamExt, Stream};
/// use stream_generator::generate_stream;
///
/// fn my_stream(start: u32) -> impl Stream<Item=u32> {
///     generate_stream(move |mut y| async move {
///         for i in start.. {
///             y.send(i).await;
///             if i == 45 {
///                 break;
///             }
///         }
///     })
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let values: Vec<_> = my_stream(42).collect().await;
///     assert_eq!(values, vec![42, 43, 44, 45]);
/// }
/// ```
pub fn generate_stream<T, F, R>(generator: F) -> impl Stream<Item = T>
where
    F: FnOnce(Yielder<T>) -> R,
    R: Future<Output = ()>,
{
    let (tx, rx) = mpsc::channel(0);

    let fake_stream = stream::once(generator(Yielder(tx))).filter_map(|_| async { None });
    stream::select(fake_stream, rx)
}

/// Creates a stream of `Result`s from `generator`.
///
/// `generator` will receive a yielder object as an argument and should return a future
/// (usually an async block) that will produce the stream's values using the yielder.
/// If the future finishes with an `Ok`, the stream will end after producing all yielded values.
/// If the future finishes with an `Err`, the stream will end after producing all yielded values
/// and then producing an `Err` returned by the future.
/// If the future never finishes, the stream will also never finish.
///
/// ```
/// use futures::stream::StreamExt;
/// use stream_generator::generate_try_stream;
///
/// async fn failing() -> Result<(), &'static str> {
///     Err("world")
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let s = generate_try_stream(|mut y| async move {
///         y.send(Ok("hello")).await;
///         failing().await?;
///         unreachable!();
///     });
///
///     let values: Vec<_> = s.collect().await;
///     assert_eq!(values, vec![Ok("hello"), Err("world")]);
/// }
/// ```
pub fn generate_try_stream<T, E, F, R>(generator: F) -> impl Stream<Item = Result<T, E>>
where
    F: FnOnce(Yielder<Result<T, E>>) -> R,
    R: Future<Output = Result<(), E>>,
{
    generate_stream(|y| async move {
        let mut y2 = y.clone();
        if let Err(err) = generator(y).await {
            y2.send(Err(err)).await;
        }
    })
}
