use procfs::ProcResult;
use std::path::PathBuf;

/// I/O statistics monitoring
pub struct IoStatsMonitor {
    device_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct IoStats {
    pub read_ios: u64,
    pub read_merges: u64,
    pub read_sectors: u64,
    pub read_ticks: u64,
    pub write_ios: u64,
    pub write_merges: u64,
    pub write_sectors: u64,
    pub write_ticks: u64,
    pub in_flight: u64,
    pub io_ticks: u64,
    pub time_in_queue: u64,
}

impl IoStatsMonitor {
    pub fn new(device_path: PathBuf) -> Self {
        Self { device_path }
    }
    
    pub fn collect(&self) -> ProcResult<IoStats> {
        // Read from /proc/diskstats
        let diskstats = procfs::diskstats()?;
        
        // Find the device
        let device_name = self.device_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        
        for entry in diskstats {
            if entry.name == device_name {
                // Map available fields from procfs DiskStat
                // Note: Some fields may not be available in all procfs versions
                return Ok(IoStats {
                    read_ios: entry.reads,
                    read_merges: entry.merged,
                    read_sectors: entry.sectors_read,
                    read_ticks: entry.time_reading,
                    write_ios: entry.writes,
                    write_merges: entry.writes_merged,
                    write_sectors: entry.sectors_written,
                    write_ticks: entry.time_writing,
                    in_flight: 0, // TODO: Get from procfs when available
                    io_ticks: 0,  // TODO: Get from procfs when available
                    time_in_queue: 0, // TODO: Get from procfs when available
                });
            }
        }
        
        Err(procfs::ProcError::NotFound(None))
    }
}

