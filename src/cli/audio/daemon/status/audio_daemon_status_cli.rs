use arbitrary::Arbitrary;
use facet::Facet;

use crate::app::audio_transcription_daemon_status;
use crate::cli::output::CliOutput;

/// Report the local `WhisperX` daemon source, cache, and tensor contract.
// audio[impl cli.daemon-status]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct AudioDaemonStatusArgs;

impl AudioDaemonStatusArgs {
    /// # Errors
    ///
    /// This function currently does not fail; it returns a static contract report using the
    /// resolved Teamy cache directory.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = app_home;
        Ok(CliOutput::facet(audio_transcription_daemon_status(
            cache_home,
        )))
    }
}
