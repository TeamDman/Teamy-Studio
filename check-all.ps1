param(
	[switch]$Full,
	[switch]$VerboseBuild
)

function Invoke-CargoWithOptionalVerbosity {
	param(
		[Parameter(Mandatory = $true)]
		[string[]]$Arguments
	)

	if ($VerboseBuild) {
		Write-Host -ForegroundColor DarkGray "> cargo $($Arguments -join ' ')"
		cargo @Arguments --verbose
		return
	}

	cargo @Arguments
}

function Write-BuildNetworkDiagnostics {
	if (-not $VerboseBuild) {
		return
	}

	Write-Host -ForegroundColor Cyan "Build/network diagnostics"
	Write-Host "cwd: $(Get-Location)"
	Write-Host "rustc: $(rustc --version)"
	Write-Host "cargo: $(cargo --version)"
	foreach ($name in @(
		"CARGO_HOME",
		"CARGO_HTTP_DEBUG",
		"CARGO_HTTP_MULTIPLEXING",
		"CARGO_NET_OFFLINE",
		"CARGO_REGISTRIES_CRATES_IO_PROTOCOL",
		"HTTPS_PROXY",
		"HTTP_PROXY"
	)) {
		$value = [Environment]::GetEnvironmentVariable($name)
		if ([string]::IsNullOrWhiteSpace($value)) {
			$value = "<unset>"
		}
		Write-Host "$name=$value"
	}

	$cargoHome = if ([string]::IsNullOrWhiteSpace($env:CARGO_HOME)) {
		Join-Path $env:USERPROFILE ".cargo"
	} else {
		$env:CARGO_HOME
	}
	Write-Host "resolved CARGO_HOME=$cargoHome"
	foreach ($relativePath in @("registry\index", "registry\cache", "git\db", "git\checkouts")) {
		$path = Join-Path $cargoHome $relativePath
		if (Test-Path $path) {
			$count = (Get-ChildItem -LiteralPath $path -Force -ErrorAction SilentlyContinue | Measure-Object).Count
			Write-Host "$relativePath entries: $count"
		} else {
			Write-Host "$relativePath entries: <missing>"
		}
	}
}

function Write-CargoSourceDiagnostics {
	if (-not $VerboseBuild) {
		return
	}

	Write-Host -ForegroundColor Cyan "Cargo package sources"
	$cargoMetadata = cargo metadata --format-version 1 --locked | ConvertFrom-Json -AsHashtable
	$cargoMetadata["packages"] |
		Where-Object { $_["source"] } |
		Group-Object { $_["source"] } |
		Sort-Object Count -Descending |
		ForEach-Object {
			Write-Host ("{0,4} {1}" -f $_.Count, $_.Name)
		}
}

function Invoke-Step {
	param(
		[Parameter(Mandatory = $true)]
		[string]$Label,
		[Parameter(Mandatory = $true)]
		[scriptblock]$Action
	)

	Write-Host -ForegroundColor Yellow "Running $Label..."
	& $Action
	if ($LASTEXITCODE -ne 0) {
		throw "$Label failed with exit code $LASTEXITCODE"
	}
}

function Get-NonTracyTestFeatureArgs {
	param(
		[switch]$Full
	)

	# tool[impl tests.exclude-tracy-feature]
	# tool[impl tests.avoid-tracy-firewall-prompt]
	$defaultExcludedFeatures = @("default", "tracy")
	if (-not $Full) {
		$defaultExcludedFeatures += "font-snapshot-tests"
	}
	if ($VerboseBuild) {
		Write-Host -ForegroundColor DarkGray "> cargo metadata --no-deps --format-version 1"
	}
	$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
	$pkg = if ($metadata.packages.Count -eq 1) {
		$metadata.packages[0]
	} else {
		$manifestPath = (Resolve-Path (Join-Path (Get-Location) 'Cargo.toml')).Path
		$metadata.packages |
			Where-Object { $_.manifest_path -eq $manifestPath } |
			Select-Object -First 1
	}
	if (-not $pkg) {
		throw "Could not determine root package from cargo metadata"
	}

	$features = @($pkg.features.PSObject.Properties.Name | Where-Object { $_ -notin $defaultExcludedFeatures })
	if ($features.Count -gt 0) {
		return @("--features", ($features -join ","))
	}

	return @()
}

function Stop-TeamyStudioProcessIfRunning {
	$running = Get-Process -Name 'teamy-studio' -ErrorAction SilentlyContinue
	if ($null -eq $running) {
		return
	}

	Write-Host -ForegroundColor DarkYellow 'Stopping running teamy-studio.exe so build outputs can be replaced...'
	taskkill /F /IM teamy-studio.exe | Out-Null
}

Invoke-Step -Label "format check" -Action {
	Invoke-CargoWithOptionalVerbosity -Arguments @("fmt", "--all")
}

Invoke-Step -Label "clippy lint check" -Action {
	# cargo clippy --all-targets --all-features -- -D warnings
	Invoke-CargoWithOptionalVerbosity -Arguments @("clippy", "--all-features", "--", "-D", "warnings")
}

Invoke-Step -Label "build" -Action {
	Stop-TeamyStudioProcessIfRunning
	if ($VerboseBuild) {
		Write-BuildNetworkDiagnostics
		Write-CargoSourceDiagnostics
		Invoke-CargoWithOptionalVerbosity -Arguments @("build", "--all-features", "--locked")
	} else {
		cargo build --all-features --quiet
	}
}

Invoke-Step -Label "tests" -Action {
	Stop-TeamyStudioProcessIfRunning
	$featuresArg = Get-NonTracyTestFeatureArgs -Full:$Full
	if ($VerboseBuild) {
		$testArguments = @("test") + $featuresArg + @("--locked")
		Invoke-CargoWithOptionalVerbosity -Arguments $testArguments
	} else {
		cargo test @featuresArg --quiet
	}
}

Invoke-Step -Label "tracey status" -Action {
	tracey query status
}