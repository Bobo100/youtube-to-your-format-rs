# No param() block on purpose: a declared parameter makes `powershell -File` parse
# pass-through flags such as clippy's `-D warnings` as its own parameter names.
$Command = $args

$ErrorActionPreference = 'Stop'
if ($Command.Count -eq 0) { throw 'A command is required.' }

$packagesRoot = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages'
$winLibsRoot = Get-ChildItem -LiteralPath $packagesRoot -Directory -Filter 'BrechtSanders.WinLibs.POSIX.UCRT_*' -ErrorAction SilentlyContinue |
  Select-Object -First 1 -ExpandProperty FullName
$mingwBin = if ($winLibsRoot) { Join-Path $winLibsRoot 'mingw64\bin' } else { $null }

if (-not $mingwBin -or -not (Test-Path -LiteralPath (Join-Path $mingwBin 'gcc.exe'))) {
  throw 'WinLibs is missing. Run: winget install BrechtSanders.WinLibs.POSIX.UCRT'
}

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH = "$mingwBin;$cargoBin;$env:PATH"
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'

$executable = $Command[0]
$arguments = if ($Command.Count -gt 1) { $Command[1..($Command.Count - 1)] } else { @() }
& $executable @arguments
exit $LASTEXITCODE
