@echo off
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -STA -File "%~dp0install-cpkg.ps1"
exit /b %errorlevel%
