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
    [string]$Version = "0.6.4",
    [string]$KeyPath = "$HOME\.tauri\applauncher.key",
    [string]$WebsiteRepo = "C:\Users\dougb\Desktop\tonic-tech-site",
    [string]$GitHubRepo = "Doogals/AppLauncher",
    # SSL.com eSigner CodeSignTool - Authenticode signing so Windows SmartScreen
    # shows "Tonic Technologies LLC" instead of "Unknown Publisher".
    [string]$CodeSignToolPath = "C:\CodeSignTool\CodeSignTool.bat",
    # Ships an UNSIGNED installer. Only for testing - never for a public release.
    [switch]$SkipCodeSigning,
    # Validates the code-signing setup and exits without building or releasing.
    # Surfaces a missing tool or bad credential in seconds instead of after a
    # full build.
    [switch]$VerifySigningSetup,
    # Prompts for the SSL.com credentials once and stores them encrypted, so
    # they never have to be re-entered before a release. Run this once.
    [switch]$SaveCredentials
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

function Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }

# Locates CodeSignTool.bat. Checks the -CodeSignToolPath parameter, then the
# CODESIGNTOOL_PATH environment variable, then the places the SSL.com zip
# normally gets extracted to. Returns $null if it can't be found, so callers
# decide whether that is fatal.
function Resolve-CodeSignTool($explicitPath) {
    if ($explicitPath -and (Test-Path $explicitPath)) { return $explicitPath }
    if ($env:CODESIGNTOOL_PATH -and (Test-Path $env:CODESIGNTOOL_PATH)) { return $env:CODESIGNTOOL_PATH }
    $candidates = @(
        "C:\CodeSignTool\CodeSignTool.bat",
        "$HOME\CodeSignTool\CodeSignTool.bat",
        "$HOME\Desktop\CodeSignTool\CodeSignTool.bat",
        "$HOME\Downloads\CodeSignTool\CodeSignTool.bat"
    )
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    # The zip extracts to a versioned folder name (CodeSignTool-v1.3.2-windows
    # and similar), so glob for it rather than guessing the version.
    foreach ($root in @("C:\", "$HOME", "$HOME\Desktop", "$HOME\Downloads")) {
        if (-not (Test-Path $root)) { continue }
        $hit = Get-ChildItem $root -Filter "CodeSignTool*" -Directory -ErrorAction SilentlyContinue |
               ForEach-Object { Join-Path $_.FullName "CodeSignTool.bat" } |
               Where-Object { Test-Path $_ } |
               Select-Object -First 1
        if ($hit) { return $hit }
    }
    return $null
}

# Reads the four SSL.com credentials from the environment. credential_id is
# optional per SSL.com's docs -- CodeSignTool defaults to the only certificate
# on the account when it is omitted, and Tonic Technologies has just one.
# Runs CodeSignTool with its own folder as the working directory.
# CodeSignTool.bat resolves its bundled JRE, lib and conf folders RELATIVE to
# itself, so invoking it from anywhere else fails with the unhelpful
# "The system cannot find the path specified." before it ever reaches SSL.com.
function Invoke-CodeSignTool($toolPath, [string[]]$arguments) {
    $dir = Split-Path $toolPath -Parent

    # Quote the VALUE half of every -name=value argument.
    #
    # Two separate problems this solves:
    #   1. PowerShell hands arguments to a .bat through cmd.exe, and cmd treats
    #      & | < > ^ as operators. An unquoted password containing any of them
    #      is torn apart before CodeSignTool ever sees it, surfacing as cmd's
    #      "The syntax of the command is incorrect."
    #   2. SSL.com's own documentation calls for exactly this: passwords with
    #      special characters must be passed as -password="...".
    #
    # Building one command line and handing it to cmd /c keeps the quoting
    # intact, rather than letting PowerShell re-quote it on the way through.
    # A literal double-quote inside a quoted argument is escaped as \" — the
    # convention the Windows C runtime uses when splitting a command line, which
    # is what Java's launcher follows. Previously this threw and demanded the
    # password be changed; escaping is friendlier and costs nothing.
    $quoted = $arguments | ForEach-Object {
        if ($_ -match '^(-[A-Za-z_]+)=(.*)$') {
            $argName  = $Matches[1]
            $argValue = $Matches[2] -replace '"', '\"'
            '{0}="{1}"' -f $argName, $argValue
        }
        else { $_ }
    }
    $cmdLine = '"{0}" {1}' -f $toolPath, ($quoted -join ' ')

    Push-Location $dir
    try {
        # Out-Host, NOT a bare call. A PowerShell function returns everything
        # written to the pipeline, so without this the tool's stdout is returned
        # alongside the exit code as an array -- and `if ($rc -ne 0)` is then
        # true even on success. Out-Host writes straight to the console and
        # contributes nothing to the pipeline, so only $LASTEXITCODE comes back.
        & cmd.exe /c $cmdLine 2>&1 | Out-Host
        return $LASTEXITCODE
    } finally {
        Pop-Location
    }
}

# Sanity-checks that the CodeSignTool folder actually contains the pieces the
# .bat needs. The SSL.com zip contains a versioned top-level folder, and it is
# easy to end up with either a nested copy or a lone .bat with no runtime.
function Test-CodeSignToolLayout($toolPath) {
    $dir = Split-Path $toolPath -Parent
    $hasJre = @(Get-ChildItem $dir -Directory -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match '^(jdk|jre|java)' }).Count -gt 0
    $hasLib = Test-Path (Join-Path $dir "lib")
    if (-not ($hasJre -or $hasLib)) {
        Write-Warning "$dir contains CodeSignTool.bat but no bundled runtime (no jdk*/jre*/lib folder)."
        Write-Warning "The zip extracts to a versioned folder - move the CONTENTS of e.g. CodeSignTool-v1.3.2-windows into $dir, not just the .bat."
    }
}

# Where saved signing credentials live. Deliberately OUTSIDE the repo so they
# can never be committed, and alongside the Tauri signing key.
$script:CredPath = Join-Path $HOME ".takeoff\signing.cred"

# Turns a SecureString back into plain text. PowerShell 7 has
# ConvertFrom-SecureString -AsPlainText, but Windows PowerShell 5.1 -- what runs
# here by default -- does not, so go through the marshaller.
function ConvertFrom-SecureStringPlain($secure) {
    if (-not $secure) { return $null }
    $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try   { [Runtime.InteropServices.Marshal]::PtrToStringAuto($ptr) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
}

# Prompts once and stores the credentials encrypted, so releases stop requiring
# secrets to be re-entered every time.
#
# ConvertFrom-SecureString uses Windows DPAPI: the file can only be decrypted by
# THIS Windows account on THIS machine, so copying it elsewhere yields nothing.
# Meaningfully safer than setx, which would leave the password in the registry
# as plain text.
#
# Read-Host -AsSecureString echoes nothing, so the values never appear on
# screen, in scrollback, or in a screenshot.
function Save-SigningCredentials {
    Step "Saving SSL.com signing credentials"
    Write-Host "Stored encrypted at $script:CredPath (this Windows account only)."
    Write-Host "Password and TOTP secret are not echoed as you type.`n"

    $u   = Read-Host "SSL.com username"
    $p   = Read-Host "SSL.com password" -AsSecureString
    $t   = Read-Host "TOTP secret"      -AsSecureString
    $cid = Read-Host "Credential ID (optional - press Enter to skip)"

    $dir = Split-Path $script:CredPath -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }

    [pscustomobject]@{
        Username     = $u
        Password     = ($p | ConvertFrom-SecureString)
        TotpSecret   = ($t | ConvertFrom-SecureString)
        CredentialId = $cid
    } | Export-Clixml -Path $script:CredPath

    Write-Host "`nSaved. Future releases pick these up automatically." -ForegroundColor Green
}

# Environment first (useful for CI or a one-off override), then the saved file.
# credential_id stays optional: CodeSignTool defaults to the only certificate on
# the account, and Tonic Technologies has just one.
function Get-SigningCredentials {
    $c = @{
        Username     = $env:SSL_COM_USERNAME
        Password     = $env:SSL_COM_PASSWORD
        CredentialId = $env:SSL_COM_CREDENTIAL_ID
        TotpSecret   = $env:SSL_COM_TOTP_SECRET
        Source       = "environment"
    }

    if ((-not $c.Username -or -not $c.Password -or -not $c.TotpSecret) -and (Test-Path $script:CredPath)) {
        try {
            $saved = Import-Clixml -Path $script:CredPath
            if (-not $c.Username)   { $c.Username   = $saved.Username }
            if (-not $c.Password)   { $c.Password   = ConvertFrom-SecureStringPlain ($saved.Password   | ConvertTo-SecureString) }
            if (-not $c.TotpSecret) { $c.TotpSecret = ConvertFrom-SecureStringPlain ($saved.TotpSecret | ConvertTo-SecureString) }
            if (-not $c.CredentialId -and $saved.CredentialId) { $c.CredentialId = $saved.CredentialId }
            $c.Source = "saved credentials"
        } catch {
            Write-Warning "Could not read $script:CredPath - $($_.Exception.Message)"
            Write-Warning "Re-run with -SaveCredentials to recreate it."
        }
    }

    $missing = @()
    if (-not $c.Username)   { $missing += "SSL_COM_USERNAME" }
    if (-not $c.Password)   { $missing += "SSL_COM_PASSWORD" }
    if (-not $c.TotpSecret) { $missing += "SSL_COM_TOTP_SECRET" }
    $c.Missing = $missing
    return $c
}

# --- 0a. Optional: store credentials, then exit -----------------------------
# Handled before the prerequisite checks so credentials can be set up without
# gh being authenticated yet.
if ($SaveCredentials) {
    Save-SigningCredentials
    exit 0
}

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

# --- 0b. Optional: verify code-signing setup, then exit ---------------------
if ($VerifySigningSetup) {
    Step "Verifying code-signing setup"

    $cst = Resolve-CodeSignTool $CodeSignToolPath
    if (-not $cst) {
        throw "CodeSignTool not found. Download the Windows build from https://ssl.com/download/codesigntool-for-windows/ and extract it to C:\CodeSignTool, or pass -CodeSignToolPath."
    }
    Write-Host "Found CodeSignTool: $cst"
    Test-CodeSignToolLayout $cst

    $cred = Get-SigningCredentials
    if ($cred.Missing.Count -gt 0) {
        throw "Missing environment variable(s): $($cred.Missing -join ', ')."
    }
    Write-Host "Credentials present for user: $($cred.Username) (from $($cred.Source))"

    # Catches the most common self-inflicted error: pasting the placeholder text
    # from the setup instructions instead of the real value.
    foreach ($f in @("Password","TotpSecret","Username")) {
        if ($cred.$f -match '^your-|placeholder|paste_') {
            throw "SSL_COM_$($f.ToUpper()) still looks like placeholder text ('$($cred.$f)'). Set it to the real value."
        }
    }

    # Round-trips the account credentials against SSL.com and prints the
    # credential ID(s). Confirms username/password without consuming a signature.
    Step "Querying SSL.com for credential IDs"
    $rc = Invoke-CodeSignTool $cst @("get_credential_ids", "-username=$($cred.Username)", "-password=$($cred.Password)")
    if ($rc -ne 0) {
        throw @"
get_credential_ids failed (exit $rc). Read the CodeSignTool output above:
  'The system cannot find the path specified' -> CodeSignTool's own files are
      missing or misplaced. Re-extract the zip so the .bat sits alongside its
      jdk/lib folders.
  'The syntax of the command is incorrect'    -> a credential contains a
      character cmd.exe treats specially. A '%' in the password is the usual
      culprit and cannot be escaped reliably - change the SSL.com password to
      alphanumerics to rule it out.
  'Error: Invalid username or password'       -> the credentials are simply wrong.
"@
    }

    # If a credential ID is configured, confirm it resolves to a real certificate
    # and show its subject and expiry.
    if ($cred.CredentialId) {
        Step "Checking certificate for credential ID $($cred.CredentialId)"
        $rc = Invoke-CodeSignTool $cst @("credential_info", "-credential_id=$($cred.CredentialId)", "-username=$($cred.Username)", "-password=$($cred.Password)")
        if ($rc -ne 0) {
            throw "credential_info failed (exit $rc) - SSL_COM_CREDENTIAL_ID may be wrong."
        }
    } else {
        Write-Host "SSL_COM_CREDENTIAL_ID not set - fine if the account holds exactly one code signing certificate (see the list above)."
    }

    Write-Host "`nSigning setup looks good. Re-run without -VerifySigningSetup to do a real release." -ForegroundColor Green
    Write-Host "Note: the TOTP secret is only exercised during an actual signature, so it is not validated here."
    exit 0
}

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

- New: Items grouped by virtual desktop in the group editor
- New: Drag items and desktop sections to set launch order
- New: Desktop and screen dropdowns on every item
- New: Add Item is its own window, with per-desktop add buttons
- Fix: Drag-and-drop never worked - Tauri's handler was swallowing DOM drag events
- Fix: Failed desktop switches no longer corrupt later switches in the same launch
- Fix: Creating a missing desktop no longer switches away from it
- Fix: Editing a URL item's bookmarks no longer wipes its desktop assignment
- Fix: Edit Layout can be reopened while windows are still open elsewhere
- Fix: Layout windows size to the monitor instead of a fixed 800x600
- Fix: Steamworks Common Redistributables removed from the Steam picker
- Style: Widget drop shadow removed, rounded corners restored
- Style: Group editor is twice as wide
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

# --- 3b. Authenticode-sign the MSI via SSL.com eSigner ----------------------
# ORDER IS CRITICAL: this MUST run BEFORE `tauri signer sign` below.
# Authenticode signing rewrites the MSI's bytes. The Tauri updater signature is
# a hash over those bytes, so signing in the other order would produce an
# updater signature for a file that no longer exists -- every existing install
# would reject the update as corrupt, silently, with no way to push a fix.
#
# Using eSigner (cloud) rather than the YubiKey deliberately: the hardware token
# prompts for a PIV PIN on every signature, which would stop this script dead
# partway through. CodeSignTool derives its own TOTP from the secret, so the
# release stays unattended.
if ($SkipCodeSigning) {
    Write-Warning "SKIPPING Authenticode signing - this installer will show 'Unknown Publisher'. Do not publish it."
} else {
    Step "Authenticode-signing installer (SSL.com eSigner)"

    $cst = Resolve-CodeSignTool $CodeSignToolPath
    if (-not $cst) {
        throw "CodeSignTool not found. Download the Windows build from https://ssl.com/download/codesigntool-for-windows/ and extract it to C:\CodeSignTool, or pass -CodeSignToolPath. (The Windows package bundles its own Java runtime - nothing else to install.)"
    }
    Write-Host "Using CodeSignTool: $cst"
    Test-CodeSignToolLayout $cst

    $cred = Get-SigningCredentials
    if ($cred.Missing.Count -gt 0) {
        throw "Missing environment variable(s): $($cred.Missing -join ', '). Set them in this shell, or pass -SkipCodeSigning to build an unsigned installer for testing."
    }
    foreach ($f in @("Password","TotpSecret","Username")) {
        if ($cred.$f -match '^your-|placeholder|paste_') {
            throw "SSL_COM_$($f.ToUpper()) still looks like placeholder text ('$($cred.$f)'). Set it to the real value."
        }
    }

    # CodeSignTool writes the signed file into a directory rather than editing in
    # place. Without -output_dir_path it prompts before overwriting, which would
    # hang an unattended release, so always sign into a temp dir and copy back.
    $signedDir = Join-Path $RepoRoot "signed-tmp"
    if (Test-Path $signedDir) { Remove-Item $signedDir -Recurse -Force }
    New-Item -ItemType Directory -Path $signedDir | Out-Null

    # -credential_id is only required when the account holds more than one code
    # signing certificate; omitted, CodeSignTool uses the single one. Built as an
    # array so the argument is simply absent when the env var isn't set.
    $cstArgs = @("sign")
    if ($cred.CredentialId) { $cstArgs += "-credential_id=$($cred.CredentialId)" }
    $cstArgs += @(
        "-username=$($cred.Username)",
        "-password=$($cred.Password)",
        "-totp_secret=$($cred.TotpSecret)",
        "-program_name=TakeOff",          # shown in the MSI's install dialog
        "-input_file_path=$signTarget",
        "-output_dir_path=$signedDir"
    )

    $rc = Invoke-CodeSignTool $cst $cstArgs
    if ($rc -ne 0) {
        Remove-Item $signedDir -Recurse -Force -ErrorAction SilentlyContinue
        throw "CodeSignTool failed (exit $rc). 'invalid otp' usually means SSL_COM_TOTP_SECRET doesn't match the enrolled certificate. Also confirm the certificate is enrolled for eSigner."
    }

    $signedMsi = Join-Path $signedDir (Split-Path $signTarget -Leaf)
    if (-not (Test-Path $signedMsi)) {
        Remove-Item $signedDir -Recurse -Force -ErrorAction SilentlyContinue
        throw "CodeSignTool reported success but produced no file at $signedMsi."
    }
    Copy-Item $signedMsi $signTarget -Force
    Remove-Item $signedDir -Recurse -Force

    # Verify the signature actually landed. CodeSignTool has been known to exit 0
    # having done nothing, and an unsigned release is worse than a failed one.
    $sig = Get-AuthenticodeSignature $signTarget
    if ($sig.Status -ne "Valid") {
        throw "Authenticode verification failed after signing: $($sig.Status) - $($sig.StatusMessage)"
    }
    Write-Host "Authenticode OK - signed by: $($sig.SignerCertificate.Subject)"
}

# --- 4. Sign (Tauri updater signature) --------------------------------------
Step "Signing installer"
# -f loads the private key FROM A FILE PATH. -k instead treats its argument
# as the literal key content and tries to base64-decode it directly -- that
# was the bug here, it was choking on the ":" in "C:\Users\..." as invalid
# base64. --no-password isn't a real flag on this CLI version (it suggested
# --password instead) -- passing an explicit empty password is the actual
# way to skip the interactive prompt for a key that has none. Using
# --password= (attached, one token) rather than --password "" (two tokens)
# because Windows PowerShell 5.1 silently drops empty-string arguments when
# calling a native exe, which turned "" into nothing and left --password
# with no value at all.
npx tauri signer sign "--password=" -f $KeyPath $signTarget
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
**Launch order and virtual desktops**
- Items in the group editor are now grouped by the virtual desktop they launch on, so the list shows the real launch order instead of hiding it.
- Drag items to reorder them, and drag a desktop heading to change which desktop launches first. Dragging an item into another desktop's section moves it to that desktop.
- Every item now has Desktop and Screen dropdowns, so you no longer need to open Edit Layout just to change where something opens.

**Adding items**
- Add Item is now its own window rather than a menu inside the group editor.
- When your items span several desktops you get an Add button for each one, so a new item goes straight into the right section.
- Steamworks Common Redistributables no longer appears in the Steam list. It is not a game and could not be launched.

**Fixes**
- Drag-and-drop in the group editor never actually worked; Tauri's own file-drop handler was swallowing the events.
- A virtual desktop switch that timed out was recorded as successful, which threw off every later switch in the same launch and scattered windows across the wrong desktops.
- Creating a missing desktop then immediately switching away from it.
- Editing a URL item's bookmarks silently cleared its desktop assignment.
- Edit Layout could not be reopened while a layout window was still open on another desktop.
- Layout editor windows opened at a fixed size that was far too small on scaled displays; they now size to your monitor and show the app name rather than its full path.
- Bookmarks: a Clear All button, press Enter to add several custom URLs or IP addresses, and Just Open Browser is available when editing.

**Appearance**
- The launcher bar and floating groups no longer have a drop shadow, and the bar's rounded corners are back.
- The group editor window is twice as wide, with more room for each item.
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

# --- 7. Sync the website's download link and version text to match --------
Step "Syncing website download link and version"
$websiteFile = Join-Path $WebsiteRepo "takeoff.html"
if (Test-Path $websiteFile) {
    $urlPattern = 'https://github\.com/[^/]+/[^/]+/releases/download/v[\d.]+/[^"]+\.msi'
    $versionPattern = 'Current version: v[\d.]+'
    (Get-Content $websiteFile -Raw) -replace $urlPattern, $downloadUrl -replace $versionPattern, "Current version: $tag" |
        Set-Content $websiteFile -NoNewline
    Write-Host "Updated $websiteFile"
} else {
    Write-Warning "Website repo not found at $WebsiteRepo - update its download link and version text manually."
}

# --- 8. Commit + push the app repo ------------------------------------------
Step "Committing and pushing latest.json"
git add latest.json
git commit -m "Update latest.json to $tag"
# Pushing HEAD:master explicitly, NOT just HEAD -- the auto-updater feed
# (tauri.conf.json's updater endpoint) reads latest.json from the master
# branch specifically. Pushing bare "HEAD" pushes to a remote branch with
# the SAME NAME as whatever is currently checked out locally, which silently
# missed master entirely the first time this ran from a feature branch --
# the push "succeeded" against the wrong branch with no error at all.
git push origin HEAD:master
if ($LASTEXITCODE -ne 0) { throw "git push to master failed -- latest.json was committed locally but NOT pushed, so the in-app updater will NOT see this release. Run 'git push origin HEAD:master' yourself in $RepoRoot, then re-run from here." }
# No separate tag push here: `gh release create` above already created the
# tag directly on GitHub. There is no matching local tag (this script never
# runs `git tag`), so `git push origin $tag` would just fail/no-op every time.

# --- 9. Commit + push the website repo --------------------------------------
if (Test-Path $websiteFile) {
    Step "Committing and pushing website"
    Push-Location $WebsiteRepo
    git add -A
    git commit -m "Update TakeOff download link to $tag"
    git push
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        throw "git push (website) failed -- the download-link update was committed locally but NOT pushed, so the live site still shows the old link. Run 'git push' yourself in $WebsiteRepo."
    }
    Pop-Location
}

# --- Cleanup ------------------------------------------------------------------
Remove-Item $signTarget, $sigFile -Force -ErrorAction SilentlyContinue

Step "Done - TakeOff $tag is live"
Write-Host "Release: https://github.com/$GitHubRepo/releases/tag/$tag"
Write-Host "Download: $downloadUrl"
