@echo off
rem Copyright 2026 Stéphane Primault <sprimault@users.noreply.github.com>
rem SPDX-License-Identifier: MIT OR Apache-2.0
rem
rem Compile l'hôte C++ de démonstration avec MSVC, lié à la bibliothèque
rem d'importation de la DLL et à SDL3.
rem
rem Séparé de build.cmd pour la même raison que du côté C : l'hôte de
rem conformance ne se lie qu'à la DLL, celui-ci ajoute SDL3, et un script à
rem arguments optionnels porterait les deux cas à la fois.
rem
rem La recherche de MSVC en est à sa quatrième copie. Elle s'extrait au
rem prochain script qui en aurait besoin, pas avant : le bloc n'a pas bougé
rem depuis qu'il est écrit, et un script commun appelé depuis quatre endroits
rem se déboguerait moins bien que celui qu'on lit sous les yeux.
rem
rem Arguments : exécutable produit, répertoire des objets, bibliothèque
rem d'importation, répertoire du header, source, racine de SDL3.

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
rem -W4 sans -WX, contrairement a build.cmd : les en-tetes de SDL3 ne sont pas
rem ecrits pour le niveau d'avertissement du projet, et une dependance tierce
rem n'a pas a l'etre.
cl -nologo -MD -std:c++17 -EHsc -W4 -O2 -I"%~4" -I"%~6\include" -Fo"%~2\\" -Fe"%~1" "%~5" "%~3" ^
  "%~6\lib\x64\SDL3.lib"
