$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
py -3 -m venv .venv
if ($LASTEXITCODE -ne 0) { throw '请先安装 Python 3' }
& .\.venv\Scripts\python.exe -m pip install -r bridge/requirements.txt
if ($LASTEXITCODE -ne 0) { throw '音频依赖安装失败' }
Write-Host '请从 https://vb-audio.com/Cable/ 安装官方 VB-CABLE，微信输入法麦克风选 CABLE Output，语音启动快捷键设为 F8。'
& .\.venv\Scripts\python.exe bridge\audio-output.py --check --device 'CABLE Input'
