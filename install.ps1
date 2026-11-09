# Install the gitty terminal app on Windows (x64), no admin needed:
#   irm https://raw.githubusercontent.com/lettdev/gitty/main/install.ps1 | iex
# Installs gitty.exe into %LOCALAPPDATA%\Programs\gitty and adds it to your PATH.
# Run it again to update. Pick a version with:  $env:GITTY_VERSION = "v0.2.3"
# Runs inside a script block so an error never closes the terminal window.

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is much faster without the progress bar
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'lettdev/gitty'
    $v = $env:GITTY_VERSION
    if (-not $v) {
        $v = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
    }
    if (-not $v.StartsWith('v')) { $v = "v$v" }
    $name = "gitty-$v-windows-x64"
    $url = "https://github.com/$repo/releases/download/$v/$name.zip"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) "gitty-install-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Force $tmp | Out-Null
    try {
        Write-Host "Downloading gitty $v..."
        $zip = Join-Path $tmp "$name.zip"
        Invoke-WebRequest $url -OutFile $zip -UseBasicParsing

        # check the download against the checksum published with the release
        # (saved to a file: release assets come back as bytes, not text, in Windows PowerShell 5.1)
        $sumFile = Join-Path $tmp "$name.zip.sha256"
        Invoke-WebRequest "$url.sha256" -OutFile $sumFile -UseBasicParsing
        $expected = ((Get-Content $sumFile -Raw).Trim() -split '\s+')[0].ToLower()
        $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) { throw "Checksum mismatch for $name.zip (expected $expected, got $actual). Nothing was installed." }

        Expand-Archive $zip $tmp -Force
        $bin = Join-Path $env:LOCALAPPDATA 'Programs\gitty'
        New-Item -ItemType Directory -Force $bin | Out-Null
        Copy-Item (Join-Path $tmp "$name\gitty.exe") $bin -Force
        Unblock-File (Join-Path $bin 'gitty.exe')

        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ((";$userPath;") -notlike "*;$bin;*") {
            [Environment]::SetEnvironmentVariable('Path', ("$userPath;$bin").TrimStart(';'), 'User')
            Write-Host "Added $bin to your PATH."
        }
        if ((";$env:Path;") -notlike "*;$bin;*") { $env:Path = "$env:Path;$bin" }   # this window too

        Write-Host "Installed $(& (Join-Path $bin 'gitty.exe') --version) to $bin" -ForegroundColor Green
        if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
            Write-Host "gitty needs Git for Windows: https://git-scm.com (or: winget install Git.Git)" -ForegroundColor Yellow
        }
        Write-Host "Run 'gitty' inside a project folder. Other terminal windows need a restart to see it."
    }
    catch {
        Write-Host "gitty install failed: $($_.Exception.Message)" -ForegroundColor Red
    }
    finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
