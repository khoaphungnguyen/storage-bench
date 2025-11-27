use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// NUMA monitoring
pub struct NumaMonitor {
    nodes: Vec<NumaNode>,
}

#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: usize,
    pub cpus: Vec<usize>,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct NumaMetrics {
    pub num_nodes: usize,
    pub node_cpus: Vec<Vec<usize>>,
    pub node_memory: Vec<u64>,
    pub current_node: Option<usize>,
}

impl NumaMonitor {
    pub fn new() -> Result<Self> {
        let nodes = Self::detect_nodes()?;
        Ok(Self { nodes })
    }

    fn detect_nodes() -> Result<Vec<NumaNode>> {
        let mut nodes = Vec::new();
        let sys_node_path = Path::new("/sys/devices/system/node");

        if !sys_node_path.exists() {
            // Fallback: single node system
            return Ok(vec![NumaNode {
                id: 0,
                cpus: (0..num_cpus::get()).collect(),
                memory_bytes: 0,
            }]);
        }

        for entry in fs::read_dir(sys_node_path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("node") {
                if let Some(node_id_str) = name_str.strip_prefix("node") {
                    if let Ok(node_id) = node_id_str.parse::<usize>() {
                        let node_path = entry.path();
                        let cpus = Self::read_node_cpus(&node_path)?;
                        let memory = Self::read_node_memory(&node_path)?;

                        nodes.push(NumaNode {
                            id: node_id,
                            cpus,
                            memory_bytes: memory,
                        });
                    }
                }
            }
        }

        nodes.sort_by_key(|n| n.id);
        Ok(nodes)
    }

    fn read_node_cpus(node_path: &Path) -> Result<Vec<usize>> {
        let cpulist_path = node_path.join("cpulist");
        if !cpulist_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&cpulist_path).context("Failed to read cpulist")?;

        let mut cpus = Vec::new();
        for range in content.trim().split(',') {
            if let Some((start, end)) = range.split_once('-') {
                let start: usize = start.trim().parse()?;
                let end: usize = end.trim().parse()?;
                cpus.extend(start..=end);
            } else {
                cpus.push(range.trim().parse()?);
            }
        }

        Ok(cpus)
    }

    fn read_node_memory(node_path: &Path) -> Result<u64> {
        let meminfo_path = node_path.join("meminfo");
        if !meminfo_path.exists() {
            return Ok(0);
        }

        let content = fs::read_to_string(&meminfo_path).context("Failed to read meminfo")?;

        // Parse "MemTotal:      123456 kB" or "MemTotal:      123456"
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                // Format: "MemTotal:      123456 kB" or "MemTotal:      123456"
                if parts.len() >= 2 {
                    if let Ok(value) = parts[1].parse::<u64>() {
                        // Check if there's a unit
                        if parts.len() >= 3 {
                            match parts[2] {
                                "kB" | "KB" => return Ok(value * 1024),
                                "MB" => return Ok(value * 1024 * 1024),
                                "GB" => return Ok(value * 1024 * 1024 * 1024),
                                _ => return Ok(value * 1024), // Default to KB
                            }
                        } else {
                            // No unit specified, assume KB
                            return Ok(value * 1024);
                        }
                    }
                }
            }
        }

        Ok(0)
    }

    pub fn collect(&mut self) -> Result<NumaMetrics> {
        // Refresh node information
        self.nodes = Self::detect_nodes()?;

        let node_cpus: Vec<Vec<usize>> = self.nodes.iter().map(|n| n.cpus.clone()).collect();

        let node_memory: Vec<u64> = self.nodes.iter().map(|n| n.memory_bytes).collect();

        let current_node = self.get_current_numa_node();

        Ok(NumaMetrics {
            num_nodes: self.nodes.len(),
            node_cpus,
            node_memory,
            current_node,
        })
    }

    pub fn get_numa_node_for_cpu(&self, cpu: usize) -> Option<usize> {
        for node in &self.nodes {
            if node.cpus.contains(&cpu) {
                return Some(node.id);
            }
        }
        None
    }

    pub fn get_current_numa_node(&self) -> Option<usize> {
        // Get current CPU
        let current_cpu = Self::get_current_cpu()?;
        self.get_numa_node_for_cpu(current_cpu)
    }

    fn get_current_cpu() -> Option<usize> {
        // Read /proc/self/stat to get current CPU
        let stat = fs::read_to_string("/proc/self/stat").ok()?;
        let fields: Vec<&str> = stat.split_whitespace().collect();
        if fields.len() > 38 {
            fields[38].parse().ok()
        } else {
            None
        }
    }

    /// Bind current thread to a NUMA node
    pub fn bind_to_node(&self, node_id: usize) -> Result<()> {
        if let Some(node) = self.nodes.iter().find(|n| n.id == node_id) {
            if let Some(&cpu) = node.cpus.first() {
                // Use libc to set CPU affinity
                unsafe {
                    let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
                    libc::CPU_ZERO(&mut cpuset);
                    libc::CPU_SET(cpu, &mut cpuset);
                    let result =
                        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset);
                    if result != 0 {
                        return Err(anyhow::anyhow!("Failed to set CPU affinity"));
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for NumaMonitor {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            // Fallback to single node
            Self {
                nodes: vec![NumaNode {
                    id: 0,
                    cpus: (0..num_cpus::get()).collect(),
                    memory_bytes: 0,
                }],
            }
        })
    }
}
