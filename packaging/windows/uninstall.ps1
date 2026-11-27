# Removes everything install.ps1 added for the current user.
#   powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Glance\uninstall.ps1"

$ErrorActionPreference = 'SilentlyContinue'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Glance'
$classes = 'HKCU:\Software\Classes'
$exts = '.png','.apng','.jpg','.jpeg','.jpe','.jfif','.gif','.webp','.bmp','.dib','.ico','.tif','.tiff',
        '.tga','.pnm','.pbm','.pgm','.ppm','.qoi','.hdr','.exr','.dds','.svg','.svgz'

Get-Process glance | Where-Object { $_.Path -like "$dir*" } | Stop-Process -Force
foreach ($e in $exts) { Remove-ItemProperty "$classes\$e\OpenWithProgids" -Name 'Glance.Image' }
Remove-Item -Recurse -Force "$classes\Glance.Image", "$classes\Applications\glance.exe", 'HKCU:\Software\Glance'
Remove-ItemProperty 'HKCU:\Software\RegisteredApplications' -Name 'Glance'
Remove-Item -Force (Join-Path ([Environment]::GetFolderPath('Programs')) 'Glance.lnk')
Remove-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA 'Glance')   # saved wallpaper copies
Remove-Item -Recurse -Force $dir
Write-Host 'Glance has been removed.'
