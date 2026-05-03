# MentatKV LearnedKV/RMI Storage Engine Plan

## Purpose

Build MentatKV into an embeddable, high-performance, ACID key-value storage engine in Rust using the core idea from LearnedKV: keep recent mutable data in a short-lived value log indexed by an LSM, and periodically convert stable data into a long-lived static value log indexed by a learned index. This plan adapts the paper's learned-index tier to use an RMI-style learned index with explicit error bounds.

Source paper: [LearnedKV: Integrating LSM and Learned Index for Superior Performance on Storage](https://arxiv.org/abs/2406.18892).

## Non-goals for the first implementation

- Distributed consensus, replication, or multi-node transactions.
- SQL, secondary indexes, or query planning.
- In-place learned-index updates; static learned tiers are rebuilt atomically.
- Relying on a remote object store for the write path.

## Design principles

- The write path must be durable before it is visible.
- The LSM owns freshness; the learned tier owns read-optimized immutable data.
- Readers must tolerate concurrent GC and tier publication without global pauses.
- Every on-disk structure must be versioned and checksummed.
- Remote cold storage is asynchronous and cache-backed, never required for low-latency recent reads.
- The first production-quality version should favor correctness and observability over exotic model tuning.

## High-level architecture

```text
                 +--------------------+
 writes -------> | WAL + active vlog   |
                 +----------+---------+
                            |
                            v
                 +--------------------+
                 | mutable memtable   |
                 +----------+---------+
                            |
                            v
                 +--------------------+       +----------------------+
 reads --------> | immutable memtables | ----> | LSM SST key->pointer |
                 +----------+---------+       +----------+-----------+
                            |                            |
                            | miss                       | pointer
                            v                            v
                 +--------------------+       +----------------------+
                 | RMI learned index  | ----> | static value log     |
                 +----------+---------+       +----------+-----------+
                            |                            |
                            v                            v
                 +--------------------+       +----------------------+
                 | local cache/index  | <---- | cold object storage  |
                 +--------------------+       +----------------------+
```

### Components

1. `Engine`
   - Public embedded API.
   - Owns lifecycle, background workers, snapshots, and configuration.

2. `Wal`
   - Append-only durability log for `Put`, `Delete`, and transaction commit records.
   - Uses record framing: magic, version, length, crc32c, sequence number, payload.
   - Replayed into memtables and LSM recovery state at open.

3. `ValueLog`
   - Short-lived append-only log for recent full key-value records.
   - Indexed by memtable/LSM entries containing key, sequence number, tombstone bit, log id, offset, and length.
   - Segmented to support GC and crash recovery.

4. `Memtable`
   - Ordered in-memory structure keyed by user key plus sequence number.
   - Candidate implementation: `crossbeam-skiplist` or an internal lock-striped `BTreeMap` for the first milestone.

5. `LsmTree`
   - Stores key-to-value-pointer metadata, not full values.
   - Handles recent writes, overwrites, deletes, and tombstones.
   - Uses leveled compaction, per-SST bloom filters, block indexes, and manifests.

6. `StaticValueLog`
   - Immutable sorted key-value record file produced by GC conversion.
   - Indexed by the RMI and a sorted key-pointer list.
   - Versioned by generation so readers can hold old generations safely.

7. `RmiIndex`
   - Two-stage recursive model index over sorted keys in a static generation.
   - Root model predicts a leaf model id; leaf model predicts a position in the key-pointer list.
   - Each leaf stores min/max key, slope, intercept, max lower/upper error, and key-list range.
   - Lookup predicts a bounded window, then binary searches the key-pointer list inside that window.

8. `Manifest`
   - Atomic metadata catalog for WAL segments, vlogs, SSTs, static generations, object-store locations, and published epoch.
   - Written with copy-on-write manifests plus atomic rename.

9. `TierManager`
   - Places data across memory, local disk, and remote object storage.
   - Keeps recent WAL/vlog/LSM files local.
   - Uploads sealed static generations to remote storage once locally durable.
   - Maintains a local hot cache for static key-list blocks and value-log chunks.

10. `GcConverter`
    - Non-blocking conversion worker.
    - Freezes the old LSM/vlog set, creates a fresh write set, extracts latest valid records, sorts by key, writes a static value log, trains the RMI, validates error bounds, and atomically publishes the new generation.

## Core data model

```text
InternalKey = user_key || sequence_number_descending || kind
ValuePointer = tier_id || file_id || offset || length || checksum
RecordKind = Put | Delete | TxnPrepare | TxnCommit | TxnAbort
```

- `sequence_number` is globally monotonic.
- Latest visible version wins.
- Deletes are tombstones and must mask older values in both the LSM and learned tier.
- Snapshot reads use a read timestamp and ignore newer sequence numbers.

## Write path

1. Reserve a sequence number.
2. Append full key-value or tombstone intent to WAL.
3. Append full key-value record to the active value log for `Put`.
4. Fsync according to `DurabilityMode`:
   - `Sync`: fsync before acknowledging.
   - `GroupCommit`: background fsync with bounded latency.
   - `Relaxed`: no per-write fsync, still replayable after OS flush.
5. Insert key-to-pointer metadata into the mutable memtable.
6. Acknowledge after the selected durability condition is satisfied.

## Read path

1. Read mutable memtable.
2. Read immutable memtables.
3. Read LSM SSTs newest to oldest.
4. If missing or masked by no newer tombstone, read active static generation through `RmiIndex`.
5. Fetch value from local disk or local cache.
6. For cold static chunks, fetch from object storage into cache, verify checksum, then serve.

Freshness rule: if a key appears in both the LSM tier and static tier, the LSM result wins because it has the newer sequence number.

## GC and LSM-to-RMI conversion

### Trigger conditions

- Active value-log invalid bytes exceed a configured ratio.
- LSM size exceeds target write tier budget.
- Static generation age exceeds a configured threshold.
- Manual `Engine::compact()` call.

### Non-blocking conversion protocol

1. Acquire a short metadata lock.
2. Seal the current mutable write set:
   - active memtable becomes immutable,
   - active value log becomes sealed,
   - current LSM generation is marked convertible.
3. Create a fresh WAL/value-log/memtable/LSM write set for new writes.
4. Release metadata lock.
5. Scan sealed value logs and LSM metadata to select latest visible key versions.
6. Merge with the previous static generation, applying newer LSM entries and tombstones.
7. Sort valid records by key.
8. Write the new static value log in key order.
9. Write the key-pointer list.
10. Train and validate the RMI.
11. Persist the static generation manifest.
12. Atomically publish the new generation.
13. Retire obsolete LSM, vlog, and static generation files after no snapshots reference them.

## RMI design

### Initial model

- Keys supported in milestone 1: arbitrary byte strings ordered lexicographically.
- Model input: stable 64-bit sortable key digest plus tie-breaking through full-key binary search.
- Root model: linear regression from key digest to leaf id.
- Leaf models: linear regression from key digest to key-list position.
- Error bounds: exact lower and upper observed error for each leaf.
- Lookup: predict position, clamp by error bounds, binary search full keys in the window.

### Later model improvements

- Numeric-key specialization for `u64`/`i64`/big-endian encoded keys.
- Radix or histogram root model for skewed byte-string distributions.
- Adaptive leaf fanout based on max error and target key-list block reads.
- Persisted model statistics for automatic retraining decisions.

## ACID plan

### Atomicity

- Single-key writes are atomic through WAL record boundaries and sequence numbers.
- Multi-key transactions use prepare/commit records before memtable visibility.
- Recovery ignores incomplete or uncommitted transactions.

### Consistency

- Manifest commits publish complete generations only.
- Checksums protect WAL, SST, key-list, model, and value-log records.
- Background workers validate generated files before publication.

### Isolation

- Milestone 1: single-key linearizable operations with snapshot reads.
- Milestone 2: optimistic multi-key transactions with conflict detection at commit.
- Internal MVCC sequence numbers protect readers during compaction and GC.

### Durability

- WAL fsync policy controls acknowledgment.
- Static generation publication is durable only after all local files and manifest updates are synced.
- Remote upload is recorded after object checksum verification; local data is retained until upload is complete.

## Tiered storage plan

### Memory tier

- Mutable memtable.
- Immutable memtables awaiting flush.
- Bloom filters, block cache, RMI models, hot key-list blocks.
- Snapshot and epoch metadata.

### Local high-performance disk tier

- WAL.
- Active value logs.
- SST files.
- Current static generation files.
- Local object cache for cold chunks.

### Remote cold object storage tier

- Sealed static generations.
- Optional archived SSTs after they are fully masked by newer static generations.
- Content-addressed chunks with checksums and generation manifests.
- Provider trait:

```text
trait ObjectStore {
    put(object_id, bytes, checksum)
    get_range(object_id, range)
    head(object_id)
    delete(object_id)
}
```

## Public API sketch

```text
Engine::open(Config) -> Result<Engine>
Engine::put(key, value) -> Result<()>
Engine::get(key) -> Result<Option<Bytes>>
Engine::delete(key) -> Result<()>
Engine::write_batch(batch) -> Result<()>
Engine::snapshot() -> Snapshot
Engine::compact() -> Result<()>
Engine::flush() -> Result<()>
Engine::stats() -> EngineStats
```

## Implementation milestones

### Milestone 0: planning and guardrails

- [x] Save this plan.
- [ ] Add crate-level architecture docs.
- [ ] Decide dependency policy for bytes, checksums, compression, async runtime, and object storage.
- [ ] Add benchmark harness skeleton.

### Milestone 1: durable embedded core

- [ ] Replace the in-memory prototype with `Engine`, `Config`, and error types.
- [ ] Implement WAL record encoding, replay, and corruption handling.
- [ ] Implement value-log segments and value pointers.
- [ ] Implement memtable reads/writes and immutable memtable flush boundaries.
- [ ] Add crash-recovery tests using temporary directories and truncated WAL cases.

### Milestone 2: LSM write tier

- [ ] Implement SST block format, bloom filters, and block indexes.
- [ ] Implement flush from immutable memtable to L0 SST.
- [ ] Implement manifest tracking for SST sets.
- [ ] Implement leveled compaction for key-pointer metadata.
- [ ] Add read/write/overwrite/delete integration tests.

### Milestone 3: static value log and RMI index

- [ ] Implement sorted static value log writer.
- [ ] Implement key-pointer list file.
- [ ] Implement two-stage RMI training and persistence.
- [ ] Implement bounded lookup with binary search verification.
- [ ] Add correctness tests across uniform, sequential, and skewed key distributions.

### Milestone 4: non-blocking GC conversion

- [ ] Implement write-set rotation.
- [ ] Implement valid-record extraction and static generation building.
- [ ] Implement atomic generation publication.
- [ ] Implement obsolete-file retirement with epoch protection.
- [ ] Add concurrent read/write/GC stress tests.

### Milestone 5: ACID transactions and snapshots

- [ ] Implement snapshot reads by sequence number.
- [ ] Implement write batches.
- [ ] Implement optimistic multi-key transactions.
- [ ] Add recovery tests for prepared, committed, and torn transactions.

### Milestone 6: tiered local/remote storage

- [ ] Implement `ObjectStore` trait and local filesystem adapter.
- [ ] Add object-store manifest metadata.
- [ ] Implement async upload of sealed static generations.
- [ ] Implement range-fetching local cache.
- [ ] Add fault-injection tests for missing, corrupt, and delayed objects.

### Milestone 7: performance and observability

- [ ] Add Criterion microbenchmarks for WAL, value-log append, RMI lookup, and SST lookup.
- [ ] Add workload benchmarks for read-heavy, write-heavy, overwrite-heavy, and Zipfian access.
- [ ] Add metrics for write amplification, read amplification, GC duration, RMI error windows, cache hit rate, and remote fetch latency.
- [ ] Tune default thresholds.

## Testing strategy

- Unit tests for record encoders, checksum validation, model training, and binary search windows.
- Property tests for key ordering, latest-version selection, and recovery idempotence.
- Crash tests using subprocesses that kill the engine between WAL append, value-log append, memtable insert, flush, and manifest publication.
- Concurrency tests for readers during GC conversion and generation swaps.
- Fault-injection tests for short reads, torn writes, checksum failures, remote object failures, and manifest rollback.
- Benchmarks comparing:
  - memtable-only baseline,
  - LSM-only KV separation,
  - LSM plus static value log with binary search,
  - LSM plus static value log with RMI.

## Key open decisions

- Should the first LSM implementation be built from scratch or wrap an existing crate temporarily while the RMI/static tier matures?
- Should byte-string RMI modeling use digest-only prediction, prefix-aware numeric projection, or adaptive radix partitioning first?
- Which durability mode should be the default: `Sync` for safety or `GroupCommit` for throughput?
- Should the public API be synchronous first, with async object-store work hidden behind background threads?
- What value size threshold should route values to the value log versus inline SST storage?
- Which object storage providers should be first-class after the local filesystem adapter?

## Immediate next iteration

1. Convert this plan into module-level Rust architecture docs.
2. Introduce `Engine`, `Config`, and `Error` while preserving the current simple `Store` only as a compatibility wrapper or example.
3. Implement WAL and value-log record formats with exhaustive tests.
4. Build a small benchmark harness before optimizing the index layers.
