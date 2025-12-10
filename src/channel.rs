use super::ring_buffer::RingBuffer;
use std::{
    collections::VecDeque,
    fmt::Display,
    marker::PhantomData,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SendError {
    BufferFull,
}

impl Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferFull => write!(f, "Buffer is full"),
        }
    }
}

pub struct Open;
pub struct Closing;
pub struct Closed;

#[derive(Debug)]
pub struct ChannelInner<T> {
    buffer: RingBuffer<T>,
    waiting_senders: Mutex<VecDeque<Waker>>,
    waiting_receivers: Mutex<VecDeque<Waker>>,
    sender_count: Arc<()>,
    capacity: usize,
}

#[derive(Debug)]
pub struct Sender<T, S> {
    inner: Arc<ChannelInner<T>>,
    _sender_ref: Arc<()>, // clone of sender_count
    _state: PhantomData<S>,
}

#[derive(Debug)]
pub struct Receiver<T, S> {
    inner: Arc<ChannelInner<T>>,
    _state: PhantomData<S>,
}

pub fn channel<T>(capacity: usize) -> (Sender<T, Open>, Receiver<T, Open>) {
    let sender_count = Arc::new(());

    let chan = Arc::new(ChannelInner {
        buffer: RingBuffer::new(capacity),
        waiting_senders: Mutex::new(VecDeque::new()),
        waiting_receivers: Mutex::new(VecDeque::new()),
        sender_count: sender_count.clone(),
        capacity,
    });

    let sender = Sender {
        inner: chan.clone(),
        _sender_ref: sender_count,
        _state: PhantomData,
    };

    let receiver = Receiver {
        inner: chan,
        _state: PhantomData,
    };

    (sender, receiver)
}

impl<T> Sender<T, Open> {
    pub fn send(&self, value: T) -> SendFuture<'_, T> {
        SendFuture {
            sender: self,
            value: Some(value),
        }
    }
}

impl<T> Receiver<T, Open> {
    pub fn recv(&self) -> RecvFuture<'_, T> {
        RecvFuture { receiver: self }
    }
}

impl<T, S> Clone for Sender<T, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _sender_ref: self._sender_ref.clone(),
            _state: PhantomData,
        }
    }
}

impl<T, S> Clone for Receiver<T, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _state: PhantomData,
        }
    }
}

pub struct SendFuture<'a, T> {
    sender: &'a Sender<T, Open>,
    value: Option<T>,
}

impl<'a, T> Future for SendFuture<'a, T>
where
    T: Unpin,
{
    type Output = Result<(), SendError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(res) = this.value.take() {
            match this.sender.inner.buffer.push(res) {
                Ok(()) => {
                    if let Some(waker) = this
                        .sender
                        .inner
                        .waiting_receivers
                        .lock()
                        .unwrap()
                        .pop_front()
                    {
                        waker.wake();
                    }
                    Poll::Ready(Ok(()))
                }
                Err(rejected) => {
                    this.sender
                        .inner
                        .waiting_senders
                        .lock()
                        .unwrap()
                        .push_back(cx.waker().clone());
                    this.value = Some(rejected);
                    Poll::Pending
                }
            }
        } else {
            Poll::Pending
        }
    }
}

pub struct RecvFuture<'a, T> {
    receiver: &'a Receiver<T, Open>,
}

impl<'a, T> Future for RecvFuture<'a, T> {
    type Output = Option<T>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let senders_alive = Arc::strong_count(&self.receiver.inner.sender_count) > 1;
        let buffer_empty = self.receiver.inner.buffer.is_empty();

        if !senders_alive && buffer_empty {
            return Poll::Ready(None); // Channel closing
        }

        match self.receiver.inner.buffer.pop() {
            Some(val) => {
                if let Some(waker) = self
                    .receiver
                    .inner
                    .waiting_senders
                    .lock()
                    .unwrap()
                    .pop_front()
                {
                    waker.wake();
                }
                Poll::Ready(Some(val))
            }
            None => {
                self.receiver
                    .inner
                    .waiting_receivers
                    .lock()
                    .unwrap()
                    .push_back(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_single_send_recv() {
        let (tx, rx) = channel(8);
        tx.send(3).await.unwrap();

        if let Some(val) = rx.recv().await {
            assert_eq!(val, 3);
        }
    }

    #[tokio::test]
    async fn test_multi_senders() {
        let (tx, rx) = channel(4);
        let tx_clone = tx.clone();

        tx_clone.send(String::from("hello")).await.unwrap();
        tx.send(String::from("World")).await.unwrap();

        if let Some(res) = rx.recv().await {
            assert!(&res == "hello");
        }

        if let Some(res) = rx.recv().await {
            assert!(&res == "World");
        }
    }

    #[tokio::test]
    async fn test_all_senders_drop() {
        let (tx, rx) = channel(4);
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();

        drop(tx); // Last sender dropped

        assert_eq!(rx.recv().await, Some(1)); // Should drain
        assert_eq!(rx.recv().await, Some(2)); // Should drain
        assert_eq!(rx.recv().await, None); // Should close
    }
}
