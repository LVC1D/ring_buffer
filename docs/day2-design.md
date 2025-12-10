# Ring Buffer + Async

## Question 1: Channel Structure

The below is the design of the ChannelInner and the Sender / receiver structs that Claude and I agreed upon after quite some time wrestling through some confusions and gaps of my knowledge.

```rust
struct ChannelInner<T> {
    buffer: RingBuffer<T>,
    waiting_senders: Mutex<VecDeque<Waker>>,
    waiting_receivers: Mutex<VecDeque<Waker>>,
    sender_count: Arc<()>,  // Only senders clone this
}

struct Sender<T, S> {
    inner: Arc<ChannelInner<T>>,
    _sender_ref: Arc<()>,  // Clone of sender_count
    _state: PhantomData<S>,
}

struct Receiver<T, S> {
    inner: Arc<ChannelInner<T>>,
    // No _sender_ref - doesn't participate in counting
    _state: PhantomData<S>,
}
```

## Question 2: Type-State Pattern

As per above, we have states as PhantomData markers, based on which we will have dedicated impl blocks that will assist in progressing the state from one to another.

What's valid per each state:
- Open: sending and receiving messages
- Closing: only processing the received messages
- Closed: draining the buffer

```rust
impl<T> Sender<T, Open> {
    pub async fn send(&self, value: T) -> Result<(), SendError> {
        // What return type?
    }
    
    // Any state transition methods?
    pub async fn start_closing(self) -> Sender<T, Closing> {}
}

impl<T> Sender<T, Closing> {
    // Can you send in Closing state?
    // What methods exist here?
    pub async fn close(self) -> Sender<T, Closed>
}
```

## Question 3: Waker Management

We are going for `VecDeque<Waker>` wrapped in Mutex (because multiple threads need access to the wakers list to push into or pop from to signal to the executor to re-poll the send or recv futures)

VecDeque is more flexible than a `Notify` instance because we gain more granular control over what wakers to wake at a given moment. Also, we went with this structure and not with HashMap because HashMap would require us storing also messageID per each waker to grab the corresponding waker

## Question 4: Async Future Implementation

Send and Rev return futures that should be re-pollable.
In order for the futures to be pollable, we need to implement the Future trait on the future, and the Wake trait on our custom Waker structs.

## Question 5: Lifecycle State Transitions

The state transitions happen depending on the count of the senders currently available.
If we reach a sender count of 1, we are getting to Closing, and of at 0 - then it's Closed and we notify all the receivers that they should expect to return None by then.

```rust
impl<T, S> Drop for Sender<T, S> {
    fn drop(&mut self) {
        // _sender_ref drops here automatically
        // Do you need to do anything else?
        // How do receivers "get notified"?
        let _ = self.close();
    }
}
```
