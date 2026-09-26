# lifeping

A tiny self-hosted page that tells friends and family whether you're alive.

You send a "ping" (a keybind on your PC, one tap on your phone), the server
records the time, and a public page shows how long ago the last ping was,
colour-coded by freshness, with a short history. The page is in French or
English and needs no login.

| Status  | Meaning                                         |
|---------|-------------------------------------------------|
| grey    | no ping yet                                     |
| green   | last ping younger than `LIFEPING_YELLOW_AFTER`  |
| amber   | older than that, but younger than `LIFEPING_RED_AFTER` |
| red     | no sign of life for longer than `LIFEPING_RED_AFTER`   |

It's one Rust binary with the web UI built in, shipped as one Docker
container with one volume.

## Deploying

```sh
mkdir -p secrets
openssl rand -hex 32 > secrets/lifeping_token
```

`secrets/` is already in `.gitignore`. Keep it out of git.

Edit the lines marked `# CHANGE ME` in `compose.yaml` (host, Traefik
entrypoint, cert resolver, external network name), then run:

```sh
docker compose up --build -d
```

The container runs as the distroless `nonroot` user (UID/GID 65532). The
named volume in `compose.yaml` works as is. If you bind-mount a host directory
to `/data` instead, make it writable by that user:

```sh
sudo chown 65532:65532 ./data
```

Pings are stored in `/data/pings.log`, one UTC timestamp per line. They are
kept forever; the file is plain text and safe to back up or read by hand.

### Configuration

| Variable | Default | Description |
|---|---|---|
| `LIFEPING_TOKEN` | – | Bearer token for `POST /api/ping` |
| `LIFEPING_TOKEN_FILE` | – | File containing the token (for Docker secrets). Set exactly one of the two token variables. |
| `LIFEPING_YELLOW_AFTER` | `12h` | Age after which the status turns amber (`90m`, `12h`, `1d 6h`, …) |
| `LIFEPING_RED_AFTER` | `24h` | Age after which the status turns red; must be greater than the amber threshold |
| `LIFEPING_HISTORY` | `10` | How many recent pings the page shows (1–1000) |
| `LIFEPING_DATA_DIR` | `/data` | Directory holding `pings.log` |
| `LIFEPING_BIND` | `0.0.0.0:8080` | Listen address |
| `RUST_LOG` | `lifeping=info` | Log filter |

The server won't start if the configuration is invalid, and the error names
the variable at fault.

## Sending pings

### PC (Hyprland keybind)

```sh
install -D --mode=755 scripts/lifeping-ping ~/.local/bin/lifeping-ping
install -D --mode=600 secrets/lifeping_token ~/.config/lifeping/token
```

Set `LIFEPING_URL` to your server (for example in your Hyprland `env`
settings), or edit the default in the script. Then add a keybind to
`hyprland.conf`:

```
bind = SUPER, P, exec, ~/.local/bin/lifeping-ping
```

The script shows a desktop notification with `notify-send`, telling you
whether the ping worked. It needs `curl`.

### Phone (HTTP Shortcuts app)

In the open-source [HTTP Shortcuts](https://http-shortcuts.rmy.ch/) app
(available on F-Droid and in the GrapheneOS app store):

1. Create a new **Regular HTTP Shortcut**, named e.g. "I'm alive".
2. **Basic request settings**: method `POST`, URL
   `https://lifeping.example.com/api/ping`.
3. **Request headers**: add `Authorization` with value `Bearer <your token>`.
4. **Response handling**: show a toast on success and on failure.
5. Save, then long-press the shortcut → **Place on home screen**, or add an
   HTTP Shortcuts widget. One tap now sends a ping.

### Anything else

```sh
curl --request POST --header "Authorization: Bearer $TOKEN" https://lifeping.example.com/api/ping
# {"timestamp":"2026-09-27T09:30:00Z"}
```

## API

| Endpoint | Auth | Description |
|---|---|---|
| `GET /api/status` | none | Current status, latest ping, recent history, thresholds |
| `POST /api/ping` | `Bearer` | Records a ping at server time, returns `{"timestamp": …}` |
| `GET /healthz` | none | Returns `ok` |

```sh
curl https://lifeping.example.com/api/status
```

```json
{
  "now": "2026-09-27T09:30:00Z",
  "status": "green",
  "latest": "2026-09-27T07:58:02Z",
  "history": ["2026-09-27T07:58:02Z", "2026-09-26T19:03:10Z"],
  "total_pings": 2,
  "thresholds": { "yellow_after_secs": 43200, "red_after_secs": 86400 }
}
```

## Development

```sh
LIFEPING_TOKEN=dev LIFEPING_DATA_DIR=./data LIFEPING_BIND=127.0.0.1:8080 cargo run
cargo test
cargo clippy --all-targets -- --deny warnings
cargo fmt --check
```

To watch the colours change without waiting a day, start the server with
`LIFEPING_YELLOW_AFTER=1m LIFEPING_RED_AFTER=2m`.
