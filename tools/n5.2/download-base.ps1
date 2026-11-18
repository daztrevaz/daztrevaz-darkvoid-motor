$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repoBase = 'Qwen/Qwen3-TTS-12Hz-1.7B-Base'
$revisionBase = 'fd4b254389122332181a7c3db7f27e918eec64e3'
$rootBase = 'D:\DARKVOID\modelos-teste'
$targetBase = Join-Path $rootBase 'models\qwen3-base'
$metadataBase = Invoke-RestMethod -Uri "https://huggingface.co/api/models/$repoBase/revision/${revisionBase}?blobs=true" -TimeoutSec 30
if ($metadataBase.sha -ne $revisionBase -or $metadataBase.cardData.license -ne 'apache-2.0') { throw 'Revisão ou licença divergente.' }
$totalBase = [long](($metadataBase.siblings | Measure-Object size -Sum).Sum)
if ($totalBase -gt 8000000000) { throw 'Download ultrapassa o limite de 8 GB.' }
$manifestBase = [ordered]@{ repo=$repoBase; revision=$revisionBase; license='Apache-2.0'; total_bytes=$totalBase; downloaded_bytes=0; files=@() }
New-Item -ItemType Directory -Path (Join-Path $rootBase 'evidence') -Force | Out-Null
foreach ($itemBase in $metadataBase.siblings) {
    $pathBase = Join-Path $targetBase $itemBase.rfilename
    New-Item -ItemType Directory -Path (Split-Path -Parent $pathBase) -Force | Out-Null
    if (-not (Test-Path -LiteralPath $pathBase)) {
        Write-Output ("Baixando {0}: {1} bytes" -f $itemBase.rfilename, $itemBase.size)
        $partBase = $pathBase + '.part'
        & curl.exe --fail --silent --show-error --location --retry 2 --continue-at - --output $partBase "https://huggingface.co/$repoBase/resolve/$revisionBase/$($itemBase.rfilename)"
        if ($LASTEXITCODE -ne 0) { throw 'Transferência interrompida; parcial preservado para retomada.' }
        if ((Get-Item -LiteralPath $partBase).Length -ne $itemBase.size) { throw 'Tamanho divergente.' }
        $shaBase = (Get-FileHash -LiteralPath $partBase -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($itemBase.lfs -and $shaBase -ne $itemBase.lfs.sha256) { throw 'SHA-256 divergente.' }
        Move-Item -LiteralPath $partBase -Destination $pathBase
        $manifestBase.downloaded_bytes += [long]$itemBase.size
    }
    $shaBase = (Get-FileHash -LiteralPath $pathBase -Algorithm SHA256).Hash.ToLowerInvariant()
    if ((Get-Item -LiteralPath $pathBase).Length -ne $itemBase.size -or ($itemBase.lfs -and $shaBase -ne $itemBase.lfs.sha256)) { throw 'Arquivo local divergente.' }
    $manifestBase.files += @{name=$itemBase.rfilename;bytes=[long]$itemBase.size;sha256=$shaBase;upstream_sha256=$itemBase.lfs.sha256}
    [System.IO.File]::WriteAllText((Join-Path $rootBase 'evidence\qwen3-base-manifest.json'), ($manifestBase | ConvertTo-Json -Depth 6), [System.Text.UTF8Encoding]::new($false))
}
[pscustomobject]$manifestBase | Select-Object revision,total_bytes,downloaded_bytes | ConvertTo-Json
