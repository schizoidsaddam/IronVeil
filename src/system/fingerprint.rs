use sha2::{Digest, Sha256};
use sysinfo::System;

/// Derives a deterministic u64 seed from this specific machine.
/// Stable across reboots. Unique per machine.
pub fn generate() -> u64 {
    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname  = System::host_name().unwrap_or_else(|| "unknown".into());
    let cpu_brand = sys.cpus()
        .first()
        .map_or_else(|| "unknown_cpu".into(), |c| c.brand().to_string());
    let total_ram = sys.total_memory().to_string();

    let raw  = format!("{hostname}::{cpu_brand}::{total_ram}");
    let hash = Sha256::digest(raw.as_bytes());

    let mut seed = [0u8; 8];
    seed.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(seed)
}
