use arbitrary::Arbitrary;
use facet::Facet;

use crate::app::{AudioInputDeviceListReport, list_active_audio_input_devices};
use crate::cli::output::CliOutput;

/// List active Windows recording devices.
// audio[impl cli.input-device-list]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioInputDeviceListArgs;

impl AudioInputDeviceListArgs {
    /// # Errors
    ///
    /// This function will return an error if Windows audio endpoints cannot be enumerated.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = app_home;
        let _ = cache_home;
        Ok(CliOutput::facet(AudioInputDeviceListReport {
            devices: list_active_audio_input_devices()?,
        }))
    }
}
