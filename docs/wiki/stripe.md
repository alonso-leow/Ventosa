# Stripe
<!-- group: integrations -->

Payments for sponsors, memberships, paid downloads.

## 1. Keys

dashboard.stripe.com ▸ **Developers** ▸ **API keys**. Work in **test mode** first (`sk_test_…`). For the AI, create a **restricted key** with only the resources it needs (e.g. read payments, write payment links). Env `STRIPE_API_KEY`.

![Stripe dashboard, Developers, API keys, restricted key](img/stripe/restricted-key.png)

## 2. Tools for the AI

Stripe's MCP server (`https://mcp.stripe.com`) or the Stripe CLI (`stripe login`). Give the tools only to roles that handle money.

## 3. Webhooks

For app forms that take payments: a webhook endpoint on the server (Developers ▸ Webhooks), verify the signature with the endpoint's secret. Test locally with `stripe listen --forward-to localhost:9000/api/stripe`.

## Rules

Refunds, price changes and payouts are elevate rules: the AI asks a person.
