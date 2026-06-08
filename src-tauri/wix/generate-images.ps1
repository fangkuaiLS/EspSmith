# Generate WiX installer banner (493x58) and dialog (493x312) BMP images
# Dark theme for EspSmith MSI installer

Add-Type -AssemblyName System.Drawing
$wixDir = $PSScriptRoot

function Create-Banner {
    $w = 493; $h = 58
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'HighQuality'
    $g.TextRenderingHint = 'AntiAlias'

    # Dark gradient background: deep navy to dark purple
    for ($x = 0; $x -lt $w; $x++) {
        $t = $x / $w
        $r = [int](20 + 15 * $t)
        $gVal = [int](22 + 18 * $t)
        $b = [int](38 + 22 * $t)
        $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb($r, $gVal, $b))
        $g.DrawLine($pen, $x, 0, $x, $h)
    }

    # Top accent line - blue to purple gradient
    for ($x = 0; $x -lt $w; $x++) {
        $t = $x / $w
        $r = [int](59 + 80 * $t)
        $gr = [int](130 - 38 * $t)
        $bl = [int](246 - 54 * $t)
        $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb($r, $gr, $bl), 2)
        $g.DrawLine($pen, $x, 0, $x, 1)
    }

    # Title text "EspSmith"
    try {
        $titleFont = New-Object System.Drawing.Font("Segoe UI", 16, [System.Drawing.FontStyle]::Bold)
    } catch {
        $titleFont = New-Object System.Drawing.Font("Microsoft YaHei", 14, [System.Drawing.FontStyle]::Bold)
    }
    $titleBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $g.DrawString("EspSmith", $titleFont, $titleBrush, 18, 12)

    # Subtitle
    try {
        $subFont = New-Object System.Drawing.Font("Segoe UI", 8.5, [System.Drawing.FontStyle]::Regular)
    } catch {
        $subFont = New-Object System.Drawing.Font("Microsoft YaHei", 8, [System.Drawing.FontStyle]::Regular)
    }
    $subBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(170, 175, 195))
    $g.DrawString("AI-Powered ESP32 Development Studio", $subFont, $subBrush, 20, 36)

    $g.Dispose()
    # WiX requires 24-bit BMP (Format24bppRgb), not 32-bit
    $bmp24 = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g24 = [System.Drawing.Graphics]::FromImage($bmp24)
    $g24.DrawImage($bmp, 0, 0)
    $g24.Dispose()
    $bmp.Dispose()
    $bmp24.Save("$wixDir\banner.bmp", [System.Drawing.Imaging.ImageFormat]::Bmp)
    $bmp24.Dispose()
}

function Create-Dialog {
    $w = 493; $h = 312
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'HighQuality'

    # Dark gradient background: top to bottom
    for ($y = 0; $y -lt $h; $y++) {
        $t = $y / $h
        $r = [int](25 - 10 * $t)
        $gVal = [int](30 - 12 * $t)
        $b = [int](50 - 17 * $t)
        $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb($r, $gVal, $b))
        $g.DrawLine($pen, 0, $y, $w, $y)
    }

    # Decorative glow circle 1 - blue (top-right area)
    $glowRect1 = New-Object System.Drawing.Rectangle(300, 40, 220, 220)
    $path1 = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path1.AddEllipse($glowRect1)
    $gb1 = New-Object System.Drawing.Drawing2D.PathGradientBrush($path1)
    $gb1.CenterColor = [System.Drawing.Color]::FromArgb(35, 59, 130, 246)
    $gb1.SurroundColors = @([System.Drawing.Color]::Transparent)
    $g.FillEllipse($gb1, $glowRect1)

    # Decorative glow circle 2 - purple (bottom-left area)
    $glowRect2 = New-Object System.Drawing.Rectangle(20, 140, 200, 200)
    $path2 = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path2.AddEllipse($glowRect2)
    $gb2 = New-Object System.Drawing.Drawing2D.PathGradientBrush($path2)
    $gb2.CenterColor = [System.Drawing.Color]::FromArgb(25, 139, 92, 246)
    $gb2.SurroundColors = @([System.Drawing.Color]::Transparent)
    $g.FillEllipse($gb2, $glowRect2)

    # Center logo box
    $logoX = 196; $logoY = 55; $logoW = 100; $logoH = 100

    # Logo background with subtle gradient
    $lgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.Rectangle($logoX, $logoY, $logoW, $logoH)),
        [System.Drawing.Color]::FromArgb(45, 45, 65),
        [System.Drawing.Color]::FromArgb(32, 37, 57),
        [System.Drawing.Drawing2D.LinearGradientMode]::Vertical
    )
    $g.FillRectangle($lgBrush, $logoX, $logoY, $logoW, $logoH)

    # Logo border
    $borderPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(70, 70, 95), 1.5)
    $g.DrawRectangle($borderPen, $logoX, $logoY, $logoW, $logoH)

    # Inner accent line at top of logo box
    $accentPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(59, 130, 246), 1.5)
    $g.DrawLine($accentPen, $logoX + 4, $logoY + 4, $logoX + $logoW - 4, $logoY + 4)

    # Logo text "ES"
    try {
        $logoFont = New-Object System.Drawing.Font("Segoe UI", 24, [System.Drawing.FontStyle]::Bold)
    } catch {
        $logoFont = New-Object System.Drawing.Font("Microsoft YaHei", 20, [System.Drawing.FontStyle]::Bold)
    }
    $logoBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(70, 140, 255))
    $text = "ES"
    $sz = $g.MeasureString($text, $logoFont)
    $tx = $logoX + ($logoW - $sz.Width) / 2
    $ty = $logoY + ($logoH - $sz.Height) / 2
    $g.DrawString($text, $logoFont, $logoBrush, $tx, $ty)

    # Main title below logo
    try {
        $titleFont = New-Object System.Drawing.Font("Segoe UI", 13, [System.Drawing.FontStyle]::Bold)
    } catch {
        $titleFont = New-Object System.Drawing.Font("Microsoft YaHei", 12, [System.Drawing.FontStyle]::Bold)
    }
    $titleBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $tsz = $g.MeasureString("EspSmith", $titleFont)
    $g.DrawString("EspSmith", $titleFont, $titleBrush, ($w - $tsz.Width) / 2, 172)

    # Subtitle
    try {
        $subFont = New-Object System.Drawing.Font("Segoe UI", 8.5, [System.Drawing.FontStyle]::Regular)
    } catch {
        $subFont = New-Object System.Drawing.Font("Microsoft YaHei", 8, [System.Drawing.FontStyle]::Regular)
    }
    $subBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(145, 150, 175))
    $ssz = $g.MeasureString("AI-Powered ESP32 IDE", $subFont)
    $g.DrawString("AI-Powered ESP32 IDE", $subFont, $subBrush, ($w - $ssz.Width) / 2, 200)

    # Feature tags row
    $featY = 245
    $features = @(
        @{Text="Smart Code"; X=95},
        @{Text="One-Click Flash"; X=215},
        @{Text="Serial Debug"; X=355}
    )
    try {
        $featFont = New-Object System.Drawing.Font("Segoe UI", 7.5, [System.Drawing.FontStyle]::Regular)
    } catch {
        $featFont = New-Object System.Drawing.Font("Microsoft YaHei", 7, [System.Drawing.FontStyle]::Regular)
    }
    $featBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(110, 115, 140))

    foreach ($f in $features) {
        # Tag background pill
        $fsz = $g.MeasureString($f.Text, $featFont)
        $pillW = $fsz.Width + 16
        $pillH = $fsz.Height + 8
        $pillX = $f.X - $pillW / 2
        $pillY = $featY - 2

        $pillPath = New-Object System.Drawing.Drawing2D.GraphicsPath
        $pillR = $pillH / 2
        $pillPath.AddArc($pillX, $pillY, $pillR * 2, $pillR * 2, 180, 90)
        $pillPath.AddArc($pillX + $pillW - $pillR * 2, $pillY, $pillR * 2, $pillR * 2, 270, 90)
        $pillPath.AddArc($pillX + $pillW - $pillR * 2, $pillY + $pillH - $pillR * 2, $pillR * 2, $pillR * 2, 0, 90)
        $pillPath.AddArc($pillX, $pillY + $pillH - $pillR * 2, $pillR * 2, $pillR * 2, 90, 90)
        $pillPath.CloseFigure()

        $pillBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(30, 30, 45))
        $g.FillPath($pillBrush, $pillPath)

        $pillBorder = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(50, 50, 70), 0.8)
        $g.DrawPath($pillBorder, $pillPath)

        $g.DrawString($f.Text, $featFont, $featBrush, $f.X - $fsz.Width / 2, $featY + 2)
    }

    # Version text at bottom
    $verFont = New-Object System.Drawing.Font("Segoe UI", 7)
    $verBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(80, 85, 105))
    $vsz = $g.MeasureString("v0.1.0", $verFont)
    $g.DrawString("v0.1.0", $verFont, $verBrush, ($w - $vsz.Width) / 2, $h - 22)

    $g.Dispose()
    # WiX requires 24-bit BMP (Format24bppRgb), not 32-bit
    $bmp24 = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g24 = [System.Drawing.Graphics]::FromImage($bmp24)
    $g24.DrawImage($bmp, 0, 0)
    $g24.Dispose()
    $bmp.Dispose()
    $bmp24.Save("$wixDir\dialog.bmp", [System.Drawing.Imaging.ImageFormat]::Bmp)
    $bmp24.Dispose()
}

Write-Host "Generating WiX installer images..."
Create-Banner
Write-Host "  Banner (493x58): OK"
Create-Dialog
Write-Host "  Dialog (493x312): OK"
Write-Host "`nDone! Images saved to: $wixDir"