@echo off
rem Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
rem SPDX-License-Identifier: MIT OR Apache-2.0
rem
rem Compile l'hôte C++ avec MSVC, lié à la bibliothèque d'importation de la DLL.
rem Appelé par le Makefile, depuis Git Bash.
rem
rem La recherche de MSVC reprend celle de hosts/c/build.cmd : deuxième
rem occurrence, notée, pas encore extraite.
rem
rem Arguments : exécutable produit, répertoire des objets, bibliothèque
rem d'importation, répertoire du header, source.

setlocal

where cl >nul 2>nul
if errorlevel 1 goto vcvars
if defined VCToolsInstallDir set "PATH=%VCToolsInstallDir%bin\Hostx64\x64;%PATH%"
goto compile

:vcvars
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VS=%%i"
if not defined VS (
  echo cl introuvable, ni dans le PATH ni par vswhere 1>&2
  exit /b 1
)
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer;%PATH%"
rem Sortie gardée et rendue en cas d'échec, comme dans hosts/c/build.cmd.
call "%VS%\VC\Auxiliary\Build\vcvars64.bat" >"%~2\vcvars.log" 2>&1
if errorlevel 1 (
  echo vcvars64 en echec, sa sortie suit : 1>&2
  type "%~2\vcvars.log" 1>&2
  exit /b 1
)

:compile
rem -EHsc : exceptions C++ standard. La DLL se lie par sa bibliothèque
rem d'importation, sans aucune bibliothèque système : c'est la DLL qui les porte.
cl -nologo -MD -std:c++17 -EHsc -W4 -WX -O2 -I"%~4" -Fo"%~2\\" -Fe"%~1" "%~5" "%~3"
