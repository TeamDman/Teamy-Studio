use crate::cli::audio::daemon::status::AudioDaemonStatusArgs;
use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Audio transcription daemon commands.
// audio[impl cli.daemon-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioDaemonArgs {
    /// The daemon subcommand to run.
    #[facet(args::subcommand)]
    pub command: AudioDaemonCommand,
}

/// Audio daemon subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum AudioDaemonCommand {
    // audio[impl cli.daemon-status]
    /// Report the Python daemon paths and tensor contract.
    Status(AudioDaemonStatusArgs),
}

impl AudioDaemonArgs {
    /// # Errors
    ///
    /// This function will return an error if the daemon action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            AudioDaemonCommand::Status(args) => args.invoke(app_home, cache_home),
        }
    }
}
