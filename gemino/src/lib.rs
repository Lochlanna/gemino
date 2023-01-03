//! Gemino is a multi producer, multi consumer channel which allows broadcasting of data from many
//! producers across multiple threads to many consumers on multiple threads
//!
//! Gemino is very fast thanks to the use of memory barriers and the relaxation of message delivery guarantees
//! All gemino channels are buffered to allow us to guarantee that a write will always succeed.
//! When the buffer is filled the writers will begin overwriting the oldest entries. This means
//! that receivers who do not keep up will loose data. It is the responsibility of the developer to
//! handle this case.
//!
//! Gemino makes use of unsafe and requires nightly for now.

#![feature(sync_unsafe_cell)]
#![feature(vec_into_raw_parts)]
#![feature(iter_collect_into)]

extern crate core;

#[allow(dead_code)]
mod channel;

#[cfg(test)]
mod async_tests;
#[cfg(test)]
mod tests;

use channel::*;
use std::fmt::Debug;
use std::sync::Arc;
use thiserror::Error;

pub use channel::ChannelValue;

/// A receiver retrieves messages from the channel in the order they were sent
#[derive(Debug)]
pub struct Receiver<T> {
    inner: Arc<Channel<T>>,
    next_id: usize,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("The receiver has lagged behind and missed {0} messages")]
    Lagged(usize),
    #[error("There is no new data to retrieve from the channel")]
    NoNewData,
    #[error("channel buffer size must be at least 1")]
    BufferTooSmall,
    #[error("channel buffer size cannot exceed isize::MAX")]
    BufferTooBig,
    #[error("channel is being written so fast that a valid read was not possible")]
    Overloaded,
    #[error("the channel has been closed")]
    Closed,
}

impl<T> Receiver<T> {
    /// Returns a copy of the pointer to the underlying channel implementation. This is safe although
    /// not a recommended way to use the channel
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// # use gemino::channel;
    /// # tokio_test::block_on(async {
    /// let (_, rx) = channel(2).expect("couldn't create channel");
    /// let inner;
    /// unsafe  {
    ///     inner = rx.to_inner();
    /// }
    /// inner.send(42);
    /// let v = inner.get(0).await.expect("Couldn't receive latest from channel");
    /// assert_eq!(v, 42);
    /// # });
    ///
    /// ```
    pub fn to_inner(self) -> Arc<Channel<T>> {
        self.inner
    }

    /// Reset the internal position counter to zero. This means that on the next call to receive it
    /// will get either the oldest value on the channel or an id too old error and skip forward to the
    /// oldest value for the next call to receive.
    ///
    /// This is useful for when you need another receiver handle and want to get the previous history of
    /// messages.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42);
    /// let v = rx.recv().expect("Couldn't receive from channel");
    /// assert_eq!(v, 42);
    /// tx.send(72);
    /// let mut rxb = rx.clone().reset();
    /// let va = rx.recv().expect("Couldn't receive from channel");
    /// let vb = rxb.recv().expect("Couldn't receive from channel");
    /// assert_eq!(va, 72);
    /// assert_eq!(vb, 42);
    /// ```
    pub fn reset(mut self) -> Self {
        self.next_id = 0;
        self
    }

    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

impl<T> Receiver<T>
where
    T: ChannelValue,
{
    /// Receives the next value from the channel. This function will block until a new value is put onto the
    /// channel; even if there are no senders. It is probably best to use `[recv_before]`
    ///
    /// # Errors
    /// - `Error::Lagged` The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42);
    /// let v = rx.recv().expect("Couldn't receive from channel");
    /// assert_eq!(v, 42);
    /// ```
    pub fn recv(&mut self) -> Result<T, Error> {
        let id = self.next_id;
        match self.inner.get_blocking(id) {
            Ok(value) => {
                self.next_id += 1;
                Ok(value)
            }
            Err(err) => match err {
                ChannelError::IdTooOld(oldest) => {
                    self.next_id = oldest as usize;
                    let missed = self.next_id - id;
                    Err(Error::Lagged(missed))
                }
                ChannelError::Closed => Err(Error::Closed),
                _ => panic!("unexpected error while receving from channel: {err}"),
            },
        }
    }

    /// Asynchronously receives the next value from the channel.
    ///
    /// # Errors
    /// - `Error::Lagged` The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # tokio_test::block_on(async {
    ///  let (tx, mut rx) = channel(2).expect("couldn't create channel");
    ///  tx.send(42);
    ///  let v = rx.async_recv().await.expect("Couldn't receive from channel");
    ///  assert_eq!(v, 42);
    /// # });
    /// ```
    pub async fn async_recv(&mut self) -> Result<T, Error> {
        let id = self.next_id;
        match self.inner.get(id).await {
            Ok(value) => {
                self.next_id += 1;
                Ok(value)
            }
            Err(err) => match err {
                ChannelError::IdTooOld(oldest) => {
                    self.next_id = oldest as usize;
                    let missed = self.next_id - id;
                    Err(Error::Lagged(missed))
                }
                ChannelError::Closed => Err(Error::Closed),
                _ => panic!("unexpected error while receving from channel: {err}"),
            },
        }
    }

    /// Reads all new values from the channel into a vector. This function does not block and does
    /// not fail. If there is no new data it does nothing. If there are missed messages the number
    /// of missed messages will be returned. New values are appended to the given vector so it is the
    /// responsibility of the caller to reset the vector.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42); // This will be overwritten because the buffer size is only 2
    /// tx.send(12);
    /// tx.send(21);
    /// let mut results = Vec::new();
    /// let v = rx.recv_many(&mut results).expect("couldn't do bulk read from channel");
    /// assert_eq!(v, 1); // We missed out on one message
    /// assert_eq!(vec![12,21], results);
    /// tx.send(5);
    /// let v = rx.try_recv().expect("couldn't get a value"); // The receiver is now caught up to the latest value
    /// assert_eq!(v, 5);
    /// ```
    pub fn recv_many(&mut self, result: &mut Vec<T>) -> Result<usize, Error> {
        let (first_id, last_id) =
            self.inner
                .read_batch_from(self.next_id, result)
                .or_else(|err| match err {
                    ChannelError::IDNotYetWritten => Err(Error::NoNewData),
                    ChannelError::Closed => Err(Error::Closed),
                    _ => panic!("unexpected error while performing bulk read: {err}"),
                })?;

        let mut missed = 0;
        if first_id as usize > self.next_id {
            missed = first_id as usize - self.next_id
        }
        self.next_id = last_id as usize + 1;
        Ok(missed)
    }

    /// Attempt to retrieve the next value from the channel immediately. This function will not block.
    /// If there is no new value to retrieve `Error::NoNewData` is returned as an Error.
    ///
    /// As with [`recv`] if the receiver is running behind so far that it hasn't read values before they're overwritten
    /// `Error::Lagged` is returned along with the number of values the receiver has missed. The receiver is then updated
    /// so that the next call to any recv function will return the oldest value on the channel
    ///
    ///
    /// # Errors
    /// - `Error::Lagged` The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error
    /// - `Error::NoNewData` There is no new data on the channel to be received
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42);
    /// let v = rx.try_recv().expect("Couldn't receive from channel");
    /// assert_eq!(v, 42);
    /// assert!(rx.try_recv().is_err())
    /// ```
    pub fn try_recv(&mut self) -> Result<T, Error> {
        let id = self.next_id;
        match self.inner.try_get(id) {
            Ok(value) => {
                self.next_id += 1;
                Ok(value)
            }
            Err(err) => {
                match err {
                    ChannelError::IdTooOld(oldest_valid_id) => {
                        //lagged
                        self.next_id = oldest_valid_id as usize;
                        let missed = oldest_valid_id as usize - id;
                        Err(Error::Lagged(missed))
                    }
                    ChannelError::Closed => Err(Error::Closed),
                    ChannelError::IDNotYetWritten => Err(Error::NoNewData),
                    _ => panic!("unexpected error while receiving value from channel: {err}"),
                }
            }
        }
    }

    fn try_latest(&mut self) -> Result<(T, isize), Error> {
        self.inner.get_latest().or_else(|err| match err {
            ChannelError::Overloaded => Err(Error::Overloaded),
            ChannelError::IDNotYetWritten => Err(Error::NoNewData),
            ChannelError::Closed => Err(Error::Closed),
            _ => panic!("unexpected data while getting latest value from channel: {err}"),
        })
    }

    /// Get the latest value from the channel unless it has already been read in which case wait (blocking)
    /// for the next value to be put onto the channel
    /// This will cause the receiver to skip forward to the most recent value meaning that recv will get
    /// the next latest value.
    ///
    ///
    /// # Errors
    /// - `Error::NoNewData` The channel has not been written to yet
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::thread;
    /// # use std::time::Duration;
    /// # use gemino::channel;
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42);
    /// let v = rx.latest().expect("Couldn't receive latest from channel");
    /// assert_eq!(v, 42);
    /// thread::spawn(move || {
    ///     thread::sleep(Duration::from_secs_f32(0.1));
    ///     tx.send(72);
    /// });
    /// let v = rx.latest().expect("Couldn't receive latest from channel");
    /// assert_eq!(v, 72);
    /// ```
    pub fn latest(&mut self) -> Result<T, Error> {
        let (value, id) = self.try_latest()?;
        if (id as usize) < self.next_id {
            return self.recv();
        }
        self.next_id = id as usize + 1;
        Ok(value)
    }

    /// Get the latest value from the channel unless it has already been read in which case wait (async)
    /// for the next value to be put onto the channel
    /// This will cause the receiver to skip forward to the most recent value meaning that recv will get
    /// the next latest value.
    ///
    ///
    /// # Errors
    /// - `Error::NoNewData` The channel has not been written to yet
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// # use gemino::channel;
    /// # tokio_test::block_on(async {
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42);
    /// let v = rx.latest_async().await.expect("Couldn't receive latest from channel");
    /// assert_eq!(v, 42);
    /// tokio::spawn(async move {
    ///     tokio::time::sleep(Duration::from_secs_f32(0.1)).await;
    ///     tx.send(72);
    /// });
    /// let v = rx.latest_async().await.expect("Couldn't receive latest from channel");
    /// assert_eq!(v, 72);
    /// # });
    ///
    /// ```
    pub async fn latest_async(&mut self) -> Result<T, Error> {
        let (value, id) = self.try_latest()?;
        if (id as usize) < self.next_id {
            return self.async_recv().await;
        }
        self.next_id = id as usize + 1;
        Ok(value)
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            next_id: self.next_id,
        }
    }
}

impl<T> From<Arc<Channel<T>>> for Receiver<T> {
    fn from(ring_buffer: Arc<Channel<T>>) -> Self {
        Self {
            inner: ring_buffer,
            next_id: 0,
        }
    }
}

/// A sender puts new messages onto the channel
#[derive(Debug)]
pub struct Sender<T> {
    inner: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    /// Returns a copy of the pointer to the underlying channel implementation. This is safe although
    /// not a recommended way to use the channel
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// # use gemino::channel;
    /// # tokio_test::block_on(async {
    /// let (tx, _) = channel(2).expect("couldn't create channel");
    /// let inner;
    /// unsafe  {
    ///     inner = tx.to_inner();
    /// }
    /// inner.send(42);
    /// let v = inner.get(0).await.expect("Couldn't receive latest from channel");
    /// assert_eq!(v, 42);
    /// # });
    ///
    /// ```
    pub fn to_inner(self) -> Arc<Channel<T>> {
        self.inner
    }

    pub fn close(&self) {
        self.inner.close();
    }

    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

impl<T> Sender<T>
where
    T: ChannelValue,
{
    /// put a new value into the channel. This function will always succeed and never block.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// let (tx, mut rx) = channel(2).expect("couldn't create channel");
    /// tx.send(42);
    /// let v = rx.try_recv().expect("Couldn't receive from channel");
    /// assert_eq!(v, 42);
    /// ```
    pub fn send(&self, val: T) -> Result<(), Error> {
        if let Err(err) = self.inner.send(val) {
            return match err {
                ChannelError::Closed => Err(Error::Closed),
                _ => panic!("unexpected error attempting to send to channel"),
            };
        }
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> From<Arc<Channel<T>>> for Sender<T> {
    fn from(ring_buffer: Arc<Channel<T>>) -> Self {
        Self { inner: ring_buffer }
    }
}

/// Creates a new channel with a given buffer size returning both sender and receiver handles
///
/// # Example
///
/// ```rust
/// # use gemino::channel;
/// let (tx, mut rx) = channel(2).expect("couldn't create channel");
/// tx.send(42);
/// let v = rx.try_recv().expect("Couldn't receive from channel");
/// assert_eq!(v, 42);
/// ```
pub fn channel<T>(buffer_size: usize) -> Result<(Sender<T>, Receiver<T>), Error> {
    let chan = Channel::new(buffer_size).or_else(|err| match err {
        ChannelError::BufferTooSmall => Err(Error::BufferTooSmall),
        ChannelError::BufferTooBig => Err(Error::BufferTooBig),
        _ => panic!("unexpected error while creating a new channel: {err}"),
    })?;
    let sender = Sender::from(chan.clone());
    let receiver = Receiver::from(chan);
    Ok((sender, receiver))
}
