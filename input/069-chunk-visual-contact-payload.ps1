Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK chunked visual contact sheet payload: start'
$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$payloadPath = Join-Path $outputDirectory 'visual-contact-tiny.base64.txt'
if (-not (Test-Path -LiteralPath $payloadPath)) {
    throw 'visual-contact-tiny.base64.txt is missing.'
}
$payload = [IO.File]::ReadAllText($payloadPath).Trim()
$chunkSize = 1000
$chunks = for ($offset = 0; $offset -lt $payload.Length; $offset += $chunkSize) {
    $length = [Math]::Min($chunkSize, $payload.Length - $offset)
    $payload.Substring($offset, $length)
}
[IO.File]::WriteAllLines(
    (Join-Path $outputDirectory 'visual-contact-tiny.base64.chunks.txt'),
    $chunks,
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK chunked visual contact sheet payload: passed ($($chunks.Count) chunks)"
