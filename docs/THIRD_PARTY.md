# Комплектные компоненты beta.2

Исходный код GLENN распространяется по MIT (`LICENSE`). OpenFOAM запускается отдельным процессом и обменивается с GLENN файлами стандартного case. Лицензии комплектных программ и библиотек сохраняются; MIT GLENN не заменяет их.

## OpenCFD OpenFOAM 2412 — GPL-3.0-or-later

- Windows: [официальный MinGW-пакет 2412](https://dl.openfoam.com/source/v2412/OpenFOAM-v2412-windows-mingw.exe), SHA-256 `7c31c0526786307465cd34b4ac820530fbd52c31225d9b03713903a8f84a7028`. Исходники [OpenFOAM-v2412.tgz](https://dl.openfoam.com/source/v2412/OpenFOAM-v2412.tgz), [ThirdParty-v2412.tgz](https://dl.openfoam.com/source/v2412/ThirdParty-v2412.tgz). Инструкции сборки входят в исходники (`doc/Build.md`, `wmake`); сведения MinGW входят в upstream `etc-mingw` и [описание OpenCFD](https://www.openfoam.com/openfoam-mingw-cross-compilation).
- Linux: пакеты OpenCFD `2412.260127-1` для Ubuntu 22.04, закреплённые SHA-256 в `scripts/install_linux_solver.sh`. Build ID — `b8cf4d35-20260127`. Соответствующие исходники: [архив коммита b8cf4d35](https://gitlab.com/openfoam/core/openfoam/-/archive/b8cf4d35/openfoam-b8cf4d35.tar.gz).
- Копии архивов исходников публикуются рядом с бинарными пакетами в релизе. Они нужны для изучения/сборки, скачивать их для запуска GLENN не требуется.

Упаковка GLENN не меняет исходный код OpenFOAM. Она выбирает шесть исполняемых команд, сохраняет библиотеки и словари и заменяет MPI Pstream на штатную последовательную реализацию из того же пакета. Windows сохраняет `COPYING`, `META-INFO`, `etc-mingw`, `upstream-licenses`; Linux сохраняет `META-INFO`, `upstream-licences` и полные тексты GPL/LGPL. Скрипты упаковки находятся в исходниках GLENN той же версии.

## Библиотеки

Пакет содержит библиотеки OpenFOAM, GCC runtime, zlib и зависимости расчётных библиотек. Их условия не заменяются лицензией GLENN. Windows сохраняет библиотеки поставки OpenCFD; исходники сторонних расчётных компонентов входят в ThirdParty. Для GCC runtime применяются GPL и GCC Runtime Library Exception: [текст исключения](https://gcc.gnu.org/onlinedocs/libstdc++/manual/license.html), [исходники GCC](https://gcc.gnu.org/releases.html). MinGW-w64: [исходники и лицензии](https://www.mingw-w64.org/).

Linux собирает тексты copyright пакетов, из которых копируются внешние библиотеки, в `runtime/openfoam/upstream-licences`. Версии и имена этих пакетов сохраняются в `runtime/openfoam/dependency-packages.txt`; соответствующие исходники доступны в [архиве Ubuntu](https://archive.ubuntu.com/ubuntu/pool/) через `apt-get source <имя>=<версия>` с включённым deb-src для Ubuntu 22.04. glibc и загрузчик ELF не комплектуются.

Шрифт DejaVu: `LICENSE-DejaVu.txt` в корне архива. Лицензии Rust-зависимостей указаны в их пакетах; точные версии закреплены в `Cargo.lock`.
