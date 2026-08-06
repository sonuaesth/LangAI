param([switch]$Force)

$ErrorActionPreference = "Stop"

$target = Join-Path (Split-Path $PSScriptRoot -Parent) ".env"
if ((Test-Path -LiteralPath $target) -and -not $Force) {
    throw ".env already exists. Use -Force only if you intentionally want to replace local secrets."
}

$databaseBytes = New-Object byte[] 24
$pepperBytes = New-Object byte[] 32
$masterKeyBytes = New-Object byte[] 32
$random = [Security.Cryptography.RandomNumberGenerator]::Create()
try {
    $random.GetBytes($databaseBytes)
    $random.GetBytes($pepperBytes)
    $random.GetBytes($masterKeyBytes)
}
finally {
    $random.Dispose()
}

$databasePassword = -join ($databaseBytes | ForEach-Object { $_.ToString("x2") })
$sessionPepper = -join ($pepperBytes | ForEach-Object { $_.ToString("x2") })
$masterKey = [Convert]::ToBase64String($masterKeyBytes)

@(
    "POSTGRES_DB=langai"
    "POSTGRES_USER=langai"
    "POSTGRES_PASSWORD=$databasePassword"
    "SESSION_PEPPER=$sessionPepper"
    "SECRETS_MASTER_KEY=$masterKey"
    "SESSION_TTL_SECONDS=2592000"
    "REGISTRATION_MODE=open"
    "MAX_AUDIO_BYTES=26214400"
    "LANGAI_ENV=development"
    "RUST_LOG=langai_server=info,tower_http=info"
) | Set-Content -LiteralPath $target -Encoding utf8

Write-Host "Created $target"
Write-Host "Keep this file private. It is ignored by Git."
