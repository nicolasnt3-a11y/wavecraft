@echo off
title Installation de Wavecraft
chcp 65001 >nul
cd /d "%~dp0"

echo ========================================================
echo             INSTALLATION DE WAVECRAFT
echo ========================================================
echo.
echo Installation en cours et creation du raccourci Bureau...
echo.

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0packaging\windows\installer.ps1"

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================================
    echo  SUCCES : Wavecraft est installe sur votre Bureau !
    echo ========================================================
    echo.
    set /p rep="Voulez-vous lancer Wavecraft maintenant ? (O/n) : "
    if /i "%rep%"=="n" goto fin
    start "" "%LOCALAPPDATA%\Programs\Wavecraft\wavecraft.exe"
) else (
    echo.
    echo Une erreur est survenue lors de l'installation.
    pause
)

:fin
