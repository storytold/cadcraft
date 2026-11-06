@echo off
setlocal
cd /d "%~dp0"
title CADCraft - Compilar interface personalizada

where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust e Cargo nao foram encontrados.
  echo Instale Rust em https://rustup.rs/ e o Visual Studio Build Tools com C++.
  echo Depois abra este arquivo novamente.
  pause
  exit /b 1
)

echo Compilando CADCraft com a faixa de opcoes personalizada...
echo A primeira compilacao pode demorar bastante.
cargo build --release -p cadcraft
if errorlevel 1 (
  echo.
  echo Nao foi possivel compilar. Verifique as mensagens acima.
  echo No Windows, o Rust precisa do compilador C++ e do Windows SDK.
  pause
  exit /b 1
)

echo.
echo Pronto. O executavel esta em:
echo %~dp0target\release\cadcraft.exe
echo.
echo Este arquivo nao substitui automaticamente o CADCraft instalado.
pause
