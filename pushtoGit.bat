@echo off

:: Get user input for commit message
set /p commitMsg="Enter commit message: "

:: Add all files
git add .

:: Commit with user message
git commit -m "%commitMsg%"

:: Push to GitHub
git push origin main