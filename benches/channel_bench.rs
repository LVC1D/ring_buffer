use criterion::{Criterion, criterion_group, criterion_main};
use integration_project::channel::*;
use tokio::task::JoinSet;

use std::hint::black_box;
use std::sync::Arc;

use tokio::{
    runtime::Runtime,
    sync::{Mutex, mpsc},
};

// Benchmark 1: Fast path (no blocking)
fn bench_send_fast_path(c: &mut Criterion) {
    c.bench_function("send_no_backpressure", |b| {
        let rt = Runtime::new().unwrap();

        b.to_async(&rt).iter(|| async {
            let (tx, rx) = channel::<String>(4);
            // Buffer empty, send completes immediately
            tx.send("message".to_string()).await.unwrap();
            // Clean up
            black_box(rx.recv().await.unwrap());
        });
    });
}

// Benchmark 2: Backpressure path (blocking + unblock)
fn bench_send_with_backpressure(c: &mut Criterion) {
    c.bench_function("send_with_backpressure", |b| {
        let rt = Runtime::new().unwrap();

        b.to_async(&rt).iter(|| async {
            let (tx, rx) = channel::<String>(4);

            // Fill buffer
            tx.send("1".to_string()).await.unwrap();
            tx.send("2".to_string()).await.unwrap();
            tx.send("3".to_string()).await.unwrap();

            // This send will block
            let tx_clone = tx.clone();
            let handle = tokio::spawn(async move { tx_clone.send("4".to_string()).await });

            tokio::task::yield_now().await;
            drop(tx);

            black_box(rx.recv().await.unwrap()); // Unblock
            handle.await.unwrap().unwrap();
        });
    });
}

fn bench_send_fast_path_mpsc(c: &mut Criterion) {
    c.bench_function("send_no_backpressure_mpsc", |b| {
        let rt = Runtime::new().unwrap();

        b.to_async(&rt).iter(|| async {
            let (tx, mut rx) = mpsc::channel::<String>(4);
            // Buffer empty, send completes immediately
            tx.send("message".to_string()).await.unwrap();
            // Clean up
            black_box(rx.recv().await.unwrap());
        });
    });
}

fn bench_mpsc_backpressure(c: &mut Criterion) {
    c.bench_function("send_backpressure_mpsc", |b| {
        let rt = Runtime::new().unwrap();

        b.to_async(&rt).iter(|| async {
            // Fill buffer, measure blocked send time
            // Your implementation here
            let (tx, mut rx) = mpsc::channel::<String>(4); // Small capacity

            tx.send(String::from("One")).await.unwrap();
            tx.send(String::from("Two")).await.unwrap();
            tx.send(String::from("Three")).await.unwrap();

            let tx_clone = tx.clone();
            let handle = tokio::spawn(async move { tx_clone.send("4".to_string()).await });

            tokio::task::yield_now().await;
            black_box(rx.recv().await.unwrap()); // Unblock
            handle.await.unwrap().unwrap();
        });
    });
}

fn bench_multi_threaded(c: &mut Criterion) {
    c.bench_function("multi_threaded_contention", |b| {
        let rt = Runtime::new().unwrap();

        b.to_async(&rt).iter(|| async {
            let (tx, rx) = channel::<String>(64);
            let mut set = JoinSet::new();

            for i in 0..4 {
                let tx_clone = tx.clone();
                set.spawn(async move {
                    for j in 0..10 {
                        tx_clone.send(format!("S{i}-M{j}")).await.unwrap();
                    }
                });
            }

            drop(tx);

            for _ in 0..4 {
                let rx_clone = rx.clone();
                set.spawn(async move {
                    while let Some(res) = rx_clone.recv().await {
                        black_box(res);
                    }
                });
            }

            while let Some(result) = set.join_next().await {
                result.unwrap();
            }
        });
    });
}

fn bench_multi_threaded_mpsc(c: &mut Criterion) {
    c.bench_function("multi_threaded_contention_mpsc", |b| {
        let rt = Runtime::new().unwrap();

        b.to_async(&rt).iter(|| async {
            let (tx, rx) = mpsc::channel::<String>(64);
            let rx = Arc::new(Mutex::new(rx));
            let mut set = JoinSet::new();

            for i in 0..4 {
                let tx_clone = tx.clone();
                set.spawn(async move {
                    for _ in 0..10 {
                        tx_clone.send(format!("test {i}")).await.unwrap();
                    }
                });
            }

            drop(tx);

            for _ in 0..4 {
                let rx_clone = rx.clone();
                set.spawn(async move {
                    while let Some(res) = rx_clone.lock().await.recv().await {
                        black_box(res);
                    }
                });
            }

            while let Some(result) = set.join_next().await {
                result.unwrap();
            }
        });
    });
}

criterion_group!(
    benches,
    bench_send_fast_path_mpsc,
    bench_mpsc_backpressure,
    bench_send_fast_path,
    bench_send_with_backpressure,
    bench_multi_threaded_mpsc,
    bench_multi_threaded,
);
criterion_main!(benches);
