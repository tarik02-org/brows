@echo off

echo Starting Installation
REM Delayed Expansion is usually disabled by default, but
REM we are explicit about it here not to make that assumption
setlocal DisableDelayedExpansion

REM Sets ARCH to ARM64 or AMD64
for /f "tokens=3" %%a in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment" /v PROCESSOR_ARCHITECTURE ^| findstr /ri "REG_SZ"') do set ARCH_WIN=%%a

REM echo %ARCH_WIN%

if "%ARCH_WIN%" == "ARM64" (set ARCH=aarch64)
if "%ARCH_WIN%" == "AMD64" (set ARCH=x86_64)

REM echo %ARCH%

fltmc >nul 2>&1 && (
  set is_admin=true
) || (
  set is_admin=false
)

set is_explicitly_requested_system=false
for %%A in (%*) do (
    if "%%A"=="--system" (
        set is_explicitly_requested_system=true
    )
)

if %is_explicitly_requested_system% == true (
  if %is_admin% == false (
    echo You must run this installer with Administrator privileges when using --system flag
    echo Please run as administrator (no --system required then^)
    echo.

    exit /b 1
  )

  set is_local_install=false
) else (
  set is_local_install=true
)

set is_silent=false
for %%A in (%*) do (
    if "%%A"=="--silent" (
        set is_silent=true
    )
)

set is_explicitly_requested_user=false
for %%A in (%*) do (
    if "%%A"=="--user" (
        set is_explicitly_requested_user=true
    )
)

REM Default to system-wide install if running with admin rights and
REM not specifying explicit user/system install mode
if %is_admin% == true (
    if not %is_explicitly_requested_user% == true (
        if not %is_explicitly_requested_system% == true (
          echo Because you are running this as an administrator we are going to install it to the whole system
          echo Please run this installer with --user flag to override this behaviour.
          echo.
          set is_local_install=false
        )
    )
)

REM C:\Users\x\AppData\Local\me.tarik02.brows\

if %is_local_install% == true (
    REM TODO: would be even more correct to take from registry
    set LocalProgramsDir=%LocalAppData%\Programs
    setlocal EnableDelayedExpansion
    set ProgramDir=!LocalProgramsDir!\me.tarik02.brows
    setlocal DisableDelayedExpansion
    echo Installing only for current user
) else (
    set ProgramDir=%ProgramFiles%\me.tarik02.brows
    echo Installing for all users
)

echo Installing to %ProgramDir%

if not exist "%ProgramDir%\" (
  mkdir "%ProgramDir%" || exit /b
)

REM .bat location with trailing \
set THIS_DIR=%~dp0

set SRC_BINARY_PATH=%THIS_DIR%%ARCH%\brows.exe

FOR /F "delims=" %%F IN ('powershell -NoLogo -NoProfile -Command ^(Get-Item \"%SRC_BINARY_PATH%\"^).VersionInfo.FileVersion') DO (SET APP_VERSION=%%F)

echo brows Version: %APP_VERSION%

copy "%SRC_BINARY_PATH%" "%ProgramDir%\brows.exe" 1>nul

copy "%THIS_DIR%uninstall.bat" "%ProgramDir%\uninstall.bat" 1>nul

if not exist "%ProgramDir%\resources\icons\512x512\" (
  mkdir "%ProgramDir%\resources\icons\512x512" || exit /b
)

copy "%THIS_DIR%resources\icons\512x512\me.tarik02.brows.png" "%ProgramDir%\resources\icons\512x512\me.tarik02.brows.png" 1>nul

if not exist "%ProgramDir%\resources\i18n\en-US\" (
  mkdir "%ProgramDir%\resources\i18n\en-US" || exit /b
)

copy "%THIS_DIR%resources\i18n\en-US\builtin.ftl" "%ProgramDir%\resources\i18n\en-US\builtin.ftl" 1>nul

if not exist "%ProgramDir%\resources\repository\" (
  mkdir "%ProgramDir%\resources\repository" || exit /b
)
copy "%THIS_DIR%resources\repository\application-repository.toml" "%ProgramDir%\resources\repository\application-repository.toml" 1>nul

REM C:\Users\x\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\brows\brows.lnk

if %is_local_install% == true (
    set ShortcutFromPath="%THIS_DIR%startmenu\user\brows.lnk"
    set ShortcutToDir=%AppData%\Microsoft\Windows\Start Menu\Programs\brows
) else (
    set ShortcutFromPath="%THIS_DIR%startmenu\system\brows.lnk"
    set ShortcutToDir=%ALLUSERSPROFILE%\Microsoft\Windows\Start Menu\Programs\brows
)

if not exist "%ShortcutToDir%\" (
  mkdir "%ShortcutToDir%" || exit /b
)

set ShortcutToPath=%ShortcutToDir%\brows.lnk
copy "%ShortcutFromPath%" "%ShortcutToPath%" 1>nul

if %is_local_install% == true (
    REM HKEY_CURRENT_USER
    set RegistryRoot=HKCU
) else (
    REM HKEY_LOCAL_MACHINE
    set RegistryRoot=HKLM
)

REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows" /ve /d "brows HTML Document" /f 1>nul
REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows" /v AppUserModelId /t REG_SZ /d "me.tarik02.brows" /f 1>nul

REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows\Application" /v AppUserModelId /t REG_SZ /d "me.tarik02.brows" /f 1>nul
REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows\Application" /v ApplicationIcon /t REG_SZ /d "%ProgramDir%\brows.exe,0" /f 1>nul
REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows\Application" /v ApplicationName /t REG_SZ /d "brows" /f 1>nul
REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows\Application" /v ApplicationDescription /t REG_SZ /d "Open the right browser at the right time" /f 1>nul

REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows\DefaultIcon" /ve /d "%ProgramDir%\brows.exe,0" /f 1>nul

REG ADD "%RegistryRoot%\Software\Classes\me.tarik02.brows\shell\open\command" /ve /d "\"%ProgramDir%\brows.exe\" \"%%1\"" /f 1>nul

REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows" /ve /d "brows" /f 1>nul

REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\Capabilities" /v ApplicationIcon /t REG_SZ /d "%ProgramDir%\brows.exe,0" /f 1>nul

REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\Capabilities" /v ApplicationName /t REG_SZ /d "brows" /f 1>nul
REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\Capabilities" /v ApplicationDescription /t REG_SZ /d "Open the right browser at the right time" /f 1>nul

REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\Capabilities\URLAssociations" /v http /t REG_SZ /d "me.tarik02.brows" /f 1>nul
REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\Capabilities\URLAssociations" /v https /t REG_SZ /d "me.tarik02.brows" /f 1>nul

REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\DefaultIcon" /ve /d "%ProgramDir%\brows.exe,0" /f 1>nul

REG ADD "%RegistryRoot%\Software\Clients\StartMenuInternet\me.tarik02.brows\shell\open\command" /ve /d "\"%ProgramDir%\brows.exe\"" /f 1>nul

REG ADD "%RegistryRoot%\Software\RegisteredApplications" /v me.tarik02.brows /t REG_SZ /d "Software\Clients\StartMenuInternet\me.tarik02.brows\Capabilities" /f 1>nul

REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\App Paths\brows.exe" /ve /d "%ProgramDir%\brows.exe" /f 1>nul
REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\App Paths\brows.exe" /v Path /t REG_SZ /d "%ProgramDir%" /f 1>nul

REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\App Paths\brows.exe\SupportedProtocols" /v http /t REG_SZ /d "" /f 1>nul
REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\App Paths\brows.exe\SupportedProtocols" /v https /t REG_SZ /d "" /f 1>nul

REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /f 1>nul
REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v DisplayIcon /t REG_SZ /d "%ProgramDir%\brows.exe" /f 1>nul

REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v DisplayVersion /t REG_SZ /d "%APP_VERSION%" /f 1>nul
REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v EstimatedSize /t REG_DWORD /d 4800 /f 1>nul

if %is_local_install% == true (
  REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v DisplayName /t REG_SZ /d "brows (Current User)" /f 1>nul
  REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v UninstallString /t REG_SZ /d "\"%ProgramDir%\uninstall.bat\" --user" /f 1>nul
) else (
  REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v DisplayName /t REG_SZ /d "brows (All Users)" /f 1>nul
  REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v UninstallString /t REG_SZ /d "\"%ProgramDir%\uninstall.bat\" --system" /f 1>nul
)

if %is_local_install% == true (
    set UninstallParameter=--user
) else (
    set UninstallParameter=--system
)
REG ADD "%RegistryRoot%\Software\Microsoft\Windows\CurrentVersion\Uninstall\me.tarik02.brows" /v UninstallString /t REG_SZ /d "\"%ProgramDir%\uninstall.bat\" %UninstallParameter%" /f 1>nul

powershell -ExecutionPolicy Bypass -File "%THIS_DIR%announce_default.ps1" || exit /b

echo brows has been installed. Enjoy!
echo Please report any issues at https://github.com/tarik02-org/brows/issues

if not %is_silent% == true (
    echo You can now press Enter to exit this installer.
    set /p input=
)
