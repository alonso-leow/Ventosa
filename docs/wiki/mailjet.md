# Mailjet
<!-- group: integrations -->

Transactional mail (notices to the team, confirmations from app forms) and campaigns.

## 1. Keys

app.mailjet.com ▸ Account settings ▸ **API key management**: an API key and a secret key. Put them in `MJ_APIKEY_PUBLIC` and `MJ_APIKEY_PRIVATE`.

![Mailjet, API key management](img/mailjet/api-keys.png)

## 2. Sender

Account settings ▸ **Sender addresses & domains**: validate the domain (SPF and DKIM records at your DNS) so mail is not flagged.

## 3. Sending

Send API v3.1: `POST https://api.mailjet.com/v3.1/send` with basic auth (key:secret). Use it from an AI-made app's service (server side), never from a page.
