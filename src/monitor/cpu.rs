use sysinfo::System;

/// CPU monitoring
pub struct CpuMonitor {
    system: System,
}

#[derive(Debug, Clone)]
pub struct CpuMetrics {
    pub utilization_per_core: Vec<f32>,
    pub avg_utilization: f32,
    pub frequency_mhz: Vec<u64>,
}

impl CpuMonitor {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_cpu();
        Self { system }
    }
    
    pub fn collect(&mut self) -> CpuMetrics {
        self.system.refresh_cpu();
        
        let cpus = self.system.cpus();
        let utilization_per_core: Vec<f32> = cpus.iter().map(|cpu| cpu.cpu_usage() as f32).collect();
        let avg_utilization = if !utilization_per_core.is_empty() {
            utilization_per_core.iter().sum::<f32>() / utilization_per_core.len() as f32
        } else {
            0.0
        };
        let frequency_mhz: Vec<u64> = cpus.iter().map(|cpu| cpu.frequency() as u64).collect();
        
        CpuMetrics {
            utilization_per_core,
            avg_utilization,
            frequency_mhz,
        }
    }
    
    pub fn is_cpu_bound(&self, threshold: f32) -> bool {
        let mut monitor = Self::new();
        let metrics = monitor.collect();
        metrics.avg_utilization > threshold
    }
}

impl Default for CpuMonitor {
    fn default() -> Self {
        Self::new()
    }
}

