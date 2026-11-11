# Setup d'installation propre de Wavecraft pour Windows
[CmdletBinding()]
param(
    [switch]$Silencieux,
    [switch]$Lancer
)

$ErrorActionPreference = 'Stop'
$racine = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path "$racine\app")) {
    $racine = (Get-Location).Path
}

$dest = Join-Path $env:LOCALAPPDATA "Programs\Wavecraft"
Write-Host "Installation de Wavecraft dans $dest..." -ForegroundColor Cyan

# 1. Création des dossiers
New-Item -ItemType Directory -Path "$dest\pilote" -Force | Out-Null

# 2. Copie de l'exécutable et des dépendances
$exeSource = "$racine\app\target\release\wavecraft.exe"
if (-not (Test-Path $exeSource)) {
    $exeSource = "$racine\wavecraft.exe"
}
if (-not (Test-Path $exeSource)) {
    throw "Exécutable wavecraft.exe introuvable dans $exeSource"
}

Copy-Item $exeSource "$dest\wavecraft.exe" -Force
Write-Host "  [OK] wavecraft.exe" -ForegroundColor Green

# DLL RTL-SDR Blog V4
$dllSource = "$racine\vendor\rtl-sdr-blog\x64\rtlsdr.dll"
if (-not (Test-Path $dllSource)) {
    $dllSource = "$racine\rtlsdr.dll"
}
if (Test-Path $dllSource) {
    Copy-Item $dllSource "$dest\rtlsdr.dll" -Force
    Write-Host "  [OK] rtlsdr.dll (RTL-SDR Blog V4)" -ForegroundColor Green
}

# Icône
$icoSource = "$racine\site\assets\img\wavecraft.ico"
if (-not (Test-Path $icoSource)) {
    $icoSource = "$racine\app\assets\wavecraft.ico"
}
if (Test-Path $icoSource) {
    Copy-Item $icoSource "$dest\wavecraft.ico" -Force
    Write-Host "  [OK] wavecraft.ico" -ForegroundColor Green
}

# Zadig (outil de pilote)
$zadigSource = "$racine\vendor\zadig\zadig-2.9.exe"
if (-not (Test-Path $zadigSource)) {
    $zadigSource = "$racine\pilote\zadig.exe"
}
if (Test-Path $zadigSource) {
    Copy-Item $zadigSource "$dest\pilote\zadig.exe" -Force
    Write-Host "  [OK] pilote\zadig.exe" -ForegroundColor Green
}
$zadigIni = "$racine\vendor\zadig\zadig.ini"
if (Test-Path $zadigIni) {
    Copy-Item $zadigIni "$dest\pilote\zadig.ini" -Force
}

# Documentation et licences
foreach ($doc in @('README.md', 'LICENSE', 'THIRD_PARTY.md')) {
    $docPath = "$racine\$doc"
    if (Test-Path $docPath) {
        Copy-Item $docPath "$dest\" -Force
    }
}

# 3. Création du raccourci sur le Bureau
$wsh = New-Object -ComObject WScript.Shell
$desktop = [Environment]::GetFolderPath('Desktop')
$shortcutDesktop = $wsh.CreateShortcut("$desktop\Wavecraft.lnk")
$shortcutDesktop.TargetPath = "$dest\wavecraft.exe"
$shortcutDesktop.WorkingDirectory = "$dest"
$shortcutDesktop.IconLocation = "$dest\wavecraft.ico,0"
$shortcutDesktop.Description = "Wavecraft - Récepteur d'ondes courtes pour stations de nombres"
$shortcutDesktop.Save()
Write-Host "  [OK] Icône créée sur le Bureau : $desktop\Wavecraft.lnk" -ForegroundColor Green

# 4. Création du raccourci dans le Menu Démarrer
$programs = [Environment]::GetFolderPath('Programs')
$shortcutStart = $wsh.CreateShortcut("$programs\Wavecraft.lnk")
$shortcutStart.TargetPath = "$dest\wavecraft.exe"
$shortcutStart.WorkingDirectory = "$dest"
$shortcutStart.IconLocation = "$dest\wavecraft.ico,0"
$shortcutStart.Description = "Wavecraft - Récepteur d'ondes courtes pour stations de nombres"
$shortcutStart.Save()
Write-Host "  [OK] Raccourci créé dans le Menu Démarrer" -ForegroundColor Green

# 5. Inscription dans le Registre Windows (Ajout/Suppression de programmes)
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wavecraft"
if (-not (Test-Path $uninstallKey)) {
    New-Item -Path $uninstallKey -Force | Out-Null
}
Set-ItemProperty -Path $uninstallKey -Name "DisplayName" -Value "Wavecraft"
Set-ItemProperty -Path $uninstallKey -Name "DisplayVersion" -Value "0.1.0"
Set-ItemProperty -Path $uninstallKey -Name "Publisher" -Value "Nicolas"
Set-ItemProperty -Path $uninstallKey -Name "DisplayIcon" -Value "$dest\wavecraft.ico"
Set-ItemProperty -Path $uninstallKey -Name "InstallLocation" -Value "$dest"
Set-ItemProperty -Path $uninstallKey -Name "UninstallString" -Value "`"$dest\desinstaller.cmd`""
Set-ItemProperty -Path $uninstallKey -Name "URLInfoAbout" -Value "https://nthill.be/wavecraft/"

# Script de désinstallation propre
$uninstContent = @"
@echo off
chcp 65001 >nul
echo Desinstallation de Wavecraft...
set "DEST=%LOCALAPPDATA%\Programs\Wavecraft"
set "DESKTOP=%USERPROFILE%\Desktop"
set "PROGRAMS=%APPDATA%\Microsoft\Windows\Start Menu\Programs"

if exist "%DESKTOP%\Wavecraft.lnk" del /f /q "%DESKTOP%\Wavecraft.lnk"
if exist "%PROGRAMS%\Wavecraft.lnk" del /f /q "%PROGRAMS%\Wavecraft.lnk"
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wavecraft" /f >nul 2>&1

echo Raccourcis supprimes.
echo Pour finaliser, supprimez le dossier : %DEST%
pause
"@
[System.IO.File]::WriteAllText("$dest\desinstaller.cmd", $uninstContent, [System.Text.Encoding]::UTF8)

Write-Host "`nInstallation terminee avec succes !" -ForegroundColor Green
Write-Host "L'icone Wavecraft est disponible sur votre Bureau." -ForegroundColor Cyan

if ($Lancer) {
    Start-Process "$dest\wavecraft.exe"
}
