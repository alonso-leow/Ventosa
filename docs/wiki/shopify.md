# Shopify
<!-- group: integrations -->

Merch and products tied to releases.

## 1. An app with an Admin API token

Shopify admin ▸ Settings ▸ **Apps and sales channels** ▸ **Develop apps** ▸ **Create an app** ▸ Configure **Admin API scopes** (only what you need, e.g. `read_products`, `write_products`, `read_orders`) ▸ **Install** ▸ reveal the **Admin API access token** (`shpat_…`, shown once). Env `SHOPIFY_ADMIN_TOKEN`, plus the shop domain `my-shop.myshopify.com`.

If your admin no longer offers "Develop apps", create the app in the Shopify **Dev Dashboard** and install it on the store; the token and scopes work the same.

![Shopify admin, Develop apps, Admin API scopes](img/shopify/scopes.png)

## 2. Calls

GraphQL Admin API: `POST https://my-shop.myshopify.com/admin/api/<version>/graphql.json` with header `X-Shopify-Access-Token`.

## 3. In the pipeline

A step "product page for the release" (AI drafts title, description, images from the trusted sources), then a human step "publish product".
