// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{marker::PhantomData, sync::Mutex};

use windows::{
    core::*,
    Win32::{
        Foundation::{ERROR_NOT_FOUND, S_FALSE},
        Media::Audio::{Endpoints::*, *},
        System::Com::*,
    },
};

pub type NotSendMarker = PhantomData<*const ()>;
pub type VolumeCallbackFn<T> =
    for<'a> fn(AUDIO_VOLUME_NOTIFICATION_DATA, &'a T) -> windows_core::Result<()>;

const MAX_NORMALIZED_VOLUME_LEVEL: f32 = 0.3;

struct CoInitializeGuard(NotSendMarker);

impl Drop for CoInitializeGuard {
    fn drop(&mut self) {
        // SAFETY: Obtaining a `CoInitializeGuard` requires that `CoInitialize()` has been called.
        // Making it !Send means that it is always called on the same thread it was created on.
        // If this function has already been called by something else, it's harmless to call it again.
        unsafe {
            CoUninitialize();
        }
    }
}

fn initialize_com() -> Option<CoInitializeGuard> {
    // SAFETY: `pvreserved` is None, and the combination of flags is valid.
    let result =
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED | COINIT_DISABLE_OLE1DDE) }.ok();

    match result {
        Ok(()) => Some(CoInitializeGuard(PhantomData)),
        Err(e) if e.code() == S_FALSE => {
            eprintln!("COM library already initialized");
            None
        }
        Err(e) => panic!("failed to initialize COM library, error code: {e}"),
    }
}

struct DefaultAudioOutput {
    device: Mutex<Option<DefaultAudioOutputDevice>>,
    device_enumerator: IMMDeviceEnumerator,
    callback: VolumeCallbackFn<()>,
}

impl DefaultAudioOutput {
    pub fn new() -> Self {
        let callback: VolumeCallbackFn<_> = Self::volume_callback;

        // SAFETY: We don't pass a pointer in `punkouter`, so it can't be invalid.
        let device_enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .expect("all parameters should be valid");

        // SAFETY: The callback doesn't do any blocking operations, nor does it wait on synchronization,
        // and it doesn't call `IAudioEndpointVolume::UnregisterControlChangeNotify` or releases any `EndPointVolume` references.
        let device = unsafe { DefaultAudioOutputDevice::acquire(&device_enumerator, callback, ()) };

        Self {
            device: Mutex::new(device),
            device_enumerator,
            callback,
        }
    }

    fn volume_callback(
        data: AUDIO_VOLUME_NOTIFICATION_DATA,
        _arg: &(),
    ) -> windows_core::Result<()> {
        println!("volume changed: {:.0}", data.fMasterVolume * 100.0);
        Ok(())
    }

    pub fn get_master_volume(&self) -> Option<f32> {
        let lock = self.device.lock().expect("thread should not panic");
        let device = lock.as_ref()?;

        // SAFETY: `volume_interface` is a valid reference.
        Some(
            unsafe { device.volume_interface.GetMasterVolumeLevelScalar() }
                .expect("`volume_interface` should be valid"),
        )
    }

    fn set_master_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, MAX_NORMALIZED_VOLUME_LEVEL.min(1.0));

        let lock = self.device.lock().expect("thread should not panic");
        let Some(device) = lock.as_ref() else {
            return;
        };

        // Pass a zeroed GUID to the volume callback since we don't need to differentiate what caused the change.
        // SAFETY: `volume_interface` is a valid reference.
        unsafe {
            device
                .volume_interface
                .SetMasterVolumeLevelScalar(volume, &windows::core::GUID::zeroed())
        }
        .expect("volume should be in safe bounds");
    }
}

struct DefaultAudioOutputDevice {
    device: IMMDevice,
    volume_interface: IAudioEndpointVolume,
    volume_callback_object: IAudioEndpointVolumeCallback,
}

impl DefaultAudioOutputDevice {
    // SAFETY: The methods in the callback must be non-blocking. The callback should never wait on a synchronization object.
    // The callback should never call the `IAudioEndpointVolume::UnregisterControlChangeNotify`.
    // The callback should never release the final reference on an `EndpointVolume` API object.
    pub unsafe fn acquire<CallbackArg>(
        device_enumerator: &IMMDeviceEnumerator,
        callback: VolumeCallbackFn<CallbackArg>,
        callback_arg: CallbackArg,
    ) -> Option<Self>
    where
        CallbackArg: 'static,
    {
        // eRender is output, eConsole is the default (and most common) role from what I can tell.
        // SAFETY: `device_enumerator` is a valid reference.
        let device = match unsafe { device_enumerator.GetDefaultAudioEndpoint(eRender, eConsole) } {
            Ok(device) => device,
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => {
                eprintln!("no output devices found");
                return None;
            }
            Err(e) => panic!("failed to retrieve default audio output device: {e}"),
        };

        // SAFETY: `device` is a valid reference, the generic is one of the allowed interfaces,
        // and we don't pass a pointer in `pactivationparams`, so it can't be invalid.
        let volume_interface =
            match unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) } {
                Ok(volume) => volume,
                Err(e) if e.code() == AUDCLNT_E_DEVICE_INVALIDATED => {
                    eprintln!("audio device was disconnected: {e}");
                    return None;
                }
                Err(e) => panic!("failed to create audio endpoint volume object: {e}"),
            };

        let volume_callback_object: IAudioEndpointVolumeCallback = AudioEndpointVolumeCallback {
            callback,
            arg: callback_arg,
        }
        .into();

        // SAFETY: `IAudioEndpointVolumeCallback` is the correct interface and `volume_interface` is a valid reference.
        unsafe { volume_interface.RegisterControlChangeNotify(&volume_callback_object) }.unwrap();

        Some(Self {
            device,
            volume_interface,
            volume_callback_object,
        })
    }
}

impl Drop for DefaultAudioOutputDevice {
    fn drop(&mut self) {
        unsafe {
            self.volume_interface
                .UnregisterControlChangeNotify(&self.volume_callback_object)
        }
        .unwrap();
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct AudioEndpointVolumeCallback<CallbackArg>
where
    CallbackArg: 'static,
{
    callback: VolumeCallbackFn<CallbackArg>,
    arg: CallbackArg,
}

impl<CallbackArg> IAudioEndpointVolumeCallback_Impl
    for AudioEndpointVolumeCallback_Impl<CallbackArg>
{
    fn OnNotify(&self, pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows_core::Result<()> {
        // SAFETY: `pnotify` is guaranteed to be a valid pointer to `AUDIO_VOLUME_NOTIFICATION_DATA`.
        let notification_data = unsafe { *pnotify };
        (self.callback)(notification_data, &self.arg)
    }
}

fn main() {
    let _guard = initialize_com();

    let audio_device = DefaultAudioOutput::new();

    let current_volume = audio_device.get_master_volume().unwrap_or(0.0);
    println!("volume before: {:.0}", current_volume * 100.0);
    audio_device.set_master_volume(current_volume * 0.5);
    println!(
        "volume after: {:.0}",
        audio_device.get_master_volume().unwrap_or(0.0) * 100.0
    );

    std::thread::sleep(std::time::Duration::from_secs(5));

    // volume_sync_lib::run()
}
