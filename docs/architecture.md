# Ring Buffer 

## Main Components

### The Buffer

The ring buffer is essentially our simple memory buffer, designed in a way like Vec (in a sense that it has a `len` field), but with a few critical differences:
it be initialized as MaybeUninit<T> because we want to avoid the premature initialization of values to avoid any relevant overhead
the capacity will be fixed as per our decision (i e.: 64 to make it cache-aligned for instance)

#### Internals

**Pointers:**
- `head: AtomicUsize` - next write position
- `tail: AtomicUsize` - next read position

**Wrap-around formula:** `(index + 1) % capacity`

**Example with capacity=4:**
- Indices cycle: 0 → 1 → 2 → 3 → 0 → 1 → ...
- With waste-one-slot: max 3 items stored (capacity - 1)

**Operations:**
- `push()`: Write at `head`, increment to `(head + 1) % capacity`
- `pop()`: Read from `tail`, increment to `(tail + 1) % capacity`

### The Channels

The data will be communicated eventually via channels to go to and out of the buffer, so the channel will contain:
the sender as Mutex
the receiver as Mutex
the buffer

Note: the channels are bounded to not only respect the fixed capacity of the buffer, but also to add back-pressure behaviour to avoid any memory-related vulnerabilities like OOM

### The Enum

This structure dictates the state of the channels based on their channel capacity:
Open
Closing (sender is dropped, awaiting all messages to be processed from the receiver)
Closed

### The Waker

The sender and receiver parts of the channel will each have a VecDeque of these wakers stored to call to communicate with each other - and those will be wrapped in Mutex.

We do not wrap the whole buffer in a Mutex so that we do not encounter elongated waits on the buffer to perform rather simple operations,but we rather wrap the waker lists as a) managing these is faster and simpler, and b) we only coordinate the communication between the two parts of the channel which, along with the head and tail as `AtomicUsize`, already ensure the safety against data races

## Behavior Description

Writing of data onto it will utilize AtomicUsize to track the head and tail positions. Why? Because we will check whether the buffer is empty or (full - 1 slot wasted).

The formula will be as such:
Empty: head == tail
Full: (head + 1) % cap == tail

The reason for wasting 1 slot is simply to avoid any further atomic operations and requiring extra fields to manage in the buffer - easier to avoid any data races especially if we get to process millions of messages per second.

To begin with, the structure will be synchronous - no async mechanism will be implemented yet - so we prepare the components before describing how they will interact with each other before turning it asynchronous.

When we get to the async part, we will wake the channels directly within the send / recv call because our program is not IO-bound but rather CPU-bound.

### Implementation Notes

In regards to the buffer structure, we have opted in for a MaybeUninit instead of an Option type because Options use up to 8 bytes per value due to the discriminant and the padding (in case it’s a Some) - so our buffer weighs less bytes.

Although the atomics exists to safely perform head and tail arithmetics asynchronously, we still needed to add in a Mutex lock field - because our push() and pop() require an IMmutable borrow of self - and that’s important to avoid any aliasing violation AND be able to concurrently perform multiple pushes and pops safely.

Upon the Drop implementation, the MaybeUninit union offers an `.assume_init_drop()` method that drops any value that is initialized. What I completely oversaw was that it does not skip over any uninitialized fields - the attempt to drop those slots would cause a Double-Free UB.

### Type-State Enforcement

States will be encoded in the type system using `PhantomData<S>`:

```rust
struct Channel<T, S> {
    inner: Arc<ChannelInner<T>>,
    _state: PhantomData<S>,
}

struct ChannelInner<T> {
    buffer: RingBuffer<T>,  // head/tail use atomics
    waiting_senders: Mutex<VecDeque<Waker>>,  // senders blocked on full
    waiting_receivers: Mutex<VecDeque<Waker>>, // receivers blocked on empty
}
```

## Async Future Implementation Strategy

### Async Layer

**send() returns a Future that:**
1. Checks if buffer has space
2. If yes → write and return Poll::Ready(Ok(()))
3. If no → register waker in `waiting_senders`, return Poll::Pending
4. When recv() frees space → wakes one sender from queue

**recv() returns a Future that:**
1. Checks if buffer has data
2. If yes → read and return Poll::Ready(Some(value))
3. If no → register waker in `waiting_receivers`, return Poll::Pending
4. When send() adds data → wakes one receiver from queue

