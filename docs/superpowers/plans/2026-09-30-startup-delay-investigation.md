# Why TakeOff takes ~1 minute to appear at login

**Status:** C (Task Scheduler) wins and was confirmed on a repeat boot: -0.7s and
-0.5s vs 56.3s baseline. **Implemented** in `register_autostart` (task
`TakeOffAutostart`, registered via `schtasks /XML` with a user-scoped logon
trigger, Run key as fallback). Not yet shipped. D is unnecessary.
**Goal:** the widget is usable within a few seconds of the desktop appearing.

## What we already know — do not re-derive this

Measured on Doug's machine, 2026-09-30:

| Event | Time |
|---|---|
| Boot | 18:00:39 |
| `explorer.exe` (desktop appears) | 18:01:04 |
| `applauncher.exe` (our process **starts**) | 18:02:01 |

**Gap from desktop to our process existing: 56.3 seconds.**

The conclusion that matters: **the delay is entirely before our code runs.** The
process does not exist for 56s. Nothing inside TakeOff — WebView2 init, config
load, window creation — can account for any of it, and optimising startup code
would be wasted effort. `config.json` is only 0.1 MB, so that is not a factor
either.

Windows' standard hold for `HKCU\...\Run` entries is around 10s, so 56s means
something is making it worse: a queue of other startup apps being serialised,
or Defender scanning the binary on first execution after boot.

Note also: the binary is `applauncher.exe`, not `TakeOff.exe` (install folder is
branded, the Cargo package name is not). Cosmetic, but customers see
"applauncher" in Task Manager and Defender prompts. Separate job — renaming
interacts with the auto-updater and the Run key, so it wants its own release.

## The measurement

Run after every test, from a normal (non-elevated) PowerShell:

```powershell
$exp = (Get-Process explorer | Sort-Object StartTime | Select-Object -First 1).StartTime
$app = (Get-Process applauncher | Where-Object { $_.Path -like 'C:\Program Files\*' }).StartTime
"Explorer: $exp"
"TakeOff:  $app"
"Gap:      $([math]::Round(($app - $exp).TotalSeconds,1))s"
```

Each test needs its own reboot. Record the gap.

## Test 0 — context

How many startup apps are competing? Tells us whether Doug's machine is a worst
case or typical.

```powershell
Get-CimInstance Win32_StartupCommand | Select-Object Name, Location | Format-Table -AutoSize
(Get-CimInstance Win32_StartupCommand).Count
```

## Test A — baseline (DONE)

HKCU Run key, as shipped. **56.3s.**

## Test B — remove Windows' startup hold

```powershell
New-Item -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize" -Force | Out-Null
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize" `
  -Name StartupDelayInMSec -Type DWord -Value 0
```

Reboot, measure.

**Result (2026-09-30, boot 18:58:36): 76.3s — WORSE than baseline.** Explorer
18:59:11, applauncher 19:00:27. 16 startup commands registered. Removing the
hold did not help, which points away from Windows' Run-key serialisation delay
and toward something else (Defender, or contention with the other 15 startup
apps). One run only, so treat the +20s as noise until repeated, but it clearly
did not win.

**What else started on that boot** (offsets from explorer):

| +s | Process | Launch mechanism |
|---|---|---|
| 18.3 | Claude.exe | MSIX `StartupTask` (packaged app) |
| 18.7 | SpotifyLauncher | MSIX `StartupTask` (packaged app) |
| 40.6 | SecurityHealthSystray | HKLM Run |
| 76.3 | applauncher | HKCU Run |
| 132.6 | bdagent | HKLM Run (Bitdefender) |

This rules out the "queue of other startup apps" theory. TakeOff is the ONLY
enabled HKCU Run entry. `StartupApproved\Run` marks Steam, Discord, Slack,
NordVPN, OneDrive, LGHUB, Epic and the rest as disabled (`03`). Nothing is
ahead of us in that queue, yet Windows still ran HKLM Run at 40s and ours at
76s. Packaged apps using the separate StartupTask mechanism were up at 18s.

Two more things came out of this:
- **The AV is Bitdefender, not Defender.** Test D as written tests the wrong
  product. A Bitdefender exclusion is the equivalent diagnostic.
- **A stale `AppLauncher` Run entry is still there** (disabled, pointing at the
  pre-rebrand `C:\Program Files\App Launcher\`). It's harmless but should be
  cleaned up. `register_autostart` could delete it.

**If this wins, it is still not automatically the answer.** Shipping it means
writing that value on a customer's machine, which speeds up EVERY startup app
they have, not just ours. That is a change to their computer they did not ask
for. Decide deliberately; do not just ship it because it is fastest.

To undo: `Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize" -Name StartupDelayInMSec`

## Test C — Task Scheduler instead of the Run key

**Read this before assuming the answer.** TakeOff moved FROM Task Scheduler TO
the Run key in 0.5.9, on the belief that ONLOGON tasks wait for the Task
Scheduler service and so take 30–60s. That was never measured. The Run key is
now measured at 56s, so the original reason may not hold. Test it properly.

```powershell
# Park the Run entry so the two don't race
$run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Set-ItemProperty $run -Name TakeOffBackup -Value (Get-ItemProperty $run).TakeOff
Remove-ItemProperty $run -Name TakeOff

schtasks /Create /TN "TakeOffStartupTest" `
  /TR "\"C:\Program Files\TakeOff\applauncher.exe\" --autostart" /SC ONLOGON /F
```

**`schtasks /SC ONLOGON` fails with "Access is denied" when not elevated.**
A customer install can't count on admin rights either. What actually worked,
without elevation, was a logon trigger scoped to the current user:

```powershell
$user = "$env:USERDOMAIN\$env:USERNAME"
Register-ScheduledTask -TaskName "TakeOffStartupTest" -Force `
  -Action   (New-ScheduledTaskAction -Execute "C:\Program Files\TakeOff\applauncher.exe" -Argument "--autostart") `
  -Trigger  (New-ScheduledTaskTrigger -AtLogOn -User $user) `
  -Principal (New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited) `
  -Settings (New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero))
```

If this wins, `register_autostart` needs the same user-scoped trigger. That
probably means the Task Scheduler COM API or an XML definition, not plain
`schtasks` arguments.

**Test C was set up 2026-09-30, after the Test B boot.**

**Result (2026-09-30, boot 19:07:25): -0.7s. TakeOff started together with
Explorer.** Explorer 19:07:48, applauncher 19:07:48, command line included
`--autostart`, and the task's LastRunTime matches. This wins by roughly a
minute, and the 0.5.9 belief that ONLOGON tasks wait 30–60s was simply wrong
for a user-scoped `-AtLogOn` trigger. That is one run. Repeat it once before
committing. It also tells us only when the process started, not when the widget
became usable. WebView2 init now overlaps shell startup, so that gap may grow a
little.

**Repeat (2026-09-30, boot 19:13:51): -0.5s.** Explorer 19:14:14, applauncher
19:14:13, command line again included `--autostart`. Two runs, both around
zero. Test C is confirmed, so implementation can start.

Same boot as the first run, for comparison: SpotifyLauncher +18.8s, SecurityHealthSystray +40.3s,
bdagent +68.0s, all roughly where they were in Test B. So the only thing that
changed was us.

**The "self-heals" claim below is FALSE on this machine.** `launch_on_startup`
is `true` and 0.6.5 contains the re-register-on-launch code (since 0.5.9), yet
after this boot the `TakeOff` Run value was still absent. Either
`register_autostart` is not running, or its write is not landing (Bitdefender
blocking HKCU Run writes is a candidate). We cannot tell which, because the
debug log is a no-op in release. That matters beyond this test: a customer
whose Run entry gets removed does not get it back. Restore manually.

Reboot, measure, then restore:

```powershell
$run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Set-ItemProperty $run -Name TakeOff -Value (Get-ItemProperty $run).TakeOffBackup
Remove-ItemProperty $run -Name TakeOffBackup
schtasks /Delete /TN "TakeOffStartupTest" /F
```

TakeOff re-registers the Run key on every launch anyway, so it self-heals.

**`--autostart` matters.** It is what tells TakeOff that Windows started it
rather than the user, and it gates the startup-groups feature. A test
registration without it silently disables that.

## Test D — is Defender the cause?

Only if B and C both disappoint. Add an exclusion for the install folder, reboot,
measure. This is a diagnostic only — never ship an installer that writes
Defender exclusions.

```powershell
Add-MpPreference -ExclusionPath "C:\Program Files\TakeOff"    # admin required
# undo: Remove-MpPreference -ExclusionPath "C:\Program Files\TakeOff"
```

## Deciding

Pick on measured gap, weighed against how invasive the change is on a
customer's machine. Rough order of preference if the numbers are close:

1. Task Scheduler (Test C) — affects only our app
2. Run key as-is — no change, if the others don't actually help
3. `StartupDelayInMSec` (Test B) — works, but changes machine-wide behaviour

Whatever wins, `register_autostart` in `src-tauri/src/lib.rs` is the single place
that needs changing. It already removes a stale 0.5.8-era Task Scheduler task, so
if we move back to Task Scheduler, that cleanup must be reworked or it will
delete the task it just created.

## Related gap worth closing regardless

`debug_log::write_debug_log` is a **no-op in release builds**, so a customer
reporting "it takes a minute to appear" gives us nothing to work with. Consider a
minimal always-on startup timing line (process start → widget shown) so this is
diagnosable in the field instead of only on Doug's machine.
