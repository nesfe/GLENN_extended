# Запуск GLENN Extended 4.0.0-beta.2

Скачайте полный архив Windows или Linux из [релиза beta.2](https://github.com/nesfe/GLENN_extended/releases/tag/v4.0.0-beta.2). OpenFOAM 2412 и его библиотеки уже внутри. Расчёты выполняются локально, без подключения к сети, отдельной установки решателя, WSL или MPI.

- **Windows 10/11 x64:** извлеките весь ZIP в обычную папку и запустите `glenn-extended.exe`. Не запускайте программу внутри ZIP и не переносите EXE отдельно от папки `runtime`. Коммерческой цифровой подписи нет.
- **Linux x64:** извлеките TAR.GZ и запустите `./glenn-extended`. Сборка рассчитана на Ubuntu 22.04 или совместимую более новую систему. Для окна и звука нужны ALSA, X11/Wayland и Vulkan/OpenGL. На Ubuntu: `sudo apt install libasound2 libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers`; в новых выпусках ALSA может называться `libasound2t64`.

macOS в этом релизе не выпускается. Старые Mac-архивы beta.1 не содержат комплектного CFD.

Рядом с приложением находятся CLI `glenn-cfd`, `runtime/openfoam`, документация и лицензии. Папку можно переносить целиком, в том числе в путь с пробелами и кириллицей. Результаты CFD сохраняются отдельно; GLENN создаёт служебные ссылки для ограничений путей OpenFOAM и удаляет только ссылки.

При отсутствии поддерживаемого графического адаптера окно не запустится. На Linux можно использовать программный Vulkan-драйвер Mesa. Исходники собираются без звука через `cargo build --release --no-default-features`; комплектование CFD описано в `scripts/bundle_windows.py` и `scripts/bundle_linux.py`.

Контрольные суммы находятся в `SHA256SUMS.txt`. В PowerShell: `Get-FileHash .\GLENN-Extended-4.0.0-beta.2-windows-x86_64.zip -Algorithm SHA256`. В Linux: `sha256sum GLENN-Extended-4.0.0-beta.2-linux-x86_64.tar.gz`.

Запуск объёмного расчёта — в [CFD.md](CFD.md). Лицензии и исходники комплектных компонентов — в [THIRD_PARTY.md](THIRD_PARTY.md).
