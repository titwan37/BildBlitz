# ==============================================================================
# BildBlitz - CUDA Build & Launch Script
# ==============================================================================

Write-Host "⚡ Starting BildBlitz with NVIDIA CUDA acceleration..." -ForegroundColor Cyan

# 1. Detect and configure CUDA Toolkit PATH
$cudaBaseDir = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA"
if (Test-Path $cudaBaseDir) {
    $latestCuda = (Get-ChildItem $cudaBaseDir | Where-Object { $_.PSIsContainer } | Sort-Object Name -Descending | Select-Object -First 1)
    if ($latestCuda) {
        $cudaDir = $latestCuda.FullName
        $env:CUDA_PATH = $cudaDir
        $env:PATH = "$cudaDir\bin\x64;$cudaDir\bin;" + $env:PATH
        Write-Host "  ✅ CUDA Environment configured: $cudaDir" -ForegroundColor Green
    }
} else {
    Write-Host "  ⚠️ CUDA default directory not found. Proceeding with system PATH..." -ForegroundColor Yellow
}

# 2. Build or Run
$choice = Read-Host "Select: [1] Run Debug, [2] Build & Run Release with CUDA, [3] Check only (Default: 2)"
if ([string]::IsNullOrWhiteSpace($choice)) { $choice = "2" }

if ($choice -eq "1") {
    cargo run --features cuda
} elseif ($choice -eq "2") {
    cargo run --release --features cuda
} elseif ($choice -eq "3") {
    cargo check --features cuda
}
