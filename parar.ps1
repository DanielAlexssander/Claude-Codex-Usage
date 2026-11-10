# Encerra qualquer instância do Claude Usage que tenha ficado aberta (inclusive escondida
# na bandeja) e o servidor de desenvolvimento (Vite) preso na porta 1420.
# Uso: powershell -ExecutionPolicy Bypass -File .\parar.ps1
param([switch]$Silencioso)

# Pega qualquer nome que o executável tenha recebido ("Claude Usage", "claude-usage"...).
# Não afeta o Claude Desktop ("claude.exe").
$procs = Get-Process -ErrorAction SilentlyContinue | Where-Object {
  $n = $_.ProcessName
  ($n -like "*claude*usage*") -or ($n -like "*claude-usage*")
}
foreach ($p in $procs) {
  if (-not $Silencioso) { Write-Host "Encerrando app: $($p.ProcessName) (PID $($p.Id))" -ForegroundColor Yellow }
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}

# Servidor de desenvolvimento órfão (npm run dev / vite) ocupando a porta 1420
$conns = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
foreach ($c in $conns) {
  $p = Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue
  if ($p -and $p.ProcessName -match "^(node|esbuild)$") {
    if (-not $Silencioso) { Write-Host "Liberando porta 1420: $($p.ProcessName) (PID $($p.Id))" -ForegroundColor Yellow }
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  }
}

if (-not $Silencioso) {
  if (-not $procs -and -not $conns) { Write-Host "Nada para encerrar: o app não estava rodando." -ForegroundColor Green }
  else { Write-Host "Pronto. Pode abrir o app de novo." -ForegroundColor Green }
}
