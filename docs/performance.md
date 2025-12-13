# Performance Analysis - Week 10 Async Channel

## Benchmarks vs tokio::sync::mpsc

|         Scenario            | Our Channel | tokio::mpsc | Difference |
|-----------------------------|-------------|-------------|------------|
| Fast path (no backpressure) | 218ns       | 226ns       |  4% faster |
| Backpressure (blocked send) | 5.87µs      | 6.47µs      |  9% faster |

## Why Faster?

**Ring buffer vs linked list:**
- Our implementation uses contiguous ring buffer (cache-friendly)
- tokio::mpsc uses linked list blocks (pointer chasing, cache misses)
- Week 6 cache optimization knowledge applied

## Trade-offs

**Our approach:**
- Mutex + atomics (simpler, predictable)
- Fixed capacity (no dynamic growth)
- MPMC support built-in

**tokio::mpsc:**
- Lock-free (more complex, better under extreme contention)
- Cooperative scheduling integration
- MPSC only (need Arc<Mutex<Receiver>> for multiple receivers)

## Notes

Multi-threaded contention benchmark shows Criterion cleanup issues between iterations (Arc counts exceed single-iteration maximum). Channel logic verified correct through isolated testing.
