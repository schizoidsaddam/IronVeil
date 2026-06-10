use sysinfo::{Components, Disks, Networks, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldTension {
    Peaceful,
    Unrest,
    Conflict,
    Crisis,
}

impl WorldTension {
    pub fn from_cpu(pct: f32) -> Self {
        if pct < 30.0      { Self::Peaceful }
        else if pct < 60.0 { Self::Unrest   }
        else if pct < 80.0 { Self::Conflict }
        else               { Self::Crisis   }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Peaceful => "PEACEFUL",
            Self::Unrest   => "UNREST",
            Self::Conflict => "CONFLICT",
            Self::Crisis   => "CRISIS",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThermalZone {
    pub label:  String,
    pub temp_c: f32,
}

#[derive(Debug, Clone, Default)]
pub struct DiskActivity {
    pub read_bytes:  u64,
    pub write_bytes: u64,
    pub spike:       bool,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkActivity {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// True only after sustained silence across multiple ticks (hysteresis)
    pub silent:   bool,
}

#[derive(Debug, Clone)]
pub struct SystemMetrics {
    pub cpu_pct:       f32,
    pub tension:       WorldTension,
    pub ram_total_mb:  u64,
    pub ram_used_mb:   u64,
    pub ram_free_mb:   u64,
    pub swap_pressure: f32,
    pub thermals:      Vec<ThermalZone>,
    pub disk:          DiskActivity,
    pub network:       NetworkActivity,
    pub uptime_secs:   u64,
    pub hour_of_day:   u32,
}

pub struct SystemPoller {
    sys:           System,
    disks:         Disks,
    networks:      Networks,
    components:    Components,
    prev_net_rx:   u64,
    prev_net_tx:   u64,
    disk_baseline: f64,
    /// Consecutive silent ticks — silence only declared after 3+ consecutive silent readings
    silent_streak: u32,
    /// First poll flag — baseline tick, metrics not yet meaningful
    warmed_up:     bool,
}

impl SystemPoller {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let mut disks = Disks::new_with_refreshed_list();
        disks.refresh();
        let mut networks = Networks::new_with_refreshed_list();
        networks.refresh();
        let mut components = Components::new_with_refreshed_list();
        components.refresh();

        // Capture baseline network counters so first real poll has a valid delta
        let (net_rx, net_tx) = networks
            .list()
            .iter()
            .fold((0u64, 0u64), |acc, (_, d)| (acc.0 + d.total_received(), acc.1 + d.total_transmitted()));

        Self {
            sys,
            disks,
            networks,
            components,
            prev_net_rx:   net_rx,
            prev_net_tx:   net_tx,
            disk_baseline: 0.0,
            silent_streak: 0,
            warmed_up:     false,
        }
    }

    pub fn poll(&mut self) -> SystemMetrics {
        self.sys.refresh_all();
        self.disks.refresh();
        self.networks.refresh();
        self.components.refresh();

        // CPU
        let cpus    = self.sys.cpus();
        let cpu_pct = if cpus.is_empty() {
            0.0f32
        } else {
            #[allow(clippy::cast_precision_loss)]
            let n = cpus.len() as f32;
            cpus.iter().map(sysinfo::Cpu::cpu_usage).sum::<f32>() / n
        };

        let ram_total_mb = self.sys.total_memory() / 1_048_576;
        let ram_used_mb  = self.sys.used_memory()  / 1_048_576;
        let ram_free_mb  = ram_total_mb.saturating_sub(ram_used_mb);

        let swap_total = self.sys.total_swap();
        let swap_used  = self.sys.used_swap();
        let swap_pressure = if swap_total == 0 {
            0.0f32
        } else {
            #[allow(clippy::cast_precision_loss)]
            { (swap_used as f64 / swap_total as f64) as f32 }
        };

        let thermals: Vec<ThermalZone> = self.components
            .list()
            .iter()
            .map(|c| ThermalZone { label: c.label().to_string(), temp_c: c.temperature() })
            .collect();

        // Disk I/O
        let (disk_r, disk_w) = self.sys.processes().values().fold((0u64, 0u64), |acc, p| {
            let du = p.disk_usage();
            (acc.0 + du.read_bytes, acc.1 + du.written_bytes)
        });
        let total_io = (disk_r + disk_w) as f64;
        self.disk_baseline = self.disk_baseline.mul_add(0.85, total_io * 0.15);
        let spike = self.warmed_up && total_io > 1_000_000.0 && total_io > self.disk_baseline * 4.0;
        let disk  = DiskActivity { read_bytes: disk_r, write_bytes: disk_w, spike };

        // Network I/O with hysteresis for silence detection
        let (net_rx, net_tx) = self.networks.list().iter()
            .fold((0u64, 0u64), |acc, (_, d)| (acc.0 + d.total_received(), acc.1 + d.total_transmitted()));

        let delta_rx = net_rx.saturating_sub(self.prev_net_rx);
        let delta_tx = net_tx.saturating_sub(self.prev_net_tx);
        self.prev_net_rx = net_rx;
        self.prev_net_tx = net_tx;

        // Require 3 consecutive low-traffic ticks before declaring silence.
        // Threshold: < 50 KB/tick total to account for background chatter.
        let low_traffic = (delta_rx + delta_tx) < 51_200;
        if low_traffic {
            self.silent_streak = self.silent_streak.saturating_add(1);
        } else {
            self.silent_streak = 0;
        }
        let silent = self.warmed_up && self.silent_streak >= 3;

        let network = NetworkActivity { rx_bytes: delta_rx, tx_bytes: delta_tx, silent };

        let hour_of_day = {
            use chrono::Timelike;
            chrono::Local::now().hour()
        };

        self.warmed_up = true;

        SystemMetrics {
            cpu_pct,
            tension: WorldTension::from_cpu(cpu_pct),
            ram_total_mb,
            ram_used_mb,
            ram_free_mb,
            swap_pressure,
            thermals,
            disk,
            network,
            uptime_secs: System::uptime(),
            hour_of_day,
        }
    }
}
