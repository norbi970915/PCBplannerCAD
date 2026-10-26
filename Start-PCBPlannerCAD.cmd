@echo off
cd /d "%~dp0"
call pnpm.cmd dev
if errorlevel 1 pause
