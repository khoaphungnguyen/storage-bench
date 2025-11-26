# Storage Bench

A high-performance storage benchmarking tool with built-in bottleneck detection and automatic parameter optimization.

## Features

- **High-performance I/O**: Direct device access with `O_DIRECT` and `io_uring` support
- **Real-time monitoring**: CPU, memory, NUMA, and I/O statistics
- **Bottleneck detection**: Automatic identification of performance bottlenecks
- **Parameter optimization**: Adaptive tuning to maximize performance
- **Multiple I/O patterns**: Sequential, random, and mixed workloads

## Goals

- Achieve maximum storage performance (e.g., 300GB/s sequential, 80M IOPS random)
- Automatically detect bottlenecks during testing
- Reduce manual parameter tuning from thousands of iterations to automated optimization

## Requirements

- Linux (for `io_uring` and direct device access)
- Rust 1.70+
- Root access (for direct device access)

## Building

```bash
cargo build --release
```

## Usage

### Basic benchmark

```bash
sudo ./target/release/storage-bench run --device /dev/nvme0n1 --mode sequential
```

### With optimization

```bash
sudo ./target/release/storage-bench run \
    --device /dev/nvme0n1 \
    --mode random \
    --optimize \
    --monitor
```

### List devices

```bash
./target/release/storage-bench list
```

### System information

```bash
./target/release/storage-bench info
```

## Architecture

- **I/O Engine**: Handles direct device I/O with multiple worker threads
- **Monitor Engine**: Collects CPU, memory, NUMA, and I/O metrics
- **Optimizer Engine**: Detects bottlenecks and adjusts parameters automatically

## Status

🚧 **Work in Progress** 🚧

This is an early-stage project. Core functionality is being implemented.

## License

MIT OR Apache-2.0

