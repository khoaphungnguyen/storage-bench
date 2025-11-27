use anyhow::Result;
use std::fs;
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};

// BLKGETSIZE64 ioctl constant (from linux/fs.h)
const BLKGETSIZE64: libc::c_ulong = 0x80081272;

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
        let name = self.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let info = Self::get_device_info(name, &self.path);
        DeviceInfo {
            path: self.path.clone(),
            size: self.size,
            model: info.0,
            device_type: info.1,
            link_speed: info.2,
            link_status: info.3,
        }
    }

    // List all available storage devices
    /// List all available storage devices
    pub fn list_devices() -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // List block devices from /sys/block
        let sys_block = Path::new("/sys/block");
        if sys_block.exists() {
            for entry in fs::read_dir(sys_block)? {
                let entry = entry?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                // Skip loop devices, ram devices, etc.
                if name_str.starts_with("loop")
                    || name_str.starts_with("ram")
                    || name_str.starts_with("dm-")
                {
                    continue;
                }

                let device_path = PathBuf::from("/dev").join(name_str.as_ref());

                // Try to get device size and info
                if let Ok(size) = Self::get_device_size(&device_path) {
                    if size > 0 {
                        let info = Self::get_device_info(&name_str, &device_path);
                        devices.push(DeviceInfo {
                            path: device_path,
                            size,
                            model: info.0,
                            device_type: info.1,
                            link_speed: info.2,
                            link_status: info.3,
                        });
                    }
                }
            }
        }

        // Also check for NVMe namespaces (skip controllers and fabrics)
        let nvme_path = Path::new("/dev");
        if nvme_path.exists() {
            for entry in fs::read_dir(nvme_path)? {
                let entry = entry?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                // Only include NVMe namespaces (nvmeXnY format) and partitions (nvmeXnYpZ)
                // Skip controllers (nvmeX without 'n'), fabrics (nvme-fabrics), and other non-block devices
                // A namespace has format nvmeXnY where X is controller and Y is namespace
                if name_str.starts_with("nvme")
                    && name_str.contains('n')
                    && !name_str.ends_with("-fabrics")
                    && name_str.len() > 5
                {
                    // At least "nvme0n1"
                    let device_path = PathBuf::from("/dev").join(name_str.as_ref());
                    if let Ok(size) = Self::get_device_size(&device_path) {
                        // Only include devices with valid size > 0, and avoid duplicates
                        if size > 0 && !devices.iter().any(|d| d.path == device_path) {
                            let info = Self::get_device_info(&name_str, &device_path);
                            devices.push(DeviceInfo {
                                path: device_path,
                                size,
                                model: info.0,
                                device_type: info.1,
                                link_speed: info.2,
                                link_status: info.3,
                            });
                        }
                    }
                }
            }
        }

        // Filter out devices with zero size from the /sys/block list as well
        devices.retain(|d| d.size > 0);

        devices.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(devices)
    }

    fn get_device_size<P: AsRef<Path>>(path: P) -> Result<u64> {
        let path_ref = path.as_ref();
        let name = path_ref
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid device path"))?;

        // Try reading from /sys/class/block first (works for all block devices including partitions)
        let class_size_path = Path::new("/sys/class/block").join(name).join("size");
        if class_size_path.exists() {
            if let Ok(content) = fs::read_to_string(&class_size_path) {
                if let Ok(sectors) = content.trim().parse::<u64>() {
                    if sectors > 0 {
                        return Ok(sectors * 512); // Convert sectors to bytes
                    }
                }
            }
        }

        // Try /sys/block (for whole devices, not partitions)
        let size_path = Path::new("/sys/block").join(name).join("size");
        if size_path.exists() {
            if let Ok(content) = fs::read_to_string(&size_path) {
                if let Ok(sectors) = content.trim().parse::<u64>() {
                    if sectors > 0 {
                        return Ok(sectors * 512);
                    }
                }
            }
        }

        // Fallback: Try to open the device (requires root for block devices)
        // Use BLKGETSIZE64 ioctl for accurate size
        match fs::File::open(path_ref) {
            Ok(file) => {
                // Try using ioctl to get block device size
                unsafe {
                    use std::os::unix::io::AsRawFd;
                    let fd = file.as_raw_fd();
                    let mut size: u64 = 0;
                    let result = libc::ioctl(fd, BLKGETSIZE64, &mut size);
                    if result == 0 && size > 0 {
                        return Ok(size);
                    }
                }
                // Fallback to metadata (may not work for block devices)
                if let Ok(metadata) = file.metadata() {
                    let len = metadata.len();
                    if len > 0 {
                        return Ok(len);
                    }
                }
            }
            Err(_) => {}
        }

        Ok(0)
    }

    /// Get device information (model, type, link speed, link status)
    fn get_device_info(
        device_name: &str,
        device_path: &Path,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        let mut model = None;
        let mut device_type = None;
        let mut link_speed = None;
        let mut link_status = None;

        // Try to get NVMe-specific info
        if device_name.starts_with("nvme") {
            device_type = Some("NVMe".to_string());

            // Extract controller (e.g., nvme0 from nvme0n1)
            if let Some(n_pos) = device_name.find('n') {
                let controller = &device_name[..n_pos];

                // Model from /sys/class/block/nvme0n1/device/model or /sys/block/nvme0n1/device/model
                let model_paths = [
                    Path::new("/sys/class/block")
                        .join(device_name)
                        .join("device")
                        .join("model"),
                    Path::new("/sys/block")
                        .join(device_name)
                        .join("device")
                        .join("model"),
                    Path::new("/sys/class/block")
                        .join(device_name)
                        .join("device")
                        .join("..")
                        .join("model"),
                ];

                for model_path in &model_paths {
                    if model_path.exists() {
                        if let Ok(content) = fs::read_to_string(model_path) {
                            model = Some(content.trim().to_string());
                            break;
                        }
                    }
                }

                // Link speed from /sys/class/nvme/nvme0/subsysnqn or /sys/block/nvme0n1/queue/optimal_io_size
                // Actually, link speed is in /sys/class/nvme/nvme0/device/subsystem/nvme-subsys0/subsysnqn
                // Better: /sys/class/nvme/nvme0/device/subsystem/nvme-subsys0/device/nvme0/firmware_rev
                // Link speed: /sys/class/nvme/nvme0/subsysnqn doesn't have it
                // Try: /sys/block/nvme0n1/queue/optimal_io_size or check PCIe info

                // For NVMe, try to get PCIe link info
                let pci_path = Path::new("/sys/class/block")
                    .join(device_name)
                    .join("device")
                    .join("..")
                    .join("..");

                // Try to find PCIe link speed
                let link_speed_path = pci_path.join("current_link_speed");
                if link_speed_path.exists() {
                    if let Ok(content) = fs::read_to_string(&link_speed_path) {
                        link_speed = Some(content.trim().to_string());
                    }
                }

                // Link width
                let link_width_path = pci_path.join("current_link_width");
                if link_width_path.exists() {
                    if let Ok(width) = fs::read_to_string(&link_width_path) {
                        if let Some(speed) = &link_speed {
                            link_speed = Some(format!("{} x{}", speed.trim(), width.trim()));
                        }
                    }
                }
            }
        } else if device_name.starts_with("sd")
            || device_name.starts_with("vd")
            || device_name.starts_with("xvd")
        {
            device_type = Some("SATA/SAS".to_string());

            // Try to get model from /sys/block/sda/device/model
            let model_path = Path::new("/sys/block")
                .join(device_name)
                .join("device")
                .join("model");
            if model_path.exists() {
                if let Ok(content) = fs::read_to_string(&model_path) {
                    model = Some(content.trim().to_string());
                }
            }
        }

        (model, device_type, link_speed, link_status)
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: std::path::PathBuf,
    pub size: u64,
    pub model: Option<String>,
    pub device_type: Option<String>,
    pub link_speed: Option<String>,
    pub link_status: Option<String>,
}

impl AsRawFd for Device {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}
