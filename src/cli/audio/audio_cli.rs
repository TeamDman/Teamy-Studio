use crate::cli::audio::daemon::AudioDaemonArgs;
use crate::cli::audio::input_device::AudioInputDeviceArgs;
use crate::cli::audio::transcribe::AudioTranscribeArgs;
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
    // audio[impl cli.daemon-command]
    /// Inspect and manage the local `WhisperX` Python daemon.
    Daemon(AudioDaemonArgs),
    // audio[impl cli.input-device-command]
    /// Enumerate and inspect audio input devices.
    InputDevice(AudioInputDeviceArgs),
    // audio[impl cli.transcribe-command]
    /// Transcribe a 16 kHz mono PCM WAV file with the Burn Whisper backend.
    Transcribe(AudioTranscribeArgs),
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
            AudioCommand::Daemon(args) => args.invoke(app_home, cache_home),
            AudioCommand::InputDevice(args) => args.invoke(app_home, cache_home),
            AudioCommand::Transcribe(args) => args.invoke(app_home, cache_home),
        }
    }
}
