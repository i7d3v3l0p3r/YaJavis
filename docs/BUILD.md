# Сборка YaJavis

Для сборки пакетов колонки нужны Git, актуальный стабильный Rust, Zig, CMake, Ninja, Python 3 и tar. Сборка официальных плагинов под ARMv7 также использует libclang для привязок QuickJS.

### Linux и macOS

Для `build.sh` также нужны POSIX shell, zip и sha256sum. На macOS команда `sha256sum` входит в GNU coreutils. Из корня проекта выполните следующие команды.

```sh
rustup target add aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf
VERSION=0.10.0 sh build.sh
cargo test --manifest-path jarvis/Cargo.toml --locked --workspace --all-features
cargo build --manifest-path jarvis/Cargo.toml --locked --release -p jarvis-setup
```

### Windows (PowerShell)

`build.ps1` запускает сборку пакетов колонки инструментами Windows. Добавьте их в `PATH`. Для Rust с целевой платформой MSVC установите Visual Studio Build Tools с компонентами C++. При сборке плагинов ARMv7 скрипт ищет `libclang.dll` в стандартном каталоге LLVM (`C:\Program Files\LLVM\bin`). Если LLVM установлен в другом месте, задайте `LIBCLANG_PATH` с путём к каталогу, содержащему `libclang.dll`.

Из корня проекта выполните следующие команды.

```powershell
rustup target add aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf
$env:VERSION = '0.10.0'
.\build.ps1
cargo test --manifest-path .\jarvis\Cargo.toml --locked --workspace --all-features
cargo build --manifest-path .\jarvis\Cargo.toml --locked --release -p jarvis-setup
```

### Пакеты и локальная установка

Сборка создаёт один пакет чистой прошивки на архитектуру и отдельный архив для каждого официального плагина. Каталог указывает, какие пакеты можно скачать, а пользователь сам выбирает их в панели.

После чистой установки колонка сама проверяет и применяет опубликованные обновления ядра, если автообновление включено. Файлы установленных плагинов и их данные находятся в отдельных каталогах.

`build/release` содержит ZIP-пакеты прошивки, ядра для обновления, пакеты плагинов и контрольные суммы. GitHub Actions собирает установщики для Windows, Linux и macOS. Для сборки под одну архитектуру задайте в окружении `STATION_ARCHES` со значением `arm64` или `armv7`. Для своего репозитория задайте `REPO` в формате `owner/repo`.

Установщик ищет `adb` рядом с собой, в PATH и папках Android SDK. На Windows, macOS и Linux x86-64 он скачает Android Platform Tools с сайта Google, если ADB ещё нет. Чтобы указать свой `adb`, задайте `JARVIS_ADB` с полным путём.

Если Rust-версия ещё не опубликована в GitHub Releases, запустите локально собранный установщик с путём к каталогу ZIP-пакетов. Команды ниже выполняются из корня проекта.

**Linux и macOS**

```sh
JARVIS_SETUP_PAYLOAD="$PWD/build/release" ./jarvis/target/release/jarvis-setup
```

**Windows (PowerShell)**

```powershell
$env:JARVIS_SETUP_PAYLOAD = (Resolve-Path .\build\release).Path
.\jarvis\target\release\jarvis-setup.exe
```

Установщик сам выберет `station-arm64.zip` или `station-armv7.zip` по архитектуре системы колонки. Для каталогов с локальными пакетами сверяйте ZIP-файлы с `SHA256SUMS` перед установкой.

| Каталог | Содержимое |
| --- | --- |
| `jarvis/core` | Rust-ядро, панель, запуск модулей и обновление |
| `jarvis/modules` | Rust-модули подключений, музыки и обучения |
| `jarvis/station-runtime` | Запуск ядра и откат обновления на колонке |
| `jarvis/setup` | Rust-установщик для компьютера |
| `jarvisd/ui`, `installer/ui` | Веб-интерфейсы ядра и установщика |
| `station` | Скрипт автозапуска и нативные аудиоадаптеры прошивки |
| `plugins` | Манифесты модулей и Lua-плагины |
| `models`, `kws` | Модели слова и инструменты подготовки обучающих данных |
