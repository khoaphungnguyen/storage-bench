use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub device: PathBuf,
    pub mode: IoMode,
    pub block_size: usize,
    pub queue_depth: usize,
    pub threads: usize,
    pub duration: Duration,
    pub optimize: bool,
    pub monitor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoMode {
    Sequential,
    Random,
    Mixed,
}

impl std::str::FromStr for IoMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sequential" | "seq" => Ok(IoMode::Sequential),
            "random" | "rand" => Ok(IoMode::Random),
            "mixed" => Ok(IoMode::Mixed),
            _ => Err(anyhow::anyhow!("Invalid I/O mode: {}", s)),
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

