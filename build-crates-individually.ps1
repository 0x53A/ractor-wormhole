# Enabled features may leak between crates in a workspace,
# this build script builds each crate individually 
# to verify that they don't accidentally depend on features that aren't actually enabled for the specific crate.


# Get the root directory
$rootDir = $PSScriptRoot

# Find all Cargo.toml files but exclude the workspace root one
$cargoFiles = Get-ChildItem -Path $rootDir -Filter "Cargo.toml" -Recurse | 
    Where-Object { $_.DirectoryName -ne $rootDir }

# Create a list to track failures
$failedBuilds = @()
$totalCrates = $cargoFiles.Count
$successCount = 0

# Process each Cargo.toml file
foreach ($cargoFile in $cargoFiles) {
    $crateDir = $cargoFile.DirectoryName
    $crateName = Split-Path $crateDir -Leaf
    
    Write-Host "Building crate: $crateName in $crateDir" -ForegroundColor Cyan
    
    # Navigate to the crate directory and build
    Push-Location $crateDir
    try {
        cargo build
        
        if ($LASTEXITCODE -eq 0) {
            Write-Host "Successfully built crate: $crateName" -ForegroundColor Green
            $successCount++
        } else {
            Write-Host "Failed to build crate: $crateName" -ForegroundColor Red
            $failedBuilds += @{
                Name = $crateName
                Directory = $crateDir
                ExitCode = $LASTEXITCODE
            }
        }
    } finally {
        # Return to previous directory
        Pop-Location
    }
    
    Write-Host "" # Empty line for readability
}

# Print summary report
Write-Host "===== BUILD SUMMARY =====" -ForegroundColor Cyan
Write-Host "Total crates: $totalCrates" -ForegroundColor Cyan
Write-Host "Successful builds: $successCount" -ForegroundColor Green

if ($failedBuilds.Count -gt 0) {
    Write-Host "Failed builds: $($failedBuilds.Count)" -ForegroundColor Red
    Write-Host ""
    Write-Host "FAILED CRATES:" -ForegroundColor Red
    
    foreach ($failure in $failedBuilds) {
        Write-Host "  - $($failure.Name) (Exit code: $($failure.ExitCode))" -ForegroundColor Red
        Write-Host "    Directory: $($failure.Directory)" -ForegroundColor Red
    }
    
    # Exit with failure code
    Write-Host ""
    Write-Host "Build process completed with errors." -ForegroundColor Red
    exit 1
} else {
    Write-Host "All crates built successfully!" -ForegroundColor Green
    exit 0
}