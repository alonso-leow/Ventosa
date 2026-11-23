# Local network deployment
<!-- group: deploy -->

Serve the web apps to the team on the same network (office, studio, home).

## 1. Users first

Create the users before opening anything to the network (access ▸ users). With no users the apps are open to anyone who reaches them.

## 2. Bind to the network

The engine's servers listen on `127.0.0.1` by default. Set `VENTOSA_BIND=0.0.0.0` in the environment the app starts from (for niri, `environment { VENTOSA_BIND "0.0.0.0"; }` in its config; for systemd user services, `Environment=VENTOSA_BIND=0.0.0.0`).

## 3. Find the hub's address

```
ip -4 addr show | grep inet
```

Give the hub a fixed address in the router (DHCP reservation), or use its mDNS name (`hostname.local`).

## 4. Firewall

Open only the app ports (see each workspace's `workspace.json ▸ ports`, plus the portal's 8790):

```
sudo ufw allow from 192.168.1.0/24 to any port 8765:8790 proto tcp
```

## 5. TLS (do not skip)

Basic auth sends the password in every request. On a network, put Caddy in front with a local certificate:

```
apps.lan {
    tls internal
    reverse_proxy /label/* 127.0.0.1:8766
    reverse_proxy 127.0.0.1:8790
}
```

Install Caddy's root certificate on the team's machines once (`caddy trust` prints where it is).

## 6. The portal

`ventosa-app portal` serves a login page that lists the apps each person may open. Send the team the portal address only.

## Desktop instead of the browser

People who should not use a browser run `ventosa-portal` (the launcher) on their machine with the hub's identity file synced, or a Tauri shell around the portal URL.
