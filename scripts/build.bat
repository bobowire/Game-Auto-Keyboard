@echo off
setlocal

REM ============================================
REM  Game Auto Keyboard - Build Script
REM ============================================

REM Change to project root (parent of scripts folder)
cd /d "%~dp0.."

echo ========================================
echo   Game Auto Keyboard - Build Script
echo   Work Dir: %CD%
echo ========================================
echo.

REM Check if cargo is available
where cargo >nul 2>nul
if errorlevel 1 (
    echo [ERROR] cargo not found. Install Rust: https://rustup.rs/
    pause
    exit /b 1
)

REM Dispatch by argument, show menu if none
if "%~1"=="" goto MENU
if /i "%~1"=="build"   goto BUILD
if /i "%~1"=="release" goto RELEASE
if /i "%~1"=="run"     goto RUN
if /i "%~1"=="test"    goto TEST
if /i "%~1"=="example" goto EXAMPLE
if /i "%~1"=="clean"   goto CLEAN
goto MENU

:MENU
echo Select an action:
echo   [1] Debug build    (cargo build)
echo   [2] Release build  (cargo build --release)
echo   [3] Run main       (cargo run)
echo   [4] Run tests      (cargo test)
echo   [5] Run example    (simple_test - PostMessage test)
echo   [6] Run example    (script_test - Script engine test)
echo   [7] Clean          (cargo clean)
echo   [0] Exit
echo.
set /p choice=Enter option number:
if "%choice%"=="1" goto BUILD
if "%choice%"=="2" goto RELEASE
if "%choice%"=="3" goto RUN
if "%choice%"=="4" goto TEST
if "%choice%"=="5" goto EXAMPLE_SIMPLE
if "%choice%"=="6" goto EXAMPLE_SCRIPT
if "%choice%"=="7" goto CLEAN
if "%choice%"=="0" exit /b 0
echo Invalid option
goto MENU

:BUILD
echo.
echo [Debug Build] cargo build
echo ----------------------------------------
cargo build
goto DONE

:RELEASE
echo.
echo [Release Build] cargo build --release
echo ----------------------------------------
cargo build --release
goto DONE

:RUN
echo.
echo [Run Main] cargo run
echo ----------------------------------------
cargo run
goto DONE

:TEST
echo.
echo [Run Tests] cargo test
echo ----------------------------------------
cargo test
goto DONE

:EXAMPLE_SIMPLE
echo.
echo [Run Example] cargo run --example simple_test
echo ----------------------------------------
echo TIP: After start, click target window within 5 seconds (e.g. Notepad)
echo.
cargo run --example simple_test
goto DONE

:EXAMPLE_SCRIPT
echo.
echo [Run Example] cargo run --example script_test
echo ----------------------------------------
echo TIP: After start, click target window within 5 seconds (e.g. Notepad)
echo.
cargo run --example script_test
goto DONE

:CLEAN
echo.
echo [Clean] cargo clean
echo ----------------------------------------
cargo clean
goto DONE

:DONE
echo.
echo ----------------------------------------
if errorlevel 1 (
    echo [FAILED] Command error, exit code: %errorlevel%
) else (
    echo [DONE] Command succeeded
)
echo ----------------------------------------
echo.
pause
endlocal
