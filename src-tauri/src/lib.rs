use tauri::ipc::Channel;
use tokio::sync::oneshot;

#[cfg(windows)]
mod windows;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // All of this is necessary because `WebView2` initializes COM on this thread, which interferes with doing it in `AudioMonitor`.
    // More info: https://github.com/tauri-apps/tauri/issues/6485
    let (finished_tx, finished_rx) = oneshot::channel();
    let (monitor_data_tx, monitor_data_rx) = oneshot::channel();

    std::thread::spawn(move || {
        let monitor = windows::AudioMonitor::new();
        let volume_events = monitor.volume_watch.clone();
        let command_sender = monitor.command_sender.clone();

        monitor_data_tx
            .send((volume_events, command_sender))
            .unwrap();

        finished_rx.blocking_recv().unwrap();
    });

    let (mut volume_events, command_sender) = monitor_data_rx.blocking_recv().unwrap();

    tauri::Builder::default()
        .setup(|_app| {
            let channel = Channel::new(move |msg| {
                let volume: f32 = match msg.deserialize() {
                    Ok(vol) => vol,
                    Err(e) => {
                        eprintln!("failed to parse request from frontend");
                        return Err(e.into());
                    }
                };

                if let Err(e) = command_sender.send(windows::AudioThreadCommand::SetVolume(volume))
                {
                    eprintln!("failed to send volume request: {e}");
                }

                Ok(())
            });

            tauri::async_runtime::spawn({
                let channel = channel.clone();
                async move {
                    while let Ok(event) = volume_events.changed().await {
                        if let Err(e) = channel.send(event) {
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

    finished_tx.send(()).unwrap();
}
