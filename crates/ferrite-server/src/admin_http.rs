//! Lightweight zero-dependency embedded HTTP/1.1 Admin & Telemetry Dashboard (v2.0).
//!
//! Opt-in via `[admin-http]` in `ferrite.toml`:
//! - `GET /health` — liveness probe JSON (`status`, `version`, `protocol`)
//! - `GET /metrics` — real-time Folia region TPS/MSPT & profiler scope telemetry JSON
//! - `GET /plugins` — loaded dynamic native plugins & JVM plugins JSON
//! - `GET /dashboard` — live auto-refreshing HTML dashboard (region heatmap + TPS/MSPT)

use std::sync::Arc;

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::game::profile::Profiler;

/// `[admin-http]` configuration table in `ferrite.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct AdminHttpSettings {
    /// Enable the embedded HTTP admin & metrics server (off by default).
    pub enabled: bool,
    /// Socket address to bind (e.g. `"127.0.0.1:8080"`).
    pub bind_address: String,
    /// Optional Bearer token required in `Authorization: Bearer <token>` header.
    pub bearer_token: Option<String>,
}

/// Callback returning a JSON array of loaded plugins for `GET /plugins`.
pub type PluginsSnapshotFn = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

/// Shared state served by the embedded HTTP admin server.
#[derive(Clone)]
pub struct AdminHttpState {
    pub profiler: Arc<Profiler>,
    pub plugins_snapshot: PluginsSnapshotFn,
    pub bearer_token: Option<String>,
}

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ferrite v2.0 — Folia Region Dashboard</title>
<style>
  body{font-family:system-ui,sans-serif;background:#0f172a;color:#e2e8f0;margin:0;padding:24px}
  h1{margin:0 0 16px;font-size:22px;color:#38bdf8}
  .grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:12px;margin-bottom:20px}
  .card{background:#1e293b;border:1px solid #334155;border-radius:10px;padding:14px}
  .label{font-size:12px;color:#94a3b8;text-transform:uppercase}
  .val{font-size:26px;font-weight:700;margin-top:4px}
  .ok{color:#4ade80}.warn{color:#facc15}.bad{color:#f87171}
  table{width:100%;border-collapse:collapse;background:#1e293b;border-radius:10px;overflow:hidden}
  th,td{padding:8px 12px;text-align:left;border-bottom:1px solid #334155;font-size:13px}
  th{color:#94a3b8;font-weight:600}
</style>
</head>
<body>
<h1>Ferrite v2.0 — Live Folia Telemetry</h1>
<div class="grid">
  <div class="card"><div class="label">Global TPS (1m / 5m / 15m)</div><div id="tps" class="val ok">--</div></div>
  <div class="card"><div class="label">Global MSPT (10s avg / p95)</div><div id="mspt" class="val">--</div></div>
  <div class="card"><div class="label">Active Regions</div><div id="rcnt" class="val">--</div></div>
  <div class="card"><div class="label">Total Ticks</div><div id="ticks" class="val">--</div></div>
</div>
<h2 style="font-size:16px;margin:12px 0 8px">Active Folia Regions</h2>
<table><thead><tr><th>Slot</th><th>TPS (5s / 1m)</th><th>MSPT avg / p95</th><th>Ticks</th></tr></thead><tbody id="regions"></tbody></table>
<script>
async function refresh(){
  try{
    const r = await fetch('/metrics');
    if(!r.ok) return;
    const d = await r.json();
    const g = d.global;
    const tpsEl = document.getElementById('tps');
    tpsEl.textContent = `${g.tps_1m.toFixed(1)} / ${g.tps_5m.toFixed(1)} / ${g.tps_15m.toFixed(1)}`;
    tpsEl.className = 'val ' + (g.tps_1m >= 19 ? 'ok' : g.tps_1m >= 15 ? 'warn' : 'bad');
    document.getElementById('mspt').textContent = `${g.mspt_10s.avg.toFixed(2)} / ${g.mspt_10s.p95.toFixed(2)} ms`;
    document.getElementById('rcnt').textContent = d.active_regions.length;
    document.getElementById('ticks').textContent = d.ticks;
    document.getElementById('regions').innerHTML = d.active_regions.map(x =>
      `<tr><td>#${x.slot}</td><td>${x.tps_5s.toFixed(1)} / ${x.tps_1m.toFixed(1)}</td><td>${x.mspt_10s_avg.toFixed(2)} / ${x.mspt_10s_p95.toFixed(2)} ms</td><td>${x.ticks}</td></tr>`
    ).join('') || '<tr><td colspan="4">No active regions yet</td></tr>';
  }catch(_){}
}
refresh(); setInterval(refresh, 2000);
</script>
</body>
</html>"#;

/// Starts the embedded HTTP admin listener on `listener` and serves requests until aborted.
pub fn spawn_admin_http(
    listener: TcpListener,
    state: AdminHttpState,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let _ = handle_connection(stream, state).await;
            });
        }
    })
}

async fn handle_connection(mut stream: TcpStream, state: AdminHttpState) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut lines = req.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    if let Some(expected) = &state.bearer_token {
        let expected_header = format!("bearer {expected}");
        let authorized = lines.any(|line| {
            line.split_once(':').is_some_and(|(k, v)| {
                k.trim().eq_ignore_ascii_case("authorization")
                    && v.trim().eq_ignore_ascii_case(&expected_header)
            })
        });
        if !authorized {
            return write_response(
                &mut stream,
                "401 Unauthorized",
                "application/json",
                br#"{"error":"unauthorized"}"#,
            )
            .await;
        }
    }

    if method != "GET" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "application/json",
            br#"{"error":"method_not_allowed"}"#,
        )
        .await;
    }

    let clean_path = path.split('?').next().unwrap_or(path);
    match clean_path {
        "/health" => {
            let body = serde_json::json!({
                "status": "ok",
                "server": "Ferrite",
                "version": env!("CARGO_PKG_VERSION"),
                "protocol": ferrite_protocol::PROTOCOL_VERSION,
            })
            .to_string();
            write_response(&mut stream, "200 OK", "application/json", body.as_bytes()).await
        }
        "/metrics" => {
            let body = state.profiler.snapshot_json().to_string();
            write_response(&mut stream, "200 OK", "application/json", body.as_bytes()).await
        }
        "/plugins" => {
            let body = (state.plugins_snapshot)().to_string();
            write_response(&mut stream, "200 OK", "application/json", body.as_bytes()).await
        }
        "/" | "/dashboard" => {
            write_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                DASHBOARD_HTML.as_bytes(),
            )
            .await
        }
        _ => {
            write_response(
                &mut stream,
                "404 Not Found",
                "application/json",
                br#"{"error":"not_found"}"#,
            )
            .await
        }
    }
}

async fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    async fn http_get(addr: std::net::SocketAddr, path: &str, token: Option<&str>) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn admin_http_serves_health_metrics_dashboard_and_enforces_bearer_token() {
        let profiler = Arc::new(Profiler::new());
        profiler.record_region(1, Duration::from_millis(12));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = spawn_admin_http(
            listener,
            AdminHttpState {
                profiler,
                plugins_snapshot: Arc::new(|| serde_json::json!({"dynamic": ["demo"], "jvm": []})),
                bearer_token: Some("secret123".into()),
            },
        );

        let unauth = http_get(addr, "/health", None).await;
        assert!(unauth.starts_with("HTTP/1.1 401 Unauthorized"));

        let health = http_get(addr, "/health", Some("secret123")).await;
        assert!(health.starts_with("HTTP/1.1 200 OK"));
        assert!(health.contains("\"status\":\"ok\""));

        let metrics = http_get(addr, "/metrics", Some("secret123")).await;
        assert!(metrics.starts_with("HTTP/1.1 200 OK"));
        assert!(metrics.contains("\"active_regions\""));

        let plugins = http_get(addr, "/plugins", Some("secret123")).await;
        assert!(plugins.contains("\"demo\""));

        let dash = http_get(addr, "/dashboard", Some("secret123")).await;
        assert!(dash.contains("Ferrite v2.0"));

        handle.abort();
    }
}
