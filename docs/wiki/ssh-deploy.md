# SSH deploy (public)
<!-- group: deploy -->

Publish apps to a server so people outside the network (or the public, for forms) can reach them. Ventosa copies the app over SSH and starts it there; events come back with `app_pull`.

## 1. A key for Ventosa

```
ssh-keygen -t ed25519 -f ~/.ssh/ventosa_deploy -C "ventosa deploy"
ssh-copy-id -i ~/.ssh/ventosa_deploy.pub deploy@203.0.113.10
ssh -i ~/.ssh/ventosa_deploy deploy@203.0.113.10 true   # no password prompt = ok
```

Use a dedicated `deploy` user on the server, not root. Keep the key in a file or the ssh agent; Ventosa never copies it.

![The server's ~/.ssh/authorized_keys with the ventosa key on its own line](img/ssh/authorized-keys.png)

## 2. An entry in ~/.ssh/config (optional)

```
Host vps
    HostName 203.0.113.10
    User deploy
    IdentityFile ~/.ssh/ventosa_deploy
```

## 3. The server in Ventosa

settings ▸ servers ▸ add: name, host, user, port 22, key file (empty = agent / ssh config), remote dir (e.g. `/srv/apps`), public url. Press **test connection**.

## 4. A web server with TLS

On the server, Caddy serves `remote_dir` with automatic HTTPS:

```
apps.example.com {
    root * /srv/apps
    file_server
    reverse_proxy /api/* 127.0.0.1:9000
}
```

Point the domain's A record at the server first; Caddy gets the certificate on its own.

## 5. Publish

Ask the AI ("publish the sign-up app to vps") or call `app_publish`. Static apps are copied; service apps run with `VENTOSA_REMOTE=1` and write their events to `events.jsonl` on the server, which Ventosa pulls back and applies through the write gate.

## Hardening

- `PasswordAuthentication no` in `/etc/ssh/sshd_config`.
- `ufw allow OpenSSH && ufw allow 80,443/tcp && ufw enable`.
- `unattended-upgrades` (Debian/Ubuntu) or a monthly update.
