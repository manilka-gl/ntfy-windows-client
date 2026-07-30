Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host 'CHECK visual contact sheet inspection payload: start'
$outputDirectory = [IO.Path]::GetFullPath($env:WORKER_OUTPUT_DIRECTORY)
$imagePath = Join-Path $outputDirectory 'visual-contact-tiny.jpg'
if (-not (Test-Path -LiteralPath $imagePath)) {
    throw 'visual-contact-tiny.jpg is missing.'
}
$payload = [Convert]::ToBase64String([IO.File]::ReadAllBytes($imagePath))
[IO.File]::WriteAllText(
    (Join-Path $outputDirectory 'visual-contact-tiny.base64.txt'),
    $payload,
    [Text.UTF8Encoding]::new($false)
)
Write-Host "CHECK visual contact sheet inspection payload: passed ($($payload.Length) characters)"
