// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use windows::Win32::{
    Foundation::{RPC_E_CHANGED_MODE, S_FALSE},
    Media::Audio::{
        eConsole, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator,
    },
    System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_DISABLE_OLE1DDE,
        COINIT_MULTITHREADED,
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

unsafe fn get_default_audio_device() -> windows::Win32::Media::Audio::IMMDevice {
    let imm_device_enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.unwrap();

    unsafe { imm_device_enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.unwrap()
}

unsafe fn get_master_volume() -> f32 {
    let device = unsafe { get_default_audio_device() };
    let endpoint_volume =
        unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.unwrap();

    unsafe { endpoint_volume.GetMasterVolumeLevelScalar() }.unwrap()
}

unsafe fn set_master_volume(volume: f32) {
    let volume = volume.clamp(0.0, 0.3);
    let device = unsafe { get_default_audio_device() };
    let endpoint_volume =
        unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.unwrap();

    unsafe { endpoint_volume.SetMasterVolumeLevelScalar(volume, &windows::core::GUID::zeroed()) }
        .unwrap();
}

fn main() {
    let _guard = initialize_com();

    let current_volume = unsafe { get_master_volume() };
    println!("Volume before: {:.0}", current_volume * 100.0);
    unsafe { set_master_volume(current_volume * 0.5) };
    println!(
        "Volume after: {:.0}",
        unsafe { get_master_volume() } * 100.0
    );
    // volume_sync_lib::run()
}
