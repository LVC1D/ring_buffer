use super::ring_buffer::RingBuffer;
use std::{
    collections::VecDeque,
    fmt::Display,
    marker::PhantomData,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};
use thiserror::Error;

pub struct Open;
pub struct Closing;
pub struct Closed;

#[derive(Debug, Error)]
pub enum SendError {
    BufferFull,
    Closed,
}

impl Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferFull => write!(f, "Buffer is full"),
            Self::Closed => write!(f, "Channel closed"),
        }
    }
}

#[derive(Debug)]
pub struct ChannelInner<T> {
    buffer: RingBuffer<T>,
    waiting_senders: Mutex<VecDeque<Waker>>,
    waiting_receivers: Mutex<VecDeque<Waker>>,
    sender_count: Arc<()>,
    capacity: usize,
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

#[derive(Debug)]
pub struct Sender<T, S> {
    inner: Arc<ChannelInner<T>>,
    _sender_ref: Arc<()>, // clone of sender_count
    _state: PhantomData<S>,
}

pub struct SendFutureRepl<'a, T> {
    sender: &'a Sender<T, Open>,
    value: Option<T>,
}

impl<'a, T> Future for SendFutureRepl<'a, T>
where
    T: Unpin,
{
    type Output = Result<(), SendError>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let sender_count = Arc::strong_count(&self.sender.inner.sender_count) > 1;

        let this = self.get_mut();
        let buffer_cap = &this.sender.inner.buffer;

        if buffer_cap.is_empty() && !sender_count {
            return Poll::Ready(Err(SendError::Closed));
        }

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
                Err(rej) => {
                    this.sender
                        .inner
                        .waiting_senders
                        .lock()
                        .unwrap()
                        .push_back(cx.waker().clone());
                    this.value = Some(rej);
                    Poll::Pending
                }
            }
        } else {
            Poll::Pending
        }
    }
}

#[derive(Debug)]
pub struct Receiver<T, S> {
    inner: Arc<ChannelInner<T>>,
    _state: PhantomData<S>,
}

impl<T> Sender<T, Open> {
    pub fn send(&self, value: T) -> SendFutureRepl<'_, T> {
        SendFutureRepl {
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

#[tokio::test]
async fn test_replication_send() {
    let (tx, rx) = channel::<u32>(4);
    tx.send(42).await.unwrap();
    assert_eq!(rx.recv().await, Some(42));
}
