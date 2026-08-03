# Памятка по деплою веб-версии LangAI

Веб-версия состоит из двух Docker-сервисов:

- `api` — Rust API и статически собранный Next.js-интерфейс;
- `postgres` — PostgreSQL 17.

Команды ниже выполняются из корня репозитория, где находятся `.env` и папка `infra`.

## 1. Требования

- Docker Engine с Docker Compose;
- свободный локальный порт `8080`;
- для публичного сервера — домен, HTTPS и reverse proxy (например, Caddy или Nginx).

Node.js и Rust на сервер устанавливать не нужно: приложение собирается внутри Docker.

## 2. Подготовка `.env`

Для локального запуска на Windows можно создать файл автоматически:

```powershell
powershell -ExecutionPolicy Bypass -File .\infra\init-local-env.ps1
```

Для сервера скопируйте шаблон и замените все тестовые секреты:

```bash
cp .env.example .env
```

Обязательные значения:

```dotenv
LANGAI_ENV=production
POSTGRES_DB=langai
POSTGRES_USER=langai
POSTGRES_PASSWORD=<длинный случайный пароль>
SESSION_PEPPER=<случайная строка длиной не менее 32 символов>
SECRETS_MASTER_KEY=<ровно 32 случайных байта в Base64>
REGISTRATION_MODE=open
```

Сгенерировать секреты в PowerShell:

```powershell
$bytes = New-Object byte[] 32
[Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
[Convert]::ToBase64String($bytes)
```

Запустите команду дважды: одно значение используйте как `SESSION_PEPPER`, второе — как `SECRETS_MASTER_KEY`.

Не добавляйте `.env` в Git и не меняйте `SESSION_PEPPER` или `SECRETS_MASTER_KEY` после запуска без отдельной процедуры миграции. Иначе активные сессии или сохранённые API-ключи перестанут работать.

## 3. Первый запуск

```powershell
docker compose --env-file .env -f infra\compose.yml up -d --build
```

Проверка:

```powershell
docker compose --env-file .env -f infra\compose.yml ps
Invoke-RestMethod http://127.0.0.1:8080/api/v1/health
```

Открыть приложение локально: <http://127.0.0.1:8080>.

## 4. Обычное обновление

После получения новых изменений кода:

```powershell
docker compose --env-file .env -f infra\compose.yml up -d --build
docker compose --env-file .env -f infra\compose.yml ps
```

Команда пересобирает образ `api`, применяет миграции базы и пересоздаёт только необходимые контейнеры. Данные PostgreSQL и аудио остаются в Docker volumes.

После обновления интерфейса пользователям может потребоваться `Ctrl+F5`, чтобы браузер не использовал старые JS-файлы.

## 5. Логи и диагностика

```powershell
docker compose --env-file .env -f infra\compose.yml logs -f api
docker compose --env-file .env -f infra\compose.yml logs -f postgres
```

Последние 100 строк API:

```powershell
docker compose --env-file .env -f infra\compose.yml logs --tail 100 api
```

Перезапуск без пересборки:

```powershell
docker compose --env-file .env -f infra\compose.yml restart
```

Если изменился код, используйте `up -d --build`, а не `restart`.

## 6. Остановка

```powershell
docker compose --env-file .env -f infra\compose.yml down
```

Эта команда сохраняет данные. Не добавляйте `-v`: флаг удалит volumes с PostgreSQL и аудио.

## 7. Публичный сервер и HTTPS

Compose публикует API только на `127.0.0.1:8080`. Это намеренно: наружу приложение должен отдавать reverse proxy с HTTPS.

Пример Caddyfile:

```caddyfile
langai.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

Для публичного запуска обязательно установите `LANGAI_ENV=production`. Откройте во внешнем firewall только `80` и `443`; PostgreSQL и порт `8080` наружу не публикуйте.

В desktop-приложении адрес синхронизации должен быть публичным HTTPS-адресом, например:

```text
https://langai.example.com
```

## 8. Регистрация пользователей

Режим задаётся переменной `REGISTRATION_MODE`:

- `open` — открытая регистрация;
- другое поддерживаемое проектом значение следует задавать только вместе с настроенным процессом приглашений.

После создания нужных аккаунтов не оставляйте открытую регистрацию на публичном сервере без необходимости.

## 9. Резервное копирование

Перед крупным обновлением сохраните дамп PostgreSQL:

```powershell
New-Item -ItemType Directory -Force .\backups | Out-Null
docker compose --env-file .env -f infra\compose.yml exec -T postgres sh -c 'pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Fc -f /tmp/langai.dump'
docker compose --env-file .env -f infra\compose.yml cp postgres:/tmp/langai.dump .\backups\langai.dump
docker compose --env-file .env -f infra\compose.yml exec -T postgres rm -f /tmp/langai.dump
```

Аудио хранится в volume `audio-data`; его также нужно включать в резервную копию хоста или Docker volumes.

## 10. Быстрый чек-лист обновления

1. Создать резервную копию.
2. Получить новый код.
3. Выполнить `docker compose --env-file .env -f infra\compose.yml up -d --build`.
4. Проверить `docker compose ... ps`.
5. Проверить `/api/v1/health`.
6. Просмотреть логи `api` на наличие ошибок миграции.
7. Открыть web и выполнить `Ctrl+F5`.
8. Запустить синхронизацию desktop и проверить одинаковый набор предложений.
