# Installs dz, the Dazzle compiler, from a GitHub release (Windows):
#
#     irm https://dazzle-lang.github.io/install.ps1 | iex
#
# Settings, all optional, read from the environment:
#   DAZZLE_HOME            where it goes (default %USERPROFILE%\.dazzle; dz ends up in its bin\)
#   DAZZLE_VERSION         a release tag such as v0.1.0 (default: the latest)
#   DAZZLE_NO_MODIFY_PATH  set to 1 to leave the user's PATH alone
#
# Everything is in one function called on the last line, so a download cut short runs
# nothing at all.

function Install-Dazzle {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # the progress bar makes Invoke-WebRequest crawl
    $repo = 'https://github.com/dazzle-lang/Dazzle'

    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -ne 'AMD64') {
        throw "no release for $arch Windows yet; build from source: https://dazzle-lang.github.io/start/install/#build-from-source"
    }
    $archive = 'dazzle-x86_64-windows.zip'
    $home_ = if ($env:DAZZLE_HOME) { $env:DAZZLE_HOME } else { Join-Path $env:USERPROFILE '.dazzle' }
    $base = if ($env:DAZZLE_VERSION) {
        "$repo/releases/download/$env:DAZZLE_VERSION"
    } else {
        "$repo/releases/latest/download"
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("dazzle-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $version = if ($env:DAZZLE_VERSION) { $env:DAZZLE_VERSION } else { 'latest release' }
        Write-Host "downloading $archive ($version)"
        Invoke-WebRequest -UseBasicParsing "$base/$archive" -OutFile (Join-Path $tmp $archive)
        Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')
        $want = (Get-Content (Join-Path $tmp 'SHA256SUMS') |
                Where-Object { $_ -match " $([regex]::Escape($archive))$" }) -replace ' .*', ''
        $got = (Get-FileHash (Join-Path $tmp $archive) -Algorithm SHA256).Hash.ToLower()
        if ($want -ne $got) { throw "$archive does not match its checksum; try again" }

        Expand-Archive (Join-Path $tmp $archive) -DestinationPath $tmp
        # Replace what an earlier install left, but only what a release puts there.
        New-Item -ItemType Directory -Force -Path $home_ | Out-Null
        foreach ($dir in 'bin', 'lib') {
            $old = Join-Path $home_ $dir
            if (Test-Path $old) { Remove-Item -Recurse -Force $old }
        }
        Get-ChildItem (Join-Path $tmp 'dazzle-x86_64-windows') |
            Move-Item -Destination $home_ -Force
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    $bin = Join-Path $home_ 'bin'
    $dz = Join-Path $bin 'dz.exe'
    Write-Host "installed $(& $dz --version) in $home_"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $onPath = ($userPath -split ';') -contains $bin
    if (-not $onPath) {
        if ($env:DAZZLE_NO_MODIFY_PATH -eq '1') {
            Write-Host ""
            Write-Host "dz is not on your PATH. Add $bin to it."
        } else {
            $new = if ($userPath) { "$bin;$userPath" } else { $bin }
            [Environment]::SetEnvironmentVariable('Path', $new, 'User')
            $env:Path = "$bin;$env:Path"
            Write-Host "added $bin to your PATH; open a new terminal for other windows to see it"
        }
    }

    # dz writes C++ and hands it to a C++ compiler, which is the one thing it needs from
    # the machine. Say so now rather than at the first build.
    $cxx = 'clang++', 'g++', 'c++' | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue }
    if (-not $cxx) {
        Write-Host ""
        Write-Host "dz also needs a C++17 compiler, and none was found. The simplest is LLVM's clang:"
        Write-Host "    winget install LLVM.LLVM"
        Write-Host "which uses the Visual Studio Build Tools' C++ libraries:"
        Write-Host "    winget install Microsoft.VisualStudio.2022.BuildTools --override `"--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive`""
    }

    Write-Host ""
    Write-Host "Start a project:  dz new hello; cd hello; dz run"
}

Install-Dazzle
