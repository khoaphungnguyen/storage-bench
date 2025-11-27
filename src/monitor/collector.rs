use crate::monitor::{CpuMonitor, MemoryMonitor, NumaMonitor, IoStatsMonitor};
use crate::monitor::cpu::CpuMetrics;
use crate::monitor::memory::MemoryMetrics;
use crate::monitor::numa::NumaMetrics;
use crate::monitor::io_stats::IoStats;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;

#[derive(Debug, Clone)]
pub enum Bottleneck {
    CpuBound { utilization: f32, cores: Vec<usize> },
    MemoryBound { utilization: f32, available_bytes: u64 },
    IoBound { queue_depth: usize, latency_p99: Duration },
    NumaBound { cross_node_access: bool },
    Balanced,
}

#[derive(Debug, Clone)]
pub struct BottleneckReport {
    pub bottleneck: Bottleneck,
    pub cpu_metrics: CpuMetrics,
    pub memory_metrics: MemoryMetrics,
    pub numa_metrics: NumaMetrics,
    pub io_stats: Option<IoStats>,
    pub recommendations: Vec<String>,
}

/// Unified monitoring collector
pub struct MonitorCollector {
    cpu_monitor: CpuMonitor,
    memory_monitor: MemoryMonitor,
    numa_monitor: NumaMonitor,
    io_monitor: Option<IoStatsMonitor>,
}

impl MonitorCollector {
    pub fn new(device_path: Option<PathBuf>) -> Self {
        Self {
            cpu_monitor: CpuMonitor::new(),
            memory_monitor: MemoryMonitor::new(),
            numa_monitor: NumaMonitor::default(),
            io_monitor: device_path.map(IoStatsMonitor::new),
        }
    }
    
    pub fn collect_metrics(&mut self) -> Result<BottleneckReport> {
        let cpu_metrics = self.cpu_monitor.collect();
        let memory_metrics = self.memory_monitor.collect();
        let numa_metrics = self.numa_monitor.collect()?;
        let io_stats = self.io_monitor.as_ref()
            .and_then(|m| m.collect().ok());
        
        let bottleneck = self.detect_bottleneck(
            &cpu_metrics,
            &memory_metrics,
            &numa_metrics,
            &io_stats,
        );
        
        let recommendations = self.generate_recommendations(&bottleneck);
        
        Ok(BottleneckReport {
            bottleneck,
            cpu_metrics,
            memory_metrics,
            numa_metrics,
            io_stats,
            recommendations,
        })
    }
    
    fn detect_bottleneck(
        &self,
        cpu: &CpuMetrics,
        memory: &MemoryMetrics,
        numa: &NumaMetrics,
        io: &Option<IoStats>,
    ) -> Bottleneck {
        // CPU bottleneck detection
        if cpu.avg_utilization > 90.0 {
            let hot_cores: Vec<usize> = cpu.utilization_per_core
                .iter()
                .enumerate()
                .filter(|(_, &util)| util > 90.0)
                .map(|(idx, _)| idx)
                .collect();
            return Bottleneck::CpuBound {
                utilization: cpu.avg_utilization,
                cores: hot_cores,
            };
        }
        
        // Memory bottleneck detection
        if memory.utilization_percent > 90.0 {
            return Bottleneck::MemoryBound {
                utilization: memory.utilization_percent,
                available_bytes: memory.available_bytes,
            };
        }
        
        // I/O bottleneck detection
        if let Some(io_stats) = io {
            if io_stats.in_flight > 1000 {
                return Bottleneck::IoBound {
                    queue_depth: io_stats.in_flight as usize,
                    latency_p99: Duration::from_millis(io_stats.time_in_queue),
                };
            }
        }
        
        // NUMA bottleneck detection
        if numa.num_nodes > 1 {
            // TODO: Implement cross-node access detection
            return Bottleneck::NumaBound {
                cross_node_access: false,
            };
        }
        
        Bottleneck::Balanced
    }
    
    fn generate_recommendations(&self, bottleneck: &Bottleneck) -> Vec<String> {
        match bottleneck {
            Bottleneck::CpuBound { utilization, cores } => {
                vec![
                    format!("CPU utilization is {}%", utilization),
                    format!("Hot cores: {:?}", cores),
                    "Consider reducing thread count or increasing block size".to_string(),
                ]
            }
            Bottleneck::MemoryBound { utilization, .. } => {
                vec![
                    format!("Memory utilization is {}%", utilization),
                    "Consider reducing buffer sizes or increasing available memory".to_string(),
                ]
            }
            Bottleneck::IoBound { queue_depth, .. } => {
                vec![
                    format!("I/O queue depth is {}", queue_depth),
                    "Consider increasing queue depth or reducing block size".to_string(),
                ]
            }
            Bottleneck::NumaBound { .. } => {
                vec![
                    "NUMA cross-node access detected".to_string(),
                    "Consider binding threads to specific NUMA nodes".to_string(),
                ]
            }
            Bottleneck::Balanced => {
                vec!["System appears balanced".to_string()]
            }
        }
    }
}

