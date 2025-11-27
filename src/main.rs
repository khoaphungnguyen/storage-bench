use crate::io::engine::{BenchmarkResults, IoEngine};
use crate::io::Device;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
mod io;
mod monitor;
mod optimizer;

use config::Config;

#[derive(Parser)]
#[command(name = "storage-bench")]
#[command(about = "High-performance storage benchmarking tool with built-in bottleneck detection", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a benchmark test
    Run {
        /// Path to storage device (e.g., /dev/nvme0n1)
        #[arg(short, long)]
        device: PathBuf,

        /// Workload type: seqread, seqwrite, randread, randwrite, seq, rand, all
        #[arg(short, long, default_value = "seqread")]
        workload: String,

        /// Block size (e.g., 4k, 8k, 16k, 32k, 64k, 128k, 256k, 512k, 1m, 2m)
        /// Default: 128k for sequential workloads, 4k for random workloads
        #[arg(short = 'b', long)]
        block_size: Option<String>,

        /// Queue depth
        #[arg(short = 'q', long, default_value = "32")]
        queue_depth: usize,

        /// Number of worker threads
        #[arg(short = 'n', long)]
        threads: Option<usize>,

        /// Test duration in seconds
        #[arg(short = 't', long, default_value = "60")]
        duration: u64,

        /// Enable automatic optimization
        #[arg(short = 'O', long)]
        optimize: bool,

        /// Enable real-time monitoring
        #[arg(short = 'm', long)]
        monitor: bool,
    },
    /// List available storage devices
    List,
    /// Show system information
    Info,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            device,
            workload,
            block_size,
            queue_depth,
            threads,
            duration,
            optimize,
            monitor,
        } => {
            let workload_parsed: crate::config::Workload = workload.parse()?;
            // Determine default block size based on workload
            let default_block_size = if workload_parsed.is_sequential() {
                "128k"
            } else {
                "4k"
            };
            let block_size_str = block_size.as_deref().unwrap_or(default_block_size);
            let block_size_bytes = crate::config::parse_block_size(block_size_str)?;

            let config = Config {
                device: device.clone(),
                workload: workload_parsed,
                block_size: block_size_bytes,
                queue_depth,
                threads: threads.unwrap_or(1),
                duration: std::time::Duration::from_secs(duration),
                optimize,
                monitor,
            };

            println!("Starting benchmark...");
            println!("Device: {:?}", config.device);
            println!("Workload: {:?}", config.workload);
            println!("Block size: {} ({})", block_size_str, config.block_size);
            println!("Queue depth: {}", config.queue_depth);
            println!("Threads: {}", config.threads);
            println!("Duration: {} seconds", duration);
            println!("I/O Engine: io_uring");

            run_benchmark(config).await?;
        }
        Commands::List => {
            list_devices().await?;
        }
        Commands::Info => {
            show_system_info().await?;
        }
    }

    Ok(())
}

async fn run_benchmark(config: Config) -> anyhow::Result<()> {
    let engine = IoEngine::new(config.clone())?;
    let results = engine.run()?;

    print_results(&results);

    Ok(())
}

async fn list_devices() -> anyhow::Result<()> {
    let devices = Device::list_devices()?;

    println!("Available storage devices:\n");
    println!(
        "{:<20} {:<15} {:<30} {:<15} {:<20}",
        "Device", "Size (GB)", "Model", "Type", "Link Speed"
    );
    println!("{}", "-".repeat(100));

    for device in devices {
        let size_gb = device.size as f64 / (1024.0 * 1024.0 * 1024.0);
        let model = device.model.as_deref().unwrap_or("N/A");
        let device_type = device.device_type.as_deref().unwrap_or("Unknown");
        let link_speed = device.link_speed.as_deref().unwrap_or("N/A");

        println!(
            "{:<20} {:<15.2} {:<30} {:<15} {:<20}",
            device.path.display(),
            size_gb,
            model,
            device_type,
            link_speed
        );
    }

    Ok(())
}

async fn show_system_info() -> anyhow::Result<()> {
    use crate::monitor::{CpuMonitor, MemoryMonitor, NumaMonitor};

    println!("System Information\n");
    println!("{}", "=".repeat(50));

    // CPU Info
    let mut cpu_monitor = CpuMonitor::new();
    let cpu_metrics = cpu_monitor.collect();
    println!("\nCPU:");
    println!("  Cores: {}", cpu_metrics.utilization_per_core.len());
    println!("  Threads: {}", num_cpus::get());
    println!("  Avg Utilization: {:.2}%", cpu_metrics.avg_utilization);

    // Memory Info
    let mut mem_monitor = MemoryMonitor::new();
    let mem_metrics = mem_monitor.collect();
    println!("\nMemory:");
    println!("  Total: {:.2} GB", mem_metrics.total_bytes as f64 / 1e9);
    println!(
        "  Available: {:.2} GB",
        mem_metrics.available_bytes as f64 / 1e9
    );

    // NUMA Info
    let mut numa_monitor = NumaMonitor::new()?;
    let numa_metrics = numa_monitor.collect()?;
    println!("\nNUMA:");
    println!("  Nodes: {}", numa_metrics.num_nodes);
    for (i, (cpus, mem)) in numa_metrics
        .node_cpus
        .iter()
        .zip(numa_metrics.node_memory.iter())
        .enumerate()
    {
        println!(
            "  Node {}: {} CPUs, {:.2} GB",
            i,
            cpus.len(),
            *mem as f64 / 1e9
        );
    }

    // Device Info
    println!("\nStorage Devices:");
    let devices = Device::list_devices()?;
    for device in devices {
        let size_gb = device.size as f64 / (1024.0 * 1024.0 * 1024.0);
        println!("  {}: {:.2} GB", device.path.display(), size_gb);
    }

    Ok(())
}

fn print_results(results: &BenchmarkResults) {
    println!("\n{}", "=".repeat(70));
    println!("Benchmark Results");
    println!("{}", "=".repeat(70));

    println!("\nDuration: {:.2} seconds", results.duration.as_secs_f64());
    println!("\nOperations:");
    println!("  Total operations: {}", results.total_ops);
    println!("  Failed operations: {}", results.failed_ops);
    println!("  IOPS: {:.2}", results.iops);

    println!("\nThroughput:");
    println!(
        "  Read:  {:.2} MB/s ({:.2} GB/s)",
        results.throughput_read_mbps,
        results.throughput_read_mbps / 1024.0
    );
    println!(
        "  Write: {:.2} MB/s ({:.2} GB/s)",
        results.throughput_write_mbps,
        results.throughput_write_mbps / 1024.0
    );

    println!("\nLatency:");
    println!("  Average: {:.2} μs", results.avg_latency_us);
    println!("  Min:     {:.2} μs", results.min_latency_us);
    println!("  Max:     {:.2} μs", results.max_latency_us);

    println!("\nData:");
    println!(
        "  Bytes read:    {} ({:.2} GB)",
        results.total_bytes_read,
        results.total_bytes_read as f64 / 1e9
    );
    println!(
        "  Bytes written: {} ({:.2} GB)",
        results.total_bytes_written,
        results.total_bytes_written as f64 / 1e9
    );

    println!("{}", "=".repeat(70));
}
