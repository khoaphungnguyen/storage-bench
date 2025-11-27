use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub device: PathBuf,
    pub workload: Workload,
    pub block_size: usize,
    pub queue_depth: usize,
    pub threads: usize,
    pub duration: Duration,
    pub optimize: bool,
    pub monitor: bool,
}

/// Parse human-readable block size (e.g., "4k", "64k", "1m", "2m")
pub fn parse_block_size(s: &str) -> anyhow::Result<usize> {
    let s = s.trim().to_lowercase();
    let (num_str, unit) = if s.ends_with('k') {
        (&s[..s.len() - 1], "k")
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('g') {
        (&s[..s.len() - 1], "g")
    } else {
        // Assume bytes if no unit
        return s.parse().map_err(|_| anyhow::anyhow!("Invalid block size: {}", s));
    };

    let num: usize = num_str.parse()
        .map_err(|_| anyhow::anyhow!("Invalid number in block size: {}", s))?;

    let multiplier = match unit {
        "k" => 1024,
        "m" => 1024 * 1024,
        "g" => 1024 * 1024 * 1024,
        _ => return Err(anyhow::anyhow!("Invalid unit in block size: {}", s)),
    };

    Ok(num * multiplier)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Workload {
    SeqRead,    // Sequential read (100% read)
    SeqWrite,   // Sequential write (100% write)
    RandRead,   // Random read (100% read)
    RandWrite,  // Random write (100% write)
    Seq,        // Sequential mixed (50% read, 50% write)
    Rand,       // Random mixed (50% read, 50% write)
    All,        // Run all workloads
}

impl Workload {
    pub fn is_sequential(&self) -> bool {
        matches!(self, Workload::SeqRead | Workload::SeqWrite | Workload::Seq)
    }

    pub fn is_random(&self) -> bool {
        matches!(self, Workload::RandRead | Workload::RandWrite | Workload::Rand)
    }

    pub fn read_percent(&self) -> u8 {
        match self {
            Workload::SeqRead | Workload::RandRead => 100,
            Workload::SeqWrite | Workload::RandWrite => 0,
            Workload::Seq | Workload::Rand => 50,
            Workload::All => 50, // Default for "all"
        }
    }
}

impl std::str::FromStr for Workload {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "seqread" | "seq-read" | "sequential-read" => Ok(Workload::SeqRead),
            "seqwrite" | "seq-write" | "sequential-write" => Ok(Workload::SeqWrite),
            "randread" | "rand-read" | "random-read" => Ok(Workload::RandRead),
            "randwrite" | "rand-write" | "random-write" => Ok(Workload::RandWrite),
            "seq" | "sequential" => Ok(Workload::Seq),
            "rand" | "random" => Ok(Workload::Rand),
            "all" => Ok(Workload::All),
            _ => Err(anyhow::anyhow!("Invalid workload: {}. Valid options: seqread, seqwrite, randread, randwrite, seq, rand, all", s)),
        }
    }
}

// Keep IoMode for backward compatibility with patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoMode {
    Sequential,
    Random,
    Mixed,
}

impl From<Workload> for IoMode {
    fn from(workload: Workload) -> Self {
        match workload {
            Workload::SeqRead | Workload::SeqWrite | Workload::Seq => IoMode::Sequential,
            Workload::RandRead | Workload::RandWrite | Workload::Rand => IoMode::Random,
            Workload::All => IoMode::Sequential, // Default
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestParams {
    pub queue_depth: usize,
    pub block_size: usize,
    pub num_threads: usize,
    pub io_pattern: IoMode,
    pub read_percent: u8, // 0-100
    pub num_jobs: usize,
}

impl Default for TestParams {
    fn default() -> Self {
        Self {
            queue_depth: 32,
            block_size: 65536,
            num_threads: num_cpus::get(),
            io_pattern: IoMode::Sequential,
            read_percent: 100,
            num_jobs: 1,
        }
    }
}

