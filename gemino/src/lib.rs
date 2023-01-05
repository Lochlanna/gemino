//! Gemino is a multi producer, multi consumer channel which allows broadcasting of data from many
//! producers to many consumers. It's built on top of a Ring Buffer and uses a lock-free design.
//! All gemino channels are buffered to guarantee that a write will always succeed with minimal blocking and reads
//! do not block if there there is data available.
//!
//! Gemino is fast thanks to the use of memory barriers and the relaxation of message delivery guarantees.
//! When the buffer is filled the senders will begin overwriting the oldest entries. This means
//! that receivers who do not keep up will miss out on messages. It is the responsibility of the developer to
//! handle this case.
//!
//! Gemino provide both a blocking and async API which can be used simultaneously on the same channel.
//!
//! Gemino makes use of unsafe.

#[cfg(test)]
mod async_tests;
#[allow(dead_code)]
mod channel;
#[cfg(test)]
mod tests;

use channel::*;
use std::fmt::Debug;
use std::ops::Add;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

/// Gemino public error type. Used for both [`Sender`] and [`Receiver`].
#[derive(Error, Debug)]
pub enum Error {
    /// The receiver has fallen behind the channel and missed out on messages. The number of messages
    /// missed out on in provided.
    #[error("The receiver has lagged behind and missed {0} messages")]
    Lagged(usize),
    /// The channel has no new messages that the  receiver hasn't read yet.
    #[error("There is no new data to retrieve from the channel")]
    NoNewData,
    /// Gemino channels must be buffered with a size of at least 1.
    #[error("channel buffer size must be at least 1")]
    BufferTooSmall,
    /// Gemino channels can be no larger than [`isize::MAX`].
    #[error("channel buffer size cannot exceed isize::MAX")]
    BufferTooBig,
    /// The buffer is being written to so quickly that a valid read cannot occur. See [`Receiver::latest`].
    #[error("channel is being written so fast that a valid read was not possible")]
    Overloaded,
    /// The channel has been closed and can no longer send new messages. Messages that have already been
    /// sent may still be received.
    #[error("the channel has been closed")]
    Closed,
    /// When a bulk read is made it must be less than or equal to the size of the underlying buffer
    #[error("request cannot be filled as the channel capacity is smaller than the requested number of elements")]
    ReadTooLarge,
}

/// Retrieves messages from the channel in the order they were sent.
#[derive(Debug)]
pub struct Receiver<T> {
    inner: Arc<Channel<T>>,
    next_id: usize,
}

impl<T> Receiver<T> {
    /// Reset the internal position counter to zero. This means that on the next call to receive it
    /// will get either the oldest value on the channel or an id too old error and skip forward to the
    /// oldest value for the next call to receive.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.recv()?;
    /// assert_eq!(v, 42);
    /// tx.send(72)?;
    /// let mut rxb = rx.clone().reset();
    /// let va = rx.recv()?;
    /// let vb = rxb.recv()?;
    /// assert_eq!(va, 72);
    /// assert_eq!(vb, 42);
    /// #   Ok(())
    /// # }
    /// ```
    pub fn reset(mut self) -> Self {
        self.next_id = 0;
        self
    }

    /// Returns true if the underlying channel has been closed.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel::<u8>(2)?;
    /// assert!(!rx.is_closed());
    /// tx.close();
    /// assert!(rx.is_closed());
    /// # Ok(())
    /// # }
    /// ```
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

impl<T> Receiver<T>
where
    T: Copy + 'static,
{
    /// Receives the next value from the channel. This function will block until a new value is put onto the
    /// channel or the channel is closed; even if there are no senders.
    ///
    /// # Errors
    /// - [`Error::Lagged`] The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error.
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.recv()?;
    /// assert_eq!(v, 42);
    /// # Ok(())
    /// # }
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

    /// Receives the next value from the channel before the timeout.
    /// This function will block even if there are no senders.
    ///
    /// # Errors
    /// - [`Error::Lagged`] The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error.
    /// - [`Error::NoNewData`] No new data was put into the channel before the timeout.
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// # use gemino::{channel, Error};
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.recv_before(Duration::from_millis(5))?;
    /// assert_eq!(v, 42);
    /// std::thread::spawn(move ||{
    ///     std::thread::sleep(Duration::from_millis(6));
    ///     tx.send(22).expect("cannot send");
    /// });
    /// let fail = rx.recv_before(Duration::from_millis(5));
    /// assert!(fail.is_err());
    /// assert!(matches!(fail.err().unwrap(), Error::NoNewData));
    /// let v = rx.recv_before(Duration::from_millis(30))?;
    /// assert_eq!(v, 22);
    /// # Ok(())
    /// # }
    /// ```
    pub fn recv_before(&mut self, timeout: core::time::Duration) -> Result<T, Error> {
        let id = self.next_id;
        match self
            .inner
            .get_blocking_before(id, Instant::now().add(timeout))
        {
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
                ChannelError::Timeout => Err(Error::NoNewData),
                _ => panic!("unexpected error while receving from channel: {err}"),
            },
        }
    }

    /// Asynchronously receives the next value from the channel.
    ///
    /// # Errors
    /// - [`Error::Lagged`] The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error.
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # async fn test()->Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.recv_async().await?;
    /// assert_eq!(v, 42);
    /// # Ok(())
    /// # }
    /// # fn main() -> Result<()> {
    /// # tokio_test::block_on(test())?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn recv_async(&mut self) -> Result<T, Error> {
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

    /// Reads all new values from the channel into a vector. Bulk reads are very fast and memory
    /// efficient as it does a direct memory copy into the target array and only has to do synchronisation
    /// checks one time. In an environment where there is high write load this will be the best way to
    /// read from the channel.
    ///
    /// This function does not block and does not fail. If there is no new data it does nothing. If there are missed messages the number
    /// of missed messages will be returned. New values are appended to the given vector so it is the
    /// responsibility of the caller to reset the vector.
    ///
    /// # Errors
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    /// - [`Error::NoNewData`] All messages on the channel have already been read by this receiver.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?; // This will be overwritten because the buffer size is only 2
    /// tx.send(12)?;
    /// tx.send(21)?;
    /// let mut results = Vec::new();
    /// let v = rx.try_recv_many(&mut results)?;
    /// assert_eq!(v, 1); // We missed out on one message
    /// assert_eq!(vec![12,21], results);
    /// tx.send(5)?;
    /// let v = rx.try_recv()?; // The receiver is now caught up to the latest value
    /// assert_eq!(v, 5);
    /// # Ok(())
    /// # }
    /// ```
    pub fn try_recv_many(&mut self, result: &mut Vec<T>) -> Result<usize, Error> {
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

    /// Reads at least `num` new values from the channel into a vector. Bulk reads are very fast and memory
    /// efficient as it does a direct memory copy into the target array and only has to do synchronisation
    /// checks one time. In an environment where there is high write load this will be the best way to
    /// read from the channel.
    ///
    /// This function will block until at least `num` elements are available to read from the channel.
    ///
    /// # Errors
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    /// - [`Error::NoNewData`] All messages on the channel have already been read by this receiver.
    /// - [`Error::ReadTooLarge`] Request cannot be filled as the channel capacity is smaller than the requested number of elements.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(3)?;
    /// tx.send(42)?;
    /// tx.send(12)?;
    /// tx.send(21)?;
    /// let mut results = Vec::new();
    /// let v = rx.recv_at_least(2, &mut results)?;
    /// assert_eq!(v, 0);
    /// assert_eq!(vec![42, 12,21], results);
    /// # Ok(())
    /// # }
    /// ```
    pub fn recv_at_least(&mut self, num: usize, result: &mut Vec<T>) -> Result<usize, Error> {
        let (first_id, last_id) = self
            .inner
            .read_at_least(num, self.next_id, result)
            .or_else(|err| match err {
                ChannelError::IDNotYetWritten => Err(Error::NoNewData),
                ChannelError::Closed => Err(Error::Closed),
                ChannelError::ReadTooLarge => Err(Error::ReadTooLarge),
                _ => panic!("unexpected error while performing bulk read: {err}"),
            })?;

        let mut missed = 0;
        if first_id as usize > self.next_id {
            missed = first_id as usize - self.next_id
        }
        self.next_id = last_id as usize + 1;
        Ok(missed)
    }

    /// Reads at least `num` new values from the channel into a vector. Bulk reads are very fast and memory
    /// efficient as it does a direct memory copy into the target array and only has to do synchronisation
    /// checks one time. In an environment where there is high write load this will be the best way to
    /// read from the channel.
    ///
    /// This function will wait until at least `num` elements are available to read from the channel.
    ///
    /// # Errors
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    /// - [`Error::NoNewData`] All messages on the channel have already been read by this receiver.
    /// - [`Error::ReadTooLarge`] Request cannot be filled as the channel capacity is smaller than the requested number of elements.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # use std::time::Duration;
    /// # async fn test() -> Result<()> {
    /// let (tx, mut rx) = channel(3)?;
    /// tx.send(42)?;
    /// tx.send(12)?;
    /// let mut results = Vec::new();
    /// let fail = tokio::time::timeout(Duration::from_millis(5), rx.recv_at_least_async(3, &mut results)).await;
    /// assert!(fail.is_err()); // timeout because it was waiting for 3 elements on the array
    /// tx.send(21)?;
    /// let v = rx.recv_at_least_async(3, &mut results).await?;
    /// assert_eq!(v, 0);
    /// assert_eq!(vec![42, 12, 21], results);
    /// # Ok(())
    /// # }
    /// # fn main() -> Result<()> {
    /// # tokio_test::block_on(test())?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn recv_at_least_async(
        &mut self,
        num: usize,
        result: &mut Vec<T>,
    ) -> Result<usize, Error> {
        let (first_id, last_id) = self
            .inner
            .read_at_least_async(num, self.next_id, result)
            .await
            .or_else(|err| match err {
                ChannelError::IDNotYetWritten => Err(Error::NoNewData),
                ChannelError::Closed => Err(Error::Closed),
                ChannelError::ReadTooLarge => Err(Error::ReadTooLarge),
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
    ///
    /// As with [`Receiver::recv`] if the receiver is running behind so far that it hasn't read values before they're overwritten
    /// `Error::Lagged` is returned along with the number of values the receiver has missed. The receiver is then updated
    /// so that the next call to any recv function will return the oldest value on the channel.
    ///
    ///
    /// # Errors
    /// - [`Error::Lagged`] The receiver has fallen behind and the next value has already been overwritten.
    /// The receiver has skipped forward to the oldest available value in the channel and the number of missed messages is returned
    /// in the error.
    /// - [`Error::NoNewData`] There is no new data on the channel to be received.
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.try_recv()?;
    /// assert_eq!(v, 42);
    /// assert!(rx.try_recv().is_err());
    /// # Ok(())
    /// # }
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
    /// for the next value to be put onto the channel.
    /// This will cause the receiver to skip forward to the most recent value meaning that recv will get
    /// the next latest value.
    ///
    ///
    /// # Errors
    /// - [`Error::NoNewData`]The channel has not been written to yet.
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    /// - [`Error::Overloaded`] The channel is being written to so quickly that the entire channel is being
    /// written before a read can take place. This should be very rare and should only really be possible in
    /// scenarios where there is extremely high right load on an extremely small buffer. Either make the buffer bigger
    /// or slow the senders.
    /// # Example
    ///
    /// ```rust
    /// # use std::thread;
    /// # use std::time::Duration;
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.latest()?;
    /// assert_eq!(v, 42);
    /// thread::spawn(move || {
    ///     thread::sleep(Duration::from_secs_f32(0.1));
    ///     tx.send(72).expect("couldn't send");
    /// });
    /// let v = rx.latest()?;
    /// assert_eq!(v, 72);
    /// # Ok(())
    /// # }
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
    /// for the next value to be put onto the channel.
    /// This will cause the receiver to skip forward to the most recent value meaning that recv will get
    /// the next latest value.
    ///
    ///
    /// # Errors
    /// - [`Error::NoNewData`]The channel has not been written to yet.
    /// - [`Error::Closed`] The channel has been closed by a sender. All subsequent calls to this function
    /// will also return this error.
    /// - [`Error::Overloaded`] The channel is being written to so quickly that the entire channel is being
    /// written before a read can take place. This should be very rare and should only really be possible in
    /// scenarios where there is extremely high right load on an extremely small buffer. Either make the buffer bigger
    /// or slow the senders.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # async fn test() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.latest_async().await?;
    /// assert_eq!(v, 42);
    /// tokio::spawn(async move {
    ///     tokio::time::sleep(Duration::from_secs_f32(0.1)).await;
    ///     tx.send(72).expect("couldn't send");
    /// });
    /// let v = rx.latest_async().await?;
    /// assert_eq!(v, 72);
    /// # Ok(())
    /// # }
    ///
    /// # fn main() -> Result<()> {
    /// # tokio_test::block_on(test())?;
    /// # Ok(())
    /// # }
    ///
    /// ```
    pub async fn latest_async(&mut self) -> Result<T, Error> {
        let (value, id) = self.try_latest()?;
        if (id as usize) < self.next_id {
            return self.recv_async().await;
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

/// Puts new messages onto the channel
#[derive(Debug)]
pub struct Sender<T> {
    inner: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    /// Closes the channel for all senders and receivers. No sender will be able to send
    /// a message once the channel has been closed. Any senders which have a send in progress but
    /// no new send operations will succeed.
    ///
    /// Receivers will continue to be able to receive remaining messages. Once they've received all messages
    /// [`Error::Closed`] will be returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel::<u8>(2)?;
    /// assert!(!rx.is_closed());
    /// assert!(!tx.is_closed());
    /// tx.close();
    /// assert!(rx.is_closed());
    /// assert!(tx.is_closed());
    /// # Ok(())
    /// # }
    /// ```
    pub fn close(&self) {
        self.inner.close();
    }

    /// Returns true if the underlying channel has been closed.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel::<u8>(2)?;
    /// assert!(!rx.is_closed());
    /// tx.close();
    /// assert!(rx.is_closed());
    /// # Ok(())
    /// # }
    /// ```
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

impl<T> Sender<T>
where
    T: 'static,
{
    /// Put a new value into the channel. This function will almost never block. The underlying implementation
    /// means that writes have to be finalised in order meaning that if thread a then b writes to the channel.
    /// Thread b will have to wait for thread a to finish as thread a was first to start. Insertion is O(1) and
    /// is a very simple operation so hopefully wont be a major issue.
    ///
    /// # Errors
    /// - [`Error::Closed`] The channel has been closed by some sender.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, mut rx) = channel(2)?;
    /// tx.send(42)?;
    /// let v = rx.try_recv()?;
    /// assert_eq!(v, 42);
    /// # Ok(())
    /// # }
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

    /// Creates a new receiver on the same channel as this sender. The receiver will start at message
    /// id 0 meaning that it's likely the first call will return [`Error::Lagged`] which will cause it
    /// to skip ahead and catch up with the underlying channel.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use gemino::channel;
    /// # use anyhow::Result;
    /// # fn main() -> Result<()> {
    /// let (tx, _) = channel(2)?;
    /// tx.send(42)?;
    /// let mut rx = tx.subscribe();
    /// let v = rx.try_recv()?;
    /// assert_eq!(v, 42);
    /// # Ok(())
    /// # }
    /// ```
    pub fn subscribe(&self) -> Receiver<T> {
        Receiver::from(self.inner.clone())
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

impl<T> From<Receiver<T>> for Sender<T> {
    fn from(receiver: Receiver<T>) -> Self {
        Self {
            inner: receiver.inner,
        }
    }
}

/// Creates a new channel with a given buffer size returning both sender and receiver handles.
///
/// # Errors
/// - [`Error::BufferTooSmall`] The buffer size must be at least 1.
/// - [`Error::BufferTooBig`] The buffer size cannot exceed [`isize::MAX`].
///
/// # Example
///
/// ```rust
/// # use gemino::channel;
/// # use anyhow::Result;
/// # fn main() -> Result<()> {
/// let (tx, mut rx) = channel(2)?;
/// tx.send(42)?;
/// let v = rx.try_recv()?;
/// assert_eq!(v, 42);
/// # Ok(())
/// # }
/// ```
pub fn channel<T: Copy + 'static>(buffer_size: usize) -> Result<(Sender<T>, Receiver<T>), Error> {
    let chan = Channel::new(buffer_size).or_else(|err| match err {
        ChannelError::BufferTooSmall => Err(Error::BufferTooSmall),
        ChannelError::BufferTooBig => Err(Error::BufferTooBig),
        _ => panic!("unexpected error while creating a new channel: {err}"),
    })?;
    let sender = Sender::from(chan.clone());
    let receiver = Receiver::from(chan);
    Ok((sender, receiver))
}
