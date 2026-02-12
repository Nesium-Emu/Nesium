# Clear Nesium ROM cache to regenerate thumbnails
# Run this after code changes to see new artwork

$cacheFile = "$env:APPDATA\nesium\roms_cache.json"

if (Test-Path $cacheFile) {
    Write-Host "Deleting old cache: $cacheFile" -ForegroundColor Yellow
    Remove-Item $cacheFile -Force
    Write-Host "✓ Cache deleted!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Now launch Nesium and click 'Rescan' to regenerate with improved thumbnails" -ForegroundColor Cyan
} else {
    Write-Host "Cache file not found at: $cacheFile" -ForegroundColor Red
    Write-Host "The cache might be in a different location or not created yet." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Press any key to continue..."
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")

