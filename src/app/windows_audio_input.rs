use arbitrary::Arbitrary;
use eyre::Context;
use facet::Facet;
use std::ffi::c_void;
use windows::Win32::Devices::Properties;
use windows::Win32::Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, ERole, IAudioClient, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator,
    MMDeviceEnumerator, eCapture,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::GUID;

const GENERIC_WINDOWS_MIC_ICON_PATH: &str = "@%SystemRoot%\\system32\\mmres.dll,-3012";
const PKEY_DEVICE_ICON: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x259abffc_507a_4ce8_8c10_9640b8a1c907),
    pid: 10,
};
const PKEY_DEVICE_CLASS_ICON: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x259abffc_507a_4ce8_8c10_9640b8a1c907),
    pid: 12,
};

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioInputDeviceSummary {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub state: String,
    pub icon: String,
    pub sample_rate_hz: Option<u32>,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct AudioInputDeviceListReport {
    pub devices: Vec<AudioInputDeviceSummary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioInputPickerKey {
    Up,
    Down,
    Tab,
    Enter,
    Escape,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioInputPickerState {
    pub selected_index: usize,
    pub devices: Vec<AudioInputDeviceSummary>,
}

impl AudioInputPickerState {
    #[must_use]
    pub fn new(devices: Vec<AudioInputDeviceSummary>) -> Self {
        Self {
            selected_index: 0,
            devices,
        }
    }

    #[must_use]
    pub fn selected_device(&self) -> Option<&AudioInputDeviceSummary> {
        self.devices.get(self.selected_index)
    }

    pub fn move_selection_up(&mut self) {
        if self.devices.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn move_selection_down(&mut self) {
        if self.devices.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = (self.selected_index + 1).min(self.devices.len() - 1);
    }

    pub fn select_index(&mut self, index: usize) {
        if self.devices.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = index.min(self.devices.len() - 1);
    }

    #[must_use]
    // audio[impl gui.keyboard-navigation]
    pub fn handle_key(&mut self, key: AudioInputPickerKey) -> AudioInputPickerKeyResult {
        match key {
            AudioInputPickerKey::Up => {
                self.move_selection_up();
                AudioInputPickerKeyResult::Handled
            }
            AudioInputPickerKey::Down | AudioInputPickerKey::Tab => {
                self.move_selection_down();
                AudioInputPickerKeyResult::Handled
            }
            AudioInputPickerKey::Enter => AudioInputPickerKeyResult::Choose,
            AudioInputPickerKey::Escape => AudioInputPickerKeyResult::Close,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioInputPickerKeyResult {
    Handled,
    Choose,
    Close,
}

#[derive(Facet, Arbitrary, Debug, PartialEq, Eq)]
pub struct AudioInputDeviceReportFixture {
    pub id: String,
    pub name: String,
}

/// List active Windows audio input endpoints.
///
/// # Errors
///
/// This function will return an error if COM or Core Audio endpoint enumeration fails.
// audio[impl enumerate.active-windows-recording]
#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "Core Audio enumeration requires small, documented FFI calls"
)]
pub fn list_active_audio_input_devices() -> eyre::Result<Vec<AudioInputDeviceSummary>> {
    let _com = ComApartment::initialize()?;
    let enumerator: IMMDeviceEnumerator = unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
            .wrap_err("failed to create Windows audio endpoint enumerator")?
    };
    let default_id = default_capture_endpoint_id(&enumerator);
    let collection: IMMDeviceCollection = unsafe {
        enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .wrap_err("failed to enumerate active Windows capture endpoints")?
    };
    let count = unsafe { collection.GetCount()? };
    let mut devices = Vec::with_capacity(usize::try_from(count).unwrap_or_default());

    for index in 0..count {
        let device: IMMDevice = unsafe { collection.Item(index)? };
        // audio[impl enumerate.endpoint-id]
        let id = unsafe { device.GetId()? };
        let id = unsafe { id.to_string()? };
        let properties = unsafe { device.OpenPropertyStore(STGM_READ).ok() };
        let name = properties
            .as_ref()
            .and_then(|properties| device_friendly_name(properties).ok())
            .unwrap_or_else(|| "Unknown microphone".to_owned());
        let icon = properties
            .as_ref()
            .and_then(device_icon_path)
            .unwrap_or_else(|| GENERIC_WINDOWS_MIC_ICON_PATH.to_owned());
        devices.push(AudioInputDeviceSummary {
            is_default: default_id
                .as_ref()
                .is_some_and(|default_id| default_id == &id),
            id,
            name,
            state: "active".to_owned(),
            // audio[impl enumerate.windows-icon]
            icon,
            // audio[impl enumerate.sample-rate]
            sample_rate_hz: device_mix_sample_rate_hz(&device).ok(),
        });
    }

    Ok(devices)
}

pub fn selected_audio_input_device_dialog_text(device: &AudioInputDeviceSummary) -> String {
    let sample_rate = device.sample_rate_hz.map_or_else(
        || "sample rate: unknown".to_owned(),
        |rate| format!("sample rate: {rate} Hz"),
    );
    format!(
        "Selected microphone:\n{}\n\nEndpoint id:\n{}\n\n{}",
        device.name, device.id, sample_rate
    )
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "Core Audio default endpoint lookup is an FFI call with no raw buffer ownership"
)]
fn default_capture_endpoint_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    let default_device = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eCapture, ERole(1))
            .ok()?
    };
    let id = unsafe { default_device.GetId().ok()? };
    unsafe { id.to_string().ok() }
}

fn device_friendly_name(properties: &IPropertyStore) -> eyre::Result<String> {
    let friendly_name_key =
        std::ptr::from_ref(&Properties::DEVPKEY_Device_FriendlyName).cast::<PROPERTYKEY>();
    property_store_string_value(properties, friendly_name_key)
}

fn device_icon_path(properties: &IPropertyStore) -> Option<String> {
    let icon_key = PKEY_DEVICE_ICON;
    if let Ok(icon_path) = property_store_string_value(properties, std::ptr::from_ref(&icon_key)) {
        return Some(icon_path);
    }
    let class_icon_key = PKEY_DEVICE_CLASS_ICON;
    if let Ok(icon_path) =
        property_store_string_value(properties, std::ptr::from_ref(&class_icon_key))
    {
        return Some(icon_path);
    }
    None
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    reason = "PROPVARIANT string extraction follows the Windows property-store layout"
)]
fn property_store_string_value(
    properties: &IPropertyStore,
    key: *const PROPERTYKEY,
) -> eyre::Result<String> {
    let mut value = unsafe { properties.GetValue(key)? };
    let variant_type = unsafe { value.Anonymous.Anonymous.vt };
    if variant_type != VT_LPWSTR {
        unsafe { PropVariantClear(&raw mut value)? };
        eyre::bail!("property value is not a UTF-16 string")
    }
    let name = unsafe {
        let pwstr = value.Anonymous.Anonymous.Anonymous.pwszVal;
        if pwstr.is_null() {
            String::new()
        } else {
            pwstr.to_string()?
        }
    };
    unsafe { PropVariantClear(&raw mut value)? };
    Ok(name)
}

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "IAudioClient mix-format query activates metadata only and frees COM memory immediately"
)]
fn device_mix_sample_rate_hz(device: &IMMDevice) -> eyre::Result<u32> {
    let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let mix_format = unsafe { audio_client.GetMixFormat()? };
    if mix_format.is_null() {
        eyre::bail!("audio client returned a null mix format")
    }
    let sample_rate_ptr = unsafe { std::ptr::addr_of!((*mix_format).nSamplesPerSec) };
    let sample_rate = unsafe { sample_rate_ptr.read_unaligned() };
    unsafe { CoTaskMemFree(Some(mix_format.cast::<c_void>())) };
    Ok(sample_rate)
}

struct ComApartment {
    uninitialize_on_drop: bool,
}

impl ComApartment {
    #[expect(
        clippy::undocumented_unsafe_blocks,
        reason = "COM apartment initialization is a process API with no borrowed pointers"
    )]
    fn initialize() -> eyre::Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            return Ok(Self {
                uninitialize_on_drop: true,
            });
        }
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self {
                uninitialize_on_drop: false,
            });
        }
        eyre::bail!("failed to initialize COM for audio endpoint enumeration: {result:?}")
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize_on_drop {
            // Safety: this instance only sets the flag when `CoInitializeEx` succeeded on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str) -> AudioInputDeviceSummary {
        AudioInputDeviceSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            is_default: false,
            state: "active".to_owned(),
            icon: "microphone".to_owned(),
            sample_rate_hz: None,
        }
    }

    #[test]
    // audio[verify gui.keyboard-navigation]
    fn picker_navigation_clamps_to_available_devices() {
        let mut state = AudioInputPickerState::new(vec![device("a", "A"), device("b", "B")]);

        state.move_selection_down();
        state.move_selection_down();
        assert_eq!(state.selected_index, 1);

        state.move_selection_up();
        state.move_selection_up();
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    // audio[verify gui.keyboard-navigation]
    fn picker_enter_chooses_current_device() {
        let mut state = AudioInputPickerState::new(vec![device("a", "A")]);

        assert_eq!(
            state.handle_key(AudioInputPickerKey::Enter),
            AudioInputPickerKeyResult::Choose
        );
        assert_eq!(
            state.selected_device().map(|device| device.id.as_str()),
            Some("a")
        );
    }

    #[test]
    // audio[verify gui.selection-dialog]
    fn selected_device_dialog_mentions_name_id_and_unknown_sample_rate() {
        let text = selected_audio_input_device_dialog_text(&device("endpoint-id", "Studio Mic"));

        assert!(text.contains("Studio Mic"));
        assert!(text.contains("endpoint-id"));
        assert!(text.contains("sample rate: unknown"));
    }
}
