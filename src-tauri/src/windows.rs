use std::{marker::PhantomData, sync::mpsc};

use tokio::sync::watch;
use windows::{
    core::*,
    Win32::{
        Foundation::{ERROR_NOT_FOUND, S_FALSE},
        Media::Audio::{Endpoints::*, *},
        System::Com::*,
    },
};

pub type NotSendMarker = PhantomData<*const ()>;
pub type VolumeCallbackFn<T> = fn(AUDIO_VOLUME_NOTIFICATION_DATA, &T) -> windows_core::Result<()>;

const MAX_NORMALIZED_VOLUME_LEVEL: f32 = 0.3;

// We need to indicate that a volume change comes from us, so we can avoid sending it to the frontend.
// The actual GUID here doesn't matter, I just generated one.
const LOCAL_VOLUME_CHANGE_GUID: GUID = GUID::from_u128(0xdc1b615d_6d18_4f6e_af33_488e23d0dc6a);

pub enum AudioThreadCommand {
    NewDefault(HSTRING),
    DeviceRemoved(HSTRING),
    SetVolume(f32),
}

#[derive(Debug)]
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

fn get_device<ID: Param<PCWSTR>>(
    device_enumerator: &IMMDeviceEnumerator,
    id: ID,
) -> Option<IMMDevice> {
    // SAFETY: `device_enumerator` is a valid reference.
    match unsafe { device_enumerator.GetDevice(id) } {
        Ok(device) => Some(device),
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => {
            eprintln!("no output devices found");
            None
        }
        Err(e) => panic!("failed to retrieve default audio output device: {e}"),
    }
}

fn get_default_device(device_enumerator: &IMMDeviceEnumerator) -> Option<IMMDevice> {
    // `eRender` is output, `eConsole` is the default (and most common) role from what I can tell.
    // SAFETY: `device_enumerator` is a valid reference.
    match unsafe { device_enumerator.GetDefaultAudioEndpoint(eRender, eConsole) } {
        Ok(device) => Some(device),
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => {
            eprintln!("no output devices found");
            None
        }
        Err(e) => panic!("failed to retrieve default audio output device: {e}"),
    }
}

#[derive(Debug)]
pub struct AudioMonitor {
    pub volume_watch: watch::Receiver<Option<f32>>,
    pub command_sender: mpsc::Sender<AudioThreadCommand>,
    _coinitialize_guard: Option<CoInitializeGuard>,
    device_enumerator: IMMDeviceEnumerator,
    device_event_notif_client: IMMNotificationClient,
}

impl AudioMonitor {
    pub fn new() -> Self {
        let _coinitialize_guard = initialize_com();

        let (command_tx, command_rx) = mpsc::channel::<AudioThreadCommand>();
        let (watch_tx, watch_rx) = watch::channel(None);

        std::thread::spawn(move || Self::audio_thread(command_rx, watch_tx));

        // SAFETY: We don't pass a pointer in `punkouter`, so it can't be invalid.
        let device_enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .expect("all parameters should be valid");

        let device = get_default_device(&device_enumerator);

        let device_id = device
            .as_ref()
            .and_then(|d| unsafe { d.GetId() }.ok())
            .and_then(|id| unsafe { id.to_hstring().ok() });

        let device_event_notif_client = MMNotificationClient {
            default_device_notifier: command_tx.clone(),
        }
        .into();

        if let Some(device_id) = device_id {
            command_tx
                .send(AudioThreadCommand::NewDefault(device_id))
                .unwrap();
        }

        // SAFETY: `device_enumerator` and `device_event_notif_client` are valid references.
        unsafe {
            device_enumerator.RegisterEndpointNotificationCallback(&device_event_notif_client)
        }
        .expect("all parameters should be valid");

        Self {
            _coinitialize_guard,
            command_sender: command_tx,
            device_enumerator,
            device_event_notif_client,
            volume_watch: watch_rx,
        }
    }

    fn audio_thread(
        commands: mpsc::Receiver<AudioThreadCommand>,
        volume_watch: watch::Sender<Option<f32>>,
    ) {
        // SAFETY: We don't pass a pointer in `punkouter`, so it can't be invalid.
        let device_enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .expect("all parameters should be valid");

        let mut current_device = None;

        for command in commands {
            match command {
                AudioThreadCommand::NewDefault(curr_device) => {
                    current_device = unsafe {
                        AudioOutputDevice::acquire(
                            curr_device,
                            &device_enumerator,
                            Self::volume_callback,
                            volume_watch.clone(),
                        )
                    };

                    // SAFETY: `device.volume_interface` is a valid reference.
                    let volume = current_device.as_ref().map(|device| {
                        unsafe { device.volume_interface.GetMasterVolumeLevelScalar() }
                            .expect("`volume_interface` should be valid")
                    });

                    if let Err(e) = volume_watch.send(volume) {
                        eprintln!("failed to send updated volume: {e}");
                    }
                }
                AudioThreadCommand::DeviceRemoved(removed_device) => {
                    if current_device
                        .as_ref()
                        .is_some_and(|curr| curr.device_id == removed_device)
                    {
                        current_device = None;

                        if let Err(e) = volume_watch.send(None) {
                            eprintln!("failed to send unavailable volume: {e}");
                        }
                    }
                }
                AudioThreadCommand::SetVolume(volume) => {
                    let volume = volume.clamp(0.0, MAX_NORMALIZED_VOLUME_LEVEL.min(1.0));

                    let Some(device) = current_device.as_ref() else {
                        return;
                    };

                    // SAFETY: `volume_interface` is a valid reference.
                    unsafe {
                        device
                            .volume_interface
                            .SetMasterVolumeLevelScalar(volume, &LOCAL_VOLUME_CHANGE_GUID)
                    }
                    .expect("volume should be in safe bounds");
                }
            }
        }
    }

    fn volume_callback(
        data: AUDIO_VOLUME_NOTIFICATION_DATA,
        volume_watch: &watch::Sender<Option<f32>>,
    ) -> windows_core::Result<()> {
        // Filter out volume changes we caused ourselves.
        if data.guidEventContext == LOCAL_VOLUME_CHANGE_GUID {
            return Ok(());
        }

        if let Err(e) = volume_watch.send(Some(data.fMasterVolume)) {
            eprintln!("failed to send updated volume: {e}");
        }

        Ok(())
    }
}

impl Drop for AudioMonitor {
    fn drop(&mut self) {
        // SAFETY: `self.device_enumerator` is a valid reference and
        // `self.device_event_notif_client` is the same interface originally registered.
        unsafe {
            self.device_enumerator
                .UnregisterEndpointNotificationCallback(&self.device_event_notif_client)
        }
        .expect("all parameters should be valid");
    }
}

#[derive(Debug)]
struct AudioOutputDevice {
    device_id: HSTRING,
    volume_interface: IAudioEndpointVolume,
    volume_callback_object: IAudioEndpointVolumeCallback,
}

impl AudioOutputDevice {
    // SAFETY: The methods in the callback must be non-blocking. The callback should never wait on a synchronization object.
    // The callback should never call `IAudioEndpointVolume::UnregisterControlChangeNotify`.
    // The callback should never release the final reference on an `EndpointVolume` API object.
    pub unsafe fn acquire<CallbackArg>(
        device_id: HSTRING,
        device_enumerator: &IMMDeviceEnumerator,
        callback: VolumeCallbackFn<CallbackArg>,
        callback_arg: CallbackArg,
    ) -> Option<Self>
    where
        CallbackArg: 'static,
    {
        let device = get_device(device_enumerator, &device_id)?;

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
            device_id,
            volume_interface,
            volume_callback_object,
        })
    }
}

impl Drop for AudioOutputDevice {
    fn drop(&mut self) {
        // SAFETY: `self.volume_interface` is a valid reference and
        // `self.volume_callback_object` is the same interface originally registered.
        unsafe {
            self.volume_interface
                .UnregisterControlChangeNotify(&self.volume_callback_object)
        }
        .expect("all parameters should be valid");
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

#[implement(IMMNotificationClient)]
struct MMNotificationClient {
    default_device_notifier: mpsc::Sender<AudioThreadCommand>,
}

impl IMMNotificationClient_Impl for MMNotificationClient_Impl {
    fn OnDeviceStateChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _dwnewstate: DEVICE_STATE,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _pwstrdeviceid: &PCWSTR) -> windows_core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> windows_core::Result<()> {
        // SAFETY: `pwstrdeviceid` is guaranteed to be a valid, null-terminated pointer.
        let removed_device = match unsafe { pwstrdeviceid.to_hstring() } {
            Ok(new) => new,
            Err(e) => {
                eprintln!("failed to convert device ID (`{pwstrdeviceid:?}`) to `HSTRING`: {e}");
                return Ok(());
            }
        };

        if let Err(e) = self
            .default_device_notifier
            .send(AudioThreadCommand::DeviceRemoved(removed_device))
        {
            eprintln!("failed to send notification that device was removed: {e}");
        }

        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        pwstrdefaultdeviceid: &PCWSTR,
    ) -> windows_core::Result<()> {
        if flow != eRender || role != eConsole {
            return Ok(());
        }

        // SAFETY: `pwstrdefaultdeviceid` is guaranteed to be a valid, null-terminated pointer.
        let new_default = match unsafe { pwstrdefaultdeviceid.to_hstring() } {
            Ok(new) => new,
            Err(e) => {
                eprintln!(
                    "failed to convert device ID (`{pwstrdefaultdeviceid:?}`) to `HSTRING`: {e}"
                );
                return Ok(());
            }
        };

        if let Err(e) = self
            .default_device_notifier
            .send(AudioThreadCommand::NewDefault(new_default))
        {
            eprintln!("failed to send notification that default device changed: {e}");
        }

        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
    ) -> windows_core::Result<()> {
        Ok(())
    }
}
