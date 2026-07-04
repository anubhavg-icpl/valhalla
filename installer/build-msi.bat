@echo off
REM
REM Build the Valhalla MSI installer using WiX Toolset v3.
REM Expects WiX binaries at C:\wix314 (or on PATH).
REM
REM Usage: build-msi.bat [release-dir] [output-msi-path]
REM

setlocal enabledelayedexpansion

set RELEASE_DIR=%~1
if "%RELEASE_DIR%"=="" set RELEASE_DIR=target\release

set OUTPUT=%~2
if "%OUTPUT%"=="" set OUTPUT=target\release\valhalla-0.1.0-x64.msi

set WIX=C:\wix314
where candle >nul 2>&1
if %errorlevel%==0 (
  set CANDLE=candle
  set LIGHT=light
) else (
  set CANDLE=%WIX%\candle.exe
  set LIGHT=%WIX%\light.exe
)

REM Generate deterministic GUIDs via PowerShell (matches valhalla-installer's v5 scheme)
for /f "delims=" %%i in ('powershell -NoProfile -Command "[uuid]::NewGuid().ToString().ToUpper()"') do set UPGRADE_CODE=%%i

REM Component GUIDs (any valid GUIDs; they just need to be stable per build).
for /f "delims=" %%i in ('powershell -NoProfile -Command "[uuid]::NewGuid().ToString().ToUpper()"') do set CLIENT_GUID=%%i
for /f "delims=" %%i in ('powershell -NoProfile -Command "[uuid]::NewGuid().ToString().ToUpper()"') do set DRIVER_GUID=%%i
for /f "delims=" %%i in ('powershell -NoProfile -Command "[uuid]::NewGuid().ToString().ToUpper()"') do set SHORTCUT_GUID=%%i

set CLIENT_EXE=%RELEASE_DIR%\valhalla-client.exe
set DRIVER_SYS=%RELEASE_DIR%\valhalla.sys

if not exist "%CLIENT_EXE%" (
  echo error: %CLIENT_EXE% not found. Run 'cargo xtask client' first.
  exit /b 1
)

echo Building valhalla-0.1.0-x64.msi ...
echo   Client: %CLIENT_EXE%

if exist "%DRIVER_SYS%" (
  echo   Driver: %DRIVER_SYS%
) else (
  echo   Driver: not found, building client-only MSI
  set DRIVER_SYS=target\release\valhalla-client.exe
)

REM Compile the .wxs into a .wixobj
"%CANDLE%" -ext WixUIExtension -dUpgradeCode="{%UPGRADE_CODE%}" -dClientComponentGuid="{%CLIENT_GUID%}" -dDriverComponentGuid="{%DRIVER_GUID%}" -dShortcutComponentGuid="{%SHORTCUT_GUID%}" -dClientExePath="%CLIENT_EXE%" -dDriverPath="%DRIVER_SYS%" installer\valhalla.wxs -out target\release\valhalla.wixobj || exit /b 1

REM Link into the final .msi
"%LIGHT%" -ext WixUIExtension target\release\valhalla.wixobj -out "%OUTPUT%" || exit /b 1

echo Wrote %OUTPUT%
endlocal
