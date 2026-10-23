@echo off
title Pixel Horde
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\launcher.ps1"
pause
