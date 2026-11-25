@echo off
rem Auto play debug run: the brain logs every decision at RUST_LOG=debug,
rem and the lines it writes carry the "auto play: " prefix, so grep for it:
rem   auto play: build ...    what the yard started
rem   auto play: micro ...    what a soldier was told to do (id, goal, deployed, reach)
rem   auto play: deploy ...   why a soldier stood up or knelt (nearest = squared cells)
rem The log is appended to logs\<name>.log next to this file.
setlocal
set RUST_LOG=debug
"%~dp0target\fast\vera20k.exe" %*
endlocal
