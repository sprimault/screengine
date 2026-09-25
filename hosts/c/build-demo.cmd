@echo off
rem Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
rem SPDX-License-Identifier: MIT OR Apache-2.0
rem
rem Compile l'hôte C de démonstration avec MSVC, lié à SDL3.
rem
rem Séparé de build.cmd parce que les deux programmes n'ont pas les mêmes
rem besoins : l'hôte de conformance ne se lie qu'à la bibliothèque statique et
rem aux bibliothèques système, celui-ci ajoute SDL3. Un script à arguments
rem optionnels porterait les deux cas à la fois, et c'est le genre de fichier
rem qu'on ne relit plus.
rem
rem Arguments : exécutable produit, répertoire des objets, bibliothèque
rem statique, répertoire du header, source, racine de SDL3.

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
call "%VS%\VC\Auxiliary\Build\vcvars64.bat" >"%~2\vcvars.log" 2>&1
if errorlevel 1 (
  echo vcvars64 en echec, sa sortie suit : 1>&2
  type "%~2\vcvars.log" 1>&2
  exit /b 1
)

:compile
rem -MD comme l'hôte de conformance : Rust se lie au CRT dynamique, et SDL3
rem aussi. -W4 sans -WX : les en-tetes de SDL3 ne sont pas ecrits pour le
rem niveau d'avertissement du projet, et une dependance tierce n'a pas a l'etre.
cl -nologo -MD -std:c17 -W4 -O2 -I"%~4" -I"%~6\include" -Fo"%~2\\" -Fe"%~1" "%~5" "%~3" ^
  "%~6\lib\x64\SDL3.lib" kernel32.lib ntdll.lib userenv.lib ws2_32.lib dbghelp.lib shell32.lib
