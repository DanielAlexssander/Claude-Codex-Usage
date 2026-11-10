# Abre o app em modo desenvolvimento (recarrega sozinho ao salvar arquivos).
# Uso: powershell -ExecutionPolicy Bypass -File .\dev.ps1
#      powershell -ExecutionPolicy Bypass -File .\dev.ps1 -Log     (mostra no terminal os dados lidos)
param([switch]$Log)

Set-Location $PSScriptRoot

# Terminal aberto antes de instalar o Rust não enxerga o cargo
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (($env:Path -split ";") -notcontains $cargoBin) { $env:Path += ";$cargoBin" }
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Host "cargo não encontrado. Rode antes o build-windows.ps1 para instalar o Rust." -ForegroundColor Red
  exit 1
}

# Limita o paralelismo à memória livre (evita "memory allocation failed")
# (sem WMI: Get-CimInstance pode travar quando o serviço WMI está lento)
$freeGB = 4
try {
  Add-Type -AssemblyName Microsoft.VisualBasic
  $freeGB = [math]::Round((New-Object Microsoft.VisualBasic.Devices.ComputerInfo).AvailablePhysicalMemory / 1GB, 1)
} catch {}
if (-not $env:CARGO_BUILD_JOBS) {
  $env:CARGO_BUILD_JOBS = [string][math]::Max(1, [math]::Min([Environment]::ProcessorCount, [math]::Floor($freeGB / 2)))
}
Write-Host "Memória livre: $freeGB GB -> $env:CARGO_BUILD_JOBS tarefa(s) em paralelo" -ForegroundColor DarkGray

# Fecha instâncias anteriores do app e o Vite órfão na porta 1420 (senão o app não abre de novo)
& (Join-Path $PSScriptRoot "parar.ps1") -Silencioso

if ($Log) { $env:CLAUDE_USAGE_DEBUG = "1" }
if (-not (Test-Path node_modules)) { npm install }
npm run app:dev
