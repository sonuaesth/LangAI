# Локальный запуск LangAI

## Требования

- Docker Desktop с запущенным Docker Engine;
- PowerShell 7 или Windows PowerShell 5.1;
- свободный порт `8080`.

Node.js и Rust не требуются для запуска готового Docker-окружения: frontend и backend собираются внутри контейнера.

## Первый запуск

Из корня репозитория:

```powershell
powershell -ExecutionPolicy Bypass -File .\infra\init-local-env.ps1
docker compose --env-file .env -f infra\compose.yml up --build
```

После запуска:

- web-интерфейс: <http://127.0.0.1:8080>;
- health-check: <http://127.0.0.1:8080/api/v1/health>.

При первом локальном запуске регистрация открыта. Создайте аккаунт через web-форму, используя пароль длиной не менее 12 символов.

## Подключение desktop

В desktop необходимо указать:

- сервер: `http://127.0.0.1:8080`;
- email и пароль созданного аккаунта;
- произвольное понятное имя устройства.

Desktop сначала создаёт резервную копию существующей SQLite-базы, затем отправляет предложения, переводы, упражнения, темы, настройки и аудио. Исходная локальная база и ключи в Windows Credential Manager не удаляются.

## Повторный запуск

```powershell
docker compose --env-file .env -f infra\compose.yml up
```

## Остановка

```powershell
docker compose --env-file .env -f infra\compose.yml down
```

Команда `down` сохраняет PostgreSQL и аудио в Docker volumes. Не добавляйте флаг `-v`, если не хотите удалить локальные серверные данные.

## Диагностика

```powershell
docker compose --env-file .env -f infra\compose.yml ps
docker compose --env-file .env -f infra\compose.yml logs -f api
docker compose --env-file .env -f infra\compose.yml logs -f postgres
```

Если Docker Engine не запущен, команды сообщат, что не удаётся подключиться к `docker_engine`. Запустите Docker Desktop и повторите команду.
