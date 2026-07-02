# 生成 Tauri 所需图标（纯色占位，可后续用 `cargo tauri icon` 替换为真实图标）。
Add-Type -AssemblyName System.Drawing

$dir = "E:\Projects\Water2Rust\desktop\src-tauri\icons"
New-Item -ItemType Directory -Force -Path $dir | Out-Null

# 主色 #2563EB（与前端 primary 一致）。
$color = [System.Drawing.Color]::FromArgb(255, 37, 99, 235)

function New-Png([int]$size, [string]$path) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear($color)
    # 画一个简单的白色水滴符号占位。
    $font = New-Object System.Drawing.Font("Segoe UI", [int]($size * 0.5), [System.Drawing.FontStyle]::Bold)
    $brush = [System.Drawing.Brushes]::White
    $fmt = New-Object System.Drawing.StringFormat
    $fmt.Alignment = [System.Drawing.StringAlignment]::Center
    $fmt.LineAlignment = [System.Drawing.StringAlignment]::Center
    $g.DrawString("W", $font, $brush, (New-Object System.Drawing.RectangleF(0, 0, $size, $size)), $fmt)
    $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

New-Png 32 "$dir\32x32.png"
New-Png 128 "$dir\128x128.png"
New-Png 256 "$dir\128x128@2x.png"
New-Png 512 "$dir\icon.png"

# 生成 .ico（含 256 尺寸）。
$icoBmp = New-Object System.Drawing.Bitmap(256, 256)
$g = [System.Drawing.Graphics]::FromImage($icoBmp)
$g.Clear($color)
$font = New-Object System.Drawing.Font("Segoe UI", 128, [System.Drawing.FontStyle]::Bold)
$fmt = New-Object System.Drawing.StringFormat
$fmt.Alignment = [System.Drawing.StringAlignment]::Center
$fmt.LineAlignment = [System.Drawing.StringAlignment]::Center
$g.DrawString("W", $font, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF(0, 0, 256, 256)), $fmt)
$g.Dispose()
$hicon = $icoBmp.GetHicon()
$icon = [System.Drawing.Icon]::FromHandle($hicon)
$fs = New-Object System.IO.FileStream("$dir\icon.ico", [System.IO.FileMode]::Create)
$icon.Save($fs)
$fs.Close()
$icoBmp.Dispose()

Write-Output "图标已生成："
Get-ChildItem $dir | ForEach-Object { "  {0}  ({1} bytes)" -f $_.Name, $_.Length }
