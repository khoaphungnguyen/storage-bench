# Storage Bench

A high-performance storage benchmarking tool with built-in bottleneck detection and automatic parameter optimization. Built with Rust and optimized for Linux using `io_uring` for maximum I/O performance.

## Features

- **High-performance I/O**: Direct device access with `O_DIRECT` and `io_uring` support
  - Fixed buffers (IORING_REGISTER_BUFFERS) for zero-copy DMA operations
  - Fixed files (IORING_REGISTER_FILES) to eliminate file descriptor lookup overhead
  - Batched submissions to minimize syscall overhead
  - Optimized hot path for sequential I/O workloads
- **Real-time monitoring**: CPU, memory, NUMA, and I/O statistics
- **Bottleneck detection**: Automatic identification of performance bottlenecks
- **Parameter optimization**: Adaptive tuning to maximize performance
- **Multiple I/O patterns**: Sequential, random, and mixed workloads
- **Performance optimizations**: 
  - Eliminated DMA mapping overhead (< 1%)
  - Reduced syscall overhead (~15%)
  - Optimized time checks (< 1% clock overhead)
  - Batched stats updates
  - Fast path for sequential reads

## Performance

Current performance characteristics (sequential reads, 128k blocks, queue depth 128):
- **Throughput**: ~1186 MB/s
- **IOPS**: ~9,500 IOPS
- **CPU overhead**: Minimized through comprehensive optimizations

### Optimizations Implemented

1. **Fixed Buffers**: Register multiple buffers with kernel to eliminate DMA mapping overhead
2. **Fixed Files**: Register file descriptor to eliminate fget overhead
3. **Batched Submissions**: Submit operations in batches to reduce syscall frequency
4. **Optimized Time Checks**: Check elapsed time only every 1000 iterations
5. **Fast Path for Sequential Reads**: Skip unnecessary operations in hot path
6. **Batched Stats Updates**: Accumulate stats locally, update atomics once per batch
7. **Optimized Circular Buffer**: Only track timestamps when needed (1% sampling)
8. **Power-of-2 Optimizations**: Use bit masks instead of modulo operations

See [IO_URING_FLOW.md](IO_URING_FLOW.md) for detailed implementation documentation.

## Goals

- Achieve maximum storage performance (e.g., 300GB/s sequential, 80M IOPS random)
- Automatically detect bottlenecks during testing
- Reduce manual parameter tuning from thousands of iterations to automated optimization
- Match or exceed fio performance through io_uring optimizations

## Requirements

- Linux (for `io_uring` and direct device access)
- Rust 1.70+
- Root access (for direct device access)
- Kernel with io_uring support (Linux 5.1+)

## Building

```bash
cargo build --release
```

The release build includes:
- Link-time optimization (LTO)
- Single codegen unit for better optimization
- Stripped symbols for smaller binary size

## Usage

### Basic benchmark

```bash
sudo ./target/release/storage-bench run -d /dev/nvme0n1 -w seqread
```

### Advanced benchmark with custom parameters

```bash
sudo ./target/release/storage-bench run \
    -d /dev/nvme0n1 \
    -w seqread \
    -b 128k \
    -q 128 \
    -t 20
```

### Random workload

```bash
sudo ./target/release/storage-bench run \
    -d /dev/nvme0n1 \
    -w randread \
    -b 4k \
    -q 256
```

### With optimization and monitoring

```bash
sudo ./target/release/storage-bench run \
    -d /dev/nvme0n1 \
    -w seqread \
    --optimize \
    --monitor
```

### List available devices

```bash
./target/release/storage-bench list
```

### System information

```bash
./target/release/storage-bench info
```

## Command-Line Options

### Run Command

- `-d, --device <PATH>`: Path to storage device (e.g., /dev/nvme0n1)
- `-w, --workload <TYPE>`: Workload type (seqread, seqwrite, randread, randwrite, seq, rand, all)
- `-b, --block-size <SIZE>`: Block size (4k, 8k, 16k, 32k, 64k, 128k, 256k, 512k, 1m, 2m)
- `-q, --queue-depth <DEPTH>`: Queue depth (default: 32)
- `-n, --threads <COUNT>`: Number of worker threads (default: auto-detect)
- `-t, --duration <SECONDS>`: Test duration in seconds (default: 60)
- `--optimize`: Enable automatic parameter optimization
- `--monitor`: Enable real-time monitoring

## Architecture

- **I/O Engine**: Handles direct device I/O with io_uring and multiple worker threads
  - Optimized for high-throughput sequential workloads
  - Supports fixed buffers and files for zero-overhead I/O
  - Batched submissions and completions for efficiency
- **Monitor Engine**: Collects CPU, memory, NUMA, and I/O metrics
- **Optimizer Engine**: Detects bottlenecks and adjusts parameters automatically

## Performance Tuning

For best performance:

1. **Use appropriate queue depth**: Start with 128 for NVMe devices
2. **Match block size to workload**: 128k for sequential, 4k for random
3. **Use multiple threads**: One thread per NUMA node or CPU core
4. **Ensure O_DIRECT**: The tool automatically uses O_DIRECT for direct device access
5. **Check CPU affinity**: Pin threads to specific CPU cores for better cache locality

## Technical Details

### io_uring Optimizations

The implementation uses several advanced io_uring features:

- **IORING_REGISTER_BUFFERS**: Pre-registers buffers with kernel to eliminate DMA mapping overhead
- **IORING_REGISTER_FILES**: Pre-registers file descriptor to eliminate lookup overhead
- **ReadFixed/WriteFixed**: Uses fixed buffer operations for zero-copy I/O
- **Batched submissions**: Submits multiple operations per syscall
- **Non-blocking completions**: Processes completions without blocking

### Performance Profiling

The tool has been optimized based on perf profiling:

- DMA mapping overhead: Reduced from 42% to < 1%
- Syscall overhead: Reduced from 40% to ~15%
- Clock overhead: Reduced from 30% to < 1%
- Circular buffer overhead: Reduced from ~25% to < 5%

See [IO_URING_FLOW.md](IO_URING_FLOW.md) for detailed performance analysis.

## Status

✅ **Core functionality implemented**

- [x] io_uring-based I/O engine with optimizations
- [x] Fixed buffers and files support
- [x] Multiple workload patterns
- [x] Real-time statistics
- [x] Performance optimizations
- [ ] IOPOLL mode support (future)
- [ ] Advanced bottleneck detection (in progress)
- [ ] Automatic parameter optimization (in progress)

## Contributing

Contributions are welcome! Areas for improvement:

- IOPOLL mode support for even better performance
- More aggressive queue management
- NUMA-aware buffer allocation
- Additional workload patterns
- Better bottleneck detection algorithms

## License

Apache-2.0
