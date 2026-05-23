[CmdletBinding(PositionalBinding = $false)]
param(
	[switch]$Release,
	[switch]$NoOpenProfiler,
	[int]$SlowFramePaddingMs = 250,
	[int]$SlowFrameExportLimit = 5,
	[Parameter(Position = 0, ValueFromRemainingArguments = $true)]
	[string[]]$QueryArgs
)

$tracyLayerEnvVar = 'TEAMY_STUDIO_ENABLE_TRACY_LAYER'
$profilerFeatures = 'extended_observability,tracing_subscriber_tracy'

function Format-Elapsed {
	param(
		[Parameter(Mandatory = $true)]
		[TimeSpan]$Elapsed
	)

	if ($Elapsed.TotalHours -ge 1) {
		return $Elapsed.ToString("hh\:mm\:ss\.fff")
	}

	return $Elapsed.ToString("mm\:ss\.fff")
}

function Get-TracyCaptureProcesses {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CapturePath
	)

	$slugPattern = [Regex]::Escape($CapturePath)
	Get-CimInstance Win32_Process -Filter "Name = 'tracy-capture.exe'" -ErrorAction SilentlyContinue |
		Where-Object { $_.CommandLine -and $_.CommandLine -match $slugPattern } |
		ForEach-Object {
			try {
				Get-Process -Id $_.ProcessId -ErrorAction Stop
			} catch {
				$null
			}
		} |
		Where-Object { $_ -ne $null }
}

function Wait-ForTracyCaptureReady {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CapturePath,
		[Parameter(Mandatory = $true)]
		[TimeSpan]$Timeout
	)

	$deadline = (Get-Date).Add($Timeout)
	do {
		$processes = @(Get-TracyCaptureProcesses -CapturePath $CapturePath)
		if ($processes.Count -gt 0) {
			return $processes
		}

		Start-Sleep -Milliseconds 250
	} while ((Get-Date) -lt $deadline)

	throw "Timed out waiting $(Format-Elapsed $Timeout) for tracy-capture to start for $CapturePath"
}

function Wait-ForTracyCaptureExit {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CapturePath,
		[Parameter(Mandatory = $true)]
		[TimeSpan]$Timeout
	)

	$waitStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
	$deadline = (Get-Date).Add($Timeout)
	do {
		$processes = @(Get-TracyCaptureProcesses -CapturePath $CapturePath)
		if ($processes.Count -eq 0) {
			$waitStopwatch.Stop()
			return $waitStopwatch.Elapsed
		}

		Start-Sleep -Milliseconds 250
	} while ((Get-Date) -lt $deadline)

	$waitStopwatch.Stop()
	return $null
}

function Stop-TracyCaptureGracefully {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CapturePath
	)

	$processes = @(Get-TracyCaptureProcesses -CapturePath $CapturePath)
	if ($processes.Count -eq 0) {
		return [TimeSpan]::Zero
	}

	$shutdownStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
	foreach ($process in $processes) {
		if ($process.HasExited) {
			continue
		}

		$requestedClose = $false
		try {
			$requestedClose = $process.CloseMainWindow()
		} catch {
			$requestedClose = $false
		}

		if (-not $requestedClose) {
			try {
				Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
			} catch {
				# Ignore shutdown failures and let the wait/kill fallback below handle them.
			}
		}
	}

	$waitDeadline = (Get-Date).AddSeconds(30)
	do {
		Start-Sleep -Milliseconds 250
		$processes = @($processes | Where-Object {
			try {
				$_.Refresh()
				-not $_.HasExited
			} catch {
				$false
			}
		})
	} while ($processes.Count -gt 0 -and (Get-Date) -lt $waitDeadline)

	foreach ($process in $processes) {
		try {
			if (-not $process.HasExited) {
				$process.Kill()
			}
		} catch {
			# Ignore final cleanup failures.
		}
	}

	$shutdownStopwatch.Stop()
	return $shutdownStopwatch.Elapsed
}

function Test-IsTimelineLiveViewSelfTestCommand {
	param(
		[string[]]$Arguments
	)

	return $null -ne $Arguments -and $Arguments.Count -ge 2 -and
		$Arguments[0] -eq 'self-test' -and $Arguments[1] -eq 'timeline-live-view'
}

function Find-LatestTimelineLiveViewResults {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CacheDir
	)

	$resultsDir = Join-Path $CacheDir 'self-test\timeline-live-view'
	if (-not (Test-Path $resultsDir)) {
		return $null
	}

	Get-ChildItem -Path $resultsDir -Filter 'timeline-live-view-*.json' -File |
		Sort-Object LastWriteTimeUtc -Descending |
		Select-Object -First 1 -ExpandProperty FullName
}

function Export-TracyMessageCsv {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CapturePath,
		[Parameter(Mandatory = $true)]
		[string]$OutputPath
	)

	& tracy-csvexport.exe -m $CapturePath | Set-Content -Path $OutputPath -Encoding utf8
	return $OutputPath
}

function Get-TimelineLiveViewSampleStartMarkers {
	param(
		[Parameter(Mandatory = $true)]
		[string]$MessagesPath
	)

	$markers = @()
	if (-not (Test-Path $MessagesPath)) {
		return $markers
	}

	foreach ($line in Get-Content -Path $MessagesPath) {
		if ([string]::IsNullOrWhiteSpace($line) -or $line -eq 'MessageName,total_ns') {
			continue
		}

		if ($line -notmatch '^(?<message>.*),(?<totalNs>\d+)$') {
			continue
		}

		$messageText = $Matches['message']
		$totalNs = [int64]$Matches['totalNs']
		if ($messageText -notmatch 'timeline_live_view_self_test_sample_start') {
			continue
		}
		if ($messageText -notmatch 'sample_index = (?<sampleIndex>\d+)') {
			continue
		}

		$markers += [pscustomobject]@{
			sample_index = [int]$Matches['sampleIndex']
			total_ns = $totalNs
			message = $messageText
		}
	}

	return @($markers | Sort-Object sample_index, total_ns)
}

function Get-TimelineLiveViewSlowFrameWindows {
	param(
		[Parameter(Mandatory = $true)]
		[psobject]$Report,
		[Parameter(Mandatory = $true)]
		[object[]]$SampleStartMarkers,
		[Parameter(Mandatory = $true)]
		[int]$PaddingMs,
		[Parameter(Mandatory = $true)]
		[int]$Limit
	)

	$paddingNs = [int64]([Math]::Max($PaddingMs, 0)) * 1000000L
	$markerBySample = @{}
	foreach ($marker in $SampleStartMarkers) {
		if (-not $markerBySample.ContainsKey($marker.sample_index)) {
			$markerBySample[$marker.sample_index] = $marker
		}
	}

	$windows = @()
	foreach ($sample in $Report.samples) {
		$sampleIndex = [int]$sample.sample_index
		if (-not $markerBySample.ContainsKey($sampleIndex)) {
			continue
		}

		$sampleStartNs = [int64]$markerBySample[$sampleIndex].total_ns
		$rank = 0
		foreach ($frame in $sample.slowest_frames) {
			$rank += 1
			$traceStartNs = $sampleStartNs + ([int64]$frame.start_offset_ms * 1000000L)
			$traceEndNs = $sampleStartNs + ([int64]$frame.end_offset_ms * 1000000L)
			if ($traceEndNs -lt $traceStartNs) {
				$traceEndNs = $traceStartNs
			}

			$windows += [pscustomobject]@{
				sample_index = $sampleIndex
				slow_frame_rank = $rank
				frame_ms = [double]$frame.frame_ms
				start_offset_ms = [int64]$frame.start_offset_ms
				end_offset_ms = [int64]$frame.end_offset_ms
				trace_start_ns = $traceStartNs
				trace_end_ns = $traceEndNs
				export_start_ns = [Math]::Max([int64]0, $traceStartNs - $paddingNs)
				export_end_ns = $traceEndNs + $paddingNs
				visible_start_ns = [int64]$frame.visible_start_ns
				visible_end_ns = [int64]$frame.visible_end_ns
				visible_duration_ns = [int64]$frame.visible_duration_ns
				minimum_visible_pixels = [int64]$frame.minimum_visible_pixels
				dataset_item_count = [int64]$frame.dataset_item_count
				dataset_span_count = [int64]$frame.dataset_span_count
				dataset_event_count = [int64]$frame.dataset_event_count
				live_record_count = [int64]$frame.live_record_count
				live_span_count = [int64]$frame.live_span_count
				active_span_count = [int64]$frame.active_span_count
				row_count = 0
				csv_path = $null
			}
		}
	}

	return @(
		$windows |
			Sort-Object @{ Expression = 'frame_ms'; Descending = $true },
				@{ Expression = 'sample_index'; Descending = $false },
				@{ Expression = 'slow_frame_rank'; Descending = $false } |
			Select-Object -First ([Math]::Max($Limit, 1))
	)
}

function Export-TimelineLiveViewSlowFrameZoneCsvs {
	param(
		[Parameter(Mandatory = $true)]
		[string]$CapturePath,
		[Parameter(Mandatory = $true)]
		[object[]]$Windows,
		[Parameter(Mandatory = $true)]
		[string]$ArtifactDir
	)

	if ($Windows.Count -eq 0) {
		return @()
	}

	if (-not (Test-Path $ArtifactDir)) {
		$null = New-Item -ItemType Directory -Path $ArtifactDir -Force
	}

	$writers = @()
	foreach ($window in $Windows) {
		$csvFileName = 'slow-frame-sample{0:D2}-rank{1:D2}.csv' -f $window.sample_index, $window.slow_frame_rank
		$csvPath = Join-Path $ArtifactDir $csvFileName
		$window.csv_path = $csvPath
		$writers += [pscustomobject]@{
			window = $window
			writer = [System.IO.StreamWriter]::new(
				$csvPath,
				$false,
				[System.Text.UTF8Encoding]::new($false)
			)
		}
	}

	$header = $null
	try {
		foreach ($line in (& tracy-csvexport.exe -u $CapturePath)) {
			if ([string]::IsNullOrWhiteSpace($line)) {
				continue
			}

			if ($null -eq $header) {
				$header = $line
				foreach ($entry in $writers) {
					$entry.writer.WriteLine($header)
				}
				continue
			}

			$parts = $line.Split(',', 6)
			if ($parts.Length -lt 5) {
				continue
			}

			[int64]$rowStartNs = 0
			[int64]$rowDurationNs = 0
			if (-not [int64]::TryParse($parts[3], [ref]$rowStartNs)) {
				continue
			}
			if (-not [int64]::TryParse($parts[4], [ref]$rowDurationNs)) {
				continue
			}

			$rowEndNs = $rowStartNs + [Math]::Max($rowDurationNs, [int64]1)
			foreach ($entry in $writers) {
				if ($rowEndNs -lt $entry.window.export_start_ns -or $rowStartNs -gt $entry.window.export_end_ns) {
					continue
				}

				$entry.writer.WriteLine($line)
				$entry.window.row_count += 1
			}
		}
	}
	finally {
		foreach ($entry in $writers) {
			$entry.writer.Dispose()
		}
	}

	return $Windows
}

$overallStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$buildElapsed = $null
$captureLaunchElapsed = $null
$commandElapsed = $null
$cleanupElapsed = $null
$profilerElapsed = $null
$captureShutdownElapsed = [TimeSpan]::Zero
$captureFlushDelay = [TimeSpan]::FromSeconds(1)
$captureStartupTimeout = [TimeSpan]::FromSeconds(10)
$captureExitTimeout = [TimeSpan]::FromMinutes(5)

$captureDir = Join-Path $PSScriptRoot "tracy"
if (-not (Test-Path $captureDir)) {
	$null = New-Item -ItemType Directory -Path $captureDir
}

$slug = "$((Get-Date).ToString("yyyy-MM-dd_HH-mm-ss")).tracy"
$capturePath = Join-Path $captureDir $slug

if (-not (Get-Command tracy-capture.exe -ErrorAction SilentlyContinue)) {
	throw "tracy-capture.exe not found in PATH"
}


$profilerCommand = Get-Command tracy-profiler.exe -ErrorAction SilentlyContinue
$csvExportCommand = Get-Command tracy-csvexport.exe -ErrorAction SilentlyContinue

if (-not $NoOpenProfiler -and -not $profilerCommand) {
	Write-Warning "tracy-profiler.exe not found in PATH; capture will still be produced at $capturePath"
}

if (-not $csvExportCommand) {
	Write-Warning "tracy-csvexport.exe not found in PATH; CSV export will be skipped"
}

if (-not $QueryArgs -or $QueryArgs.Count -eq 0) {
	$QueryArgs = @()
}

$SlowFramePaddingMs = [Math]::Max($SlowFramePaddingMs, 0)
$SlowFrameExportLimit = [Math]::Max($SlowFrameExportLimit, 1)
$timelineLiveViewSelfTest = Test-IsTimelineLiveViewSelfTestCommand -Arguments $QueryArgs
$timelineLiveViewProfilerArtifactDir = $null
$timelineLiveViewCacheDir = $null
$timelineLiveViewResultsPath = $null
$timelineLiveViewMessagesPath = $null
$timelineLiveViewSlowFrameSummaryPath = $null
$previousTimelineLiveViewCacheDir = $null
$commandExitCode = 0
$commandFailureMessage = $null
$captureReadyForPostProcessing = $false
$capturePostProcessingSkipReason = $null
$profileOutputDirectory = if ($Release) { 'profiling' } else { 'debug' }
$profileLabel = if ($Release) { 'profiling release' } else { 'debug' }
$buildArgs = @('build', '--bin', 'teamy-studio', '--features', $profilerFeatures)
if ($Release) {
	$buildArgs += @('--profile', 'profiling')
}
$teamyStudioPath = Join-Path $PSScriptRoot "target\$profileOutputDirectory\teamy-studio.exe"
$appArgs = @($QueryArgs)
$appArgs += '--log-filter'
$appArgs += 'trace'

$buildStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
Write-Host "Building $profileLabel with features ${profilerFeatures}: cargo $($buildArgs -join ' ')"
& cargo @buildArgs
$buildStopwatch.Stop()
$buildElapsed = $buildStopwatch.Elapsed
Write-Host "Build time: $(Format-Elapsed $buildElapsed)"
if ($LASTEXITCODE -ne 0) {
	throw "cargo build failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $teamyStudioPath)) {
	throw "built Teamy Studio executable not found at $teamyStudioPath"
}

$captureDir = Join-Path $PSScriptRoot "tracy"
if (-not (Test-Path $captureDir)) {
	$null = New-Item -ItemType Directory -Path $captureDir
}

$slug = "$((Get-Date).ToString("yyyy-MM-dd_HH-mm-ss")).tracy"
$capturePath = Join-Path $captureDir $slug

if ($timelineLiveViewSelfTest) {
	$captureStem = [System.IO.Path]::GetFileNameWithoutExtension($capturePath)
	$timelineLiveViewProfilerArtifactDir = Join-Path $captureDir "$captureStem.timeline-live-view"
	$timelineLiveViewCacheDir = Join-Path $timelineLiveViewProfilerArtifactDir 'cache'
	$null = New-Item -ItemType Directory -Path $timelineLiveViewCacheDir -Force
	Write-Host "Timeline self-test artifacts: $timelineLiveViewProfilerArtifactDir"
}

Write-Host "Capture: $capturePath"
Write-Host "Logging Teamy Studio runtime performance information to $capturePath"
$capture = $null
$wt = Get-Command wt.exe -ErrorAction SilentlyContinue
$captureLaunchStopwatch = [System.Diagnostics.Stopwatch]::StartNew()

if ($wt) {
	Start-Process -FilePath "wt.exe" -ArgumentList @("-w", "new", "tracy-capture.exe", "-o", $capturePath)
} else {
	Write-Warning "wt.exe not found in PATH; launching tracy-capture in the current session"
	$capture = Start-Process -FilePath "tracy-capture.exe" -ArgumentList @("-o", $capturePath) -PassThru
}
$captureLaunchStopwatch.Stop()
$captureLaunchElapsed = $captureLaunchStopwatch.Elapsed
Write-Host "Capture launch time: $(Format-Elapsed $captureLaunchElapsed)"
Write-Host "Waiting for tracy-capture process to appear (timeout $(Format-Elapsed $captureStartupTimeout))"
$captureProcesses = @(Wait-ForTracyCaptureReady -CapturePath $capturePath -Timeout $captureStartupTimeout)
Write-Host "tracy-capture ready (pid: $($captureProcesses.Id -join ', '))"
Write-Host "Waiting 00:01.000 for tracy-capture to get ready"
Start-Sleep -Seconds 1

try {
	$previousTracyLayerSetting = [Environment]::GetEnvironmentVariable($tracyLayerEnvVar)
	$previousTimelineLiveViewCacheDir = [Environment]::GetEnvironmentVariable('TEAMY_STUDIO_CACHE_DIR')
	Set-Item -Path "Env:$tracyLayerEnvVar" -Value '1'
	if ($timelineLiveViewSelfTest) {
		Set-Item -Path 'Env:TEAMY_STUDIO_CACHE_DIR' -Value $timelineLiveViewCacheDir
	}
	Write-Host "Running built $profileLabel Teamy Studio with ${tracyLayerEnvVar}=1: $teamyStudioPath $($appArgs -join ' ')"
	$commandStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
	& $teamyStudioPath @appArgs
	$commandStopwatch.Stop()
	$commandElapsed = $commandStopwatch.Elapsed
	$commandExitCode = $LASTEXITCODE
	Write-Host "Traced app time: $(Format-Elapsed $commandElapsed)"
	if ($commandExitCode -ne 0) {
		$commandFailureMessage = "teamy-studio.exe failed with exit code $commandExitCode"
		Write-Warning $commandFailureMessage
	}
}
finally {
	if ($null -eq $previousTracyLayerSetting) {
		Remove-Item "Env:$tracyLayerEnvVar" -ErrorAction SilentlyContinue
	} else {
		Set-Item -Path "Env:$tracyLayerEnvVar" -Value $previousTracyLayerSetting
	}
	if ($null -eq $previousTimelineLiveViewCacheDir) {
		Remove-Item 'Env:TEAMY_STUDIO_CACHE_DIR' -ErrorAction SilentlyContinue
	} else {
		Set-Item -Path 'Env:TEAMY_STUDIO_CACHE_DIR' -Value $previousTimelineLiveViewCacheDir
	}
	$cleanupStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
	Write-Host "Waiting $(Format-Elapsed $captureFlushDelay) before watching tracy-capture shutdown"
	Start-Sleep -Milliseconds ([int]$captureFlushDelay.TotalMilliseconds)
	$naturalCaptureShutdownElapsed = Wait-ForTracyCaptureExit -CapturePath $capturePath -Timeout $captureExitTimeout
	if ($null -ne $naturalCaptureShutdownElapsed) {
		$captureShutdownElapsed = $naturalCaptureShutdownElapsed
		$captureReadyForPostProcessing = $true
		Write-Host "tracy-capture exited after the client disconnected"
	} else {
		$capturePostProcessingSkipReason = "tracy-capture did not finish saving within $(Format-Elapsed $captureExitTimeout)"
		Write-Warning "Timed out waiting $(Format-Elapsed $captureExitTimeout) for tracy-capture to finish saving; forcing shutdown"
		$captureShutdownElapsed = Stop-TracyCaptureGracefully -CapturePath $capturePath
	}
	$cleanupStopwatch.Stop()
	$cleanupElapsed = $cleanupStopwatch.Elapsed
	Write-Host "Capture cleanup time: $(Format-Elapsed $cleanupElapsed)"
	Write-Host "Capture shutdown wait: $(Format-Elapsed $captureShutdownElapsed)"
}

if ($captureReadyForPostProcessing -and -not (Test-Path $capturePath)) {
	$captureReadyForPostProcessing = $false
	$capturePostProcessingSkipReason = "tracy-capture exited but no capture file was written to $capturePath"
}

if ($timelineLiveViewSelfTest) {
	$timelineLiveViewResultsPath = Find-LatestTimelineLiveViewResults -CacheDir $timelineLiveViewCacheDir
	if ($timelineLiveViewResultsPath) {
		Write-Host "Timeline live-view self-test results: $timelineLiveViewResultsPath"
	} else {
		Write-Warning "Timeline live-view self-test did not write a results JSON under $timelineLiveViewCacheDir"
	}
}


if ($NoOpenProfiler) {
	if ($captureReadyForPostProcessing) {
		Write-Host "Skipping tracy-profiler launch (-NoOpenProfiler). Capture saved to $capturePath"
	} else {
		Write-Warning "Skipping tracy-profiler launch (-NoOpenProfiler). $capturePostProcessingSkipReason"
	}
} elseif ($profilerCommand -and $captureReadyForPostProcessing) {
	Write-Host "Displaying results from $capturePath"
	$profilerStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
	tracy-profiler.exe "$capturePath"
	$profilerStopwatch.Stop()
	$profilerElapsed = $profilerStopwatch.Elapsed
	Write-Host "Profiler time: $(Format-Elapsed $profilerElapsed)"
} else {
	if ($captureReadyForPostProcessing) {
		Write-Host "Capture saved to $capturePath"
	} else {
		Write-Warning "Skipping tracy-profiler launch because $capturePostProcessingSkipReason"
	}
}

$overallStopwatch.Stop()

if ($csvExportCommand -and $captureReadyForPostProcessing) {
	Write-Host "CSV from tracy-csvexport.exe $capturePath"
	tracy-csvexport.exe $capturePath

	if ($timelineLiveViewSelfTest -and $timelineLiveViewResultsPath) {
		$timelineLiveViewMessagesPath = Join-Path $timelineLiveViewProfilerArtifactDir 'messages.csv'
		Export-TracyMessageCsv -CapturePath $capturePath -OutputPath $timelineLiveViewMessagesPath | Out-Null
		$sampleStartMarkers = Get-TimelineLiveViewSampleStartMarkers -MessagesPath $timelineLiveViewMessagesPath
		$report = Get-Content -Path $timelineLiveViewResultsPath -Raw | ConvertFrom-Json
		$slowFrameWindows = Get-TimelineLiveViewSlowFrameWindows `
			-Report $report `
			-SampleStartMarkers $sampleStartMarkers `
			-PaddingMs $SlowFramePaddingMs `
			-Limit $SlowFrameExportLimit

		if ($slowFrameWindows.Count -eq 0) {
			Write-Warning "Could not align any slow-frame report entries with Tracy sample-start markers"
		} else {
			$exportedSlowFrameWindows = Export-TimelineLiveViewSlowFrameZoneCsvs `
				-CapturePath $capturePath `
				-Windows $slowFrameWindows `
				-ArtifactDir $timelineLiveViewProfilerArtifactDir
			$timelineLiveViewSlowFrameSummaryPath = Join-Path $timelineLiveViewProfilerArtifactDir 'slow-frame-windows.json'
			[pscustomobject]@{
				capture_path = $capturePath
				results_path = $timelineLiveViewResultsPath
				messages_path = $timelineLiveViewMessagesPath
				slow_frame_padding_ms = $SlowFramePaddingMs
				slow_frame_export_limit = $SlowFrameExportLimit
				windows = $exportedSlowFrameWindows
			} |
				ConvertTo-Json -Depth 8 |
				Set-Content -Path $timelineLiveViewSlowFrameSummaryPath -Encoding utf8
			Write-Host "Timeline slow-frame export summary: $timelineLiveViewSlowFrameSummaryPath"
		}
	}

} elseif ($csvExportCommand) {
	Write-Warning "Skipping tracy-csvexport because $capturePostProcessingSkipReason"
}

Write-Host "Timing summary:"
if ($buildElapsed) {
	Write-Host "  build:          $(Format-Elapsed $buildElapsed)"
}
Write-Host "  capture launch: $(Format-Elapsed $captureLaunchElapsed)"
if ($commandElapsed) {
	Write-Host "  traced app:     $(Format-Elapsed $commandElapsed)"
}
Write-Host "  cleanup:        $(Format-Elapsed $cleanupElapsed)"
Write-Host "  capture stop:   $(Format-Elapsed $captureShutdownElapsed)"
if ($profilerElapsed) {
	Write-Host "  profiler:       $(Format-Elapsed $profilerElapsed)"
}
Write-Host "  total wrapper:  $(Format-Elapsed $overallStopwatch.Elapsed)"

if ($timelineLiveViewSlowFrameSummaryPath) {
	Write-Host "  slow-frame csv: $timelineLiveViewSlowFrameSummaryPath"
}

if ($commandFailureMessage) {
	throw $commandFailureMessage
}
