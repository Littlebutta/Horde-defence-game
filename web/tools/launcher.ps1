# Pixel Horde launcher for Windows.
# Pulls the latest build from GitHub, serves the game on localhost, opens your browser,
# and keeps pulling new commits in the background. The open game page notices and reloads.
# Run it through Play.bat (or: powershell -ExecutionPolicy Bypass -File tools\launcher.ps1).

$ErrorActionPreference = 'Continue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$UserAgent = 'PixelHorde/0.3 (+https://github.com/Littlebutta/Horde-defence-game)'
$Overpass = @('https://overpass-api.de/api/interpreter', 'https://overpass.kumi.systems/api/interpreter')
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Game = $Root
$PullEverySeconds = 60
$HaveGit = [bool](Get-Command git -ErrorAction SilentlyContinue)

function Write-Info($msg) { Write-Host ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $msg) }

function Update-Game {
    if (-not $HaveGit) { return }
    $before = (& git -C $Root rev-parse HEAD 2>$null)
    $out = (& git -C $Root pull --ff-only 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0) {
        Write-Info "Could not pull updates: $($out.Trim())"
        return
    }
    $after = (& git -C $Root rev-parse HEAD 2>$null)
    if ($before -ne $after) { Write-Info "Updated to $($after.Substring(0,7)). The game page will reload." }
}

function Get-GameVersion {
    # Commit id plus the newest file time, so both pulled commits and local edits trigger a reload.
    $stamp = (Get-ChildItem -Path $Game -Recurse -File | Measure-Object -Property LastWriteTimeUtc -Maximum).Maximum.Ticks
    $head = if ($HaveGit) { (& git -C $Root rev-parse HEAD 2>$null) } else { 'nogit' }
    return "$head-$stamp"
}

$Mime = @{
    '.html' = 'text/html; charset=utf-8'; '.js' = 'text/javascript; charset=utf-8'; '.mjs' = 'text/javascript; charset=utf-8'
    '.css' = 'text/css; charset=utf-8'; '.json' = 'application/json'; '.png' = 'image/png'; '.jpg' = 'image/jpeg'
    '.svg' = 'image/svg+xml'; '.wasm' = 'application/wasm'; '.txt' = 'text/plain; charset=utf-8'
}

function Send-Bytes($res, [byte[]]$bytes, $type, $status = 200) {
    $res.StatusCode = $status
    $res.ContentType = $type
    $res.ContentLength64 = $bytes.Length
    $res.OutputStream.Write($bytes, 0, $bytes.Length)
}

# Fetches from OpenStreetMap on the game's behalf, so the browser never has to talk to those servers directly.
function Invoke-Remote([string]$url, [string]$method = 'GET', [string]$body = $null, [int]$timeoutMs = 180000) {
    $req = [System.Net.HttpWebRequest]::Create($url)
    $req.Method = $method
    $req.UserAgent = $UserAgent
    $req.Timeout = $timeoutMs
    $req.ReadWriteTimeout = $timeoutMs
    $req.AutomaticDecompression = [System.Net.DecompressionMethods]::GZip -bor [System.Net.DecompressionMethods]::Deflate
    if ($body) {
        $bytes = [Text.Encoding]::UTF8.GetBytes($body)
        $req.ContentType = 'application/x-www-form-urlencoded'
        $req.ContentLength = $bytes.Length
        $s = $req.GetRequestStream(); $s.Write($bytes, 0, $bytes.Length); $s.Close()
    }
    $resp = $req.GetResponse()
    try {
        $reader = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
        return $reader.ReadToEnd()
    } finally { $resp.Close() }
}

function Invoke-Osm($ctx, $path) {
    $res = $ctx.Response
    try {
        if ($path -eq '/__osm/ping') {
            Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes('ok')) 'text/plain; charset=utf-8'
            return
        }
        if ($path -eq '/__osm/geocode') {
            $q = $ctx.Request.QueryString['q']
            Write-Info "Looking up '$q' on OpenStreetMap..."
            $json = Invoke-Remote ("https://nominatim.openstreetmap.org/search?format=jsonv2&limit=1&q=" + [Uri]::EscapeDataString($q)) 'GET' $null 30000
            Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes($json)) 'application/json; charset=utf-8'
            return
        }
        if ($path -eq '/__osm/overpass') {
            $reader = New-Object IO.StreamReader($ctx.Request.InputStream, [Text.Encoding]::UTF8)
            $query = $reader.ReadToEnd()
            $last = $null
            foreach ($u in $Overpass) {
                try {
                    Write-Info "Downloading map data from $([Uri]$u | Select-Object -ExpandProperty Host)..."
                    $json = Invoke-Remote $u 'POST' ('data=' + [Uri]::EscapeDataString($query))
                    Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes($json)) 'application/json; charset=utf-8'
                    Write-Info "Map data downloaded ($([Math]::Round($json.Length / 1MB, 1)) MB)."
                    return
                } catch { $last = $_.Exception.Message; Write-Info "Map server failed: $last" }
            }
            throw $last
        }
        Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes('Not found')) 'text/plain' 404
    } catch {
        $msg = "$($_.Exception.Message)"
        Write-Info "Map request failed: $msg"
        Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes($msg)) 'text/plain; charset=utf-8' 502
    }
}

function Invoke-Request($ctx) {
    $res = $ctx.Response
    # Cross-origin isolation unlocks SharedArrayBuffer, which later builds use for zero-copy threading.
    $res.Headers.Add('Cross-Origin-Opener-Policy', 'same-origin')
    $res.Headers.Add('Cross-Origin-Embedder-Policy', 'credentialless')
    $res.Headers.Add('Cache-Control', 'no-store')
    $path = [Uri]::UnescapeDataString($ctx.Request.Url.AbsolutePath)
    if ($path.StartsWith('/__osm/')) { Invoke-Osm $ctx $path; return }
    if ($path -eq '/__version') {
        Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes((Get-GameVersion))) 'text/plain; charset=utf-8'
        return
    }
    if ($path -eq '/' -or $path -eq '') { $path = '/index.html' }
    $file = [IO.Path]::GetFullPath((Join-Path $Game $path.TrimStart('/')))
    if (-not $file.StartsWith($Game, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $file -PathType Leaf)) {
        Send-Bytes $res ([Text.Encoding]::UTF8.GetBytes('Not found')) 'text/plain' 404
        return
    }
    $ext = [IO.Path]::GetExtension($file).ToLowerInvariant()
    $type = if ($Mime.ContainsKey($ext)) { $Mime[$ext] } else { 'application/octet-stream' }
    Send-Bytes $res ([IO.File]::ReadAllBytes($file)) $type
}

if ($HaveGit) {
    Write-Info 'Checking GitHub for the latest build...'
    Update-Game
} else {
    Write-Info 'Git is not installed, so the game will not update itself. Install it from https://git-scm.com/download/win and clone the repo.'
}

$listener = $null
foreach ($port in 8787..8797) {
    try {
        $l = New-Object System.Net.HttpListener
        $l.Prefixes.Add("http://localhost:$port/")
        $l.Start()
        $listener = $l
        break
    } catch { }
}
if (-not $listener) { Write-Info 'Could not open a local port between 8787 and 8797.'; exit 1 }

$url = $listener.Prefixes | Select-Object -First 1
Write-Info "Pixel Horde is running at $url"
Write-Info "Checking for updates every $PullEverySeconds seconds. Close this window to stop the game server."
Start-Process $url

$nextPull = (Get-Date).AddSeconds($PullEverySeconds)
$pending = $listener.BeginGetContext($null, $null)
try {
    while ($listener.IsListening) {
        if ($pending.AsyncWaitHandle.WaitOne(1000)) {
            $ctx = $listener.EndGetContext($pending)
            $pending = $listener.BeginGetContext($null, $null)
            try { Invoke-Request $ctx } catch { } finally { try { $ctx.Response.Close() } catch { } }
        }
        if ((Get-Date) -ge $nextPull) {
            Update-Game
            $nextPull = (Get-Date).AddSeconds($PullEverySeconds)
        }
    }
} finally {
    $listener.Stop()
}
