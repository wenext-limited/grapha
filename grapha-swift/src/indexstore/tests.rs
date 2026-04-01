use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use super::{HandleCache, StoreHandle};

#[derive(Debug)]
struct DummyHandle(&'static str);

#[test]
fn caches_handles_per_store_path() {
    let cache = HandleCache::default();

    let a = cache
        .get_or_insert_with(PathBuf::from("/tmp/store-a").as_path(), || {
            Some(DummyHandle("a"))
        })
        .unwrap();
    let b = cache
        .get_or_insert_with(PathBuf::from("/tmp/store-b").as_path(), || {
            Some(DummyHandle("b"))
        })
        .unwrap();

    assert!(!Arc::ptr_eq(&a, &b));
}

#[test]
fn reuses_live_handle_for_same_path() {
    let cache = HandleCache::default();
    let path = PathBuf::from("/tmp/store-a");

    let first = cache
        .get_or_insert_with(&path, || Some(DummyHandle("first")))
        .unwrap();
    let second = cache
        .get_or_insert_with(&path, || Some(DummyHandle("second")))
        .unwrap();

    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn retains_handle_for_sequential_same_path_lookups() {
    let cache = HandleCache::default();
    let path = PathBuf::from("/tmp/store-a");

    let first = cache
        .get_or_insert_with(&path, || Some(DummyHandle("first")))
        .unwrap();
    let first_ptr = Arc::as_ptr(&first);
    drop(first);

    let reused = cache
        .get_or_insert_with(&path, || Some(DummyHandle("reopened")))
        .unwrap();
    assert_eq!(Arc::as_ptr(&reused), first_ptr);
    assert_eq!(reused.0, "first");
}

#[test]
fn reopens_after_capacity_eviction() {
    let cache = HandleCache::with_capacity(1);
    let path = PathBuf::from("/tmp/store-a");
    let other = PathBuf::from("/tmp/store-b");

    let first = cache
        .get_or_insert_with(&path, || Some(DummyHandle("first")))
        .unwrap();
    drop(first);
    cache
        .get_or_insert_with(&other, || Some(DummyHandle("other")))
        .unwrap();

    let reopened = cache
        .get_or_insert_with(&path, || Some(DummyHandle("reopened")))
        .unwrap();
    assert_eq!(reopened.0, "reopened");
}

unsafe extern "C" fn count_close(ptr: *mut std::ffi::c_void) {
    let counter = unsafe { &*(ptr as *const AtomicUsize) };
    counter.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn capacity_eviction_closes_unused_store_handles() {
    let cache = HandleCache::with_capacity(1);
    let path_a = PathBuf::from("/tmp/store-a");
    let path_b = PathBuf::from("/tmp/store-b");
    let closed_a = Box::new(AtomicUsize::new(0));
    let closed_b = Box::new(AtomicUsize::new(0));

    let first = cache
        .get_or_insert_with(&path_a, || {
            Some(StoreHandle {
                ptr: (&*closed_a as *const AtomicUsize).cast_mut().cast(),
                close: count_close,
            })
        })
        .unwrap();
    drop(first);

    let retained = cache
        .get_or_insert_with(&path_b, || {
            Some(StoreHandle {
                ptr: (&*closed_b as *const AtomicUsize).cast_mut().cast(),
                close: count_close,
            })
        })
        .unwrap();

    assert_eq!(closed_a.load(Ordering::SeqCst), 1);
    assert_eq!(closed_b.load(Ordering::SeqCst), 0);
    assert!(Arc::strong_count(&retained) >= 1);

    drop(retained);
    drop(cache);
    assert_eq!(closed_b.load(Ordering::SeqCst), 1);
}

#[test]
fn opens_same_path_only_once_during_concurrent_miss() {
    let cache = Arc::new(HandleCache::default());
    let path = PathBuf::from("/tmp/store-a");
    let start = Arc::new(Barrier::new(9));
    let opens = Arc::new(AtomicUsize::new(0));

    let mut threads = Vec::new();
    for _ in 0..8 {
        let cache = Arc::clone(&cache);
        let path = path.clone();
        let start = Arc::clone(&start);
        let opens = Arc::clone(&opens);
        threads.push(thread::spawn(move || {
            start.wait();
            cache
                .get_or_insert_with(&path, || {
                    opens.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(50));
                    Some(DummyHandle("shared"))
                })
                .unwrap()
        }));
    }

    start.wait();
    let handles: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().expect("thread should finish"))
        .collect();

    assert_eq!(opens.load(Ordering::SeqCst), 1);
    for handle in &handles[1..] {
        assert!(Arc::ptr_eq(&handles[0], handle));
    }
}
