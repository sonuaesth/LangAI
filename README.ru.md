# LangAI

Локальный тренажёр переводов для Windows на Tauri 2, Next.js static export, Rust и SQLite.

## Запуск

Установите Node.js 20+, Rust stable с MSVC, Microsoft C++ Build Tools и WebView2.

```powershell
npm install
npm run tauri dev
```

API-ключ вводится в «Настройках» и хранится только в Windows Credential Manager. SQLite создаётся в системной директории данных приложения.

## Проверка и сборка

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

Инсталляторы появятся в `src-tauri/target/release/bundle`. Node.js-сервер готовому приложению не требуется.
