use std::mem::MaybeUninit;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
pub struct RingBuffer<T> {
    buffer: Vec<MaybeUninit<T>>,
    capacity: usize,
    head: AtomicUsize, // next write position
    tail: AtomicUsize, // next read position
    lock: Mutex<()>,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two(), "Capacity must be power of 2");

        let mut buffer = Vec::with_capacity(capacity);

        // SAFETY: As the vector's internals are uninitialized,
        // we need to explicitly state it's length that corresponds
        // to the capacity
        unsafe {
            buffer.set_len(capacity);
        }

        Self {
            buffer,
            capacity,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            lock: Mutex::new(()),
        }
    }

    /// Try to push a value into the buffer.
    /// Returns Err(value) if buffer is full.
    pub fn push(&self, value: T) -> Result<(), T> {
        let _guard = self.lock.lock().unwrap();

        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % self.capacity;

        if self.is_full() {
            return Err(value);
        }

        let base = self.buffer.as_ptr() as *mut MaybeUninit<T>;
        let slot_ptr = unsafe { base.add(head) };

        // SAFETY: Lock ensures no concurrent access. head verified
        // to point to uninitialized slot (not full). Transferring
        // ownership of `value` into the slot via ptr::write.
        unsafe {
            (*slot_ptr).write(value);
        }

        self.head.store(next_head, Ordering::Relaxed);

        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        let _guard = self.lock.lock().unwrap();
        let tail = self.tail.load(Ordering::Relaxed);

        if self.is_empty() {
            return None;
        }

        let base = self.buffer.as_ptr();

        // SAFETY: Lock ensures exclusive access. is_empty() check
        // guarantees tail points to initialized data. Reading moves
        // the value out, leaving slot uninitialized (OK because tail
        // will advance past it).
        let slot_ptr = unsafe { base.add(tail) };
        let value = unsafe { MaybeUninit::assume_init_read(&*slot_ptr) };

        let next_tail = (tail + 1) % self.capacity;
        self.tail.store(next_tail, Ordering::Relaxed);

        Some(value)
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed) == self.tail.load(Ordering::Relaxed)
    }

    pub fn is_full(&self) -> bool {
        (self.head.load(Ordering::Relaxed) + 1) % self.capacity == self.tail.load(Ordering::Relaxed)
    }
}

impl<T> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        let mut current = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Relaxed);

        while current != head {
            unsafe {
                // SAFETY: Elements between tail and head are initialized.
                // This loop walks through exactly those elements.
                let ptr = self.buffer.as_ptr().add(current) as *mut MaybeUninit<T>;
                (*ptr).assume_init_drop();
            }
            current = (current + 1) % self.capacity;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_single() {
        let rb = RingBuffer::new(4);
        rb.push(42).unwrap();
        assert_eq!(rb.pop(), Some(42));
    }

    #[test]
    fn test_pop_till_empty() {
        let rb = RingBuffer::new(2);
        rb.push(2).unwrap();

        assert_eq!(rb.pop(), Some(2));
        assert!(rb.is_empty());
        assert!(rb.pop().is_none());
    }

    #[test]
    fn test_push_till_full() {
        let rb = RingBuffer::new(2);
        rb.push(2).unwrap();

        assert!(rb.push(2).is_err());
        assert!(rb.is_full());
    }

    #[test]
    fn test_wrap_around_strings() {
        let rb = RingBuffer::new(4);
        rb.push(String::from("hi")).unwrap();
        rb.push(String::from("test")).unwrap();
        rb.push(String::from("String")).unwrap();

        let _ = rb.pop();

        rb.push(String::from("Wrapped")).unwrap();

        let head = rb.head.load(Ordering::Relaxed);
        let tail = rb.tail.load(Ordering::Relaxed);

        assert_eq!(head, 0);
        assert_eq!(tail, 1);
        assert!(rb.is_full());
    }
}
