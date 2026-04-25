use crate::cli::audio::input_device::AudioInputDeviceArgs;
use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Audio commands.
// audio[impl cli.audio-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioArgs {
    /// The audio subcommand to run.
    #[facet(args::subcommand)]
    pub command: AudioCommand,
}

/// Audio subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum AudioCommand {
    // audio[impl cli.input-device-command]
    /// Enumerate and inspect audio input devices.
    InputDevice(AudioInputDeviceArgs),
}

impl AudioArgs {
    /// # Errors
    ///
    /// This function will return an error if the audio action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            AudioCommand::InputDevice(args) => args.invoke(app_home, cache_home),
        }
    }
}
