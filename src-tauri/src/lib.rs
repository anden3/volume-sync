use tauri::{Emitter as _, Listener as _};
use tokio::sync::oneshot;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::{AudioMonitor, AudioThreadCommand};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // All of this is necessary because `WebView2` initializes COM on this thread, which interferes with doing it in `AudioMonitor`.
    // More info: https://github.com/tauri-apps/tauri/issues/6485
    let (finished_tx, finished_rx) = oneshot::channel();
    let (monitor_data_tx, monitor_data_rx) = oneshot::channel();

    std::thread::spawn(move || {
        let monitor = AudioMonitor::new();

        monitor_data_tx
            .send((monitor.volume_watch.clone(), monitor.command_sender.clone()))
            .expect("should be able to send monitor data back from thread");

        if let Err(e) = finished_rx.blocking_recv() {
            eprintln!("tauri panicked, shutting down monitor thread: {e}");
        }
    });

    let (mut volume_events, command_sender) = monitor_data_rx.blocking_recv().unwrap();

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();

            app.listen("web-volume-changed", move |evt| {
                let volume: f32 = match serde_json::from_str(evt.payload()) {
                    Ok(vol) => vol,
                    Err(e) => {
                        eprintln!("failed to parse request from frontend: {e}");
                        return;
                    }
                };

                if let Err(e) = command_sender.send(AudioThreadCommand::SetVolume(volume)) {
                    eprintln!("failed to send volume request: {e}");
                }
            });

            tauri::async_runtime::spawn({
                async move {
                    // Send the initial volume (do-while would be nice here).
                    if let Err(e) = handle.emit("system-volume-changed", *volume_events.borrow()) {
                        eprintln!("failed to send volume event to frontend: {e}");
                    }

                    loop {
                        if let Err(e) = volume_events.changed().await {
                            eprintln!("failed to listen to system volume events: {e}");
                            break;
                        }

                        if let Err(e) =
                            handle.emit("system-volume-changed", *volume_events.borrow())
                        {
                            eprintln!("failed to send volume event to frontend: {e}");
                        }
                    }
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    finished_tx
        .send(())
        .expect("monitor thread should be alive");
}
