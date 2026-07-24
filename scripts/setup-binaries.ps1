# Télécharge yt-dlp.exe et ffmpeg.exe dans src-tauri/binaries.
# Relance ce script pour mettre à jour les moteurs.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"  # accélère Invoke-WebRequest

$root = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $root "src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

# --- yt-dlp ---
$ytPath = Join-Path $binDir "yt-dlp.exe"
Write-Host "Téléchargement de yt-dlp..." -ForegroundColor Cyan
Invoke-WebRequest -Uri "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" -OutFile $ytPath
Write-Host "  -> $ytPath" -ForegroundColor Green

# --- ffmpeg + ffprobe ---
if (Test-Path (Join-Path $binDir "ffmpeg.exe")) {
    Write-Host "ffmpeg déjà présent, on saute." -ForegroundColor Yellow
} else {
    Write-Host "Téléchargement de ffmpeg (build essentials)..." -ForegroundColor Cyan
    $zip = Join-Path $env:TEMP "ffmpeg-essentials.zip"
    $extract = Join-Path $env:TEMP "ffmpeg-extract"
    Invoke-WebRequest -Uri "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -OutFile $zip
    if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
    Expand-Archive -Path $zip -DestinationPath $extract -Force
    $bin = Get-ChildItem -Path $extract -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
    $srcDir = $bin.DirectoryName
    Copy-Item (Join-Path $srcDir "ffmpeg.exe") (Join-Path $binDir "ffmpeg.exe") -Force
    Copy-Item (Join-Path $srcDir "ffprobe.exe") (Join-Path $binDir "ffprobe.exe") -Force
    Remove-Item $zip -Force
    Remove-Item -Recurse -Force $extract
    Write-Host "  -> ffmpeg.exe + ffprobe.exe" -ForegroundColor Green
}

# --- Real-ESRGAN (agrandissement IA, ncnn/Vulkan — tourne sur GPU NVIDIA/AMD/Intel) ---
$realDir = Join-Path $binDir "realesrgan"
if (Test-Path (Join-Path $realDir "realesrgan-ncnn-vulkan.exe")) {
    Write-Host "Real-ESRGAN déjà présent, on saute." -ForegroundColor Yellow
} else {
    Write-Host "Téléchargement de Real-ESRGAN (upscaling IA)..." -ForegroundColor Cyan
    $zip = Join-Path $env:TEMP "realesrgan.zip"
    $extract = Join-Path $env:TEMP "realesrgan-extract"
    Invoke-WebRequest -Uri "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip" -OutFile $zip
    if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
    Expand-Archive -Path $zip -DestinationPath $extract -Force
    $exe = Get-ChildItem -Path $extract -Recurse -Filter "realesrgan-ncnn-vulkan.exe" | Select-Object -First 1
    $srcDir = $exe.DirectoryName
    New-Item -ItemType Directory -Force -Path $realDir | Out-Null
    Copy-Item (Join-Path $srcDir "realesrgan-ncnn-vulkan.exe") $realDir -Force
    Copy-Item (Join-Path $srcDir "models") $realDir -Recurse -Force
    # Certaines archives incluent aussi les DLL Vulkan à côté de l'exe.
    Get-ChildItem -Path $srcDir -Filter "*.dll" | ForEach-Object { Copy-Item $_.FullName $realDir -Force }
    Remove-Item $zip -Force
    Remove-Item -Recurse -Force $extract
    Write-Host "  -> realesrgan-ncnn-vulkan.exe + models" -ForegroundColor Green
}

Write-Host "Terminé. Binaires dans $binDir" -ForegroundColor Green
