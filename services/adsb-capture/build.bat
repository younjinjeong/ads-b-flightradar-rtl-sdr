@echo off
set PROTOC=C:\Users\younj\protoc\bin\protoc.exe
cd /d "c:\Users\younj\Workspace\SDR-project\services\adsb-capture"
C:\Users\younj\.cargo\bin\cargo.exe build --release
