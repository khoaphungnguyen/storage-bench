use crate::io::{Device, IoWorker};
use crate::config::{Config, IoMode};
use crate::io::worker::WorkerStats;
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;

/// Main I/O engine that coordinates workers
pub struct IoEngine {
    device: Arc<Device>,
    config: Config,
}

#[derive(Debug)]
pub struct BenchmarkResults {
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub total_ops: u64,
    pub failed_ops: u64,
    pub duration: Duration,
    pub throughput_read_mbps: f64,
    pub throughput_write_mbps: f64,
    pub iops: f64,
    pub avg_latency_us: f64,
    pub min_latency_us: f64,
    pub max_latency_us: f64,
}

impl IoEngine {
    pub fn new(config: Config) -> Result<Self> {
        let device = Arc::new(Device::open(&config.device)?);
        Ok(Self { device, config })
    }
    
    /// Run benchmark
    pub fn run(&self) -> Result<BenchmarkResults> {
        let mut workers = Vec::new();
        let mut handles = Vec::new();
        
        // Create worker threads
        for _ in 0..self.config.threads {
            let mut worker = IoWorker::new(
                Arc::clone(&self.device),
                self.config.mode,
                self.config.block_size,
                self.config.queue_depth,
            );
            let stats = worker.stats();
            let stop_flag = worker.stop_flag();
            let duration = self.config.duration;
            
            let handle = std::thread::spawn(move || {
                let mut w = worker;
                w.run(duration).unwrap();
            });
            
            handles.push(handle);
            workers.push(stats);
        }
        
        // Wait for all workers to complete
        for handle in handles {
            handle.join().unwrap();
        }
        
        // Aggregate statistics
        let mut total_bytes_read = 0u64;
        let mut total_bytes_written = 0u64;
        let mut total_ops = 0u64;
        let mut total_latency_ns = 0u64;
        let mut min_latency_ns = u64::MAX;
        let mut max_latency_ns = 0u64;
        
        for stats in workers {
            total_bytes_read += stats.bytes_read.load(std::sync::atomic::Ordering::Relaxed);
            total_bytes_written += stats.bytes_written.load(std::sync::atomic::Ordering::Relaxed);
            total_ops += stats.ops_completed.load(std::sync::atomic::Ordering::Relaxed);
            total_latency_ns += stats.total_latency_ns.load(std::sync::atomic::Ordering::Relaxed);
            
            let min = stats.min_latency_ns.load(std::sync::atomic::Ordering::Relaxed);
            if min < min_latency_ns {
                min_latency_ns = min;
            }
            
            let max = stats.max_latency_ns.load(std::sync::atomic::Ordering::Relaxed);
            if max > max_latency_ns {
                max_latency_ns = max;
            }
        }
        
        let duration_secs = self.config.duration.as_secs_f64();
        let throughput_read_mbps = (total_bytes_read as f64 / duration_secs) / (1024.0 * 1024.0);
        let throughput_write_mbps = (total_bytes_written as f64 / duration_secs) / (1024.0 * 1024.0);
        let iops = total_ops as f64 / duration_secs;
        let avg_latency_us = if total_ops > 0 {
            (total_latency_ns / total_ops) as f64 / 1000.0
        } else {
            0.0
        };
        
        Ok(BenchmarkResults {
            total_bytes_read,
            total_bytes_written,
            total_ops,
            failed_ops: 0,
            duration: self.config.duration,
            throughput_read_mbps,
            throughput_write_mbps,
            iops,
            avg_latency_us,
            min_latency_us: min_latency_ns as f64 / 1000.0,
            max_latency_us: max_latency_ns as f64 / 1000.0,
        })
    }
}

