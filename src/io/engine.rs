use crate::config::{Config, IoMode, Workload};
use crate::io::{Device, IoWorker};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
        // Handle "all" workload by running all workloads sequentially
        if self.config.workload == Workload::All {
            return self.run_all_workloads();
        }

        let stop_flag = Arc::new(AtomicBool::new(false));

        // Create shared stats collection
        let workers_stats = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut workers_final = Vec::new();

        // Pre-create stats for all workers
        let read_percent = self.config.workload.read_percent();
        for _ in 0..self.config.threads {
            let stats = Arc::new(crate::io::worker::WorkerStats::new());
            workers_stats.lock().unwrap().push(Arc::clone(&stats));
            workers_final.push(stats);
        }

        // Start monitoring thread if enabled
        let monitor_handle = if self.config.monitor {
            let stats_for_monitor = Arc::clone(&workers_stats);
            let stop_monitor = Arc::clone(&stop_flag);
            let duration = self.config.duration;

            Some(thread::spawn(move || {
                use std::io::{self, Write};
                let interval = Duration::from_millis(1000); // Update every 1 second
                let start = std::time::Instant::now();
                let mut last_bytes_read = 0u64;
                let mut last_bytes_written = 0u64;
                let mut last_ops = 0u64;
                let mut last_time = start;

                while !stop_monitor.load(Ordering::Relaxed) && start.elapsed() < duration {
                    thread::sleep(interval);

                    let stats = stats_for_monitor.lock().unwrap();
                    let mut total_bytes_read = 0u64;
                    let mut total_bytes_written = 0u64;
                    let mut total_ops = 0u64;

                    for s in stats.iter() {
                        total_bytes_read += s.bytes_read.load(Ordering::Relaxed);
                        total_bytes_written += s.bytes_written.load(Ordering::Relaxed);
                        total_ops += s.ops_completed.load(Ordering::Relaxed);
                    }

                    let now = std::time::Instant::now();
                    let elapsed_total = start.elapsed().as_secs_f64();
                    let elapsed_interval = now.duration_since(last_time).as_secs_f64();

                    if elapsed_interval > 0.0 {
                        // Calculate per-second rate (delta since last update)
                        let bytes_read_delta = total_bytes_read.saturating_sub(last_bytes_read);
                        let bytes_written_delta =
                            total_bytes_written.saturating_sub(last_bytes_written);
                        let ops_delta = total_ops.saturating_sub(last_ops);

                        let throughput_read =
                            (bytes_read_delta as f64 / elapsed_interval) / (1024.0 * 1024.0);
                        let throughput_write =
                            (bytes_written_delta as f64 / elapsed_interval) / (1024.0 * 1024.0);
                        let iops = ops_delta as f64 / elapsed_interval;

                        // Also show cumulative average
                        let avg_throughput_read = if elapsed_total > 0.0 {
                            (total_bytes_read as f64 / elapsed_total) / (1024.0 * 1024.0)
                        } else {
                            0.0
                        };
                        let avg_iops = if elapsed_total > 0.0 {
                            total_ops as f64 / elapsed_total
                        } else {
                            0.0
                        };

                        print!("\r[{}s] Read: {:.2} MB/s (avg: {:.2}), Write: {:.2} MB/s, IOPS: {:.0} (avg: {:.0})     ", 
                               elapsed_total as u64, throughput_read, avg_throughput_read, throughput_write, iops, avg_iops);
                        io::stdout().flush().ok();

                        last_bytes_read = total_bytes_read;
                        last_bytes_written = total_bytes_written;
                        last_ops = total_ops;
                        last_time = now;
                    }
                }
                println!(); // New line after monitoring
            }))
        } else {
            None
        };

        // Spawn worker threads - each worker will use its pre-allocated stats
        let mut worker_handles = Vec::new();
        for i in 0..self.config.threads {
            let device_clone = Arc::clone(&self.device);
            let workload_mode: IoMode = self.config.workload.into();
            let block_size = self.config.block_size;
            let queue_depth = self.config.queue_depth;
            let read_percent = self.config.workload.read_percent();
            let duration = self.config.duration;
            let worker_stats = Arc::clone(&workers_final[i]);

            let handle = thread::spawn(move || {
                let mut worker = IoWorker::new_with_read_percent(
                    device_clone,
                    workload_mode,
                    block_size,
                    queue_depth,
                    read_percent,
                );
                // Replace worker's internal stats with shared stats
                worker.set_stats(worker_stats);
                worker.run(duration).unwrap();
            });

            worker_handles.push(handle);
        }

        // Wait for all workers to complete
        for handle in worker_handles {
            handle.join().unwrap();
        }

        stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = monitor_handle {
            handle.join().unwrap();
        }

        // Aggregate statistics
        let mut total_bytes_read = 0u64;
        let mut total_bytes_written = 0u64;
        let mut total_ops = 0u64;
        let mut total_latency_ns = 0u64;
        let mut min_latency_ns = u64::MAX;
        let mut max_latency_ns = 0u64;

        for stats in workers_final {
            total_bytes_read += stats.bytes_read.load(std::sync::atomic::Ordering::Relaxed);
            total_bytes_written += stats
                .bytes_written
                .load(std::sync::atomic::Ordering::Relaxed);
            total_ops += stats
                .ops_completed
                .load(std::sync::atomic::Ordering::Relaxed);
            total_latency_ns += stats
                .total_latency_ns
                .load(std::sync::atomic::Ordering::Relaxed);

            let min = stats
                .min_latency_ns
                .load(std::sync::atomic::Ordering::Relaxed);
            if min < min_latency_ns {
                min_latency_ns = min;
            }

            let max = stats
                .max_latency_ns
                .load(std::sync::atomic::Ordering::Relaxed);
            if max > max_latency_ns {
                max_latency_ns = max;
            }
        }

        let duration_secs = self.config.duration.as_secs_f64();
        let throughput_read_mbps = (total_bytes_read as f64 / duration_secs) / (1024.0 * 1024.0);
        let throughput_write_mbps =
            (total_bytes_written as f64 / duration_secs) / (1024.0 * 1024.0);
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

    /// Run all workloads sequentially
    fn run_all_workloads(&self) -> Result<BenchmarkResults> {
        let workloads = [
            Workload::SeqRead,
            Workload::SeqWrite,
            Workload::RandRead,
            Workload::RandWrite,
            Workload::Seq,
            Workload::Rand,
        ];

        let mut combined_results = BenchmarkResults {
            total_bytes_read: 0,
            total_bytes_written: 0,
            total_ops: 0,
            failed_ops: 0,
            duration: Duration::ZERO,
            throughput_read_mbps: 0.0,
            throughput_write_mbps: 0.0,
            iops: 0.0,
            avg_latency_us: 0.0,
            min_latency_us: f64::MAX,
            max_latency_us: 0.0,
        };

        for workload in workloads.iter() {
            println!("\n=== Running workload: {:?} ===", workload);
            let mut config = self.config.clone();
            config.workload = *workload;

            let engine = IoEngine::new(config)?;
            let results = engine.run()?;

            combined_results.total_bytes_read += results.total_bytes_read;
            combined_results.total_bytes_written += results.total_bytes_written;
            combined_results.total_ops += results.total_ops;
            combined_results.failed_ops += results.failed_ops;
            combined_results.duration += results.duration;

            if results.min_latency_us < combined_results.min_latency_us {
                combined_results.min_latency_us = results.min_latency_us;
            }
            if results.max_latency_us > combined_results.max_latency_us {
                combined_results.max_latency_us = results.max_latency_us;
            }
        }

        let duration_secs = combined_results.duration.as_secs_f64();
        combined_results.throughput_read_mbps =
            (combined_results.total_bytes_read as f64 / duration_secs) / (1024.0 * 1024.0);
        combined_results.throughput_write_mbps =
            (combined_results.total_bytes_written as f64 / duration_secs) / (1024.0 * 1024.0);
        combined_results.iops = combined_results.total_ops as f64 / duration_secs;
        combined_results.avg_latency_us = if combined_results.total_ops > 0 {
            // Approximate average latency
            (combined_results.total_ops as f64 / duration_secs) / 1000.0
        } else {
            0.0
        };

        Ok(combined_results)
    }
}
