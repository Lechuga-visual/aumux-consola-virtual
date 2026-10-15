//! Detect our allocations AND deallocations on a simulated audio callback thread.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! { static ACTIVE:Cell<bool>=const{Cell::new(false)}; static OPS:Cell<usize>=const{Cell::new(0)}; }
struct Counting;
fn count() {
    if ACTIVE.try_with(|a| a.get()).unwrap_or(false) {
        let _ = OPS.try_with(|n| n.set(n.get() + 1));
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count();
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
pub fn no_heap<T>(f: impl FnOnce() -> T) -> T {
    OPS.with(|n| n.set(0));
    ACTIVE.with(|a| a.set(true));
    let result = f();
    ACTIVE.with(|a| a.set(false));
    assert_eq!(
        OPS.with(|n| n.get()),
        0,
        "audio callback allocated or destroyed heap storage"
    );
    result
}
