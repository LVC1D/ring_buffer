# Week 10 Integration: Async Channel Theory

## 1. Future Implementation & Waker Coordination

We had to implement a Future trait on our custom returned types of `.send()` and `.recv()` to enable the polling - because without them, there would be no way how to yield the push() and pop() calls to the runtime executor to be done in the future (thus, blocking the possibility of converting the program into its true async version).

By implementing poll(), we teach the executor: 'here's how to check if my operation is ready, and here's my Waker so you can wake me when it becomes ready.

Take a look here at this part of our Future implementation for the returned type of the Sender's `.send()` :

```rust
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
        let senders_alive = Arc::strong_count(&this.sender.inner.sender_count) >= 1;

        if !senders_alive {
            return Poll::Ready(Err(SendError::Closed));
        }

        if let Some(res) = this.value.take() {
           // the polling logic here
        } else {
            Poll::Pending
        }
    }
}

```

Notice how we used `Option::take()` to extract the value from the returned value's `sender` field.

Option::take() enables the retry pattern: we extract the value to attempt push(), but if it fails (buffer full), we restore it with this.value = Some(rej) so the next poll can retry. This is why the value must be Option<T> - we need to distinguish 'not attempted yet' from 'attempted and pending'.

In the case of a push being successful - it's done, it's `Poll::Ready()`, and if it's not - we re-store that value as an option and add in a new sender waker for the receiver to later retrieve it to check for that value's availability. 

## 2. Synchronization Primitives: Atomics vs Mutex

- Atomics: For simple values (`usize` positions) where you just need atomic reads/writes
(i.e.: in our Ring Buffer - we store the `head` position - the new position to write into, and `tail` position - the recent position we read the data from)

- `Mutex`: For complex state requiring multi-step consistency (`VecDeque` operations, ensuring wake happens after queue modification)
(i.e.: When each thread needed to lock into the wakers list to grab the waker and wake it)

We cannot rely just on atomics on thew wakers because the whole logic behind the wakers mechanism to ensure the effective and SAFE multi-threaded communication between threads for both sends and receives is much more than just one or two atomic updates.

## 3. Type-State Pattern for Compile-Time Safety

There was no need for explicit API to be written per each type state - as it turned out - because that is handled automatically based on what the futures polls, and because the type-state encodes channel lifecycle in the type system

In our API, the send and receive is only available on the Sender / Receiver type whose state is Open and for the relevant type only - guaranteed at compile.

Look here:
```rust
impl<T> Sender<T, Open> {
    // never available on Receiver struct
    pub fn send(&self, value: T) -> SendFuture<'_, T> {
        SendFuture {
            sender: self,
            value: Some(value),
        }
    }
}
```

This API would not be possible for any sender or receiver that is in Closing nor Closed state.
In addition, those states are already taken care of by the time our explicit Drop implementation kicks in.

```rust
impl<T, S> Drop for Sender<T, S> {
    fn drop(&mut self) {
        let count = Arc::strong_count(&self._sender_ref);

        if count == 2 {
            let wakers: Vec<_> = self
                .inner
                .waiting_receivers
                .lock()
                .unwrap()
                .drain(..)
                .collect();

            for waker in wakers {
                waker.wake();
            }
        }
    }
}

```

## Key Takeaways

- Explicit type states allow us to implement the logic we need per each type and their state, guaranteed at compile

- A future trait implementation on the necessary functions' / methods' returned type is necessary if we want our API to work asynchronously and to communicate the future effectively with an async runtime executor like Tokio

- Atomics work for simple single-value updates. Mutex (or Arc<Mutex>) is for multi-step operations requiring consistency guarantees.

- When benchmarking the async code (custom-designed or a simplified one that uses already-available tools like tokio's `sync::mpsc`) - it's important to understand:
1. WHAT you want to spawn 
2. For WHAT exact task 
3. WHEN to await them all 
4. When to drop the original sender - to signal we are done with sending new stuff to the channel

The above will greatly assist in designing the tests and benchmarks that will not get stuck in an infinite loop, and let you know - through further profiling and logging techniques - whether certain bits of your API require optimizations or not.
