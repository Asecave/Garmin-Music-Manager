@ECHO OFF
CLS

:: Set this variable to the location of the folder containing the Garmin-Music-Manager binary

SET "manager=/mnt/c/Users/asecave/Documents/Garmin-Music-Manager"


:: ============================================================================
:: Check for Administrative Privileges and self-elevate if not present
:: ============================================================================
NET FILE >NUL 2>NUL
IF '%ERRORLEVEL%' NEQ '0' (
    ECHO Requesting administrative privileges...
    ECHO(
    powershell.exe -Command "Start-Process '%~f0' -Verb RunAs"
    EXIT /B
)
ECHO(

:: ============================================================================
:: Administrative Code
:: ============================================================================

FOR /F "tokens=*" %%g IN ('usbipd.exe list ^| findstr 091e') do (SET garmin_device_line=%%g)

SET "busid=%garmin_device_line:~0,5%"

usbipd.exe bind --busid %busid%

start wsl.exe bash -c "sleep 12 && cd '%manager%' && ./Garmin-Music-Manager"

timeout 10

usbipd attach --wsl --busid %busid%

GOTO :EOF