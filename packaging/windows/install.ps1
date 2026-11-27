# Installs Glance for the current user (no admin rights needed):
#  - copies glance.exe to %LOCALAPPDATA%\Programs\Glance
#  - adds a Start menu shortcut
#  - adds Glance to "Open with" for image files and to Settings > Default apps
# Run from the folder that contains glance.exe:
#   powershell -ExecutionPolicy Bypass -File .\install.ps1

$ErrorActionPreference = 'Stop'
$src = Join-Path $PSScriptRoot 'glance.exe'
if (-not (Test-Path $src)) { throw "glance.exe not found next to this script ($PSScriptRoot)" }

$dir = Join-Path $env:LOCALAPPDATA 'Programs\Glance'
New-Item -ItemType Directory -Force $dir | Out-Null
Get-Process glance -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" } | Stop-Process -Force
Copy-Item $src (Join-Path $dir 'glance.exe') -Force
$uninstall = Join-Path $PSScriptRoot 'uninstall.ps1'
if (Test-Path $uninstall) { Copy-Item $uninstall (Join-Path $dir 'uninstall.ps1') -Force }
$exe = Join-Path $dir 'glance.exe'

# Start menu shortcut
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Glance.lnk'
$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.IconLocation = "$exe,0"
$s.Description = 'Glance Image Viewer'
$s.Save()

# File associations (per user, HKCU only)
$exts = '.png','.apng','.jpg','.jpeg','.jpe','.jfif','.gif','.webp','.bmp','.dib','.ico','.tif','.tiff',
        '.tga','.pnm','.pbm','.pgm','.ppm','.qoi','.hdr','.exr','.dds','.svg','.svgz'
$classes = 'HKCU:\Software\Classes'
$progId = 'Glance.Image'
$cmd = "`"$exe`" `"%1`""

New-Item -Force "$classes\$progId" | Out-Null
Set-ItemProperty "$classes\$progId" -Name '(default)' -Value 'Image'
Set-ItemProperty "$classes\$progId" -Name 'FriendlyTypeName' -Value 'Image'
New-Item -Force "$classes\$progId\DefaultIcon" | Out-Null
Set-ItemProperty "$classes\$progId\DefaultIcon" -Name '(default)' -Value "$exe,0"
New-Item -Force "$classes\$progId\shell\open\command" | Out-Null
Set-ItemProperty "$classes\$progId\shell\open\command" -Name '(default)' -Value $cmd

New-Item -Force "$classes\Applications\glance.exe\shell\open\command" | Out-Null
Set-ItemProperty "$classes\Applications\glance.exe" -Name 'FriendlyAppName' -Value 'Glance'
Set-ItemProperty "$classes\Applications\glance.exe\shell\open\command" -Name '(default)' -Value $cmd
New-Item -Force "$classes\Applications\glance.exe\SupportedTypes" | Out-Null

$cap = 'HKCU:\Software\Glance\Capabilities'
New-Item -Force "$cap\FileAssociations" | Out-Null
Set-ItemProperty $cap -Name 'ApplicationName' -Value 'Glance'
Set-ItemProperty $cap -Name 'ApplicationDescription' -Value 'A fast, simple image viewer'
foreach ($e in $exts) {
    New-Item -Force "$classes\$e\OpenWithProgids" | Out-Null
    New-ItemProperty -Force "$classes\$e\OpenWithProgids" -Name $progId -Value '' -PropertyType String | Out-Null
    New-ItemProperty -Force "$classes\Applications\glance.exe\SupportedTypes" -Name $e -Value '' -PropertyType String | Out-Null
    Set-ItemProperty "$cap\FileAssociations" -Name $e -Value $progId
}
New-Item -Force 'HKCU:\Software\RegisteredApplications' | Out-Null
Set-ItemProperty 'HKCU:\Software\RegisteredApplications' -Name 'Glance' -Value 'Software\Glance\Capabilities'

# Tell Explorer that associations changed.
Add-Type -Namespace Win32 -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, int f, IntPtr a, IntPtr b);'
[Win32.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host "Glance installed to $dir"
Write-Host "To make it your default image viewer: Settings > Apps > Default apps > Glance."
