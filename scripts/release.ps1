<#
  TakeOff release script.

  Run from anywhere; it figures out the repo paths itself. Builds, signs,
  publishes a GitHub release, updates latest.json (the auto-updater feed),
  syncs the website's download link, and pushes both repos.

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts\release.ps1
    powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Version 0.6.0

  Requirements (one-time setup, not handled by this script):
    - GitHub CLI installed and authenticated: https://cli.github.com, then `gh auth login`
    - Signing key present at the path below (or pass -KeyPath)
#>

param(
    [string]$Version = "0.5.0",
    [string]$KeyPath = "$HOME\.tauri\applauncher.key",
    [string]$WebsiteRepo = "C:\Users\dougb\Desktop\tonic-tech-site",
    [string]$GitHubRepo = "Doogals/AppLauncher"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

function Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }

# --- 0. Prerequisites ------------------------------------------------------
Step "Checking prerequisites"

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "GitHub CLI ('gh') not found. Install it from https://cli.github.com, run 'gh auth login', then re-run this script."
}
gh auth status 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "'gh' is installed but not authenticated. Run 'gh auth login' first."
}
if (-not (Test-Path $KeyPath)) {
    throw "Signing key not found at $KeyPath. Pass -KeyPath if it's somewhere else."
}
Write-Host "OK - gh authenticated, signing key found."

# --- 1. Commit whatever's pending in the app repo --------------------------
Step "Committing pending changes in AppLauncher"

# Stale .git/index.lock left over from an earlier interrupted git operation
# blocks every git command with a confusing error. Only remove it if no git
# process is actually running right now -- a real in-progress git operation
# would still hold this lock legitimately.
$lockFile = Join-Path $RepoRoot ".git\index.lock"
if (Test-Path $lockFile) {
    if (Get-Process git -ErrorAction SilentlyContinue) {
        throw "A git process is currently running and holding $lockFile. Wait for it to finish, then re-run this script."
    }
    Write-Host "Removing stale .git/index.lock (no git process is running)."
    Remove-Item $lockFile -Force
}

git add -A
git reset -- app.msi app.msi.sig 2>$null | Out-Null   # belt-and-suspenders; .gitignore already excludes these
$pending = git status --porcelain
if ($pending) {
    $commitMsg = @"
Release v$Version

- Add "Edit Command Line" feature: attach a .bat/.ps1 script to Command
  Prompt/PowerShell items (create one in Notepad or link an existing file),
  runs automatically when the item launches
- Add duplicate button for items, including command-line attachments
- Fix: Microsoft Edge missing from suggested apps; "cmd" search now finds
  Command Prompt
- Fix: maximized browser windows now respect saved position/size
- Fix: assorted UI polish (button sizing/padding, color contrast, right-click
  menu alignment, orphaned command-file cleanup)
"@
    # Writing to a temp file and using -F instead of -m $commitMsg directly --
    # passing a string with embedded "quotes" as a native-command argument
    # confuses PowerShell's own argument parsing (same issue hit below with
    # gh release create --notes), so this sidesteps it entirely.
    $commitMsgFile = Join-Path $env:TEMP "takeoff-commit-msg.txt"
    Set-Content -Path $commitMsgFile -Value $commitMsg -NoNewline
    git commit -F $commitMsgFile
    Remove-Item $commitMsgFile -Force
    Write-Host "Committed."
} else {
    Write-Host "Nothing to commit, continuing."
}

# --- 2. Build ---------------------------------------------------------------
Step "Building (npm run tauri build) - this takes a few minutes"
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw "Build failed." }

# --- 3. Locate the MSI, copy to a space-free name if needed ----------------
Step "Locating built MSI"
$msiDir = Join-Path $RepoRoot "src-tauri\target\release\bundle\msi"
$msi = Get-ChildItem $msiDir -Filter "*.msi" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $msi) { throw "No MSI found in $msiDir" }
Write-Host "Found: $($msi.Name)"

# tauri signer chokes on filenames with spaces - the established workaround
# in this project is copying to a plain "app.msi" at the repo root first.
$signTarget = Join-Path $RepoRoot "app.msi"
Copy-Item $msi.FullName $signTarget -Force

# --- 4. Sign -----------------------------------------------------------------
Step "Signing installer"
# -f loads the private key FROM A FILE PATH. -k instead treats its argument
# as the literal key content and tries to base64-decode it directly -- that
# was the bug here, it was choking on the ":" in "C:\Users\..." as invalid
# base64.
npx tauri signer sign --no-password -f $KeyPath $signTarget
if ($LASTEXITCODE -ne 0) { throw "Signing failed." }
$sigFile = "$signTarget.sig"
if (-not (Test-Path $sigFile)) { throw "Expected signature file not found at $sigFile" }
$signature = (Get-Content $sigFile -Raw).Trim()

# --- 5. GitHub release -------------------------------------------------------
Step "Creating GitHub release v$Version"
$tag = "v$Version"
$finalMsiName = "TakeOff_${Version}_x64_en-US.msi"
$releaseAsset = Join-Path $RepoRoot $finalMsiName
Copy-Item $signTarget $releaseAsset -Force

$notes = @"
- New: attach a command line to Command Prompt/PowerShell items via "Edit Command Line" - create one in Notepad or link an existing script, runs automatically on launch
- New: duplicate button for items
- Fix: Microsoft Edge missing from suggested apps; "cmd" search now finds Command Prompt
- Fix: maximized browser windows now respect saved position/size
- Fix: assorted UI polish and orphaned-file cleanup
"@

# --notes-file instead of --notes $notes -- same reasoning as the commit
# message above: the embedded "quotes" in the text broke PowerShell's
# argument passing to native commands (this is what actually caused the
# "no matches found for `Command`" error on the previous run).
$notesFile = Join-Path $env:TEMP "takeoff-release-notes.txt"
Set-Content -Path $notesFile -Value $notes -NoNewline
gh release create $tag $releaseAsset --repo $GitHubRepo --title "TakeOff $tag" --notes-file $notesFile
if ($LASTEXITCODE -ne 0) { throw "GitHub release creation failed." }
Remove-Item $releaseAsset, $notesFile -Force

# --- 6. Update latest.json (the auto-updater feed) --------------------------
Step "Updating latest.json"
$pubDate = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.000Z")
$downloadUrl = "https://github.com/$GitHubRepo/releases/download/$tag/$finalMsiName"

$latestJsonObj = [ordered]@{
    version   = $Version
    notes     = "TakeOff $tag"
    pub_date  = $pubDate
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature
            url       = $downloadUrl
        }
    }
}
$latestJsonObj | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $RepoRoot "latest.json") -NoNewline
Write-Host "Download URL: $downloadUrl"

# --- 7. Sync the website's download link to match ---------------------------
Step "Syncing website download link"
$websiteFile = Join-Path $WebsiteRepo "takeoff.html"
if (Test-Path $websiteFile) {
    $pattern = 'https://github\.com/[^/]+/[^/]+/releases/download/v[\d.]+/[^"]+\.msi'
    (Get-Content $websiteFile -Raw) -replace $pattern, $downloadUrl | Set-Content $websiteFile -NoNewline
    Write-Host "Updated $websiteFile"
} else {
    Write-Warning "Website repo not found at $WebsiteRepo - update its download link manually."
}

# --- 8. Commit + push the app repo ------------------------------------------
Step "Committing and pushing latest.json"
git add latest.json
git commit -m "Update latest.json to $tag"
git push origin HEAD
git push origin $tag

# --- 9. Commit + push the website repo --------------------------------------
if (Test-Path $websiteFile) {
    Step "Committing and pushing website"
    Push-Location $WebsiteRepo
    git add -A
    git commit -m "Update TakeOff download link to $tag"
    git push
    Pop-Location
}

# --- Cleanup ------------------------------------------------------------------
Remove-Item $signTarget, $sigFile -Force -ErrorAction SilentlyContinue

Step "Done - TakeOff $tag is live"
Write-Host "Release: https://github.com/$GitHubRepo/releases/tag/$tag"
Write-Host "Download: $downloadUrl"
