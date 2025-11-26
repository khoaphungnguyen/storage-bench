use crate::config::IoMode;
use rand::Rng;
use std::sync::Mutex;

/// I/O pattern generator
pub struct IoPattern {
    mode: IoMode,
    block_size: usize,
    device_size: u64,
    rng: Mutex<rand::rngs::StdRng>,
}

impl IoPattern {
    pub fn new(mode: IoMode, block_size: usize, device_size: u64) -> Self {
        use rand::SeedableRng;
        Self {
            mode,
            block_size,
            device_size,
            rng: Mutex::new(rand::rngs::StdRng::from_entropy()),
        }
    }

    /// Generate next I/O offset
    pub fn next_offset(&self, current: u64) -> u64 {
        match self.mode {
            IoMode::Sequential => {
                let next = current + self.block_size as u64;
                if next >= self.device_size { 0 } else { next }
            }
            IoMode::Random => {
                let max_offset = self.device_size.saturating_sub(self.block_size as u64);
                self.rng.lock().unwrap().gen_range(0..=max_offset)
            }
            IoMode::Mixed => {
                // 70% sequential, 30% random
                let mut rng = self.rng.lock().unwrap();
                if rng.gen_bool(0.7) {
                    let next = current + self.block_size as u64;
                    if next >= self.device_size { 0 } else { next }
                } else {
                    let max_offset = self.device_size.saturating_sub(self.block_size as u64);
                    rng.gen_range(0..=max_offset)
                }
            }
        }
    }

    /// Check if this is a read operation (based on read_percent)
    pub fn is_read(&self, read_percent: u8) -> bool {
        self.rng.lock().unwrap().gen_range(0..100) < read_percent
    }
}
