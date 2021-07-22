//! Hacky way of configuring uffd stuff.

use std::sync::atomic::{AtomicU64, Ordering};

static UFFD_PSEUDO_PAGE_SIZE: AtomicU64 = AtomicU64::new(0);

/// Dummy comment.
pub fn set_pseudo_page_size(size: u64) {
    UFFD_PSEUDO_PAGE_SIZE.store(size, Ordering::Relaxed);
}

/// Dummy comment.
pub fn pseudo_page_size() -> u64 {
    UFFD_PSEUDO_PAGE_SIZE.load(Ordering::Relaxed)
}
