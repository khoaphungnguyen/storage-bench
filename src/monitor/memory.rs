use sysinfo::System;

/// Memory monitoring
pub struct MemoryMonitor {
    system: System,
}

#[derive(Debug, Clone)]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub utilization_percent: f32,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        Self { system }
    }
    
    pub fn collect(&mut self) -> MemoryMetrics {
        self.system.refresh_memory();
        
        let total = self.system.total_memory();
        let used = self.system.used_memory();
        let free = self.system.free_memory();
        let available = self.system.available_memory();
        let utilization = if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        
        MemoryMetrics {
            total_bytes: total * 1024, // Convert from KB to bytes
            used_bytes: used * 1024,
            free_bytes: free * 1024,
            available_bytes: available * 1024,
            utilization_percent: utilization,
        }
    }
    
    pub fn is_memory_bound(&self, threshold: f32) -> bool {
        let mut monitor = Self::new();
        let metrics = monitor.collect();
        metrics.utilization_percent > threshold
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

