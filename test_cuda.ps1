# ==============================================================================
# BildBlitz - GPU & NVIDIA CUDA Environment Verification Probe
# ==============================================================================

Write-Host "`n========================================================" -ForegroundColor Cyan
Write-Host "⚡ BildBlitz: NVIDIA CUDA & GPU Diagnostic Probe" -ForegroundColor Cyan
Write-Host "========================================================`n" -ForegroundColor Cyan

# 1. Check all installed Display Adapters / GPUs via WMI
Write-Host "🔍 [1/5] Querying Installed Graphics Hardware (WMI)..." -ForegroundColor Yellow
try {
    $adapters = Get-CimInstance Win32_VideoController | Select-Object Name, AdapterRAM, DriverVersion, Status
    foreach ($adapter in $adapters) {
        $ramMB = [math]::Round($adapter.AdapterRAM / 1MB, 0)
        Write-Host "  🖥️ Video Adapter: $($adapter.Name) | VRAM: ${ramMB}MB | Driver: $($adapter.DriverVersion) | Status: $($adapter.Status)" -ForegroundColor Cyan
    }
}
catch {
    Write-Host "  ⚠️ Could not query Win32_VideoController." -ForegroundColor Yellow
}

# 2. Check NVIDIA GPU & Driver via nvidia-smi
Write-Host "`n🔍 [2/5] Checking NVIDIA Driver & Hardware (nvidia-smi)..." -ForegroundColor Yellow
$nvidiaSmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if ($nvidiaSmi) {
    try {
        $smiOutput = & nvidia-smi --query-gpu=name, driver_version, memory.total, memory.free --format=csv, noheader
        Write-Host "  ✅ GPU Detected: $smiOutput" -ForegroundColor Green
        & nvidia-smi
    }
    catch {
        Write-Host "  ⚠️ nvidia-smi exists but encountered an error querying details." -ForegroundColor Yellow
    }
}
else {
    Write-Host "  ❌ nvidia-smi not found in PATH. No NVIDIA driver active or GPU missing." -ForegroundColor Red
}

# 2. Check CUDA Toolkit Directory Installations
Write-Host "`n🔍 [2/4] Checking CUDA Toolkit Installation Directories..." -ForegroundColor Yellow
$cudaBaseDir = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA"
if (Test-Path $cudaBaseDir) {
    $cudaVersions = Get-ChildItem $cudaBaseDir | Where-Object { $_.PSIsContainer }
    foreach ($ver in $cudaVersions) {
        Write-Host "  ✅ Found CUDA Toolkit Installation: $($ver.FullName)" -ForegroundColor Green
    }
}
else {
    Write-Host "  ⚠️ CUDA base directory not found at $cudaBaseDir" -ForegroundColor Yellow
}

# 3. Check CUDA Environment Variables
Write-Host "`n🔍 [3/4] Checking CUDA Environment Variables..." -ForegroundColor Yellow
$cudaPath = $env:CUDA_PATH
if (-not [string]::IsNullOrWhiteSpace($cudaPath)) {
    Write-Host "  ✅ `$env:CUDA_PATH is set to: $cudaPath" -ForegroundColor Green
}
else {
    Write-Host "  ⚠️ `$env:CUDA_PATH is currently not set in this session." -ForegroundColor Yellow
    # Auto-detect latest installed CUDA
    if (Test-Path $cudaBaseDir) {
        $latest = (Get-ChildItem $cudaBaseDir | Where-Object { $_.PSIsContainer } | Sort-Object Name -Descending | Select-Object -First 1)
        if ($latest) {
            Write-Host "  💡 Suggested fix: `$env:CUDA_PATH = `"$($latest.FullName)`"" -ForegroundColor Cyan
        }
    }
}

# 4. Check NVCC (NVIDIA CUDA Compiler)
Write-Host "`n🔍 [4/4] Checking NVCC Compiler..." -ForegroundColor Yellow
$nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
if ($nvcc) {
    $nvccVer = & nvcc --version | Select-String "release"
    Write-Host "  ✅ NVCC Compiler available: $nvccVer" -ForegroundColor Green
}
else {
    Write-Host "  ⚠️ nvcc not directly in current PATH. (Will resolve once CUDA bin is appended)." -ForegroundColor Yellow
}

Write-Host "`n========================================================" -ForegroundColor Cyan
Write-Host "Diagnostic Complete." -ForegroundColor Cyan
Write-Host "========================================================`n" -ForegroundColor Cyan
