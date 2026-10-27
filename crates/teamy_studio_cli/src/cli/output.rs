use arbitrary::Arbitrary;
use std::io::{self, IsTerminal, Write};

use eyre::Context;
use facet::Facet;
use facet_pretty::{ColorMode, PrettyPrinter};

#[derive(Arbitrary, Facet, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Csv,
}

pub struct CliOutput {
    value: Option<Box<dyn CliOutputValue>>,
    /// Completion-time budget including output; None explicitly exempts interactive or unbounded work.
    pub elapsed_duration_warn_threshold: Option<std::time::Duration>,
}

impl core::fmt::Debug for CliOutput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CliOutput")
            .field("has_value", &self.value.is_some())
            .field(
                "elapsed_duration_warn_threshold",
                &self.elapsed_duration_warn_threshold,
            )
            .finish()
    }
}

trait CliOutputValue {
    fn render(
        &self,
        format: OutputFormat,
        stdout_is_terminal: bool,
    ) -> eyre::Result<Option<String>>;
}

struct FacetCliOutput<T> {
    value: T,
}

impl CliOutput {
    #[must_use]
    pub const fn none(elapsed_duration_warn_threshold: Option<std::time::Duration>) -> Self {
        Self {
            value: None,
            elapsed_duration_warn_threshold,
        }
    }

    #[must_use]
    pub fn facet<T>(value: T, elapsed_duration_warn_threshold: Option<std::time::Duration>) -> Self
    where
        T: Facet<'static> + 'static,
    {
        Self {
            value: Some(Box::new(FacetCliOutput { value })),
            elapsed_duration_warn_threshold,
        }
    }

    /// # Errors
    ///
    /// This function will return an error if the selected output format cannot be rendered
    /// or if the rendered output cannot be written to stdout.
    /// cli[impl global.output-format]
    /// cli[impl global.output-format.default-terminal]
    /// cli[impl global.output-format.default-redirected]
    pub fn emit(self, requested_format: Option<OutputFormat>) -> eyre::Result<()> {
        let terminal = io::stdout().is_terminal();
        self.emit_to(&mut io::stdout().lock(), requested_format, terminal)
    }

    /// Render into an explicit writer using the existing output-format policy.
    /// # Errors
    /// Returns serialization, write or flush errors.
    pub fn emit_to<W: Write + ?Sized>(
        self,
        stdout: &mut W,
        requested_format: Option<OutputFormat>,
        stdout_is_terminal: bool,
    ) -> eyre::Result<()> {
        let Some(output) = self.value else {
            return Ok(());
        };

        let format = requested_format.unwrap_or(if stdout_is_terminal {
            OutputFormat::Text
        } else {
            OutputFormat::Json
        });
        let Some(rendered) = output.render(format, stdout_is_terminal)? else {
            return Ok(());
        };

        stdout
            .write_all(rendered.as_bytes())
            .wrap_err("failed to write command output")?;
        if !rendered.ends_with('\n') {
            stdout
                .write_all(b"\n")
                .wrap_err("failed to terminate command output")?;
        }
        stdout.flush().wrap_err("failed to flush command output")?;
        Ok(())
    }
}

impl<T> CliOutputValue for FacetCliOutput<T>
where
    T: Facet<'static> + 'static,
{
    fn render(
        &self,
        format: OutputFormat,
        stdout_is_terminal: bool,
    ) -> eyre::Result<Option<String>> {
        let rendered = match format {
            OutputFormat::Text => PrettyPrinter::new()
                .with_colors(if stdout_is_terminal {
                    ColorMode::Always
                } else {
                    ColorMode::Never
                })
                .format(&self.value),
            OutputFormat::Json => facet_json::to_string_pretty(&self.value)
                .wrap_err("failed to serialize command output as JSON")?,
            OutputFormat::Csv => facet_csv::to_string(&self.value)
                .wrap_err("failed to serialize command output as CSV")?,
        };
        Ok(Some(rendered))
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    #[derive(Debug, Facet)]
    struct Receipt {
        count: u32,
    }
    #[test]
    fn budget_metadata_never_changes_result_formats() {
        for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Csv] {
            let mut reference = Vec::new();
            CliOutput::facet(
                Receipt { count: 3 },
                Some(std::time::Duration::from_secs(1)),
            )
            .emit_to(&mut reference, Some(format), false)
            .unwrap();
            for threshold in [None, Some(std::time::Duration::from_secs(30))] {
                let mut actual = Vec::new();
                CliOutput::facet(Receipt { count: 3 }, threshold)
                    .emit_to(&mut actual, Some(format), false)
                    .unwrap();
                assert_eq!(actual, reference);
            }
        }
    }
    #[test]
    fn explicit_exemption_and_failed_output_preserve_the_result() {
        let mut empty = Vec::new();
        CliOutput::none(None)
            .emit_to(&mut empty, None, false)
            .unwrap();
        assert!(empty.is_empty());
        let error = CliOutput::facet(
            Receipt { count: 3 },
            Some(std::time::Duration::from_secs(1)),
        )
        .emit_to(&mut FailingWriter, Some(OutputFormat::Json), false)
        .unwrap_err();
        assert!(error.to_string().contains("failed to write command output"));
    }
    #[derive(Debug)]
    struct FailingWriter;
    impl std::io::Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic write failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
