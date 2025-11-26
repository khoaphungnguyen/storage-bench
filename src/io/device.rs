use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;
use anyhow::Result;

/// Abstraction for storage device access
pub struct Device {
    file: File,
    path: std::path::PathBuf,
    size: u64,
}

impl Device {
    /// Open a storage device with O_DIRECT for direct I/O
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        use std::fs::OpenOptions;
        
        let path_buf = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_DIRECT | libc::O_RDWR)
            .open(&path_buf)?;
        
        let metadata = file.metadata()?;
        let size = metadata.len();
        
        Ok(Device {
            file,
            path: path_buf,
            size,
        })
    }
    
    /// Get the raw file descriptor
    pub fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
    
    /// Get device path
    pub fn path(&self) -> &Path {
        &self.path
    }
    
    /// Get device size in bytes
    pub fn size(&self) -> u64 {
        self.size
    }
    
    /// Get device information
    pub fn info(&self) -> DeviceInfo {
        DeviceInfo {
            path: self.path.clone(),
            size: self.size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: std::path::PathBuf,
    pub size: u64,
}

impl AsRawFd for Device {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

