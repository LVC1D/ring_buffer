# Performance Baseline

## 1. MPMC Stress-Test

For both the sender and the receiver sides, we need to assert two things:
- All the message are sent ONCE (no duplicate messages)
- The amount of messages sent == the amount of the received

The most optimal approach is via using a `HashSet`  because each entry is a unique one and, compared to a `HashMap`, we do not need any additional values to assist in tracking each message like an ID.

## 2. Backpressure

What we are testing is the effectiveness of the channel's capacity property, as well as 
how well the sender's and receiver's futures handle the full / empty channels.

The way how we will test their futures' implementation rigidness is to `tokio::spawn()` task per each sent message. If the future returned is still pending, since we cannot directly access the returned future's private field (sender or receiver), Tokio has a nice method for each Tokio task - `.is_finished()`.

So with that - we check if a task is still pending, and whenever it gets awaited, the returned value is indeed a `Some()`

We also verify that as we fill up the buffer, the active amount of sent tasks never surpasses the capacity

## 3. Proptesting
We will randomize the number of messages to send to test the effectiveness of the backpressure, along with a random number of capacity values.

The core invariant we are checking: For ANY capacity and ANY number of messages, send all → receive all → they match.
