# Gera o executável do Claude + Codex Usage no Windows.
# Uso (PowerShell, na pasta do projeto):
#   powershell -ExecutionPolicy Bypass -File .\build-windows.ps1
#   powershell -ExecutionPolicy Bypass -File .\build-windows.ps1 -SemInstalador   (só o .exe)
#
# Saída em .\release\
#   Claude + Codex Usage.exe  -> um único .exe, roda sem instalar
#   *-setup.exe / *.msi             -> instaladores (opcionais)

param(
  # Gera só o .exe portátil (pula instaladores NSIS/MSI, que baixam ferramentas da internet)
  [switch]$SemInstalador
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Have($cmd) { return [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

Write-Host "== Verificando pré-requisitos ==" -ForegroundColor Cyan

Write-Host "  Node.js..." -ForegroundColor DarkGray
if (-not (Have node)) {
  Write-Host "Node.js não encontrado. Instalando via winget..." -ForegroundColor Yellow
  winget install -e --id OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements
  $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
}

Write-Host "  Rust..." -ForegroundColor DarkGray
if (-not (Have cargo)) {
  Write-Host "Rust não encontrado. Instalando rustup via winget..." -ForegroundColor Yellow
  winget install -e --id Rustlang.Rustup --accept-source-agreements --accept-package-agreements
  $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User") + ";$env:USERPROFILE\.cargo\bin"
  rustup default stable-msvc
}

# O Rust no Windows precisa do linker do Visual Studio (Build Tools com "Desenvolvimento para desktop com C++").
Write-Host "  Visual Studio Build Tools..." -ForegroundColor DarkGray
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasVC = (Test-Path $vswhere) -and (& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $hasVC) {
  Write-Host "Visual Studio Build Tools (C++) não encontrado. Instalando (pode demorar)..." -ForegroundColor Yellow
  winget install -e --id Microsoft.VisualStudio.2022.BuildTools --accept-source-agreements --accept-package-agreements `
    --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
}

# Rede corporativa: alguns proxies/firewalls bloqueiam a consulta de revogação (CRL/OCSP) dos
# certificados e o cargo falha com CRYPT_E_NO_REVOCATION_CHECK. Desliga só essa consulta durante o
# build (a conexão continua criptografada e o certificado continua sendo validado).
$env:CARGO_HTTP_CHECK_REVOKE = "false"
$env:CARGO_NET_RETRY = "5"

# Memória: cada compilação do Rust em paralelo pode usar 1–2 GB. Com pouca RAM livre o rustc
# falha com "memory allocation ... failed" / STATUS_STACK_BUFFER_OVERRUN. Ajusta o paralelismo
# à memória livre (defina $env:CARGO_BUILD_JOBS antes para forçar outro valor).
# Lê a memória livre sem WMI (Get-CimInstance pode travar quando o serviço WMI está lento).
Write-Host "  Memória..." -ForegroundColor DarkGray
$freeGB = 4
try {
  Add-Type -AssemblyName Microsoft.VisualBasic
  $freeGB = [math]::Round((New-Object Microsoft.VisualBasic.Devices.ComputerInfo).AvailablePhysicalMemory / 1GB, 1)
} catch {
  Write-Host "  (não deu para medir a memória livre; usando 2 tarefas em paralelo)" -ForegroundColor DarkGray
}
if (-not $env:CARGO_BUILD_JOBS) {
  $cores = [Environment]::ProcessorCount
  $env:CARGO_BUILD_JOBS = [string][math]::Max(1, [math]::Min($cores, [math]::Floor($freeGB / 2)))
}
Write-Host "Memória livre: $freeGB GB -> compilando com $env:CARGO_BUILD_JOBS tarefa(s) em paralelo" -ForegroundColor DarkGray
if ($freeGB -lt 3) {
  Write-Host "Pouca memória livre. Feche navegador/Teams durante o build se der erro de memória." -ForegroundColor Yellow
}

Write-Host "== Instalando dependências (npm) ==" -ForegroundColor Cyan
npm ci
if ($LASTEXITCODE -ne 0) { Write-Host "Falha no 'npm ci'. Veja o erro acima." -ForegroundColor Red; exit 1 }

Write-Host "== Compilando (release) ==" -ForegroundColor Cyan
if ($SemInstalador) {
  npx tauri build --no-bundle
} else {
  npx tauri build
}
$buildOk = ($LASTEXITCODE -eq 0)

# O .exe é gerado antes dos instaladores; se só o empacotamento falhar (ele baixa NSIS/WiX
# da internet), ainda entregamos o executável portátil.
$exe = Get-ChildItem "src-tauri\target\release\*.exe" -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $exe) {
  Write-Host ""
  Write-Host "A compilação falhou antes de gerar o executável. Veja o primeiro erro acima." -ForegroundColor Red
  Write-Host "Se for erro de SSL/certificado, mande a mensagem para análise." -ForegroundColor Yellow
  exit 1
}

$out = Join-Path $PSScriptRoot "release"
New-Item -ItemType Directory -Force $out | Out-Null
# se o app estiver aberto, o .exe fica travado e não pode ser substituído
& (Join-Path $PSScriptRoot "parar.ps1") -Silencioso
Copy-Item $exe.FullName (Join-Path $out "Claude + Codex Usage.exe") -Force
Get-ChildItem "src-tauri\target\release\bundle\nsis\*.exe" -ErrorAction SilentlyContinue | Copy-Item -Destination $out -Force
Get-ChildItem "src-tauri\target\release\bundle\msi\*.msi" -ErrorAction SilentlyContinue | Copy-Item -Destination $out -Force

Write-Host ""
if ($buildOk) {
  Write-Host "Pronto! Arquivos em: $out" -ForegroundColor Green
} else {
  Write-Host "Executável portátil gerado, mas os instaladores falharam (provavelmente download do NSIS/WiX bloqueado pela rede)." -ForegroundColor Yellow
  Write-Host "O 'Claude + Codex Usage.exe' funciona sozinho. Para pular os instaladores: .\build-windows.ps1 -SemInstalador" -ForegroundColor Yellow
}
Get-ChildItem $out | Format-Table Name, @{n="MB";e={[math]::Round($_.Length/1MB,1)}}
