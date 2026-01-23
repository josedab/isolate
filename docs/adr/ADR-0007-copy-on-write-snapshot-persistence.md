# ADR-0007: Copy-on-Write Snapshot Persistence

## Status

Accepted

## Context

Snapshotting WASM memory enables fast sandbox restoration from a known state, avoiding cold start compilation costs. However, naive snapshotting has significant drawbacks:

- Full memory copies are expensive (64MB+ per snapshot)
- Multiple snapshots of similar states waste storage
- Zero-filled pages consume space unnecessarily
- Snapshot versioning requires full copies

We needed a snapshot system that:

- Minimizes storage through deduplication
- Enables efficient incremental snapshots (diffs)
- Handles large memories without excessive I/O
- Supports snapshot versioning for rollback

## Decision

We implemented **Copy-on-Write (CoW) snapshot persistence** with page-level deduplication.

### Architecture

```rust
pub struct CowMemoryStore {
    pages: DashMap<PageHash, Arc<PageData>>,  // Hash -> data
    page_size: usize,                          // Typically 4KB
    storage_path: PathBuf,                     // Disk overflow
    max_memory_pages: usize,                   // Memory limit
}

pub struct CowSnapshot {
    id: SnapshotId,
    page_hashes: HashMap<usize, PageHash>,  // Page index -> hash
    zero_pages: Vec<usize>,                 // Known-zero pages
    memory_size: usize,
    page_size: usize,
}
```

### Page Deduplication

Pages are identified by SHA256 hash. Identical pages share storage:

```rust
pub fn store_page(&self, data: &[u8]) -> PageHash {
    let hash = PageHash::from_data(data);

    // Check if we already have this page
    if let Some(existing) = self.pages.get(&hash) {
        existing.increment();  // Increment ref count
        self.bytes_saved.fetch_add(data.len() as u64, Ordering::Relaxed);
        return hash;
    }

    // Store new page
    let page_data = Arc::new(PageData::new(data.to_vec()));
    self.pages.insert(hash.clone(), page_data);
    hash
}
```

### Zero Page Optimization

Zero-filled pages (common in WASM memory) are tracked separately without storage:

```rust
for (page_idx, chunk) in memory.chunks(page_size).enumerate() {
    if chunk.iter().all(|&b| b == 0) {
        zero_pages.push(page_idx);  // No storage needed
    } else {
        let hash = store.store_page(chunk);
        page_hashes.insert(page_idx, hash);
    }
}
```

### Incremental Diffs

Changes between snapshots are stored as diffs:

```rust
pub struct CowSnapshotDiff {
    base_id: SnapshotId,
    modified_pages: HashMap<usize, PageHash>,  // Changed pages
    added_pages: HashMap<usize, PageHash>,     // New non-zero pages
    removed_pages: Vec<usize>,                 // Became zero
}
```

### Memory/Disk Tiering

When in-memory pages exceed limit, cold pages spill to disk:

```rust
fn spill_to_disk(&self) {
    let candidates = self.pages.iter()
        .filter(|e| e.ref_count.load(Ordering::Relaxed) == 1)
        .take(100);

    for (hash, page_data) in candidates {
        self.write_to_disk(&hash, &page_data.data)?;
        self.pages.remove(&hash);
    }
}
```

### Version Management

```rust
pub struct SnapshotVersioner {
    current_version: AtomicU64,
    history: DashMap<u64, SnapshotId>,
    max_history: usize,
}
```

## Consequences

### Positive

- **Storage efficiency**: Deduplication can achieve 10x+ compression for similar states
- **Fast diffs**: Only changed pages need to be stored/transferred
- **Zero-page optimization**: Common WASM memory patterns use minimal storage
- **Reference counting**: Safe page sharing across snapshots
- **Overflow handling**: Large working sets don't OOM

### Negative

- **Hash overhead**: SHA256 computation for every page
- **Complexity**: More code paths than naive copy
- **Fragmentation**: Disk storage can fragment over time
- **No compression**: Individual pages aren't compressed (could be added)

### Implications

- Snapshot creation requires iterating all memory pages
- Page size should align with system page size (4KB) for efficiency
- Disk storage uses content-addressed paths: `<prefix>/<hash[0:2]>/<hash[2:4]>/<hash>`
- Restore requires random access to page store
