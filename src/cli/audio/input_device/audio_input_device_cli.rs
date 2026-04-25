use crate::cli::audio::input_device::list::AudioInputDeviceListArgs;
use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Audio input-device commands.
// audio[impl cli.input-device-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioInputDeviceArgs {
    /// The input-device subcommand to run.
    #[facet(args::subcommand)]
    pub command: AudioInputDeviceCommand,
}

/// Audio input-device subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum AudioInputDeviceCommand {
    // audio[impl cli.input-device-list]
    /// List active Windows audio capture endpoints.
    List(AudioInputDeviceListArgs),
}

impl AudioInputDeviceArgs {
    /// # Errors
    ///
    /// This function will return an error if the input-device action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            AudioInputDeviceCommand::List(args) => args.invoke(app_home, cache_home),
        }
    }
}
