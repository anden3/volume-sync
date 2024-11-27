// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use windows::{
    core::*,
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, S_FALSE},
        Media::Audio::{
            eConsole, eRender,
            Endpoints::{
                IAudioEndpointVolume, IAudioEndpointVolumeCallback,
                IAudioEndpointVolumeCallback_Impl,
            },
            IMMDeviceEnumerator, MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA,
        },
        System::Com::*,
    },
};

struct CoInitializeGuard;

impl Drop for CoInitializeGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn initialize_com() -> CoInitializeGuard {
    // SAFETY: `pvreserved` is None, and the combination of flags is valid.
    let result =
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED | COINIT_DISABLE_OLE1DDE) }.ok();
    match result {
        Ok(()) => CoInitializeGuard,
        Err(e) if e.code() == S_FALSE => panic!("COM library already initialized"),
        Err(e) if e.code() == RPC_E_CHANGED_MODE => {
            panic!("COM library already initialized with incompatible concurrency model");
        }
        Err(e) => panic!("failed to initialize COM library, error code: {e}"),
    }
}

unsafe fn get_default_audio_volume_interface() -> IAudioEndpointVolume {
    let imm_device_enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.unwrap();

    let device =
        unsafe { imm_device_enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.unwrap();

    unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.unwrap()
}

unsafe fn get_master_volume(volume_interface: &IAudioEndpointVolume) -> f32 {
    unsafe { volume_interface.GetMasterVolumeLevelScalar() }.unwrap()
}

unsafe fn set_master_volume(volume_interface: &IAudioEndpointVolume, volume: f32) {
    let volume = volume.clamp(0.0, 0.3);
    unsafe { volume_interface.SetMasterVolumeLevelScalar(volume, &windows::core::GUID::zeroed()) }
        .unwrap();
}

unsafe fn register_volume_callback(
    volume_interface: &IAudioEndpointVolume,
) -> IAudioEndpointVolumeCallback {
    let volume_callback: IAudioEndpointVolumeCallback = AudioEndpointVolumeCallback.into();

    unsafe { volume_interface.RegisterControlChangeNotify(&volume_callback) }.unwrap();
    volume_callback
}

#[implement(IAudioEndpointVolumeCallback)]
struct AudioEndpointVolumeCallback;

impl IAudioEndpointVolumeCallback_Impl for AudioEndpointVolumeCallback_Impl {
    fn OnNotify(&self, pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows_core::Result<()> {
        println!(
            "volume changed: {:.0}",
            unsafe { *pnotify }.fMasterVolume * 100.0
        );
        Ok(())
    }
}

fn main() {
    let _guard = initialize_com();

    let volume_interface = unsafe { get_default_audio_volume_interface() };
    let volume_callback = unsafe { register_volume_callback(&volume_interface) };

    let current_volume = unsafe { get_master_volume(&volume_interface) };
    println!("Volume before: {:.0}", current_volume * 100.0);
    unsafe { set_master_volume(&volume_interface, current_volume * 0.5) };
    println!(
        "Volume after: {:.0}",
        unsafe { get_master_volume(&volume_interface) } * 100.0
    );

    std::thread::sleep(std::time::Duration::from_secs(5));

    unsafe { volume_interface.UnregisterControlChangeNotify(&volume_callback) }.unwrap();
    // volume_sync_lib::run()
}
