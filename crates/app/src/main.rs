//! SterOidSoundBoard — one executable: audio engine + web server + UI.
mod server;
mod state;

use std::net::SocketAddr;
use std::time::Duration;

struct Args {
    bind: String,
    port: u16,
    open_browser: bool,
}

fn parse_args() -> Args {
    let mut a = Args { bind: "0.0.0.0".into(), port: 8420, open_browser: true };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--port" => a.port = it.next().and_then(|p| p.parse().ok()).unwrap_or(a.port),
            "--bind" => a.bind = it.next().unwrap_or(a.bind),
            "--local" => a.bind = "127.0.0.1".into(),
            "--no-browser" | "--headless" => a.open_browser = false,
            "--version" | "-V" => {
                println!("steroidsoundboard {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "steroidsoundboard {}\n\n  --port N       HTTP port (default 8420)\n  --bind ADDR    bind address (default 0.0.0.0)\n  --local        only listen on 127.0.0.1\n  --headless     don't open a browser (RPi/servers)\n",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            other => eprintln!("unknown argument: {other}"),
        }
    }
    a
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = parse_args();
    let app = state::App::new();
    log::info!("SterOidSoundBoard {} — data dir {}", env!("CARGO_PKG_VERSION"), app.paths.data.display());

    // Auto-start audio with the last (or default) configuration.
    {
        let app = app.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = app.start_audio(None) {
                log::warn!("audio auto-start failed: {e:#} — choose a device in the UI");
            }
        });
    }

    // Meters (20 Hz) and autosave (every 2 s).
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_millis(50));
            let mut n = 0u32;
            loop {
                t.tick().await;
                if app.events.receiver_count() > 0 {
                    let _ = app.events.send(app.meters_json());
                }
                n = n.wrapping_add(1);
                if n % 40 == 0 {
                    let a = app.clone();
                    let _ = tokio::task::spawn_blocking(move || a.save_if_dirty()).await;
                }
            }
        });
    }

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = format!("http://127.0.0.1:{}", args.port);
    log::info!("UI: {local}");
    if args.bind == "0.0.0.0" {
        if let Ok(ip) = local_ip_address::local_ip() {
            log::info!("Tablet/phone: http://{ip}:{}", args.port);
        }
    }
    if args.open_browser {
        let _ = webbrowser::open(&local);
    }

    let app_shutdown = app.clone();
    axum::serve(listener, server::router(app))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            log::info!("shutting down");
            app_shutdown.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            app_shutdown.save_if_dirty();
            app_shutdown.stop_audio();
        })
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
