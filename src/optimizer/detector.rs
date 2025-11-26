use crate::monitor::BottleneckReport;

/// Bottleneck detector
pub struct BottleneckDetector;

impl BottleneckDetector {
    pub fn new() -> Self {
        Self
    }
    
    pub fn analyze(&self, report: &BottleneckReport) -> String {
        match &report.bottleneck {
            crate::monitor::Bottleneck::CpuBound { utilization, cores } => {
                format!("CPU-bound: {}% utilization on cores {:?}", utilization, cores)
            }
            crate::monitor::Bottleneck::MemoryBound { utilization, .. } => {
                format!("Memory-bound: {}% utilization", utilization)
            }
            crate::monitor::Bottleneck::IoBound { queue_depth, .. } => {
                format!("I/O-bound: queue depth {}", queue_depth)
            }
            crate::monitor::Bottleneck::NumaBound { .. } => {
                "NUMA-bound: cross-node access detected".to_string()
            }
            crate::monitor::Bottleneck::Balanced => {
                "System appears balanced".to_string()
            }
        }
    }
}

impl Default for BottleneckDetector {
    fn default() -> Self {
        Self::new()
    }
}

