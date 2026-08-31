@echo off
setlocal EnableDelayedExpansion

REM ============================================================================
REM  TakeOff backup
REM
REM  COPIES everything needed to continue development on another machine.
REM  Nothing is deleted or moved from this machine.
REM
REM  Deliberately EXCLUDES node_modules, src-tauri\target and dist. Those are
REM  several GB, regenerate from `npm install` and `cargo build`, and would
REM  dominate the copy time for no benefit.
REM
REM  Deliberately INCLUDES the Tauri updater signing key, which lives OUTSIDE
REM  the repo at %USERPROFILE%\.tauri. That key is the one irreplaceable item
REM  here: lose it and no future release can ever update an existing install,
REM  because every shipped build verifies against the public key baked into it.
REM  There is no remote rotation and no recovery path.
REM
REM  NOT included, by design: SSL.com credentials and the LemonSqueezy API key.
REM  Those are secrets and do not belong in a plaintext backup. They live in
REM  environment variables and Cloudflare respectively. See RESTORE-README.txt.
REM ============================================================================

set "SRC_REPO=%~dp0"
if "%SRC_REPO:~-1%"=="\" set "SRC_REPO=%SRC_REPO:~0,-1%"
set "SRC_KEY=%USERPROFILE%\.tauri"
set "SRC_CONFIG=%LOCALAPPDATA%\TakeOff"
set "SRC_SITE=%USERPROFILE%\Desktop\tonic-tech-site"

echo.
echo ==========================================================
echo   TakeOff Backup
echo ==========================================================
echo.
echo   Source repo : %SRC_REPO%
echo.
echo   This COPIES files. Nothing on this machine is moved or
echo   deleted.
echo.

set "DEST="
set /p "DEST=Enter the destination folder (e.g. E:\Backups): "

if not defined DEST (
    echo.
    echo   No destination entered. Nothing was copied.
    goto :end
)

REM Strip surrounding quotes if the user pasted a quoted path.
set "DEST=%DEST:"=%"
if "%DEST:~-1%"=="\" set "DEST=%DEST:~0,-1%"

if not exist "%DEST%\" (
    echo.
    echo   Destination does not exist: %DEST%
    echo   Create it first, or check the drive letter is correct.
    goto :end
)

REM Sortable timestamp. Uses PowerShell rather than %DATE%, whose format
REM depends on Windows regional settings and would produce inconsistent
REM or invalid folder names on some machines.
for /f "usebackq delims=" %%i in (`powershell -NoProfile -Command "Get-Date -Format yyyy-MM-dd_HHmmss"`) do set "STAMP=%%i"

set "TARGET=%DEST%\TakeOff-backup-%STAMP%"
echo.
echo   Destination : %TARGET%
echo.

mkdir "%TARGET%" 2>nul
if not exist "%TARGET%\" (
    echo   Could not create %TARGET%
    echo   Check the drive is writable and not full.
    goto :end
)

set "FAILED="

echo ----------------------------------------------------------
echo  1 of 4  Repository
echo ----------------------------------------------------------
robocopy "%SRC_REPO%" "%TARGET%\AppLauncher" /E /NFL /NDL /NJH /NJS /R:2 /W:2 ^
    /XD node_modules target dist .wrangler signed-tmp ^
    /XF app.msi app.msi.sig
REM Robocopy uses exit codes 0-7 for success, 8 and above for genuine errors.
if errorlevel 8 set "FAILED=1" & echo   ERROR copying repository.

echo.
echo ----------------------------------------------------------
echo  2 of 4  Tauri signing key  ^(irreplaceable^)
echo ----------------------------------------------------------
if exist "%SRC_KEY%\" (
    robocopy "%SRC_KEY%" "%TARGET%\dot-tauri" /E /NFL /NDL /NJH /NJS /R:2 /W:2
    if errorlevel 8 set "FAILED=1" & echo   ERROR copying signing key.
) else (
    echo   WARNING: %SRC_KEY% not found.
    echo   Without the signing key you cannot publish updates to existing
    echo   installs. Locate applauncher.key before relying on this backup.
    set "FAILED=1"
)

echo.
echo ----------------------------------------------------------
echo  3 of 4  App config  ^(your groups and settings^)
echo ----------------------------------------------------------
if exist "%SRC_CONFIG%\" (
    robocopy "%SRC_CONFIG%" "%TARGET%\LocalAppData-TakeOff" /E /NFL /NDL /NJH /NJS /R:2 /W:2
    if errorlevel 8 set "FAILED=1" & echo   ERROR copying config.
) else (
    echo   Skipped - %SRC_CONFIG% not found.
)

echo.
echo ----------------------------------------------------------
echo  4 of 4  Website repo
echo ----------------------------------------------------------
if exist "%SRC_SITE%\" (
    robocopy "%SRC_SITE%" "%TARGET%\tonic-tech-site" /E /NFL /NDL /NJH /NJS /R:2 /W:2 ^
        /XD node_modules .netlify
    if errorlevel 8 set "FAILED=1" & echo   ERROR copying website repo.
) else (
    echo   Skipped - %SRC_SITE% not found.
)

REM --- Restore notes --------------------------------------------------------
set "README=%TARGET%\RESTORE-README.txt"
> "%README%" echo TakeOff backup taken %STAMP%
>> "%README%" echo.
>> "%README%" echo WHAT IS HERE
>> "%README%" echo   AppLauncher\             The app repo, minus build output.
>> "%README%" echo   dot-tauri\               Updater signing key. IRREPLACEABLE.
>> "%README%" echo   LocalAppData-TakeOff\    Your groups and settings.
>> "%README%" echo   tonic-tech-site\         The website repo.
>> "%README%" echo.
>> "%README%" echo TO RESTORE ON A NEW MACHINE
>> "%README%" echo   1. Copy AppLauncher\ to the Desktop.
>> "%README%" echo   2. Copy dot-tauri\ to %%USERPROFILE%%\.tauri
>> "%README%" echo      This must be in place before any release, or the
>> "%README%" echo      updater signature cannot be produced.
>> "%README%" echo   3. Copy LocalAppData-TakeOff\ to %%LOCALAPPDATA%%\TakeOff
>> "%README%" echo   4. Install Node, Rust and the Visual Studio C++ build tools.
>> "%README%" echo   5. In AppLauncher\ run: npm install
>> "%README%" echo   6. Install GitHub CLI and run: gh auth login
>> "%README%" echo.
>> "%README%" echo NOT IN THIS BACKUP - secrets, on purpose
>> "%README%" echo   SSL.com code signing credentials. Set as environment
>> "%README%" echo     variables before a release:
>> "%README%" echo       SSL_COM_USERNAME, SSL_COM_PASSWORD, SSL_COM_TOTP_SECRET
>> "%README%" echo   CodeSignTool itself. Re-download from SSL.com and extract
>> "%README%" echo     to C:\CodeSignTool
>> "%README%" echo   LemonSqueezy API key. Lives as a Cloudflare Worker secret.
>> "%README%" echo   Cloudflare login. Run: npx wrangler login

echo.
echo ==========================================================
if defined FAILED (
    echo   FINISHED WITH PROBLEMS - read the messages above.
) else (
    echo   Backup complete.
)
echo.
echo   Location: %TARGET%
echo   Read RESTORE-README.txt there for restore steps.
echo ==========================================================

:end
echo.
pause
endlocal
