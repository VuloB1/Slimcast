# Genera el icono (assets/slimcast.ico) y las imágenes del instalador (installer/art/*.png).
# Uso: powershell -ExecutionPolicy Bypass -File installer\art.ps1
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot
$art = New-Item -ItemType Directory -Force "$PSScriptRoot\art"
$graphite = [Drawing.Color]::FromArgb(0x14, 0x14, 0x15)
$track = [Drawing.Color]::FromArgb(0x4a, 0x4a, 0x4f)
$amber = [Drawing.Color]::FromArgb(0xe8, 0xa3, 0x3d)

function Fill-RRect($g, $color, $x, $y, $w, $h, $r) {
  $p = New-Object Drawing.Drawing2D.GraphicsPath
  $d = 2 * $r
  $p.AddArc($x, $y, $d, $d, 180, 90); $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90); $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure(); $g.FillPath((New-Object Drawing.SolidBrush $color), $p)
}
function New-Canvas($w, $h) {
  $bmp = New-Object Drawing.Bitmap ([int]$w), ([int]$h)
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.PixelOffsetMode = 'HighQuality'; $g.TextRenderingHint = 'AntiAliasGridFit'
  return $bmp, $g
}
# El logo: barra larga (original) y barra corta ámbar (comprimido), sobre un cuadrado de 96 unidades.
function Draw-Logo($g, $ox, $oy, $s, [switch]$NoTile) {
  $k = $s / 96.0
  if (-not $NoTile) { Fill-RRect $g $graphite $ox $oy $s $s (22 * $k) }
  # en tamaños pequeños las barras engordan y se ajustan a píxeles enteros para seguir leyéndose
  if ($s -le 32) { $bh = [math]::Round(16 * $k); $y1 = [math]::Round(26 * $k); $y2 = [math]::Round(54 * $k) }
  else { $bh = 12 * $k; $y1 = 32 * $k; $y2 = 52 * $k }
  $x = [math]::Round(18 * $k); $r = [math]::Max(1, 3 * $k)
  Fill-RRect $g $track ($ox + $x) ($oy + $y1) ([math]::Round(60 * $k)) $bh $r
  Fill-RRect $g $amber ($ox + $x) ($oy + $y2) ([math]::Round(27 * $k)) $bh $r
}

# Icono .ico con PNGs dentro
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$pngs = foreach ($s in $sizes) {
  $bmp, $g = New-Canvas $s $s
  Draw-Logo $g 0 0 $s
  $ms = New-Object IO.MemoryStream; $bmp.Save($ms, [Drawing.Imaging.ImageFormat]::Png); , $ms.ToArray()
}
$f = New-Object IO.BinaryWriter ([IO.File]::Create("$root\assets\slimcast.ico"))
$f.Write([uint16]0); $f.Write([uint16]1); $f.Write([uint16]$sizes.Count)
$off = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $b = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
  $f.Write([byte]$b); $f.Write([byte]$b); $f.Write([uint16]0); $f.Write([uint16]1); $f.Write([uint16]32)
  $f.Write([uint32]$pngs[$i].Length); $f.Write([uint32]$off); $off += $pngs[$i].Length
}
foreach ($p in $pngs) { $f.Write($p) }
$f.Close()

# Imagen lateral del asistente (164x314 a 100 %) y la pequeña de la cabecera (55x55), en varias escalas
foreach ($pct in 100, 125, 150, 175, 200) {
  $k = $pct / 100.0
  $w = [math]::Round(164 * $k); $h = [math]::Round(314 * $k)
  $bmp, $g = New-Canvas $w $h
  $g.Clear($graphite)
  $logo = [math]::Round(96 * $k)
  Draw-Logo $g ([math]::Round(($w - $logo) / 2)) ([math]::Round(96 * $k)) $logo -NoTile
  $font = New-Object Drawing.Font 'Segoe UI Semibold', (15 * $k), ([Drawing.FontStyle]::Regular), ([Drawing.GraphicsUnit]::Pixel)
  $fmt = New-Object Drawing.StringFormat; $fmt.Alignment = 'Center'
  $g.DrawString('Slimcast', $font, (New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(0xed, 0xed, 0xed))), (New-Object Drawing.RectangleF 0, (196 * $k), $w, (30 * $k)), $fmt)
  $bmp.Save("$art\wizard-$pct.png")

  $s = [math]::Round(55 * $k)
  $bmp, $g = New-Canvas $s $s
  Draw-Logo $g 0 0 $s
  $bmp.Save("$art\small-$pct.png")
}
"ok"
