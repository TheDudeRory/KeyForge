#[derive(Debug, Clone, PartialEq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub output: bool,
    pub default: bool,
}

pub trait AudioDeviceManager: Send + Sync {
    /// Active render + capture endpoints.
    fn list(&self) -> Result<Vec<AudioDevice>, String>;
    fn set_default(&self, id: &str) -> Result<(), String>;
    /// Master volume by delta percent; returns the new percent.
    fn adjust_master_volume(&self, delta_percent: i32) -> Result<i32, String>;
    /// Returns the new mute state.
    fn toggle_mute(&self) -> Result<bool, String>;
}

/// Devices are stored by name; IDs change across replugs, so re-match fuzzily:
/// exact name → name contains query → query contains name.
pub fn match_device<'a>(
    query: &str,
    devices: &'a [AudioDevice],
    want_output: bool,
) -> Option<&'a AudioDevice> {
    let q = query.trim().to_lowercase();
    let pool = || devices.iter().filter(|d| d.output == want_output);
    pool()
        .find(|d| d.name.to_lowercase() == q)
        .or_else(|| pool().find(|d| d.name.to_lowercase().contains(&q)))
        .or_else(|| pool().find(|d| q.contains(&d.name.to_lowercase())))
}

pub fn device_exists(query: &str, devices: &[AudioDevice]) -> bool {
    match_device(query, devices, true).is_some() || match_device(query, devices, false).is_some()
}

#[cfg(windows)]
pub use win_impl::NativeAudioManager;
#[cfg(not(windows))]
pub use pactl_impl::NativeAudioManager;

#[cfg(windows)]
mod win_impl {
    use super::{AudioDevice, AudioDeviceManager};
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice,
        IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
        STGM_READ,
    };

    const PKEY_DEVICE_FRIENDLYNAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// Undocumented but stable-since-Vista interface used by every audio
    /// switcher; only SetDefaultEndpoint is called, earlier slots are padding.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: IUnknown {
        unsafe fn slot_get_mix_format(&self) -> HRESULT;
        unsafe fn slot_get_device_format(&self) -> HRESULT;
        unsafe fn slot_reset_device_format(&self) -> HRESULT;
        unsafe fn slot_set_device_format(&self) -> HRESULT;
        unsafe fn slot_get_processing_period(&self) -> HRESULT;
        unsafe fn slot_set_processing_period(&self) -> HRESULT;
        unsafe fn slot_get_share_mode(&self) -> HRESULT;
        unsafe fn slot_set_share_mode(&self) -> HRESULT;
        unsafe fn slot_get_property_value(&self) -> HRESULT;
        unsafe fn slot_set_property_value(&self) -> HRESULT;
        unsafe fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
    }

    pub struct NativeAudioManager;

    fn estr<E: std::fmt::Display>(e: E) -> String {
        e.to_string()
    }

    fn com_init() {
        // S_FALSE / RPC_E_CHANGED_MODE are fine — some thread already did it.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn device_id(dev: &IMMDevice) -> Result<String, String> {
        unsafe {
            let p = dev.GetId().map_err(estr)?;
            let s = p.to_string().map_err(estr)?;
            CoTaskMemFree(Some(p.0 as _));
            Ok(s)
        }
    }

    unsafe fn friendly_name(dev: &IMMDevice) -> Result<String, String> {
        unsafe {
            let store = dev.OpenPropertyStore(STGM_READ).map_err(estr)?;
            let value = store.GetValue(&PKEY_DEVICE_FRIENDLYNAME).map_err(estr)?;
            let p = PropVariantToStringAlloc(&value).map_err(estr)?;
            let s = p.to_string().map_err(estr)?;
            CoTaskMemFree(Some(p.0 as _));
            Ok(s)
        }
    }

    fn enumerator() -> Result<IMMDeviceEnumerator, String> {
        com_init();
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(estr) }
    }

    fn default_volume_endpoint() -> Result<IAudioEndpointVolume, String> {
        unsafe {
            let device = enumerator()?
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(estr)?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).map_err(estr)
        }
    }

    impl AudioDeviceManager for NativeAudioManager {
        fn list(&self) -> Result<Vec<AudioDevice>, String> {
            let enumerator = enumerator()?;
            let mut out = Vec::new();
            unsafe {
                for (flow, output) in [(eRender, true), (eCapture, false)] {
                    let default_id = enumerator
                        .GetDefaultAudioEndpoint(flow, eConsole)
                        .ok()
                        .and_then(|d| device_id(&d).ok());
                    let coll = enumerator
                        .EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)
                        .map_err(estr)?;
                    for i in 0..coll.GetCount().map_err(estr)? {
                        let dev = coll.Item(i).map_err(estr)?;
                        let id = device_id(&dev)?;
                        let name = friendly_name(&dev).unwrap_or_else(|_| id.clone());
                        out.push(AudioDevice {
                            default: default_id.as_deref() == Some(id.as_str()),
                            id,
                            name,
                            output,
                        });
                    }
                }
            }
            Ok(out)
        }

        fn set_default(&self, id: &str) -> Result<(), String> {
            com_init();
            unsafe {
                let policy: IPolicyConfig =
                    CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(estr)?;
                let id = wide(id);
                for role in [eConsole, eMultimedia, eCommunications] {
                    policy
                        .SetDefaultEndpoint(PCWSTR(id.as_ptr()), role)
                        .ok()
                        .map_err(estr)?;
                }
            }
            Ok(())
        }

        fn adjust_master_volume(&self, delta_percent: i32) -> Result<i32, String> {
            unsafe {
                let vol = default_volume_endpoint()?;
                let current = vol.GetMasterVolumeLevelScalar().map_err(estr)?;
                let new = (current + delta_percent as f32 / 100.0).clamp(0.0, 1.0);
                vol.SetMasterVolumeLevelScalar(new, std::ptr::null()).map_err(estr)?;
                Ok((new * 100.0).round() as i32)
            }
        }

        fn toggle_mute(&self) -> Result<bool, String> {
            unsafe {
                let vol = default_volume_endpoint()?;
                let muted = vol.GetMute().map_err(estr)?.as_bool();
                vol.SetMute(!muted, std::ptr::null()).map_err(estr)?;
                Ok(!muted)
            }
        }
    }
}

/// ponytail: untested-on-this-box pactl shell-out (PulseAudio/PipeWire-pulse);
/// uses internal sink/source names as both id and display name — full
/// friendly-name parsing when a Linux session can verify it.
#[cfg(not(windows))]
mod pactl_impl {
    use super::{AudioDevice, AudioDeviceManager};
    use std::process::Command;

    pub struct NativeAudioManager;

    fn pactl(args: &[&str]) -> Result<String, String> {
        let out = Command::new("pactl")
            .args(args)
            .output()
            .map_err(|e| format!("pactl unavailable: {e}"))?;
        if !out.status.success() {
            return Err(format!("pactl {:?}: {}", args, String::from_utf8_lossy(&out.stderr)));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    impl AudioDeviceManager for NativeAudioManager {
        fn list(&self) -> Result<Vec<AudioDevice>, String> {
            let mut out = Vec::new();
            for (kind, output, default_cmd) in
                [("sinks", true, "get-default-sink"), ("sources", false, "get-default-source")]
            {
                let default = pactl(&[default_cmd]).map(|s| s.trim().to_string()).unwrap_or_default();
                for line in pactl(&["list", "short", kind])?.lines() {
                    if let Some(name) = line.split('\t').nth(1) {
                        out.push(AudioDevice {
                            id: name.to_string(),
                            name: name.to_string(),
                            output,
                            default: name == default,
                        });
                    }
                }
            }
            Ok(out)
        }

        fn set_default(&self, id: &str) -> Result<(), String> {
            let sinks = pactl(&["list", "short", "sinks"])?;
            let cmd = if sinks.lines().any(|l| l.split('\t').nth(1) == Some(id)) {
                "set-default-sink"
            } else {
                "set-default-source"
            };
            pactl(&[cmd, id]).map(|_| ())
        }

        fn adjust_master_volume(&self, delta_percent: i32) -> Result<i32, String> {
            let sign = if delta_percent >= 0 { "+" } else { "-" };
            pactl(&["set-sink-volume", "@DEFAULT_SINK@", &format!("{sign}{}%", delta_percent.abs())])?;
            let text = pactl(&["get-sink-volume", "@DEFAULT_SINK@"])?;
            text.split('%')
                .next()
                .and_then(|s| s.split_whitespace().last())
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| "cannot parse volume".into())
        }

        fn toggle_mute(&self) -> Result<bool, String> {
            pactl(&["set-sink-mute", "@DEFAULT_SINK@", "toggle"])?;
            Ok(pactl(&["get-sink-mute", "@DEFAULT_SINK@"])?.contains("yes"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, output: bool, default: bool) -> AudioDevice {
        AudioDevice { id: format!("id-{name}"), name: name.into(), output, default }
    }

    #[test]
    fn fuzzy_matching() {
        let devices = vec![
            dev("Speakers (Realtek(R) Audio)", true, true),
            dev("LG ULTRAWIDE (NVIDIA High Definition Audio)", true, false),
            dev("Microphone (USB Audio)", false, true),
        ];
        // exact beats contains
        assert_eq!(
            match_device("Speakers (Realtek(R) Audio)", &devices, true).unwrap().id,
            "id-Speakers (Realtek(R) Audio)"
        );
        // substring, case-insensitive
        assert_eq!(match_device("ultrawide", &devices, true).unwrap().name.contains("LG"), true);
        // query contains device name (stored a longer label than current name)
        let short = vec![dev("Speakers", true, true)];
        assert!(match_device("Speakers (High Definition)", &short, true).is_some());
        // direction respected
        assert!(match_device("ultrawide", &devices, false).is_none());
        assert!(device_exists("usb audio", &devices));
        assert!(!device_exists("bluetooth", &devices));
    }
}
