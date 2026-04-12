$ErrorActionPreference = "Stop"

cargo build --release --locked
cargo install --path . --force --locked

$cargoHome = if ($env:CARGO_HOME) {
	$env:CARGO_HOME
} else {
	Join-Path $HOME ".cargo"
}

$cargoBin = Join-Path $cargoHome "bin"
$releaseDir = Join-Path $PSScriptRoot "target\release"

foreach ($fileName in @("teamy-studio.exe", "ghostty-vt.dll", "conpty.dll", "OpenConsole.exe")) {
	$source = Join-Path $releaseDir $fileName
	if (-not (Test-Path $source)) {
		throw "Missing runtime artifact: $source"
	}

	Copy-Item $source (Join-Path $cargoBin $fileName) -Force
}