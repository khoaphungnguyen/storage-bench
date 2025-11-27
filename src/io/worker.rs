use crate::config::IoMode;
use crate::io::patterns::IoPattern;
use crate::io::Device;
use anyhow::Result;
use io_uring::{opcode, types, IoUring};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Statistics collected by a worker
#[derive(Debug, Default)]
pub struct WorkerStats {
    pub bytes_read: AtomicU64,
    pub bytes_written: AtomicU64,
    pub ops_completed: AtomicU64,
    pub ops_failed: AtomicU64,
    pub total_latency_ns: AtomicU64,
    pub min_latency_ns: AtomicU64,
    pub max_latency_ns: AtomicU64,
}

impl WorkerStats {
    pub fn new() -> Self {
        Self {
            min_latency_ns: AtomicU64::new(u64::MAX),
            ..Default::default()
        }
    }

    pub fn record_op(&self, bytes: usize, latency_ns: u64, is_read: bool) {
        if is_read {
            self.bytes_read.fetch_add(bytes as u64, Ordering::Relaxed);
        } else {
            self.bytes_written
                .fetch_add(bytes as u64, Ordering::Relaxed);
        }

        self.ops_completed.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns
            .fetch_add(latency_ns, Ordering::Relaxed);

        // Update min/max latency
        let mut current_min = self.min_latency_ns.load(Ordering::Relaxed);
        while latency_ns < current_min {
            match self.min_latency_ns.compare_exchange_weak(
                current_min,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        let mut current_max = self.max_latency_ns.load(Ordering::Relaxed);
        while latency_ns > current_max {
            match self.max_latency_ns.compare_exchange_weak(
                current_max,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }
    }
}

/// I/O worker thread with io_uring support
pub struct IoWorker {
    device: Arc<Device>,
    pattern: Arc<IoPattern>,
    stats: Arc<WorkerStats>,
    stop_flag: Arc<AtomicBool>,
    block_size: usize,
    queue_depth: usize,
    read_percent: u8,
    // Multiple aligned buffers for O_DIRECT I/O (one per queue depth for fixed buffers)
    // Each buffer must be aligned to filesystem block size (typically 512 bytes)
    buffers: Vec<Vec<u8>>,
    buffer_ptrs: Vec<*mut libc::c_void>, // Track original pointers for cleanup
}

impl IoWorker {
    pub fn new(device: Arc<Device>, mode: IoMode, block_size: usize, queue_depth: usize) -> Self {
        Self::new_with_read_percent(device, mode, block_size, queue_depth, 100)
    }

    pub fn new_with_read_percent(
        device: Arc<Device>,
        mode: IoMode,
        block_size: usize,
        queue_depth: usize,
        read_percent: u8,
    ) -> Self {
        let device_size = device.size();

        // Allocate multiple properly aligned buffers for O_DIRECT I/O
        // O_DIRECT requires:
        // 1. Buffer aligned to filesystem block size (typically 512 bytes)
        // 2. Buffer size must be multiple of block size
        // Use posix_memalign for guaranteed alignment
        // CRITICAL: Need one buffer per queue depth for fixed buffers to work correctly!
        let alignment = 512;
        let total_size = ((block_size + alignment - 1) / alignment) * alignment; // Round up to alignment

        let mut buffers = Vec::with_capacity(queue_depth);
        let mut buffer_ptrs = Vec::with_capacity(queue_depth);

        for _ in 0..queue_depth {
            let buffer_ptr = unsafe {
                let mut ptr: *mut libc::c_void = std::ptr::null_mut();
                let result = libc::posix_memalign(&mut ptr, alignment, total_size);
                if result != 0 || ptr.is_null() {
                    // Fallback to Vec if posix_memalign fails
                    let mut buf = Vec::with_capacity(total_size);
                    unsafe {
                        buf.set_len(block_size);
                    }
                    buffers.push(buf);
                    buffer_ptrs.push(std::ptr::null_mut());
                    continue;
                }
                ptr
            };

            // Create Vec from aligned memory
            let buffer =
                unsafe { Vec::from_raw_parts(buffer_ptr as *mut u8, block_size, total_size) };
            buffers.push(buffer);
            buffer_ptrs.push(buffer_ptr);
        }

        Self {
            device,
            pattern: Arc::new(IoPattern::new(mode, block_size, device_size)),
            stats: Arc::new(WorkerStats::new()),
            stop_flag: Arc::new(AtomicBool::new(false)),
            block_size,
            queue_depth,
            read_percent,
            buffers,
            buffer_ptrs,
        }
    }

    pub fn stats(&self) -> Arc<WorkerStats> {
        Arc::clone(&self.stats)
    }

    /// Replace the internal stats with external shared stats
    pub fn set_stats(&mut self, stats: Arc<WorkerStats>) {
        self.stats = stats;
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_flag)
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Run the worker with io_uring (blocking)
    pub fn run(&mut self, duration: Duration) -> Result<()> {
        let fd = self.device.as_raw_fd();
        let mut ring = IoUring::new(self.queue_depth as u32)?;

        // OPTIMIZATION: Register multiple buffers with kernel (IORING_REGISTER_BUFFERS)
        // CRITICAL FIX: Register one buffer per queue depth to eliminate DMA mapping overhead!
        // Each operation gets its own buffer, so kernel doesn't need to map/unmap per operation
        let buffer_iovecs: Vec<libc::iovec> = self
            .buffers
            .iter()
            .map(|buf| libc::iovec {
                iov_base: buf.as_ptr() as *mut libc::c_void,
                iov_len: buf.len(),
            })
            .collect();

        let use_fixed_buffers =
            unsafe { ring.submitter().register_buffers(&buffer_iovecs).is_ok() };

        if use_fixed_buffers {
            eprintln!(
                "Successfully registered {} fixed buffers",
                self.buffers.len()
            );
        } else {
            eprintln!("Warning: Fixed buffers registration failed, falling back to standard I/O");
        }

        // OPTIMIZATION: Register file descriptor (IORING_REGISTER_FILES)
        // This reduces fd lookup overhead per operation
        let use_fixed_files = unsafe { ring.submitter().register_files(&[fd]).is_ok() };

        if use_fixed_files {
            eprintln!("Successfully registered fixed file descriptor");
        } else {
            eprintln!(
                "Warning: Fixed file registration failed - will use fget per operation (slow!)"
            );
        }

        // Fallback to non-fixed if registration fails
        if !use_fixed_buffers || !use_fixed_files {
            eprintln!(
                "Warning: Fixed buffers/files registration failed, falling back to standard I/O"
            );
        }

        // Track which buffer index to use next (round-robin assignment)
        let mut next_buf_index = 0usize;

        // CRITICAL OPTIMIZATION: Fast path for sequential reads (100% reads)
        // Avoid Mutex locks and function call overhead in hot path
        let is_sequential_reads = self.read_percent == 100
            && matches!(self.pattern.mode(), crate::config::IoMode::Sequential);
        let block_size_u64 = self.block_size as u64;
        let device_size = self.pattern.device_size();
        let buffers_len = self.buffers.len();

        let start = Instant::now();
        let deadline = start + duration; // Calculate deadline once to avoid repeated elapsed() calls
        let mut offset = 0u64;
        let mut pending_ops = 0usize; // Operations in-flight (submitted to kernel)
        let mut queued_ops = 0usize; // Operations queued but not yet submitted
                                     // Use circular buffer for timestamps (pre-allocated, no reallocation)
        let timestamp_capacity = self.queue_depth * 2;
        let mut op_timestamps_circular: Vec<(Instant, bool)> =
            vec![(Instant::now(), true); timestamp_capacity];
        let mut timestamp_head = 0usize;

        // OPTIMIZATION: Latency sampling - track only 1% of operations to reduce overhead
        let latency_sample_rate = 100; // Track 1 in 100 operations
        let mut op_counter = 0u64;

        // CRITICAL: Cache elapsed time check to avoid clock_gettime overhead (30%!)
        // Only check time every N iterations instead of every iteration
        let mut elapsed_check_counter = 0u64;
        const ELAPSED_CHECK_INTERVAL: u64 = 1000; // Check every 1000 iterations

        // Pre-calculate power-of-2 checks to avoid repeated calls in hot path
        let timestamp_capacity_is_pow2 = timestamp_capacity.is_power_of_two();
        let buffers_len_is_pow2 = buffers_len.is_power_of_two();
        let timestamp_mask = timestamp_capacity - 1;
        let buffers_mask = buffers_len - 1;

        // Initial fill: submit up to queue depth
        let init_time = Instant::now();
        while (pending_ops + queued_ops) < self.queue_depth && Instant::now() < deadline {
            // Use fast path for sequential reads
            let is_read = if is_sequential_reads {
                true
            } else {
                self.pattern.is_read(self.read_percent)
            };
            offset = if is_sequential_reads {
                let next = offset + block_size_u64;
                if next >= device_size {
                    0
                } else {
                    next
                }
            } else {
                self.pattern.next_offset(offset)
            };

            // Store in circular buffer only if needed (for latency tracking or mixed reads/writes)
            if !is_sequential_reads
                || ((pending_ops + queued_ops) % latency_sample_rate as usize == 0)
            {
                op_timestamps_circular[pending_ops + queued_ops] = (init_time, is_read);
            }

            // OPTIMIZATION: Use ReadFixed/WriteFixed with registered buffers and files
            // Use round-robin buffer assignment - each operation gets its own buffer
            let buf_index = if use_fixed_buffers {
                let idx = next_buf_index % self.buffers.len();
                next_buf_index = (next_buf_index + 1) % self.buffers.len();
                idx as u16
            } else {
                0u16 // Not used if fixed buffers not available
            };

            if is_read {
                let read_e = if use_fixed_buffers && use_fixed_files {
                    opcode::ReadFixed::new(
                        types::Fixed(0),
                        self.buffers[buf_index as usize].as_mut_ptr() as *mut _,
                        self.buffers[buf_index as usize].len() as u32,
                        buf_index,
                    )
                    .offset(offset)
                    .build()
                } else {
                    opcode::Read::new(
                        types::Fd(fd),
                        self.buffers[0].as_mut_ptr() as *mut _,
                        self.buffers[0].len() as u32,
                    )
                    .offset(offset)
                    .build()
                };

                unsafe {
                    ring.submission()
                        .push(&read_e)
                        .map_err(|_| anyhow::anyhow!("Failed to push read operation"))?;
                }
            } else {
                let write_e = if use_fixed_buffers && use_fixed_files {
                    opcode::WriteFixed::new(
                        types::Fixed(0),
                        self.buffers[buf_index as usize].as_ptr(),
                        self.buffers[buf_index as usize].len() as u32,
                        buf_index,
                    )
                    .offset(offset)
                    .build()
                } else {
                    opcode::Write::new(
                        types::Fd(fd),
                        self.buffers[0].as_ptr(),
                        self.buffers[0].len() as u32,
                    )
                    .offset(offset)
                    .build()
                };

                unsafe {
                    ring.submission()
                        .push(&write_e)
                        .map_err(|_| anyhow::anyhow!("Failed to push write operation"))?;
                }
            }

            queued_ops += 1;
        }
        // Submit initial batch
        ring.submit()?;
        pending_ops += queued_ops;
        queued_ops = 0;

        // Main loop: keep queue full at all times (like fio does)
        // Continue using the circular buffer initialized above
        // CRITICAL FIX: Cache elapsed check to avoid 30% clock_gettime overhead!
        loop {
            // Check elapsed time only occasionally (every N iterations) to avoid overhead
            elapsed_check_counter += 1;
            if elapsed_check_counter >= ELAPSED_CHECK_INTERVAL {
                elapsed_check_counter = 0;
                if self.stop_flag.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    break;
                }
            }

            // Process completions first (non-blocking) - process ALL available
            let cq = ring.completion();
            let mut completed_count = 0;
            // CRITICAL OPTIMIZATION: Batch stats updates to reduce atomic operation overhead
            // Accumulate stats locally, then update atomics once per batch
            let mut batch_bytes_read = 0u64;
            let mut batch_bytes_written = 0u64;
            let mut batch_ops = 0u64;
            let mut batch_failed = 0u64;

            for cqe in cq {
                if cqe.result() >= 0 {
                    let bytes = cqe.result() as usize;
                    // OPTIMIZATION: Sample latency tracking (only 1% of operations)
                    // This reduces overhead significantly while still providing useful metrics
                    let track_latency = (op_counter % latency_sample_rate) == 0;
                    op_counter += 1;

                    if track_latency {
                        // Only call clock_gettime when we actually need it (1% of ops)
                        let now = Instant::now();
                        let idx = if timestamp_capacity_is_pow2 {
                            (timestamp_head + completed_count) & timestamp_mask
                        } else {
                            (timestamp_head + completed_count) % timestamp_capacity
                        };
                        let (op_start, is_read) = op_timestamps_circular[idx];
                        let latency_ns = now.duration_since(op_start).as_nanos() as u64;
                        self.stats.record_op(bytes, latency_ns, is_read);
                        // Also count in batch for ops_completed
                        batch_ops += 1;
                    } else {
                        // CRITICAL OPTIMIZATION: Fast path - skip circular buffer lookup!
                        // For sequential reads (100% reads), we know is_read is always true
                        if is_sequential_reads {
                            batch_bytes_read += bytes as u64;
                        } else {
                            // Only lookup when we have mixed reads/writes
                            let idx = if timestamp_capacity_is_pow2 {
                                (timestamp_head + completed_count) & timestamp_mask
                            } else {
                                (timestamp_head + completed_count) % timestamp_capacity
                            };
                            let is_read = op_timestamps_circular
                                .get(idx)
                                .map(|(_, r)| *r)
                                .unwrap_or(true);
                            if is_read {
                                batch_bytes_read += bytes as u64;
                            } else {
                                batch_bytes_written += bytes as u64;
                            }
                        }
                        batch_ops += 1;
                    }
                } else {
                    batch_failed += 1;
                }
                completed_count += 1;
                pending_ops -= 1;
            }

            // Update atomics once per batch (much faster than per-operation updates)
            if batch_bytes_read > 0 {
                self.stats
                    .bytes_read
                    .fetch_add(batch_bytes_read, Ordering::Relaxed);
            }
            if batch_bytes_written > 0 {
                self.stats
                    .bytes_written
                    .fetch_add(batch_bytes_written, Ordering::Relaxed);
            }
            if batch_ops > 0 {
                self.stats
                    .ops_completed
                    .fetch_add(batch_ops, Ordering::Relaxed);
            }
            if batch_failed > 0 {
                self.stats
                    .ops_failed
                    .fetch_add(batch_failed, Ordering::Relaxed);
            }

            // Update circular buffer head
            if completed_count > 0 {
                timestamp_head = (timestamp_head + completed_count) % timestamp_capacity;
            }

            // CRITICAL: Immediately refill queue to keep it FULL at all times!
            // Perf shows 52% time in schedule/blocking - we MUST keep queue full
            // Fill submission queue (but don't submit immediately - batch submissions)
            // Only get batch_start_time when we need it (for latency tracking - 1% of ops)
            // Check if next operation will need latency tracking
            let need_batch_time = (op_counter % latency_sample_rate) == 0;
            let batch_start_time = if need_batch_time {
                Instant::now()
            } else {
                start // Dummy value, won't be used in fast path
            };

            // Fill submission queue until we have enough in-flight + queued operations
            while (pending_ops + queued_ops) < self.queue_depth {
                // Skip deadline check in inner loop - already checked in outer loop

                // CRITICAL OPTIMIZATION: Fast path for sequential reads
                // Avoid Mutex locks and function calls in hot path
                let is_read = if is_sequential_reads {
                    true // Always read for 100% reads
                } else {
                    self.pattern.is_read(self.read_percent)
                };

                // Inline sequential offset calculation to avoid function call overhead
                offset = if is_sequential_reads {
                    let next = offset + block_size_u64;
                    if next >= device_size {
                        0
                    } else {
                        next
                    }
                } else {
                    self.pattern.next_offset(offset)
                };

                // CRITICAL OPTIMIZATION: Only store timestamps when we need them (1% of ops)
                // This eliminates 99% of circular buffer writes for sequential reads
                if need_batch_time {
                    let idx = if timestamp_capacity_is_pow2 {
                        (timestamp_head + pending_ops) & timestamp_mask
                    } else {
                        (timestamp_head + pending_ops) % timestamp_capacity
                    };
                    op_timestamps_circular[idx] = (batch_start_time, is_read);
                } else if !is_sequential_reads {
                    // For mixed reads/writes, we still need to track is_read for stats
                    // But we can skip the timestamp (we don't need it for non-latency tracking)
                    let idx = if timestamp_capacity_is_pow2 {
                        (timestamp_head + pending_ops) & timestamp_mask
                    } else {
                        (timestamp_head + pending_ops) % timestamp_capacity
                    };
                    // Only store is_read flag, use dummy timestamp
                    op_timestamps_circular[idx] = (start, is_read);
                }
                // For sequential reads without latency tracking: skip circular buffer entirely!

                // OPTIMIZATION: Use ReadFixed/WriteFixed with registered buffers and files
                // Use round-robin buffer assignment - each operation gets its own buffer
                // Use pre-calculated mask for power-of-2 optimization
                let buf_index = if use_fixed_buffers {
                    let idx = if buffers_len_is_pow2 {
                        next_buf_index & buffers_mask
                    } else {
                        next_buf_index % buffers_len
                    };
                    next_buf_index = if buffers_len_is_pow2 {
                        (next_buf_index + 1) & buffers_mask
                    } else {
                        (next_buf_index + 1) % buffers_len
                    };
                    idx as u16
                } else {
                    0u16 // Not used if fixed buffers not available
                };

                if is_read {
                    let read_e = if use_fixed_buffers && use_fixed_files {
                        opcode::ReadFixed::new(
                            types::Fixed(0),
                            self.buffers[buf_index as usize].as_mut_ptr() as *mut _,
                            self.buffers[buf_index as usize].len() as u32,
                            buf_index,
                        )
                        .offset(offset)
                        .build()
                    } else {
                        opcode::Read::new(
                            types::Fd(fd),
                            self.buffers[0].as_mut_ptr() as *mut _,
                            self.buffers[0].len() as u32,
                        )
                        .offset(offset)
                        .build()
                    };

                    unsafe {
                        ring.submission()
                            .push(&read_e)
                            .map_err(|_| anyhow::anyhow!("Failed to push read operation"))?;
                    }
                } else {
                    let write_e = if use_fixed_buffers && use_fixed_files {
                        opcode::WriteFixed::new(
                            types::Fixed(0),
                            self.buffers[buf_index as usize].as_ptr(),
                            self.buffers[buf_index as usize].len() as u32,
                            buf_index,
                        )
                        .offset(offset)
                        .build()
                    } else {
                        opcode::Write::new(
                            types::Fd(fd),
                            self.buffers[0].as_ptr(),
                            self.buffers[0].len() as u32,
                        )
                        .offset(offset)
                        .build()
                    };

                    unsafe {
                        ring.submission()
                            .push(&write_e)
                            .map_err(|_| anyhow::anyhow!("Failed to push write operation"))?;
                    }
                }

                queued_ops += 1;
            }

            // CRITICAL OPTIMIZATION: Batch submissions to reduce syscall overhead!
            // Perf shows 40% syscall overhead - we're submitting too frequently
            // Strategy: Only submit when we have a significant batch (>= 8 ops) OR queue is getting full
            // This reduces syscall frequency from every iteration to every 8+ operations
            let should_submit = queued_ops >= 8 || // Significant batch ready
                               (pending_ops + queued_ops) >= self.queue_depth; // Queue full

            if should_submit && queued_ops > 0 {
                ring.submit()?;
                pending_ops += queued_ops;
                queued_ops = 0;
            }

            // CRITICAL FIX: Minimize blocking!
            // Perf shows 52% time in schedule/blocking - we MUST avoid waiting
            // Strategy: Only wait when queue is critically low (< 8)
            // If queue is full, just continue loop - don't wait!
            if pending_ops < 8 && pending_ops > 0 {
                // Queue is critically low, must wait for completions
                ring.submit_and_wait(1)?;
            }
            // Otherwise: don't wait! Continue loop to check for completions non-blocking
            // This keeps CPU busy and avoids blocking/sleeping
        }

        // Wait for remaining operations
        let final_time = Instant::now();
        while pending_ops > 0 {
            ring.submit_and_wait(1)?;
            let cq = ring.completion();
            let mut completed_count = 0;
            for cqe in cq {
                if cqe.result() >= 0 {
                    let bytes = cqe.result() as usize;
                    let idx = (timestamp_head + completed_count) % timestamp_capacity;
                    let (op_start, is_read) = op_timestamps_circular[idx];
                    let latency_ns = final_time.duration_since(op_start).as_nanos() as u64;
                    self.stats.record_op(bytes, latency_ns, is_read);
                } else {
                    self.stats.ops_failed.fetch_add(1, Ordering::Relaxed);
                }
                completed_count += 1;
                pending_ops -= 1;
            }
            if completed_count > 0 {
                timestamp_head = (timestamp_head + completed_count) % timestamp_capacity;
            }
        }

        Ok(())
    }
}
