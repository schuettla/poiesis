# Android build environment for this session only. Changes nothing permanent.
# Use it from PowerShell before any `npx tauri android ...` command in mobile/:
#
#   . .\android-env.ps1
#
# It assumes Android Studio and the Android SDK are installed in their default places, and the NDK
# version named below (Android Studio > SDK Manager > SDK Tools > NDK (Side by side)).
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "$env:LOCALAPPDATA\Android\Sdk" }
$ndk = "28.2.13676358"

$env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
$env:ANDROID_HOME = $sdk
$env:NDK_HOME = "$sdk\ndk\$ndk"
$env:Path = "$sdk\platform-tools;$env:JAVA_HOME\bin;$env:Path"

foreach ($p in @($env:JAVA_HOME, $env:ANDROID_HOME, $env:NDK_HOME)) {
  if (-not (Test-Path $p)) { Write-Warning "Not found: $p" }
}
