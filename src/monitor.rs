#![forbid(unsafe_code)]

//! Administration System Performance, Resource Usage, Abnormality & Overload Monitoring (Pure Safe Rust).

use crate::config::AppConfig;
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use sysinfo::{Disks, Pid, ProcessesToUpdate, System};

#[derive(Clone, Copy, Debug)]
pub struct ArchiveCounts {
    pub identities: usize,
    pub vectors: usize,
    pub locations: usize,
    pub documents: usize,
    pub pending_requests: usize,
}

pub struct SystemMonitor {
    started_at: Instant,
    total_requests: AtomicU64,
    total_errors: AtomicU64,
    total_latency_ms: AtomicU64,
    max_latency_ms: AtomicU64,
    in_flight: AtomicUsize,
    sys: Mutex<System>,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all().expect("sysinfo System::new_all");
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Self {
            started_at: Instant::now(),
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            max_latency_ms: AtomicU64::new(0),
            in_flight: AtomicUsize::new(0),
            sys: Mutex::new(sys),
        }
    }

    pub fn begin_request(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    pub fn finish_request(&self, elapsed_ms: u64, is_error: bool) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }
        self.total_latency_ms
            .fetch_add(elapsed_ms, Ordering::Relaxed);
        self.max_latency_ms.fetch_max(elapsed_ms, Ordering::Relaxed);
    }

    pub async fn snapshot(
        &self,
        cfg: &AppConfig,
        http: &reqwest::Client,
        counts: ArchiveCounts,
    ) -> Value {
        let (
            cpu_pct,
            ram_total_gb,
            ram_used_gb,
            ram_pct,
            proc_rss_mb,
            disk_total_gb,
            disk_avail_gb,
            disk_used_pct,
        ) = {
            let mut sys = self.sys.lock().expect("sysinfo lock");
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let pid = Pid::from_u32(std::process::id());
            sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

            let cpu_pct = f64::from(sys.global_cpu_usage());
            let total_mem = sys.total_memory() as f64;
            let used_mem = sys.used_memory() as f64;
            let ram_total_gb = total_mem / (1024.0 * 1024.0 * 1024.0);
            let ram_used_gb = used_mem / (1024.0 * 1024.0 * 1024.0);
            let ram_pct = if total_mem > 0.0 {
                (used_mem / total_mem) * 100.0
            } else {
                0.0
            };
            let proc_rss_mb = sys
                .process(pid)
                .map(|p| (p.memory() as f64) / (1024.0 * 1024.0))
                .unwrap_or(0.0);

            let disks = Disks::new_with_refreshed_list().ok();
            let root_str = cfg.project_root.to_string_lossy().to_ascii_lowercase();
            let chosen_disk = disks.as_ref().and_then(|ds| {
                ds.iter()
                    .filter(|d| {
                        let mp = d.mount_point().to_string_lossy().to_ascii_lowercase();
                        !mp.is_empty() && root_str.starts_with(&mp)
                    })
                    .max_by_key(|d| d.mount_point().to_string_lossy().len())
                    .or_else(|| ds.iter().next())
            });

            let (dt, da, dp) = if let Some(d) = chosen_disk {
                let t = d.total_space() as f64;
                let a = d.available_space() as f64;
                let u_pct = if t > 0.0 { ((t - a) / t) * 100.0 } else { 0.0 };
                (
                    t / (1024.0 * 1024.0 * 1024.0),
                    a / (1024.0 * 1024.0 * 1024.0),
                    u_pct,
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            (
                cpu_pct,
                ram_total_gb,
                ram_used_gb,
                ram_pct,
                proc_rss_mb,
                dt,
                da,
                dp,
            )
        };

        // Probe local Ollama AI engine reachability + latency.
        let ollama_tags_url = cfg.ollama_url.replace("/api/chat", "/api/tags");
        let t0 = Instant::now();
        let (ollama_ok, ollama_ms) = match http
            .get(&ollama_tags_url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => (true, t0.elapsed().as_millis() as u64),
            _ => (false, t0.elapsed().as_millis() as u64),
        };

        let total_req = self.total_requests.load(Ordering::Relaxed);
        let total_err = self.total_errors.load(Ordering::Relaxed);
        let total_lat = self.total_latency_ms.load(Ordering::Relaxed);
        let max_lat = self.max_latency_ms.load(Ordering::Relaxed);
        let in_flight = self.in_flight.load(Ordering::Relaxed);
        let avg_lat_ms = if total_req > 0 {
            (total_lat as f64) / (total_req as f64)
        } else {
            0.0
        };
        let err_rate_pct = if total_req > 0 {
            ((total_err as f64) / (total_req as f64)) * 100.0
        } else {
            0.0
        };

        let mut abnormalities: Vec<Value> = Vec::new();
        let mut actions: Vec<String> = Vec::new();
        let mut severity_level = 0u8; // 0 = HEALTHY, 1 = WARNING, 2 = CRITICAL_OVERLOAD

        if cpu_pct >= 92.0 {
            severity_level = severity_level.max(2);
            abnormalities.push(json!({ "component": "CPU", "severity": "CRITICAL", "detail": format!("CPU utilization overloaded at {cpu_pct:.1}%.") }));
            actions.push("Throttle concurrent OCR/biometric uploads or lower Ollama thread count (`PARAMETER num_thread` in Modelfile).".into());
        } else if cpu_pct >= 80.0 {
            severity_level = severity_level.max(1);
            abnormalities.push(json!({ "component": "CPU", "severity": "WARNING", "detail": format!("Elevated CPU usage at {cpu_pct:.1}%.") }));
            actions.push(
                "Monitor active Ollama inference jobs and stagger bulk archive ingestion.".into(),
            );
        }

        if ram_pct >= 92.0 {
            severity_level = severity_level.max(2);
            abnormalities.push(json!({ "component": "RAM", "severity": "CRITICAL", "detail": format!("System memory critically full ({ram_pct:.1}%, {ram_used_gb:.1}/{ram_total_gb:.1} GB).") }));
            actions.push("Unload idle Ollama models (`ollama stop`), close unused background apps, or add system RAM.".into());
        } else if ram_pct >= 82.0 {
            severity_level = severity_level.max(1);
            abnormalities.push(json!({ "component": "RAM", "severity": "WARNING", "detail": format!("High memory usage ({ram_pct:.1}%).") }));
            actions.push("Reduce `keep_alive` window for Ollama models or monitor container memory limits in Docker Desktop.".into());
        }

        if disk_used_pct >= 92.0 || (disk_total_gb > 0.0 && disk_avail_gb < 5.0) {
            severity_level = severity_level.max(2);
            abnormalities.push(json!({ "component": "Disk", "severity": "CRITICAL", "detail": format!("Critically low disk space ({disk_avail_gb:.1} GB free, {disk_used_pct:.1}% used).") }));
            actions.push("Purge temporary OCR chunks & enhanced image cache (`POST /admin/api/system/clear-cache`) and archive old staging files.".into());
        } else if disk_used_pct >= 85.0 {
            severity_level = severity_level.max(1);
            abnormalities.push(json!({ "component": "Disk", "severity": "WARNING", "detail": format!("Disk usage approaching capacity ({disk_used_pct:.1}% used, {disk_avail_gb:.1} GB free).") }));
            actions.push("Schedule archival backup and clear temporary `ocr-chunks/` and `enhanced/` caches.".into());
        }

        if !ollama_ok {
            severity_level = severity_level.max(2);
            abnormalities.push(json!({ "component": "Ollama AI Engine", "severity": "CRITICAL", "detail": "Local Ollama service (127.0.0.1:11434) is unreachable." }));
            actions.push("Start Ollama immediately (`ollama serve`) or run `start.bat` to restore vision/summarization capabilities.".into());
        } else if ollama_ms > 1500 {
            severity_level = severity_level.max(1);
            abnormalities.push(json!({ "component": "Ollama AI Engine", "severity": "WARNING", "detail": format!("Ollama API response slow ({ollama_ms} ms).") }));
            actions.push("Check GPU/CPU load on Ollama and ensure only `qwen3-vl:4b-instruct-16k` is loaded.".into());
        }

        if in_flight >= 8 {
            severity_level = severity_level.max(2);
            abnormalities.push(json!({ "component": "Concurrency", "severity": "CRITICAL", "detail": format!("Request overload: {in_flight} concurrent requests in flight.") }));
            actions.push(
                "Enable request rate-limiting or queue bulk archive ingestion tasks sequentially."
                    .into(),
            );
        }

        if total_req >= 10 && err_rate_pct >= 10.0 {
            severity_level = severity_level.max(1);
            abnormalities.push(json!({ "component": "API Error Rate", "severity": "WARNING", "detail": format!("Elevated error rate ({err_rate_pct:.1}% across {total_req} requests).") }));
            actions.push(
                "Inspect server logs and verify ONNX models, file permissions, and disk health."
                    .into(),
            );
        }

        if counts.pending_requests >= 25 {
            severity_level = severity_level.max(1);
            abnormalities.push(json!({ "component": "Approval Queue", "severity": "WARNING", "detail": format!("{} pending guest requests awaiting review.", counts.pending_requests) }));
            actions.push("Review and approve/reject pending requests in the Admin Portal to keep the staging folder clean.".into());
        }

        if actions.is_empty() {
            actions.push("All systems operating within normal thresholds. Run periodic SHA-256 checksum integrity audits for archival compliance.".into());
        }

        let status = match severity_level {
            2 => "CRITICAL_OVERLOAD",
            1 => "WARNING",
            _ => "HEALTHY",
        };

        json!({
            "status": status,
            "uptime_seconds": self.started_at.elapsed().as_secs(),
            "resources": {
                "cpu_usage_pct": (cpu_pct * 10.0).round() / 10.0,
                "ram_total_gb": (ram_total_gb * 100.0).round() / 100.0,
                "ram_used_gb": (ram_used_gb * 100.0).round() / 100.0,
                "ram_usage_pct": (ram_pct * 10.0).round() / 10.0,
                "server_process_ram_mb": (proc_rss_mb * 10.0).round() / 10.0,
                "disk_total_gb": (disk_total_gb * 10.0).round() / 10.0,
                "disk_available_gb": (disk_avail_gb * 10.0).round() / 10.0,
                "disk_used_pct": (disk_used_pct * 10.0).round() / 10.0,
            },
            "services": {
                "ollama_reachable": ollama_ok,
                "ollama_ping_ms": ollama_ms,
                "vision_model": cfg.vision_model,
            },
            "workload": {
                "total_requests": total_req,
                "total_errors": total_err,
                "error_rate_pct": (err_rate_pct * 10.0).round() / 10.0,
                "avg_latency_ms": (avg_lat_ms * 10.0).round() / 10.0,
                "max_latency_ms": max_lat,
                "in_flight_requests": in_flight,
            },
            "archive_counts": {
                "enrolled_people": counts.identities,
                "face_vectors": counts.vectors,
                "locations": counts.locations,
                "documents": counts.documents,
                "pending_requests": counts.pending_requests,
            },
            "abnormalities": abnormalities,
            "recommended_actions": actions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_request_accounting() {
        let m = SystemMonitor::new();
        m.begin_request();
        m.finish_request(42, false);
        m.begin_request();
        m.finish_request(100, true);
        assert_eq!(m.total_requests.load(Ordering::Relaxed), 2);
        assert_eq!(m.total_errors.load(Ordering::Relaxed), 1);
        assert_eq!(m.max_latency_ms.load(Ordering::Relaxed), 100);
        assert_eq!(m.in_flight.load(Ordering::Relaxed), 0);
    }
}
