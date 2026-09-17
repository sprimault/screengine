@echo off
rem Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
rem SPDX-License-Identifier: MIT OR Apache-2.0
rem
rem Compile l'hôte C avec MSVC. Appelé par le Makefile, depuis Git Bash.
rem
rem Un fichier de commandes plutôt qu'une recette : vcvars64 ne s'applique qu'à
rem l'invite qui l'appelle, et l'écrire dans une recette Make imposerait deux
rem couches de guillemets. Sans cl dans le PATH, MSVC se trouve par vswhere.
rem
rem Le répertoire des outils MSVC passe devant le PATH dans les deux cas : Git
rem Bash y place le sien, dont le link.exe n'est pas l'éditeur de liens.
rem
rem Arguments : exécutable produit, répertoire des objets, bibliothèque
rem statique, répertoire du header, source.

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
rem vcvars64 appelle lui-même vswhere, qu'il cherche dans le PATH.
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer;%PATH%"
rem vcvars64 écrit ses erreurs sur la sortie standard : jetée, un échec sortirait
rem en code 1 sans un mot. Elle est gardée, et rendue seulement en cas d'échec.
call "%VS%\VC\Auxiliary\Build\vcvars64.bat" >"%~2\vcvars.log" 2>&1
if errorlevel 1 (
  echo vcvars64 en echec, sa sortie suit : 1>&2
  type "%~2\vcvars.log" 1>&2
  exit /b 1
)

:compile
rem -MD : Rust se lie au CRT dynamique, et deux CRT dans un même binaire
rem donnent deux tas. -std:c17 : sans lui, cl ne définit pas __STDC_VERSION__
rem et les assertions de disposition du header disparaissent.
rem Les bibliothèques système sont celles que rend `make native-libs`.
cl -nologo -MD -std:c17 -W4 -WX -O2 -I"%~4" -Fo"%~2\\" -Fe"%~1" "%~5" "%~3" ^
  kernel32.lib ntdll.lib userenv.lib ws2_32.lib dbghelp.lib
