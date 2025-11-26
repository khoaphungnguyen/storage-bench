use crate::io::Device;
use crate::io::patterns::IoPattern;
use crate::config::IoMode;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;

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
            self.bytes_written.fetch_add(bytes as u64, Ordering::Relaxed);
        }
        
        self.ops_completed.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);
        
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

/// I/O worker thread
pub struct IoWorker {
    device: Arc<Device>,
    pattern: Arc<IoPattern>,
    stats: Arc<WorkerStats>,
    stop_flag: Arc<AtomicBool>,
    block_size: usize,
    queue_depth: usize,
}

impl IoWorker {
    pub fn new(
        device: Arc<Device>,
        mode: IoMode,
        block_size: usize,
        queue_depth: usize,
    ) -> Self {
        let device_size = device.size();
        Self {
            device,
            pattern: Arc::new(IoPattern::new(mode, block_size, device_size)),
            stats: Arc::new(WorkerStats::new()),
            stop_flag: Arc::new(AtomicBool::new(false)),
            block_size,
            queue_depth,
        }
    }
    
    pub fn stats(&self) -> Arc<WorkerStats> {
        Arc::clone(&self.stats)
    }
    
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_flag)
    }
    
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
    
    /// Run the worker (blocking)
    pub fn run(&mut self, duration: Duration) -> Result<()> {
        let start = Instant::now();
        let mut offset = 0u64;
        
        // TODO: Implement actual I/O operations using io_uring or async I/O
        // This is a placeholder implementation
        while !self.stop_flag.load(Ordering::Relaxed) && start.elapsed() < duration {
            let op_start = Instant::now();
            
            // Simulate I/O operation
            std::thread::sleep(Duration::from_micros(10));
            
            let latency_ns = op_start.elapsed().as_nanos() as u64;
            let is_read = self.pattern.is_read(100); // 100% reads for now
            
            self.stats.record_op(self.block_size, latency_ns, is_read);
            
            offset = self.pattern.next_offset(offset);
        }
        
        Ok(())
    }
}

