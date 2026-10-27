param([switch]$RunOnly,[string]$SnapshotFile='',[string]$ArduinoJsonInclude='')
$ErrorActionPreference='Stop'
$panelRepo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$panelNativeBuild=Join-Path $panelRepo 'build/lvgl-row'
New-Item -ItemType Directory -Path (Join-Path $panelNativeBuild 'obj') -Force|Out-Null
if(-not $RunOnly) {
    if(-not $ArduinoJsonInclude){
        $panelLibraries=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Arduino/libraries'
        $panelJsonLibrary=Get-ChildItem -LiteralPath $panelLibraries -Directory -Filter 'ArduinoJson*' -ErrorAction SilentlyContinue|
            Where-Object{Test-Path -LiteralPath (Join-Path $_.FullName 'src/ArduinoJson.h')}|
            Sort-Object Name -Descending|Select-Object -First 1
        if(-not $panelJsonLibrary){throw 'Install ArduinoJson 7 or pass -ArduinoJsonInclude with its src directory.'}
        $ArduinoJsonInclude=Join-Path $panelJsonLibrary.FullName 'src'
    }
    if(-not(Test-Path -LiteralPath (Join-Path $ArduinoJsonInclude 'ArduinoJson.h'))){throw 'ArduinoJsonInclude must contain ArduinoJson.h.'}
    $panelConfig=Get-Content -LiteralPath (Join-Path $panelRepo 'firmware/libraries/lvgl8/lv_conf.h') -Raw
    $panelConfig=$panelConfig.Replace('#define LV_COLOR_DEPTH 16','#define LV_COLOR_DEPTH 32').Replace('#define LV_COLOR_16_SWAP 1','#define LV_COLOR_16_SWAP 0').Replace('#define LV_COLOR_SCREEN_TRANSP 1','#define LV_COLOR_SCREEN_TRANSP 0')
    [IO.File]::WriteAllText((Join-Path $panelNativeBuild 'lv_conf.h'),$panelConfig,[Text.UTF8Encoding]::new($false))
    $panelLvgl=Join-Path $panelRepo 'firmware/libraries/lvgl8/lvgl'
    $panelIncludes=@(('/I"'+$panelNativeBuild+'"'),('/I"'+$panelLvgl+'"'),('/I"'+(Join-Path $panelRepo 'firmware/ai-status-screen')+'"'))
    $panelArguments=@('/nologo','/W0','/utf-8','/std:c11','/TC','/O1','/D_CRT_SECURE_NO_WARNINGS','/DLV_CONF_INCLUDE_SIMPLE')+$panelIncludes+@('/c',('/Fo"'+(Join-Path $panelNativeBuild 'obj')+'\\"'))
    $panelSourceFiles=@(Get-ChildItem -LiteralPath (Join-Path $panelLvgl 'src') -Recurse -Filter '*.c' -File|ForEach-Object{$_.FullName})
    $panelSourceFiles+=(Join-Path $panelRepo 'firmware/ai-status-screen/panel_font_16.c')
    $panelSourceFiles+=(Join-Path $panelRepo 'firmware/ai-status-screen/panel_font_24.c')
    $panelSourceFiles+=(Join-Path $panelRepo 'firmware/ai-status-screen/panel_font_30.c')
    $panelSourceFiles+=(Join-Path $PSScriptRoot 'main.c')
    $panelSources=@($panelSourceFiles|Where-Object{$panelObject=Join-Path $panelNativeBuild ('obj/'+[IO.Path]::GetFileNameWithoutExtension($_)+'.obj');-not(Test-Path -LiteralPath $panelObject) -or (Get-Item -LiteralPath $_).LastWriteTime -gt (Get-Item -LiteralPath $panelObject).LastWriteTime}|ForEach-Object{'"'+$_+'"'})
    [IO.File]::WriteAllLines((Join-Path $panelNativeBuild 'compile.rsp'),($panelArguments+$panelSources),[Text.UTF8Encoding]::new($false))
    $panelVswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if(-not(Test-Path -LiteralPath $panelVswhere)){throw 'MSVC Build Tools and vswhere are required.'}
    $panelVs=& $panelVswhere -latest -products '*' -property installationPath
    $panelVc=Join-Path $panelVs 'VC/Auxiliary/Build/vcvars64.bat'
    $panelCpp='/nologo /W0 /utf-8 /std:c++17 /TP /EHsc /O1 /D_CRT_SECURE_NO_WARNINGS /DLV_CONF_INCLUDE_SIMPLE '+($panelIncludes -join ' ')+' /I"'+$ArduinoJsonInclude+'" /c "'+(Join-Path $PSScriptRoot 'panel_host.cpp')+'" /Foobj\panel_host.obj'
    $panelCompileC=if($panelSources.Count){'cl.exe @compile.rsp > compile.log 2>&1'}else{'echo C objects cached > compile.log'}
    $panelCmd='@echo off'+"`r`n"+'call "'+$panelVc+'" >nul'+"`r`n"+$panelCompileC+"`r`n"+'if errorlevel 1 exit /b 1'+"`r`n"+'cl.exe '+$panelCpp+' >> compile.log 2>&1'+"`r`n"+'if errorlevel 1 exit /b 1'+"`r`n"+'link.exe /NOLOGO /OUT:lvgl-row-test.exe obj\*.obj > link.log 2>&1'+"`r`n"+'exit /b %errorlevel%'+"`r`n"
    [IO.File]::WriteAllText((Join-Path $panelNativeBuild 'build.cmd'),$panelCmd,[Text.ASCIIEncoding]::new())
    Push-Location $panelNativeBuild
    try { & cmd.exe /c build.cmd;if($LASTEXITCODE -ne 0){Get-Content compile.log -Tail 15;Get-Content link.log -Tail 15 -ErrorAction SilentlyContinue;throw 'Native LVGL build failed'} }finally{Pop-Location}
}
Push-Location $panelNativeBuild
try { if($SnapshotFile){ & '.\lvgl-row-test.exe' $SnapshotFile }else{ & '.\lvgl-row-test.exe' };if($LASTEXITCODE -ne 0){throw 'Native LVGL row test failed'} }finally{Pop-Location}
